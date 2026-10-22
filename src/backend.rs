//! Telegram-бекенд: окремий потік з tokio-рантаймом, спілкується з UI через канали.

use std::collections::{HashMap, HashSet};
use std::path::PathBuf;
use std::sync::{Arc, Mutex, mpsc};
use std::time::Duration;

use base64::Engine as _;
use eframe::egui::ColorImage;
use grammers_client::client::{LoginToken, PasswordToken};
use grammers_client::media::{Attribute, Downloadable, InputMedia, Media, Photo, Uploaded};
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
use tokio::task::JoinHandle;

use crate::format::{self, Span};
use crate::i18n::{l, lf};

const DIALOG_LIMIT: usize = 300;
pub const PAGE: usize = 50;
/// Найбільші розміри показу в точках; у пікселі переводимо з урахуванням щільності екрана,
/// щоб текстура не зменшувалась GPU (egui-wgpu не має mipmap — інакше з'являються «драбинки»).
const AVATAR_PT: f32 = 48.0;
const PHOTO_PT: f32 = 420.0;
const STICKER_PT: f32 = 160.0;
const FULL_PT: f32 = 1600.0;
/// Фото, більші за цей розмір (px по довшій стороні), стискаємо перед надсиланням, як Telegram Desktop.
const SEND_PHOTO_PX: u32 = 2560;
const SEARCH_LIMIT: usize = 60;
/// Скільки повідомлень максимум підвантажуємо, щоб дістатися до знайденого.
const JUMP_LIMIT: usize = 1000;

#[derive(Debug)]
pub enum Cmd {
    Connect { api_id: i32, api_hash: String },
    /// Завершити потік бекенда (перемикання акаунта).
    Shutdown,
    RequestCode(String),
    SubmitCode(String),
    SubmitPassword(String),
    QrLogin,
    CancelQr,
    ReloadDialogs,
    OpenChat(PeerId),
    LoadOlder { peer: PeerId, before: i32 },
    Send { peer: PeerId, text: String, reply_to: Option<i32>, opts: SendOpts },
    Edit { peer: PeerId, id: i32, text: String },
    Delete { peer: PeerId, ids: Vec<i32> },
    /// `id` — номер завантаження в UI, для прогресу.
    /// Кілька файлів: зображення йдуть альбомами по 10, решта — окремими повідомленнями.
    /// `as_file` — без стиснення, як документи.
    SendFiles { id: u64, peer: PeerId, paths: Vec<PathBuf>, caption: String, reply_to: Option<i32>, opts: SendOpts, as_file: bool },
    LoadFullPhoto { peer: PeerId, id: i32 },
    /// Поставити реакцію (`None` — зняти свою).
    React { peer: PeerId, id: i32, emoji: Option<String> },
    Vote { peer: PeerId, id: i32, options: Vec<Vec<u8>> },
    LoadScheduled(PeerId),
    SendScheduledNow { peer: PeerId, ids: Vec<i32> },
    DeleteScheduled { peer: PeerId, ids: Vec<i32> },
    Reschedule { peer: PeerId, id: i32, at: i64 },
    InlineQuery { peer: PeerId, bot: String, query: String },
    /// Завантажити медіафайл (відео, GIF, аудіо) у кеш для програвання.
    LoadMedia { peer: PeerId, id: i32 },
    /// Записане голосове (OGG/Opus); `waveform` — рівні 0..31.
    SendVoice { id: u64, peer: PeerId, path: PathBuf, duration: f64, waveform: Vec<u8>, reply_to: Option<i32> },
    /// Записане відеоповідомлення (MP4 384×384).
    SendRound { id: u64, peer: PeerId, path: PathBuf, duration: f64, reply_to: Option<i32> },
    /// Голосове чи кружечок прослухано.
    MarkListened { peer: PeerId, id: i32 },
    SendInline { peer: PeerId, query_id: i64, result: String, reply_to: Option<i32> },
    /// Створити або змінити папку (id нової — вільний, від 2).
    SaveFolder(Folder),
    DeleteFolder(i32),
    /// Теми форуму (група з увімкненими темами).
    LoadTopics(PeerId),
    /// Історія теми: перша сторінка (`before: None`) або раніші повідомлення.
    OpenTopic { peer: PeerId, topic: i32, before: Option<i32> },
    CreateTopic { peer: PeerId, title: String, color: u32 },
    /// Перейменувати (`title`) та/або закрити чи відкрити (`closed`) тему.
    EditTopic { peer: PeerId, id: i32, title: Option<String>, closed: Option<bool> },
    /// Позначити тему прочитаною до `max_id`.
    ReadTopic { peer: PeerId, topic: i32, max_id: i32 },
    /// Коментарі до допису каналу (з прив'язаної групи обговорення).
    /// `before` — раніші коментарі (id найстарішого завантаженого).
    OpenComments { peer: PeerId, id: i32, before: Option<i32> },
    /// Вивантажити всю історію чату в «Завантаження» (HTML + JSON, за бажанням — з медіа).
    ExportChat { peer: PeerId, title: String, media: bool },
    /// Знайти публічний чат за @username і відкрити без вступу.
    ResolveUsername(String),
    /// Пошук публічних груп, каналів і ботів за назвою.
    SearchPublic(String),
    /// Вступити в публічну групу чи канал.
    Join(PeerId),
    LoadSessions,
    /// Завершити сеанс на іншому пристрої (`hash` з `Session`).
    TerminateSession(i64),
    TerminateOtherSessions,
    LoadPrivacy,
    SetPrivacy(PrivacyKey, Audience),
    Forward { from: PeerId, ids: Vec<i32>, to: PeerId },
    /// Натиснута інлайн-кнопка з callback-даними.
    PressButton { peer: PeerId, id: i32, data: Vec<u8> },
    SendSticker { peer: PeerId, doc: i64, reply_to: Option<i32> },
    LoadStickers,
    LoadStickerThumbs(Vec<i64>),
    Download { peer: PeerId, id: i32 },
    /// Відкрити чат і підвантажити історію до повідомлення `id` включно.
    OpenAt { peer: PeerId, id: i32 },
    SetPinned { peer: PeerId, pinned: bool },
    SetMuted { peer: PeerId, muted: bool },
    SetArchived { peer: PeerId, archived: bool },
    MarkRead(PeerId),
    MarkUnread(PeerId),
    SaveDraft { peer: PeerId, text: String },
    Typing(PeerId),
    /// `peer: None` — глобальний пошук по всіх чатах.
    Search { peer: Option<PeerId>, query: String },
    Logout,
    // Внутрішні: результати QR-задачі.
    QrPassword(PasswordToken),
    QrSuccess(Box<tl::types::auth::Authorization>),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ImgKey {
    Avatar(PeerId),
    Photo(PeerId, i32),
    /// Фото в повному розмірі для переглядача.
    Full(PeerId, i32),
    Sticker(i64),
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
    /// `peer: None` — видалення без прив'язки до каналу (особисті чати й малі групи).
    Deleted { ids: Vec<i32>, peer: Option<PeerId> },
    Image { key: ImgKey, image: ColorImage },
    Folders(Vec<Folder>),
    Stickers(Vec<StickerSetInfo>),
    UploadProgress { id: u64, done: u64, total: u64 },
    UploadDone(u64),
    Reactions { peer: PeerId, id: i32, reactions: Vec<Reaction> },
    MediaReady { peer: PeerId, id: i32, path: PathBuf },
    /// Голосові чи кружечки прослухано (`peer: None` — особисті чати й малі групи).
    Listened { peer: Option<PeerId>, ids: Vec<i32> },
    Scheduled { peer: PeerId, msgs: Vec<Msg> },
    InlineResults { peer: PeerId, bot: String, query: String, query_id: i64, results: Vec<InlineResult> },
    /// Відповідь бота на натискання кнопки: текст і чи показувати як попередження.
    BotAnswer { text: String, alert: bool },
    /// Хтось друкує (`active: false` — перестав).
    Typing { peer: PeerId, who: String, active: bool },
    Presence { peer: PeerId, presence: Presence },
    Draft { peer: PeerId, text: String },
    /// Прочитано на іншому пристрої: скільки лишилось непрочитаних.
    ReadInbox { peer: PeerId, unread: i32 },
    SearchResults { peer: Option<PeerId>, query: String, hits: Vec<SearchHit> },
    Topics { peer: PeerId, topics: Vec<Topic> },
    /// Сторінка історії теми: зливається з історією чату (`older` — раніші).
    TopicHistory { peer: PeerId, topic: i32, msgs: Vec<Msg>, older: bool },
    /// Коментарі до допису `post`: група обговорення, її повідомлення-корінь і самі коментарі.
    Comments { peer: PeerId, post: i32, discussion: PeerId, root: i32, msgs: Vec<Msg>, older: bool },
    /// Експорт: скільки повідомлень уже вивантажено.
    ExportProgress { peer: PeerId, done: usize },
    Exported { peer: PeerId, dir: PathBuf, count: usize },
    /// Публічний чат, знайдений за username (може бути не в списку діалогів).
    ChatFound(ChatInfo),
    PublicResults { query: String, chats: Vec<ChatInfo> },
    Joined(PeerId),
    Sessions(Vec<Session>),
    Privacy(PrivacyKey, PrivacyState),
    Notice(String),
    Error(String),
    LoggedOut,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ChatKind {
    User,
    Bot,
    Group,
    Channel,
    Saved,
}

#[derive(Debug, Clone)]
pub struct ChatInfo {
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
    /// Позначено «непрочитаним» вручну.
    pub unread_mark: bool,
    /// Чернетка в розмітці поля вводу.
    pub draft: String,
    pub presence: Presence,
    /// Група-форум: повідомлення розкладено по темах.
    pub forum: bool,
    /// Ми учасник (для знайдених публічних чатів — ні, поки не вступили).
    pub joined: bool,
    /// Можна писати: у каналі — лише власник чи адмін з правом публікувати.
    pub can_post: bool,
}

/// Тема форуму. Id загальної теми — 1.
#[derive(Debug, Clone)]
pub struct Topic {
    pub id: i32,
    pub title: String,
    /// Колір значка (RGB з Telegram).
    pub color: u32,
    pub unread: i32,
    pub pinned: bool,
    pub closed: bool,
    pub last_text: String,
    pub last_time: String,
    pub last_ts: i64,
}

/// Загальна тема форуму.
pub const GENERAL_TOPIC: i32 = 1;

/// Кольори значків тем, які приймає Telegram.
pub const TOPIC_COLORS: [u32; 6] = [0x6FB9F0, 0xFFD67E, 0xCB86DB, 0x8EEE98, 0xFF93B2, 0xFB6F5F];

/// Активний сеанс акаунта (пристрій, на якому виконано вхід).
#[derive(Debug, Clone)]
pub struct Session {
    pub hash: i64,
    /// «Glassgram 0.1 · Linux», «Telegram Android 11.2 · Pixel 7».
    pub app: String,
    pub device: String,
    /// Країна й IP.
    pub place: String,
    pub active: String,
    pub current: bool,
}

/// Налаштування приватності, які показуємо (як у розділі «Конфіденційність» Telegram Desktop).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum PrivacyKey {
    LastSeen,
    Phone,
    Photo,
    Forwards,
    Calls,
    Invites,
    Voice,
}

impl PrivacyKey {
    pub const ALL: [PrivacyKey; 7] = [Self::Phone, Self::LastSeen, Self::Photo, Self::Forwards, Self::Calls, Self::Voice, Self::Invites];

    pub fn title(self) -> &'static str {
        match self {
            Self::Phone => l!("Номер телефону", "Phone number"),
            Self::LastSeen => l!("Час останнього входу", "Last seen"),
            Self::Photo => l!("Фото профілю", "Profile photo"),
            Self::Forwards => l!("Посилання на акаунт у пересланих", "Account link in forwarded messages"),
            Self::Calls => l!("Дзвінки", "Calls"),
            Self::Voice => l!("Голосові повідомлення", "Voice messages"),
            Self::Invites => l!("Запрошення в групи", "Group invites"),
        }
    }

    fn input(self) -> tl::enums::InputPrivacyKey {
        use tl::enums::InputPrivacyKey as K;
        match self {
            Self::LastSeen => K::StatusTimestamp,
            Self::Phone => K::PhoneNumber,
            Self::Photo => K::ProfilePhoto,
            Self::Forwards => K::Forwards,
            Self::Calls => K::PhoneCall,
            Self::Invites => K::ChatInvite,
            Self::Voice => K::VoiceMessages,
        }
    }
}

/// Хто бачить: основне правило без винятків.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Audience {
    Everybody,
    Contacts,
    Nobody,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PrivacyState {
    pub audience: Audience,
    /// Є винятки для окремих людей чи груп (при зміні пресету вони зникнуть).
    pub exceptions: bool,
}

