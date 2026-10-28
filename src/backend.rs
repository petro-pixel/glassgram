//! Telegram backend: a separate thread with a tokio runtime, talks to the UI through channels.

use std::collections::{HashMap, HashSet, VecDeque};
use std::path::PathBuf;
use std::sync::{Arc, Mutex, mpsc};
use std::time::Duration;

use base64::Engine as _;
use eframe::egui::ColorImage;
use grammers_client::client::{LoginToken, PasswordToken};
use grammers_client::media::{Downloadable, Media, Photo, Uploaded};
use grammers_client::message::InputReactions;
use grammers_client::message::{InputMessage, Message};
use grammers_client::peer::{Peer, User};
use grammers_client::sender::UpdatesConfiguration;
use grammers_client::session::Session as _;
use grammers_client::session::storages::SqliteSession;
use grammers_client::session::types::{PeerId, PeerInfo, PeerRef, UpdateState, UpdatesState};
use grammers_client::session::updates::UpdatesLike;
use grammers_client::update::Update;
use grammers_client::{Client, SenderPool, SignInError, tl};
use tokio::sync::{Notify, Semaphore, mpsc as tmpsc};
use serde::{Deserialize, Serialize};
use tokio::task::JoinHandle;

use crate::cache::{self, Cache};

mod types;
pub use types::*;
mod loader;
use loader::*;
mod convert;
use convert::*;
mod updates;
use updates::*;
mod upload;
use upload::*;
pub use upload::is_image;
mod account;
mod topics;
mod comments;
mod inline;
mod folders;
mod sessions;
mod login;
mod sending;
mod scheduled;
mod dialogs;
mod history;
mod media;
mod search;
mod outbox;
mod reply;
mod editmedia;
mod newchat;
use crate::format::{self, Span};
use crate::i18n::{l, lf};

mod chatview;
mod suggest;
pub use suggest::{BotCmd, Mentionable, WebPageInfo};
mod info;
pub use info::{ChatDetails, MUTE_FOREVER, Member, MemberRole, NotifyState, SHARED_PAGE, SharedKind};
mod sponsored;
pub use sponsored::{Sponsored, SponsoredReport};
mod notify;
mod translate;
mod contacts;
mod webapp;
mod polls;
mod privacy;
mod websites;
mod storage;
pub use storage::CacheKind;
mod autodelete;
pub use websites::WebSession;
mod password;
pub use password::{PasswordChange, PasswordState};
mod chatedit;
mod birthdays;
mod botstart;
mod similar;
mod saved;
pub use saved::SavedTag;
mod stories;
mod gifts;
mod emojikw;
mod report;
pub use report::ReportStep;
pub use gifts::Gift;
pub use stories::{Story, StoryPeer};
pub use birthdays::Birthday;
pub use chatedit::ChatRights;
pub use privacy::{GlobalPrivacy, PrivacyExceptions};
pub use polls::NewPoll;
pub use webapp::{WebAppKind, WebAppOpened};
pub use notify::{NotifyException, NotifyScope};

const DIALOG_LIMIT: usize = 300;
pub const PAGE: usize = 50;
/// Maximum display sizes in points; converted to pixels with the screen density in mind,
/// so the GPU never downscales the texture (egui-wgpu has no mipmaps, otherwise you get jaggies).
const AVATAR_PT: f32 = 48.0;
const PHOTO_PT: f32 = 420.0;
const STICKER_PT: f32 = 160.0;
const FULL_PT: f32 = 1600.0;
/// Photos larger than this (px on the long side) are compressed before sending, like Telegram Desktop.
const SEND_PHOTO_PX: u32 = 2560;
const SEARCH_LIMIT: usize = 60;
/// Maximum number of messages to load while reaching the found one.
const JUMP_LIMIT: usize = 1000;

