//! Data the backend hands to the UI: chats, messages, media, folders, privacy, presence.

use super::*;

/// Own profile (the "My account" section).
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Profile {
    /// For the avatar (`ImgKey::Avatar`); `None` — not loaded yet.
    pub id: Option<PeerId>,
    pub first: String,
    pub last: String,
    pub bio: String,
    pub username: String,
    pub phone: String,
    pub birthday: Option<Birthday>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UsernameState {
    Free,
    Taken,
    Invalid,
}

/// The part of a message a reply quotes: plain text and where it starts in the message (UTF-16 units).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Quote {
    pub text: String,
    pub offset: i32,
    /// Formatting of `text`, cut out of the message's own.
    #[serde(default)]
    pub spans: Vec<format::Span>,
}

/// What a new message replies to: a message in this chat or in another one, maybe quoting a part of it.
#[derive(Debug, Clone, PartialEq)]
pub struct ReplyTo {
    pub id: i32,
    /// The chat of the replied message when it is not the chat we send to.
    pub peer: Option<PeerId>,
    pub quote: Option<Quote>,
}

impl ReplyTo {
    pub fn to(id: i32) -> Self {
        Self { id, peer: None, quote: None }
    }

    /// grammers can only reply to a message id in the same chat; the rest needs our own request.
    pub fn extended(&self) -> bool {
        self.peer.is_some() || self.quote.is_some()
    }

    /// The replied message id when it is in the same chat (bubbles look it up there).
    pub fn local(reply: &Option<ReplyTo>) -> Option<i32> {
        reply.as_ref().filter(|r| r.peer.is_none()).map(|r| r.id)
    }
}

/// A message in the send queue. `key` is the queue's own key: there is no server id yet.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Queued {
    pub key: i64,
    #[serde(with = "crate::cache::peer_id")]
    pub peer: PeerId,
    /// Text with markup, as typed in the composer.
    pub text: String,
    pub reply_to: Option<i32>,
    pub silent: bool,
    /// When it was queued (Unix time) — for the bubble timestamp.
    pub ts: i64,
    /// Final failure: after a restart we don't retry on our own, we wait for "Retry".
    pub failed: bool,
    /// Link preview turned off with ✕ in the composer.
    #[serde(default)]
    pub no_webpage: bool,
    /// The quoted part of the message replied to.
    #[serde(default)]
    pub quote: Option<Quote>,
    /// The chat of the replied message, when it is another one.
    #[serde(default, with = "crate::cache::peer_id_opt")]
    pub reply_peer: Option<PeerId>,
}

impl Queued {
    pub fn reply(&self) -> Option<ReplyTo> {
        self.reply_to.map(|id| ReplyTo { id, peer: self.reply_peer, quote: self.quote.clone() })
    }