impl PrivacyState {
    fn from_rules(rules: &[tl::enums::PrivacyRule]) -> Self {
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

fn privacy_rules(a: Audience) -> Vec<tl::enums::InputPrivacyRule> {
    use tl::enums::InputPrivacyRule as R;
    match a {
        Audience::Everybody => vec![R::InputPrivacyValueAllowAll],
        Audience::Contacts => vec![R::InputPrivacyValueAllowContacts, R::InputPrivacyValueDisallowAll],
        Audience::Nobody => vec![R::InputPrivacyValueDisallowAll],
    }
}

/// Параметри надсилання: без звуку та/або за розкладом (Unix-час).
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct SendOpts {
    pub silent: bool,
    pub schedule: Option<i64>,
}

impl SendOpts {
    fn apply(self, m: InputMessage) -> InputMessage {
        let at = self.schedule.map(|ts| std::time::UNIX_EPOCH + Duration::from_secs(ts.max(0) as u64));
        m.silent(self.silent).schedule_date(at)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MediaKind {
    /// Анімація без звуку (GIF у Telegram — це MP4).
    Gif,
    Video,
    /// Кругле відеоповідомлення.
    Round,
    Audio,
    Voice,
}

#[derive(Debug, Clone, PartialEq)]
pub struct MediaInfo {
    pub kind: MediaKind,
    /// Тривалість у секундах.
    pub duration: f64,
    pub size: (i32, i32),
    /// Назва треку або ім'я файлу.
    pub title: String,
    pub performer: String,
    /// Хвильова форма голосового (значення 0..31).
    pub waveform: Vec<u8>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Reaction {
    pub emoji: String,
    pub count: i32,
    pub mine: bool,
}

#[derive(Debug, Clone, PartialEq)]
pub struct PollOption {
    pub text: String,
    pub option: Vec<u8>,
    pub voters: i32,
    pub chosen: bool,
    pub correct: bool,
}

#[derive(Debug, Clone, PartialEq)]
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

#[derive(Debug, Clone, PartialEq)]
pub struct GeoInfo {
    pub lat: f64,
    pub lon: f64,
    /// Назва й адреса місця (для venue) або порожньо.
    pub title: String,
    pub address: String,
    pub live: bool,
}

/// Результат inline-запиту до бота.
#[derive(Debug, Clone)]
pub struct InlineResult {
    pub id: String,
    pub title: String,
    pub description: String,
    pub kind: String,
}

/// Кнопка під повідомленням бота.
#[derive(Debug, Clone, PartialEq)]
pub enum ButtonAction {
    Callback(Vec<u8>),
    Url(String),
    /// Кнопка звичайної клавіатури: надсилає свій текст.
    Text,
    Copy(String),
    /// Тип, який ще не підтримується (оплата, ігри, web app…).
    Other,
}

#[derive(Debug, Clone)]
pub struct Button {
    pub text: String,
    pub action: ButtonAction,
}

/// Статус «в мережі» користувача.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Presence {
    /// Не користувач або статус прихований.
    #[default]
    None,
    /// В мережі до цієї мітки часу.
    Online(i64),
    Offline(i64),
    Recently,
    LastWeek,
    LastMonth,
}

impl Presence {
    fn from_status(s: &tl::enums::UserStatus) -> Self {
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

    /// Підпис у шапці чату, як у Telegram Desktop.
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

/// Результат пошуку: повідомлення разом із назвою чату, де воно знайдене.
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

/// Папка (dialog filter) з Telegram.
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
    /// Закріплені в папці (підмножина `include`).
    pub pinned: Vec<PeerId>,
    /// Спільна папка (chatlist) — її правила тут не редагуються.
    pub shared: bool,
}

impl Folder {
    /// Ті самі правила, що й у Telegram Desktop (Data::ChatFilter::contains).
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
    /// (id документа, емодзі)
    pub stickers: Vec<(i64, String)>,
}

#[derive(Debug, Clone)]
pub struct Msg {
    pub id: i32,
    pub peer: PeerId,
    pub out: bool,
    pub sender: String,
    pub text: String,
    pub time: String,
    pub ts: i64,
    pub day: String,
    pub edited: bool,
    pub reply_to: Option<i32>,
    pub photo: bool,
    /// (ім'я файлу, розмір)
    pub file: Option<(String, String)>,
    /// (id документа, емодзі)
    pub sticker: Option<(i64, String)>,
    /// Форматування `text`.
    pub spans: Vec<Span>,
    /// Від кого переслано.
    pub forwarded: Option<String>,
    /// Рядки кнопок під повідомленням.
    pub buttons: Vec<Vec<Button>>,
    /// Альбом, до якого належить повідомлення.
    pub group: Option<i64>,
    pub reactions: Vec<Reaction>,
    pub poll: Option<PollInfo>,
    pub geo: Option<GeoInfo>,
    /// (ім'я, телефон)
    pub contact: Option<(String, String)>,
    pub media: Option<MediaInfo>,
    /// Відео-стікер (webm), який можна анімувати.
    pub video_sticker: bool,
    /// Голосове чи кружечок ще не прослухано.
    pub media_unread: bool,
    /// Тема форуму (`None` — не форум або загальна тема).
    pub topic: Option<i32>,
    /// Кількість коментарів до допису каналу (`None` — коментарі вимкнено).
    pub comments: Option<i32>,
}

impl Msg {
    /// Порожнє повідомлення — для рядків списку, де потрібен лише чат.
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
        }
    }

    /// Тема, у якій лежить повідомлення: без заголовка теми — загальна.
    pub fn topic_id(&self) -> i32 {
        self.topic.unwrap_or(GENERAL_TOPIC)
    }
}

pub struct Handle {
    pub tx: tmpsc::UnboundedSender<Cmd>,
    pub rx: mpsc::Receiver<Ev>,
}

pub fn data_dir() -> PathBuf {
    let base = std::env::var_os("XDG_DATA_HOME")
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".local/share")))
        .unwrap_or_else(|| PathBuf::from("."));
    base.join("glassgram")
}

/// Повертає (api_id, api_hash) зі змінних оточення або збереженого конфіга.
pub fn saved_credentials() -> Option<(i32, String)> {
    if let (Ok(id), Ok(hash)) = (std::env::var("TG_API_ID"), std::env::var("TG_API_HASH")) {
        return Some((id.trim().parse().ok()?, hash.trim().to_owned()));
    }
    let text = std::fs::read_to_string(data_dir().join("credentials")).ok()?;
    let mut lines = text.lines();
    let id = lines.next()?.trim().parse().ok()?;
    let hash = lines.next()?.trim().to_owned();
    Some((id, hash))
}

fn save_credentials(api_id: i32, api_hash: &str) {
    let dir = data_dir();
    let _ = std::fs::create_dir_all(&dir);
    let path = dir.join("credentials");
    let _ = std::fs::write(&path, format!("{api_id}\n{api_hash}\n"));
    make_private(&dir);
    make_private(&path);
}

/// Лише для власника: ключ сесії дає повний доступ до акаунта.
/// На Unix — права 0600 (тека 0700); на Windows профіль користувача й так закритий.
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

/// Запускає бекенд для сесії у файлі `session`.
pub fn spawn(ctx: eframe::egui::Context, session: PathBuf) -> Handle {
    let (cmd_tx, cmd_rx) = tmpsc::unbounded_channel();
    let (ev_tx, ev_rx) = mpsc::channel();
    let back = cmd_tx.clone();
    std::thread::Builder::new()
        .name("telegram".into())
        .spawn(move || {
            let rt = tokio::runtime::Builder::new_multi_thread()
                .enable_all()
                .build()
                .expect("tokio runtime");
            rt.block_on(Worker::new(ctx, ev_tx, back, session).run(cmd_rx));
            // Фонові задачі (оновлення, завантаження) зупиняються разом із рантаймом.
            rt.shutdown_background();
        })
        .expect("spawn backend thread");
    Handle { tx: cmd_tx, rx: ev_rx }
}

#[derive(Clone)]
struct Emitter {
    ctx: eframe::egui::Context,
    tx: mpsc::Sender<Ev>,
}

impl Emitter {
    fn send(&self, ev: Ev) {
        let _ = self.tx.send(ev);
        self.ctx.request_repaint();
    }
}

type Refs = Arc<Mutex<HashMap<PeerId, PeerRef>>>;
/// Назви чатів і користувачів: для «… друкує» та «Переслано від …».
type Names = Arc<Mutex<HashMap<PeerId, String>>>;

/// Фонове завантаження аватарок і фото з дисковим кешем.
#[derive(Clone)]
struct Loader {
    client: Client,
    out: Emitter,
    limit: Arc<Semaphore>,
    requested: Arc<Mutex<HashSet<String>>>,
}

impl Loader {
    fn fetch<D>(&self, key: ImgKey, file: String, max_pt: f32, source: D)
    where
        D: Downloadable + Send + Sync + 'static,
    {
        if !self.requested.lock().unwrap().insert(file.clone()) {
            return;
        }
        let this = self.clone();
        tokio::spawn(async move {
            let _permit = this.limit.acquire().await;
            let path = data_dir().join("cache").join(&file);
            let bytes = match tokio::fs::read(&path).await {
                Ok(b) => b,
                Err(_) => {
                    let mut data = Vec::new();
                    let mut it = this.client.iter_download(&source);
                    while let Ok(Some(chunk)) = it.next().await {
                        data.extend(chunk);
                    }
                    if data.is_empty() {
                        this.requested.lock().unwrap().remove(&file);
                        return;
                    }
                    let _ = tokio::fs::create_dir_all(path.parent().unwrap()).await;
                    let _ = tokio::fs::write(&path, &data).await;
                    data
                }
            };
            let max_px = (max_pt * this.out.ctx.pixels_per_point()).ceil() as u32;
            let decoded = tokio::task::spawn_blocking(move || decode(&bytes, max_px)).await;
            if let Ok(Some(image)) = decoded {
                this.out.send(Ev::Image { key, image });
            }
        });
    }

    fn avatar(&self, peer: &Peer) {
        let photo_id = match peer {
            Peer::User(u) => u.photo().map(|p| p.photo_id),
            Peer::Group(g) => g.photo().map(|p| p.photo_id),
            Peer::Channel(c) => c.photo().map(|p| p.photo_id),
            Peer::Community(c) => c.photo().map(|p| p.photo_id),
        };
        let Some(photo_id) = photo_id else { return };
        let file = format!("a{}_{photo_id}.jpg", peer.id().bot_api_dialog_id_unchecked());
        if self.requested.lock().unwrap().contains(&file) {
            return;
        }
        let (this, peer) = (self.clone(), peer.clone());
        tokio::spawn(async move {
            if let Ok(Some(photo)) = peer.photo(false).await {
                this.fetch(ImgKey::Avatar(peer.id()), file, AVATAR_PT, photo);
            }
        });
    }