#[derive(Debug)]
pub enum Cmd {
    Connect { api_id: i32, api_hash: String },
    /// Stop the backend thread (account switch, logout).
    Shutdown,
    /// Snapshot of the chat list from the UI (with new previews and counters) for the local cache.
    CacheDialogs(Vec<ChatInfo>),
    RequestCode(String),
    SubmitCode(String),
    SubmitPassword(String),
    QrLogin,
    CancelQr,
    ReloadDialogs,
    OpenChat(PeerId),
    LoadOlder { peer: PeerId, before: i32 },
    Send { peer: PeerId, text: String, reply_to: Option<ReplyTo>, opts: SendOpts },
    Edit { peer: PeerId, id: i32, text: String },
    /// Replace a sent message's photo or file with `path` (`as_file`: an image as a document),
    /// keeping its spoiler and caption position; answered with `Ev::MediaReplaced`.
    EditMedia { peer: PeerId, id: i32, text: String, path: PathBuf, as_file: bool, spoiler: bool, above: bool },
    Delete { peer: PeerId, ids: Vec<i32> },
    LoadFullPhoto { peer: PeerId, id: i32 },
    /// Set a reaction (`None` removes ours).
    React { peer: PeerId, id: i32, emoji: Option<String> },
    Vote { peer: PeerId, id: i32, options: Vec<Vec<u8>> },
    LoadScheduled(PeerId),
    SendScheduledNow { peer: PeerId, ids: Vec<i32> },
    DeleteScheduled { peer: PeerId, ids: Vec<i32> },
    Reschedule { peer: PeerId, id: i32, at: i64 },
    InlineQuery { peer: PeerId, bot: String, query: String },
    /// Download a media file (video, GIF, audio) into the cache for playback.
    LoadMedia { peer: PeerId, id: i32 },
    /// A message's contents were seen (`readMessageContents`): a voice or round video message
    /// listened to, or a mention looked at.
    MarkListened { peer: PeerId, id: i32 },
    SendInline { peer: PeerId, query_id: i64, result: String, reply_to: Option<ReplyTo> },
    /// Create or edit a folder (a new one gets a free id, starting from 2).
    SaveFolder(Folder),
    DeleteFolder(i32),
    /// Folder tags on chat rows (`messages.toggleDialogFilterTags`, Premium).
    SetFolderTags(bool),
    /// Folders Telegram recommends (`messages.getSuggestedDialogFilters`).
    LoadSuggestedFolders,
    /// Forum topics (a group with topics enabled).
    LoadTopics(PeerId),
    /// Topic history: the first page (`before: None`) or older messages.
    OpenTopic { peer: PeerId, topic: i32, before: Option<i32> },
    CreateTopic { peer: PeerId, title: String, color: u32 },
    /// Rename (`title`) and/or close or reopen (`closed`) a topic.
    EditTopic { peer: PeerId, id: i32, title: Option<String>, closed: Option<bool> },
    /// Mark a topic as read up to `max_id`.
    ReadTopic { peer: PeerId, topic: i32, max_id: i32 },
    /// Comments on a channel post (from the linked discussion group).
    /// `before`: older comments (id of the oldest loaded one).
    OpenComments { peer: PeerId, id: i32, before: Option<i32> },
    /// Export the whole chat history to Downloads (HTML + JSON, optionally with media).
    ExportChat { peer: PeerId, title: String, media: bool },
    /// Find a public chat by @username and open it without joining.
    ResolveUsername(String),
    /// Search public groups, channels and bots by name.
    SearchPublic(String),
    /// Join a public group or channel.
    Join(PeerId),
    LoadSessions,
    /// Terminate the session on another device (`hash` from `Session`).
    TerminateSession(i64),
    TerminateOtherSessions,
    LoadPrivacy,
    /// A privacy rule: the base audience plus "always" / "never" exceptions.
    SetPrivacy { key: PrivacyKey, audience: Audience, allow: Vec<PeerId>, deny: Vec<PeerId> },
    /// Archive / read time / who can message me (`account.getGlobalPrivacySettings`).
    LoadGlobalPrivacy,
    SetGlobalPrivacy(GlobalPrivacy),
    /// Whether sensitive media is shown unfiltered (`account.getContentSettings`).
    LoadContentSettings,
    /// Websites logged in with Telegram; each answer is `Ev::Websites`.
    /// The auto-delete timer for new chats, in seconds (0 — off); answered with `Ev::DefaultTtl`.
    LoadDefaultTtl,
    SetDefaultTtl(i32),
    /// A chat's auto-delete timer; answered with `Ev::HistoryTtl`.
    SetHistoryTtl { peer: PeerId, period: i32 },
    /// What the local cache takes by type; answered with `Ev::CacheUsage`.
    LoadCacheUsage,
    ClearCache(Vec<CacheKind>),
    /// Remove the oldest cached files over this many bytes.
    TrimCache(u64),
    LoadWebsites,
    LogoutWebsite(i64),
    LogoutAllWebsites,
    SetSensitive(bool),
    /// Two-step verification state (`account.getPassword`); answered with `Ev::Password`.
    LoadPassword,
    /// Checks the current cloud password before the page offers to change it.
    CheckPassword(String),
    UpdatePassword { current: String, change: PasswordChange },
    ConfirmPasswordEmail(String),
    ResendPasswordEmail,
    CancelPasswordEmail,
    Forward { from: PeerId, ids: Vec<i32>, to: PeerId },
    /// An inline button with callback data was pressed.
    PressButton { peer: PeerId, id: i32, data: Vec<u8> },
    SendSticker { peer: PeerId, doc: i64, reply_to: Option<ReplyTo> },
    LoadStickers,
    LoadStickerThumbs(Vec<i64>),
    Download { peer: PeerId, id: i32 },
    /// Open a chat and load history up to and including message `id`.
    OpenAt { peer: PeerId, id: i32 },
    SetPinned { peer: PeerId, pinned: bool },
    SetMuted { peer: PeerId, muted: bool },
    SetArchived { peer: PeerId, archived: bool },
    MarkRead(PeerId),
    MarkUnread(PeerId),
    SaveDraft { peer: PeerId, text: String },
    Typing(PeerId),
    /// `peer: None` means a global search across all chats.
    Search { peer: Option<PeerId>, query: String, kind: Option<SharedKind>, from: Option<PeerId> },
    Logout,
    // Internal: results of the QR task.
    QrPassword(PasswordToken),
    QrSuccess(Box<tl::types::auth::Authorization>),
    /// Pinned messages of a chat (`messages.search` with the pinned filter).
    LoadPinned(PeerId),
    /// Pin or unpin a message. `one_side` — only for me (private chats).
    PinMessage { peer: PeerId, id: i32, unpin: bool, silent: bool, one_side: bool },
    /// Unpin all messages of a chat.
    UnpinAll(PeerId),
    /// Unread mentions (`reactions: false`) or reactions to our messages.
    LoadUnreadMarks { peer: PeerId, reactions: bool },
    /// Mark all mentions or reactions as read.
    ReadMarks { peer: PeerId, reactions: bool },
    /// Which group members have read our message.
    LoadReadBy { peer: PeerId, id: i32 },
    /// Save a contact: an existing user (`user`), or import by `phone`.
    AddContact { user: Option<PeerId>, phone: String, first: String, last: String },
    DeleteContact(PeerId),
    /// Block or unblock a user (for bots: stop / restart).
    Block { peer: PeerId, blocked: bool },
    LoadBlocked,
    /// Ask the server for a mini app URL; `theme` is the `theme_params` JSON.
    OpenWebApp { peer: PeerId, bot: PeerId, kind: WebAppKind, theme: String },
    /// `t.me/<bot>/<app>?startapp=…` (`app: Some`) or `t.me/<bot>?startapp=…`.
    OpenWebAppLink { username: String, app: Option<String>, start_param: String, theme: String },
    /// Keep an inline-button mini app session alive (every minute while open).
    ProlongWebApp { peer: PeerId, bot: PeerId, query_id: i64 },
    /// `Telegram.WebApp.sendData` from a keyboard-button mini app.
    WebAppData { bot: PeerId, button_text: String, data: String },
    /// `invokeCustomMethod` (cloud storage and the like), answered by `req_id`.
    WebAppMethod { bot: PeerId, req_id: String, method: String, params: String },
    /// A mini app asks whether its bot may message us: `allow` grants it (`bots.allowSendMessage`),
    /// otherwise only check (`bots.canSendMessage`). Answered with `Ev::BotWriteAccess`.
    BotWriteAccess { bot: PeerId, allow: bool },
    /// Share our phone number with a bot as a contact, after the user agreed (`requestPhone`).
    SharePhone { bot: PeerId },
    /// New title and / or description of a group or channel (`None` — unchanged).
    EditChat { peer: PeerId, title: Option<String>, about: Option<String> },
    SetChatPhoto { peer: PeerId, path: PathBuf },
    /// Default permissions of group members.
    SetChatRights { peer: PeerId, rights: ChatRights },
    /// Slow mode of a supergroup, in seconds (0 — off).
    SetSlowMode { peer: PeerId, seconds: i32 },
    /// Decode again pictures the UI dropped to save memory.
    ReloadImages(Vec<ImgKey>),
    /// People and channels with active stories, for the strip above the chat list.
    LoadStories,
    /// One peer's stories for the viewer.
    LoadPeerStories(PeerId),
    /// Mark a peer's stories seen up to `max_id` (the owner sees us among the viewers).
    ReadStories { peer: PeerId, max_id: i32 },
    /// Download a video story to play it.
    LoadStoryVideo { peer: PeerId, id: i32 },
    /// Our reaction to a story (`None` takes it back).
    ReactStory { peer: PeerId, id: i32, emoji: Option<String> },
    /// A private reply to a story: a message to its owner quoting it.
    ReplyStory { peer: PeerId, id: i32, text: String },
    /// Where the messages in Saved Messages came from.
    LoadSavedDialogs,
    /// Saved Messages from one source; `saved` is the Saved Messages chat itself.
    LoadSavedHistory { saved: PeerId, source: PeerId, before: Option<i32> },
    /// Reaction tags of Saved Messages (`messages.getSavedReactionTags`).
    LoadSavedTags,
    /// Messages of Saved Messages tagged with this emoji.
    LoadSavedTagged { saved: PeerId, emoji: String },
    /// Name a tag (Premium); an empty title removes the name.
    RenameSavedTag { emoji: String, title: String },
    /// Gifts shown in a profile.
    LoadGifts(PeerId),
    /// Report messages; start with an empty `option` and pass the chosen one in the next step.
    Report { peer: PeerId, ids: Vec<i32>, option: Vec<u8>, message: String },
    /// Emoji keywords of a language (`en`, `uk`…) for `:word` suggestions.
    LoadEmojiKeywords(String),
    /// Channels similar to this one, for its profile.
    LoadSimilarChannels(PeerId),
    /// `t.me/<bot>?start=<param>`: open the bot chat ready to start with the parameter.
    OpenBotStart { username: String, param: String },
    /// Start a bot with a deep-link parameter (`messages.startBot`).
    StartBot { peer: PeerId, param: String },
    /// Our birthday (`None` removes it).
    SetBirthday(Option<Birthday>),
    /// Contacts' birthdays today, for the reminder above the chat list.
    LoadBirthdays,
    /// Send a new poll or quiz.
    SendPoll { peer: PeerId, poll: NewPoll, reply_to: Option<ReplyTo> },
    /// Translate a message's text into `to` (a language code).
    Translate { peer: PeerId, id: i32, to: String },
    /// Find the first message sent at or after `date` (unix time), for the calendar.
    JumpToDate { peer: PeerId, date: i32 },
    /// Days with photos or videos before `before` (unix time), for the calendar.
    LoadMediaDays { peer: PeerId, before: i32 },
    /// Sponsored messages of a channel (`messages.getSponsoredMessages`).
    LoadSponsored(PeerId),
    /// The ad was shown on screen; the server counts views.
    ViewSponsored(Vec<u8>),
    /// The ad's button or card was clicked.
    ClickSponsored(Vec<u8>),
    /// Report an ad; an empty `option` asks the server for the list of reasons.
    ReportSponsored { peer: PeerId, random_id: Vec<u8>, option: Vec<u8> },
    /// Show or hide ads everywhere — Premium only (`account.toggleSponsoredMessages`).
    SetAdsEnabled(bool),
    /// Turn notifications for a whole category on or off.
    SetNotifyDefault { scope: NotifyScope, muted: bool },
    /// Chats of a category with settings of their own.
    LoadNotifyExceptions(NotifyScope),
    /// Drop a chat's own notification settings: it follows its category again.
    ResetNotify(PeerId),
    /// Preview for the link being typed (`messages.getWebPagePreview`).
    WebPreview(String),
    /// Chat members for the @mention popup.
    LoadMentions { peer: PeerId, query: String },
    /// Chat bot commands for the `/` menu.
    LoadBotCommands(PeerId),
    /// Info panel: description, links, members, notifications (`getFullUser` / `getFullChat` / `getFullChannel`).
    LoadChatInfo(PeerId),
    /// Group or channel members; empty `query` means everyone, `admins` means admins only.
    LoadMembers { peer: PeerId, query: String, admins: bool },
    /// Create an invite link (for admins that have none yet).
    ExportInvite(PeerId),
    /// One tab of a chat's shared media; `before` loads older items.
    LoadShared { peer: PeerId, kind: SharedKind, before: Option<i32> },
    /// Chat notifications: `mute_until` is unix time (0 = on, `i32::MAX` = forever).
    SetNotify { peer: PeerId, mute_until: i32, silent: bool },
    /// Leave a group or channel.
    LeaveChat(PeerId),
    /// Clear history; `revoke` also clears it for the other side (private chats only).
    ClearHistory { peer: PeerId, revoke: bool },
    /// Delete the chat from the list (groups and channels are left as well).
    DeleteChat { peer: PeerId, revoke: bool },
    /// Own profile for "My account": name, bio, username, phone, avatar.
    LoadProfile,
    /// First name, last name and bio (`account.updateProfile`).
    SaveProfile { first: String, last: String, bio: String },
    /// Whether a username is free (`account.checkUsername`); answered with `Ev::UsernameCheck`.
    CheckUsername(String),
    /// New username (`account.updateUsername`); empty removes it.
    SetUsername(String),
    /// Show or hide one of our extra usernames in the profile (`account.toggleUsername`).
    ToggleUsername { name: String, active: bool },
    /// The order of our usernames in the profile (`account.reorderUsernames`).
    ReorderUsernames(Vec<String>),
    /// New profile photo from a file (`photos.uploadProfilePhoto`).
    SetProfilePhoto(PathBuf),
    /// Send a queued message; until it succeeds it stays in the cache and survives a restart.
    SendQueued(Queued),
    /// Only store the state of a queued message (e.g. "failed") without sending.
    StoreQueued(Queued),
    /// Remove a message from the queue (deleted by the user).
    DropQueued(i64),
    /// New order of pinned chats in the main list or the archive (`messages.reorderPinnedDialogs`).
    ReorderPinned { archived: bool, order: Vec<PeerId> },
    /// New folder order (`messages.updateDialogFiltersOrder`).
    ReorderFolders(Vec<i32>),
    /// Contact list for "New chat".
    LoadContacts,
    /// People search by name or @username: own and global results.
    SearchContacts(String),
    /// New group with the chosen members (`messages.createChat`).
    CreateGroup { title: String, users: Vec<PeerId> },
    /// New channel (`channels.createChannel`).
    CreateChannel { title: String, about: String },
    /// Whether `name` is free for a channel's public link (`channels.checkUsername`).
    CheckChannelUsername { peer: PeerId, name: String },
    /// A channel's public link `t.me/<name>`; empty makes it private (`channels.updateUsername`).
    SetChannelUsername { peer: PeerId, name: String },
    /// Add people to a channel (`channels.inviteToChannel`).
    InviteToChannel { peer: PeerId, users: Vec<PeerId> },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ImgKey {
    Avatar(PeerId),
    Photo(PeerId, i32),
    /// Full-size photo for the viewer.
    Full(PeerId, i32),
    Sticker(i64),
    /// Link preview picture above the composer (by page id).
    WebPage(i64),
    /// Sponsored message picture (by photo id).
    Sponsored(i64),
    /// A story picture or a video story's cover.
    Story(PeerId, i32),
}

pub enum Ev {
    NeedPhone,
    NeedCode,
    NeedPassword(Option<String>),
    Qr(String),
    LoggedIn(String),
    Dialogs(Vec<ChatInfo>),
    History { peer: PeerId, msgs: Vec<Msg>, older: bool },
    NewMsg(Msg),
    Edited(Msg),
    /// A message whose media was replaced: its old pictures and files are stale.
    MediaReplaced(Msg),
    /// `peer: None`: deletion not tied to a channel (private chats and small groups).
    Deleted { ids: Vec<i32>, peer: Option<PeerId> },
    Image { key: ImgKey, image: ColorImage },
    Folders(Vec<Folder>),
    /// Whether chat rows show folder tags.
    FolderTags(bool),
    /// Recommended folders with their descriptions.
    SuggestedFolders(Vec<(Folder, String)>),
    Stickers(Vec<StickerSetInfo>),
    UploadProgress { id: u64, done: u64, total: u64 },
    UploadDone(u64),
    /// `unread`: someone else's reaction to our message we haven't seen yet.
    Reactions { peer: PeerId, id: i32, reactions: Vec<Reaction>, unread: bool },
    MediaReady { peer: PeerId, id: i32, path: PathBuf },
    /// Voice or round video messages were listened to (`peer: None`: private chats and small groups).
    Listened { peer: Option<PeerId>, ids: Vec<i32> },
    Scheduled { peer: PeerId, msgs: Vec<Msg> },
    InlineResults { peer: PeerId, bot: String, query: String, query_id: i64, results: Vec<InlineResult> },
    /// Bot's answer to a button press: the text and whether to show it as an alert.
    BotAnswer { text: String, alert: bool },
    /// Someone is typing (`active: false` means they stopped).
    Typing { peer: PeerId, who: String, active: bool },
    Presence { peer: PeerId, presence: Presence },
    Draft { peer: PeerId, text: String },
    /// Read on another device: how many unread remain.
    ReadInbox { peer: PeerId, unread: i32 },
    /// The peer read our messages up to `max_id`.
    ReadOutbox { peer: PeerId, max_id: i32 },
    /// Search answer; `kind` and `from` limit in-chat search to one type of message and one member.
    SearchResults { peer: Option<PeerId>, query: String, kind: Option<SharedKind>, from: Option<PeerId>, hits: Vec<SearchHit> },
    Topics { peer: PeerId, topics: Vec<Topic> },
    /// A page of topic history: merged into the chat history (`older` means earlier messages).
    TopicHistory { peer: PeerId, topic: i32, msgs: Vec<Msg>, older: bool },
    /// Comments on post `post`: the discussion group, its root message and the comments themselves.
    Comments { peer: PeerId, post: i32, discussion: PeerId, root: i32, msgs: Vec<Msg>, older: bool },
    /// Export: how many messages have been exported so far.
    ExportProgress { peer: PeerId, done: usize },
    Exported { peer: PeerId, dir: PathBuf, count: usize },
    /// A public chat found by username (may not be in the dialog list).
    ChatFound(ChatInfo),
    PublicResults { query: String, chats: Vec<ChatInfo> },
    Joined(PeerId),
    Sessions(Vec<Session>),
    Privacy(PrivacyKey, PrivacyState),
    PrivacyExceptions(PrivacyKey, PrivacyExceptions),
    GlobalPrivacy(GlobalPrivacy),
    /// Sensitive content: shown unfiltered, and whether the account may change that.
    ContentSettings { enabled: bool, can_change: bool },
    Websites(Vec<WebSession>),
    /// (type, bytes, files) of the local cache.
    CacheUsage(Vec<(CacheKind, u64, usize)>),
    DefaultTtl(i32),
    HistoryTtl { peer: PeerId, period: i32 },
    Password(PasswordState),
    /// The current password was right; the recovery email it is bound to.
    PasswordChecked { email: String },
    /// A two-step verification request was refused (wrong password, bad email or code).
    PasswordFailed(String),
    Notice(String),
    Error(String),
    LoggedOut,
    /// All pinned messages of a chat, oldest first.
    Pinned { peer: PeerId, msgs: Vec<Msg> },
    /// Messages were pinned or unpinned (our action or a server update).
    PinnedChanged { peer: PeerId, ids: Vec<i32>, pinned: bool },
    /// Ids of unread mentions or reactions, oldest first.
    UnreadMarks { peer: PeerId, reactions: bool, ids: Vec<i32> },
    /// Who has read the message: (name, when).
    ReadBy { peer: PeerId, id: i32, readers: Vec<(String, String)> },
    /// New view count of a channel post.
    Views { peer: PeerId, id: i32, views: i32 },
    WebAppOpened(WebAppOpened),
    /// The bot's menu button opens a mini app: its text and URL.
    BotMenuApp { peer: PeerId, text: String, url: String },
    /// Answer to `WebAppMethod`: the result JSON or an error.
    WebAppMethodResult { bot: PeerId, req_id: String, result: Result<String, String> },
    /// Whether the bot may message us now; `asked`: the answer to an `allow`.
    BotWriteAccess { bot: PeerId, allowed: bool, asked: bool },
    /// Our contact reached the bot (`false`: it failed).
    PhoneShared { bot: PeerId, sent: bool },
    /// A file was saved to Downloads; the UI opens it (asking first for programs).
    Downloaded(PathBuf),
    /// Contacts whose birthday is today.
    Birthdays(Vec<(ChatInfo, Birthday)>),
    /// The story strip, in the server's order (unseen first).
    Stories(Vec<StoryPeer>),
    /// A peer's stories in posting order and the last one we saw.
    PeerStories { peer: PeerId, stories: Vec<Story>, max_read: i32 },
    /// A video story is on disk.
    StoryVideo { peer: PeerId, id: i32, path: PathBuf },
    /// Sources of Saved Messages, newest first.
    SavedDialogs(Vec<ChatInfo>),
    /// A page of Saved Messages from one source.
    SavedHistory { source: PeerId, msgs: Vec<Msg>, older: bool },
    SavedTags(Vec<SavedTag>),
    /// The tags changed on another device or by a reaction here.
    SavedTagsChanged,
    SavedTagged(Vec<Msg>),
    /// The next step of a report.
    ReportStep { peer: PeerId, ids: Vec<i32>, step: ReportStep },
    /// Keywords of one language, each with its emoji.
    EmojiKeywords { words: Vec<(String, Vec<String>)> },
    /// A profile's gifts and how many there are in all.
    Gifts { peer: PeerId, gifts: Vec<Gift>, total: i32 },
    /// Recommended channels for a channel's profile; `total` counts those shown only with Premium.
    SimilarChannels { peer: PeerId, chats: Vec<ChatInfo>, total: i32 },
    /// The bot chat was opened from a start link: the Start bar carries `param`.
    BotStart { peer: PeerId, param: String },
    /// A group or channel got a new title.
    ChatTitle { peer: PeerId, title: String },
    /// A contact was added or renamed.
    ContactSaved(ChatInfo),
    ContactRemoved(PeerId),
    Blocked { peer: PeerId, blocked: bool },
    /// Blocked users and chats, newest first.
    BlockedList(Vec<ChatInfo>),
    /// Translated text (formatting as spans), or the server's error.
    Translated { peer: PeerId, id: i32, to: String, result: Result<(String, Vec<Span>), String> },
    /// The message to jump to for a calendar date (`None`: the chat is empty).
    DateFound { peer: PeerId, id: Option<i32> },
    /// Calendar periods with photos and videos: (start of the day, how many).
    MediaDays { peer: PeerId, days: Vec<(i32, i32)> },
    /// Ads for the end of a channel's history (empty: none).
    /// `between`: posts between two ads, when the channel shows more than one.
    Sponsored { peer: PeerId, ads: Vec<Sponsored>, between: Option<i32> },
    SponsoredReport { peer: PeerId, random_id: Vec<u8>, result: SponsoredReport },
    /// Default notification settings of a category (at start and when changed anywhere).
    NotifyDefault { scope: NotifyScope, state: NotifyState },
    NotifyExceptions { scope: NotifyScope, list: Vec<NotifyException> },
    /// `page: None`: the text has no link with a preview.
    WebPreview { text: String, page: Option<WebPageInfo> },
    Mentions { peer: PeerId, query: String, members: Vec<Mentionable> },
    BotCommands { peer: PeerId, commands: Vec<BotCmd> },
    ChatDetails { peer: PeerId, details: ChatDetails },
    /// The profile photo of `peer` is a video, now on disk.
    AvatarVideo { peer: PeerId, path: PathBuf },
    Members { peer: PeerId, query: String, admins: bool, list: Vec<Member> },
    Invite { peer: PeerId, link: String },
    Shared { peer: PeerId, kind: SharedKind, msgs: Vec<Msg>, older: bool },
    Notify { peer: PeerId, state: NotifyState },
    /// The chat is gone from the list: we left or deleted it.
    ChatRemoved(PeerId),
    HistoryCleared(PeerId),
    Profile(Profile),
    /// Answer to `Cmd::CheckChannelUsername`.
    ChannelUsernameCheck { peer: PeerId, name: String, state: UsernameState },
    /// Answer to `Cmd::CheckUsername` for `name`.
    UsernameCheck { name: String, state: UsernameState },
    /// Queue from the previous run (from the cache).
    Outbox(Vec<Queued>),
    /// Queued message sent: the real one has already arrived as `NewMsg`.
    QueueSent { key: i64, peer: PeerId },
    /// Not sent; `retry` means a network error worth another attempt.
    QueueFailed { key: i64, error: String, retry: bool },
    Contacts(Vec<ChatInfo>),
    ContactResults { query: String, chats: Vec<ChatInfo> },
    /// A freshly created group or channel: add it to the list and open it.
    ChatCreated(ChatInfo),
}

/// The General forum topic.
pub const GENERAL_TOPIC: i32 = 1;

/// Topic icon colors that Telegram accepts.
pub const TOPIC_COLORS: [u32; 6] = [0x6FB9F0, 0xFFD67E, 0xCB86DB, 0x8EEE98, 0xFF93B2, 0xFB6F5F];

pub struct Handle {
    pub tx: tmpsc::UnboundedSender<Cmd>,
    pub rx: mpsc::Receiver<Ev>,
    /// Closed when the backend thread has finished (cache written).
    pub done: mpsc::Receiver<()>,
}

pub use crate::platform::data_dir;

/// Returns (api_id, api_hash): environment variables, then the user's own saved keys,
/// then the keys built into a release (see `build.rs`).
pub fn saved_credentials() -> Option<(i32, String)> {
    if let (Ok(id), Ok(hash)) = (std::env::var("TG_API_ID"), std::env::var("TG_API_HASH")) {
        return Some((id.trim().parse().ok()?, hash.trim().to_owned()));
    }
    let saved = || {
        let text = std::fs::read_to_string(data_dir().join("credentials")).ok()?;
        let mut lines = text.lines();
        let id = lines.next()?.trim().parse().ok()?;
        let hash = lines.next()?.trim().to_owned();
        Some((id, hash))
    };
    saved().or_else(builtin_credentials)
}

mod builtin {
    include!(concat!(env!("OUT_DIR"), "/api_keys.rs"));
}

/// Keys from the release build, unmasked only here and only in memory.
fn builtin_credentials() -> Option<(i32, String)> {
    let (id, hash, mask) = builtin::API?;
    let m = u32::from_le_bytes(mask.get(..4)?.try_into().ok()?);
    let hash: String = hash.iter().zip(&mask[4..]).map(|(b, k)| char::from(b ^ k)).collect();
    Some(((id ^ m) as i32, hash))
}

/// The keys are the ones built into the release: never shown in the form or written to disk.
pub fn is_builtin(api_id: i32, api_hash: &str) -> bool {
    builtin_credentials().is_some_and(|(id, hash)| id == api_id && hash == api_hash)
}

fn save_credentials(api_id: i32, api_hash: &str) {
    let dir = data_dir();
    if is_builtin(api_id, api_hash) {
        // Older versions saved them in plain text.
        let _ = std::fs::remove_file(dir.join("credentials"));
        return;
    }
    let _ = std::fs::create_dir_all(&dir);
    let path = dir.join("credentials");
    let _ = std::fs::write(&path, format!("{api_id}\n{api_hash}\n"));
    make_private(&dir);
    make_private(&path);
}

/// Owner only: the session key gives full access to the account.
/// On Unix, mode 0600 (directory 0700); on Windows the user profile is already private.
pub fn make_private(path: &std::path::Path) {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mode = if path.is_dir() { 0o700 } else { 0o600 };
        let _ = std::fs::set_permissions(path, std::fs::Permissions::from_mode(mode));
    }
    #[cfg(not(unix))]
    let _ = path;
}