    /// Bubble for a message the server doesn't have yet (`id` is local and negative).
    pub fn to_msg(&self, id: i32) -> Msg {
        let (text, ents) = format::parse(&self.text);
        let at = jiff::Timestamp::from_second(self.ts).unwrap_or_else(|_| jiff::Timestamp::now());
        Msg {
            id,
            out: true,
            spans: format::from_entities(&text, &ents, 0),
            text,
            time: clock(at),
            ts: self.ts,
            day: day(at),
            reply_to: self.reply_to,
            reply_quote: self.quote.as_ref().map(|q| q.text.clone()),
            reply_elsewhere: self.reply_peer.map(|_| String::new()),
            ..Msg::placeholder(self.peer)
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ChatKind {
    User,
    Bot,
    Group,
    Channel,
    Saved,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChatInfo {
    #[serde(with = "crate::cache::peer_id")]
    pub id: PeerId,
    pub title: String,
    pub kind: ChatKind,
    pub last_text: String,
    pub last_time: String,
    pub last_ts: i64,
    pub unread: i32,
    pub pinned: bool,
    pub contact: bool,
    pub muted: bool,
    pub archived: bool,
    /// Manually marked as unread.
    pub unread_mark: bool,
    /// Draft in the input field markup.
    pub draft: String,
    pub presence: Presence,
    /// Forum group: messages are split into topics.
    pub forum: bool,
    /// We are a member (not for found public chats until we join).
    pub joined: bool,
    /// Can write: in a channel, only the owner or an admin allowed to post.
    pub can_post: bool,
    /// The peer has read our messages up to this id (✓✓).
    #[serde(default)]
    pub read_out: i32,
    /// We have read incoming messages up to this id: a chat with unread ones opens after it.
    #[serde(default)]
    pub read_in: i32,
    /// Unread mentions and reactions to our messages — the "@" and "❤" buttons above the composer.
    #[serde(default)]
    pub unread_mentions: i32,
    #[serde(default)]
    pub unread_reactions: i32,
    /// Protected content: no forwarding, copying or saving.
    #[serde(default)]
    pub noforwards: bool,
    /// We may pin messages here.
    #[serde(default)]
    pub can_pin: bool,
    /// Telegram hides the whole chat here (copyright, local law…): why, instead of its messages.
    #[serde(default)]
    pub restricted: Option<String>,
}

/// Forum topic. The General topic has id 1.
#[derive(Debug, Clone)]
pub struct Topic {
    pub id: i32,
    pub title: String,
    /// Icon color (RGB from Telegram).
    pub color: u32,
    pub unread: i32,
    pub pinned: bool,
    pub closed: bool,
    pub last_text: String,
    pub last_time: String,
    pub last_ts: i64,
}

/// An active account session (a device that is logged in).
#[derive(Debug, Clone)]
pub struct Session {
    pub hash: i64,
    /// «Glassgram 0.1 · Linux», «Telegram Android 11.2 · Pixel 7».
    pub app: String,
    pub device: String,
    /// Country and IP.
    pub place: String,
    pub active: String,
    pub current: bool,
}

/// Privacy settings we show (as in the Privacy section of Telegram Desktop).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum PrivacyKey {
    LastSeen,
    Phone,
    Photo,
    Forwards,
    Calls,
    Invites,
    Voice,
    Birthday,
}

impl PrivacyKey {
    pub const ALL: [PrivacyKey; 8] = [Self::Phone, Self::LastSeen, Self::Photo, Self::Birthday, Self::Forwards, Self::Calls, Self::Voice, Self::Invites];

    pub fn title(self) -> &'static str {
        match self {
            Self::Phone => l!("Номер телефону", "Phone number"),
            Self::LastSeen => l!("Час останнього входу", "Last seen"),
            Self::Photo => l!("Фото профілю", "Profile photo"),
            Self::Forwards => l!("Посилання на акаунт у пересланих", "Account link in forwarded messages"),
            Self::Calls => l!("Дзвінки", "Calls"),
            Self::Voice => l!("Голосові повідомлення", "Voice messages"),
            Self::Invites => l!("Запрошення в групи", "Group invites"),
            Self::Birthday => l!("День народження", "Date of birth"),
        }
    }

    pub(super) fn input(self) -> tl::enums::InputPrivacyKey {
        use tl::enums::InputPrivacyKey as K;
        match self {
            Self::LastSeen => K::StatusTimestamp,
            Self::Phone => K::PhoneNumber,
            Self::Photo => K::ProfilePhoto,
            Self::Forwards => K::Forwards,
            Self::Calls => K::PhoneCall,
            Self::Invites => K::ChatInvite,
            Self::Voice => K::VoiceMessages,
            Self::Birthday => K::Birthday,
        }
    }
}

/// Who can see: the base rule without exceptions.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Audience {
    Everybody,
    Contacts,
    Nobody,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PrivacyState {
    pub audience: Audience,
    /// There are exceptions for specific people or groups (changing the preset drops them).
    pub exceptions: bool,
}

impl PrivacyState {
    pub(super) fn from_rules(rules: &[tl::enums::PrivacyRule]) -> Self {
        use tl::enums::PrivacyRule as R;
        let has = |f: fn(&R) -> bool| rules.iter().any(f);
        let audience = if has(|r| matches!(r, R::PrivacyValueAllowAll)) {
            Audience::Everybody
        } else if has(|r| matches!(r, R::PrivacyValueAllowContacts)) {
            Audience::Contacts
        } else {
            Audience::Nobody
        };
        let exceptions = has(|r| {
            matches!(
                r,
                R::PrivacyValueAllowUsers(_) | R::PrivacyValueDisallowUsers(_) | R::PrivacyValueAllowChatParticipants(_) | R::PrivacyValueDisallowChatParticipants(_)
            )
        });
        Self { audience, exceptions }
    }
}

/// Send options: silent and/or scheduled (Unix time).
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct SendOpts {
    pub silent: bool,
    pub schedule: Option<i64>,
    /// No link preview (turned off with ✕ above the composer).
    pub no_webpage: bool,
}

impl SendOpts {
    pub(super) fn apply(self, m: InputMessage) -> InputMessage {
        let at = self.schedule.map(|ts| std::time::UNIX_EPOCH + Duration::from_secs(ts.max(0) as u64));
        m.silent(self.silent).schedule_date(at).link_preview(!self.no_webpage)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum MediaKind {
    /// Silent animation (a GIF in Telegram is an MP4).
    Gif,
    Video,
    /// Round video message.
    Round,
    Audio,
    Voice,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct MediaInfo {
    pub kind: MediaKind,
    /// Duration in seconds.
    pub duration: f64,
    pub size: (i32, i32),
    /// Track title or file name.
    pub title: String,
    pub performer: String,
    /// Voice message waveform (values 0..31).
    pub waveform: Vec<u8>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Reaction {
    pub emoji: String,
    pub count: i32,
    pub mine: bool,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PollOption {
    pub text: String,
    pub option: Vec<u8>,
    pub voters: i32,
    pub chosen: bool,
    pub correct: bool,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PollInfo {
    pub question: String,
    pub answers: Vec<PollOption>,
    pub total: i32,
    pub closed: bool,
    pub quiz: bool,
    pub multiple: bool,
}

impl PollInfo {
    pub fn voted(&self) -> bool {
        self.answers.iter().any(|a| a.chosen)
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct GeoInfo {
    pub lat: f64,
    pub lon: f64,
    /// Place name and address (for a venue) or empty.
    pub title: String,
    pub address: String,
    pub live: bool,
}

/// Result of an inline query to a bot.
#[derive(Debug, Clone)]
pub struct InlineResult {
    pub id: String,
    pub title: String,
    pub description: String,
    pub kind: String,
}

/// A button under a bot message.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum ButtonAction {
    Callback(Vec<u8>),
    Url(String),
    /// A regular keyboard button: sends its own text.
    Text,
    Copy(String),
    /// Mini app button: inline (`simple: false`) or on the reply keyboard;
    /// `bot` is the bare id of the bot that owns it (sender or "via" bot).
    WebApp { url: String, simple: bool, bot: i64 },
    /// A type that is not supported yet (payment, games…).
    Other,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Button {
    pub text: String,
    pub action: ButtonAction,
}

/// A user's online status.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub enum Presence {
    /// Not a user, or the status is hidden.
    #[default]
    None,
    /// Online until this timestamp.
    Online(i64),
    Offline(i64),
    Recently,
    LastWeek,
    LastMonth,
}

impl Presence {
    pub(super) fn from_status(s: &tl::enums::UserStatus) -> Self {
        use tl::enums::UserStatus as S;
        match s {
            S::Online(o) => Self::Online(o.expires as i64),
            S::Offline(o) => Self::Offline(o.was_online as i64),
            S::Recently(_) => Self::Recently,
            S::LastWeek(_) => Self::LastWeek,
            S::LastMonth(_) => Self::LastMonth,
            S::Empty => Self::None,
        }
    }

    pub fn online(&self) -> bool {
        matches!(self, Self::Online(t) if *t > jiff::Timestamp::now().as_second())
    }

    /// Subtitle in the chat header, as in Telegram Desktop.
    pub fn text(&self) -> Option<String> {
        Some(match *self {
            Self::None => return None,
            _ if self.online() => l!("в мережі", "online").to_owned(),
            Self::Online(ts) | Self::Offline(ts) => {
                let Ok(t) = jiff::Timestamp::from_second(ts) else { return None };
                let (z, now) = (local(t), jiff::Zoned::now());
                if z.date() == now.date() {
                    lf!("був(ла) о {}", "last seen at {}", clock(t))
                } else if now.date().yesterday().is_ok_and(|y| y == z.date()) {
                    lf!("був(ла) вчора о {}", "last seen yesterday at {}", clock(t))
                } else {
                    lf!("був(ла) {}", "last seen {}", z.strftime("%d.%m.%y"))
                }
            }
            Self::Recently => l!("був(ла) нещодавно", "last seen recently").to_owned(),
            Self::LastWeek => l!("був(ла) цього тижня", "last seen this week").to_owned(),
            Self::LastMonth => l!("був(ла) цього місяця", "last seen this month").to_owned(),
        })
    }
}

/// Search result: a message together with the name of the chat it was found in.
#[derive(Debug, Clone)]
pub struct SearchHit {
    pub chat: String,
    pub kind: ChatKind,
    pub msg: Msg,
}

impl ChatInfo {
    pub fn is_unread(&self) -> bool {
        self.unread > 0 || self.unread_mark
    }
}

/// A folder (dialog filter) from Telegram.
#[derive(Debug, Clone, Default)]
pub struct Folder {
    pub id: i32,
    pub title: String,
    pub emoticon: String,
    pub include: Vec<PeerId>,
    pub exclude: Vec<PeerId>,
    pub contacts: bool,
    pub non_contacts: bool,
    pub groups: bool,
    pub broadcasts: bool,
    pub bots: bool,
    pub exclude_muted: bool,
    pub exclude_read: bool,
    pub exclude_archived: bool,
    /// Pinned in the folder (a subset of `include`).
    pub pinned: Vec<PeerId>,
    /// Shared folder (chatlist): its rules are not edited here.
    pub shared: bool,
}

impl Folder {
    /// The same rules as in Telegram Desktop (Data::ChatFilter::contains).
    pub fn contains(&self, c: &ChatInfo) -> bool {
        if self.exclude.contains(&c.id) {
            return false;
        }
        if self.include.contains(&c.id) {
            return true;
        }
        if (self.exclude_muted && c.muted) || (self.exclude_read && !c.is_unread()) || (self.exclude_archived && c.archived) {
            return false;
        }
        match c.kind {
            ChatKind::User | ChatKind::Saved => if c.contact { self.contacts } else { self.non_contacts },
            ChatKind::Bot => self.bots,
            ChatKind::Group => self.groups,
            ChatKind::Channel => self.broadcasts,
        }
    }
}

#[derive(Debug, Clone)]
pub struct StickerSetInfo {
    pub id: i64,
    pub title: String,
    /// (document id, emoji)
    pub stickers: Vec<(i64, String)>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Msg {
    pub id: i32,
    #[serde(with = "crate::cache::peer_id")]
    pub peer: PeerId,
    pub out: bool,
    pub sender: String,
    pub text: String,
    pub time: String,
    pub ts: i64,
    pub day: String,
    pub edited: bool,
    pub reply_to: Option<i32>,
    /// The part of the replied message this reply quotes.
    #[serde(default)]
    pub reply_quote: Option<String>,
    /// The replied message is in another chat (its title): `reply_to` is an id there.
    #[serde(default)]
    pub reply_elsewhere: Option<String>,
    pub photo: bool,
    /// (file name, size)
    pub file: Option<(String, String)>,
    /// (document id, emoji)
    pub sticker: Option<(i64, String)>,
    /// Formatting of `text`.
    pub spans: Vec<Span>,
    /// Who it was forwarded from.
    pub forwarded: Option<String>,
    /// Rows of buttons under the message.
    pub buttons: Vec<Vec<Button>>,
    /// The album this message belongs to.
    pub group: Option<i64>,
    pub reactions: Vec<Reaction>,
    pub poll: Option<PollInfo>,
    pub geo: Option<GeoInfo>,
    /// (name, phone)
    pub contact: Option<(String, String)>,
    pub media: Option<MediaInfo>,
    /// Video sticker (webm) that can be animated.
    pub video_sticker: bool,
    /// Voice or round video message not listened to yet.
    pub media_unread: bool,
    /// Mentions us and is not read yet: counts for the "@" button.
    #[serde(default)]
    pub mentioned: bool,
    /// Someone reacted to it and we haven't seen that yet: counts for the "❤" button.
    #[serde(default)]
    pub reaction_unread: bool,
    /// Forum topic (`None`: not a forum, or the General topic).
    pub topic: Option<i32>,
    /// Number of comments on a channel post (`None`: comments are disabled).
    pub comments: Option<i32>,
    #[serde(default)]
    pub pinned: bool,
    /// Views of a channel post.
    #[serde(default)]
    pub views: Option<i32>,
    /// Protected message: no forwarding, copying or saving.
    #[serde(default)]
    pub noforwards: bool,
    /// Link preview under the text.
    #[serde(default)]
    pub webpage: Option<WebPageInfo>,
    /// In Saved Messages: the chat it was saved from. Not cached: source tabs load their own pages.
    #[serde(skip)]
    pub saved_from: Option<PeerId>,
}

impl Msg {
    /// Empty message, for list rows that only need the chat.
    pub fn placeholder(peer: PeerId) -> Self {
        Self {
            id: 0,
            peer,
            out: false,
            sender: String::new(),
            text: String::new(),
            time: String::new(),
            ts: 0,
            day: String::new(),
            edited: false,
            reply_to: None,
            reply_quote: None,
            reply_elsewhere: None,
            mentioned: false,
            reaction_unread: false,
            photo: false,
            file: None,
            sticker: None,
            spans: Vec::new(),
            forwarded: None,
            buttons: Vec::new(),
            group: None,
            reactions: Vec::new(),
            poll: None,
            geo: None,
            contact: None,
            media: None,
            video_sticker: false,
            media_unread: false,
            topic: None,
            comments: None,
            pinned: false,
            views: None,
            noforwards: false,
            webpage: None,
            saved_from: None,
        }
    }

    /// The topic the message belongs to: no topic header means General.
    pub fn topic_id(&self) -> i32 {
        self.topic.unwrap_or(GENERAL_TOPIC)
    }
}