    fn message_photo(&self, peer: PeerId, m: &Message) {
        if let (Some(Media::Sticker(_)), Some(doc)) = (m.media(), raw_document(m)) {
            return self.sticker(&doc);
        }
        if let (Some(Media::Document(_)), Some(doc)) = (m.media(), raw_document(m))
            && let Some(thumb) = doc.thumbs.iter().flatten().filter_map(|t| match t {
                tl::enums::PhotoSize::Size(s) => Some((s.w, s.r#type.clone())),
                tl::enums::PhotoSize::Progressive(s) => Some((s.w, s.r#type.clone())),
                _ => None,
            }).max_by_key(|t| t.0)
        {
            // Мініатюра відео чи GIF — показуємо як фото, поки не завантажено сам файл.
            let file = format!("v{}_{}.jpg", peer.bot_api_dialog_id_unchecked(), m.id());
            return self.fetch(ImgKey::Photo(peer, m.id()), file, PHOTO_PT, DocLoc { doc, thumb: thumb.1 });
        }
        let Some(Media::Photo(photo)) = m.media() else { return };
        let file = format!("p{}_{}.jpg", peer.bot_api_dialog_id_unchecked(), m.id());
        let key = ImgKey::Photo(peer, m.id());
        match best_thumb(&photo) {
            Some(size) => self.fetch(key, file, PHOTO_PT, size),
            None => self.fetch(key, file, PHOTO_PT, photo),
        }
    }
}

/// Розташування документа (стікера) для завантаження: сам файл або його мініатюра.
struct DocLoc {
    doc: tl::types::Document,
    thumb: String,
}

impl Downloadable for DocLoc {
    fn to_raw_input_location(&self) -> Option<tl::enums::InputFileLocation> {
        Some(tl::types::InputDocumentFileLocation {
            id: self.doc.id,
            access_hash: self.doc.access_hash,
            file_reference: self.doc.file_reference.clone(),
            thumb_size: self.thumb.clone(),
        }.into())
    }

    fn dc_id(&self) -> Option<i32> {
        Some(self.doc.dc_id)
    }
}

impl Loader {
    /// Статичні webp-стікери вантажимо цілком, анімовані (tgs/webm) — мініатюрою.
    fn sticker(&self, doc: &tl::types::Document) {
        let thumb = if doc.mime_type == "image/webp" {
            String::new()
        } else {
            let Some(t) = doc.thumbs.iter().flatten().find_map(|t| match t {
                tl::enums::PhotoSize::Size(s) => Some(s.r#type.clone()),
                _ => None,
            }) else {
                return;
            };
            t
        };
        let file = format!("s{}{}.img", doc.id, if thumb.is_empty() { "" } else { "_t" });
        self.fetch(ImgKey::Sticker(doc.id), file, STICKER_PT, DocLoc { doc: doc.clone(), thumb });
    }
}

fn raw_document(m: &Message) -> Option<tl::types::Document> {
    let doc = match m.media()? {
        Media::Sticker(s) => s.document.raw.document,
        Media::Document(d) => d.raw.document,
        _ => return None,
    };
    match doc? {
        tl::enums::Document::Document(d) => Some(d),
        _ => None,
    }
}

fn best_thumb(photo: &Photo) -> Option<grammers_client::media::PhotoSize> {
    let thumbs = photo.thumbs();
    ["x", "m", "y", "w"]
        .iter()
        .find_map(|t| thumbs.iter().find(|s| s.photo_type() == *t).cloned())
}

fn decode(bytes: &[u8], max_px: u32) -> Option<ColorImage> {
    let img = image::load_from_memory(bytes).ok()?;
    let img = if img.width() > max_px || img.height() > max_px {
        // Lanczos дає чисте зменшення без муару на тонких деталях.
        img.resize(max_px, max_px, image::imageops::FilterType::Lanczos3)
    } else {
        img
    };
    let rgba = img.to_rgba8();
    Some(ColorImage::from_rgba_unmultiplied([rgba.width() as usize, rgba.height() as usize], rgba.as_raw()))
}

struct Worker {
    /// Файл сесії поточного акаунта.
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
    /// Боти для inline-режиму за username.
    bots: HashMap<String, PeerRef>,
}

impl Worker {
    fn new(ctx: eframe::egui::Context, tx: mpsc::Sender<Ev>, back: tmpsc::UnboundedSender<Cmd>, session_path: PathBuf) -> Self {
        Self {
            session_path,
            out: Emitter { ctx, tx },
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
        }
    }

    async fn run(mut self, mut rx: tmpsc::UnboundedReceiver<Cmd>) {
        while let Some(cmd) = rx.recv().await {
            if matches!(cmd, Cmd::Shutdown) {
                self.stop_qr();
                break;
            }
            if let Err(e) = self.handle(cmd).await {
                self.out.send(Ev::Error(e));
            }
        }
    }

    fn client(&self) -> Result<Client, String> {
        self.client.clone().ok_or_else(|| l!("Немає з'єднання", "No connection").to_owned())
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
            Cmd::Connect { api_id, api_hash } => self.connect(api_id, api_hash).await,
            // Обробляється в `run` до цього місця.
            Cmd::Shutdown => Ok(()),
            Cmd::RequestCode(phone) => {
                self.stop_qr();
                let client = self.client()?;
                let token = client
                    .request_login_code(phone.trim(), &self.api_hash)
                    .await
                    .map_err(|e| lf!("Не вдалося надіслати код: {e}", "Could not send the code: {e}"))?;
                self.login_token = Some(token);
                self.out.send(Ev::NeedCode);
                Ok(())
            }
            Cmd::SubmitCode(code) => {
                let client = self.client()?;
                let token = self.login_token.as_ref().ok_or(l!("Спершу запросіть код", "Request a code first"))?;
                match client.sign_in(token, code.trim()).await {
                    Ok(_) => self.after_login().await,
                    Err(SignInError::PasswordRequired(pt)) => {
                        self.out.send(Ev::NeedPassword(pt.hint().map(str::to_owned)));
                        self.password_token = Some(pt);
                        Ok(())
                    }
                    Err(SignInError::InvalidCode) => Err(l!("Невірний код", "Wrong code").into()),
                    Err(SignInError::SignUpRequired) => {
                        Err(l!("Акаунт не знайдено — зареєструйтесь в офіційному застосунку", "Account not found — sign up in the official app").into())
                    }
                    Err(e) => Err(e.to_string()),
                }
            }
            Cmd::SubmitPassword(pw) => {
                let client = self.client()?;
                let token = self.password_token.take().ok_or(l!("Пароль не очікувався", "Password was not expected"))?;
                match client.check_password(token, pw.as_bytes()).await {
                    Ok(_) => self.after_login().await,
                    Err(SignInError::InvalidPassword(pt)) => {
                        self.password_token = Some(pt);
                        Err(l!("Невірний пароль", "Wrong passcode").into())
                    }
                    Err(e) => Err(e.to_string()),
                }
            }
            Cmd::QrLogin => {
                self.stop_qr();
                let client = self.client()?;
                let session = self.session.clone().ok_or(l!("Немає сесії", "No session"))?;
                self.qr_task = Some(tokio::spawn(qr_loop(
                    client,
                    session,
                    self.api_id,
                    self.api_hash.clone(),
                    self.login_update.clone(),
                    self.out.clone(),
                    self.back.clone(),
                )));
                Ok(())
            }
            Cmd::CancelQr => {
                self.stop_qr();
                Ok(())
            }
            Cmd::QrPassword(pt) => {
                self.out.send(Ev::NeedPassword(pt.hint().map(str::to_owned)));
                self.password_token = Some(pt);
                Ok(())
            }
            Cmd::QrSuccess(auth) => {
                self.complete_login(*auth).await?;
                self.after_login().await
            }
            Cmd::ReloadDialogs => self.load_dialogs().await,
            Cmd::OpenChat(peer) => {
                let client = self.client()?;
                let r = self.peer_ref(peer)?;
                let msgs = self.fetch_history(&client, r, None).await?;
                self.out.send(Ev::History { peer, msgs, older: false });
                let _ = client.mark_as_read(r).await;
                Ok(())
            }
            Cmd::LoadOlder { peer, before } => {
                let client = self.client()?;
                let r = self.peer_ref(peer)?;
                let msgs = self.fetch_history(&client, r, Some(before)).await?;
                self.out.send(Ev::History { peer, msgs, older: true });
                Ok(())
            }
            Cmd::Send { peer, text, reply_to, opts } => {
                let client = self.client()?;
                let r = self.peer_ref(peer)?;
                let m = client
                    .send_message(r, opts.apply(rich(&text).reply_to(reply_to)))
                    .await
                    .map_err(|e| lf!("Не надіслано: {e}", "Not sent: {e}"))?;
                // В особистих чатах сервер відповідає коротким UpdateShortSentMessage, де лише частина
                // сутностей (без `коду` тощо). Свої сутності ми знаємо точно — беремо їх.
                let (plain, ents) = format::parse(&text);
                self.sent().emit_with(peer, &m, reply_to, opts, Some((&plain, &ents)));
                Ok(())
            }
            Cmd::Forward { from, ids, to } => {
                let client = self.client()?;
                let (src, dst) = (self.peer_ref(from)?, self.peer_ref(to)?);
                let sent = client.forward_messages(dst, &ids, src).await.map_err(|e| lf!("Не переслано: {e}", "Not forwarded: {e}"))?;
                let emitter = self.sent();
                for m in sent.iter().flatten() {
                    emitter.emit(to, m, None, SendOpts::default());
                }
                let title = self.names.lock().unwrap().get(&to).cloned().unwrap_or_default();
                self.out.send(Ev::Notice(lf!("Переслано: {} → {title}", "Forwarded: {} → {title}", ids.len())));
                Ok(())
            }
            Cmd::PressButton { peer, id, data } => {
                let client = self.client()?;
                let r = self.peer_ref(peer)?;
                let out = self.out.clone();
                // Бот може відповідати кілька секунд — не блокуємо інші команди.
                tokio::spawn(async move {
                    let req = tl::functions::messages::GetBotCallbackAnswer { game: false, peer: r.into(), msg_id: id, data: Some(data), password: None };
                    match client.invoke(&req).await {
                        Ok(tl::enums::messages::BotCallbackAnswer::Answer(a)) => {
                            if let Some(url) = a.url {
                                let _ = std::process::Command::new("xdg-open").arg(url).spawn();
                            }
                            if let Some(text) = a.message.filter(|t| !t.is_empty()) {
                                out.send(Ev::BotAnswer { text, alert: a.alert });
                            }
                        }
                        Err(e) => out.send(Ev::Error(lf!("Кнопка: {e}", "Button: {e}"))),
                    }
                });
                Ok(())
            }
            Cmd::Edit { peer, id, text } => {
                let client = self.client()?;
                let r = self.peer_ref(peer)?;
                client
                    .edit_message(r, id, rich(&text))
                    .await
                    .map_err(|e| lf!("Не вдалося змінити: {e}", "Could not edit: {e}"))
            }
            Cmd::Delete { peer, ids } => {
                let client = self.client()?;
                let r = self.peer_ref(peer)?;
                client
                    .delete_messages(r, &ids)
                    .await
                    .map_err(|e| lf!("Не вдалося видалити: {e}", "Could not delete: {e}"))?;
                self.out.send(Ev::Deleted { ids, peer: Some(peer) });
                Ok(())
            }
            Cmd::SendFiles { id, peer, paths, caption, reply_to, opts, as_file } => {
                let (client, r) = match (self.client(), self.peer_ref(peer)) {
                    (Ok(c), Ok(r)) => (c, r),
                    (Err(e), _) | (_, Err(e)) => {
                        self.out.send(Ev::UploadDone(id));
                        return Err(e);
                    }
                };
                let sent = self.sent();
                // Завантаження йде у фоні, щоб довгі файли не блокували інші команди.
                tokio::spawn(async move {
                    let res = send_files(&client, r, id, paths, caption, reply_to, opts, as_file, &sent).await;
                    sent.out.send(Ev::UploadDone(id));
                    if let Err(e) = res {
                        sent.out.send(Ev::Error(e));
                    }
                });
                Ok(())
            }
            Cmd::SendVoice { id, peer, path, duration, waveform, reply_to } => {
                let (client, r) = match (self.client(), self.peer_ref(peer)) {
                    (Ok(c), Ok(r)) => (c, r),
                    (Err(e), _) | (_, Err(e)) => {
                        self.out.send(Ev::UploadDone(id));
                        return Err(e);
                    }
                };
                let sent = self.sent();
                tokio::spawn(async move {
                    let res = async {
                        let total = std::fs::metadata(&path).map(|m| m.len()).unwrap_or(0);
                        let (up, _) = upload(&client, &path, id, 0, total, &sent.out).await?;
                        let msg = InputMessage::new()
                            .reply_to(reply_to)
                            .mime_type("audio/ogg")
                            .document(up)
                            .attribute(Attribute::Voice { duration: Duration::from_secs_f64(duration.max(1.0)), waveform: Some(pack_waveform(&waveform)) });
                        let m = client.send_message(r, msg).await.map_err(|e| lf!("Голосове не надіслано: {e}", "Voice message not sent: {e}"))?;
                        sent.emit(r.id, &m, reply_to, SendOpts::default());
                        Ok::<_, String>(())
                    }
                    .await;
                    sent.out.send(Ev::UploadDone(id));
                    let _ = std::fs::remove_file(&path);
                    if let Err(e) = res {
                        sent.out.send(Ev::Error(e));
                    }
                });
                Ok(())
            }
            Cmd::SaveFolder(f) => {
                let client = self.client()?;
                let peers = |ids: &mut dyn Iterator<Item = &PeerId>| ids.filter_map(|p| self.peer_ref(*p).ok()).map(tl::enums::InputPeer::from).collect::<Vec<_>>();
                let filter = tl::types::DialogFilter {
                    contacts: f.contacts,
                    non_contacts: f.non_contacts,
                    groups: f.groups,
                    broadcasts: f.broadcasts,
                    bots: f.bots,
                    exclude_muted: f.exclude_muted,
                    exclude_read: f.exclude_read,
                    exclude_archived: f.exclude_archived,
                    title_noanimate: false,
                    id: f.id,
                    title: tl::types::TextWithEntities { text: f.title.clone(), entities: Vec::new() }.into(),
                    emoticon: (!f.emoticon.is_empty()).then(|| f.emoticon.clone()),
                    color: None,
                    pinned_peers: peers(&mut f.pinned.iter().filter(|p| f.include.contains(p))),
                    include_peers: peers(&mut f.include.iter().filter(|p| !f.pinned.contains(p))),
                    exclude_peers: peers(&mut f.exclude.iter()),
                };
                client
                    .invoke(&tl::functions::messages::UpdateDialogFilter { id: f.id, filter: Some(filter.into()) })
                    .await
                    .map_err(|e| lf!("Папку не збережено: {e}", "Folder not saved: {e}"))?;
                self.load_folders().await
            }
            Cmd::DeleteFolder(id) => {
                let client = self.client()?;
                client
                    .invoke(&tl::functions::messages::UpdateDialogFilter { id, filter: None })
                    .await
                    .map_err(|e| lf!("Папку не видалено: {e}", "Folder not deleted: {e}"))?;
                self.load_folders().await
            }
            Cmd::LoadTopics(peer) => {
                let client = self.client()?;
                let r = self.peer_ref(peer)?;
                let req = tl::functions::messages::GetForumTopics { peer: r.into(), q: None, offset_date: 0, offset_id: 0, offset_topic: 0, limit: 100 };
                let tl::enums::messages::ForumTopics::Topics(res) = client.invoke(&req).await.map_err(|e| lf!("Теми: {e}", "Topics: {e}"))?;
                // Останні повідомлення тем приходять разом зі списком — беремо з них прев'ю.
                let last: HashMap<i32, &tl::types::Message> = res
                    .messages
                    .iter()
                    .filter_map(|m| match m {
                        tl::enums::Message::Message(x) => Some((x.id, x)),
                        _ => None,
                    })
                    .collect();
                let mut topics: Vec<Topic> = res
                    .topics
                    .iter()
                    .filter_map(|t| match t {
                        tl::enums::ForumTopic::Topic(t) if !t.hidden || t.id == GENERAL_TOPIC => {
                            let m = last.get(&t.top_message);
                            let ts = m.map_or(t.date, |m| m.date) as i64;
                            let when = jiff::Timestamp::from_second(ts).unwrap_or_default();
                            Some(Topic {
                                id: t.id,
                                title: t.title.clone(),
                                color: t.icon_color as u32,
                                unread: t.unread_count,
                                pinned: t.pinned,
                                closed: t.closed,
                                last_text: m.map(|m| m.message.replace('\n', " ")).filter(|s| !s.is_empty()).unwrap_or_else(|| l!("Вкладення", "Attachment").into()),
                                last_time: short_time(when),
                                last_ts: ts,
                            })
                        }
                        _ => None,
                    })
                    .collect();
                topics.sort_by_key(|t| (!t.pinned, -t.last_ts));
                self.out.send(Ev::Topics { peer, topics });
                Ok(())
            }
            Cmd::OpenTopic { peer, topic, before } => {
                let client = self.client()?;
                let r = self.peer_ref(peer)?;
                let req = tl::functions::messages::GetReplies {
                    peer: r.into(),
                    msg_id: topic,
                    offset_id: before.unwrap_or(0),
                    offset_date: 0,
                    add_offset: 0,
                    limit: PAGE as i32,
                    max_id: 0,
                    min_id: 0,
                    hash: 0,
                };
                let raw = client.invoke(&req).await.map_err(|e| lf!("Тема: {e}", "Topic: {e}"))?;
                let list = match raw {
                    tl::enums::messages::Messages::Messages(m) => m.messages,
                    tl::enums::messages::Messages::Slice(m) => m.messages,
                    tl::enums::messages::Messages::ChannelMessages(m) => m.messages,
                    tl::enums::messages::Messages::NotModified(_) => Vec::new(),
                };
                let ids: Vec<i32> = list.iter().map(|m| m.id()).collect();
                // Сирі повідомлення без мапи учасників не конвертуються — дотягуємо їх звичайним запитом.
                let mut msgs: Vec<Msg> = if ids.is_empty() {
                    Vec::new()
                } else {
                    client
                        .get_messages_by_id(r, &ids)
                        .await
                        .map_err(|e| lf!("Тема: {e}", "Topic: {e}"))?
                        .into_iter()
                        .flatten()
                        .map(|m| self.convert_loaded(peer, &m))
                        .collect()
                };
                msgs.sort_by_key(|m| m.id);
                self.out.send(Ev::TopicHistory { peer, topic, msgs, older: before.is_some() });
                Ok(())
            }
            Cmd::CreateTopic { peer, title, color } => {
                let client = self.client()?;
                let r = self.peer_ref(peer)?;
                let req = tl::functions::messages::CreateForumTopic {
                    title_missing: false,
                    peer: r.into(),
                    title,
                    icon_color: Some(color as i32),
                    icon_emoji_id: None,
                    random_id: jiff::Timestamp::now().as_nanosecond() as i64,
                    send_as: None,
                };
                client.invoke(&req).await.map_err(|e| lf!("Тему не створено: {e}", "Topic not created: {e}"))?;
                let _ = self.back.send(Cmd::LoadTopics(peer));
                Ok(())
            }
            Cmd::EditTopic { peer, id, title, closed } => {
                let client = self.client()?;
                let r = self.peer_ref(peer)?;
                let req = tl::functions::messages::EditForumTopic {
                    peer: r.into(),
                    topic_id: id,
                    title,
                    icon_emoji_id: None,
                    closed: closed.map(Into::into),
                    hidden: None,
                };
                client.invoke(&req).await.map_err(|e| lf!("Тему не змінено: {e}", "Topic not changed: {e}"))?;
                let _ = self.back.send(Cmd::LoadTopics(peer));
                Ok(())
            }
            Cmd::ReadTopic { peer, topic, max_id } => {
                let client = self.client()?;
                let r = self.peer_ref(peer)?;
                client
                    .invoke(&tl::functions::messages::ReadDiscussion { peer: r.into(), msg_id: topic, read_max_id: max_id })
                    .await
                    .map(drop)
                    .map_err(|e| lf!("Тема: {e}", "Topic: {e}"))
            }
            Cmd::OpenComments { peer, id, before } => {
                let client = self.client()?;
                let r = self.peer_ref(peer)?;
                let tl::enums::messages::DiscussionMessage::Message(d) = client
                    .invoke(&tl::functions::messages::GetDiscussionMessage { peer: r.into(), msg_id: id })
                    .await
                    .map_err(|e| lf!("Коментарі: {e}", "Comments: {e}"))?;
                let (discussion, root) = d
                    .messages
                    .iter()
                    .find_map(|m| match m {
                        tl::enums::Message::Message(x) => Some((PeerId::from(&x.peer_id), x.id)),
                        _ => None,
                    })
                    .ok_or(l!("Коментарі: немає групи обговорення", "Comments: no discussion group"))?;
                // Посилання на групу обговорення потрібне, щоб читати й писати коментарі.
                for chat in d.chats {
                    let p = Peer::from_raw(&client, chat);
                    if p.id() == discussion
                        && let Ok(Some(rf)) = p.to_ref().await
                    {
                        self.refs.lock().unwrap().insert(discussion, rf);
                        self.names.lock().unwrap().insert(discussion, peer_title(&p));
                    }
                }
                let disc = self.peer_ref(discussion)?;
                let req = tl::functions::messages::GetReplies { peer: r.into(), msg_id: id, offset_id: before.unwrap_or(0), offset_date: 0, add_offset: 0, limit: PAGE as i32, max_id: 0, min_id: 0, hash: 0 };
                let ids: Vec<i32> = match client.invoke(&req).await.map_err(|e| lf!("Коментарі: {e}", "Comments: {e}"))? {
                    tl::enums::messages::Messages::Messages(m) => m.messages,
                    tl::enums::messages::Messages::Slice(m) => m.messages,
                    tl::enums::messages::Messages::ChannelMessages(m) => m.messages,
                    tl::enums::messages::Messages::NotModified(_) => Vec::new(),
                }
                .iter()
                .map(|m| m.id())
                .collect();
                let mut msgs: Vec<Msg> = if ids.is_empty() {
                    Vec::new()
                } else {
                    client
                        .get_messages_by_id(disc, &ids)
                        .await
                        .map_err(|e| lf!("Коментарі: {e}", "Comments: {e}"))?
                        .into_iter()
                        .flatten()
                        .map(|m| self.convert_loaded(discussion, &m))
                        .collect()
                };
                msgs.sort_by_key(|m| m.id);
                self.out.send(Ev::Comments { peer, post: id, discussion, root, msgs, older: before.is_some() });
                Ok(())
            }
            Cmd::ExportChat { peer, title, media } => {
                let client = self.client()?;
                let r = self.peer_ref(peer)?;
                let out = self.out.clone();
                // Велика історія вантажиться довго — у фоні, щоб не блокувати інші команди.
                tokio::spawn(async move {
                    let dir = crate::export::target_dir(&title);
                    let progress = |done| out.send(Ev::ExportProgress { peer, done });
                    match crate::export::run(&client, r, &title, media, &dir, progress).await {
                        Ok(count) => out.send(Ev::Exported { peer, dir, count }),
                        Err(e) => out.send(Ev::Error(e)),
                    }
                });
                Ok(())
            }
            Cmd::ResolveUsername(name) => {
                let client = self.client()?;
                let name = name.trim_start_matches('@').to_owned();
                let peer = client
                    .resolve_username(&name)
                    .await
                    .map_err(|e| lf!("@{name}: {e}", "@{name}: {e}"))?
                    .ok_or_else(|| lf!("@{name} не знайдено", "@{name} not found"))?;
                let r = peer.to_ref().await.ok().flatten().ok_or_else(|| lf!("@{name}: немає доступу", "@{name}: no access"))?;
                self.refs.lock().unwrap().insert(peer.id(), r);
                self.names.lock().unwrap().insert(peer.id(), peer_title(&peer));
                if let Some(loader) = &self.loader {
                    loader.avatar(&peer);
                }
                self.out.send(Ev::ChatFound(found_chat(&peer, format!("@{name}"))));
                Ok(())
            }
            Cmd::SearchPublic(query) => {
                let client = self.client()?;
                let req = tl::functions::contacts::Search { broadcasts: false, bots: false, q: query.clone(), limit: 20 };
                let tl::enums::contacts::Found::Found(res) = client.invoke(&req).await.map_err(|e| lf!("Пошук: {e}", "Search: {e}"))?;
                // Лише глобальні результати; свої чати й так є в списку.
                let wanted: Vec<PeerId> = res.results.iter().map(PeerId::from).collect();
                let mut chats = Vec::new();
                for raw in res.chats {
                    let peer = Peer::from_raw(&client, raw);
                    if !wanted.contains(&peer.id()) {
                        continue;
                    }
                    let Ok(Some(r)) = peer.to_ref().await else { continue };
                    self.refs.lock().unwrap().insert(peer.id(), r);
                    self.names.lock().unwrap().insert(peer.id(), peer_title(&peer));
                    if let Some(loader) = &self.loader {
                        loader.avatar(&peer);
                    }
                    let members = match &peer {
                        Peer::Channel(c) => c.raw.participants_count,
                        Peer::Group(g) => match &g.raw {
                            tl::enums::Chat::Channel(c) => c.participants_count,
                            tl::enums::Chat::Chat(c) => Some(c.participants_count),
                            _ => None,
                        },
                        _ => None,
                    };
                    let user = peer.username().map(|u| format!("@{u}")).unwrap_or_default();
                    let label = match members {
                        Some(n) => lf!("{user} · учасників: {n}", "{user} · members: {n}"),
                        None => user,
                    };
                    chats.push(found_chat(&peer, label.trim_start_matches(" · ").to_owned()));
                }
                self.out.send(Ev::PublicResults { query, chats });
                Ok(())
            }
            Cmd::Join(peer) => {
                let client = self.client()?;
                let r = self.peer_ref(peer)?;
                client
                    .invoke(&tl::functions::channels::JoinChannel { channel: r.into() })
                    .await
                    .map_err(|e| lf!("Не вдалося приєднатися: {e}", "Could not join: {e}"))?;
                self.out.send(Ev::Joined(peer));
                self.load_dialogs().await
            }
            Cmd::LoadSessions => {
                let client = self.client()?;
                let tl::enums::account::Authorizations::Authorizations(res) =
                    client.invoke(&tl::functions::account::GetAuthorizations {}).await.map_err(|e| lf!("Сеанси: {e}", "Sessions: {e}"))?;
                let mut list: Vec<Session> = res
                    .authorizations
                    .into_iter()
                    .map(|tl::enums::Authorization::Authorization(a)| {
                        let active = jiff::Timestamp::from_second(a.date_active as i64).unwrap_or_default();
                        Session {
                            hash: a.hash,
                            app: format!("{} {}", a.app_name, a.app_version).trim().to_owned(),
                            device: [a.device_model, a.platform, a.system_version].into_iter().filter(|s| !s.is_empty()).collect::<Vec<_>>().join(" · "),
                            place: [a.country, a.ip].into_iter().filter(|s| !s.is_empty()).collect::<Vec<_>>().join(" · "),
                            active: if a.current { l!("зараз", "now").to_owned() } else { format!("{} {}", day(active), clock(active)) },
                            current: a.current,
                        }
                    })
                    .collect();
                list.sort_by_key(|s| !s.current);
                self.out.send(Ev::Sessions(list));
                Ok(())
            }
            Cmd::TerminateSession(hash) => {
                let client = self.client()?;
                client.invoke(&tl::functions::account::ResetAuthorization { hash }).await.map_err(|e| lf!("Сеанс не завершено: {e}", "Session not terminated: {e}"))?;
                let _ = self.back.send(Cmd::LoadSessions);
                Ok(())
            }
            Cmd::TerminateOtherSessions => {
                let client = self.client()?;
                client.invoke(&tl::functions::auth::ResetAuthorizations {}).await.map_err(|e| lf!("Сеанси не завершено: {e}", "Sessions not terminated: {e}"))?;
                let _ = self.back.send(Cmd::LoadSessions);
                Ok(())
            }
            Cmd::LoadPrivacy => {
                let client = self.client()?;
                let out = self.out.clone();
                tokio::spawn(async move {
                    for key in PrivacyKey::ALL {
                        match client.invoke(&tl::functions::account::GetPrivacy { key: key.input() }).await {
                            Ok(tl::enums::account::PrivacyRules::Rules(r)) => out.send(Ev::Privacy(key, PrivacyState::from_rules(&r.rules))),
                            Err(e) => log_error(&lf!("Приватність: {e}", "Privacy: {e}")),
                        }
                    }
                });
                Ok(())
            }
            Cmd::SetPrivacy(key, audience) => {
                let client = self.client()?;
                let req = tl::functions::account::SetPrivacy { key: key.input(), rules: privacy_rules(audience) };
                let tl::enums::account::PrivacyRules::Rules(r) = client.invoke(&req).await.map_err(|e| lf!("Приватність: {e}", "Privacy: {e}"))?;
                self.out.send(Ev::Privacy(key, PrivacyState::from_rules(&r.rules)));
                Ok(())
            }
            Cmd::MarkListened { peer, id } => {
                let client = self.client()?;
                let r = self.peer_ref(peer)?;
                let res = if peer.kind() == grammers_client::session::types::PeerKind::Channel {
                    client.invoke(&tl::functions::channels::ReadMessageContents { channel: r.into(), id: vec![id] }).await.map(drop)
                } else {
                    client.invoke(&tl::functions::messages::ReadMessageContents { id: vec![id] }).await.map(drop)
                };
                res.map_err(|e| lf!("Не вдалося позначити прослуханим: {e}", "Could not mark as listened: {e}"))
            }
            Cmd::SendRound { id, peer, path, duration, reply_to } => {
                let (client, r) = match (self.client(), self.peer_ref(peer)) {
                    (Ok(c), Ok(r)) => (c, r),
                    (Err(e), _) | (_, Err(e)) => {
                        self.out.send(Ev::UploadDone(id));
                        return Err(e);
                    }
                };
                let sent = self.sent();
                tokio::spawn(async move {
                    let res = async {
                        let total = std::fs::metadata(&path).map(|m| m.len()).unwrap_or(0);
                        let (up, _) = upload(&client, &path, id, 0, total, &sent.out).await?;
                        let side = crate::camera::SIDE as i32;
                        let msg = InputMessage::new().reply_to(reply_to).mime_type("video/mp4").document(up).attribute(Attribute::Video {
                            round_message: true,
                            supports_streaming: true,
                            duration: Duration::from_secs_f64(duration.max(1.0)),
                            w: side,
                            h: side,
                        });
                        let m = client.send_message(r, msg).await.map_err(|e| lf!("Відеоповідомлення не надіслано: {e}", "Video message not sent: {e}"))?;
                        sent.emit(r.id, &m, reply_to, SendOpts::default());
                        Ok::<_, String>(())
                    }
                    .await;
                    sent.out.send(Ev::UploadDone(id));
                    let _ = std::fs::remove_file(&path);
                    if let Err(e) = res {
                        sent.out.send(Ev::Error(e));
                    }
                });
                Ok(())
            }
            Cmd::SendSticker { peer, doc, reply_to } => {
                let client = self.client()?;
                let r = self.peer_ref(peer)?;
                let d = self.docs.get(&doc).ok_or(l!("Стікер не знайдено", "Sticker not found"))?;
                let media = tl::types::InputMediaDocument {
                    spoiler: false,
                    id: tl::types::InputDocument { id: d.id, access_hash: d.access_hash, file_reference: d.file_reference.clone() }.into(),
                    video_cover: None,
                    video_timestamp: None,
                    ttl_seconds: None,
                    query: None,
                };
                let m = client
                    .send_message(r, InputMessage::new().media(media).reply_to(reply_to))
                    .await
                    .map_err(|e| lf!("Стікер не надіслано: {e}", "Sticker not sent: {e}"))?;
                self.sent().emit(peer, &m, reply_to, SendOpts::default());
                Ok(())
            }
            Cmd::LoadStickers => self.load_stickers().await,
            Cmd::LoadStickerThumbs(ids) => {
                if let Some(loader) = &self.loader {
                    for id in ids {
                        if let Some(d) = self.docs.get(&id) {
                            loader.sticker(d);
                        }
                    }
                }
                Ok(())
            }
            Cmd::LoadFullPhoto { peer, id } => {
                let client = self.client()?;
                let r = self.peer_ref(peer)?;
                let loader = self.loader.clone().ok_or(l!("Немає з'єднання", "No connection"))?;
                tokio::spawn(async move {
                    if let Ok(msgs) = client.get_messages_by_id(r, &[id]).await
                        && let Some(Some(m)) = msgs.into_iter().next()
                        && let Some(Media::Photo(photo)) = m.media()
                    {
                        let file = format!("f{}_{id}.jpg", peer.bot_api_dialog_id_unchecked());
                        loader.fetch(ImgKey::Full(peer, id), file, FULL_PT, photo);
                    }
                });
                Ok(())
            }
            Cmd::React { peer, id, emoji } => {
                let client = self.client()?;
                let r = self.peer_ref(peer)?;
                let reaction = match emoji {
                    Some(e) => InputReactions::emoticon(e),
                    None => InputReactions::remove(),
                };
                client.send_reactions(r, id, reaction).await.map_err(|e| lf!("Реакція: {e}", "Reaction: {e}"))
            }
            Cmd::Vote { peer, id, options } => {
                let client = self.client()?;
                let r = self.peer_ref(peer)?;
                client
                    .invoke(&tl::functions::messages::SendVote { peer: r.into(), msg_id: id, options })
                    .await
                    .map_err(|e| lf!("Голосування: {e}", "Vote: {e}"))?;
                // Оновлені результати беремо з самого повідомлення.
                if let Ok(msgs) = client.get_messages_by_id(r, &[id]).await
                    && let Some(Some(m)) = msgs.into_iter().next()
                {
                    let mut msg = convert(&m, &self.names);
                    msg.peer = peer;
                    self.out.send(Ev::Edited(msg));
                }
                Ok(())
            }
            Cmd::LoadScheduled(peer) => self.load_scheduled(peer).await,
            Cmd::SendScheduledNow { peer, ids } => {
                let client = self.client()?;
                let r = self.peer_ref(peer)?;
                let n = ids.len();
                client
                    .invoke(&tl::functions::messages::SendScheduledMessages { peer: r.into(), id: ids })
                    .await
                    .map_err(|e| lf!("Не надіслано: {e}", "Not sent: {e}"))?;
                self.emit_latest(peer, n).await;
                self.load_scheduled(peer).await
            }
            Cmd::DeleteScheduled { peer, ids } => {
                let client = self.client()?;
                let r = self.peer_ref(peer)?;
                client
                    .invoke(&tl::functions::messages::DeleteScheduledMessages { peer: r.into(), id: ids })
                    .await
                    .map_err(|e| lf!("Не видалено: {e}", "Not deleted: {e}"))?;
                self.load_scheduled(peer).await
            }
            Cmd::Reschedule { peer, id, at } => {
                let client = self.client()?;
                let r = self.peer_ref(peer)?;
                let req = tl::functions::messages::EditMessage {
                    no_webpage: false,
                    invert_media: false,
                    peer: r.into(),
                    id,
                    message: None,
                    media: None,
                    reply_markup: None,
                    entities: None,
                    schedule_date: Some(at as i32),
                    schedule_repeat_period: None,
                    quick_reply_shortcut_id: None,
                    rich_message: None,
                };
                client.invoke(&req).await.map_err(|e| lf!("Не вдалося змінити час: {e}", "Could not change the time: {e}"))?;
                self.load_scheduled(peer).await
            }
            Cmd::InlineQuery { peer, bot, query } => {
                let client = self.client()?;
                let r = self.peer_ref(peer)?;
                let bot_ref = self.resolve_bot(&client, &bot).await?;
                let out = self.out.clone();
                tokio::spawn(async move {
                    let req = tl::functions::messages::GetInlineBotResults {
                        bot: bot_ref.into(),
                        peer: r.into(),
                        geo_point: None,
                        query: query.clone(),
                        offset: String::new(),
                    };
                    match client.invoke(&req).await {
                        Ok(tl::enums::messages::BotResults::Results(res)) => {
                            let results = res.results.into_iter().map(inline_result).collect();
                            out.send(Ev::InlineResults { peer, bot, query, query_id: res.query_id, results });
                        }
                        // Бот міг не встигнути — це не помилка для користувача, просто порожньо.
                        Err(e) => {
                            log_error(&format!("inline @{bot}: {e}"));
                            out.send(Ev::InlineResults { peer, bot, query, query_id: 0, results: Vec::new() });
                        }
                    }
                });
                Ok(())
            }
            Cmd::SendInline { peer, query_id, result, reply_to } => {
                let client = self.client()?;
                let r = self.peer_ref(peer)?;
                let req = tl::functions::messages::SendInlineBotResult {
                    silent: false,
                    background: false,
                    clear_draft: true,
                    hide_via: false,
                    peer: r.into(),
                    reply_to: reply_to.map(|id| {
                        tl::types::InputReplyToMessage {
                            reply_to_msg_id: id,
                            top_msg_id: None,
                            reply_to_peer_id: None,
                            quote_text: None,
                            quote_entities: None,
                            quote_offset: None,
                            monoforum_peer_id: None,
                            todo_item_id: None,
                            poll_option: None,
                        }
                        .into()
                    }),
                    random_id: jiff::Timestamp::now().as_nanosecond() as i64,
                    query_id,
                    id: result,
                    schedule_date: None,
                    send_as: None,
                    quick_reply_shortcut: None,
                    allow_paid_stars: None,
                };
                client.invoke(&req).await.map_err(|e| lf!("Не надіслано: {e}", "Not sent: {e}"))?;
                self.emit_latest(peer, 1).await;
                Ok(())
            }
            Cmd::LoadMedia { peer, id } => {
                let client = self.client()?;
                let r = self.peer_ref(peer)?;
                let out = self.out.clone();
                tokio::spawn(async move {
                    match cache_media(&client, r, id).await {
                        Ok(path) => out.send(Ev::MediaReady { peer, id, path }),
                        Err(e) => out.send(Ev::Error(e)),
                    }
                });
                Ok(())
            }
            Cmd::Download { peer, id } => {
                let client = self.client()?;
                let r = self.peer_ref(peer)?;
                let out = self.out.clone();
                tokio::spawn(async move {
                    let res = download(&client, r, id).await;
                    out.send(match res {
                        Ok(path) => Ev::Notice(lf!("Збережено: {}", "Saved: {}", path.display())),
                        Err(e) => Ev::Error(e),
                    });
                });
                Ok(())
            }
            Cmd::OpenAt { peer, id } => {
                let client = self.client()?;
                let r = self.peer_ref(peer)?;
                let mut iter = client.iter_messages(r).limit(JUMP_LIMIT);
                let mut msgs = Vec::new();
                while let Some(m) = iter.next().await.map_err(|e| lf!("Історія: {e}", "History: {e}"))? {
                    let reached = m.id() <= id;
                    msgs.push(self.convert_loaded(r.id, &m));
                    if reached && msgs.len() >= PAGE {
                        break;
                    }
                }
                msgs.reverse();
                self.out.send(Ev::History { peer, msgs, older: false });
                Ok(())
            }
            Cmd::SetPinned { peer, pinned } => {
                let r = self.peer_ref(peer)?;
                let req = tl::functions::messages::ToggleDialogPin { pinned, peer: dialog_peer(r) };
                self.chat_action(&req, l!("Не вдалося закріпити", "Could not pin")).await
            }
            Cmd::SetMuted { peer, muted } => {
                let r = self.peer_ref(peer)?;
                let req = tl::functions::account::UpdateNotifySettings {
                    peer: tl::types::InputNotifyPeer { peer: r.into() }.into(),
                    settings: tl::types::InputPeerNotifySettings {
                        show_previews: None,
                        silent: None,
                        mute_until: Some(if muted { i32::MAX } else { 0 }),
                        sound: None,
                        stories_muted: None,
                        stories_hide_sender: None,
                        stories_sound: None,
                    }
                    .into(),
                };
                self.chat_action(&req, l!("Не вдалося змінити сповіщення", "Could not change notifications")).await
            }
            Cmd::SetArchived { peer, archived } => {
                let r = self.peer_ref(peer)?;
                let req = tl::functions::folders::EditPeerFolders {
                    folder_peers: vec![tl::types::InputFolderPeer { peer: r.into(), folder_id: archived as i32 }.into()],
                };
                self.chat_action(&req, l!("Не вдалося перемістити в архів", "Could not move to archive")).await
            }
            Cmd::MarkRead(peer) => {
                let client = self.client()?;
                let r = self.peer_ref(peer)?;
                let _ = client.mark_as_read(r).await;
                let req = tl::functions::messages::MarkDialogUnread { unread: false, parent_peer: None, peer: dialog_peer(r) };
                self.chat_action(&req, l!("Не вдалося позначити прочитаним", "Could not mark as read")).await
            }
            Cmd::MarkUnread(peer) => {
                let r = self.peer_ref(peer)?;
                let req = tl::functions::messages::MarkDialogUnread { unread: true, parent_peer: None, peer: dialog_peer(r) };
                self.chat_action(&req, l!("Не вдалося позначити непрочитаним", "Could not mark as unread")).await
            }
            Cmd::SaveDraft { peer, text } => {
                let client = self.client()?;
                let r = self.peer_ref(peer)?;
                let (message, entities) = format::parse(text.trim());
                let req = tl::functions::messages::SaveDraft {
                    no_webpage: false,
                    invert_media: false,
                    reply_to: None,
                    peer: r.into(),
                    message,
                    entities: (!entities.is_empty()).then_some(entities),
                    media: None,
                    effect: None,
                    suggested_post: None,
                    rich_message: None,
                };
                client.invoke(&req).await.map(drop).map_err(|e| lf!("Чернетка: {e}", "Draft: {e}"))
            }
            Cmd::Typing(peer) => {
                let client = self.client()?;
                let r = self.peer_ref(peer)?;
                tokio::spawn(async move {
                    let _ = client.action(r).oneshot(tl::types::SendMessageTypingAction {}).await;
                });
                Ok(())
            }
            Cmd::Search { peer, query } => self.search(peer, query).await,
            Cmd::Logout => {
                self.stop_qr();
                let client = self.client()?;
                let _ = client.sign_out().await;
                self.client = None;
                self.session = None;
                self.loader = None;
                let _ = std::fs::remove_file(&self.session_path);
                self.out.send(Ev::LoggedOut);
                Ok(())
            }
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
        let proxy = crate::settings::Settings::load().proxy;
        let params = grammers_client::sender::ConnectionParams {
            proxy_url: (!proxy.is_empty() && crate::settings::proxy_error(&proxy).is_none()).then_some(proxy),
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
        };
        tokio::spawn(update_loop(
            client.clone(),
            updates,
            self.authorized.clone(),
            self.login_update.clone(),
            self.out.clone(),
            self.refs.clone(),
            self.names.clone(),
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
        if authorized {
            self.after_login().await
        } else {
            self.out.send(Ev::NeedPhone);
            Ok(())
        }
    }

    /// Те саме, що робить grammers після звичайного входу: кешує себе та стан оновлень.
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
        self.out.send(Ev::LoggedIn(name));
        self.load_dialogs().await
    }

    /// Повідомлення, надіслані запитом, відповідь на який не йде в потік оновлень.
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
        let ids = |v: &[tl::enums::InputPeer]| v.iter().map(|p| PeerRef::from(p.clone()).id).collect::<Vec<_>>();
        let title = |t: &tl::enums::TextWithEntities| {
            let tl::enums::TextWithEntities::Entities(t) = t;
            t.text.clone()
        };
        let folders = res
            .filters
            .iter()
            .filter_map(|f| match f {
                tl::enums::DialogFilter::Filter(f) => Some(Folder {
                    id: f.id,
                    title: title(&f.title),
                    emoticon: f.emoticon.clone().unwrap_or_default(),
                    include: [ids(&f.pinned_peers), ids(&f.include_peers)].concat(),
                    pinned: ids(&f.pinned_peers),
                    exclude: ids(&f.exclude_peers),
                    contacts: f.contacts,
                    non_contacts: f.non_contacts,
                    groups: f.groups,
                    broadcasts: f.broadcasts,
                    bots: f.bots,
                    exclude_muted: f.exclude_muted,
                    exclude_read: f.exclude_read,
                    exclude_archived: f.exclude_archived,
                    shared: false,
                }),
                tl::enums::DialogFilter::Chatlist(f) => Some(Folder {
                    id: f.id,
                    title: title(&f.title),
                    emoticon: f.emoticon.clone().unwrap_or_default(),
                    include: [ids(&f.pinned_peers), ids(&f.include_peers)].concat(),
                    pinned: ids(&f.pinned_peers),
                    shared: true,
                    ..Default::default()
                }),
                tl::enums::DialogFilter::Default => None,
            })
            .collect();
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
        let mut iter = client.iter_dialogs();
        let mut chats = Vec::new();
        let mut peers = Vec::new();
        while chats.len() < DIALOG_LIMIT {
            let Some(d) = iter.next().await.map_err(|e| lf!("Чати: {e}", "Chats: {e}"))? else {
                break;
            };
            let peer = d.peer();
            self.refs.lock().unwrap().insert(peer.id(), d.peer_ref());
            let (unread, pinned, muted, archived, unread_mark, draft) = match &d.raw {
                tl::enums::Dialog::Dialog(x) => {
                    let tl::enums::PeerNotifySettings::Settings(n) = &x.notify_settings;
                    let muted = n.mute_until.is_some_and(|t| t as i64 > jiff::Timestamp::now().as_second());
                    (x.unread_count, x.pinned, muted, x.folder_id == Some(1), x.unread_mark, draft_text(&x.draft))
                }
                _ => (0, false, false, false, false, String::new()),
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
            });
            peers.push(peer.clone());
        }
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

    /// Конвертує повідомлення з історії: заодно ставить у чергу фото, аватарку й ім'я автора.
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

    /// Дія над чатом зі списку; при помилці перезавантажуємо діалоги, щоб скасувати оптимістичну зміну в UI.
    async fn chat_action<R: tl::RemoteCall>(&mut self, req: &R, what: &str) -> Result<(), String> {
        let client = self.client()?;
        if let Err(e) = client.invoke(req).await {
            let _ = self.load_dialogs().await;
            return Err(format!("{what}: {e}"));
        }
        Ok(())
    }

    async fn search(&self, peer: Option<PeerId>, query: String) -> Result<(), String> {
        let client = self.client()?;
        let mut hits = Vec::new();
        let err = |e| lf!("Пошук: {e}", "Search: {e}");
        match peer {
            Some(peer) => {
                let r = self.peer_ref(peer)?;
                let mut iter = client.search_messages(r).query(&query).limit(SEARCH_LIMIT);
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
        self.out.send(Ev::SearchResults { peer, query, hits });
        Ok(())
    }
}

/// Показ щойно надісланого повідомлення в UI (з фонових задач теж).
#[derive(Clone)]
struct Sent {
    out: Emitter,
    loader: Option<Loader>,
    names: Names,
}

impl Sent {
    fn emit(&self, peer: PeerId, m: &Message, reply_to: Option<i32>, opts: SendOpts) {
        self.emit_with(peer, m, reply_to, opts, None);
    }

    /// `own` — текст і сутності, з якими ми надсилали: замінюють неповні з відповіді сервера.
    fn emit_with(&self, peer: PeerId, m: &Message, reply_to: Option<i32>, opts: SendOpts, own: Option<(&str, &[tl::enums::MessageEntity])>) {
        // Заплановані повідомлення живуть в окремій історії — лише повідомляємо про них.
        if let Some(ts) = opts.schedule {
            let when = jiff::Timestamp::from_second(ts).map(|t| format!("{} {}", day(t), clock(t))).unwrap_or_default();
            return self.out.send(Ev::Notice(lf!("Заплановано на {when}", "Scheduled for {when}")));
        }
        if let Some(loader) = &self.loader {
            loader.message_photo(peer, m);
        }
        let mut msg = convert(m, &self.names);
        msg.peer = peer;
        msg.out = true;
        msg.reply_to = msg.reply_to.or(reply_to);
        if let Some((plain, ents)) = own
            && msg.text == plain
        {
            msg.spans = format::from_entities(plain, ents, 0);
        }
        self.out.send(Ev::NewMsg(msg));
    }
}

/// Читач файлу, що повідомляє UI про прогрес (не частіше ніж кожні 2%).
struct Progress<R> {
    inner: R,
    id: u64,
    done: u64,
    total: u64,
    reported: u64,
    out: Emitter,
}

impl<R: tokio::io::AsyncRead + Unpin> tokio::io::AsyncRead for Progress<R> {
    fn poll_read(
        mut self: std::pin::Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
        buf: &mut tokio::io::ReadBuf<'_>,
    ) -> std::task::Poll<std::io::Result<()>> {
        let before = buf.filled().len();
        let res = std::pin::Pin::new(&mut self.inner).poll_read(cx, buf);
        self.done += (buf.filled().len() - before) as u64;
        if self.done - self.reported >= (self.total / 50).max(1) || self.done == self.total {
            self.reported = self.done;
            self.out.send(Ev::UploadProgress { id: self.id, done: self.done, total: self.total });
        }
        res
    }
}

fn is_image(path: &std::path::Path) -> bool {
    let ext = path.extension().map(|e| e.to_string_lossy().to_lowercase()).unwrap_or_default();
    matches!(ext.as_str(), "jpg" | "jpeg" | "png" | "webp")
}

/// Стискає велике фото до `SEND_PHOTO_PX` у JPEG (якість 87). `None` — надсилати як є.
fn compress_photo(path: &std::path::Path) -> Option<PathBuf> {
    let len = std::fs::metadata(path).ok()?.len();
    let (w, h) = image::image_dimensions(path).ok()?;
    if w.max(h) <= SEND_PHOTO_PX && len <= 1_500_000 {
        return None;
    }
    let img = image::open(path).ok()?;
    let img = if w.max(h) > SEND_PHOTO_PX {
        img.resize(SEND_PHOTO_PX, SEND_PHOTO_PX, image::imageops::FilterType::Lanczos3)
    } else {
        img
    };
    let dir = data_dir().join("cache");
    std::fs::create_dir_all(&dir).ok()?;
    let stem = path.file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_default();
    let out = dir.join(format!("send-{}-{stem}.jpg", jiff::Timestamp::now().as_millisecond()));
    let file = std::io::BufWriter::new(std::fs::File::create(&out).ok()?);
    image::codecs::jpeg::JpegEncoder::new_with_quality(file, 87).encode_image(&img.to_rgb8()).ok()?;
    Some(out)
}

/// Завантаження одного файлу з прогресом, зсунутим на `base` у межах `total` усієї групи.
async fn upload(client: &Client, path: &std::path::Path, id: u64, base: u64, total: u64, out: &Emitter) -> Result<(Uploaded, u64), String> {
    let name = path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
    let err = |e: &dyn std::fmt::Display| format!("{name}: {e}");
    let file = tokio::fs::File::open(path).await.map_err(|e| err(&e))?;
    let len = file.metadata().await.map_err(|e| err(&e))?.len();
    let mut stream = Progress { inner: file, id, done: base, total: total.max(base + len), reported: base, out: out.clone() };
    let up = client.upload_stream(&mut stream, len as usize, name.clone()).await.map_err(|e| err(&e))?;
    Ok((up, len))
}

/// Надсилання групи файлів: фото — по одному або альбомами до 10 (зі стисненням),
/// решта — документами. Підпис і відповідь чіпляються до першого повідомлення.
#[allow(clippy::too_many_arguments)]
async fn send_files(
    client: &Client,
    r: PeerRef,
    id: u64,
    paths: Vec<PathBuf>,
    caption: String,
    reply_to: Option<i32>,
    opts: SendOpts,
    as_file: bool,
    sent: &Sent,
) -> Result<(), String> {
    let (photos, docs): (Vec<PathBuf>, Vec<PathBuf>) = paths.into_iter().partition(|p| !as_file && is_image(p));
    let photos: Vec<PathBuf> = tokio::task::spawn_blocking(move || {
        photos.into_iter().map(|p| compress_photo(&p).unwrap_or(p)).collect()
    })
    .await
    .map_err(|e| e.to_string())?;
    let total: u64 = photos.iter().chain(&docs).filter_map(|p| std::fs::metadata(p).ok()).map(|m| m.len()).sum();
    let mut base = 0;
    let mut first = Some((caption, reply_to));

    for chunk in photos.chunks(10) {
        let (cap, reply) = first.take().unwrap_or_default();
        if chunk.len() == 1 {
            let (up, len) = upload(client, &chunk[0], id, base, total, &sent.out).await?;
            base += len;
            let m = client.send_message(r, opts.apply(rich(&cap).reply_to(reply)).photo(up)).await.map_err(|e| e.to_string())?;
            sent.emit(r.id, &m, reply, opts);
            continue;
        }
        let mut medias = Vec::new();
        for (i, path) in chunk.iter().enumerate() {
            let (up, len) = upload(client, path, id, base, total, &sent.out).await?;
            base += len;
            let mut media = InputMedia::new().photo(up);
            if i == 0 {
                let (text, entities) = format::parse(&cap);
                media = media.caption(text).fmt_entities(entities).reply_to(reply);
            }
            medias.push(media);
        }
        let msgs = client.send_album(r, medias).await.map_err(|e| lf!("Альбом: {e}", "Album: {e}"))?;
        for m in msgs.iter().flatten() {
            sent.emit(r.id, m, reply, opts);
        }
    }
    for path in &docs {
        let (cap, reply) = first.take().unwrap_or_default();
        let (up, len) = upload(client, path, id, base, total, &sent.out).await?;
        base += len;
        let msg = opts.apply(rich(&cap).reply_to(reply)).document(up);
        let m = client.send_message(r, msg).await.map_err(|e| e.to_string())?;
        sent.emit(r.id, &m, reply, opts);
    }
    Ok(())
}

fn inline_result(r: tl::enums::BotInlineResult) -> InlineResult {
    match r {
        tl::enums::BotInlineResult::Result(r) => InlineResult {
            id: r.id,
            title: r.title.unwrap_or_default(),
            description: r.description.or(r.url).unwrap_or_default(),
            kind: r.r#type,
        },
        tl::enums::BotInlineResult::BotInlineMediaResult(r) => InlineResult {
            id: r.id,
            title: r.title.unwrap_or_default(),
            description: r.description.unwrap_or_default(),
            kind: r.r#type,
        },
    }
}

/// Сире повідомлення (заплановані приходять без мапи учасників) → `Msg`.
fn convert_raw(peer: PeerId, m: &tl::types::Message) -> Msg {
    let ts = jiff::Timestamp::from_second(m.date as i64).unwrap_or_default();
    let text = if m.message.is_empty() && m.media.is_some() { l!("📎 Вкладення", "📎 Attachment").to_owned() } else { m.message.clone() };
    let spans = if m.message.is_empty() { Vec::new() } else { format::from_entities(&m.message, m.entities.as_deref().unwrap_or_default(), 0) };
    Msg { id: m.id, out: true, text, time: clock(ts), ts: m.date as i64, day: day(ts), spans, ..Msg::placeholder(peer) }
}

/// Текст із поля вводу з розміткою → повідомлення з сутностями.
fn rich(markup: &str) -> InputMessage {
    let (text, entities) = format::parse(markup);
    InputMessage::new().text(text).fmt_entities(entities)
}

fn dialog_peer(r: PeerRef) -> tl::enums::InputDialogPeer {
    tl::types::InputDialogPeer { peer: r.into() }.into()
}

fn remember_name(names: &Names, m: &Message) {
    if let Some(s) = m.sender() {
        names.lock().unwrap().entry(s.id()).or_insert_with(|| peer_title(s));
    }
}

fn draft_text(d: &Option<tl::enums::DraftMessage>) -> String {
    match d {
        Some(tl::enums::DraftMessage::Message(d)) => {
            let spans = format::from_entities(&d.message, d.entities.as_deref().unwrap_or_default(), 0);
            format::to_markup(&d.message, &spans)
        }
        _ => String::new(),
    }
}

fn is_login_token_update(u: &UpdatesLike) -> bool {
    let is = |x: &tl::enums::Update| matches!(x, tl::enums::Update::LoginToken);
    match u {
        UpdatesLike::Updates(tl::enums::Updates::UpdateShort(s)) => is(&s.update),
        UpdatesLike::Updates(tl::enums::Updates::Updates(s)) => s.updates.iter().any(is),
        UpdatesLike::Updates(tl::enums::Updates::Combined(s)) => s.updates.iter().any(is),
        _ => false,
    }
}

async fn update_loop(
    client: Client,
    mut updates: tmpsc::Receiver<UpdatesLike>,
    authorized: Arc<Notify>,
    login_update: Arc<Notify>,
    out: Emitter,
    refs: Refs,
    names: Names,
    loader: Loader,
) {
    // До авторизації слухаємо лише updateLoginToken (сигнал, що QR відскановано).
    loop {
        tokio::select! {
            _ = authorized.notified() => break,
            u = updates.recv() => match u {
                Some(u) if is_login_token_update(&u) => login_update.notify_one(),
                Some(_) => {}
                None => return,
            },
        }
    }
    let mut stream = match client.stream_updates(updates, UpdatesConfiguration { catch_up: false }).await {
        Ok(s) => s,
        Err(e) => return out.send(Ev::Error(lf!("Оновлення: {e}", "Updates: {e}"))),
    };
    loop {
        match stream.next().await {
            Ok(Update::NewMessage(m)) => {
                if let Ok(Some(r)) = m.peer_ref().await {
                    refs.lock().unwrap().insert(r.id, r);
                }
                loader.message_photo(m.peer_id(), &m);
                remember_name(&names, &m);
                if let Some(s) = m.sender() {
                    out.send(Ev::Typing { peer: m.peer_id(), who: short_name(s), active: false });
                }
                out.send(Ev::NewMsg(convert(&m, &names)));
            }
            Ok(Update::MessageEdited(m)) => out.send(Ev::Edited(convert(&m, &names))),
            Ok(Update::MessageDeleted(d)) => {
                let peer = d.channel_id().and_then(PeerId::channel);
                out.send(Ev::Deleted { ids: d.into_messages(), peer })
            }
            Ok(Update::Raw(raw)) => {
                if let Some(ev) = raw_event(&raw.raw, &names) {
                    out.send(ev);
                }
            }
            Ok(_) => {}
            Err(e) => {
                out.send(Ev::Error(lf!("З'єднання втрачено: {e}", "Connection lost: {e}")));
                break;
            }
        }
    }
}

/// Сирі оновлення, для яких у grammers немає обгорток: «друкує», статуси, прочитання, чернетки.
fn raw_event(u: &tl::enums::Update, names: &Names) -> Option<Ev> {
    use tl::enums::{SendMessageAction as A, Update as U};
    let typing = |peer: PeerId, from: PeerId, action: &A| {
        let who = names.lock().unwrap().get(&from).cloned().unwrap_or_default();
        Ev::Typing { peer, who, active: !matches!(action, A::SendMessageCancelAction) }
    };
    Some(match u {
        U::UserTyping(t) => {
            let peer = PeerId::user(t.user_id)?;
            typing(peer, peer, &t.action)
        }
        U::ChatUserTyping(t) => typing(PeerId::chat(t.chat_id)?, PeerId::from(&t.from_id), &t.action),
        U::ChannelUserTyping(t) => typing(PeerId::channel(t.channel_id)?, PeerId::from(&t.from_id), &t.action),
        U::UserStatus(s) => Ev::Presence { peer: PeerId::user(s.user_id)?, presence: Presence::from_status(&s.status) },
        U::ReadHistoryInbox(r) => Ev::ReadInbox { peer: PeerId::from(&r.peer), unread: r.still_unread_count },
        U::ReadChannelInbox(r) => Ev::ReadInbox { peer: PeerId::channel(r.channel_id)?, unread: r.still_unread_count },
        U::ReadMessagesContents(r) => Ev::Listened { peer: None, ids: r.messages.clone() },
        U::ChannelReadMessagesContents(r) => Ev::Listened { peer: Some(PeerId::channel(r.channel_id)?), ids: r.messages.clone() },
        U::MessageReactions(r) => Ev::Reactions { peer: PeerId::from(&r.peer), id: r.msg_id, reactions: reactions(&r.reactions) },
        U::DraftMessage(d) if d.top_msg_id.is_none() => Ev::Draft { peer: PeerId::from(&d.peer), text: draft_text(&Some(d.draft.clone())) },
        _ => return None,
    })
}

/// Вхід через QR: показуємо токен, чекаємо сканування, обробляємо міграцію DC та 2FA.
async fn qr_loop(
    client: Client,
    session: Arc<SqliteSession>,
    api_id: i32,
    api_hash: String,
    login_update: Arc<Notify>,
    out: Emitter,
    back: tmpsc::UnboundedSender<Cmd>,
) {
    use tl::enums::auth::LoginToken as T;
    let export = tl::functions::auth::ExportLoginToken { api_id, api_hash, except_ids: Vec::new() };
    let mut result = client.invoke(&export).await;
    loop {
        match result {
            Ok(T::Token(t)) => {
                let token = base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(&t.token);
                out.send(Ev::Qr(format!("tg://login?token={token}")));
                let left = (t.expires as i64 - jiff::Timestamp::now().as_second()).clamp(3, 30);
                tokio::select! {
                    _ = login_update.notified() => {}
                    _ = tokio::time::sleep(Duration::from_secs(left as u64)) => {}
                }
                result = client.invoke(&export).await;
            }
            Ok(T::MigrateTo(m)) => {
                if let Err(e) = session.set_home_dc_id(m.dc_id).await {
                    return out.send(Ev::Error(lf!("Міграція DC: {e}", "DC migration: {e}")));
                }
                result = client.invoke(&tl::functions::auth::ImportLoginToken { token: m.token }).await;
            }
            Ok(T::Success(s)) => {
                if let tl::enums::auth::Authorization::Authorization(a) = s.authorization {
                    let _ = back.send(Cmd::QrSuccess(Box::new(a)));
                }
                return;
            }
            Err(e) if e.is("SESSION_PASSWORD_NEEDED") => {
                match client.invoke(&tl::functions::account::GetPassword {}).await {
                    Ok(pw) => {
                        let tl::enums::account::Password::Password(pw) = pw;
                        let _ = back.send(Cmd::QrPassword(PasswordToken::new(pw)));
                    }
                    Err(e) => out.send(Ev::Error(format!("2FA: {e}"))),
                }
                return;
            }
            Err(e) => return out.send(Ev::Error(lf!("QR-вхід: {e}", "QR login: {e}"))),
        }
    }
}

fn peer_title(peer: &Peer) -> String {
    match peer {
        Peer::User(u) if u.is_self() => l!("Збережене", "Saved Messages").to_owned(),
        Peer::User(u) if u.deleted() => l!("Видалений акаунт", "Deleted account").to_owned(),
        Peer::User(u) => u.full_name(),
        other => other.name().unwrap_or(l!("Без назви", "Untitled")).to_owned(),
    }
}

fn short_name(peer: &Peer) -> String {
    match peer {
        Peer::User(u) => u.first_name().unwrap_or("?").to_owned(),
        other => other.name().unwrap_or("?").to_owned(),
    }
}

fn peer_kind(peer: &Peer) -> ChatKind {
    match peer {
        Peer::User(u) if u.is_self() => ChatKind::Saved,
        Peer::User(u) if u.is_bot() => ChatKind::Bot,
        Peer::User(_) => ChatKind::User,
        Peer::Channel(_) => ChatKind::Channel,
        _ => ChatKind::Group,
    }
}

fn media_label(m: &Message) -> Option<&'static str> {
    Some(match m.media()? {
        Media::Photo(_) => l!("📷 Фото", "📷 Photo"),
        Media::Document(_) => match media_info(m).map(|i| i.kind) {
            Some(MediaKind::Voice) => l!("🎤 Голосове повідомлення", "🎤 Voice message"),
            Some(MediaKind::Round) => l!("📹 Відеоповідомлення", "📹 Video message"),
            Some(MediaKind::Video) => l!("🎬 Відео", "🎬 Video"),
            Some(MediaKind::Gif) => "GIF",
            Some(MediaKind::Audio) => l!("🎵 Аудіо", "🎵 Audio"),
            None => l!("📎 Файл", "📎 File"),
        },
        Media::Sticker(_) => l!("Стікер", "Sticker"),
        Media::Contact(_) => l!("👤 Контакт", "👤 Contact"),
        Media::Poll(_) => l!("📊 Опитування", "📊 Poll"),
        Media::Geo(_) | Media::GeoLive(_) | Media::Venue(_) => l!("📍 Локація", "📍 Location"),
        Media::Dice(_) => "🎲",
        Media::WebPage(_) => return None,
        #[allow(unreachable_patterns)]
        _ => l!("Вкладення", "Attachment"),
    })
}

fn preview_text(m: &Message) -> String {
    let text = m.text().replace('\n', " ");
    match (media_label(m), text.is_empty()) {
        (Some(label), true) => label.to_owned(),
        (Some(label), false) => format!("{label} · {text}"),
        (None, true) if m.action().is_some() => l!("Сервісне повідомлення", "Service message").to_owned(),
        _ => text,
    }
}

/// Кнопки під повідомленням (інлайн-клавіатура або звичайна клавіатура бота).
fn buttons(m: &Message) -> Vec<Vec<Button>> {
    use tl::enums::{InlineButtonType as T, KeyboardButton as K, KeyboardInlineButton as I, ReplyMarkup as R};
    match m.reply_markup() {
        Some(R::ReplyInlineMarkup(k)) => k
            .rows
            .into_iter()
            .map(|tl::enums::KeyboardInlineButtonRow::Row(row)| {
                row.buttons
                    .into_iter()
                    .map(|I::Button(b)| Button {
                        text: b.text,
                        action: match b.r#type {
                            T::Callback(c) => ButtonAction::Callback(c.data),
                            T::Url(u) => ButtonAction::Url(u.url),
                            T::UrlAuth(u) => ButtonAction::Url(u.url),
                            T::WebView(u) => ButtonAction::Url(u.url),
                            T::Copy(c) => ButtonAction::Copy(c.copy_text),
                            _ => ButtonAction::Other,
                        },
                    })
                    .collect()
            })
            .collect(),
        Some(R::ReplyKeyboardMarkup(k)) => k
            .rows
            .into_iter()
            .map(|tl::enums::KeyboardButtonRow::Row(row)| {
                row.buttons
                    .into_iter()
                    .map(|K::Button(b)| Button {
                        action: if matches!(b.r#type, tl::enums::ButtonType::Default) { ButtonAction::Text } else { ButtonAction::Other },
                        text: b.text,
                    })
                    .collect()
            })
            .collect(),
        _ => Vec::new(),
    }
}

fn forwarded(m: &Message, names: &Names) -> Option<String> {
    let tl::enums::MessageFwdHeader::Header(h) = m.forward_header()?;
    let name = h
        .from_name
        .clone()
        .or_else(|| h.from_id.as_ref().and_then(|p| names.lock().unwrap().get(&PeerId::from(p)).cloned()))
        .unwrap_or_else(|| l!("невідомого", "unknown").to_owned());
    Some(match h.post_author {
        Some(author) => format!("{name} ({author})"),
        None => name,
    })
}

fn convert(m: &Message, names: &Names) -> Msg {
    let photo = matches!(m.media(), Some(Media::Photo(_)));
    // Опитування, місця й контакти малюються картками — мітка в тексті не потрібна.
    let card = matches!(m.media(), Some(Media::Poll(_) | Media::Geo(_) | Media::GeoLive(_) | Media::Venue(_) | Media::Contact(_)));
    let text = match (media_label(m), m.text().is_empty()) {
        _ if photo || card => m.text().to_owned(),
        (Some(label), true) => label.to_owned(),
        (Some(label), false) => format!("{label}\n{}", m.text()),
        (None, true) if m.action().is_some() => l!("Сервісне повідомлення", "Service message").to_owned(),
        _ => m.text().to_owned(),
    };
    let sticker = match m.media() {
        Some(Media::Sticker(s)) => Some((s.document.id(), s.emoji().to_owned())),
        _ => None,
    };
    let media = media_info(m);
    let file = match m.media() {
        Some(Media::Document(d)) if media.is_none() => Some((
            d.name().filter(|n| !n.is_empty()).unwrap_or(l!("Файл", "File")).to_owned(),
            d.size().map(human_size).unwrap_or_default(),
        )),
        _ => None,
    };
    let text = if sticker.is_some() || file.is_some() || media.is_some() { m.text().to_owned() } else { text };
    let spans = match (m.fmt_entities(), text.strip_suffix(m.text())) {
        (Some(ents), Some(prefix)) if !m.text().is_empty() => format::from_entities(&text, ents, prefix.len()),
        _ => Vec::new(),
    };
    Msg {
        id: m.id(),
        peer: m.peer_id(),
        out: m.outgoing(),
        sender: m.sender().map(peer_title).unwrap_or_default(),
        text,
        time: clock(m.date()),
        ts: m.date().as_second(),
        day: day(m.date()),
        edited: m.edit_date().is_some(),
        reply_to: m.reply_to_message_id(),
        photo,
        file,
        sticker,
        spans,
        forwarded: forwarded(m, names),
        buttons: buttons(m),
        group: m.grouped_id(),
        reactions: match &m.raw {
            tl::enums::Message::Message(x) => x.reactions.as_ref().map(reactions).unwrap_or_default(),
            _ => Vec::new(),
        },
        poll: match m.media() {
            Some(Media::Poll(p)) => Some(poll_info(&p)),
            _ => None,
        },
        geo: geo_info(m),
        contact: match m.media() {
            Some(Media::Contact(c)) => Some((format!("{} {}", c.first_name(), c.last_name()).trim().to_owned(), c.phone_number().to_owned())),
            _ => None,
        },
        media,
        video_sticker: matches!(m.media(), Some(Media::Sticker(_))) && raw_document(m).is_some_and(|d| d.mime_type == "video/webm"),
        media_unread: matches!(&m.raw, tl::enums::Message::Message(x) if x.media_unread),
        topic: message_topic(m),
        comments: match &m.raw {
            tl::enums::Message::Message(x) => match &x.replies {
                Some(tl::enums::MessageReplies::Replies(r)) if r.comments => Some(r.replies),
                _ => None,
            },
            _ => None,
        },
    }
}

/// Тема форуму з заголовка відповіді; повідомлення, що створило тему, належить їй самій.
fn message_topic(m: &Message) -> Option<i32> {
    match &m.raw {
        tl::enums::Message::Message(x) => match &x.reply_to {
            Some(tl::enums::MessageReplyHeader::Header(h)) if h.forum_topic => h.reply_to_top_id.or(h.reply_to_msg_id),
            _ => None,
        },
        tl::enums::Message::Service(x) if matches!(x.action, tl::enums::MessageAction::TopicCreate(_)) => Some(x.id),
        _ => None,
    }
}

/// Чат, знайдений поза списком діалогів (за username чи в пошуку): ще не учасник.
fn found_chat(peer: &Peer, last_text: String) -> ChatInfo {
    ChatInfo {
        id: peer.id(),
        title: peer_title(peer),
        kind: peer_kind(peer),
        last_text,
        last_time: String::new(),
        last_ts: 0,
        unread: 0,
        pinned: false,
        contact: matches!(peer, Peer::User(u) if u.contact()),
        muted: false,
        archived: false,
        unread_mark: false,
        draft: String::new(),
        presence: match peer {
            Peer::User(u) if !u.is_bot() => Presence::from_status(u.status()),
            _ => Presence::None,
        },
        forum: matches!(peer, Peer::Group(g) if matches!(&g.raw, tl::enums::Chat::Channel(c) if c.forum)),
        // Користувачам і ботам вступати не треба.
        joined: matches!(peer, Peer::User(_)),
        can_post: can_post(peer),
    }
}

fn can_post(peer: &Peer) -> bool {
    match peer {
        Peer::Channel(c) => c.raw.creator || matches!(&c.raw.admin_rights, Some(tl::enums::ChatAdminRights::Rights(r)) if r.post_messages),
        _ => true,
    }
}

/// Тип медіа документа за його атрибутами.
fn media_info(m: &Message) -> Option<MediaInfo> {
    let Some(Media::Document(d)) = m.media() else { return None };
    let doc = raw_document(m)?;
    let mut info = MediaInfo {
        kind: MediaKind::Video,
        duration: 0.0,
        size: (0, 0),
        title: d.name().unwrap_or_default().to_owned(),
        performer: String::new(),
        waveform: Vec::new(),
    };
    let mut kind = None;
    let animated = doc.attributes.iter().any(|a| matches!(a, tl::enums::DocumentAttribute::Animated));
    let has_video = doc.attributes.iter().any(|a| matches!(a, tl::enums::DocumentAttribute::Video(_)));
    for a in &doc.attributes {
        match a {
            tl::enums::DocumentAttribute::Video(v) => {
                info.duration = v.duration;
                info.size = (v.w, v.h);
                kind = Some(if animated { MediaKind::Gif } else if v.round_message { MediaKind::Round } else { MediaKind::Video });
            }
            // Сервер додає аудіоатрибут і відео зі звуком (зокрема кружечкам) — відео важливіше.
            tl::enums::DocumentAttribute::Audio(a) if !has_video => {
                info.duration = a.duration as f64;
                info.title = a.title.clone().filter(|t| !t.is_empty()).unwrap_or(info.title);
                info.performer = a.performer.clone().unwrap_or_default();
                info.waveform = a.waveform.as_deref().map(unpack_waveform).unwrap_or_default();
                kind = Some(if a.voice { MediaKind::Voice } else { MediaKind::Audio });
            }
            _ => {}
        }
    }
    if kind.is_none() && doc.mime_type == "image/gif" {
        kind = Some(MediaKind::Gif);
    }
    info.kind = kind?;
    Some(info)
}

/// Хвильова форма Telegram: 5-бітні значення, впаковані підряд.
fn unpack_waveform(bytes: &[u8]) -> Vec<u8> {
    let n = bytes.len() * 8 / 5;
    (0..n)
        .map(|i| {
            let bit = i * 5;
            let (byte, shift) = (bit / 8, bit % 8);
            let lo = bytes[byte] as u16;
            let hi = bytes.get(byte + 1).copied().unwrap_or(0) as u16;
            (((hi << 8 | lo) >> shift) & 31) as u8
        })
        .collect()
}

/// Зворотне до `unpack_waveform`: рівні 0..31 у 5-бітний потік.
fn pack_waveform(levels: &[u8]) -> Vec<u8> {
    let mut out = vec![0u8; (levels.len() * 5).div_ceil(8)];
    for (i, &v) in levels.iter().enumerate() {
        let (bit, v) = (i * 5, (v.min(31)) as u16);
        let (byte, shift) = (bit / 8, bit % 8);
        out[byte] |= (v << shift) as u8;
        if shift > 3 {
            out[byte + 1] |= (v >> (8 - shift)) as u8;
        }
    }
    out
}

/// Завантажує медіа повідомлення в кеш (один раз) і повертає шлях.
async fn cache_media(client: &Client, r: PeerRef, id: i32) -> Result<PathBuf, String> {
    let dir = data_dir().join("media");
    let _ = tokio::fs::create_dir_all(&dir).await;
    let msg = client
        .get_messages_by_id(r, &[id])
        .await
        .map_err(|e| lf!("Медіа: {e}", "Media: {e}"))?
        .into_iter()
        .flatten()
        .next()
        .ok_or(l!("Повідомлення не знайдено", "Message not found"))?;
    let media = msg.media().ok_or(l!("Немає медіа", "No media"))?;
    let ext = match &media {
        Media::Document(d) => match d.mime_type() {
            Some("video/webm") => "webm",
            Some("audio/ogg") => "ogg",
            Some("audio/mpeg") => "mp3",
            Some("image/gif") => "gif",
            Some(m) if m.starts_with("audio/") => "m4a",
            _ => "mp4",
        },
        Media::Sticker(_) => "webm",
        _ => "bin",
    };
    let path = dir.join(format!("{}_{id}.{ext}", r.id.bot_api_dialog_id_unchecked()));
    if tokio::fs::metadata(&path).await.is_err() {
        let part = path.with_extension("part");
        client.download_media(&media, &part).await.map_err(|e| lf!("Медіа: {e}", "Media: {e}"))?;
        tokio::fs::rename(&part, &path).await.map_err(|e| e.to_string())?;
    }
    Ok(path)
}

#[cfg(test)]
mod tests {
    #[test]
    fn waveform_unpacks_5bit_values() {
        // 31, 0, 31, 0 у 5-бітному пакуванні.
        let packed = [0b0001_1111, 0b0111_1100, 0b0000_0000];
        assert_eq!(&super::unpack_waveform(&packed)[..4], &[31, 0, 31, 0]);
    }

    #[test]
    fn waveform_roundtrip() {
        let levels: Vec<u8> = (0..100).map(|i| (i * 7 % 32) as u8).collect();
        assert_eq!(super::unpack_waveform(&super::pack_waveform(&levels))[..100], levels[..]);
    }
}

fn reactions(r: &tl::enums::MessageReactions) -> Vec<Reaction> {
    let tl::enums::MessageReactions::Reactions(r) = r;
    r.results
        .iter()
        .filter_map(|tl::enums::ReactionCount::Count(c)| match &c.reaction {
            tl::enums::Reaction::Emoji(e) => Some(Reaction { emoji: e.emoticon.clone(), count: c.count, mine: c.chosen_order.is_some() }),
            _ => None,
        })
        .collect()
}

fn poll_info(p: &grammers_client::media::Poll) -> PollInfo {
    let text = |t: &tl::enums::TextWithEntities| {
        let tl::enums::TextWithEntities::Entities(t) = t;
        t.text.clone()
    };
    let voters: Vec<&tl::types::PollAnswerVoters> = p.iter_voters_summary().map(|v| v.collect()).unwrap_or_default();
    let answers = p
        .iter_answers()
        .filter_map(|a| match a {
            tl::enums::PollAnswer::Answer(a) => Some(a),
            _ => None,
        })
        .map(|a| {
            let v = voters.iter().find(|v| v.option == a.option);
            PollOption {
                text: text(&a.text),
                option: a.option.clone(),
                voters: v.and_then(|v| v.voters).unwrap_or(0),
                chosen: v.is_some_and(|v| v.chosen),
                correct: v.is_some_and(|v| v.correct),
            }
        })
        .collect();
    PollInfo {
        question: text(p.question()),
        answers,
        total: p.total_voters().unwrap_or(0),
        closed: p.closed(),
        quiz: p.is_quiz(),
        multiple: p.raw.multiple_choice,
    }
}

fn geo_info(m: &Message) -> Option<GeoInfo> {
    let point = |g: &grammers_client::media::Geo| (g.latitue(), g.longitude());
    let (lat, lon, title, address, live) = match m.media()? {
        Media::Geo(g) => {
            let (lat, lon) = point(&g);
            (lat, lon, String::new(), String::new(), false)
        }
        Media::GeoLive(g) => {
            let (lat, lon) = point(g.geo.as_ref()?);
            (lat, lon, String::new(), String::new(), true)
        }
        Media::Venue(v) => {
            let (lat, lon) = point(v.geo.as_ref()?);
            (lat, lon, v.title().to_owned(), v.address().to_owned(), false)
        }
        _ => return None,
    };
    Some(GeoInfo { lat, lon, title, address, live })
}

fn local(ts: jiff::Timestamp) -> jiff::Zoned {
    ts.to_zoned(jiff::tz::TimeZone::system())
}

fn clock(ts: jiff::Timestamp) -> String {
    local(ts).strftime("%H:%M").to_string()
}

fn short_time(ts: jiff::Timestamp) -> String {
    let z = local(ts);
    let now = jiff::Zoned::now();
    if z.date() == now.date() {
        z.strftime("%H:%M").to_string()
    } else if z.year() == now.year() {
        z.strftime("%d.%m").to_string()
    } else {
        z.strftime("%d.%m.%y").to_string()
    }
}

const MONTHS_UK: [&str; 12] = [
    "січня", "лютого", "березня", "квітня", "травня", "червня",
    "липня", "серпня", "вересня", "жовтня", "листопада", "грудня",
];
const MONTHS_EN: [&str; 12] = ["Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec"];

fn day(ts: jiff::Timestamp) -> String {
    let z = local(ts);
    let now = jiff::Zoned::now();
    let months = if crate::i18n::en() { &MONTHS_EN } else { &MONTHS_UK };
    let base = format!("{} {}", z.day(), months[(z.month() - 1) as usize]);
    if z.date() == now.date() {
        l!("Сьогодні", "Today").to_owned()
    } else if z.year() == now.year() {
        base
    } else {
        format!("{base} {}", z.year())
    }
}

fn log_error(e: &str) {
    eprintln!("glassgram: {e}");
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

fn downloads_dir() -> PathBuf {
    std::process::Command::new("xdg-user-dir")
        .arg("DOWNLOAD")
        .output()
        .ok()
        .map(|o| PathBuf::from(String::from_utf8_lossy(&o.stdout).trim()))
        .filter(|p| p.is_dir())
        .or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join("Downloads")))
        .unwrap_or_else(|| PathBuf::from("."))
}

/// Завантажує файл або фото з повідомлення в теку «Завантаження» і відкриває його.
async fn download(client: &Client, r: PeerRef, id: i32) -> Result<PathBuf, String> {
    let msg = client
        .get_messages_by_id(r, &[id])
        .await
        .map_err(|e| lf!("Завантаження: {e}", "Download: {e}"))?
        .into_iter()
        .flatten()
        .next()
        .ok_or(l!("Повідомлення не знайдено", "Message not found"))?;
    let media = msg.media().ok_or(l!("У повідомленні немає файлу", "The message has no file"))?;
    let name = match &media {
        Media::Document(d) => d.name().filter(|n| !n.is_empty()).map(str::to_owned),
        _ => None,
    }
    .unwrap_or_else(|| format!("telegram_{}_{id}.jpg", r.id.bot_api_dialog_id_unchecked()));
    let dir = downloads_dir();
    let _ = std::fs::create_dir_all(&dir);
    let mut path = dir.join(&name);
    let mut n = 1;
    while path.exists() {
        let stem = std::path::Path::new(&name).file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_default();
        let ext = std::path::Path::new(&name).extension().map(|e| format!(".{}", e.to_string_lossy())).unwrap_or_default();
        path = dir.join(format!("{stem} ({n}){ext}"));
        n += 1;
    }
    client.download_media(&media, &path).await.map_err(|e| lf!("Завантаження: {e}", "Download: {e}"))?;
    let _ = std::process::Command::new("xdg-open").arg(&path).spawn();
    Ok(path)
}