/// Starts the backend for the session in file `session`.
pub fn spawn(ctx: eframe::egui::Context, session: PathBuf) -> Handle {
    let (cmd_tx, cmd_rx) = tmpsc::unbounded_channel();
    let (ev_tx, ev_rx) = mpsc::channel();
    let back = cmd_tx.clone();
    let (done_tx, done) = mpsc::channel::<()>();
    std::thread::Builder::new()
        .name("telegram".into())
        .spawn(move || {
            let rt = tokio::runtime::Builder::new_multi_thread()
                .enable_all()
                .build()
                .expect("tokio runtime");
            rt.block_on(Worker::new(ctx, ev_tx, back, session).run(cmd_rx));
            // Background tasks (updates, downloads) stop together with the runtime.
            rt.shutdown_background();
            drop(done_tx);
        })
        .expect("spawn backend thread");
    Handle { tx: cmd_tx, rx: ev_rx, done }
}

#[derive(Clone)]
struct Emitter {
    ctx: eframe::egui::Context,
    tx: mpsc::Sender<Ev>,
    /// Everything sent from the network to the UI also goes into the local cache.
    cache: Option<Cache>,
}

impl Emitter {
    fn send(&self, ev: Ev) {
        if let Some(c) = &self.cache {
            c.observe(&ev);
        }
        self.replay(ev);
    }

