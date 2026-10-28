use std::cell::RefCell;
use std::collections::{HashMap, HashSet};
use std::path::PathBuf;
use std::sync::mpsc;
use std::time::Duration;

use eframe::egui::{
    self, Align, Align2, Color32, FontId, Key, Rect, Sense, TextureHandle, TextureOptions, Ui, UiBuilder,
    pos2, text::LayoutJob, text::TextWrapping, vec2,
};
use grammers_client::session::types::PeerId;

use crate::accounts::{self, Accounts};
use crate::backend::{
    self, Audience, ButtonAction, ChatInfo, ChatKind, Cmd, Ev, Folder, Handle, ImgKey, InlineResult, MediaKind, Msg, PrivacyKey, PrivacyState, Reaction,
    SearchHit, SendOpts, SharedKind, Session, Topic, GENERAL_TOPIC, TOPIC_COLORS,
    StickerSetInfo, ReplyTo, QueuedMedia,
};
use crate::demo;
use crate::emoji;
use crate::viewer::{Viewer, ViewerAction};
use crate::format::{self, Style};
use crate::glass::{self, ACCENT, Glass, ink, ink_dim, rgba};
use crate::icons::{self, Icon};
use crate::player::{self, Player};
use crate::camera::{self, RoundRecorder};
use crate::recorder::{self, Recorder, Voice};
use crate::settings::{self, Settings};
use crate::tray::{self, TrayEv};
use crate::video::{Clip, Spec};
use crate::i18n::{l, lf};

mod chatview;
mod suggest;
mod info;
mod hotkeys;
mod outbox;
mod settings_pages;
mod chatlist;
mod sponsored;
mod notify;
mod calendar;
mod translate;
mod contacts;
mod webapp;
mod polls;
mod privacy;
mod password;
mod chatedit;
mod birthdays;
mod botstart;
mod similar;
mod saved;
mod chatsearch;
mod stories;
mod login;
mod accounts_ui;
mod forum;
mod comments;
mod dialogs;
mod composer;
mod recording;
mod media;
mod folders;
mod history;
mod confirm;
mod gifts;
mod textures;
mod emojikw;
mod newchat;
mod quote;
mod report;
mod bubbles;
use bubbles::*;
mod sendbox;
mod editmedia;
mod websites;
mod spoiler;
use chatlist::{Reorder, sort_chats};
use newchat::NewChat;

const GAP: f32 = 14.0;
const HEADER_H: f32 = 62.0;
const COMPOSER_H: f32 = 50.0;
/// Maximum lines in the input field; beyond that it scrolls.
const COMPOSER_ROWS: usize = 6;
const COMPOSE_BAR_H: f32 = 42.0;
const TABS_H: f32 = 32.0;
/// Id of the "Archive" tab among folders.
const ARCHIVE: i32 = i32::MIN;
/// Pause after typing before searching on the server or saving the draft.
const SEARCH_DELAY: f64 = 0.45;
const DRAFT_DELAY: f64 = 3.0;
/// How often a changed chat list goes to the local cache (and always on exit).
const CHATS_SAVE_EVERY: f64 = 30.0;
const TYPING_EVERY: f64 = 5.0;
const TYPING_TTL: f64 = 6.0;
const JUMP_FLASH: f64 = 1.6;
/// How far off-screen we still draw bubbles: wheel scrolling can't outrun it and show emptiness.
const VIRTUAL_MARGIN: f32 = 800.0;
/// Inline video size in points.
const VIDEO_PT: f32 = 340.0;
const ROUND_PT: f32 = 220.0;
const STICKER_PT: f32 = 160.0;

#[derive(Clone, PartialEq)]
enum Step {
    Creds,
    Connecting,
    Phone,
    Qr(Option<String>),
    Code,
    Password(Option<String>),
}

enum Screen {
    Login(Step),
    Main,
}

/// What the input field does: a plain message, a reply or an edit.
#[derive(Clone, Copy, PartialEq)]
enum Compose {
    New,
    Reply(i32),
    Edit(i32),
}

enum Action {
    Reply(i32),
    Edit(i32),
    Copy(String),
    Delete(i32),
    Download(i32),
    /// Click on a link, mention, hashtag or bot command.
    Open(String),
    /// A link whose text differs from its address: asks before opening.
    OpenHidden(String),
    RevealSpoiler(i32),
    /// Enable selection with this message or toggle it in the selection.
    Pick(i32),
    Forward(i32),
    /// Bot button: (message, row, column).
    Button(i32, usize, usize),
    /// Open a photo in the viewer.
    View(i32),
    /// Add or remove a reaction.
    React(i32, String),
    Vote(i32, Vec<Vec<u8>>),
    /// Actions on the whole album.
    ForwardMany(Vec<i32>),
    DeleteMany(Vec<i32>),
    PickMany(Vec<i32>),
    OpenMap(f64, f64),
    /// Tick an option in a multiple-answer poll.
    TogglePoll(i32, Vec<u8>),
    Copy2(String, &'static str),
    /// Play / pause media (video opens in the system player).
    Play(i32),
    /// Seek the current audio to a fraction of its duration.
    Seek(i32, f32),
    Speed,
    /// Open a video in the system player.
    OpenExternal(i32),
    /// Open comments on a channel post.
    Comments(i32),
    /// Pin: (message, silent, only for me).
    Pin(i32, bool, bool),
    Unpin(i32),
    /// Show which group members have read it.
    ReadBy(i32),
    Translate(i32),
    /// Report a message to Telegram's moderators.
    Report(i32),
    /// Queued message: send again / drop without sending.
    Retry(i32),
    Discard(i32),
}

/// What is being scheduled: a draft from the input field or an already scheduled message (time change).
#[derive(Clone, Copy, PartialEq)]
enum ScheduleFor {
    Draft,
    Existing(i32),
}

/// State of the inline query `@bot query`.
struct Inline {
    bot: String,
    query: String,
    requested: Option<(String, String)>,
    results: Vec<InlineResult>,
    query_id: i64,
    /// Which query the results came for.
    results_for: Option<(String, String)>,
}

/// How to draw a bubble.
#[derive(Clone, Copy)]
struct Look<'a> {
    /// Ticked options of multiple-answer polls.
    chosen: &'a [Vec<u8>],
    group: bool,
    revealed: bool,
    /// Selection mode: a click toggles instead of opening.
    picking: bool,
    picked: bool,
    play: PlayView,
    /// Ticks next to the time of our messages: `Some(true)` means read (✓✓).
    ticks: Option<bool>,
    /// Chat the bubble belongs to: menu items depend on it (pinning, protected content).
    chat: Option<&'a ChatInfo>,
    /// Queued message: `Some(false)` — sending (clock), `Some(true)` — failed (red mark).
    pending: Option<bool>,
}

/// Playback state for a media card.
#[derive(Clone, Copy, Default)]
struct PlayView {
    /// This message is in the player right now.
    current: bool,
    playing: bool,
    loading: bool,
    progress: f32,
    position: f64,
    speed: f32,
}

/// A file being sent right now.
struct Upload {
    id: u64,
    name: String,
    done: u64,
    total: u64,
}

/// The "Forward to…" window.
struct ForwardPicker {
    from: PeerId,
    ids: Vec<i32>,
    query: String,
    /// Choosing the chat to reply in ("Reply in another chat"), not to forward to.
    reply: bool,
}

/// Comments panel for a channel post.
struct CommentsView {
    peer: PeerId,
    post: i32,
    /// Discussion group and its root message (known after loading).
    discussion: Option<(PeerId, i32)>,
    msgs: Vec<Msg>,
    draft: String,
    loading: bool,
    exhausted: bool,
}

/// Name from a `t.me/<name>` link (no further path: posts and invites open in the browser).
fn tme_username(url: &str) -> Option<String> {
    let rest = url.trim_start_matches("https://").trim_start_matches("http://");
    let name = rest.strip_prefix("t.me/").or_else(|| rest.strip_prefix("telegram.me/"))?.trim_end_matches('/');
    let ok = name.len() >= 4 && name.chars().all(|c| c.is_ascii_alphanumeric() || c == '_') && !name.starts_with('+');
    ok.then(|| name.to_owned())
}

/// Mini app link: `t.me/<bot>/<app>[?startapp=…]` or `t.me/<bot>?startapp[=…]`.
/// Returns (bot username, app short name, start parameter).
fn tme_web_app(url: &str) -> Option<(String, Option<String>, String)> {
    let rest = url.trim_start_matches("https://").trim_start_matches("http://");
    let rest = rest.strip_prefix("t.me/").or_else(|| rest.strip_prefix("telegram.me/"))?;
    let (path, query) = rest.split_once('?').unwrap_or((rest, ""));
    let start = query.split('&').find_map(|kv| match kv.split_once('=') {
        Some(("startapp", v)) => Some(v.to_owned()),
        None if kv == "startapp" => Some(String::new()),
        _ => None,
    });
    let mut parts = path.trim_end_matches('/').split('/');
    let bot = parts.next().filter(|b| b.len() >= 4 && b.chars().all(|c| c.is_ascii_alphanumeric() || c == '_'))?;
    match (parts.next(), parts.next()) {
        // A second segment that is a number is a post link, not an app.
        (Some(app), None) if bot.to_lowercase().ends_with("bot") && !app.chars().all(|c| c.is_ascii_digit()) => {
            Some((bot.to_owned(), Some(app.to_owned()), start.unwrap_or_default()))
        }
        (None, None) => Some((bot.to_owned(), None, start?)),
        _ => None,
    }
}

/// "1 comment", "3 comments", "5 comments".
fn comments_label(n: i32) -> String {
    let word = match (n % 10, n % 100) {
        (1, r) if r != 11 => l!("коментар", "comment"),
        (2..=4, r) if !(12..=14).contains(&r) => l!("коментарі", "comments"),
        _ => l!("коментарів", "comments"),
    };
    if n == 0 { l!("Залишити коментар", "Leave a comment").to_owned() } else { format!("{n} {word}") }
}

/// Folder create or edit window.
struct FolderEdit {
    folder: Folder,
    new: bool,
    query: String,
}