    /// An event from the cache itself: we don't write it back to the cache.
    fn replay(&self, ev: Ev) {
        let _ = self.tx.send(ev);
        self.ctx.request_repaint();
    }
}

type Refs = Arc<Mutex<HashMap<PeerId, PeerRef>>>;
/// Chat and user names: for "… is typing" and "Forwarded from …".
type Names = Arc<Mutex<HashMap<PeerId, String>>>;

struct Worker {
    /// Session file of the current account.
    session_path: PathBuf,
    out: Emitter,
    back: tmpsc::UnboundedSender<Cmd>,
    client: Option<Client>,
    session: Option<Arc<SqliteSession>>,
    loader: Option<Loader>,
    api_id: i32,
    api_hash: String,
    login_token: Option<LoginToken>,
    password_token: Option<PasswordToken>,
    authorized: Arc<Notify>,
    login_update: Arc<Notify>,
    qr_task: Option<JoinHandle<()>>,
    refs: Refs,
    names: Names,
    docs: HashMap<i64, tl::types::Document>,
    /// Bots for inline mode, by username.
    bots: HashMap<String, PeerRef>,
    /// Category notification defaults, to tell which chats are muted.
    notifs: notify::Notifs,
}

impl Worker {
    fn new(ctx: eframe::egui::Context, tx: mpsc::Sender<Ev>, back: tmpsc::UnboundedSender<Cmd>, session_path: PathBuf) -> Self {
        Self {
            session_path,
            out: Emitter { ctx, tx, cache: None },
            back,
            client: None,
            session: None,
            loader: None,
            api_id: 0,
            api_hash: String::new(),
            login_token: None,
            password_token: None,
            authorized: Arc::new(Notify::new()),
            login_update: Arc::new(Notify::new()),
            qr_task: None,
            refs: Arc::default(),
            names: Arc::default(),
            docs: HashMap::new(),
            bots: HashMap::new(),
            notifs: Arc::default(),
        }
    }

    async fn run(mut self, mut rx: tmpsc::UnboundedReceiver<Cmd>) {
        // The cache starts here, not in `new`: it needs the tokio runtime.
        self.out.cache = Some(Cache::spawn(cache::path(&self.session_path)));
        let mut queue = VecDeque::new();
        loop {
            let cmd = match queue.pop_front() {
                Some(c) => c,
                None => match rx.recv().await {
                    Some(c) => c,
                    None => break,
                },
            };
            if matches!(cmd, Cmd::Shutdown) {
                break;
            }
            let result = if matches!(cmd, Cmd::Connect { .. }) {
                // Without network, connecting can take a long time: meanwhile chats open from the cache,
                // and the remaining commands wait their turn.
                let out = self.out.clone();
                let connect = self.handle(cmd);
                tokio::pin!(connect);
                loop {
                    tokio::select! {
                        r = &mut connect => break r,
                        Some(c) = rx.recv() => {
                            // Logout does not wait for the network: the connection is simply cancelled.
                            if matches!(c, Cmd::Shutdown) {
                                queue.push_front(c);
                                break Ok(());
                            }
                            if let (Cmd::OpenChat(peer), Some(cache)) = (&c, &out.cache) {
                                let msgs = cache.history(*peer, None, PAGE).await;
                                if !msgs.is_empty() {
                                    out.replay(Ev::History { peer: *peer, msgs, older: false });
                                }
                            }
                            queue.push_back(c);
                        }
                    }
                }
            } else {
                self.handle(cmd).await
            };
            if let Err(e) = result {
                self.out.send(Ev::Error(e));
            }
        }
        self.stop_qr();
        // Otherwise the runtime would stop in the middle of a write.
        if let Some(c) = &self.out.cache {
            c.flush().await;
        }
    }

    fn client(&self) -> Result<Client, String> {
        self.client.clone().ok_or_else(|| l!("Немає з'єднання", "No connection").to_owned())
    }

    /// A public chat or user by username, remembered for later requests.
    async fn resolve_peer(&self, name: &str) -> Result<Peer, String> {
        let client = self.client()?;
        let name = name.trim_start_matches('@');
        let peer = client
            .resolve_username(name)
            .await
            .map_err(|e| lf!("@{name}: {e}", "@{name}: {e}"))?
            .ok_or_else(|| lf!("@{name} не знайдено", "@{name} not found"))?;
        let r = peer.to_ref().await.ok().flatten().ok_or_else(|| lf!("@{name}: немає доступу", "@{name}: no access"))?;
        self.refs.lock().unwrap().insert(peer.id(), r);
        self.names.lock().unwrap().insert(peer.id(), peer_title(&peer));
        if let Some(loader) = &self.loader {
            loader.avatar(&peer);
        }
        Ok(peer)
    }

    async fn resolve(&self, name: &str) -> Result<PeerId, String> {
        Ok(self.resolve_peer(name).await?.id())
    }

    fn peer_ref(&self, peer: PeerId) -> Result<PeerRef, String> {
        self.refs
            .lock()
            .unwrap()
            .get(&peer)
            .copied()
            .ok_or_else(|| l!("Невідомий чат", "Unknown chat").to_owned())
    }

    fn stop_qr(&mut self) {
        if let Some(t) = self.qr_task.take() {
            t.abort();
        }
    }

    async fn handle(&mut self, cmd: Cmd) -> Result<(), String> {
        match cmd {
            cmd @ Cmd::ExportChat { .. } => self.export_cmd(cmd).await,
            cmd @ (Cmd::StoreQueued(..) | Cmd::DropQueued(..)) => self.queue_cmd(cmd).await,
            cmd @ (Cmd::LoadPrivacy | Cmd::SetPrivacy { .. }) => self.privacy_rules_cmd(cmd).await,
            cmd @ (Cmd::SearchPublic(..) | Cmd::ResolveUsername(..)) => self.search_cmd(cmd).await,
            cmd @ (Cmd::LoadFullPhoto { .. } | Cmd::Download { .. } | Cmd::LoadMedia { .. } | Cmd::MarkListened { .. } | Cmd::LoadStickerThumbs(..)) => self.media_cmd(cmd).await,
            cmd @ (Cmd::OpenChat(..) | Cmd::OpenAt { .. } | Cmd::LoadOlder { .. } | Cmd::Typing(..)) => self.history_cmd(cmd).await,
            cmd @ (Cmd::SetMuted { .. } | Cmd::SetArchived { .. } | Cmd::SetPinned { .. } | Cmd::ReorderPinned { .. } | Cmd::MarkRead(..) | Cmd::MarkUnread(..) | Cmd::SaveDraft { .. } | Cmd::Join(..)) => self.dialogs_cmd(cmd).await,
            cmd @ (Cmd::Reschedule { .. } | Cmd::SendScheduledNow { .. } | Cmd::DeleteScheduled { .. }) => self.scheduled_cmd(cmd).await,
            cmd @ (Cmd::LoadDefaultTtl | Cmd::SetDefaultTtl(_) | Cmd::SetHistoryTtl { .. }) => self.autodelete_cmd(cmd).await,
            cmd @ (Cmd::LoadCacheUsage | Cmd::ClearCache(_) | Cmd::TrimCache(_)) => self.storage_cmd(cmd).await,
            cmd @ (Cmd::LoadWebsites | Cmd::LogoutWebsite(_) | Cmd::LogoutAllWebsites) => self.websites_cmd(cmd).await,
            cmd @ Cmd::EditMedia { .. } => self.edit_media_cmd(cmd).await,
            cmd @ (Cmd::Send { .. } | Cmd::SendSticker { .. } | Cmd::Forward { .. } | Cmd::Edit { .. } | Cmd::Delete { .. } | Cmd::React { .. } | Cmd::Vote { .. }) => self.send_cmd(cmd).await,
            cmd @ (Cmd::RequestCode(..) | Cmd::SubmitCode(..) | Cmd::SubmitPassword(..) | Cmd::QrLogin | Cmd::QrPassword(..) | Cmd::QrSuccess(..) | Cmd::Logout) => self.login_cmd(cmd).await,
            cmd @ (Cmd::LoadSessions | Cmd::TerminateSession(..) | Cmd::TerminateOtherSessions) => self.sessions_cmd(cmd).await,
            cmd @ (Cmd::SaveFolder(..) | Cmd::DeleteFolder(..) | Cmd::ReorderFolders(..) | Cmd::SetFolderTags(_) | Cmd::LoadSuggestedFolders) => self.folders_cmd(cmd).await,
            cmd @ (Cmd::InlineQuery { .. } | Cmd::SendInline { .. } | Cmd::PressButton { .. }) => self.inline_cmd(cmd).await,
            cmd @ Cmd::OpenComments { .. } => self.comments_cmd(cmd).await,
            cmd @ (Cmd::LoadTopics(..) | Cmd::OpenTopic { .. } | Cmd::CreateTopic { .. } | Cmd::EditTopic { .. } | Cmd::ReadTopic { .. }) => self.topics_cmd(cmd).await,
            Cmd::Connect { api_id, api_hash } => self.connect(api_id, api_hash).await,
            // Handled in `run` before reaching this point.
            Cmd::Shutdown => Ok(()),
            Cmd::CancelQr => {
                self.stop_qr();
                Ok(())
            }
            Cmd::ReloadDialogs => self.load_dialogs().await,
            Cmd::CacheDialogs(chats) => {
                self.cache_dialogs(chats);
                Ok(())
            }
            Cmd::LoadStickers => self.load_stickers().await,
            Cmd::LoadScheduled(peer) => self.load_scheduled(peer).await,
            Cmd::Search { peer, query, kind, from } => self.search(peer, query, kind, from).await,
            cmd @ (Cmd::LoadPinned(_) | Cmd::PinMessage { .. } | Cmd::UnpinAll(_) | Cmd::LoadUnreadMarks { .. } | Cmd::ReadMarks { .. } | Cmd::LoadReadBy { .. } | Cmd::JumpToDate { .. } | Cmd::LoadMediaDays { .. }) => {
                self.chat_view_cmd(cmd).await
            }
            Cmd::Translate { peer, id, to } => self.translate(peer, id, to),
            Cmd::SendPoll { peer, poll, reply_to } => self.send_poll(peer, poll, reply_to).await,
            Cmd::ReloadImages(keys) => {
                if let Some(l) = &self.loader {
                    l.reload(keys);
                }
                Ok(())
            }
            cmd @ (Cmd::LoadStories | Cmd::LoadPeerStories(_) | Cmd::ReadStories { .. } | Cmd::LoadStoryVideo { .. } | Cmd::ReactStory { .. } | Cmd::ReplyStory { .. }) => self.stories_cmd(cmd).await,
            cmd @ (Cmd::LoadSavedDialogs | Cmd::LoadSavedHistory { .. } | Cmd::LoadSavedTags | Cmd::LoadSavedTagged { .. } | Cmd::RenameSavedTag { .. }) => self.saved_cmd(cmd).await,
            cmd @ Cmd::LoadGifts(_) => self.gifts_cmd(cmd).await,
            cmd @ Cmd::Report { .. } => self.report_cmd(cmd).await,
            cmd @ Cmd::LoadEmojiKeywords(_) => self.emoji_keywords_cmd(cmd).await,
            Cmd::LoadSimilarChannels(peer) => self.load_similar(peer).await,
            cmd @ (Cmd::OpenBotStart { .. } | Cmd::StartBot { .. }) => self.bot_start_cmd(cmd).await,
            cmd @ (Cmd::SetBirthday(_) | Cmd::LoadBirthdays) => self.birthdays_cmd(cmd).await,
            cmd @ (Cmd::EditChat { .. } | Cmd::SetChatPhoto { .. } | Cmd::SetChatRights { .. } | Cmd::SetSlowMode { .. }) => self.chat_edit_cmd(cmd).await,
            cmd @ (Cmd::LoadGlobalPrivacy | Cmd::SetGlobalPrivacy(_) | Cmd::LoadContentSettings | Cmd::SetSensitive(_)) => self.global_privacy_cmd(cmd).await,
            cmd @ (Cmd::LoadPassword | Cmd::CheckPassword(_) | Cmd::UpdatePassword { .. } | Cmd::ConfirmPasswordEmail(_) | Cmd::ResendPasswordEmail | Cmd::CancelPasswordEmail) => self.password_cmd(cmd).await,
            cmd @ (Cmd::OpenWebApp { .. } | Cmd::OpenWebAppLink { .. } | Cmd::ProlongWebApp { .. } | Cmd::WebAppData { .. } | Cmd::WebAppMethod { .. } | Cmd::BotWriteAccess { .. } | Cmd::SharePhone { .. }) => self.webapp_cmd(cmd).await,
            cmd @ (Cmd::AddContact { .. } | Cmd::DeleteContact(_) | Cmd::Block { .. } | Cmd::LoadBlocked) => self.contacts_cmd(cmd).await,
            cmd @ (Cmd::SetNotifyDefault { .. } | Cmd::LoadNotifyExceptions(_) | Cmd::ResetNotify(_)) => self.notify_cmd(cmd).await,
            cmd @ (Cmd::LoadSponsored(_) | Cmd::ViewSponsored(_) | Cmd::ClickSponsored(_) | Cmd::ReportSponsored { .. } | Cmd::SetAdsEnabled(_)) => self.sponsored_cmd(cmd).await,
            Cmd::WebPreview(text) => self.web_preview(text),
            Cmd::LoadMentions { peer, query } => self.load_members(peer, query),
            Cmd::LoadBotCommands(peer) => self.load_bot_commands(peer),
            Cmd::LoadChatInfo(_)
            | Cmd::LoadMembers { .. }
            | Cmd::ExportInvite(_)
            | Cmd::LoadShared { .. }
            | Cmd::SetNotify { .. }
            | Cmd::LeaveChat(_)
            | Cmd::ClearHistory { .. }
            | Cmd::DeleteChat { .. } => self.chat_info_cmd(cmd).await,
            Cmd::LoadProfile => self.load_profile().await,
            Cmd::SaveProfile { first, last, bio } => self.save_profile(first, last, bio).await,
            Cmd::CheckUsername(name) => self.check_username(name),
            Cmd::SetUsername(name) => self.set_username(name).await,
            Cmd::ToggleUsername { name, active } => self.toggle_username(name, active).await,
            Cmd::ReorderUsernames(order) => self.reorder_usernames(order).await,
            Cmd::SetProfilePhoto(path) => self.set_profile_photo(path),
            Cmd::SendQueued(q) => self.send_queued(q).await,
            Cmd::LoadContacts => self.load_contacts().await,
            Cmd::SearchContacts(query) => self.search_contacts(query).await,
            cmd @ (Cmd::CreateGroup { .. } | Cmd::CreateChannel { .. } | Cmd::CheckChannelUsername { .. } | Cmd::SetChannelUsername { .. } | Cmd::InviteToChannel { .. }) => self.new_chat_cmd(cmd).await,
        }
    }