/// Settings pages: the main list and the sections as in Telegram Desktop.
#[derive(Clone, Copy, PartialEq)]
enum SettingsTab {
    Main,
    Account,
    Notifications,
    Privacy,
    Chats,
    Folders,
    Advanced,
    Devices,
    Language,
}

/// Auto-lock options in minutes (0 means only at startup).
fn lock_choices() -> [(u32, &'static str); 5] {
    [(0, l!("Лише на старті", "Only at startup")), (1, l!("1 хв", "1 min")), (5, l!("5 хв", "5 min")), (15, l!("15 хв", "15 min")), (60, l!("1 год", "1 hour"))]
}

enum FolderMenu {
    New,
    Edit(i32),
    Delete(i32),
}

/// Longest folder name Telegram accepts.
const FOLDER_TITLE_MAX: usize = 12;

/// Actions from a chat's context menu in the list.
enum ChatAction {
    Pin(bool),
    Mute(bool),
    Read,
    Unread,
    Archive(bool),
    Export,
}

/// Search: what was typed, what was already requested and what came back.
#[derive(Default)]
struct Search {
    query: String,
    /// In-chat search only: one type of message (photos, files, …), which needs no query.
    kind: Option<SharedKind>,
    /// In-chat search in groups: messages of one member (id, name).
    from: Option<(PeerId, String)>,
    /// The field searches members to pick `from`; the typed query waits in `stash`.
    picking: bool,
    stash: String,
    /// Member search last asked for while picking.
    members_asked: Option<String>,
    edited_at: f64,
    requested: SearchKey,
    hits: Vec<SearchHit>,
    hits_for: SearchKey,
}

/// What results are for: query, type filter, sender.
type SearchKey = (String, Option<SharedKind>, Option<PeerId>);

impl Search {
    fn key(&self) -> SearchKey {
        (self.query.trim().to_owned(), self.kind, self.from.as_ref().map(|f| f.0))
    }

    /// Whether it's time to send the request (after a pause in typing).
    fn due(&self, now: f64) -> Option<String> {
        let key = self.key();
        (!self.picking && self.active() && key != self.requested && now - self.edited_at > SEARCH_DELAY).then_some(key.0)
    }

    /// Something to search for: results (or a spinner) are shown. Filters alone are enough.
    fn active(&self) -> bool {
        let q = self.query.trim();
        q.chars().count() >= 2 || q.is_empty() && (self.kind.is_some() || self.from.is_some())
    }

    fn ready(&self) -> bool {
        self.hits_for == self.key()
    }

    fn accept(&mut self, key: SearchKey, hits: Vec<SearchHit>) {
        if key == self.key() {
            self.hits = hits;
            self.hits_for = key;
        }
    }
}

pub struct App {
    ctx: egui::Context,
    backend: Option<Handle>,
    screen: Screen,
    busy: bool,
    /// The chat list changed since the last cache snapshot.
    chats_dirty: bool,
    chats_saved_at: f64,
    /// Sign-in form fields.
    login: login::LoginForm,
    /// (text, time shown, is error)
    toast: Option<(String, f64, bool)>,
    me: String,
    chats: Vec<ChatInfo>,
    selected: Option<PeerId>,
    history: HashMap<PeerId, Vec<Msg>>,
    exhausted: HashMap<PeerId, bool>,
    textures: textures::Textures,
    loading_older: bool,
    search: Search,
    /// Search in the open chat (`None` means the panel is closed).
    chat_search: Option<Search>,
    draft: String,
    /// Draft last saved on the server for the open chat.
    draft_synced: String,
    draft_edited_at: f64,
    /// Draft set aside while a message is being edited.
    edit_stash: String,
    typing: HashMap<PeerId, (String, f64)>,
    typing_sent_at: f64,
    /// History scrolling: measured row heights, staying at the bottom, messages that came below, the jump target.
    scroll: history::Scroll,
    revealed: HashSet<(PeerId, i32)>,
    compose: Compose,
    folders: Vec<Folder>,
    /// Folder tags switch and suggested folders.
    folder_meta: folders::FolderMeta,
    folder: Option<i32>,
    /// The sticker and emoji panel: sets, the open set, thumbnails asked for.
    stickers: composer::StickerPanel,
    uploads: Vec<Upload>,
    /// Selected messages of the open chat.
    picked: Vec<i32>,
    forward: Option<ForwardPicker>,
    /// Open scheduling window: what for, and the typed time "HH:MM".
    schedule: Option<(ScheduleFor, String)>,
    scheduled: HashMap<PeerId, Vec<Msg>>,
    scheduled_open: bool,
    viewer: Option<Viewer>,
    inline: Option<Inline>,
    inline_edited_at: f64,
    /// Options ticked but not yet sent in multiple-answer polls.
    multi_vote: HashMap<(PeerId, i32), Vec<Vec<u8>>>,
    settings: Settings,
    settings_open: bool,
    title_unread: usize,
    /// Files from the system dialog: (chat, files, uncompressed).
    picked_tx: mpsc::Sender<(PeerId, Vec<PathBuf>, bool)>,
    picked_rx: mpsc::Receiver<(PeerId, Vec<PathBuf>, bool)>,
    /// Playing media: the audio player, downloaded files, animated clips.
    playback: media::Playback,
    /// Voice and round video recording: what is being recorded, a draft waiting to be sent.
    rec: recording::Recording,
    tray: Option<tray::TrayHandle>,
    tray_rx: mpsc::Receiver<TrayEv>,
    /// "Quit" in the tray: the window really closes instead of hiding to the tray.
    quitting: bool,
    /// The "still running in the tray" notice is shown once per run.
    tray_hint_shown: bool,
    folder_edit: Option<FolderEdit>,
    accounts: Accounts,
    /// Open accounts menu under the name in the header.
    accounts_open: bool,
    accounts_anchor: Rect,
    /// The other accounts, connected in the background for counters and notifications.
    background: Vec<accounts_ui::Background>,
    /// Demo mode: no network and no account switching.
    demo: bool,
    /// Input field height in the last frame; space is reserved for it at the bottom of the history.
    composer_h: f32,
    /// Forum topics, the open topic and the topic editor.
    forum: forum::Forum,
    comments: Option<CommentsView>,
    /// Public chats from the global search by name.
    public_hits: Vec<ChatInfo>,
    /// Export window: chat and whether to export media.
    export_ask: Option<(PeerId, bool)>,
    /// Chats being exported right now.
    exporting: HashSet<PeerId>,
    settings_tab: SettingsTab,
    /// Cameras for settings (looked up when the tab is opened).
    cameras: Option<Vec<camera::Camera>>,
    sessions: Vec<Session>,
    /// Privacy and Security page: rules, their exceptions, the open rule, the people picker, global switches.
    privacy: privacy::PrivacyPage,
    /// Two-step verification section of Privacy and Security.
    two_step: password::TwoStep,
    /// A second press confirms "Terminate all other sessions".
    confirm_terminate: bool,
    /// Local passcode fields: new, repeat, current (for disabling).
    pass_fields: [String; 3],
    /// Local passcode lock: whether locked, the typed passcode, the last error, the last input.
    lock: login::Lock,
    /// Pinned messages of the open chat: the list, which one the bar shows, the list panel, chats where the bar was hidden.
    pinned: chatview::Pinned,
    /// Ids of unread mentions (`false`) and reactions (`true`), oldest first.
    marks: HashMap<(PeerId, bool), Vec<i32>>,
    /// The "@"/"❤" button was pressed while the list was still loading.
    mark_wanted: Option<(PeerId, bool)>,
    /// "Seen by" popup: message and readers (`None` — still loading).
    read_by: Option<(PeerId, i32, Option<Vec<(String, String)>>)>,
    /// Sponsored messages of channels and their popups.
    ads: sponsored::Ads,
    /// Notification defaults of chat categories and their exceptions (Settings → Notifications).
    notify: notify::NotifyPrefs,
    /// "Jump to date" calendar over the open chat.
    calendar: Option<calendar::Calendar>,
    /// "Translate" window and the last language chosen in it.
    translate: Option<translate::TranslateView>,
    translate_to: Option<String>,
    /// "New contact" / "Add to contacts" box.
    contact_form: Option<contacts::ContactForm>,
    /// Blocked users for Privacy and Security (`None` — not loaded).
    blocked: Option<Vec<ChatInfo>>,
    /// "New poll" box.
    poll_form: Option<polls::PollForm>,
    /// The "Send files" box with what is about to be sent.
    send_box: Option<sendbox::SendBox>,
    spoilers: spoiler::Spoilers,
    edit_media: editmedia::EditMedia,
    websites: websites::Websites,
    /// Open mini app windows (separate WebView processes).
    web_apps: Vec<webapp::WebAppWin>,
    /// Text of the keyboard button whose mini app is being opened (for `sendWebViewData`).
    web_app_button: String,
    /// A mini app's request waiting for the user's answer in this window.
    web_app_ask: Option<(PeerId, webapp::Ask)>,
    /// Composer helpers: link preview, @mentions, bot commands.
    suggest: suggest::Suggest,
    /// Chat info column (`None` = closed); follows the open chat.
    info: Option<info::InfoPanel>,
    /// Own profile for the settings header and "My account".
    profile: backend::Profile,
    profile_edit: Option<settings_pages::ProfileEdit>,
    /// Profile photo picked in the system dialog.
    photo_tx: mpsc::Sender<PathBuf>,
    photo_rx: mpsc::Receiver<PathBuf>,
    /// Once-per-session requests (birthdays, stories) were sent after the chat list arrived.
    session_extras: bool,
    /// Today's birthdays reminder and our own birthday being edited.
    birthdays: birthdays::Birthdays,
    /// Saved Messages split by source.
    saved: saved::Saved,
    /// The story strip and the story being watched.
    stories: stories::Stories,
    /// The "Report" box for messages.
    report: Option<report::ReportBox>,
    /// Picking a quote for the reply, and the chosen one.
    quoting: quote::Quoting,
    /// A hidden link or a received program waiting for "Open anyway".
    opening: Option<confirm::Opening>,
    /// Start parameters from `t.me/<bot>?start=` links, until Start is pressed in that bot chat.
    bot_start: HashMap<PeerId, String>,
    /// "Edit group / channel" box and the photos picked in it.
    chat_edit: Option<chatedit::ChatEdit>,
    chat_photo_tx: mpsc::Sender<(PeerId, PathBuf)>,
    chat_photo_rx: mpsc::Receiver<(PeerId, PathBuf)>,
    /// Send queue (`app/outbox.rs`) and the last local (negative) bubble id.
    outbox: Vec<outbox::Pending>,
    next_local: i32,
    /// Ctrl+K: focus the chat list search field on the next frame.
    focus_search: bool,
    /// Pinned chat being dragged, with the folder it is dragged in.
    pin_drag: Option<(Option<i32>, Reorder<PeerId>)>,
    folder_drag: Option<Reorder<i32>>,
    /// Open "New chat" menu, anchored to the pencil button.
    new_chat_menu: Option<Rect>,
    new_chat: Option<NewChat>,
    contacts: Vec<ChatInfo>,
    /// People search for "New chat": query and results.
    contact_hits: (String, Vec<ChatInfo>),
}

impl App {
    pub fn new(cc: &eframe::CreationContext<'_>, demo_mode: bool) -> Self {
        setup_style(&cc.egui_ctx);
        glass::set_gpu(crate::blur::install(cc));
        let (tray_tx, tray_rx) = mpsc::channel();
        let settings = Settings::load();
        crate::i18n::set(&settings.lang);
        cc.egui_ctx.set_zoom_factor(settings.scale);
        let tray = tray::spawn(tray_tx, cc.egui_ctx.clone(), settings.notifications);
        let mut app = Self::fresh(cc.egui_ctx.clone(), settings, tray, tray_rx);
        if demo_mode {
            app.demo = true;
            let (me, chats, history) = demo::data();
            app.accounts = Accounts { active: 0, list: vec![accounts::Account { id: 0, name: me.clone() }, accounts::Account { id: 1, name: "Робочий акаунт".into() }] };
            // The second account "in the background" has unread messages: the dot and the badge.
            app.background.push(accounts_ui::Background::demo(1, 3));
            app.me = me;
            app.chats = chats;
            app.history = history;
            app.folders = demo::folders();
            app.folder_meta.tags = true;
            if let Some(first) = app.chats.first().map(|c| c.id) {
                app.open_chat(first);
            }
            for (key, image) in demo::images() {
                app.push_event(Ev::Image { key, image });
            }
            app.push_event(Ev::Outbox(demo::outbox()));
            app.session_extras = true;
            app.send(Cmd::LoadBirthdays);
            app.send(Cmd::LoadStories);
            app.screen = Screen::Main;
            return app;
        }
        app.start_backend();
        app.start_background();
        app
    }

    /// Backend for the active account; with saved api_id/api_hash we connect right away.
    fn start_backend(&mut self) {
        let session = accounts::session_path(self.accounts.active);
        self.backend = Some(backend::spawn(self.ctx.clone(), session));
        if let Some((id, hash)) = backend::saved_credentials() {
            // The built-in keys stay out of the form: an offline fallback to the key screen would show them.
            if !backend::is_builtin(id, &hash) {
                self.login.api_id = id.to_string();
                self.login.api_hash = hash.clone();
            }
            self.send(Cmd::Connect { api_id: id, api_hash: hash });
            self.screen = Screen::Login(Step::Connecting);
        }
    }

    /// Snapshot of the chat list to the cache: the next launch shows fresh previews and counters before the network.
    fn save_chats(&mut self) {
        if self.backend.is_none() || !matches!(self.screen, Screen::Main) || self.chats.is_empty() {
            return;
        }
        self.chats_dirty = false;
        let chats = self.chats.iter().filter(|c| c.joined).cloned().collect();
        self.send(Cmd::CacheDialogs(chats));
    }

    /// App state without an account: launch and account switching start like this.
    fn fresh(ctx: egui::Context, settings: Settings, tray: Option<tray::TrayHandle>, tray_rx: mpsc::Receiver<TrayEv>) -> Self {
        let (picked_tx, picked_rx) = mpsc::channel();
        let (photo_tx, photo_rx) = mpsc::channel();
        let (chat_photo_tx, chat_photo_rx) = mpsc::channel();
        Self {
            ctx,
            backend: None,
            screen: Screen::Login(Step::Creds),
            busy: false,
            chats_dirty: false,
            chats_saved_at: 0.0,
            login: login::LoginForm {
                api_id: String::new(),
                api_hash: String::new(),
                phone: String::new(),
                code: String::new(),
                password: String::new(),
            },
            toast: None,
            me: String::new(),
            chats: Vec::new(),
            selected: None,
            history: HashMap::new(),
            exhausted: HashMap::new(),
            textures: textures::Textures::default(),
            loading_older: false,
            search: Search::default(),
            chat_search: None,
            draft: String::new(),
            draft_synced: String::new(),
            draft_edited_at: 0.0,
            edit_stash: String::new(),
            typing: HashMap::new(),
            typing_sent_at: 0.0,
            scroll: history::Scroll {
                row_h: RefCell::new(HashMap::new()),
                at_bottom: true,
                to_bottom: false,
                unseen_below: 0,
                jump: None,
                unread: None,
            },
            revealed: HashSet::new(),
            compose: Compose::New,
            folders: Vec::new(),
            folder_meta: folders::FolderMeta::default(),
            folder: None,
            stickers: composer::StickerPanel {
                sets: Vec::new(),
                open: false,
                set: 0,
                thumbs: HashSet::new(),
                requested: false,
                emoji_tab: true,
            },
            uploads: Vec::new(),
            picked: Vec::new(),
            forward: None,
            schedule: None,
            scheduled: HashMap::new(),
            scheduled_open: false,
            viewer: None,
            inline: None,
            inline_edited_at: 0.0,
            multi_vote: HashMap::new(),
            settings: settings.clone(),
            accounts: Accounts::load(),
            settings_open: false,
            title_unread: 0,
            picked_tx,
            picked_rx,
            playback: media::Playback {
                player: Player::new(),
                available: player::available(),
                files: HashMap::new(),
                wanted: HashMap::new(),
                loading: HashSet::new(),
                clips: HashMap::new(),
                clip_wanted: RefCell::new(Vec::new()),
            },
            rec: recording::Recording {
                voice: None,
                draft: None,
                round: None,
                round_tex: None,
                round_seen: 0,
                available: recorder::available(),
            },
            tray,
            tray_rx,
            quitting: false,
            tray_hint_shown: false,
            folder_edit: None,
            accounts_open: false,
            accounts_anchor: Rect::NOTHING,
            background: Vec::new(),
            demo: false,
            composer_h: COMPOSER_H,
            forum: forum::Forum::default(),
            comments: None,
            public_hits: Vec::new(),
            export_ask: None,
            exporting: HashSet::new(),
            settings_tab: SettingsTab::Main,
            cameras: None,
            sessions: Vec::new(),
            privacy: privacy::PrivacyPage {
                rules: HashMap::new(),
                exceptions: HashMap::new(),
                open: None,
                pick: None,
                global: None,
                sensitive: None,
            },
            two_step: password::TwoStep::default(),
            confirm_terminate: false,
            pass_fields: Default::default(),
            lock: login::Lock {
                on: !settings.passcode.is_empty(),
                input: String::new(),
                error: false,
                last_activity: 0.0,
            },
            pinned: chatview::Pinned {
                list: HashMap::new(),
                shown: HashMap::new(),
                open: false,
                hidden: HashSet::new(),
            },
            marks: HashMap::new(),
            mark_wanted: None,
            read_by: None,
            ads: Default::default(),
            notify: Default::default(),
            calendar: None,
            translate: None,
            translate_to: None,
            contact_form: None,
            blocked: None,
            poll_form: None,
            send_box: None,
            spoilers: spoiler::Spoilers::default(),
            edit_media: editmedia::EditMedia::default(),
            websites: websites::Websites::default(),
            web_apps: Vec::new(),
            web_app_button: String::new(),
            web_app_ask: None,
            suggest: Default::default(),
            info: None,
            profile: backend::Profile::default(),
            profile_edit: None,
            photo_tx,
            photo_rx,
            chat_edit: None,
            session_extras: false,
            birthdays: birthdays::Birthdays::default(),
            bot_start: HashMap::new(),
            opening: None,
            report: None,
            quoting: Default::default(),
            stories: stories::Stories::default(),
            saved: saved::Saved::default(),
            chat_photo_tx,
            chat_photo_rx,
            outbox: Vec::new(),
            next_local: 0,
            focus_search: false,
            pin_drag: None,
            folder_drag: None,
            new_chat_menu: None,
            new_chat: None,
            contacts: Vec::new(),
            contact_hits: Default::default(),
        }
    }


    fn send(&mut self, cmd: Cmd) {
        match &self.backend {
            Some(b) => {
                let _ = b.tx.send(cmd);
            }
            None => demo::handle(self, cmd),
        }
    }

    pub fn push_event(&mut self, ev: Ev) {
        if !matches!(ev, Ev::Image { .. } | Ev::Qr(_)) {
            self.busy = false;
        }
        if matches!(ev, Ev::NewMsg(_) | Ev::Edited(_) | Ev::Deleted { .. } | Ev::ReadInbox { .. } | Ev::ReadOutbox { .. } | Ev::Draft { .. } | Ev::Presence { .. }) {
            self.chats_dirty = true;
        }
        // A fresh history page replaces the list: queued bubbles are put back after it.
        let refreshed = match &ev {
            Ev::History { peer, .. } | Ev::TopicHistory { peer, .. } => Some(*peer),
            _ => None,
        };
        self.handle_event(ev);
        if let Some(peer) = refreshed {
            self.keep_outbox(peer);
        }
    }

    fn handle_event(&mut self, ev: Ev) {
        match ev {
            ev @ (Ev::Dialogs(..) | Ev::Folders(..) | Ev::PublicResults { .. } | Ev::ChatFound(..) | Ev::Joined(..) | Ev::ChatTitle { .. } | Ev::SearchResults { .. }) => self.list_event(ev),
            ev @ (Ev::Exported { .. } | Ev::ExportProgress { .. } | Ev::Scheduled { .. }) => self.dialogs_event(ev),
            ev @ (Ev::InlineResults { .. } | Ev::Stickers(..) | Ev::UploadProgress { .. } | Ev::UploadDone(..)) => self.composer_event(ev),
            ev @ (Ev::MediaReady { .. } | Ev::Listened { .. }) => self.media_event(ev),
            ev @ Ev::Comments { .. } => self.comments_event(ev),
            ev @ (Ev::Topics { .. } | Ev::TopicHistory { .. }) => self.forum_event(ev),
            ev @ (Ev::NeedPhone | Ev::NeedCode | Ev::NeedPassword(..) | Ev::Qr(..) | Ev::LoggedIn(..) | Ev::LoggedOut) => self.login_event(ev),
            Ev::ReportStep { peer, ids, step } => self.on_report_step(peer, ids, step),
            Ev::Downloaded(path) => {
                self.toast = Some((lf!("Збережено: {}", "Saved: {}", path.display()), 0.0, false));
                self.open_received(&path);
            }
            Ev::Birthdays(list) => self.birthdays.today = list,
            Ev::SavedDialogs(list) => self.saved.sources = list,
            Ev::Stories(list) => self.on_stories(list),
            Ev::PeerStories { peer, stories, max_read } => self.on_peer_stories(peer, stories, max_read),
            Ev::StoryVideo { peer, id, path } => self.on_story_video(peer, id, path),
            Ev::SavedHistory { source, msgs, older } => self.on_saved_history(source, msgs, older),
            Ev::SavedTags(tags) => self.on_saved_tags(tags),
            Ev::FolderTags(on) => self.folder_meta.tags = on,
            Ev::SuggestedFolders(list) => self.folder_meta.suggested = list,
            Ev::SavedTagsChanged => self.on_saved_tags_changed(),
            Ev::SavedTagged(msgs) => self.on_saved_tagged(msgs),
            Ev::BotStart { peer, param } => {
                self.bot_start.insert(peer, param);
            }
            Ev::History { peer, msgs, older } => {
                if older {
                    self.loading_older = false;
                    self.exhausted.insert(peer, msgs.is_empty());
                    let list = self.history.entry(peer).or_default();
                    let mut merged: Vec<Msg> =
                        msgs.into_iter().filter(|m| !list.iter().any(|x| x.id == m.id)).collect();
                    merged.append(list);
                    *list = merged;
                } else {
                    self.history.insert(peer, msgs);
                }
                self.unread_after_history(peer);
            }
            Ev::NewMsg(m) => self.on_new_message(m),
            Ev::MediaReplaced(m) => self.on_media_replaced(m),
            Ev::Edited(m) => {
                if let Some(x) = self.history.get_mut(&m.peer).and_then(|l| l.iter_mut().find(|x| x.id == m.id)) {
                    *x = m;
                }
            }
            Ev::Deleted { ids, peer } => {
                for (p, list) in self.history.iter_mut() {
                    if peer.is_none_or(|c| c == *p) {
                        list.retain(|m| !ids.contains(&m.id));
                    }
                }
            }
            Ev::Image { key, image } => {
                self.on_spoiler_image(key, &image);
                let tex = self.ctx.load_texture(format!("{key:?}"), image, TextureOptions::LINEAR);
                self.textures.insert(key, tex);
            }
            ev @ (Ev::Contacts(_) | Ev::ContactResults { .. } | Ev::ChatCreated(_) | Ev::ChannelUsernameCheck { .. }) => self.new_chat_event(ev),
            Ev::Error(e) => {
                if let Some(n) = &mut self.new_chat {
                    n.busy = false;
                }
                self.contact_failed();
                if matches!(self.screen, Screen::Login(Step::Connecting)) {
                    self.screen = Screen::Login(Step::Creds);
                }
                self.toast = Some((e, 0.0, true));
            }
            Ev::BotAnswer { text, alert } => self.toast = Some((text, 0.0, alert)),
            Ev::Reactions { peer, id, reactions, unread } => {
                let mut fresh = false;
                if let Some(m) = self.find_mut(peer, id) {
                    m.reactions = reactions;
                    fresh = unread && m.out && !m.reaction_unread;
                    m.reaction_unread = unread;
                }
                if fresh {
                    self.live_mark(peer, id, true);
                }
            }
            Ev::Typing { peer, who, active } => {
                if active {
                    let until = self.ctx.input(|i| i.time) + TYPING_TTL;
                    self.typing.insert(peer, (who, until));
                } else {
                    self.typing.remove(&peer);
                }
            }
            Ev::Presence { peer, presence } => {
                if let Some(c) = self.chat_mut(peer) {
                    c.presence = presence;
                }
            }
            Ev::Draft { peer, text } => {
                // Don't touch the open chat's draft, so as not to interrupt typing.
                if self.selected != Some(peer)
                    && let Some(c) = self.chat_mut(peer)
                {
                    c.draft = text;
                }
            }
            Ev::ReadInbox { peer, unread } => {
                if let Some(c) = self.chat_mut(peer) {
                    c.unread = unread;
                    c.unread_mark &= unread > 0;
                }
            }
            Ev::ReadOutbox { peer, max_id } => {
                if let Some(c) = self.chat_mut(peer) {
                    c.read_out = c.read_out.max(max_id);
                }
            }
            Ev::Sessions(list) => self.sessions = list,
            Ev::GlobalPrivacy(p) => self.privacy.global = Some(p),
            Ev::Websites(list) => self.on_websites(list),
            Ev::ContentSettings { enabled, can_change } => self.privacy.sensitive = Some((enabled, can_change)),
            Ev::Password(state) => self.on_password(state),
            Ev::PasswordChecked { email } => self.on_password_checked(email),
            Ev::PasswordFailed(e) => self.on_password_failed(e),
            Ev::PrivacyExceptions(key, ex) => {
                self.privacy.exceptions.insert(key, ex);
            }
            Ev::Privacy(key, state) => {
                self.privacy.rules.insert(key, state);
            }
            ev @ (Ev::WebPreview { .. } | Ev::Mentions { .. } | Ev::BotCommands { .. } | Ev::EmojiKeywords { .. }) => self.suggest_event(ev),
            Ev::Notice(text) => self.toast = Some((text, 0.0, false)),
            ev @ (Ev::Pinned { .. } | Ev::PinnedChanged { .. } | Ev::UnreadMarks { .. } | Ev::ReadBy { .. } | Ev::Views { .. }) => self.chat_view_event(ev),
            ev @ (Ev::Sponsored { .. } | Ev::SponsoredReport { .. }) => self.sponsored_event(ev),
            Ev::DateFound { peer, id } => self.date_found(peer, id),
            Ev::MediaDays { peer, days } => self.media_days(peer, days),
            Ev::Translated { peer, id, to, result } => self.translated(peer, id, to, result),
            ev @ (Ev::WebAppOpened(_) | Ev::BotMenuApp { .. } | Ev::WebAppMethodResult { .. } | Ev::BotWriteAccess { .. } | Ev::PhoneShared { .. }) => self.web_app_event(ev),
            ev @ (Ev::ContactSaved(_) | Ev::ContactRemoved(_) | Ev::Blocked { .. } | Ev::BlockedList(_)) => self.contacts_event(ev),
            ev @ (Ev::NotifyDefault { .. } | Ev::NotifyExceptions { .. }) => self.notify_prefs_event(ev),
            ev @ (Ev::ChatDetails { .. } | Ev::AvatarVideo { .. } | Ev::SimilarChannels { .. } | Ev::Gifts { .. } | Ev::Members { .. } | Ev::Invite { .. } | Ev::Shared { .. } | Ev::Notify { .. } | Ev::ChatRemoved(_) | Ev::HistoryCleared(_)) => {
                self.info_event(ev)
            }
            Ev::Profile(p) => self.on_profile(p),
            Ev::UsernameCheck { name, state } => self.on_username_check(name, state),
            Ev::Outbox(list) => self.restore_outbox(list),
            Ev::QueueSent { key, peer } => self.queue_sent(key, peer),
            Ev::QueueFailed { key, error, retry } => self.queue_failed(key, error, retry),
        }
    }

    fn on_new_message(&mut self, m: Msg) {
        // New comment in the open discussion: a reply to the root or to another comment.
        if let Some(c) = self.comments.as_mut()
            && let Some((disc, root)) = c.discussion
            && m.peer == disc
            && m.reply_to.is_some_and(|r| r == root || c.msgs.iter().any(|x| x.id == r))
            && !c.msgs.iter().any(|x| x.id == m.id)
        {
            c.msgs.push(m.clone());
            if let Some(post) = self.history.get_mut(&c.peer).and_then(|l| l.iter_mut().find(|x| x.id == c.post)) {
                post.comments = Some(post.comments.unwrap_or(0) + 1);
            }
        }
        let open = self.selected == Some(m.peer);
        let focused = self.ctx.input(|i| i.viewport().focused.unwrap_or(true));
        if self.settings.notifications
            && !m.out
            && (!open || !focused)
            && let Some(chat) = self.chats.iter().find(|c| c.id == m.peer && !c.muted)
        {
            // Under a local passcode, don't show who sent what.
            if self.lock.on {
                notify("Glassgram", l!("Нове повідомлення", "New message"));
            } else {
                let body = if chat.kind == ChatKind::Group && !m.sender.is_empty() { format!("{}: {}", m.sender, quote(&m)) } else { quote(&m) };
                notify(&chat.title, &body);
            }
        }
        if !m.out && m.mentioned && !self.history.get(&m.peer).is_some_and(|l| l.iter().any(|x| x.id == m.id)) {
            self.live_mark(m.peer, m.id, false);
        }
        if let Some(list) = self.history.get_mut(&m.peer)
            && !list.iter().any(|x| x.id == m.id)
        {
            list.push(m.clone());
            if open && !m.out && !self.scroll.at_bottom {
                self.scroll.unseen_below += 1;
            }
        }
        let label = match m.media.as_ref().map(|i| i.kind) {
            _ if m.photo => Some(l!("📷 Фото", "📷 Photo")),
            Some(MediaKind::Voice) => Some(l!("🎤 Голосове повідомлення", "🎤 Voice message")),
            Some(MediaKind::Round) => Some(l!("📹 Відеоповідомлення", "📹 Video message")),
            Some(MediaKind::Video) => Some(l!("🎬 Відео", "🎬 Video")),
            Some(MediaKind::Gif) => Some("GIF"),
            Some(MediaKind::Audio) => Some(l!("🎵 Аудіо", "🎵 Audio")),
            None if m.sticker.is_some() => Some(l!("Стікер", "Sticker")),
            None if m.file.is_some() => Some(l!("📎 Файл", "📎 File")),
            None => None,
        };
        let text = m.text.replace('\n', " ");
        let preview = match label {
            Some(l) if text.is_empty() => l.to_owned(),
            Some(l) => format!("{l} · {text}"),
            None => text,
        };
        match self.chats.iter_mut().find(|c| c.id == m.peer) {
            Some(chat) => {
                chat.last_text = if m.out { lf!("Ви: {preview}", "You: {preview}") } else { preview };
                chat.last_time = m.time.clone();
                chat.last_ts = m.ts;
                if !open && !m.out {
                    chat.unread += 1;
                }
                // As in Telegram: a new message brings chats with unmuted notifications back from the archive.
                if chat.archived && !chat.muted && !m.out {
                    chat.archived = false;
                }
                sort_chats(&mut self.chats);
            }
            None => self.send(Cmd::ReloadDialogs),
        }
    }

    fn open_chat(&mut self, id: PeerId) {
        if self.selected == Some(id) {
            return;
        }
        self.leave_chat();
        self.selected = Some(id);
        self.compose = Compose::New;
        self.chat_search = None;
        self.picked.clear();
        self.schedule = None;
        self.scheduled_open = false;
        self.inline = None;
        self.suggest_reset();
        self.send(Cmd::LoadScheduled(id));
        self.unread_on_open(id);
        if let Some(c) = self.chat_mut(id) {
            c.unread = 0;
            c.unread_mark = false;
        }
        self.draft = self.chat_mut(id).map(|c| c.draft.clone()).unwrap_or_default();
        self.draft_synced = self.draft.clone();
        self.send(Cmd::OpenChat(id));
        self.open_chat_view(id);
        self.forum.open = None;
        self.saved_reset(id);
        if self.chats.iter().any(|c| c.id == id && c.forum) {
            self.send(Cmd::LoadTopics(id));
        }
    }

    /// Before switching chats: restore the set-aside draft and save it.
    fn leave_chat(&mut self) {
        if matches!(self.compose, Compose::Edit(_)) {
            self.draft = std::mem::take(&mut self.edit_stash);
        }
        self.compose = Compose::New;
        self.flush_draft();
    }

    fn flush_draft(&mut self) {
        let Some(peer) = self.selected else { return };
        if matches!(self.compose, Compose::Edit(_)) || self.draft == self.draft_synced {
            return;
        }
        let text = self.draft.clone();
        self.draft_synced = text.clone();
        if let Some(c) = self.chat_mut(peer) {
            c.draft = text.clone();
        }
        self.send(Cmd::SaveDraft { peer, text });
    }

    fn chat_mut(&mut self, id: PeerId) -> Option<&mut ChatInfo> {
        self.chats.iter_mut().find(|c| c.id == id)
    }

    /// Open a chat and scroll to a message (from search).
    fn jump_to(&mut self, hit: &SearchHit) {
        let (peer, id) = (hit.msg.peer, hit.msg.id);
        if !self.chats.iter().any(|c| c.id == peer) {
            self.chats.push(ChatInfo {
                id: peer,
                title: hit.chat.clone(),
                kind: hit.kind,
                last_text: String::new(),
                last_time: String::new(),
                last_ts: 0,
                unread: 0,
                pinned: false,
                contact: false,
                muted: false,
                archived: false,
                unread_mark: false,
                draft: String::new(),
                presence: Default::default(),
                forum: false,
                joined: true,
                can_post: true,
                read_out: 0,
                read_in: 0,
                unread_mentions: 0,
                unread_reactions: 0,
                noforwards: false,
                can_pin: false,
                restricted: None,
            });
        }
        self.open_chat(peer);
        if self.chats.iter().any(|c| c.id == peer && c.forum) {
            let topic = hit.msg.topic_id();
            self.open_topic(peer, topic);
        }
        if self.find(peer, id).is_none() {
            self.send(Cmd::OpenAt { peer, id });
        }
        self.scroll.jump = Some((peer, id, self.ctx.input(|i| i.time)));
    }

    fn apply_chat(&mut self, peer: PeerId, action: ChatAction) {
        if matches!(action, ChatAction::Export) {
            self.export_ask = Some((peer, false));
            return;
        }
        let Some(c) = self.chat_mut(peer) else { return };
        let cmd = match action {
            ChatAction::Export => return,
            ChatAction::Pin(pinned) => {
                c.pinned = pinned;
                // A newly pinned chat goes on top of the pinned ones, as in Telegram.
                if pinned && let Some(i) = self.chats.iter().position(|c| c.id == peer) {
                    let c = self.chats.remove(i);
                    self.chats.insert(0, c);
                }
                Cmd::SetPinned { peer, pinned }
            }
            ChatAction::Mute(muted) => {
                c.muted = muted;
                Cmd::SetMuted { peer, muted }
            }
            ChatAction::Read => {
                c.unread = 0;
                c.unread_mark = false;
                Cmd::MarkRead(peer)
            }
            ChatAction::Unread => {
                c.unread_mark = true;
                Cmd::MarkUnread(peer)
            }
            ChatAction::Archive(archived) => {
                c.archived = archived;
                if archived {
                    c.pinned = false;
                }
                Cmd::SetArchived { peer, archived }
            }
        };
        sort_chats(&mut self.chats);
        self.send(cmd);
    }

    fn apply(&mut self, peer: PeerId, action: Action) {
        let Some(action) = self.chat_view_action(peer, action) else { return };
        match action {
            Action::Reply(id) => {
                self.compose = Compose::Reply(id);
                self.quoting.clear();
            }
            Action::Edit(id) => {
                if let Some(m) = self.find(peer, id) {
                    let markup = format::to_markup(&m.text, &m.spans);
                    if !matches!(self.compose, Compose::Edit(_)) {
                        self.edit_stash = std::mem::replace(&mut self.draft, markup);
                    } else {
                        self.draft = markup;
                    }
                    self.compose = Compose::Edit(id);
                    self.clear_replacement();
                }
            }
            Action::Copy(text) => self.ctx.copy_text(text),
            Action::Play(id) => self.play(peer, id),
            Action::Seek(id, frac) => {
                if self.playback.player.is(peer, id) {
                    self.playback.player.seek(frac);
                }
            }
            Action::Speed => self.playback.player.cycle_speed(),
            Action::Comments(id) => {
                self.comments = Some(CommentsView { peer, post: id, discussion: None, msgs: Vec::new(), draft: String::new(), loading: true, exhausted: false });
                self.send(Cmd::OpenComments { peer, id, before: None });
            }
            Action::OpenExternal(id) => match self.playback.files.get(&(peer, id)).cloned() {
                Some(path) => self.open_received(&path),
                None => {
                    if self.playback.wanted.insert((peer, id), true).is_none() {
                        self.send(Cmd::LoadMedia { peer, id });
                    }
                }
            },
            Action::Copy2(text, note) => {
                self.ctx.copy_text(text);
                self.toast = Some((note.into(), 0.0, false));
            }
            Action::TogglePoll(id, opt) => {
                let v = self.multi_vote.entry((peer, id)).or_default();
                match v.iter().position(|o| *o == opt) {
                    Some(i) => {
                        v.remove(i);
                    }
                    None => v.push(opt),
                }
            }
            Action::Delete(id) => self.send(Cmd::Delete { peer, ids: vec![id] }),
            Action::Download(id) => self.send(Cmd::Download { peer, id }),
            Action::RevealSpoiler(id) => {
                self.revealed.insert((peer, id));
            }
            Action::Retry(id) => self.retry_queued(id),
            Action::Discard(id) => self.discard_queued(id),
            // Queued bubbles have no server id yet — nothing to select.
            Action::Pick(id) if id < 0 => {}
            Action::Pick(id) => match self.picked.iter().position(|x| *x == id) {
                Some(i) => {
                    self.picked.remove(i);
                }
                None => self.picked.push(id),
            },
            Action::Forward(id) => self.forward = Some(ForwardPicker { from: peer, ids: vec![id], query: String::new(), reply: false }),
            Action::ForwardMany(ids) => self.forward = Some(ForwardPicker { from: peer, ids, query: String::new(), reply: false }),
            Action::DeleteMany(ids) => self.send(Cmd::Delete { peer, ids }),
            Action::PickMany(ids) => {
                let all = ids.iter().all(|id| self.picked.contains(id));
                self.picked.retain(|id| !ids.contains(id));
                if !all {
                    self.picked.extend(ids);
                }
            }
            Action::View(id) => self.open_viewer(peer, id),
            Action::OpenMap(lat, lon) => {
                let url = format!("https://www.openstreetmap.org/?mlat={lat}&mlon={lon}#map=16/{lat}/{lon}");
                self.open_link(&url);
            }
            Action::React(id, emoji) => {
                let Some(m) = self.find_mut(peer, id) else { return };
                let was_mine = m.reactions.iter().any(|r| r.mine && r.emoji == emoji);
                // Without Premium, one reaction per message: remove the previous one.
                for r in &mut m.reactions {
                    if r.mine {
                        r.mine = false;
                        r.count -= 1;
                    }
                }
                if !was_mine {
                    match m.reactions.iter_mut().find(|r| r.emoji == emoji) {
                        Some(r) => {
                            r.mine = true;
                            r.count += 1;
                        }
                        None => m.reactions.push(Reaction { emoji: emoji.clone(), count: 1, mine: true }),
                    }
                }
                m.reactions.retain(|r| r.count > 0);
                self.send(Cmd::React { peer, id, emoji: (!was_mine).then_some(emoji) });
            }
            Action::Vote(id, options) => {
                self.multi_vote.remove(&(peer, id));
                if let Some(poll) = self.find_mut(peer, id).and_then(|m| m.poll.as_mut()) {
                    for a in &mut poll.answers {
                        if options.contains(&a.option) {
                            a.chosen = true;
                            a.voters += 1;
                        }
                    }
                    poll.total += 1;
                }
                self.send(Cmd::Vote { peer, id, options });
            }
            Action::Button(id, row, col) => {
                let Some(b) = self.find(peer, id).and_then(|m| m.buttons.get(row)?.get(col)).cloned() else { return };
                match b.action {
                    ButtonAction::Callback(data) => self.send(Cmd::PressButton { peer, id, data }),
                    ButtonAction::Url(url) => self.open_link(&url),
                    ButtonAction::Text => {
                        let reply_to = self.forum.open.filter(|t| *t != GENERAL_TOPIC).map(ReplyTo::to);
                        self.send(Cmd::Send { peer, text: b.text, reply_to, opts: SendOpts::default() });
                    }
                    ButtonAction::Copy(text) => {
                        self.ctx.copy_text(text);
                        self.toast = Some((l!("Скопійовано", "Copied").into(), 0.0, false));
                    }
                    ButtonAction::WebApp { url, simple, bot } => {
                        // Inline buttons in groups belong to the bot that sent the message.
                        let bot = PeerId::user(bot).filter(|_| bot != 0).unwrap_or(peer);
                        let kind = if simple { backend::WebAppKind::Simple(url) } else { backend::WebAppKind::Inline(url) };
                        self.open_web_app(peer, bot, kind, b.text);
                    }
                    ButtonAction::Other => self.toast = Some((l!("Цей тип кнопки ще не підтримується", "This button type is not supported yet").into(), 0.0, true)),
                }
            }
            Action::OpenHidden(url) => self.opening = Some(confirm::Opening::Link(url)),
            Action::Open(target) if target.starts_with("tg://user?id=") => {
                // A mention by id opens the chat with that person if we have it.
                let id = target["tg://user?id=".len()..].parse::<i64>().ok().map(PeerId::user_unchecked);
                match id.filter(|p| self.chats.iter().any(|c| c.id == *p)) {
                    Some(p) => self.open_chat(p),
                    None => self.toast = Some((l!("Цього користувача немає у ваших чатах", "This user is not in your chats").into(), 0.0, false)),
                }
            }
            Action::Open(target) => match target.chars().next() {
                // A hashtag is searched in this chat, a bot command is sent.
                Some('#' | '$') => {
                    let s = self.chat_search.get_or_insert_with(Search::default);
                    s.query = target;
                    s.edited_at = 0.0;
                }
                Some('/') => self.send(Cmd::Send { peer, text: target, reply_to: self.reply_target(), opts: SendOpts::default() }),
                // A mention opens the chat in the client (a public one without joining).
                Some('@') if !self.demo => self.send(Cmd::ResolveUsername(target[1..].to_owned())),
                Some('@') => self.open_link(&format!("https://t.me/{}", &target[1..])),
                _ if botstart::tme_bot_start(&target).is_some() => {
                    if let Some((username, param)) = botstart::tme_bot_start(&target) {
                        self.open_bot_start(username, param);
                    }
                }
                _ if tme_web_app(&target).is_some() => {
                    if let Some((username, app, start_param)) = tme_web_app(&target) {
                        self.open_web_app_link(username, app, start_param);
                    }
                }
                _ if !self.demo && tme_username(&target).is_some() => {
                    let name = tme_username(&target).unwrap_or_default();
                    self.send(Cmd::ResolveUsername(name));
                }
                _ => {
                    let url = if target.contains("://") || target.starts_with("mailto:") { target } else { format!("https://{target}") };
                    self.open_link(&url);
                }
            },
            // Handled by `chat_view_action`.
            Action::Pin(..) | Action::Unpin(_) | Action::ReadBy(_) | Action::Translate(_) | Action::Report(_) => {}
        }
    }

    /// Every link from a message, button or profile goes through here: unsafe schemes are refused.
    pub(super) fn open_link(&mut self, url: &str) {
        // Bare "example.com" or "t.me/x" from text is a web link.
        let full = if url.contains("://") || url.starts_with("mailto:") { url.to_owned() } else { format!("https://{url}") };
        if !crate::platform::open_url(&full) {
            self.toast = Some((lf!("Посилання не відкрито: непідтримуваний протокол ({url})", "Link not opened: unsupported protocol ({url})"), 0.0, true));
        }
    }

    /// For demo search: (chat, (name, type), message).
    pub fn demo_history(&self) -> impl Iterator<Item = (PeerId, (&str, ChatKind), &Vec<Msg>)> {
        self.history.iter().filter_map(|(p, msgs)| {
            let c = self.chats.iter().find(|c| c.id == *p)?;
            Some((*p, (c.title.as_str(), c.kind), msgs))
        })
    }

    pub fn demo_scheduled(&self, peer: PeerId) -> Vec<Msg> {
        self.scheduled.get(&peer).cloned().unwrap_or_default()
    }

    fn find_mut(&mut self, peer: PeerId, id: i32) -> Option<&mut Msg> {
        self.history.get_mut(&peer)?.iter_mut().find(|m| m.id == id)
    }

    fn find(&self, peer: PeerId, id: i32) -> Option<&Msg> {
        self.history.get(&peer)?.iter().find(|m| m.id == id)
    }

    // ───────────────────────────── Sign-in ─────────────────────────────

    // ───────────────────────────── Main screen ─────────────────────────────

    fn main_view(&mut self, ui: &mut Ui) {
        let mut full = ui.max_rect().shrink(GAP);
        if self.settings.folders_side && self.has_folder_bar() {
            let column = Rect::from_min_size(full.min, vec2(chatlist::COLUMN_W, full.height()));
            self.folder_column(ui, column);
            full.min.x = column.right() + GAP;
        }
        let side_w = (full.width() * 0.34).clamp(250.0, 370.0);
        let side = Rect::from_min_size(full.min, vec2(side_w, full.height()));
        let mut chat = Rect::from_min_max(pos2(side.right() + GAP, full.top()), full.max);
        self.sidebar(ui, side);
        // Info column: beside the chat when there is room, otherwise over its right edge.
        self.sync_info();
        let info = self.info.is_some().then(|| {
            let r = Rect::from_min_max(pos2(chat.right() - info::INFO_W.min(chat.width()), chat.top()), chat.max);
            if chat.width() - info::INFO_W - GAP >= info::INFO_MIN_CHAT {
                chat.max.x = r.left() - GAP;
            }
            r
        });
        self.chat_area(ui, chat);
        if let Some(r) = info {
            self.info_view(ui, r);
        }
    }

    /// Unread counter in the window title (like a taskbar badge).
    fn update_title(&mut self, ui: &Ui) {
        let unread: usize = self.chats.iter().filter(|c| !c.muted && !c.archived).map(|c| c.unread.max(c.unread_mark as i32) as usize).sum();
        if unread != self.title_unread {
            self.title_unread = unread;
            let title = if unread > 0 { format!("Glassgram ({unread})") } else { "Glassgram".to_owned() };
            ui.ctx().send_viewport_cmd(egui::ViewportCommand::Title(title));
            if let Some(t) = &self.tray {
                t.set_unread(unread);
            }
        }
    }

    /// With a tray (Windows), closing the window hides it there, as Telegram Desktop does.
    fn close_to_tray(&mut self, ctx: &egui::Context, frame: &eframe::Frame) {
        let Some(t) = &self.tray else { return };
        t.attach(frame);
        if !self.quitting && ctx.input(|i| i.viewport().close_requested()) && t.hide_window() {
            ctx.send_viewport_cmd(egui::ViewportCommand::CancelClose);
            if !std::mem::replace(&mut self.tray_hint_shown, true) {
                notify("Glassgram", l!("Glassgram працює далі в треї. Вийти — через меню іконки.", "Glassgram keeps running in the tray. Quit from the icon's menu."));
            }
        }
    }

    fn tray_events(&mut self) {
        while let Ok(ev) = self.tray_rx.try_recv() {
            match ev {
                TrayEv::Show => {
                    self.ctx.send_viewport_cmd(egui::ViewportCommand::Minimized(false));
                    self.ctx.send_viewport_cmd(egui::ViewportCommand::Focus);
                }
                TrayEv::ToggleNotifications => {
                    self.settings.notifications = !self.settings.notifications;
                    self.settings.save();
                    if let Some(t) = &self.tray {
                        t.set_notifications(self.settings.notifications);
                    }
                }
                TrayEv::Quit => {
                    self.quitting = true;
                    self.ctx.send_viewport_cmd(egui::ViewportCommand::Close);
                }
            }
        }
    }

    fn toast_view(&mut self, ui: &mut Ui, now: f64) {
        let Some((text, start, error)) = &mut self.toast else { return };
        let tint = if *error { Color32::from_rgb(230, 60, 90) } else { Color32::from_rgb(40, 170, 120) };
        if *start == 0.0 {
            *start = now;
        }
        let age = (now - *start) as f32;
        if age > 5.0 {
            self.toast = None;
            return;
        }
        let alpha = (age * 4.0).min(1.0).min((5.0 - age) * 2.0);
        let p = ui.painter();
        let galley = p.layout(text.clone(), FontId::proportional(14.0), ink(), 420.0);
        let size = galley.size() + vec2(36.0, 22.0);
        let screen = ui.max_rect();
        let rect = Rect::from_center_size(pos2(screen.center().x, screen.top() + 24.0 + size.y / 2.0 - (1.0 - alpha) * 20.0), size);
        glass::glass(p, rect, Glass::tinted(size.y.min(44.0) / 2.0, tint, (170.0 * alpha) as u8).with_shadow().frosted(alpha));
        p.galley(rect.center() - galley.size() / 2.0, galley, ink());
    }
}

impl eframe::App for App {
    fn on_exit(&mut self) {
        self.close_web_apps();
        self.save_chats();
        self.send(Cmd::Shutdown);
        self.stop_background();
        // Wait for the backend to finish writing the cache, but don't hold the window long.
        if let Some(b) = &self.backend {
            let _ = b.done.recv_timeout(std::time::Duration::from_secs(2));
        }
    }

    fn ui(&mut self, ui: &mut Ui, frame: &mut eframe::Frame) {
        self.close_to_tray(ui.ctx(), frame);
        let reload = self.textures.frame(ui.input(|i| i.time));
        if !reload.is_empty() {
            self.send(Cmd::ReloadImages(reload));
        }
        let events: Vec<Ev> = self.backend.as_ref().map(|b| b.rx.try_iter().collect()).unwrap_or_default();
        let account = self.accounts.active;
        self.poll_web_apps();
        self.poll_background();
        for ev in events {
            // After an account switch, the remaining events come from the old backend.
            if self.accounts.active != account {
                break;
            }
            self.push_event(ev);
        }
        while let Ok((peer, paths, as_file)) = self.picked_rx.try_recv() {
            self.send_files(peer, paths, as_file);
        }
        self.poll_player();
        self.tray_events();
        self.take_profile_photos();
        self.take_chat_photos();
        let light = match self.settings.theme {
            settings::Theme::Light => true,
            settings::Theme::Dark => false,
            settings::Theme::System => ui.ctx().system_theme() == Some(egui::Theme::Light),
        };
        glass::set_msg_font(self.settings.text_size);
        if light != glass::is_light() {
            apply_theme(ui.ctx(), light);
        }

        let now = ui.input(|i| i.time);
        self.pump_outbox(now);
        if self.chats_dirty && now - self.chats_saved_at > CHATS_SAVE_EVERY {
            self.chats_saved_at = now;
            self.save_chats();
        }
        glass::background(ui.painter(), ui.max_rect(), now);
        ui.ctx().request_repaint_after(Duration::from_millis(33));

        // Auto-lock after inactivity.
        if ui.input(|i| !i.events.is_empty() || i.pointer.is_moving()) {
            self.lock.last_activity = now;
        }
        let idle = self.settings.lock_minutes as f64 * 60.0;
        if !self.settings.passcode.is_empty() && idle > 0.0 && now - self.lock.last_activity > idle {
            self.lock.on = true;
        }
        if self.lock.on && !self.settings.passcode.is_empty() && matches!(self.screen, Screen::Main) {
            self.viewer = None;
            self.lock_view(ui);
            return;
        }

        self.hotkeys(ui.ctx());
        match &self.screen {
            Screen::Login(step) => {
                let step = step.clone();
                self.login_view(ui, step);
            }
            Screen::Main => {
                self.main_view(ui);
                self.drop_overlay(ui);
                self.update_clips(now);
            }
        }
        if self.viewer.is_some() {
            self.viewer_view(ui);
        }
        if self.stories.view.is_some() {
            self.story_viewer(ui);
        }
        self.new_chat_overlays(ui);
        if self.accounts_open {
            self.accounts_menu(ui);
        }
        if self.settings_open {
            self.settings_view(ui);
        }
        // Above the settings: folders are also edited from the "Folders" page.
        if self.folder_edit.is_some() {
            self.folder_editor(ui);
        }
        if self.export_ask.is_some() {
            self.export_view(ui);
        }
        self.info_confirm_view(ui);
        self.opening_view(ui);
        self.web_app_ask_view(ui);
        self.contact_form_view(ui);
        self.poll_form_view(ui);
        self.send_box_view(ui);
        self.chat_edit_view(ui);
        self.privacy_pick_view(ui);
        self.tag_rename_view(ui);
        self.toast_view(ui, now);
        self.update_title(ui);
    }
}

fn author(m: &Msg, me: &str) -> String {
    if m.out {
        if me.is_empty() { l!("Ви", "You").to_owned() } else { me.to_owned() }
    } else if m.sender.is_empty() {
        l!("Повідомлення", "Message").to_owned()
    } else {
        m.sender.clone()
    }
}

fn quote(m: &Msg) -> String {
    match (m.photo, m.text.is_empty()) {
        (true, true) => l!("📷 Фото", "📷 Photo").to_owned(),
        (true, false) => format!("📷 {}", m.text.replace('\n', " ")),
        _ => m.text.replace('\n', " "),
    }
}

fn kind_icon(kind: ChatKind) -> Option<Icon> {
    match kind {
        ChatKind::Saved => Some(Icon::Bookmark),
        _ => None,
    }
}

fn elided(ui: &Ui, text: &str, size: f32, color: Color32, width: f32) -> std::sync::Arc<egui::Galley> {
    let mut job = LayoutJob::single_section(text.to_owned(), egui::TextFormat::simple(FontId::proportional(size), color));
    job.wrap = TextWrapping::truncate_at_width(width.max(10.0));
    ui.painter().layout_job(job)
}

fn chat_row(
    ui: &mut Ui,
    chat: &ChatInfo,
    selected: bool,
    avatar: Option<&TextureHandle>,
    typing: Option<&str>,
    draft: Option<&str>,
    tags: &[(String, i32)],
    density: settings::Density,
) -> egui::Response {
    let w = ui.available_width();
    let (rect, resp) = ui.allocate_exact_size(vec2(w, density.row_h()), Sense::click());
    let (above, below) = density.lines();
    let mid = rect.center().y;
    let hover = ui.ctx().animate_bool(resp.id, resp.hovered());
    let sel = ui.ctx().animate_bool(resp.id.with("sel"), selected);
    let p = ui.painter();
    if sel > 0.0 {
        glass::glass(p, rect, Glass::tinted(20.0, ACCENT, (sel * 110.0) as u8));
    } else if hover > 0.0 {
        p.rect_filled(rect, 20.0, glass::veil((hover * 14.0) as u8));
    }
    let seed = chat.id.bot_api_dialog_id_unchecked();
    let r = density.avatar_r();
    let center = pos2(rect.left() + 10.0 + r, mid);
    glass::avatar(p, center, r, &chat.title, seed, kind_icon(chat.kind), avatar);
    if chat.presence.online() {
        let dot = center + egui::Vec2::splat(r * 0.7);
        p.circle_filled(dot, 6.5, glass::chrome());
        p.circle_filled(dot, 4.5, Color32::from_rgb(70, 220, 120));
    }

    let x = rect.left() + 20.0 + r * 2.0;
    let time = elided(ui, &chat.last_time, 12.0, ink_dim(), 80.0);
    let time_w = time.size().x;
    p.galley(pos2(rect.right() - 12.0 - time_w, mid - above + 2.0), time, ink_dim());

    let title_icon = match chat.kind {
        ChatKind::Channel => Some(Icon::Channel),
        ChatKind::Bot => Some(Icon::Bot),
        _ => None,
    };
    let title_y = mid - above + 9.0;
    let mut tx = x;
    if let Some(icon) = title_icon {
        icons::draw(p, Rect::from_center_size(pos2(tx + 7.0, title_y), vec2(15.0, 15.0)), icon, ink_dim());
        tx += 19.0;
    }
    let mute_w = if chat.muted { 18.0 } else { 0.0 };
    let title = elided(ui, &chat.title, 15.0, ink(), rect.right() - tx - time_w - 22.0 - mute_w);
    let title_w = title.size().x;
    p.galley(pos2(tx, mid - above), title, ink());
    if chat.muted {
        icons::draw(p, Rect::from_center_size(pos2(tx + title_w + 11.0, title_y), vec2(13.0, 13.0)), Icon::Muted, ink_dim());
    }

    let badge_w = if chat.unread > 0 || chat.unread_mark {
        let label = match chat.unread {
            0 => String::new(),
            n if n > 999 => "999+".to_owned(),
            n => n.to_string(),
        };
        let g = elided(ui, &label, 12.0, Color32::WHITE, 60.0);
        let bw = (g.size().x + 14.0).max(22.0);
        let b = Rect::from_min_size(pos2(rect.right() - 12.0 - bw, mid + below - 2.0), vec2(bw, 22.0));
        // As in Telegram: muted chats have a grey counter.
        let (tint, alpha) = if chat.muted { (Color32::from_rgb(150, 158, 190), 120) } else { (ACCENT, 230) };
        glass::glass(p, b, Glass::tinted(11.0, tint, alpha));
        p.galley(b.center() - g.size() / 2.0, g, Color32::WHITE);
        bw + 8.0
    } else if chat.pinned {
        icons::draw(p, Rect::from_center_size(pos2(rect.right() - 20.0, mid + below + 9.0), vec2(14.0, 14.0)), Icon::Pin, ink_dim());
        22.0
    } else {
        0.0
    };
    // Folder tags sit before the badge and take at most a third of the line.
    let tags_w = if tags.is_empty() { 0.0 } else { folders::draw_tags(ui, tags, rect.right() - 12.0 - badge_w, mid + below + 9.0, (rect.width() - 68.0) / 3.0) + 4.0 };
    let preview_w = rect.right() - x - badge_w - tags_w - 14.0;
    let preview_pos = pos2(x, mid + below);
    match (typing, draft) {
        (Some(who), _) => {
            let text = if who.is_empty() || matches!(chat.kind, ChatKind::User | ChatKind::Bot) { l!("друкує…", "typing…").to_owned() } else { lf!("{who} друкує…", "{who} is typing…") };
            p.galley(preview_pos, elided(ui, &text, 13.5, ACCENT, preview_w), ACCENT);
        }
        (None, Some(draft)) => {
            let red = Color32::from_rgb(255, 110, 130);
            let label = elided(ui, l!("Чернетка: ", "Draft: "), 13.5, red, preview_w);
            let lw = label.size().x;
            p.galley(preview_pos, label, red);
            let text = draft.replace('\n', " ");
            p.galley(preview_pos + vec2(lw, 0.0), elided(ui, &text, 13.5, ink_dim(), preview_w - lw), ink_dim());
        }
        (None, None) => {
            p.galley(preview_pos, elided(ui, &chat.last_text, 13.5, ink_dim(), preview_w), ink_dim());
        }
    }
    if resp.hovered() {
        ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
    }
    resp
}

/// Chat context menu in the list.
fn chat_menu(ui: &mut Ui, chat: &ChatInfo) -> Option<ChatAction> {
    ui.set_min_width(210.0);
    let mut action = None;
    let mut item = |ui: &mut Ui, icon: Icon, label: &str, a: ChatAction| {
        if glass::menu_button(ui, icon, label).clicked() {
            action = Some(a);
        }
    };
    if !chat.archived {
        let (icon, label) = if chat.pinned { (Icon::PinOff, l!("Відкріпити", "Unpin")) } else { (Icon::Pin, l!("Закріпити", "Pin")) };
        item(ui, icon, label, ChatAction::Pin(!chat.pinned));
    }
    if chat.kind != ChatKind::Saved {
        let (icon, label) = if chat.muted { (Icon::Bell, l!("Увімкнути сповіщення", "Unmute")) } else { (Icon::BellOff, l!("Вимкнути сповіщення", "Mute")) };
        item(ui, icon, label, ChatAction::Mute(!chat.muted));
    }
    if chat.is_unread() {
        item(ui, Icon::CheckCheck, l!("Позначити прочитаним", "Mark as read"), ChatAction::Read);
    } else {
        item(ui, Icon::MarkUnread, l!("Позначити непрочитаним", "Mark as unread"), ChatAction::Unread);
    }
    if chat.kind != ChatKind::Saved {
        let (icon, label) = if chat.archived { (Icon::Unarchive, l!("Повернути з архіву", "Unarchive")) } else { (Icon::Archive, l!("Архівувати", "Archive")) };
        item(ui, icon, label, ChatAction::Archive(!chat.archived));
    }
    ui.separator();
    item(ui, Icon::Export, l!("Експортувати історію…", "Export chat history…"), ChatAction::Export);
    action
}

fn section_title(ui: &mut Ui, title: &str) {
    let w = ui.available_width();
    let (r, _) = ui.allocate_exact_size(vec2(w, 30.0), Sense::hover());
    ui.painter().text(pos2(r.left() + 12.0, r.bottom() - 6.0), Align2::LEFT_BOTTOM, title, FontId::proportional(13.0), ink_dim());
}

/// Search result row: author/chat, time and a text snippet around the match.
fn hit_row(ui: &mut Ui, hit: &SearchHit, query: &str, avatar: Option<&TextureHandle>) -> egui::Response {
    let w = ui.available_width();
    let (rect, resp) = ui.allocate_exact_size(vec2(w, 56.0), Sense::click());
    let hover = ui.ctx().animate_bool(resp.id, resp.hovered());
    let p = ui.painter();
    if hover > 0.0 {
        p.rect_filled(rect, 16.0, glass::veil((hover * 16.0) as u8));
    }
    let mut x = rect.left() + 12.0;
    if !hit.chat.is_empty() {
        let seed = hit.msg.peer.bot_api_dialog_id_unchecked();
        glass::avatar(p, pos2(rect.left() + 30.0, rect.center().y), 19.0, &hit.chat, seed, kind_icon(hit.kind), avatar);
        x = rect.left() + 58.0;
    }
    let time = elided(ui, &format!("{} {}", hit.msg.day, hit.msg.time), 11.5, ink_dim(), 140.0);
    let time_w = time.size().x;
    p.galley(pos2(rect.right() - 10.0 - time_w, rect.top() + 10.0), time, ink_dim());
    p.galley(pos2(x, rect.top() + 8.0), elided(ui, &hit.chat, 14.0, ink(), rect.right() - x - time_w - 18.0), ink());

    // Snippet with the match highlighted.
    let text = quote(&hit.msg);
    let lower = text.to_lowercase();
    let q = query.trim().to_lowercase();
    let mut job = LayoutJob { wrap: TextWrapping::truncate_at_width(rect.right() - x - 10.0), ..Default::default() };
    let fmt = |c| egui::TextFormat::simple(FontId::proportional(13.0), c);
    // Lowercasing may shift bytes, so highlight only when the lengths match.
    let found = (lower.len() == text.len()).then(|| lower.find(&q)).flatten().filter(|_| !q.is_empty());
    match found {
        Some(i) => {
            let start = text[..i].char_indices().rev().nth(24).map_or(0, |(j, _)| j);
            let lead = if start > 0 { "…" } else { "" };
            job.append(&format!("{lead}{}", &text[start..i]), 0.0, fmt(ink_dim()));
            job.append(&text[i..i + q.len()], 0.0, egui::TextFormat { background: rgba(ACCENT, 120), ..fmt(ink()) });
            job.append(&text[i + q.len()..], 0.0, fmt(ink_dim()));
        }
        None => job.append(&text, 0.0, fmt(ink_dim())),
    }
    p.galley(pos2(x, rect.top() + 30.0), ui.painter().layout_job(job), ink_dim());
    if resp.hovered() {
        ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
    }
    resp
}

/// Topic row: colored icon with the first letter, name, last message, unread count.
fn topic_row(ui: &mut Ui, t: &Topic) -> egui::Response {
    let w = ui.available_width();
    let (row, resp) = ui.allocate_exact_size(vec2(w, 62.0), Sense::click());
    let p = ui.painter();
    let hover = ui.ctx().animate_bool(resp.id, resp.hovered());
    p.rect_filled(row, 14.0, glass::veil((hover * 16.0) as u8));
    if resp.hovered() {
        ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
    }
    let c = pos2(row.left() + 30.0, row.center().y);
    let color = Color32::from_rgb((t.color >> 16) as u8, (t.color >> 8) as u8, t.color as u8);
    let color = if t.color == 0 || t.id == GENERAL_TOPIC { ACCENT } else { color };
    p.circle_filled(c, 19.0, color);
    let letter = if t.id == GENERAL_TOPIC { "#".to_owned() } else { t.title.chars().next().map(|ch| ch.to_uppercase().to_string()).unwrap_or_default() };
    p.text(c, Align2::CENTER_CENTER, letter, FontId::proportional(17.0), glass::ON_DARK);
    let x = row.left() + 60.0;
    let right = row.right() - 12.0;
    let time = p.layout_no_wrap(t.last_time.clone(), FontId::proportional(12.0), ink_dim());
    let mut title = t.title.clone();
    if t.closed {
        title.push_str(" 🔒");
    }
    let bold = FontId::new(15.0, egui::FontFamily::Name(format::BOLD.into()));
    let mut job = LayoutJob::single_section(title, egui::TextFormat::simple(bold, ink()));
    job.wrap = TextWrapping::truncate_at_width(right - x - time.size().x - 28.0);
    p.galley(pos2(x, row.top() + 10.0), p.layout_job(job), ink());
    if t.pinned {
        icons::draw(p, Rect::from_min_size(pos2(right - time.size().x - 22.0, row.top() + 11.0), vec2(14.0, 14.0)), Icon::Pin, ink_dim());
    }
    p.galley(pos2(right - time.size().x, row.top() + 12.0), time, ink_dim());
    let badge_w = if t.unread > 0 { 34.0 } else { 0.0 };
    p.galley(pos2(x, row.top() + 34.0), elided(ui, &t.last_text, 13.5, ink_dim(), right - x - badge_w), ink());
    if t.unread > 0 {
        let label = if t.unread > 999 { "999+".to_owned() } else { t.unread.to_string() };
        let g = p.layout_no_wrap(label, FontId::proportional(12.0), glass::ON_DARK);
        let b = Rect::from_min_size(pos2(right - g.size().x - 14.0, row.top() + 33.0), vec2(g.size().x + 14.0, 20.0));
        p.rect_filled(b, 10.0, ACCENT);
        p.galley(b.min + vec2(7.0, 2.0), g, glass::ON_DARK);
    }
    resp
}

const MEDIA_H: f32 = 48.0;
/// Player key for recording preview (real ids are positive).
const PREVIEW_ID: i32 = -1;

/// Room for ticks in the time label (≈14 pt at font size 11).
const TICKS_GAP: &str = "     ";

fn notify(title: &str, body: &str) {
    crate::platform::notify(title, body);
}

/// Saves a clipboard image as PNG in the cache to send as a photo.
fn save_clipboard_image(img: &arboard::ImageData<'_>) -> Option<PathBuf> {
    let buf = image::RgbaImage::from_raw(img.width as u32, img.height as u32, img.bytes.to_vec())?;
    let dir = backend::data_dir().join("cache");
    std::fs::create_dir_all(&dir).ok()?;
    let path = dir.join(format!("clipboard-{}.png", jiff::Timestamp::now().as_millisecond()));
    buf.save(&path).ok()?;
    Some(path)
}

fn setup_style(ctx: &egui::Context) {
    format::install_fonts(ctx);
    apply_theme(ctx, false);
}

/// Theme: glass colors (`glass::set_light`) and egui widget colors (menus, checkboxes, selection).
fn apply_theme(ctx: &egui::Context, light: bool) {
    glass::set_light(light);
    ctx.set_theme(if light { egui::Theme::Light } else { egui::Theme::Dark });
    ctx.set_visuals(if light { egui::Visuals::light() } else { egui::Visuals::dark() });
    ctx.all_styles_mut(|style| {
        let v = &mut style.visuals;
        v.override_text_color = Some(ink());
        v.panel_fill = Color32::TRANSPARENT;
        v.window_fill = if light { Color32::from_rgba_unmultiplied(250, 251, 255, 245) } else { Color32::from_rgba_unmultiplied(24, 28, 60, 235) };
        v.window_corner_radius = egui::CornerRadius::same(14);
        v.menu_corner_radius = egui::CornerRadius::same(14);
        v.extreme_bg_color = Color32::TRANSPARENT;
        v.selection.bg_fill = rgba(ACCENT, 140);
        v.selection.stroke.color = ink();
        v.text_cursor.stroke.color = ink();
        style.spacing.scroll = egui::style::ScrollStyle::floating();
        style.spacing.scroll.bar_width = 6.0;
    });
}

#[cfg(test)]
mod tests {
    use super::tme_web_app;

    #[test]
    fn parses_mini_app_links() {
        assert_eq!(tme_web_app("https://t.me/somebot/game?startapp=ref1"), Some(("somebot".into(), Some("game".into()), "ref1".into())));
        assert_eq!(tme_web_app("https://t.me/somebot?startapp"), Some(("somebot".into(), None, String::new())));
        assert_eq!(tme_web_app("https://t.me/somebot"), None);
        assert_eq!(tme_web_app("https://t.me/somechannel/123"), None);
        assert_eq!(tme_web_app("https://t.me/somechannel/news"), None);
    }
}