    async fn connect(&mut self, api_id: i32, api_hash: String) -> Result<(), String> {
        if let Some(dir) = self.session_path.parent() {
            std::fs::create_dir_all(dir).map_err(|e| e.to_string())?;
        }
        let session = Arc::new(
            SqliteSession::open(&self.session_path)
                .await
                .map_err(|e| lf!("Сесія: {e}", "Session: {e}"))?,
        );
        make_private(&self.session_path);
        if let Some(dir) = self.session_path.parent() {
            make_private(dir);
        }
        // Chats from the previous run, right away, while connecting (or with no connection at all).
        if let Some(c) = &self.out.cache {
            let start = c.start().await;
            if !start.chats.is_empty() {
                self.refs.lock().unwrap().extend(start.refs.iter().map(|r| (r.id, *r)));
                self.out.replay(Ev::LoggedIn(start.me));
                self.out.replay(Ev::Dialogs(start.chats));
            }
            let queued = c.outbox().await;
            prune_outbox(&self.session_path, &queued);
            if !queued.is_empty() {
                self.out.replay(Ev::Outbox(queued));
            }
        }
        let proxy = crate::settings::Settings::load().proxy;
        let params = grammers_client::sender::ConnectionParams {
            proxy_url: (!proxy.is_empty() && crate::settings::proxy_error(&proxy).is_none()).then_some(proxy),
            // Otherwise Active Sessions shows the grammers version as ours.
            app_version: env!("CARGO_PKG_VERSION").to_owned(),
            ..Default::default()
        };
        let SenderPool { runner, updates, handle } = SenderPool::with_configuration(Arc::clone(&session), api_id, params);
        let client = Client::new(handle);
        tokio::spawn(runner.run());

        self.authorized = Arc::new(Notify::new());
        self.login_update = Arc::new(Notify::new());
        let loader = Loader {
            client: client.clone(),
            out: self.out.clone(),
            limit: Arc::new(Semaphore::new(6)),
            requested: Arc::default(),
            files: Arc::default(),
        };
        // Defaults are per account: fetch them again for this connection.
        self.notifs = Arc::default();
        tokio::spawn(update_loop(
            client.clone(),
            updates,
            self.authorized.clone(),
            self.login_update.clone(),
            self.out.clone(),
            self.refs.clone(),
            self.names.clone(),
            self.notifs.clone(),
            loader.clone(),
        ));

        let authorized = client
            .is_authorized()
            .await
            .map_err(|e| lf!("Не вдалося підключитися: {e}", "Could not connect: {e}"))?;
        save_credentials(api_id, &api_hash);
        self.api_id = api_id;
        self.api_hash = api_hash;
        self.client = Some(client);
        self.session = Some(session);
        self.loader = Some(loader);
        // The cache limit from the settings, once per connection, off the async threads.
        let limit = crate::settings::Settings::load().cache_limit_gb;
        if limit > 0 {
            tokio::task::spawn_blocking(move || storage::trim(u64::from(limit) << 30));
        }
        if authorized {
            self.after_login().await
        } else {
            // The session was revoked: the cache belonged to it.
            if let Some(c) = &self.out.cache {
                c.clear();
            }
            self.out.send(Ev::NeedPhone);
            Ok(())
        }
    }

    /// The same thing grammers does after a regular login: caches itself and the update state.
    async fn complete_login(&self, auth: tl::types::auth::Authorization) -> Result<(), String> {
        let client = self.client()?;
        let session = self.session.clone().ok_or(l!("Немає сесії", "No session"))?;
        let state = client.invoke(&tl::functions::updates::GetState {}).await.ok();
        let user = User::from_raw(&client, auth.user);
        if let Ok(Some(r)) = user.to_ref().await {
            session
                .cache_peer(PeerInfo::User {
                    id: user.id().bare_id_unchecked(),
                    auth: Some(r.auth),
                    bot: Some(user.is_bot()),
                    is_self: Some(true),
                })
                .await
                .map_err(|e| e.to_string())?;
        }
        if let Some(tl::enums::updates::State::State(s)) = state {
            session
                .set_update_state(UpdateState::All(UpdatesState {
                    pts: s.pts,
                    qts: s.qts,
                    date: s.date,
                    seq: s.seq,
                    channels: Vec::new(),
                }))
                .await
                .map_err(|e| e.to_string())?;
        }
        Ok(())
    }

    async fn after_login(&mut self) -> Result<(), String> {
        self.stop_qr();
        let client = self.client()?;
        self.authorized.notify_one();
        let name = client.get_me().await.map(|u| u.full_name()).unwrap_or_default();
        if let Some(c) = &self.out.cache {
            c.me(name.clone());
        }
        self.out.send(Ev::LoggedIn(name));
        self.load_dialogs().await
    }

    /// Messages sent by a request whose response does not go into the update stream.
    async fn emit_latest(&self, peer: PeerId, n: usize) {
        let (Ok(client), Ok(r)) = (self.client(), self.peer_ref(peer)) else { return };
        let mut iter = client.iter_messages(r).limit(n.max(1));
        let mut msgs = Vec::new();
        while let Ok(Some(m)) = iter.next().await {
            msgs.push(self.convert_loaded(peer, &m));
        }
        for m in msgs.into_iter().rev() {
            self.out.send(Ev::NewMsg(m));
        }
    }

    async fn load_scheduled(&self, peer: PeerId) -> Result<(), String> {
        let client = self.client()?;
        let r = self.peer_ref(peer)?;
        use tl::enums::messages::Messages as M;
        let res = client
            .invoke(&tl::functions::messages::GetScheduledHistory { peer: r.into(), hash: 0 })
            .await
            .map_err(|e| lf!("Заплановані: {e}", "Scheduled: {e}"))?;
        let raw = match res {
            M::Messages(m) => m.messages,
            M::Slice(m) => m.messages,
            M::ChannelMessages(m) => m.messages,
            M::NotModified(_) => Vec::new(),
        };
        let mut msgs: Vec<Msg> = raw
            .iter()
            .filter_map(|m| match m {
                tl::enums::Message::Message(m) => Some(convert_raw(peer, m)),
                _ => None,
            })
            .collect();
        msgs.sort_by_key(|m| m.ts);
        self.out.send(Ev::Scheduled { peer, msgs });
        Ok(())
    }

    async fn resolve_bot(&mut self, client: &Client, name: &str) -> Result<PeerRef, String> {
        let key = name.to_lowercase();
        if let Some(r) = self.bots.get(&key) {
            return Ok(*r);
        }
        let peer = client
            .resolve_username(name)
            .await
            .map_err(|e| format!("@{name}: {e}"))?
            .ok_or_else(|| lf!("@{name} не знайдено", "@{name} not found"))?;
        let r = peer.to_ref().await.ok().flatten().ok_or_else(|| lf!("@{name}: немає доступу", "@{name}: no access"))?;
        self.bots.insert(key, r);
        Ok(r)
    }

    fn sent(&self) -> Sent {
        Sent { out: self.out.clone(), loader: self.loader.clone(), names: self.names.clone() }
    }

    async fn load_folders(&self) -> Result<(), String> {
        let client = self.client()?;
        let tl::enums::messages::DialogFilters::Filters(res) = client
            .invoke(&tl::functions::messages::GetDialogFilters {})
            .await
            .map_err(|e| lf!("Папки: {e}", "Folders: {e}"))?;
        self.out.send(Ev::FolderTags(res.tags_enabled));
        let folders = res.filters.iter().filter_map(folders::folder_from).collect();
        self.out.send(Ev::Folders(folders));
        Ok(())
    }

    async fn load_stickers(&mut self) -> Result<(), String> {
        let client = self.client()?;
        let mut sets = Vec::new();
        if let Ok(tl::enums::messages::RecentStickers::Stickers(r)) =
            client.invoke(&tl::functions::messages::GetRecentStickers { attached: false, hash: 0 }).await
        {
            let stickers = self.remember_docs(r.stickers);
            if !stickers.is_empty() {
                sets.push(StickerSetInfo { id: 0, title: l!("Нещодавні", "Recent").into(), stickers });
            }
        }
        let all = client
            .invoke(&tl::functions::messages::GetAllStickers { hash: 0 })
            .await
            .map_err(|e| lf!("Стікери: {e}", "Stickers: {e}"))?;
        let tl::enums::messages::AllStickers::Stickers(all) = all else {
            self.out.send(Ev::Stickers(sets));
            return Ok(());
        };
        for set in all.sets.into_iter().take(24) {
            let tl::enums::StickerSet::Set(set) = set;
            if set.archived || set.masks || set.emojis {
                continue;
            }
            let input = tl::types::InputStickerSetId { id: set.id, access_hash: set.access_hash };
            let Ok(tl::enums::messages::StickerSet::Set(full)) = client
                .invoke(&tl::functions::messages::GetStickerSet { stickerset: input.into(), hash: 0 })
                .await
            else {
                continue;
            };
            let stickers = self.remember_docs(full.documents);
            sets.push(StickerSetInfo { id: set.id, title: set.title, stickers });
        }
        self.out.send(Ev::Stickers(sets));
        Ok(())
    }

    fn remember_docs(&mut self, docs: Vec<tl::enums::Document>) -> Vec<(i64, String)> {
        docs.into_iter()
            .filter_map(|d| match d {
                tl::enums::Document::Document(d) => Some(d),
                _ => None,
            })
            .map(|d| {
                let emoji = d
                    .attributes
                    .iter()
                    .find_map(|a| match a {
                        tl::enums::DocumentAttribute::Sticker(s) => Some(s.alt.clone()),
                        _ => None,
                    })
                    .unwrap_or_default();
                let id = d.id;
                self.docs.insert(id, d);
                (id, emoji)
            })
            .collect()
    }

    async fn load_dialogs(&mut self) -> Result<(), String> {
        let client = self.client()?;
        self.load_notify_defaults().await;
        let mut iter = client.iter_dialogs();
        let mut chats = Vec::new();
        let mut peers = Vec::new();
        while chats.len() < DIALOG_LIMIT {
            let Some(d) = iter.next().await.map_err(|e| lf!("Чати: {e}", "Chats: {e}"))? else {
                break;
            };
            let peer = d.peer();
            self.refs.lock().unwrap().insert(peer.id(), d.peer_ref());
            let (unread, pinned, muted, archived, unread_mark, draft, (read_out, read_in)) = match &d.raw {
                tl::enums::Dialog::Dialog(x) => {
                    let mut notifs = self.notifs.lock().unwrap();
                    notifs.remember(peer);
                    let muted = notifs.resolve(peer.id(), NotifyState::from_raw(&x.notify_settings)).muted();
                    drop(notifs);
                    (x.unread_count, x.pinned, muted, x.folder_id == Some(1), x.unread_mark, draft_text(&x.draft), (x.read_outbox_max_id, x.read_inbox_max_id))
                }
                _ => (0, false, false, false, false, String::new(), (0, 0)),
            };
            let (unread_mentions, unread_reactions) = match &d.raw {
                tl::enums::Dialog::Dialog(x) => (x.unread_mentions_count, x.unread_reactions_count),
                _ => (0, 0),
            };
            self.names.lock().unwrap().insert(peer.id(), peer_title(peer));
            let (last_text, last_time, last_ts) = match &d.last_message {
                Some(m) => {
                    let mut text = preview_text(m);
                    if m.outgoing() {
                        text = lf!("Ви: {text}", "You: {text}");
                    } else if !matches!(peer, Peer::User(_) | Peer::Channel(_))
                        && let Some(s) = m.sender()
                    {
                        text = format!("{}: {text}", short_name(s));
                    }
                    (text, short_time(m.date()), m.date().as_second())
                }
                None => (String::new(), String::new(), 0),
            };
            chats.push(ChatInfo {
                id: peer.id(),
                title: peer_title(peer),
                kind: peer_kind(peer),
                last_text,
                last_time,
                last_ts,
                unread,
                pinned,
                contact: matches!(peer, Peer::User(u) if u.contact()),
                muted,
                archived,
                unread_mark,
                draft,
                presence: match peer {
                    Peer::User(u) if !u.is_self() && !u.is_bot() => Presence::from_status(u.status()),
                    _ => Presence::None,
                },
                forum: matches!(peer, Peer::Group(g) if matches!(&g.raw, tl::enums::Chat::Channel(c) if c.forum)),
                joined: true,
                can_post: can_post(peer),
                read_out,
                read_in,
                unread_mentions,
                unread_reactions,
                noforwards: noforwards(peer),
                can_pin: can_pin(peer),
                restricted: chat_restriction(peer),
            });
            peers.push(peer.clone());
        }
        self.cache_dialogs(chats.clone());
        self.out.send(Ev::Dialogs(chats));
        if let Err(e) = self.load_folders().await {
            log_error(&e);
        }
        if let Some(loader) = &self.loader {
            for p in &peers {
                loader.avatar(p);
            }
        }
        Ok(())
    }

    /// The chat list goes to the cache with access hashes: with them a chat opens even before connecting.
    fn cache_dialogs(&self, chats: Vec<ChatInfo>) {
        let Some(c) = &self.out.cache else { return };
        let refs = self.refs.lock().unwrap();
        c.dialogs(
            chats
                .into_iter()
                .map(|chat| {
                    let auth = refs.get(&chat.id).map(|r| r.auth.hash());
                    (chat, auth)
                })
                .collect(),
        );
    }

    async fn fetch_history(&self, client: &Client, r: PeerRef, before: Option<i32>) -> Result<Vec<Msg>, String> {
        let mut iter = client.iter_messages(r).limit(PAGE);
        if let Some(id) = before {
            iter = iter.offset_id(id);
        }
        let mut out = Vec::with_capacity(PAGE);
        while let Some(m) = iter.next().await.map_err(|e| lf!("Історія: {e}", "History: {e}"))? {
            out.push(self.convert_loaded(r.id, &m));
        }
        out.reverse();
        Ok(out)
    }

    /// Converts a history message, also queuing the photo, avatar and author name.
    fn convert_loaded(&self, peer: PeerId, m: &Message) -> Msg {
        if let Some(loader) = &self.loader {
            loader.message_photo(peer, m);
            if let Some(s) = m.sender() {
                loader.avatar(s);
            }
        }
        remember_name(&self.names, m);
        let mut msg = convert(m, &self.names);
        msg.peer = peer;
        msg
    }

    /// An action on a chat from the list; on error reload dialogs to undo the optimistic change in the UI.
    async fn chat_action<R: tl::RemoteCall>(&mut self, req: &R, what: &str) -> Result<(), String> {
        let client = self.client()?;
        if let Err(e) = client.invoke(req).await {
            let _ = self.load_dialogs().await;
            return Err(format!("{what}: {e}"));
        }
        Ok(())
    }

    async fn search(&self, peer: Option<PeerId>, query: String, kind: Option<SharedKind>, from: Option<PeerId>) -> Result<(), String> {
        let client = self.client()?;
        let mut hits = Vec::new();
        let err = |e| lf!("Пошук: {e}", "Search: {e}");
        match peer {
            // grammers can only filter by our own messages: one member needs the raw request.
            Some(peer) if from.is_some() => {
                let r = self.peer_ref(peer)?;
                let who = self.peer_ref(from.unwrap_or(peer))?;
                let req = tl::functions::messages::Search {
                    peer: r.into(),
                    q: query.clone(),
                    from_id: Some(who.into()),
                    saved_peer_id: None,
                    saved_reaction: None,
                    top_msg_id: None,
                    // A type filter with a sender is INPUT_FILTER_INVALID; the UI never sends both.
                    filter: tl::enums::MessagesFilter::InputMessagesFilterEmpty,
                    min_date: 0,
                    max_date: 0,
                    offset_id: 0,
                    add_offset: 0,
                    limit: SEARCH_LIMIT as i32,
                    max_id: 0,
                    min_id: 0,
                    hash: 0,
                };
                let ids: Vec<i32> = match client.invoke(&req).await.map_err(err)? {
                    tl::enums::messages::Messages::Messages(m) => m.messages,
                    tl::enums::messages::Messages::Slice(m) => m.messages,
                    tl::enums::messages::Messages::ChannelMessages(m) => m.messages,
                    tl::enums::messages::Messages::NotModified(_) => Vec::new(),
                }
                .iter()
                .map(|m| m.id())
                .collect();
                if !ids.is_empty() {
                    for m in client.get_messages_by_id(r, &ids).await.map_err(err)?.into_iter().flatten() {
                        let mut msg = convert(&m, &self.names);
                        msg.peer = peer;
                        hits.push(SearchHit { chat: String::new(), kind: ChatKind::User, msg });
                    }
                }
            }
            Some(peer) => {
                let r = self.peer_ref(peer)?;
                let mut iter = client.search_messages(r).query(&query).limit(SEARCH_LIMIT);
                if let Some(k) = kind {
                    iter = iter.filter(k.filter());
                }
                while let Some(m) = iter.next().await.map_err(err)? {
                    let mut msg = convert(&m, &self.names);
                    msg.peer = peer;
                    hits.push(SearchHit { chat: String::new(), kind: ChatKind::User, msg });
                }
            }
            None => {
                let mut iter = client.search_all_messages().query(&query).limit(SEARCH_LIMIT);
                while let Some(m) = iter.next().await.map_err(err)? {
                    if let Ok(Some(r)) = m.peer_ref().await {
                        self.refs.lock().unwrap().entry(r.id).or_insert(r);
                    }
                    let (chat, kind) = m.peer().map_or_else(|| (String::new(), ChatKind::User), |p| (peer_title(p), peer_kind(p)));
                    hits.push(SearchHit { chat, kind, msg: convert(&m, &self.names) });
                }
            }
        }
        self.out.send(Ev::SearchResults { peer, query, kind, from, hits });
        Ok(())
    }
}

/// New chat: contacts, people search, creating groups and channels.
impl Worker {
    /// Remembers access to a found user or chat and fetches its avatar.
    async fn remember(&self, peer: &Peer) -> bool {
        let Ok(Some(r)) = peer.to_ref().await else { return false };
        self.refs.lock().unwrap().insert(peer.id(), r);
        self.names.lock().unwrap().insert(peer.id(), peer_title(peer));
        if let Some(loader) = &self.loader {
            loader.avatar(peer);
        }
        true
    }

    async fn load_contacts(&self) -> Result<(), String> {
        let client = self.client()?;
        let res = client
            .invoke(&tl::functions::contacts::GetContacts { hash: 0 })
            .await
            .map_err(|e| lf!("Контакти: {e}", "Contacts: {e}"))?;
        let tl::enums::contacts::Contacts::Contacts(res) = res else { return Ok(()) };
        let mut list = Vec::new();
        for raw in res.users {
            let peer = Peer::User(User::from_raw(&client, raw));
            if self.remember(&peer).await {
                list.push(person(&peer));
            }
        }
        list.sort_by_key(|c| c.title.to_lowercase());
        self.out.send(Ev::Contacts(list));
        Ok(())
    }

    async fn search_contacts(&self, query: String) -> Result<(), String> {
        let client = self.client()?;
        let req = tl::functions::contacts::Search { broadcasts: false, bots: false, q: query.clone(), limit: 30 };
        let tl::enums::contacts::Found::Found(res) = client.invoke(&req).await.map_err(|e| lf!("Пошук: {e}", "Search: {e}"))?;
        // Own results (contacts, dialogs) first, then global ones, as the server orders them.
        let wanted: Vec<PeerId> = res.my_results.iter().chain(&res.results).map(PeerId::from).collect();
        let mut chats = Vec::new();
        for raw in res.users {
            let peer = Peer::User(User::from_raw(&client, raw));
            if wanted.contains(&peer.id()) && self.remember(&peer).await {
                chats.push(person(&peer));
            }
        }
        chats.sort_by_key(|c| wanted.iter().position(|p| *p == c.id));
        self.out.send(Ev::ContactResults { query, chats });
        Ok(())
    }

}

#[cfg(test)]
mod tests {
    #[test]
    fn waveform_unpacks_5bit_values() {
        // 31, 0, 31, 0 in 5-bit packing.
        let packed = [0b0001_1111, 0b0111_1100, 0b0000_0000];
        assert_eq!(&super::unpack_waveform(&packed)[..4], &[31, 0, 31, 0]);
    }

    #[test]
    fn waveform_roundtrip() {
        let levels: Vec<u8> = (0..100).map(|i| (i * 7 % 32) as u8).collect();
        assert_eq!(super::unpack_waveform(&super::pack_waveform(&levels))[..100], levels[..]);
    }
}

fn log_error(e: &str) {
    eprintln!("glassgram: {e}");
}

/// Files of messages no longer in this account's queue (sent before a crash, or the demo's): removed.
fn prune_outbox(session: &std::path::Path, queued: &[Queued]) {
    let Ok(dirs) = std::fs::read_dir(session.with_file_name("outbox")) else { return };
    // A fresh folder may belong to a message the UI is queueing right now.
    let old = |d: &std::fs::DirEntry| d.metadata().and_then(|m| m.modified()).ok().and_then(|t| t.elapsed().ok()).is_some_and(|age| age > Duration::from_secs(3600));
    for d in dirs.flatten() {
        let key = d.file_name().to_string_lossy().parse::<i64>().ok();
        if old(&d) && !key.is_some_and(|k| queued.iter().any(|q| q.key == k)) {
            let _ = std::fs::remove_dir_all(d.path());
        }
    }
}

pub fn human_size(bytes: usize) -> String {
    let b = bytes as f64;
    match bytes {
        0..1024 => lf!("{bytes} Б", "{bytes} B"),
        1024..1_048_576 => lf!("{:.1} КБ", "{:.1} KB", b / 1024.0),
        1_048_576..1_073_741_824 => lf!("{:.1} МБ", "{:.1} MB", b / 1_048_576.0),
        _ => lf!("{:.2} ГБ", "{:.2} GB", b / 1_073_741_824.0),
    }
}


#[cfg(test)]
mod builtin_tests {
    /// Only meaningful in a build with keys: `GLASSGRAM_API_ID=… GLASSGRAM_API_HASH=… cargo test builtin`.
    #[test]
    fn builtin_keys_round_trip() {
        if let (Some(id), Some(hash)) = (option_env!("GLASSGRAM_API_ID"), option_env!("GLASSGRAM_API_HASH")) {
            assert_eq!(super::builtin_credentials(), Some((id.trim().parse().unwrap(), hash.trim().to_owned())));
        } else {
            assert_eq!(super::builtin_credentials(), None);
        }
    }
}
