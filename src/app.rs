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
    SearchHit, SendOpts, Session, Topic, GENERAL_TOPIC, TOPIC_COLORS,
    StickerSetInfo,
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
mod chatedit;
mod birthdays;
mod botstart;
mod similar;
mod saved;
use chatlist::{NewChat, Reorder, sort_chats};

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
    edited_at: f64,
    requested: String,
    hits: Vec<SearchHit>,
    hits_for: String,
}

impl Search {
    /// Whether it's time to send the request (after a pause in typing).
    fn due(&self, now: f64) -> Option<String> {
        let q = self.query.trim();
        (q.chars().count() >= 2 && q != self.requested && now - self.edited_at > SEARCH_DELAY).then(|| q.to_owned())
    }

    fn accept(&mut self, query: String, hits: Vec<SearchHit>) {
        if query == self.query.trim() {
            self.hits = hits;
            self.hits_for = query;
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
    api_id: String,
    api_hash: String,
    phone: String,
    code: String,
    password: String,
    /// (text, time shown, is error)
    toast: Option<(String, f64, bool)>,
    me: String,
    chats: Vec<ChatInfo>,
    selected: Option<PeerId>,
    history: HashMap<PeerId, Vec<Msg>>,
    exhausted: HashMap<PeerId, bool>,
    textures: HashMap<ImgKey, TextureHandle>,
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
    /// (chat, message, when the jump started)
    jump: Option<(PeerId, i32, f64)>,
    revealed: HashSet<(PeerId, i32)>,
    compose: Compose,
    folders: Vec<Folder>,
    folder: Option<i32>,
    stickers: Vec<StickerSetInfo>,
    sticker_panel: bool,
    sticker_set: usize,
    sticker_thumbs: HashSet<i64>,
    stickers_requested: bool,
    uploads: Vec<Upload>,
    next_upload: u64,
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
    /// Emoji tab in the sticker panel.
    emoji_tab: bool,
    /// Options ticked but not yet sent in multiple-answer polls.
    multi_vote: HashMap<(PeerId, i32), Vec<Vec<u8>>>,
    settings: Settings,
    settings_open: bool,
    title_unread: usize,
    /// Files from the system dialog: (chat, files, uncompressed).
    picked_tx: mpsc::Sender<(PeerId, Vec<PathBuf>, bool)>,
    picked_rx: mpsc::Receiver<(PeerId, Vec<PathBuf>, bool)>,
    player: Player,
    /// Whether `ffplay` is available for sound.
    can_play: bool,
    /// Media already downloaded to the cache.
    media: HashMap<(PeerId, i32), PathBuf>,
    /// Media being downloaded, and what to do afterwards: `true` means open in the system player.
    media_wanted: HashMap<(PeerId, i32), bool>,
    /// Media being downloaded for autoplay (GIFs, round videos, video stickers).
    media_loading: HashSet<(PeerId, i32)>,
    /// Video frames currently on screen.
    clips: HashMap<(PeerId, i32), Clip>,
    /// Messages with video visible in this frame (filled during drawing).
    clip_wanted: RefCell<Vec<(PeerId, i32)>>,
    /// Measured heights of history rows (keyed by the row's first message), for virtualization.
    row_h: RefCell<HashMap<(PeerId, i32), f32>>,
    /// Voice message recording into a chat.
    recorder: Option<(PeerId, Recorder)>,
    /// Recorded but not yet sent voice message (preview playback).
    voice_draft: Option<(PeerId, Voice)>,
    /// Video message recording: chat, process, preview texture and number of the last frame.
    round_rec: Option<(PeerId, RoundRecorder)>,
    round_tex: Option<TextureHandle>,
    round_seen: u64,
    can_record: bool,
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
    /// Demo mode: no network and no account switching.
    demo: bool,
    /// Input field height in the last frame; space is reserved for it at the bottom of the history.
    composer_h: f32,
    /// Forum topics and the open topic in the current chat (`None` means the topic list).
    topics: HashMap<PeerId, Vec<Topic>>,
    topic: Option<i32>,
    topic_exhausted: HashSet<(PeerId, i32)>,
    comments: Option<CommentsView>,
    /// Public chats from the global search by name.
    public_hits: Vec<ChatInfo>,
    /// Export window: chat and whether to export media.
    export_ask: Option<(PeerId, bool)>,
    /// Chats being exported right now.
    exporting: HashSet<PeerId>,
    /// New topic or rename window: (chat, topic or `None` for a new one, name, color).
    topic_edit: Option<(PeerId, Option<i32>, String, u32)>,
    settings_tab: SettingsTab,
    /// Cameras for settings (looked up when the tab is opened).
    cameras: Option<Vec<camera::Camera>>,
    sessions: Vec<Session>,
    privacy: HashMap<PrivacyKey, PrivacyState>,
    /// A second press confirms "Terminate all other sessions".
    confirm_terminate: bool,
    /// Local passcode fields: new, repeat, current (for disabling).
    pass_fields: [String; 3],
    /// The interface is locked by a local passcode.
    locked: bool,
    lock_input: String,
    lock_error: bool,
    last_activity: f64,
    /// Pinned messages per chat (oldest first) and which one the bar shows.
    pins: HashMap<PeerId, Vec<Msg>>,
    pin_shown: HashMap<PeerId, i32>,
    /// The list of all pinned messages is open.
    pins_open: bool,
    /// Bar hidden by users who cannot unpin — until something new is pinned.
    pins_hidden: HashSet<PeerId>,
    /// Ids of unread mentions (`false`) and reactions (`true`), oldest first.
    marks: HashMap<(PeerId, bool), Vec<i32>>,
    /// The "@"/"❤" button was pressed while the list was still loading.
    mark_wanted: Option<(PeerId, bool)>,
    /// History is scrolled to the end (as of last frame); request to scroll down.
    at_bottom: bool,
    scroll_bottom: bool,
    /// New messages that arrived while history was scrolled up.
    unseen_below: usize,
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
    /// "Always" / "never" lists of each privacy rule.
    privacy_ex: HashMap<PrivacyKey, backend::PrivacyExceptions>,
    /// The rule and side whose exception list is unfolded.
    privacy_open: Option<(PrivacyKey, bool)>,
    privacy_pick: Option<privacy::PrivacyPick>,
    /// Global privacy switches (`None` — not loaded yet).
    global_privacy: Option<backend::GlobalPrivacy>,
    /// "New poll" box.
    poll_form: Option<polls::PollForm>,
    /// Open mini app windows (separate WebView processes).
    web_apps: Vec<webapp::WebAppWin>,
    /// Text of the keyboard button whose mini app is being opened (for `sendWebViewData`).
    web_app_button: String,
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
    /// Contacts with a birthday today, whether they were asked for, the day the reminder was hidden,
    /// and our own birthday being edited in My Account.
    birthdays: Vec<(ChatInfo, backend::Birthday)>,
    birthdays_requested: bool,
    birthdays_dismissed: Option<jiff::civil::Date>,
    birthday_edit: Option<backend::Birthday>,
    /// Saved Messages: where messages came from, the source picked (`None` — all), sources fully loaded.
    saved_sources: Vec<ChatInfo>,
    saved_source: Option<PeerId>,
    saved_exhausted: HashSet<PeerId>,
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
            app.accounts = Accounts { active: 0, list: vec![accounts::Account { id: 0, name: me.clone() }] };
            app.me = me;
            app.chats = chats;
            app.history = history;
            app.folders = demo::folders();
            if let Some(first) = app.chats.first().map(|c| c.id) {
                app.open_chat(first);
            }
            for (key, image) in demo::images() {
                app.push_event(Ev::Image { key, image });
            }
            app.push_event(Ev::Outbox(demo::outbox()));
            app.birthdays_requested = true;
            app.send(Cmd::LoadBirthdays);
            app.screen = Screen::Main;
            return app;
        }
        app.start_backend();
        app
    }

    /// Backend for the active account; with saved api_id/api_hash we connect right away.
    fn start_backend(&mut self) {
        let session = accounts::session_path(self.accounts.active);
        self.backend = Some(backend::spawn(self.ctx.clone(), session));
        if let Some((id, hash)) = backend::saved_credentials() {
            self.api_id = id.to_string();
            self.api_hash = hash.clone();
            self.send(Cmd::Connect { api_id: id, api_hash: hash });
            self.screen = Screen::Login(Step::Connecting);
        }
    }

    /// Switch to another account: stop the backend and start from a fresh state.
    fn switch_account(&mut self, id: u32) {
        self.close_web_apps();
        self.save_chats();
        self.send(Cmd::Shutdown);
        self.accounts.active = id;
        self.accounts.save();
        let tray = self.tray.take();
        let tray_rx = std::mem::replace(&mut self.tray_rx, mpsc::channel().1);
        let mut next = Self::fresh(self.ctx.clone(), self.settings.clone(), tray, tray_rx);
        // The passcode was already entered in this run, so don't lock the other account.
        next.locked = false;
        next.start_backend();
        *self = next;
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

    /// Accounts menu under the name in the header: switching and adding.
    fn accounts_menu(&mut self, ui: &mut Ui) {
        let screen = ui.max_rect();
        // A click outside the menu closes it.
        if ui.interact(screen, ui.scope_id().with("accounts-bg"), Sense::click()).clicked() {
            self.accounts_open = false;
            return;
        }
        let rows = self.accounts.list.len() as f32 + 1.0;
        let rect = Rect::from_min_size(self.accounts_anchor.left_bottom() + vec2(-6.0, 8.0), vec2(260.0, rows * 44.0 + 16.0));
        glass::glass(ui.painter(), rect, Glass::panel(20.0).with_shadow());
        ui.interact(rect, ui.scope_id().with("accounts-panel"), Sense::click());
        let mut pick = None;
        let mut add = false;
        let mut y = rect.top() + 8.0;
        for a in &self.accounts.list {
            let r = Rect::from_min_size(pos2(rect.left() + 8.0, y), vec2(rect.width() - 16.0, 40.0));
            let resp = ui.interact(r, ui.scope_id().with(("account", a.id)), Sense::click());
            let p = ui.painter();
            if resp.hovered() {
                p.rect_filled(r, 12.0, glass::veil(16));
            }
            let name = if a.name.is_empty() { l!("Новий акаунт", "New account").to_owned() } else { a.name.clone() };
            glass::avatar(p, pos2(r.left() + 20.0, r.center().y), 15.0, &name, a.id as i64 + 7, None, None);
            p.galley(pos2(r.left() + 44.0, r.center().y - 9.0), elided(ui, &name, 14.5, ink(), r.width() - 80.0), ink());
            if a.id == self.accounts.active {
                icons::draw(p, Rect::from_center_size(pos2(r.right() - 16.0, r.center().y), vec2(18.0, 18.0)), Icon::Check, ACCENT);
            }
            if resp.clicked() && a.id != self.accounts.active {
                pick = Some(a.id);
            }
            y += 44.0;
        }
        let r = Rect::from_min_size(pos2(rect.left() + 8.0, y), vec2(rect.width() - 16.0, 40.0));
        if glass::button(ui, r, "account-add", l!("Додати акаунт", "Add account"), false, true).clicked() {
            add = true;
        }
        if self.demo && (pick.is_some() || add) {
            self.accounts_open = false;
            self.toast = Some((l!("У демо-режимі акаунти не перемикаються", "Accounts can't be switched in demo mode").into(), 0.0, true));
        } else if let Some(id) = pick {
            self.accounts_open = false;
            self.switch_account(id);
        } else if add {
            self.accounts_open = false;
            self.add_account();
        } else if ui.input(|i| i.key_pressed(Key::Escape)) {
            self.accounts_open = false;
        }
    }

    /// Add an account: a new empty entry and sign-in to it.
    fn add_account(&mut self) {
        let id = self.accounts.add();
        self.switch_account(id);
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
            api_id: String::new(),
            api_hash: String::new(),
            phone: String::new(),
            code: String::new(),
            password: String::new(),
            toast: None,
            me: String::new(),
            chats: Vec::new(),
            selected: None,
            history: HashMap::new(),
            exhausted: HashMap::new(),
            textures: HashMap::new(),
            loading_older: false,
            search: Search::default(),
            chat_search: None,
            draft: String::new(),
            draft_synced: String::new(),
            draft_edited_at: 0.0,
            edit_stash: String::new(),
            typing: HashMap::new(),
            typing_sent_at: 0.0,
            jump: None,
            revealed: HashSet::new(),
            compose: Compose::New,
            folders: Vec::new(),
            folder: None,
            stickers: Vec::new(),
            sticker_panel: false,
            sticker_set: 0,
            sticker_thumbs: HashSet::new(),
            stickers_requested: false,
            uploads: Vec::new(),
            next_upload: 0,
            picked: Vec::new(),
            forward: None,
            schedule: None,
            scheduled: HashMap::new(),
            scheduled_open: false,
            viewer: None,
            inline: None,
            inline_edited_at: 0.0,
            emoji_tab: true,
            multi_vote: HashMap::new(),
            settings: settings.clone(),
            accounts: Accounts::load(),
            settings_open: false,
            title_unread: 0,
            picked_tx,
            picked_rx,
            player: Player::new(),
            can_play: player::available(),
            media: HashMap::new(),
            media_wanted: HashMap::new(),
            media_loading: HashSet::new(),
            clips: HashMap::new(),
            clip_wanted: RefCell::new(Vec::new()),
            row_h: RefCell::new(HashMap::new()),
            recorder: None,
            voice_draft: None,
            round_rec: None,
            round_tex: None,
            round_seen: 0,
            can_record: recorder::available(),
            tray,
            tray_rx,
            quitting: false,
            tray_hint_shown: false,
            folder_edit: None,
            accounts_open: false,
            accounts_anchor: Rect::NOTHING,
            demo: false,
            composer_h: COMPOSER_H,
            topics: HashMap::new(),
            topic: None,
            topic_exhausted: HashSet::new(),
            comments: None,
            public_hits: Vec::new(),
            export_ask: None,
            exporting: HashSet::new(),
            topic_edit: None,
            settings_tab: SettingsTab::Main,
            cameras: None,
            sessions: Vec::new(),
            privacy: HashMap::new(),
            confirm_terminate: false,
            pass_fields: Default::default(),
            locked: !settings.passcode.is_empty(),
            lock_input: String::new(),
            lock_error: false,
            last_activity: 0.0,
            pins: HashMap::new(),
            pin_shown: HashMap::new(),
            pins_open: false,
            pins_hidden: HashSet::new(),
            marks: HashMap::new(),
            mark_wanted: None,
            at_bottom: true,
            scroll_bottom: false,
            unseen_below: 0,
            read_by: None,
            ads: Default::default(),
            notify: Default::default(),
            calendar: None,
            translate: None,
            translate_to: None,
            contact_form: None,
            blocked: None,
            poll_form: None,
            global_privacy: None,
            privacy_ex: HashMap::new(),
            privacy_open: None,
            privacy_pick: None,
            web_apps: Vec::new(),
            web_app_button: String::new(),
            suggest: Default::default(),
            info: None,
            profile: backend::Profile::default(),
            profile_edit: None,
            photo_tx,
            photo_rx,
            chat_edit: None,
            birthdays: Vec::new(),
            birthdays_requested: false,
            birthdays_dismissed: None,
            birthday_edit: None,
            bot_start: HashMap::new(),
            saved_sources: Vec::new(),
            saved_source: None,
            saved_exhausted: HashSet::new(),
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
            Ev::NeedPhone => self.screen = Screen::Login(Step::Phone),
            Ev::NeedCode => self.screen = Screen::Login(Step::Code),
            Ev::NeedPassword(hint) => self.screen = Screen::Login(Step::Password(hint)),
            Ev::Qr(url) => {
                if matches!(self.screen, Screen::Login(Step::Qr(_))) {
                    self.screen = Screen::Login(Step::Qr(Some(url)));
                }
            }
            Ev::LoggedIn(name) => {
                self.accounts.set_name(self.accounts.active, &name);
                self.accounts.save();
                self.me = name;
                self.screen = Screen::Main;
                self.code.clear();
                self.password.clear();
            }
            Ev::Dialogs(chats) => {
                self.chats = chats;
                // Once per session, when the account's chats are in.
                if !self.birthdays_requested {
                    self.birthdays_requested = true;
                    self.send(Cmd::LoadBirthdays);
                }
            }
            Ev::Birthdays(list) => self.birthdays = list,
            Ev::SavedDialogs(list) => self.saved_sources = list,
            Ev::SavedHistory { source, msgs, older } => self.on_saved_history(source, msgs, older),
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
            }
            Ev::NewMsg(m) => self.on_new_message(m),
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
                let tex = self.ctx.load_texture(format!("{key:?}"), image, TextureOptions::LINEAR);
                self.textures.insert(key, tex);
            }
            ev @ (Ev::Contacts(_) | Ev::ContactResults { .. } | Ev::ChatCreated(_)) => self.chatlist_event(ev),
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
            Ev::Folders(f) => {
                if self.folder.is_some_and(|id| id != ARCHIVE && !f.iter().any(|x| x.id == id)) {
                    self.folder = None;
                }
                self.folders = f;
            }
            Ev::Stickers(sets) => {
                self.stickers = sets;
                self.sticker_set = 0;
                self.request_sticker_thumbs();
            }
            Ev::UploadProgress { id, done, total } => {
                if let Some(u) = self.uploads.iter_mut().find(|u| u.id == id) {
                    (u.done, u.total) = (done, total);
                }
            }
            Ev::UploadDone(id) => self.uploads.retain(|u| u.id != id),
            Ev::BotAnswer { text, alert } => self.toast = Some((text, 0.0, alert)),
            Ev::Reactions { peer, id, reactions } => {
                if let Some(m) = self.find_mut(peer, id) {
                    m.reactions = reactions;
                }
            }
            Ev::Listened { peer, ids } => {
                use grammers_client::session::types::PeerKind;
                for (p, msgs) in &mut self.history {
                    // Without the chat, ids are unique within an account for everything except channels.
                    if peer.map_or(p.kind() != PeerKind::Channel, |x| x == *p) {
                        msgs.iter_mut().filter(|m| ids.contains(&m.id)).for_each(|m| m.media_unread = false);
                    }
                }
            }
            Ev::MediaReady { peer, id, path } => {
                self.media_loading.remove(&(peer, id));
                self.media.insert((peer, id), path.clone());
                match self.media_wanted.remove(&(peer, id)) {
                    Some(true) => open_external(&path),
                    Some(false) => {
                        let duration = self.find(peer, id).and_then(|m| m.media.as_ref()).map_or(0.0, |i| i.duration);
                        self.player.play(peer, id, &path, duration);
                    }
                    None => {}
                }
            }
            Ev::Scheduled { peer, msgs } => {
                if msgs.is_empty() && self.selected == Some(peer) {
                    self.scheduled_open = false;
                }
                self.scheduled.insert(peer, msgs);
            }
            Ev::InlineResults { peer, bot, query, query_id, results } => {
                if self.selected == Some(peer)
                    && let Some(i) = &mut self.inline
                    && i.bot == bot
                    && i.query == query
                {
                    i.results = results;
                    i.query_id = query_id;
                    i.results_for = Some((bot, query));
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
            Ev::SearchResults { peer: None, query, hits } => self.search.accept(query, hits),
            Ev::SearchResults { peer: Some(peer), query, hits } => {
                if self.selected == Some(peer)
                    && let Some(s) = &mut self.chat_search
                {
                    s.accept(query, hits);
                }
            }
            Ev::Sessions(list) => self.sessions = list,
            Ev::PublicResults { query, chats } => {
                if query == self.search.query.trim() {
                    self.public_hits = chats;
                }
            }
            Ev::Joined(peer) => {
                if let Some(c) = self.chat_mut(peer) {
                    c.joined = true;
                }
                self.toast = Some((l!("Ви приєдналися", "You joined").into(), 0.0, false));
            }
            Ev::ChatFound(info) => {
                let id = info.id;
                if !self.chats.iter().any(|c| c.id == id) {
                    self.chats.push(info);
                }
                self.open_chat(id);
            }
            Ev::ExportProgress { peer, done } => {
                let t = self.chats.iter().find(|c| c.id == peer).map(|c| c.title.clone()).unwrap_or_default();
                self.toast = Some((lf!("Експорт «{t}»: {done} повідомлень…", "Exporting “{t}”: {done} messages…"), 0.0, false));
            }
            Ev::Exported { peer, dir, count } => {
                self.exporting.remove(&peer);
                self.toast = Some((lf!("Експортовано {count} повідомлень", "Exported {count} messages"), 0.0, false));
                if count > 0 {
                    let _ = open::that_detached(&dir);
                }
            }
            Ev::Comments { peer, post, discussion, root, msgs, older } => {
                if let Some(c) = self.comments.as_mut().filter(|c| c.peer == peer && c.post == post) {
                    c.discussion = Some((discussion, root));
                    c.loading = false;
                    // Incomplete page: there are no earlier comments.
                    c.exhausted = msgs.len() < backend::PAGE;
                    if older {
                        let mut merged: Vec<Msg> = msgs.into_iter().filter(|m| !c.msgs.iter().any(|x| x.id == m.id)).collect();
                        merged.append(&mut c.msgs);
                        c.msgs = merged;
                    } else {
                        c.msgs = msgs;
                    }
                }
            }
            Ev::Topics { peer, topics } => {
                self.topics.insert(peer, topics);
            }
            Ev::TopicHistory { peer, topic, msgs, older } => {
                if older {
                    self.loading_older = false;
                }
                // Incomplete page: there are no earlier messages in the topic.
                if msgs.len() < backend::PAGE {
                    self.topic_exhausted.insert((peer, topic));
                }
                let last = msgs.last().map(|m| m.id);
                // Topics share the chat history: merge without duplicates and keep order by id.
                let list = self.history.entry(peer).or_default();
                for m in msgs {
                    if !list.iter().any(|x| x.id == m.id) {
                        list.push(m);
                    }
                }
                list.sort_by_key(|m| m.id);
                if !older
                    && let Some(max_id) = last
                    && let Some(t) = self.topics.get_mut(&peer).and_then(|ts| ts.iter_mut().find(|t| t.id == topic))
                    && t.unread > 0
                {
                    t.unread = 0;
                    self.send(Cmd::ReadTopic { peer, topic, max_id });
                }
            }
            Ev::GlobalPrivacy(p) => self.global_privacy = Some(p),
            Ev::ChatTitle { peer, title } => {
                if let Some(c) = self.chat_mut(peer) {
                    c.title = title;
                }
                self.chats_dirty = true;
            }
            Ev::PrivacyExceptions(key, ex) => {
                self.privacy_ex.insert(key, ex);
            }
            Ev::Privacy(key, state) => {
                self.privacy.insert(key, state);
            }
            ev @ (Ev::WebPreview { .. } | Ev::Mentions { .. } | Ev::BotCommands { .. }) => self.suggest_event(ev),
            Ev::Notice(text) => self.toast = Some((text, 0.0, false)),
            ev @ (Ev::Pinned { .. } | Ev::PinnedChanged { .. } | Ev::UnreadMarks { .. } | Ev::ReadBy { .. } | Ev::Views { .. }) => self.chat_view_event(ev),
            ev @ (Ev::Sponsored { .. } | Ev::SponsoredReport { .. }) => self.sponsored_event(ev),
            Ev::DateFound { peer, id } => self.date_found(peer, id),
            Ev::Translated { peer, id, to, result } => self.translated(peer, id, to, result),
            ev @ (Ev::WebAppOpened(_) | Ev::BotMenuApp { .. } | Ev::WebAppMethodResult { .. }) => self.web_app_event(ev),
            ev @ (Ev::ContactSaved(_) | Ev::ContactRemoved(_) | Ev::Blocked { .. } | Ev::BlockedList(_)) => self.contacts_event(ev),
            ev @ (Ev::NotifyDefault { .. } | Ev::NotifyExceptions { .. }) => self.notify_prefs_event(ev),
            ev @ (Ev::ChatDetails { .. } | Ev::SimilarChannels { .. } | Ev::Members { .. } | Ev::Invite { .. } | Ev::Shared { .. } | Ev::Notify { .. } | Ev::ChatRemoved(_) | Ev::HistoryCleared(_)) => {
                self.info_event(ev)
            }
            Ev::LoggedOut => {
                // Other accounts remain: remove this one and move to the next.
                if self.accounts.list.len() > 1 {
                    let gone = self.accounts.active;
                    self.accounts.remove(gone);
                    let next = self.accounts.active;
                    self.switch_account(next);
                    return;
                }
                self.accounts.set_name(self.accounts.active, "");
                self.accounts.save();
                self.chats.clear();
                self.history.clear();
                self.textures.clear();
                self.selected = None;
                self.screen = Screen::Login(Step::Creds);
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
            if self.locked {
                notify("Glassgram", l!("Нове повідомлення", "New message"));
            } else {
                let body = if chat.kind == ChatKind::Group && !m.sender.is_empty() { format!("{}: {}", m.sender, quote(&m)) } else { quote(&m) };
                notify(&chat.title, &body);
            }
        }
        if let Some(list) = self.history.get_mut(&m.peer)
            && !list.iter().any(|x| x.id == m.id)
        {
            list.push(m.clone());
            if open && !m.out && !self.at_bottom {
                self.unseen_below += 1;
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
        if let Some(c) = self.chat_mut(id) {
            c.unread = 0;
            c.unread_mark = false;
        }
        self.draft = self.chat_mut(id).map(|c| c.draft.clone()).unwrap_or_default();
        self.draft_synced = self.draft.clone();
        self.send(Cmd::OpenChat(id));
        self.open_chat_view(id);
        self.topic = None;
        self.saved_reset(id);
        if self.chats.iter().any(|c| c.id == id && c.forum) {
            self.send(Cmd::LoadTopics(id));
        }
    }

    fn open_topic(&mut self, peer: PeerId, topic: i32) {
        self.topic = Some(topic);
        self.compose = Compose::New;
        self.picked.clear();
        self.send(Cmd::OpenTopic { peer, topic, before: None });
    }

    /// Comments panel: bubbles from the discussion group and a field for your own comment.
    fn comments_view(&mut self, ui: &mut Ui, rect: Rect) {
        let Some(c) = &self.comments else { return };
        ui.interact(rect, ui.scope_id().with("comments-bg"), Sense::click());
        glass::glass(ui.painter(), rect, Glass::panel(24.0).with_shadow());
        let inner = rect.shrink(14.0);
        let title = match (c.discussion.is_some(), c.msgs.len()) {
            (false, _) => l!("Коментарі", "Comments").to_owned(),
            (true, 0) => l!("Коментарів ще немає", "No comments yet").to_owned(),
            (true, n) => comments_label(n as i32),
        };
        ui.painter().text(inner.left_top() + vec2(4.0, 4.0), Align2::LEFT_TOP, title, FontId::proportional(17.0), ink());
        let close = Rect::from_min_size(pos2(inner.right() - 32.0, inner.top() - 2.0), vec2(32.0, 32.0));
        let closed = glass::icon_button(ui, close, "comments-close", Icon::Close, false, true).clicked();
        let field = Rect::from_min_max(pos2(inner.left(), inner.bottom() - 40.0), pos2(inner.right() - 48.0, inner.bottom()));
        let send = Rect::from_min_size(pos2(inner.right() - 40.0, inner.bottom() - 40.0), vec2(40.0, 40.0));
        let list = Rect::from_min_max(pos2(inner.left() - 6.0, inner.top() + 40.0), pos2(inner.right() + 6.0, field.top() - 8.0));
        let mut child = ui.new_child(UiBuilder::new().max_rect(list));
        child.set_clip_rect(list);
        let mut action = None;
        let mut older = None;
        egui::ScrollArea::vertical().id_salt(("comments", c.peer, c.post)).auto_shrink(false).stick_to_bottom(true).show(&mut child, |ui| {
            if c.discussion.is_none() {
                ui.add_space(30.0);
                ui.vertical_centered(|ui| ui.add(egui::Spinner::new().color(ink())));
                return;
            }
            if !c.exhausted && !c.msgs.is_empty() {
                let w = ui.available_width();
                let (r, _) = ui.allocate_exact_size(vec2(w, 36.0), Sense::hover());
                let b = Rect::from_center_size(r.center(), vec2(200.0, 32.0));
                let label = if c.loading { l!("Завантаження…", "Loading…") } else { l!("Раніші коментарі", "Earlier comments") };
                if glass::button(ui, b, ("older-comments", c.post), label, false, !c.loading).clicked() {
                    older = c.msgs.first().map(|m| m.id);
                }
            }
            let root = c.discussion.map(|d| d.1);
            for m in &c.msgs {
                let reply = m.reply_to.filter(|r| Some(*r) != root).map(|id| match c.msgs.iter().find(|x| x.id == id) {
                    Some(r) => (author(r, &self.me), quote(r)),
                    None => (l!("Відповідь", "Reply").to_owned(), l!("Коментар", "Comment").to_owned()),
                });
                let photo = self.textures.get(&ImgKey::Photo(m.peer, m.id));
                let sticker = m.sticker.as_ref().and_then(|s| self.textures.get(&ImgKey::Sticker(s.0)));
                let look = Look { chosen: &[], group: true, revealed: false, picking: false, picked: false, play: PlayView::default(), ticks: None, chat: None, pending: None };
                if let Some(a) = bubble(ui, m, look, reply, photo, sticker) {
                    action = Some(a);
                }
            }
        });
        let Some(c) = self.comments.as_mut() else { return };
        if let Some(before) = older {
            c.loading = true;
            let (peer, id) = (c.peer, c.post);
            self.send(Cmd::OpenComments { peer, id, before: Some(before) });
        }
        let send_mods = self.send_mods();
        let Some(c) = self.comments.as_mut() else { return };
        let ready = c.discussion.is_some();
        let (resp, submitted) = glass::text_area(ui, field, &mut c.draft, l!("Коментар…", "Comment…"), send_mods);
        let can = ready && !c.draft.trim().is_empty();
        let clicked = glass::icon_button(ui, send, "comment-send", Icon::Send, true, can).clicked();
        if (clicked || submitted) && can
            && let Some((disc, root)) = c.discussion
        {
            let text = std::mem::take(&mut c.draft).trim().to_owned();
            self.send(Cmd::Send { peer: disc, text, reply_to: Some(root), opts: SendOpts::default() });
            resp.request_focus();
        }
        // Actions on a comment (copy, react, download) happen in the discussion group.
        if let Some(a) = action
            && let Some((disc, _)) = self.comments.as_ref().and_then(|c| c.discussion)
        {
            match a {
                Action::Reply(_) | Action::Edit(_) | Action::Pick(_) | Action::Forward(_) | Action::Delete(_) | Action::Pin(..) | Action::Unpin(_) | Action::ReadBy(_) | Action::Translate(_) => {}
                a => self.apply(disc, a),
            }
        }
        if closed {
            self.comments = None;
        }
    }

    /// Export confirmation: with media or text only.
    fn export_view(&mut self, ui: &mut Ui) {
        let Some((peer, media)) = &mut self.export_ask else { return };
        let peer = *peer;
        let title = self.chats.iter().find(|c| c.id == peer).map(|c| c.title.clone()).unwrap_or_default();
        let screen = ui.max_rect();
        ui.painter().rect_filled(screen, 0.0, Color32::from_black_alpha(90));
        ui.interact(screen, ui.scope_id().with("export-bg"), Sense::click());
        let rect = Rect::from_center_size(screen.center(), vec2(380.0, 200.0));
        glass::glass(ui.painter(), rect, Glass::panel(24.0).with_shadow());
        let inner = rect.shrink(18.0);
        ui.painter().text(inner.left_top(), Align2::LEFT_TOP, l!("Експорт історії", "Export chat history"), FontId::proportional(17.0), ink());
        ui.painter().galley(inner.left_top() + vec2(0.0, 28.0), elided(ui, &title, 14.0, ink_dim(), inner.width()), ink());
        let mut child = ui.new_child(UiBuilder::new().max_rect(Rect::from_min_size(inner.left_top() + vec2(0.0, 58.0), vec2(inner.width(), 60.0))));
        child.checkbox(media, l!("З фото й файлами (довше)", "Include photos and files (slower)"));
        child.label(egui::RichText::new(l!("HTML і JSON у теці «Завантаження/Glassgram Export»", "HTML and JSON in Downloads/Glassgram Export")).color(ink_dim()).size(12.5));
        let media = *media;
        let half = (inner.width() - 8.0) / 2.0;
        let y = inner.bottom() - 34.0;
        let cancel = glass::button(ui, Rect::from_min_size(pos2(inner.left(), y), vec2(half, 34.0)), "export-cancel", l!("Скасувати", "Cancel"), false, true).clicked();
        let busy = self.exporting.contains(&peer);
        let go = glass::button(ui, Rect::from_min_size(pos2(inner.right() - half, y), vec2(half, 34.0)), "export-go", l!("Експортувати", "Export"), true, !busy).clicked();
        if go && !busy {
            self.exporting.insert(peer);
            self.export_ask = None;
            self.toast = Some((l!("Експорт почався…", "Export started…").into(), 0.0, false));
            self.send(Cmd::ExportChat { peer, title, media });
        } else if cancel || ui.input(|i| i.key_pressed(Key::Escape)) {
            self.export_ask = None;
        }
    }

    /// Forum topic list instead of the history.
    fn forum_view(&mut self, ui: &mut Ui, peer: PeerId, chat: &ChatInfo, rect: Rect) {
        let header = Rect::from_min_size(rect.min, vec2(rect.width(), HEADER_H));
        let list = Rect::from_min_max(pos2(rect.left(), header.bottom() + 10.0), rect.max);
        glass::glass(ui.painter(), list, Glass::panel(24.0));
        let mut chosen = None;
        let mut edit = None;
        let mut child = ui.new_child(UiBuilder::new().max_rect(list.shrink2(vec2(8.0, 10.0))));
        child.set_clip_rect(list.shrink(4.0));
        egui::ScrollArea::vertical().id_salt(("topics", peer)).auto_shrink(false).show(&mut child, |ui| {
            let Some(topics) = self.topics.get(&peer) else {
                ui.add_space(30.0);
                ui.vertical_centered(|ui| ui.add(egui::Spinner::new().color(ink())));
                return;
            };
            ui.spacing_mut().item_spacing.y = 2.0;
            for t in topics {
                let resp = topic_row(ui, t);
                if resp.clicked() {
                    chosen = Some(t.id);
                }
                resp.context_menu(|ui| {
                    if glass::menu_button(ui, Icon::Pencil, l!("Перейменувати", "Rename")).clicked() {
                        edit = Some(Cmd::EditTopic { peer, id: t.id, title: None, closed: None });
                    }
                    if t.id != GENERAL_TOPIC {
                        let (icon, label) = if t.closed { (Icon::LockOpen, l!("Відкрити тему", "Reopen topic")) } else { (Icon::Lock, l!("Закрити тему", "Close topic")) };
                        if glass::menu_button(ui, icon, label).clicked() {
                            edit = Some(Cmd::EditTopic { peer, id: t.id, title: None, closed: Some(!t.closed) });
                        }
                    }
                });
            }
        });
        glass::glass(ui.painter(), header, Glass::tinted(HEADER_H / 2.0, glass::chrome(), 110).with_shadow().frosted(0.96));
        let p = ui.painter();
        let tex = self.textures.get(&ImgKey::Avatar(peer));
        glass::avatar(p, pos2(header.left() + 34.0, header.center().y), 21.0, &chat.title, peer.bot_api_dialog_id_unchecked(), kind_icon(chat.kind), tex);
        let text_x = header.left() + 66.0;
        p.galley(pos2(text_x, header.center().y - 20.0), elided(ui, &chat.title, 16.5, ink(), header.right() - text_x - 20.0), ink());
        let n = self.topics.get(&peer).map_or(0, Vec::len);
        p.text(pos2(text_x, header.center().y + 11.0), Align2::LEFT_CENTER, lf!("форум · тем: {n}", "forum · topics: {n}"), FontId::proportional(12.5), ink_dim());
        let add = Rect::from_center_size(pos2(header.right() - 32.0, header.center().y), vec2(38.0, 38.0));
        if glass::icon_button(ui, add, "topic-new", Icon::Plus, false, true).on_hover_text(l!("Нова тема", "New topic")).clicked() {
            self.topic_edit = Some((peer, None, String::new(), TOPIC_COLORS[0]));
        }
        if let Some(t) = chosen {
            self.open_topic(peer, t);
        }
        match edit {
            // Renaming opens the window with the current name.
            Some(Cmd::EditTopic { id, title: None, closed: None, .. }) => {
                let t = self.topics.get(&peer).and_then(|ts| ts.iter().find(|t| t.id == id));
                self.topic_edit = t.map(|t| (peer, Some(id), t.title.clone(), t.color));
            }
            Some(cmd) => self.send(cmd),
            None => {}
        }
        if self.topic_edit.is_some() {
            self.topic_editor(ui);
        }
    }

    /// New topic or rename window: name and icon color.
    fn topic_editor(&mut self, ui: &mut Ui) {
        let Some((peer, id, title, color)) = &mut self.topic_edit else { return };
        let screen = ui.max_rect();
        ui.painter().rect_filled(screen, 0.0, Color32::from_black_alpha(90));
        ui.interact(screen, ui.scope_id().with("topic-bg"), Sense::click());
        let rect = Rect::from_center_size(screen.center(), vec2(360.0, if id.is_none() { 210.0 } else { 160.0 }));
        glass::glass(ui.painter(), rect, Glass::panel(24.0).with_shadow());
        let inner = rect.shrink(18.0);
        let head = if id.is_none() { l!("Нова тема", "New topic") } else { l!("Перейменувати тему", "Rename topic") };
        ui.painter().text(inner.left_top(), Align2::LEFT_TOP, head, FontId::proportional(17.0), ink());
        let field = Rect::from_min_size(inner.left_top() + vec2(0.0, 34.0), vec2(inner.width(), 38.0));
        let resp = glass::text_field(ui, field, title, l!("Назва теми", "Topic name"), false);
        if title.is_empty() && !resp.has_focus() {
            resp.request_focus();
        }
        let mut y = field.bottom() + 12.0;
        if id.is_none() {
            // The icon color can only be set on creation.
            for (k, c) in TOPIC_COLORS.into_iter().enumerate() {
                let center = pos2(inner.left() + 16.0 + k as f32 * 40.0, y + 14.0);
                let r = Rect::from_center_size(center, vec2(30.0, 30.0));
                let resp = ui.interact(r, ui.scope_id().with(("topic-color", k)), Sense::click());
                let col = Color32::from_rgb((c >> 16) as u8, (c >> 8) as u8, c as u8);
                ui.painter().circle_filled(center, 13.0, col);
                if *color == c {
                    ui.painter().circle_stroke(center, 16.0, egui::Stroke::new(2.0, ink()));
                }
                if resp.clicked() {
                    *color = c;
                }
            }
            y += 40.0;
        }
        let ok = !title.trim().is_empty();
        let half = (inner.width() - 8.0) / 2.0;
        let cancel = glass::button(ui, Rect::from_min_size(pos2(inner.left(), y), vec2(half, 34.0)), "topic-cancel", l!("Скасувати", "Cancel"), false, true).clicked();
        let save = glass::button(ui, Rect::from_min_size(pos2(inner.right() - half, y), vec2(half, 34.0)), "topic-save", l!("Зберегти", "Save"), true, ok).clicked();
        let enter = resp.lost_focus() && ui.input(|i| i.key_pressed(Key::Enter));
        if (save || enter) && ok {
            let (peer, title, color) = (*peer, title.trim().to_owned(), *color);
            let cmd = match id {
                Some(id) => Cmd::EditTopic { peer, id: *id, title: Some(title), closed: None },
                None => Cmd::CreateTopic { peer, title, color },
            };
            self.topic_edit = None;
            self.send(cmd);
        } else if cancel || ui.input(|i| i.key_pressed(Key::Escape)) {
            self.topic_edit = None;
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
                unread_mentions: 0,
                unread_reactions: 0,
                noforwards: false,
                can_pin: false,
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
        self.jump = Some((peer, id, self.ctx.input(|i| i.time)));
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

    fn send_draft(&mut self) {
        self.send_draft_with(SendOpts::default());
    }

    fn send_draft_with(&mut self, opts: SendOpts) {
        let opts = SendOpts { no_webpage: opts.no_webpage || self.link_preview_off(), ..opts };
        let text = self.draft.trim().to_owned();
        let Some(peer) = self.selected else { return };
        if text.is_empty() {
            return;
        }
        self.draft.clear();
        match std::mem::replace(&mut self.compose, Compose::New) {
            Compose::Edit(id) => {
                if let Some(m) = self.history.get_mut(&peer).and_then(|l| l.iter_mut().find(|m| m.id == id)) {
                    let (plain, ents) = format::parse(&text);
                    m.spans = format::from_entities(&plain, &ents, 0);
                    m.text = plain;
                    m.edited = true;
                }
                self.draft = std::mem::take(&mut self.edit_stash);
                self.send(Cmd::Edit { peer, id, text });
            }
            // Scheduled messages go straight to the server; the rest through the send queue.
            Compose::Reply(id) if opts.schedule.is_some() => self.send(Cmd::Send { peer, text, reply_to: Some(id), opts }),
            Compose::Reply(id) => self.enqueue(peer, text, Some(id), &opts),
            Compose::New => {
                let reply_to = self.reply_target();
                if opts.schedule.is_some() {
                    self.send(Cmd::Send { peer, text, reply_to, opts });
                } else {
                    self.enqueue(peer, text, reply_to, &opts);
                }
            }
        }
        if opts.schedule.is_some() {
            self.send(Cmd::LoadScheduled(peer));
        }
        // After sending, the server clears the draft itself.
        if self.draft.is_empty() {
            self.draft_synced.clear();
            if let Some(c) = self.chat_mut(peer) {
                c.draft.clear();
            }
        }
    }

    fn apply(&mut self, peer: PeerId, action: Action) {
        let Some(action) = self.chat_view_action(peer, action) else { return };
        match action {
            Action::Reply(id) => self.compose = Compose::Reply(id),
            Action::Edit(id) => {
                if let Some(m) = self.find(peer, id) {
                    let markup = format::to_markup(&m.text, &m.spans);
                    if !matches!(self.compose, Compose::Edit(_)) {
                        self.edit_stash = std::mem::replace(&mut self.draft, markup);
                    } else {
                        self.draft = markup;
                    }
                    self.compose = Compose::Edit(id);
                }
            }
            Action::Copy(text) => self.ctx.copy_text(text),
            Action::Play(id) => self.play(peer, id),
            Action::Seek(id, frac) => {
                if self.player.is(peer, id) {
                    self.player.seek(frac);
                }
            }
            Action::Speed => self.player.cycle_speed(),
            Action::Comments(id) => {
                self.comments = Some(CommentsView { peer, post: id, discussion: None, msgs: Vec::new(), draft: String::new(), loading: true, exhausted: false });
                self.send(Cmd::OpenComments { peer, id, before: None });
            }
            Action::OpenExternal(id) => match self.media.get(&(peer, id)) {
                Some(path) => open_external(path),
                None => {
                    if self.media_wanted.insert((peer, id), true).is_none() {
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
            Action::Forward(id) => self.forward = Some(ForwardPicker { from: peer, ids: vec![id], query: String::new() }),
            Action::ForwardMany(ids) => self.forward = Some(ForwardPicker { from: peer, ids, query: String::new() }),
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
                self.ctx.open_url(egui::OpenUrl::new_tab(url));
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
                    ButtonAction::Url(url) => self.ctx.open_url(egui::OpenUrl::new_tab(url)),
                    ButtonAction::Text => {
                        let reply_to = self.topic.filter(|t| *t != GENERAL_TOPIC);
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
                Some('@') => self.ctx.open_url(egui::OpenUrl::new_tab(format!("https://t.me/{}", &target[1..]))),
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
                    self.ctx.open_url(egui::OpenUrl::new_tab(url));
                }
            },
            // Handled by `chat_view_action`.
            Action::Pin(..) | Action::Unpin(_) | Action::ReadBy(_) | Action::Translate(_) => {}
        }
    }

    fn play(&mut self, peer: PeerId, id: i32) {
        if self.player.is(peer, id) {
            if self.player.playing() { self.player.pause() } else { self.player.resume() }
            return;
        }
        let Some(info) = self.find(peer, id).and_then(|m| m.media.clone()) else { return };
        // Our own voice message is marked as listened by the other side, not by us.
        if let Some(m) = self.find_mut(peer, id).filter(|m| m.media_unread && !m.out) {
            m.media_unread = false;
            self.send(Cmd::MarkListened { peer, id });
        }
        // Videos and round videos play in the bubble with sound; a GIF loops on its own anyway.
        let external = info.kind == MediaKind::Gif;
        if !external && !self.can_play {
            self.toast = Some((l!("Для звуку потрібен ffplay (пакет ffmpeg)", "Sound needs ffplay (ffmpeg package)").into(), 0.0, true));
            return;
        }
        match self.media.get(&(peer, id)) {
            Some(path) if external => open_external(path),
            Some(path) => {
                let path = path.clone();
                self.player.play(peer, id, &path, info.duration);
            }
            None => {
                if !external {
                    // While loading, the previous one doesn't play and an earlier pressed one won't start.
                    self.player.stop();
                    self.media_wanted.retain(|_, ext| *ext);
                }
                if self.media_wanted.insert((peer, id), external).is_none() {
                    self.send(Cmd::LoadMedia { peer, id });
                }
            }
        }
    }

    /// Playback state for a bubble.
    fn play_view(&self, peer: PeerId, id: i32) -> PlayView {
        let current = self.player.is(peer, id);
        PlayView {
            current,
            playing: current && self.player.playing(),
            loading: self.media_wanted.contains_key(&(peer, id)),
            progress: if current { self.player.progress() } else { 0.0 },
            position: if current { self.player.position() } else { 0.0 },
            speed: self.player.speed,
        }
    }

    /// After a voice message, the next voice message in the same chat, as in Telegram.
    fn poll_player(&mut self) {
        let Some((peer, id)) = self.player.poll() else { return };
        let next = self.history.get(&peer).and_then(|msgs| {
            let voice = |m: &Msg| m.media.as_ref().is_some_and(|i| i.kind == MediaKind::Voice);
            let cur = msgs.iter().find(|m| m.id == id)?;
            voice(cur).then(|| msgs.iter().filter(|m| m.id > id && voice(m)).min_by_key(|m| m.id).map(|m| m.id))?
        });
        if let Some(next) = next {
            self.play(peer, next);
        }
    }

    /// Starts, syncs with audio and removes video clips according to the visible list.
    fn update_clips(&mut self, now: f64) {
        let wanted = std::mem::take(self.clip_wanted.get_mut());
        self.clips.retain(|k, _| wanted.contains(k));
        let ppp = self.ctx.pixels_per_point();
        for key in wanted {
            let Some(m) = self.find(key.0, key.1) else { continue };
            let info = m.media.clone();
            let kind = info.as_ref().map(|i| i.kind);
            let sound = self.player.is(key.0, key.1) && matches!(kind, Some(MediaKind::Video | MediaKind::Round));
            let Some(path) = self.media.get(&key).cloned() else {
                if kind != Some(MediaKind::Video) && self.media_loading.insert(key) {
                    self.send(Cmd::LoadMedia { peer: key.0, id: key.1 });
                }
                continue;
            };
            let even = |v: f32| ((v / 2.0).round() as usize * 2).max(2);
            let size = match &info {
                None => [even(STICKER_PT * ppp); 2],
                Some(i) if i.kind == MediaKind::Round => [even(ROUND_PT * ppp); 2],
                Some(i) => {
                    let (sw, sh) = if i.size.0 > 0 && i.size.1 > 0 { (i.size.0 as f32, i.size.1 as f32) } else { (16.0, 9.0) };
                    let w = (VIDEO_PT * ppp).min(sw);
                    [even(w), even(w * sh / sw)]
                }
            };
            let spec = |from: f64, speed: f32, looped: bool| Spec { path: path.clone(), size, from, speed, looped, contain: info.is_none() };
            if sound {
                let (run, from) = self.player.run();
                if self.player.playing() {
                    if self.clips.get(&key).is_none_or(|c| c.run != run) {
                        self.clips.insert(key, Clip::start(spec(from, self.player.speed, false), run, now));
                    }
                } else if let Some(c) = self.clips.get_mut(&key) {
                    c.stop();
                }
            } else if kind == Some(MediaKind::Video) {
                self.clips.remove(&key);
            } else if self.clips.get(&key).is_none_or(|c| c.run != 0) {
                self.clips.insert(key, Clip::start(spec(0.0, 1.0, true), 0, now));
            }
            if let Some(c) = self.clips.get_mut(&key) {
                c.advance(&self.ctx, &format!("clip{}_{}", key.0.bot_api_dialog_id_unchecked(), key.1), now);
            }
        }
    }

    /// What a new message replies to: an explicit reply or the open forum topic
    /// (replying to the topic's first message puts the message into it).
    /// Modifiers that send from the composer (Settings → Chat Settings).
    fn send_mods(&self) -> egui::Modifiers {
        if self.settings.ctrl_enter { egui::Modifiers::COMMAND } else { egui::Modifiers::NONE }
    }

    fn reply_target(&self) -> Option<i32> {
        match self.compose {
            Compose::Reply(id) => Some(id),
            _ => self.topic.filter(|t| *t != GENERAL_TOPIC),
        }
    }

    fn send_files(&mut self, peer: PeerId, paths: Vec<PathBuf>, as_file: bool) {
        if paths.is_empty() {
            return;
        }
        let caption = if self.selected == Some(peer) { std::mem::take(&mut self.draft) } else { String::new() };
        let reply_to = self.reply_target();
        if reply_to.is_some() {
            self.compose = Compose::New;
        }
        self.next_upload += 1;
        let id = self.next_upload;
        let name = match paths.as_slice() {
            [one] => one.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default(),
            many => lf!("Файлів: {}", "Files: {}", many.len()),
        };
        let total = paths.iter().filter_map(|p| std::fs::metadata(p).ok()).map(|m| m.len()).sum();
        self.uploads.push(Upload { id, name, done: 0, total });
        let caption = caption.trim().to_owned();
        self.send(Cmd::SendFiles { id, peer, paths, caption, reply_to, opts: SendOpts::default(), as_file });
    }

    fn start_recording(&mut self, peer: PeerId) {
        if !self.can_record {
            self.toast = Some((l!("Для запису потрібен ffmpeg", "Recording needs ffmpeg").into(), 0.0, true));
            return;
        }
        // Your own voice in headphones is distracting, so stop playback.
        self.player.stop();
        match Recorder::start() {
            Ok(r) => self.recorder = Some((peer, r)),
            Err(e) => self.toast = Some((e, 0.0, true)),
        }
    }

    fn cancel_recording(&mut self) {
        if let Some((_, r)) = self.recorder.take() {
            r.cancel();
        }
        if let Some((_, r)) = self.round_rec.take() {
            r.cancel();
        }
        if let Some((peer, v)) = self.voice_draft.take() {
            if self.player.is(peer, PREVIEW_ID) {
                self.player.stop();
            }
            v.discard();
        }
    }

    fn start_round(&mut self, peer: PeerId) {
        if !self.can_record {
            self.toast = Some((l!("Для запису потрібен ffmpeg", "Recording needs ffmpeg").into(), 0.0, true));
            return;
        }
        self.player.stop();
        match RoundRecorder::start(&self.settings.camera) {
            Ok(r) => {
                self.round_rec = Some((peer, r));
                self.round_tex = None;
                self.round_seen = 0;
            }
            Err(e) => self.toast = Some((e, 0.0, true)),
        }
    }

    fn send_round(&mut self) {
        let Some((peer, r)) = self.round_rec.take() else { return };
        self.round_tex = None;
        let Some(round) = r.finish() else {
            self.toast = Some((l!("Відео закоротке або камера недоступна", "Video is too short or the camera is unavailable").into(), 0.0, true));
            return;
        };
        let reply_to = self.reply_target();
        if reply_to.is_some() {
            self.compose = Compose::New;
        }
        self.next_upload += 1;
        let id = self.next_upload;
        let total = std::fs::metadata(&round.path).map_or(0, |m| m.len());
        self.uploads.push(Upload { id, name: l!("Відеоповідомлення", "Video message").into(), done: 0, total });
        self.send(Cmd::SendRound { id, peer, path: round.path, duration: round.duration, reply_to });
    }

    /// Round video recording: a round preview above the input field and a time row in the field.
    fn round_bar(&mut self, ui: &mut Ui, input: Rect, chat: Rect, now: f64) {
        let Some((_, rec)) = self.round_rec.as_mut() else { return };
        let elapsed = rec.elapsed();
        if !rec.alive() {
            // ffmpeg exits by itself at the limit, then we send; earlier means the camera failed to open.
            if elapsed >= camera::MAX_SECS - 1.0 {
                self.send_round();
            } else {
                if let Some((_, r)) = self.round_rec.take() {
                    r.cancel();
                }
                self.toast = Some((l!("Камера недоступна", "Camera unavailable").into(), 0.0, true));
            }
            return;
        }
        if let Some((n, rgba)) = rec.frame_after(self.round_seen) {
            self.round_seen = n;
            let img = egui::ColorImage::from_rgba_unmultiplied([camera::PREVIEW, camera::PREVIEW], &rgba);
            match &mut self.round_tex {
                Some(t) => t.set(img, TextureOptions::LINEAR),
                None => self.round_tex = Some(ui.ctx().load_texture("round-preview", img, TextureOptions::LINEAR)),
            }
        }
        let p = ui.painter().clone();
        // Round preview.
        let side = (chat.height() * 0.5).clamp(160.0, 300.0);
        let c = pos2(chat.center().x, input.top() - 24.0 - side / 2.0);
        let r = Rect::from_center_size(c, vec2(side, side));
        p.rect_filled(chat, 0.0, Color32::from_black_alpha(70));
        glass::glass(&p, r.expand(6.0), Glass::panel(side / 2.0 + 6.0));
        match &self.round_tex {
            Some(t) => glass::image_rounded(&p, r, side / 2.0, t.id(), t.size_vec2()),
            None => {
                p.circle_filled(c, side / 2.0, Color32::from_black_alpha(160));
                spinner(&p, c, 18.0);
            }
        }
        let frac = (elapsed / camera::MAX_SECS) as f32;
        let a0 = -std::f32::consts::FRAC_PI_2;
        let n = (frac * 128.0).ceil() as usize;
        let pts: Vec<_> = (0..=n).map(|k| a0 + k as f32 / 128.0 * std::f32::consts::TAU).map(|a| c + vec2(a.cos(), a.sin()) * (side / 2.0 + 3.0)).collect();
        if pts.len() > 1 {
            p.add(egui::Shape::line(pts, egui::Stroke::new(3.5, Color32::from_rgb(255, 80, 90))));
        }
        // Input field: dot, time and hint.
        glass::glass(&p, input, Glass::tinted(COMPOSER_H / 2.0, glass::chrome(), 110).with_shadow().frosted(0.96));
        let pulse = 0.55 + 0.45 * (now * 4.0).sin().abs() as f32;
        let dot = pos2(input.left() + 24.0, input.center().y);
        p.circle_filled(dot, 6.0, rgba(Color32::from_rgb(255, 80, 90), (255.0 * pulse) as u8));
        let time = format!("{} / {}", player::clock(elapsed), player::clock(camera::MAX_SECS));
        p.text(pos2(dot.x + 16.0, dot.y), Align2::LEFT_CENTER, time, FontId::proportional(15.0), ink());
        p.text(pos2(input.right() - 20.0, dot.y), Align2::RIGHT_CENTER, l!("Esc — скасувати", "Esc — cancel"), FontId::proportional(12.5), ink_dim());
    }

    /// Stop recording; `None` means too short.
    fn finish_recording(&mut self) -> Option<(PeerId, Voice)> {
        let (peer, r) = self.recorder.take()?;
        let voice = r.finish();
        if voice.is_none() {
            self.toast = Some((l!("Запис закороткий", "Recording is too short").into(), 0.0, true));
        }
        Some((peer, voice?))
    }

    /// Send the recording: straight from recording mode or from preview playback.
    fn send_recording(&mut self) {
        let next = match self.voice_draft.take() {
            Some(d) => Some(d),
            None => self.finish_recording(),
        };
        let Some((peer, voice)) = next else { return };
        if self.player.is(peer, PREVIEW_ID) {
            self.player.stop();
        }
        let reply_to = self.reply_target();
        if reply_to.is_some() {
            self.compose = Compose::New;
        }
        self.next_upload += 1;
        let id = self.next_upload;
        let total = std::fs::metadata(&voice.path).map_or(0, |m| m.len());
        self.uploads.push(Upload { id, name: l!("Голосове повідомлення", "Voice message").into(), done: 0, total });
        self.send(Cmd::SendVoice { id, peer, path: voice.path, duration: voice.duration, waveform: voice.waveform, reply_to });
    }

    /// Input field while recording: red dot, time, live level and hint.
    fn recording_bar(&mut self, ui: &mut Ui, input: Rect, now: f64) {
        let Some((_, rec)) = self.recorder.as_mut() else { return };
        if !rec.alive() {
            self.recorder = None;
            self.toast = Some((l!("Мікрофон недоступний", "Microphone unavailable").into(), 0.0, true));
            return;
        }
        let p = ui.painter().clone();
        glass::glass(&p, input, Glass::tinted(COMPOSER_H / 2.0, glass::chrome(), 110).with_shadow().frosted(0.96));
        // ■ stops and lets you listen before sending.
        let stop = Rect::from_center_size(pos2(input.left() + 25.0, input.center().y), vec2(36.0, 36.0));
        let stop_clicked = glass::icon_button(ui, stop, "rec-stop", Icon::Stop, false, true).on_hover_text(l!("Зупинити й прослухати", "Stop and listen")).clicked();
        let pulse = 0.55 + 0.45 * (now * 4.0).sin().abs() as f32;
        let dot = pos2(stop.right() + 16.0, input.center().y);
        p.circle_filled(dot, 6.0, rgba(Color32::from_rgb(255, 80, 90), (255.0 * pulse) as u8));
        let time = format!("{},{}", player::clock(rec.elapsed()), (rec.elapsed().fract() * 10.0) as u32);
        p.text(pos2(dot.x + 16.0, dot.y), Align2::LEFT_CENTER, time, FontId::proportional(15.0), ink());
        let hint = p.layout_no_wrap(l!("Esc — скасувати", "Esc — cancel").into(), FontId::proportional(12.5), ink_dim());
        let hint_x = input.right() - hint.size().x - 20.0;
        p.galley(pos2(hint_x, input.center().y - hint.size().y / 2.0), hint, ink_dim());
        // Level over the last seconds: bars from right to left, the newest on the right.
        let wave = Rect::from_min_max(pos2(dot.x + 80.0, input.top() + 12.0), pos2(hint_x - 16.0, input.bottom() - 12.0));
        let n = ((wave.width() / 4.0) as usize).max(1);
        let levels = rec.recent(n);
        for (k, v) in levels.iter().rev().enumerate() {
            let h = (2.0 + v.sqrt() * wave.height()).min(wave.height());
            let x = wave.right() - k as f32 * 4.0 - 2.4;
            p.rect_filled(Rect::from_min_max(pos2(x, wave.center().y - h / 2.0), pos2(x + 2.4, wave.center().y + h / 2.0)), 1.2, rgba(ink(), 200));
        }
        if stop_clicked {
            self.voice_draft = self.finish_recording();
        }
    }

    /// Recording preview: delete, play, waveform with progress, duration.
    fn voice_draft_bar(&mut self, ui: &mut Ui, input: Rect) {
        let Some((peer, v)) = &self.voice_draft else { return };
        let (peer, path, duration) = (*peer, v.path.clone(), v.duration);
        let play = self.play_view(peer, PREVIEW_ID);
        let p = ui.painter().clone();
        glass::glass(&p, input, Glass::tinted(COMPOSER_H / 2.0, glass::chrome(), 110).with_shadow().frosted(0.96));
        let trash = Rect::from_center_size(pos2(input.left() + 25.0, input.center().y), vec2(36.0, 36.0));
        if glass::icon_button(ui, trash, "rec-trash", Icon::Trash, false, true).on_hover_text(l!("Видалити запис", "Delete recording")).clicked() {
            self.cancel_recording();
            return;
        }
        let btn = Rect::from_center_size(pos2(trash.right() + 24.0, input.center().y), vec2(36.0, 36.0));
        let icon = if play.playing { Icon::Pause } else { Icon::Play };
        if glass::icon_button(ui, btn, "rec-play", icon, true, true).clicked() {
            if play.current {
                if play.playing { self.player.pause() } else { self.player.resume() }
            } else {
                self.player.play(peer, PREVIEW_ID, &path, duration);
            }
        }
        let time = if play.current { player::clock(play.position) } else { player::clock(duration) };
        let tg = p.layout_no_wrap(time, FontId::proportional(14.0), ink());
        let tx = input.right() - tg.size().x - 20.0;
        p.galley(pos2(tx, input.center().y - tg.size().y / 2.0), tg, ink());
        let wave = Rect::from_min_max(pos2(btn.right() + 14.0, input.top() + 13.0), pos2(tx - 16.0, input.bottom() - 13.0));
        let levels = self.voice_draft.as_ref().map(|(_, v)| v.waveform.clone()).unwrap_or_default();
        draw_wave(&p, wave, &levels, play.current.then_some(play.progress));
        if let Some(a) = seek_area(ui, PREVIEW_ID, wave, play) {
            if let Action::Seek(_, frac) = a {
                self.player.seek(frac);
            }
        }
    }

    /// System file picker in a separate thread, so as not to block the UI.
    fn pick_files(&self, peer: PeerId, as_file: bool) {
        let (tx, ctx) = (self.picked_tx.clone(), self.ctx.clone());
        let title = if as_file { l!("Надіслати без стиснення", "Send without compression") } else { l!("Надіслати файли", "Send files") };
        std::thread::spawn(move || {
            let paths = rfd::FileDialog::new().set_title(title).pick_files().unwrap_or_default();
            if !paths.is_empty() {
                let _ = tx.send((peer, paths, as_file));
            }
            ctx.request_repaint();
        });
    }

    fn request_sticker_thumbs(&mut self) {
        let Some(set) = self.stickers.get(self.sticker_set) else { return };
        let ids: Vec<i64> = set.stickers.iter().map(|s| s.0).filter(|id| self.sticker_thumbs.insert(*id)).collect();
        if !ids.is_empty() {
            self.send(Cmd::LoadStickerThumbs(ids));
        }
    }

    fn folder_unread(&self, folder: Option<i32>) -> usize {
        self.chats.iter().filter(|c| c.is_unread() && !c.muted && self.in_folder(c, folder)).count()
    }

    /// "All" excludes the archive, `ARCHIVE` is the archive only, otherwise the folder's rules.
    fn in_folder(&self, c: &ChatInfo, folder: Option<i32>) -> bool {
        match folder {
            None => !c.archived,
            Some(ARCHIVE) => c.archived,
            Some(id) => self.folders.iter().find(|f| f.id == id).is_none_or(|f| f.contains(c)),
        }
    }

    pub fn demo_folders(&self) -> Vec<Folder> {
        self.folders.clone()
    }

    fn new_folder(&mut self) {
        // Ids 0 and 1 are reserved by Telegram ("All chats" and archive).
        let id = self.folders.iter().map(|f| f.id).max().unwrap_or(1).max(1) + 1;
        let folder = Folder { id, ..Default::default() };
        self.folder_edit = Some(FolderEdit { folder, new: true, query: String::new() });
    }

    /// Folder window: name, chat types, exceptions and individual chats.
    fn folder_editor(&mut self, ui: &mut Ui) {
        let Some(ed) = &mut self.folder_edit else { return };
        let screen = ui.max_rect();
        ui.painter().rect_filled(screen, 0.0, Color32::from_black_alpha(90));
        ui.interact(screen, ui.scope_id().with("folder-bg"), Sense::click());
        let rect = Rect::from_center_size(screen.center(), vec2(screen.width().min(460.0) - 20.0, (screen.height() - 60.0).min(640.0)));
        glass::glass(ui.painter(), rect, Glass::panel(26.0).with_shadow());
        let inner = rect.shrink(18.0);
        let title = if ed.new { l!("Нова папка", "New folder") } else { l!("Папка", "Folder") };
        ui.painter().text(inner.left_top() + vec2(4.0, 2.0), Align2::LEFT_TOP, title, FontId::proportional(18.0), ink());
        let close = Rect::from_min_size(pos2(inner.right() - 32.0, inner.top() - 4.0), vec2(32.0, 32.0));
        let mut done = glass::icon_button(ui, close, "folder-close", Icon::Close, false, true).clicked();

        let emoji = Rect::from_min_size(inner.left_top() + vec2(0.0, 40.0), vec2(52.0, 38.0));
        glass::text_field(ui, emoji, &mut ed.folder.emoticon, "📁", false);
        if ed.folder.emoticon.chars().count() > 2 {
            ed.folder.emoticon = ed.folder.emoticon.chars().take(2).collect();
        }
        let name = Rect::from_min_max(pos2(emoji.right() + 8.0, emoji.top()), pos2(inner.right(), emoji.bottom()));
        let r = glass::text_field(ui, name, &mut ed.folder.title, l!("Назва папки", "Folder name"), false);
        if ed.new && ed.folder.title.is_empty() && !r.has_focus() {
            r.request_focus();
        }
        if ed.folder.title.chars().count() > FOLDER_TITLE_MAX {
            ed.folder.title = ed.folder.title.chars().take(FOLDER_TITLE_MAX).collect();
        }

        let buttons = Rect::from_min_max(pos2(inner.left(), inner.bottom() - 38.0), inner.max);
        let body = Rect::from_min_max(pos2(inner.left(), emoji.bottom() + 12.0), pos2(inner.right(), buttons.top() - 10.0));
        let mut child = ui.new_child(UiBuilder::new().max_rect(body));
        child.set_clip_rect(body);
        let q = ed.query.trim().to_lowercase();
        let chats = &self.chats;
        let textures = &self.textures;
        egui::ScrollArea::vertical().id_salt("folder-body").auto_shrink(false).show(&mut child, |ui| {
            let f = &mut ed.folder;
            if f.shared {
                ui.label(egui::RichText::new(l!("Це спільна папка за посиланням — її правила змінюються в офіційному клієнті.", "This is a shared folder from a link — its rules are changed in the official app.")).color(ink_dim()));
                return;
            }
            section_title(ui, l!("Включити типи чатів", "Include chat types"));
            ui.checkbox(&mut f.contacts, l!("Контакти", "Contacts"));
            ui.checkbox(&mut f.non_contacts, l!("Не контакти", "Non-contacts"));
            ui.checkbox(&mut f.groups, l!("Групи", "Groups"));
            ui.checkbox(&mut f.broadcasts, l!("Канали", "Channels"));
            ui.checkbox(&mut f.bots, l!("Боти", "Bots"));
            ui.add_space(6.0);
            section_title(ui, l!("Не показувати", "Exclude"));
            ui.checkbox(&mut f.exclude_muted, l!("Без сповіщень", "Muted"));
            ui.checkbox(&mut f.exclude_read, l!("Прочитані", "Read"));
            ui.checkbox(&mut f.exclude_archived, l!("Архівовані", "Archived"));
            ui.add_space(6.0);
            section_title(ui, l!("Окремі чати (клік: додати → виключити → прибрати)", "Specific chats (click: include → exclude → remove)"));
            let field = ui.allocate_exact_size(vec2(ui.available_width(), 36.0), Sense::hover()).0;
            glass::search_field(ui, field, &mut ed.query, l!("Пошук чату…", "Search chats…"));
            ui.add_space(4.0);
            // First those already in the rules, then the rest.
            let state = |f: &Folder, id: &PeerId| if f.include.contains(id) { 1 } else if f.exclude.contains(id) { 2 } else { 0 };
            let mut list: Vec<&ChatInfo> = chats.iter().filter(|c| c.title.to_lowercase().contains(&q)).collect();
            list.sort_by_key(|c| state(f, &c.id) == 0);
            for c in list {
                let st = state(f, &c.id);
                let hit = SearchHit { chat: c.title.clone(), kind: c.kind, msg: Msg::placeholder(c.id) };
                let resp = hit_row(ui, &hit, "", textures.get(&ImgKey::Avatar(c.id)));
                let mark = Rect::from_center_size(pos2(resp.rect.right() - 22.0, resp.rect.center().y), vec2(22.0, 22.0));
                match st {
                    1 => icons::draw(ui.painter(), mark, Icon::Check, Color32::from_rgb(110, 230, 150)),
                    2 => icons::draw(ui.painter(), mark, Icon::Close, Color32::from_rgb(255, 110, 120)),
                    _ => {}
                }
                if resp.clicked() {
                    match st {
                        0 => f.include.push(c.id),
                        1 => {
                            f.include.retain(|x| *x != c.id);
                            f.pinned.retain(|x| *x != c.id);
                            f.exclude.push(c.id);
                        }
                        _ => f.exclude.retain(|x| *x != c.id),
                    }
                }
            }
        });

        let f = &ed.folder;
        let has_rules = f.contacts || f.non_contacts || f.groups || f.broadcasts || f.bots || !f.include.is_empty();
        let valid = !f.title.trim().is_empty() && has_rules && !f.shared;
        let w = if ed.new { buttons.width() } else { (buttons.width() - 8.0) / 2.0 };
        let save = Rect::from_min_size(pos2(buttons.right() - w, buttons.top()), vec2(w, buttons.height()));
        let save_resp = glass::button(ui, save, "folder-save", l!("Зберегти", "Save"), true, valid);
        let hint = if f.title.trim().is_empty() { l!("Вкажіть назву", "Enter a name") } else { l!("Додайте хоча б один тип чатів або чат", "Add at least one chat type or chat") };
        let save_clicked = if valid { save_resp.clicked() } else { save_resp.on_disabled_hover_text(hint).clicked() };
        let mut delete = false;
        if !ed.new {
            let del = Rect::from_min_size(buttons.min, vec2(w, buttons.height()));
            delete = glass::button(ui, del, "folder-delete", l!("Видалити папку", "Delete folder"), false, true).clicked();
        }
        let cmd = if save_clicked && valid {
            let mut f = ed.folder.clone();
            f.title = f.title.trim().to_owned();
            Some(Cmd::SaveFolder(f))
        } else if delete {
            Some(Cmd::DeleteFolder(ed.folder.id))
        } else {
            None
        };
        if let Some(cmd) = cmd {
            self.send(cmd);
            done = true;
        }
        if done || ui.input(|i| i.key_pressed(Key::Escape)) {
            self.folder_edit = None;
        }
    }

    /// Settings window. Changes are saved immediately.
    fn settings_view(&mut self, ui: &mut Ui) {
        let screen = ui.max_rect();
        ui.painter().rect_filled(screen, 0.0, Color32::from_black_alpha(90));
        ui.interact(screen, ui.scope_id().with("settings-bg"), Sense::click());
        let rect = Rect::from_center_size(screen.center(), vec2(screen.width().min(460.0) - 20.0, (screen.height() - 60.0).min(640.0)));
        glass::glass(ui.painter(), rect, Glass::panel(26.0).with_shadow());
        let inner = rect.shrink(18.0);
        let tab = self.settings_tab;
        // Sections open as pages with a back button, as in Telegram Desktop.
        let mut back = false;
        let title_x = if tab == SettingsTab::Main {
            inner.left() + 4.0
        } else {
            let b = Rect::from_min_size(pos2(inner.left() - 4.0, inner.top() - 4.0), vec2(32.0, 32.0));
            back = glass::icon_button(ui, b, "settings-back", Icon::ChevronLeft, false, true).on_hover_text(l!("Назад", "Back")).clicked();
            inner.left() + 38.0
        };
        ui.painter().text(pos2(title_x, inner.top() + 12.0), Align2::LEFT_CENTER, self.settings_title(), FontId::proportional(17.0), ink());
        let close = Rect::from_min_size(pos2(inner.right() - 32.0, inner.top() - 4.0), vec2(32.0, 32.0));
        let done = glass::icon_button(ui, close, "settings-close", Icon::Close, false, true).clicked();
        let before = self.settings.clone();
        // No "Done" button, as in Telegram Desktop: changes apply at once and ✕ closes.
        let body = Rect::from_min_max(pos2(inner.left(), inner.top() + 40.0), inner.max);
        let mut child = ui.new_child(UiBuilder::new().max_rect(body));
        child.set_clip_rect(body);
        self.settings_page(&mut child, tab);
        if self.settings != before {
            // Don't save an invalid proxy, so as not to break the next connection.
            if settings::proxy_error(&self.settings.proxy).is_none() {
                self.settings.save();
            }
            if self.settings.lang != before.lang {
                crate::i18n::set(&self.settings.lang);
                if let Some(t) = &self.tray {
                    t.refresh();
                }
                self.title_unread = usize::MAX;
            }
            if self.settings.scale != before.scale {
                self.ctx.set_zoom_factor(self.settings.scale);
            }
            if self.settings.notifications != before.notifications
                && let Some(t) = &self.tray
            {
                t.set_notifications(self.settings.notifications);
            }
        }
        // Esc goes back to the main page first; the folder window on top handles its own Esc.
        let esc = self.folder_edit.is_none() && ui.input(|i| i.key_pressed(Key::Escape));
        if back || (esc && tab != SettingsTab::Main) {
            self.settings_tab = SettingsTab::Main;
            self.profile_edit = None;
        } else if done || esc {
            self.settings_open = false;
            self.profile_edit = None;
        }
    }

    /// "Privacy" tab: who sees what, and the local passcode.
    fn privacy_tab(&mut self, ui: &mut Ui) {
        let mut set = None;
        let mut lock_now = false;
        egui::ScrollArea::vertical().id_salt("privacy-body").auto_shrink(false).show(ui, |ui| {
            // First on the page, as in Telegram Desktop.
            self.blocked_section(ui);
            ui.add_space(6.0);
            for key in PrivacyKey::ALL {
                section_title(ui, key.title());
                let state = self.privacy.get(&key).copied();
                ui.horizontal(|ui| {
                    ui.spacing_mut().item_spacing.x = 6.0;
                    let opts = [(Audience::Everybody, l!("Усі", "Everybody")), (Audience::Contacts, l!("Мої контакти", "My contacts")), (Audience::Nobody, l!("Ніхто", "Nobody"))];
                    for (a, label) in opts {
                        let (r, _) = ui.allocate_exact_size(vec2(122.0, 30.0), Sense::hover());
                        let on = state.is_some_and(|s| s.audience == a);
                        if glass::button(ui, r, ("privacy", key.title(), label), label, on, state.is_some()).clicked() && !on {
                            set = Some((key, a));
                        }
                    }
                });
                self.privacy_exception_rows(ui, key);
                ui.add_space(6.0);
            }
            ui.add_space(6.0);
            self.global_privacy_section(ui);
            ui.add_space(6.0);
            section_title(ui, l!("Локальний пароль", "Local passcode"));
            let s = &mut self.settings;
            let [new, again, current] = &mut self.pass_fields;
            if s.passcode.is_empty() {
                ui.label(egui::RichText::new(l!("Блокує Glassgram на цьому комп'ютері. Сесію на диску не шифрує.", "Locks Glassgram on this computer. Does not encrypt the session on disk.")).color(ink_dim()).size(12.5));
                for (field, hint) in [(&mut *new, l!("Новий пароль", "New passcode")), (&mut *again, l!("Повторіть пароль", "Repeat passcode"))] {
                    let r = ui.allocate_exact_size(vec2(ui.available_width(), 36.0), Sense::hover()).0;
                    glass::text_field(ui, r, field, hint, true);
                }
                let ok = new.chars().count() >= 4 && new == again;
                let r = ui.allocate_exact_size(vec2(ui.available_width(), 34.0), Sense::hover()).0;
                let resp = glass::button(ui, r, "pass-set", l!("Увімкнути пароль", "Turn on passcode"), true, ok);
                let resp = if ok { resp } else { resp.on_disabled_hover_text(l!("Щонайменше 4 символи, обидва поля однакові", "At least 4 characters, both fields equal")) };
                if resp.clicked() && ok {
                    s.passcode = crate::lock::make(new);
                    new.clear();
                    again.clear();
                }
            } else {
                ui.label(egui::RichText::new(l!("Пароль увімкнено. Автоблокування:", "Passcode is on. Auto-lock:")).color(ink_dim()).size(12.5));
                ui.horizontal_wrapped(|ui| {
                    ui.spacing_mut().item_spacing = vec2(6.0, 6.0);
                    for (min, label) in lock_choices() {
                        let w = elided(ui, label, 14.0, ink(), 300.0).size().x + 26.0;
                        let (r, _) = ui.allocate_exact_size(vec2(w, 30.0), Sense::hover());
                        if glass::button(ui, r, ("lock-min", min), label, s.lock_minutes == min, true).clicked() {
                            s.lock_minutes = min;
                        }
                    }
                });
                ui.add_space(4.0);
                let r = ui.allocate_exact_size(vec2(ui.available_width(), 34.0), Sense::hover()).0;
                lock_now = glass::button(ui, r, "pass-lock", l!("Заблокувати зараз", "Lock now"), false, true).clicked();
                let r = ui.allocate_exact_size(vec2(ui.available_width(), 36.0), Sense::hover()).0;
                glass::text_field(ui, r, current, l!("Поточний пароль — щоб вимкнути", "Current passcode — to turn off"), true);
                let r = ui.allocate_exact_size(vec2(ui.available_width(), 34.0), Sense::hover()).0;
                let valid = crate::lock::check(&s.passcode, current);
                if glass::button(ui, r, "pass-off", l!("Вимкнути пароль", "Turn off passcode"), false, valid).clicked() && valid {
                    s.passcode.clear();
                    current.clear();
                }
            }
        });
        if let Some((key, a)) = set {
            self.set_privacy_audience(key, a);
        }
        if lock_now {
            self.settings_open = false;
            self.locked = true;
        }
    }

    /// "Devices" tab: the account's active sessions.
    fn devices_tab(&mut self, ui: &mut Ui) {
        let mut terminate = None;
        let mut all = false;
        egui::ScrollArea::vertical().id_salt("devices-body").auto_shrink(false).show(ui, |ui| {
            if self.sessions.is_empty() {
                ui.add(egui::Spinner::new().color(ink_dim()));
                return;
            }
            for (i, s) in self.sessions.iter().enumerate() {
                if i == 0 && s.current {
                    section_title(ui, l!("Цей пристрій", "This device"));
                } else if i == 1 || (i == 0 && !s.current) {
                    section_title(ui, l!("Інші сеанси", "Other sessions"));
                }
                let (row, _) = ui.allocate_exact_size(vec2(ui.available_width(), 64.0), Sense::hover());
                let p = ui.painter();
                p.rect_filled(row, 14.0, glass::veil(10));
                let x = row.left() + 12.0;
                let bold = FontId::new(14.5, egui::FontFamily::Name(format::BOLD.into()));
                let mut job = LayoutJob::single_section(s.app.clone(), egui::TextFormat::simple(bold, ink()));
                job.wrap = TextWrapping::truncate_at_width(row.width() - 130.0);
                p.galley(pos2(x, row.top() + 6.0), p.layout_job(job), ink());
                p.galley(pos2(x, row.top() + 26.0), elided(ui, &s.device, 12.5, ink_dim(), row.width() - 130.0), ink());
                let sub = if s.place.is_empty() { s.active.clone() } else { format!("{} · {}", s.place, s.active) };
                p.galley(pos2(x, row.top() + 43.0), elided(ui, &sub, 12.0, ink_dim(), row.width() - 24.0), ink());
                if !s.current {
                    let b = Rect::from_min_size(pos2(row.right() - 112.0, row.top() + 8.0), vec2(104.0, 28.0));
                    if glass::button(ui, b, ("term", s.hash), l!("Завершити", "Terminate"), false, true).clicked() {
                        terminate = Some(s.hash);
                    }
                }
                ui.add_space(4.0);
            }
            if self.sessions.len() > 1 {
                ui.add_space(6.0);
                let r = ui.allocate_exact_size(vec2(ui.available_width(), 36.0), Sense::hover()).0;
                let label = if self.confirm_terminate { l!("Точно завершити всі інші?", "Really terminate all others?") } else { l!("Завершити всі інші сеанси", "Terminate all other sessions") };
                all = glass::button(ui, r, "term-all", label, self.confirm_terminate, true).clicked();
            }
        });
        if let Some(hash) = terminate {
            self.send(Cmd::TerminateSession(hash));
        }
        if all {
            if self.confirm_terminate {
                self.send(Cmd::TerminateOtherSessions);
            }
            self.confirm_terminate = !self.confirm_terminate;
        }
    }

    /// Lock screen: just a passcode field over the background.
    fn lock_view(&mut self, ui: &mut Ui) {
        let screen = ui.max_rect();
        let rect = Rect::from_center_size(screen.center(), vec2(320.0, 230.0));
        glass::glass(ui.painter(), rect, Glass::panel(28.0).with_shadow());
        let logo = pos2(rect.center().x, rect.top() + 46.0);
        glass::avatar(ui.painter(), logo, 26.0, "", 4, Some(Icon::Lock), None);
        ui.painter().text(pos2(rect.center().x, rect.top() + 88.0), Align2::CENTER_CENTER, l!("Glassgram заблоковано", "Glassgram is locked"), FontId::proportional(17.0), ink());
        let field = Rect::from_min_size(pos2(rect.left() + 24.0, rect.top() + 112.0), vec2(rect.width() - 48.0, 40.0));
        let resp = glass::text_field(ui, field, &mut self.lock_input, l!("Локальний пароль", "Local passcode"), true);
        if !resp.has_focus() {
            resp.request_focus();
        }
        if resp.changed() {
            self.lock_error = false;
        }
        let enter = resp.lost_focus() && ui.input(|i| i.key_pressed(Key::Enter));
        let b = Rect::from_min_size(pos2(rect.left() + 24.0, rect.bottom() - 58.0), vec2(rect.width() - 48.0, 36.0));
        if glass::button(ui, b, "unlock", l!("Розблокувати", "Unlock"), true, !self.lock_input.is_empty()).clicked() || enter {
            if crate::lock::check(&self.settings.passcode, &self.lock_input) {
                self.locked = false;
                self.lock_error = false;
            } else {
                self.lock_error = true;
            }
            self.lock_input.clear();
        }
        if self.lock_error {
            ui.painter().text(pos2(rect.center().x, field.bottom() + 9.0), Align2::CENTER_CENTER, l!("Невірний пароль", "Wrong passcode"), FontId::proportional(12.5), Color32::from_rgb(255, 110, 120));
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

    /// Chat photos in order, for paging in the viewer.
    fn photo_ids(&self, peer: PeerId) -> Vec<i32> {
        self.history.get(&peer).map(|l| l.iter().filter(|m| m.photo).map(|m| m.id).collect()).unwrap_or_default()
    }

    fn open_viewer(&mut self, peer: PeerId, id: i32) {
        match &mut self.viewer {
            Some(v) if v.peer == peer => v.show_id(id),
            _ => self.viewer = Some(Viewer::new(peer, id)),
        }
        if !self.textures.contains_key(&ImgKey::Full(peer, id)) {
            self.send(Cmd::LoadFullPhoto { peer, id });
        }
    }

    fn viewer_view(&mut self, ui: &mut Ui) {
        let Some(v) = &self.viewer else { return };
        let (peer, id) = (v.peer, v.id);
        let full = self.textures.get(&ImgKey::Full(peer, id));
        let tex = full.or_else(|| self.textures.get(&ImgKey::Photo(peer, id))).cloned();
        let loading = full.is_none() && self.backend.is_some();
        let ids = self.photo_ids(peer);
        let pos = ids.iter().position(|x| *x == id).map_or(0, |i| i + 1);
        let caption = self.find(peer, id).map(|m| m.text.replace('\n', " ")).unwrap_or_default();
        let Some(v) = &mut self.viewer else { return };
        match v.ui(ui, tex.as_ref(), &caption, (pos, ids.len()), loading) {
            Some(ViewerAction::Close) => self.viewer = None,
            Some(ViewerAction::Download) if !self.protected(peer, &[id]) => self.send(Cmd::Download { peer, id }),
            Some(ViewerAction::Prev) if pos > 1 => self.open_viewer(peer, ids[pos - 2]),
            Some(ViewerAction::Next) if pos < ids.len() => self.open_viewer(peer, ids[pos]),
            _ => {}
        }
    }

    fn find(&self, peer: PeerId, id: i32) -> Option<&Msg> {
        self.history.get(&peer)?.iter().find(|m| m.id == id)
    }

    // ───────────────────────────── Sign-in ─────────────────────────────

    fn login_view(&mut self, ui: &mut Ui, step: Step) {
        if let Step::Qr(url) = &step {
            return self.qr_view(ui, url.as_deref());
        }
        let screen = ui.max_rect();
        let card_h = match step {
            Step::Creds => 400.0,
            Step::Connecting => 300.0,
            Step::Phone => 420.0,
            _ => 360.0,
        };
        let card = Rect::from_center_size(screen.center(), vec2(380.0_f32.min(screen.width() - 32.0), card_h));
        glass::glass(ui.painter(), card, Glass::panel(34.0));
        // Signing in to an added account can be abandoned to return to the already open one.
        let active = self.accounts.active;
        if let Some(back) = self.accounts.list.iter().find(|a| a.id != active && !a.name.is_empty()).cloned() {
            let b = Rect::from_center_size(pos2(card.center().x, card.bottom() + 30.0), vec2(card.width(), 38.0));
            if glass::button(ui, b, "account-back", &lf!("← Повернутися до «{}»", "← Back to “{}”", back.name), false, true).clicked() {
                if self.accounts.list.iter().any(|a| a.id == active && a.name.is_empty()) {
                    self.accounts.remove(active);
                    if active != 0 {
                        let _ = std::fs::remove_dir_all(accounts::session_path(active).parent().unwrap_or(std::path::Path::new("")));
                    }
                }
                self.switch_account(back.id);
                return;
            }
        }

        let p = ui.painter().clone();
        let mut y = card.top() + 34.0;
        let logo = pos2(card.center().x, y + 34.0);
        glass::avatar(&p, logo, 34.0, "", 4, Some(Icon::Send), None);
        y += 84.0;
        p.text(pos2(card.center().x, y), Align2::CENTER_TOP, "Glassgram", FontId::proportional(26.0), ink());
        y += 36.0;
        let subtitle = match &step {
            Step::Creds => l!("Введіть API ID та API hash з my.telegram.org", "Enter API ID and API hash from my.telegram.org").to_owned(),
            Step::Connecting => l!("Підключення до Telegram…", "Connecting to Telegram…").to_owned(),
            Step::Phone => l!("Ваш номер телефону у міжнародному форматі", "Your phone number in international format").to_owned(),
            Step::Code => l!("Код надіслано в Telegram або SMS", "The code was sent via Telegram or SMS").to_owned(),
            Step::Password(h) => match h {
                Some(h) => lf!("Пароль двоетапної перевірки (підказка: {h})", "Two-step verification password (hint: {h})"),
                None => l!("Пароль двоетапної перевірки", "Two-step verification password").to_owned(),
            },
            Step::Qr(_) => unreachable!(),
        };
        let galley = p.layout(subtitle, FontId::proportional(14.0), ink_dim(), card.width() - 64.0);
        p.galley(pos2(card.center().x - galley.size().x / 2.0, y), galley.clone(), ink_dim());
        y += galley.size().y + 22.0;

        if step == Step::Connecting {
            let spin = Rect::from_center_size(pos2(card.center().x, y + 20.0), vec2(28.0, 28.0));
            ui.put(spin, egui::Spinner::new().size(28.0).color(ink()));
            return;
        }

        let field = |y: f32| Rect::from_min_size(pos2(card.left() + 32.0, y), vec2(card.width() - 64.0, 46.0));
        let mut submit = false;
        let mut fields: Vec<(&mut String, &str, bool)> = match &step {
            Step::Creds => vec![(&mut self.api_id, "API ID", false), (&mut self.api_hash, "API hash", false)],
            Step::Phone => vec![(&mut self.phone, "+380 00 000 00 00", false)],
            Step::Code => vec![(&mut self.code, l!("Код", "Code"), false)],
            Step::Password(_) => vec![(&mut self.password, l!("Пароль", "Password"), true)],
            Step::Connecting | Step::Qr(_) => unreachable!(),
        };
        for (text, hint, pw) in fields.iter_mut() {
            let r = glass::text_field(ui, field(y), text, hint, *pw);
            submit |= r.lost_focus() && ui.input(|i| i.key_pressed(Key::Enter));
            y += 58.0;
        }
        drop(fields);

        let label = match (&step, self.busy) {
            (_, true) => l!("Зачекайте…", "Please wait…"),
            (Step::Creds, _) => l!("Підключитися", "Connect"),
            (Step::Phone, _) => l!("Отримати код", "Get code"),
            _ => l!("Увійти", "Log in"),
        };
        y += 6.0;
        submit |= glass::button(ui, field(y), "login-btn", label, true, !self.busy).clicked();
        if submit && !self.busy {
            self.submit_login(&step);
        }
        if step == Step::Phone && glass::button(ui, field(y + 58.0), "qr-btn", l!("Увійти через QR-код", "Log in with QR code"), false, !self.busy).clicked() {
            self.screen = Screen::Login(Step::Qr(None));
            self.send(Cmd::QrLogin);
        }
    }

    fn qr_view(&mut self, ui: &mut Ui, url: Option<&str>) {
        let screen = ui.max_rect();
        let card = Rect::from_center_size(screen.center(), vec2(400.0_f32.min(screen.width() - 32.0), 510.0));
        glass::glass(ui.painter(), card, Glass::panel(34.0));
        let p = ui.painter().clone();
        let mut y = card.top() + 30.0;
        p.text(pos2(card.center().x, y), Align2::CENTER_TOP, l!("Вхід за QR-кодом", "QR code login"), FontId::proportional(24.0), ink());
        y += 46.0;

        let qr_rect = Rect::from_min_size(pos2(card.center().x - 120.0, y), vec2(240.0, 240.0));
        match url {
            Some(url) => glass::qr(&p, qr_rect, url),
            None => {
                glass::glass(&p, qr_rect, Glass::tinted(24.0, Color32::WHITE, 40));
                ui.put(Rect::from_center_size(qr_rect.center(), vec2(32.0, 32.0)), egui::Spinner::new().size(32.0).color(ink()));
            }
        }
        y = qr_rect.bottom() + 22.0;

        let steps = l!("1. Відкрийте Telegram на телефоні\n2. Налаштування → Пристрої\n3. «Підключити пристрій» і наведіть камеру на код", "1. Open Telegram on your phone\n2. Settings → Devices\n3. “Link Desktop Device” and point the camera at the code");
        let g = p.layout(steps.to_owned(), FontId::proportional(14.5), ink_dim(), card.width() - 72.0);
        p.galley(pos2(card.left() + 36.0, y), g.clone(), ink_dim());
        y += g.size().y + 24.0;

        let btn = Rect::from_min_size(pos2(card.left() + 32.0, y), vec2(card.width() - 64.0, 46.0));
        if glass::button(ui, btn, "qr-back", l!("Увійти за номером", "Log in with phone number"), false, true).clicked() {
            self.send(Cmd::CancelQr);
            self.screen = Screen::Login(Step::Phone);
        }
    }

    fn submit_login(&mut self, step: &Step) {
        let cmd = match step {
            Step::Creds => match self.api_id.trim().parse() {
                Ok(api_id) if !self.api_hash.trim().is_empty() => {
                    self.screen = Screen::Login(Step::Connecting);
                    Cmd::Connect { api_id, api_hash: self.api_hash.trim().to_owned() }
                }
                _ => {
                    self.toast = Some((l!("API ID має бути числом, API hash — непорожнім", "API ID must be a number and API hash must not be empty").into(), 0.0, true));
                    return;
                }
            },
            Step::Phone => Cmd::RequestCode(self.phone.clone()),
            Step::Code => Cmd::SubmitCode(self.code.clone()),
            Step::Password(_) => Cmd::SubmitPassword(self.password.clone()),
            Step::Connecting | Step::Qr(_) => return,
        };
        self.busy = true;
        self.send(cmd);
    }

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

    fn sidebar(&mut self, ui: &mut Ui, rect: Rect) {
        glass::glass(ui.painter(), rect, Glass::panel(28.0));
        let p = ui.painter().clone();
        let inner = rect.shrink(14.0);

        p.text(inner.left_top() + vec2(6.0, 2.0), Align2::LEFT_TOP, l!("Чати", "Chats"), FontId::proportional(24.0), ink());
        if !self.me.is_empty() {
            // The name is the accounts menu button.
            let label = format!("{}  ▾", self.me);
            let r = p.text(inner.left_top() + vec2(6.0, 32.0), Align2::LEFT_TOP, label, FontId::proportional(12.5), ink_dim());
            let resp = ui.interact(r.expand(4.0), ui.scope_id().with("accounts-btn"), Sense::click());
            if resp.hovered() {
                ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
            }
            if resp.on_hover_text(l!("Акаунти", "Accounts")).clicked() {
                self.accounts_open = !self.accounts_open;
            }
            self.accounts_anchor = r;
        }
        let btn = |i: f32| Rect::from_min_size(pos2(inner.right() - 36.0 - i * 44.0, inner.top() + 4.0), vec2(36.0, 36.0));
        if glass::icon_button(ui, btn(0.0), "logout", Icon::Power, false, true).on_hover_text(l!("Вийти", "Log out")).clicked() {
            self.close_web_apps();
            self.send(Cmd::Logout);
        }
        if glass::icon_button(ui, btn(1.0), "reload", Icon::Reload, false, true).on_hover_text(l!("Оновити", "Refresh")).clicked() {
            self.send(Cmd::ReloadDialogs);
        }
        let (bell, hint) = if self.settings.notifications { (Icon::Bell, l!("Вимкнути сповіщення", "Turn off notifications")) } else { (Icon::Muted, l!("Увімкнути сповіщення", "Turn on notifications")) };
        if glass::icon_button(ui, btn(3.0), "settings", Icon::Gear, self.settings_open, true).on_hover_text(l!("Налаштування", "Settings")).clicked() {
            self.open_settings();
        }
        if glass::icon_button(ui, btn(2.0), "notify", bell, false, true).on_hover_text(hint).clicked() {
            self.settings.notifications = !self.settings.notifications;
            self.settings.save();
            if let Some(t) = &self.tray {
                t.set_notifications(self.settings.notifications);
            }
        }

        let search = Rect::from_min_size(inner.left_top() + vec2(0.0, 56.0), vec2(inner.width() - 48.0, 40.0));
        self.new_chat_button(ui, Rect::from_min_size(pos2(search.right() + 8.0, search.top()), vec2(40.0, 40.0)));
        let now = ui.input(|i| i.time);
        let search_resp = glass::search_field(ui, search, &mut self.search.query, l!("Пошук", "Search"));
        if std::mem::take(&mut self.focus_search) {
            search_resp.request_focus();
        }
        if search_resp.changed() {
            self.search.edited_at = now;
        }
        if let Some(query) = self.search.due(now) {
            self.search.requested = query.clone();
            self.public_hits.clear();
            if query.chars().count() >= 3 && !self.demo {
                self.send(Cmd::SearchPublic(query.clone()));
            }
            self.send(Cmd::Search { peer: None, query });
        }

        let mut list_top = search.bottom() + 10.0;
        if !self.settings.folders_side && self.has_folder_bar() {
            let tabs = Rect::from_min_size(pos2(inner.left(), list_top), vec2(inner.width(), TABS_H));
            self.folder_tabs(ui, tabs);
            list_top = tabs.bottom() + 8.0;
        }
        let list_rect = Rect::from_min_max(pos2(inner.left() - 6.0, list_top), pos2(inner.right() + 6.0, inner.bottom()));
        let query = self.search.query.trim().to_lowercase();
        let mut clicked = None;
        let mut jump = None;
        let mut menu = None;
        let mut found = None;
        let mut dropped = None;
        let mut drag = self.pin_drag.take();
        let mut birthday = None;
        let (rows, pinned) = self.list_rows(&query, drag.as_ref());
        let mut child = ui.new_child(UiBuilder::new().max_rect(list_rect));
        child.set_clip_rect(list_rect);
        egui::ScrollArea::vertical()
            .id_salt("chat-list")
            .auto_shrink(false)
            .show(&mut child, |ui| {
                ui.spacing_mut().item_spacing.y = 4.0;
                if query.is_empty() && self.folder.is_none() {
                    birthday = self.birthday_banner(ui);
                }
                let rows_out = self.chat_rows(ui, &rows, pinned, &mut drag);
                clicked = rows_out.clicked;
                menu = rows_out.menu;
                dropped = rows_out.dropped;
                // Public chats not yet in the list.
                let public: Vec<&ChatInfo> = self.public_hits.iter().filter(|p| !self.chats.iter().any(|c| c.id == p.id)).collect();
                if !query.is_empty() && !public.is_empty() {
                    section_title(ui, l!("Публічні чати", "Public chats"));
                    for chat in public {
                        let tex = self.textures.get(&ImgKey::Avatar(chat.id));
                        if chat_row(ui, chat, false, tex, None, None).clicked() {
                            found = Some(chat.clone());
                        }
                    }
                }
                if !query.is_empty() && self.search.hits_for.to_lowercase() == query {
                    section_title(ui, if self.search.hits.is_empty() { l!("Повідомлень не знайдено", "No messages found") } else { l!("Повідомлення", "Messages") });
                    for hit in &self.search.hits {
                        if hit_row(ui, hit, &self.search.hits_for, self.textures.get(&ImgKey::Avatar(hit.msg.peer))).clicked() {
                            jump = Some(hit.clone());
                        }
                    }
                } else if query.chars().count() >= 2 && self.backend.is_some() {
                    ui.add_space(12.0);
                    ui.vertical_centered(|ui| ui.add(egui::Spinner::new().color(ink_dim())));
                }
                if self.chats.is_empty() {
                    ui.add_space(30.0);
                    ui.vertical_centered(|ui| ui.add(egui::Spinner::new().color(ink_dim())));
                }
            });
        drop(rows);
        self.pin_drag = drag;
        if let Some(order) = dropped {
            self.drop_pins(order);
        }
        if let Some(id) = clicked {
            self.open_chat(id);
        }
        if let Some(a) = birthday {
            self.birthday_action(a);
        }
        if let Some(chat) = found {
            let id = chat.id;
            self.chats.push(chat);
            self.open_chat(id);
        }
        if let Some(hit) = jump {
            self.jump_to(&hit);
        }
        if let Some((peer, a)) = menu {
            self.apply_chat(peer, a);
        }
    }

    fn chat_area(&mut self, ui: &mut Ui, rect: Rect) {
        let Some(peer) = self.selected else {
            let pill = Rect::from_center_size(rect.center(), vec2(230.0, 44.0));
            glass::glass(ui.painter(), pill, Glass::panel(22.0));
            ui.painter().text(pill.center(), Align2::CENTER_CENTER, l!("Оберіть чат зліва", "Select a chat on the left"), FontId::proportional(15.0), ink());
            return;
        };
        let Some(chat) = self.chats.iter().find(|c| c.id == peer).cloned() else { return };
        if chat.forum && self.topic.is_none() {
            self.forum_view(ui, peer, &chat, rect);
            return;
        }
        let topic = self.topic.filter(|_| chat.forum);
        let source = self.saved_source.filter(|_| chat.kind == ChatKind::Saved);
        let group = matches!(chat.kind, ChatKind::Group);
        // In channels views replace ticks; in "Saved Messages" there is nobody to read.
        let ticks = |m: &Msg| (m.out && !matches!(chat.kind, ChatKind::Channel | ChatKind::Saved)).then_some(m.id <= chat.read_out);
        let bar = if self.compose == Compose::New { 0.0 } else { COMPOSE_BAR_H + 8.0 };

        let now = ui.input(|i| i.time);
        if self.jump.is_some_and(|(p, _, start)| p != peer || now - start > JUMP_FLASH) {
            self.jump = None;
        }
        // Messages scroll under the floating glass header and input field.
        let mut load_older = None;
        let mut action = None;
        let (mut ad_seen, mut ad_action) = (None, None);
        let mut open_calendar = None;
        let mut child = ui.new_child(UiBuilder::new().max_rect(rect));
        child.set_clip_rect(rect);
        let pin_h = self.pin_bar_h(peer) + self.saved_bar_h(peer);
        let scroll_bottom = std::mem::take(&mut self.scroll_bottom);
        let scrolled = egui::ScrollArea::vertical()
            .id_salt(("history", peer))
            .auto_shrink(false)
            .stick_to_bottom(true)
            .show(&mut child, |ui| {
                ui.add_space(HEADER_H + 16.0 + pin_h);
                match self.history.get(&peer) {
                    None => {
                        ui.add_space(40.0);
                        ui.vertical_centered(|ui| ui.add(egui::Spinner::new().color(ink())));
                    }
                    Some(all) => {
                        // In a forum, show only the open topic.
                        // Saved Messages likewise show only the picked source.
                        let msgs: Vec<&Msg> = all.iter().filter(|m| topic.is_none_or(|t| m.topic_id() == t) && source.is_none_or(|s| m.saved_from == Some(s))).collect();
                        let exhausted = match (topic, source) {
                            (Some(t), _) => self.topic_exhausted.contains(&(peer, t)),
                            (_, Some(s)) => self.saved_exhausted.contains(&s),
                            _ => self.exhausted.get(&peer).copied().unwrap_or(false),
                        };
                        if !msgs.is_empty() && !exhausted {
                            let w = ui.available_width();
                            let (r, _) = ui.allocate_exact_size(vec2(w, 36.0), Sense::hover());
                            let b = Rect::from_center_size(r.center(), vec2(200.0, 32.0));
                            let label = if self.loading_older { l!("Завантаження…", "Loading…") } else { l!("Раніші повідомлення", "Earlier messages") };
                            if glass::button(ui, b, ("older", peer), label, false, !self.loading_older).clicked() {
                                load_older = Some(msgs[0].id);
                            }
                        }
                        let mut last_day = "";
                        let jump = self.jump.filter(|j| j.0 == peer);
                        let mut heights = self.row_h.borrow_mut();
                        let spacing = ui.spacing().item_spacing.y;
                        let mut i = 0;
                        while i < msgs.len() {
                            let m = msgs[i];
                            // Album: consecutive photos with the same grouped_id form one row.
                            let mut end = i + 1;
                            if m.photo && m.group.is_some() {
                                while end < msgs.len() && msgs[end].photo && msgs[end].group == m.group {
                                    end += 1;
                                }
                            }
                            let album = &msgs[i..end];
                            i = end;
                            let new_day = m.day != last_day;
                            last_day = &m.day;
                            // Virtualization: a row far from the screen and already measured is just empty space.
                            // The height may be stale, but it's remeasured as soon as the row comes close.
                            let top = ui.cursor().top();
                            let jumping = jump.is_some_and(|j| album.iter().any(|x| x.id == j.1));
                            if !jumping
                                && let Some(&h) = heights.get(&(peer, m.id))
                                && !ui.is_rect_visible(Rect::from_x_y_ranges(ui.max_rect().x_range(), top..=top + h).expand(VIRTUAL_MARGIN))
                            {
                                ui.allocate_space(vec2(ui.available_width(), (h - spacing).max(0.0)));
                                continue;
                            }
                            if new_day && day_chip(ui, &m.day).clicked() {
                                open_calendar = Some(m.ts);
                            }
                            if album.len() > 1 {
                                let ids: Vec<i32> = album.iter().map(|m| m.id).collect();
                                let look = Look {
                                    chosen: &[],
                                    group,
                                    revealed: false,
                                    picking: !self.picked.is_empty(),
                                    picked: ids.iter().all(|id| self.picked.contains(id)),
                                    play: PlayView::default(),
                                    ticks: ticks(m),
                                    chat: Some(&chat),
                                    pending: None,
                                };
                                let texs: Vec<_> = album.iter().map(|m| self.textures.get(&ImgKey::Photo(peer, m.id))).collect();
                                if let Some(a) = album_bubble(ui, album, &texs, look) {
                                    action = Some(a);
                                }
                                heights.insert((peer, m.id), ui.cursor().top() - top);
                                continue;
                            }
                            // In a forum, a "reply" to the topic root is just a message in the topic, no quote.
                            let reply = m.reply_to.filter(|r| topic.is_none_or(|t| *r != t)).map(|id| match msgs.iter().find(|x| x.id == id) {
                                Some(r) => (author(r, &self.me), quote(r)),
                                None => (l!("Відповідь", "Reply").to_owned(), l!("Повідомлення", "Message").to_owned()),
                            });
                            let kind = m.media.as_ref().map(|i| i.kind);
                            let animated = self.settings.autoplay && m.video_sticker
                                || matches!(kind, Some(MediaKind::Gif | MediaKind::Round))
                                || (kind == Some(MediaKind::Video) && self.player.is(peer, m.id));
                            let clip = self.clips.get(&(peer, m.id)).and_then(|c| c.tex.as_ref());
                            let photo = clip.or(self.textures.get(&ImgKey::Photo(peer, m.id)));
                            let sticker = clip.or(m.sticker.as_ref().and_then(|s| self.textures.get(&ImgKey::Sticker(s.0))));
                            let revealed = self.revealed.contains(&(peer, m.id));
                            let bubble_top = ui.cursor().top();
                            let chosen = self.multi_vote.get(&(peer, m.id)).map_or(&[][..], |v| v.as_slice());
                            let look = Look {
                                chosen,
                                group,
                                revealed,
                                picking: !self.picked.is_empty(),
                                picked: self.picked.contains(&m.id),
                                play: if m.media.is_some() { self.play_view(peer, m.id) } else { PlayView::default() },
                                ticks: ticks(m).filter(|_| m.id > 0),
                                chat: Some(&chat),
                                pending: self.pending_state(m.id),
                            };
                            if let Some(a) = bubble(ui, m, look, reply, photo, sticker) {
                                action = Some(a);
                            }
                            if let Some(n) = m.comments {
                                let w = (ui.available_width() * 0.72).min(340.0);
                                let (row, _) = ui.allocate_exact_size(vec2(ui.available_width(), 36.0), Sense::hover());
                                let b = Rect::from_min_size(pos2(row.left() + 10.0, row.top() + 2.0), vec2(w, 32.0));
                                if glass::button(ui, b, ("comments", m.id), &format!("💬  {}", comments_label(n)), false, true).clicked() {
                                    action = Some(Action::Comments(m.id));
                                }
                            }
                            if animated && ui.is_rect_visible(Rect::from_x_y_ranges(ui.max_rect().x_range(), bubble_top..=ui.cursor().top())) {
                                self.clip_wanted.borrow_mut().push((peer, m.id));
                            }
                            if let Some((_, id, start)) = jump
                                && id == m.id
                            {
                                let row = Rect::from_x_y_ranges(ui.max_rect().x_range(), bubble_top..=ui.cursor().top());
                                let age = now - start;
                                // Several frames in a row: content height may change while photos load.
                                if age < 0.3 {
                                    ui.scroll_to_rect(row, Some(Align::Center));
                                }
                                let a = (1.0 - age / JUMP_FLASH).clamp(0.0, 1.0) as f32;
                                ui.painter().rect_filled(row.shrink2(vec2(4.0, 0.0)), 14.0, rgba(ACCENT, (a * 70.0) as u8));
                            }
                            heights.insert((peer, m.id), ui.cursor().top() - top);
                        }
                        // The ad goes after the newest post, as in Telegram Desktop.
                        if topic.is_none()
                            && !msgs.is_empty()
                            && let Some(ad) = self.ads.first(peer)
                        {
                            let tex = ad.photo.and_then(|id| self.textures.get(&ImgKey::Sponsored(id)));
                            let (visible, a) = sponsored::card(ui, ad, tex);
                            if visible {
                                ad_seen = Some(ad.random_id.clone());
                            }
                            ad_action = a;
                        }
                        if msgs.is_empty() {
                            ui.add_space(60.0);
                            ui.vertical_centered(|ui| ui.label(egui::RichText::new(l!("Тут поки порожньо", "Nothing here yet")).color(ink_dim())));
                        }
                    }
                }
                ui.add_space(self.composer_h + 26.0 + bar);
                if scroll_bottom {
                    ui.scroll_to_cursor(Some(Align::BOTTOM));
                }
            });
        self.track_bottom(&scrolled);
        if let Some(before) = load_older {
            self.loading_older = true;
            match (topic, source) {
                (Some(t), _) => self.send(Cmd::OpenTopic { peer, topic: t, before: Some(before) }),
                (_, Some(s)) => self.send(Cmd::LoadSavedHistory { saved: peer, source: s, before: Some(before) }),
                _ => self.send(Cmd::LoadOlder { peer, before }),
            }
        }
        if let Some(a) = action {
            self.apply(peer, a);
        }
        if let Some(ts) = open_calendar {
            self.open_calendar(peer, Some(ts));
        }
        if let Some(id) = ad_seen {
            self.ad_seen(&id);
        }
        if let Some(a) = ad_action {
            self.ad_action(peer, a);
        }

        // Header.
        let header = Rect::from_min_size(rect.min, vec2(rect.width(), HEADER_H));
        glass::glass(ui.painter(), header, Glass::tinted(HEADER_H / 2.0, glass::chrome(), 110).with_shadow().frosted(0.96));
        let p = ui.painter().clone();
        let open_topic = topic.and_then(|t| self.topics.get(&peer)?.iter().find(|x| x.id == t).cloned());
        if topic.is_some() {
            // In a topic, a back-to-topic-list button replaces the avatar.
            let back = Rect::from_center_size(pos2(header.left() + 34.0, header.center().y), vec2(40.0, 40.0));
            if glass::icon_button(ui, back, "topic-back", Icon::ChevronLeft, false, true).on_hover_text(l!("До списку тем", "Back to topics")).clicked() {
                self.topic = None;
                self.compose = Compose::New;
            }
        } else {
            let tex = self.textures.get(&ImgKey::Avatar(peer));
            glass::avatar(&p, pos2(header.left() + 34.0, header.center().y), 21.0, &chat.title, peer.bot_api_dialog_id_unchecked(), kind_icon(chat.kind), tex);
        }
        let search_btn = Rect::from_center_size(pos2(header.right() - 32.0, header.center().y), vec2(38.0, 38.0));
        let searching = self.chat_search.is_some();
        if glass::icon_button(ui, search_btn, "chat-search", Icon::Search, searching, true).on_hover_text(l!("Пошук у чаті", "Search in chat")).clicked() {
            self.chat_search = if searching { None } else { Some(Search::default()) };
        }
        let text_x = header.left() + 66.0;
        if self.chat_search.is_none() {
            // Click on the title opens chat info, as in Telegram Desktop.
            let left = if topic.is_some() { header.left() + 60.0 } else { header.left() + 8.0 };
            let area = Rect::from_min_max(pos2(left, header.top()), pos2(search_btn.left() - 6.0, header.bottom()));
            let resp = ui.interact(area, ui.scope_id().with(("chat-title", peer)), Sense::click());
            if resp.hovered() {
                ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
            }
            if resp.on_hover_text(l!("Інформація про чат", "Chat info")).clicked() {
                if self.info.is_some() {
                    self.info = None;
                } else {
                    self.open_info(peer);
                }
            }
        }
        if self.chat_search.is_some() {
            let cal = search_btn.translate(vec2(-44.0, 0.0));
            if glass::icon_button(ui, cal, "chat-calendar", Icon::Calendar, self.calendar.is_some(), true).on_hover_text(l!("Перейти до дати", "Jump to date")).clicked() {
                self.open_calendar(peer, None);
            }
        }
        if let Some(s) = &mut self.chat_search {
            let field = Rect::from_min_max(pos2(text_x - 4.0, header.top() + 11.0), pos2(search_btn.left() - 54.0, header.bottom() - 11.0));
            let resp = glass::search_field(ui, field, &mut s.query, l!("Пошук у чаті", "Search in chat"));
            if !resp.has_focus() && s.query.is_empty() && s.edited_at == 0.0 {
                resp.request_focus();
                s.edited_at = now;
            }
            if resp.changed() {
                s.edited_at = now;
            }
            if let Some(query) = s.due(now) {
                s.requested = query.clone();
                self.send(Cmd::Search { peer: Some(peer), query });
            }
        } else {
            let title_w = search_btn.left() - text_x - 12.0;
            let title = open_topic.as_ref().map_or(chat.title.as_str(), |t| t.title.as_str());
            p.galley(pos2(text_x, header.center().y - 20.0), elided(ui, title, 16.5, ink(), title_w), ink());
            let typing = self.typing.get(&peer).filter(|(_, until)| *until > now);
            let (sub, color) = match typing {
                Some((who, _)) if group && !who.is_empty() => (lf!("{who} друкує…", "{who} is typing…"), ACCENT),
                Some(_) => (l!("друкує…", "typing…").to_owned(), ACCENT),
                None if topic.is_some() => (lf!("тема · {}", "topic · {}", chat.title), ink_dim()),
                None => match chat.kind {
                    ChatKind::User => match chat.presence.text() {
                        Some(t) => (t, if chat.presence.online() { ACCENT } else { ink_dim() }),
                        None => (l!("особистий чат", "private chat").to_owned(), ink_dim()),
                    },
                    ChatKind::Bot => (l!("бот", "bot").to_owned(), ink_dim()),
                    ChatKind::Group => (l!("група", "group").to_owned(), ink_dim()),
                    ChatKind::Channel => (l!("канал", "channel").to_owned(), ink_dim()),
                    ChatKind::Saved => (l!("ваші нотатки", "your notes").to_owned(), ink_dim()),
                },
            };
            p.text(pos2(text_x, header.center().y + 11.0), Align2::LEFT_CENTER, sub, FontId::proportional(12.5), color);
        }

        // Not a member of a public chat: "Join"; a channel subscriber gets only the notifications toggle.
        if !chat.joined || !chat.can_post {
            let bar = Rect::from_min_max(pos2(rect.left(), rect.bottom() - COMPOSER_H), rect.max);
            let (label, accent) = match (chat.joined, chat.muted) {
                (false, _) => (l!("Приєднатися", "Join"), true),
                (true, true) => (l!("Увімкнути сповіщення", "Unmute"), false),
                (true, false) => (l!("Вимкнути сповіщення", "Mute"), false),
            };
            if glass::button(ui, bar, ("bottom-bar", peer), label, accent, true).clicked() {
                if chat.joined {
                    self.apply_chat(peer, ChatAction::Mute(!chat.muted));
                } else {
                    self.send(Cmd::Join(peer));
                }
            }
            self.composer_h = COMPOSER_H;
            self.chat_overlays(ui, peer, rect, header, bar.top() - 8.0);
            return;
        }
        if self.start_bot_bar(ui, peer, chat.kind, rect) {
            self.composer_h = COMPOSER_H;
            self.chat_overlays(ui, peer, rect, header, rect.bottom() - COMPOSER_H - 8.0);
            return;
        }
        // [📎] [input field] [🙂] [➤]
        let circle = |x: f32| Rect::from_min_size(pos2(x, rect.bottom() - COMPOSER_H), vec2(COMPOSER_H, COMPOSER_H));
        let send_btn = circle(rect.right() - COMPOSER_H);
        let sticker_btn = circle(send_btn.left() - 8.0 - COMPOSER_H);
        let attach_btn = circle(rect.left());
        let n_scheduled = self.scheduled.get(&peer).map_or(0, Vec::len);
        let sched_btn = circle(sticker_btn.left() - 8.0 - COMPOSER_H);
        let input_right = if n_scheduled > 0 { sched_btn.left() } else { sticker_btn.left() } - 8.0;
        // The field grows with the text (Shift+Enter), buttons stay at the bottom.
        let bot_menu = self.bot_menu_width(peer, chat.kind);
        let input_left = attach_btn.right() + 8.0 + bot_menu;
        let rows = {
            let job = LayoutJob::simple(self.draft.clone(), FontId::proportional(glass::AREA_FONT), ink(), (input_right - input_left - 40.0).max(40.0));
            ui.painter().layout_job(job).rows.len().clamp(1, COMPOSER_ROWS)
        };
        let line_h = ui.fonts_mut(|f| f.row_height(&FontId::proportional(glass::AREA_FONT)));
        let input_h = (rows as f32 * line_h + 26.0).max(COMPOSER_H);
        self.composer_h = input_h;
        let input = Rect::from_min_max(pos2(input_left, rect.bottom() - input_h), pos2(input_right, rect.bottom()));
        let mut above = input.top() - 8.0;
        if self.compose != Compose::New {
            let bar_rect = Rect::from_min_max(pos2(rect.left(), above - COMPOSE_BAR_H), pos2(send_btn.right(), above));
            self.compose_bar(ui, peer, bar_rect);
            above = bar_rect.top() - 8.0;
        }
        above = self.link_preview_bar(ui, rect.left(), send_btn.right(), above, now);
        for u in self.uploads.iter().take(3) {
            above = upload_pill(ui, u, pos2(rect.left(), above)) - 6.0;
        }
        if self.uploads.len() > 3 {
            let more = lf!("і ще {}", "and {} more", self.uploads.len() - 3);
            ui.painter().text(pos2(rect.left() + 14.0, above - 8.0), Align2::LEFT_CENTER, more, FontId::proportional(12.5), ink_dim());
            above -= 20.0;
        }
        let attach = glass::icon_button(ui, attach_btn, "attach", Icon::Attach, false, true).on_hover_text(l!("Надіслати файл", "Send file"));
        if attach.clicked() {
            self.pick_files(peer, false);
        }
        let can_poll = self.chats.iter().any(|c| c.id == peer && matches!(c.kind, ChatKind::Group | ChatKind::Channel | ChatKind::Bot | ChatKind::Saved));
        attach.context_menu(|ui| {
            if glass::menu_button(ui, Icon::File, l!("Надіслати без стиснення", "Send without compression")).clicked() {
                self.pick_files(peer, true);
            }
            // Polls go to groups, channels, bots and Saved Messages, as in Telegram.
            if can_poll && glass::menu_button(ui, Icon::Poll, l!("Опитування", "Poll")).clicked() {
                self.open_poll_form(peer);
            }
        });
        if glass::icon_button(ui, sticker_btn, "stickers", Icon::Smile, self.sticker_panel, true).on_hover_text(l!("Стікери", "Stickers")).clicked() {
            self.sticker_panel = !self.sticker_panel;
            if self.sticker_panel && !self.stickers_requested {
                self.stickers_requested = true;
                self.send(Cmd::LoadStickers);
            }
        }
        if n_scheduled > 0 {
            let hint = lf!("Заплановані: {n_scheduled}", "Scheduled: {n_scheduled}");
            if glass::icon_button(ui, sched_btn, "scheduled", Icon::Clock, self.scheduled_open, true).on_hover_text(hint).clicked() {
                self.scheduled_open = !self.scheduled_open;
            }
        }
        if self.sticker_panel {
            let panel = Rect::from_min_max(pos2((send_btn.right() - 380.0).max(rect.left()), above - 340.0), pos2(send_btn.right(), above));
            self.sticker_view(ui, peer, panel);
        }
        if self.scheduled_open && n_scheduled > 0 {
            let panel = Rect::from_min_max(pos2(rect.left(), (above - 360.0).max(header.bottom() + 8.0)), pos2(rect.right(), above));
            self.scheduled_view(ui, peer, panel);
        }
        if !self.picked.is_empty() {
            let bar = Rect::from_min_max(pos2(rect.left(), send_btn.top()), send_btn.max);
            self.selection_bar(ui, peer, bar);
            self.chat_overlays(ui, peer, rect, header, above);
            return;
        }
        if self.recorder.as_ref().is_some_and(|(p, _)| *p != peer) {
            self.cancel_recording();
        }
        if self.voice_draft.as_ref().is_some_and(|(p, _)| *p != peer) || self.round_rec.as_ref().is_some_and(|(p, _)| *p != peer) {
            self.cancel_recording();
        }
        if self.round_rec.is_some() {
            self.round_bar(ui, input, rect, now);
            if glass::icon_button(ui, send_btn, "send", Icon::Send, true, true).on_hover_text(l!("Надіслати відеоповідомлення", "Send video message")).clicked() {
                self.send_round();
            }
            self.chat_overlays(ui, peer, rect, header, above);
            return;
        }
        if self.voice_draft.is_some() {
            self.voice_draft_bar(ui, input);
            if glass::icon_button(ui, send_btn, "send", Icon::Send, true, true).on_hover_text(l!("Надіслати голосове", "Send voice message")).clicked() {
                self.send_recording();
            }
            self.chat_overlays(ui, peer, rect, header, above);
            return;
        }
        if self.recorder.is_some() {
            self.recording_bar(ui, input, now);
            if glass::icon_button(ui, send_btn, "send", Icon::Send, true, true).on_hover_text(l!("Надіслати голосове", "Send voice message")).clicked() {
                self.send_recording();
            }
            self.chat_overlays(ui, peer, rect, header, above);
            return;
        }
        self.paste_files(ui, peer);
        if bot_menu > 0.0 {
            self.bot_menu_button(ui, circle(attach_btn.right() + 8.0));
        }
        let suggest_rect = Rect::from_min_max(pos2(rect.left(), (above - 300.0).max(header.bottom() + 8.0)), pos2(send_btn.right(), above));
        self.suggest_view(ui, peer, chat.kind, suggest_rect, now);
        glass::glass(ui.painter(), input, Glass::tinted((input.height() / 2.0).min(25.0), glass::chrome(), 110).with_shadow().frosted(0.96));
        let arrow_edit = self.can_edit_by_arrow() && ui.input(|i| i.key_pressed(Key::ArrowUp) && i.modifiers.is_none());
        let send_mods = self.send_mods();
        let (resp, submitted) = glass::text_area(ui, input, &mut self.draft, l!("Повідомлення", "Message"), send_mods);
        // ↑ in an empty composer edits the last own message, as in Telegram Desktop.
        if arrow_edit && resp.has_focus() {
            self.edit_last();
        }
        if resp.changed() {
            self.draft_edited_at = now;
            if !self.draft.is_empty() && now - self.typing_sent_at > TYPING_EVERY && chat.kind != ChatKind::Saved {
                self.typing_sent_at = now;
                self.send(Cmd::Typing(peer));
            }
        }
        if now - self.draft_edited_at > DRAFT_DELAY {
            self.flush_draft();
        }
        self.update_inline(peer, now, resp.changed());
        if self.inline.as_ref().is_some_and(|i| i.results_for.is_some()) {
            let panel = Rect::from_min_max(pos2(rect.left(), (above - 320.0).max(header.bottom() + 8.0)), pos2(rect.right(), above));
            self.inline_view(ui, peer, panel);
        }
        let enter = submitted;
        let can_send = !self.draft.trim().is_empty();
        let editing = matches!(self.compose, Compose::Edit(_));
        // Empty field shows the microphone button, as in Telegram.
        let mic = !can_send && !editing;
        let icon = if editing { Icon::Check } else if mic { Icon::Mic } else { Icon::Send };
        let mut send = glass::icon_button(ui, send_btn, "send", icon, true, can_send || mic);
        if mic {
            send = send.on_hover_text(l!("Записати голосове (права кнопка — відеоповідомлення)", "Record voice (right click — video message)"));
            let mut round = false;
            send.context_menu(|ui| {
                if glass::menu_button(ui, Icon::Video, l!("Записати відеоповідомлення", "Record video message")).clicked() {
                    round = true;
                }
            });
            if round {
                self.start_round(peer);
            }
        }
        if mic && send.clicked() {
            self.start_recording(peer);
        } else if send.clicked() || (enter && can_send) {
            self.send_draft();
            resp.request_focus();
        }
        // Right-click on "send": silent or scheduled, as in Telegram Desktop.
        if can_send && !editing {
            let mut choice = None;
            send.context_menu(|ui| {
                ui.set_min_width(200.0);
                if glass::menu_button(ui, Icon::BellOff, l!("Надіслати без звуку", "Send without sound")).clicked() {
                    choice = Some(false);
                }
                if glass::menu_button(ui, Icon::Clock, l!("Запланувати…", "Schedule…")).clicked() {
                    choice = Some(true);
                }
            });
            match choice {
                Some(false) => self.send_draft_with(SendOpts { silent: true, schedule: None, ..Default::default() }),
                Some(true) => self.schedule = Some((ScheduleFor::Draft, String::new())),
                None => {}
            }
        }
        if self.schedule.is_some() {
            let panel = Rect::from_min_max(pos2((send_btn.right() - 300.0).max(rect.left()), above - 236.0), pos2(send_btn.right(), above));
            self.schedule_view(ui, panel);
        }
        self.chat_overlays(ui, peer, rect, header, above);
    }

    /// Panels over the chat (search, forwarding) and Esc handling.
    fn chat_overlays(&mut self, ui: &mut Ui, peer: PeerId, rect: Rect, header: Rect, above: f32) {
        self.chat_view_overlays(ui, peer, rect, header, above);
        if self.comments.as_ref().is_some_and(|c| c.peer != peer) {
            self.comments = None;
        }
        if self.comments.is_some() {
            let w = rect.width().min(440.0);
            let panel = Rect::from_min_max(pos2(rect.right() - w, header.bottom() + 8.0), rect.max);
            self.comments_view(ui, panel);
        }
        if self.chat_search.is_some() {
            let panel = Rect::from_min_max(pos2(rect.left(), header.bottom() + 8.0), pos2(rect.right(), (header.bottom() + 340.0).min(above - 8.0)));
            self.chat_search_view(ui, peer, panel);
        }
        if self.forward.is_some() {
            let w = rect.width().min(380.0);
            let panel = Rect::from_center_size(rect.center(), vec2(w, (rect.height() - 120.0).min(480.0)));
            self.forward_view(ui, panel);
        }
        let esc = ui.input(|i| i.key_pressed(Key::Escape));
        // Windows above the chat close themselves on this Esc.
        if !esc || self.modal_open() {
            return;
        }
        if self.viewer.is_some() {
            // Closes the viewer itself.
        } else if self.comments.is_some() {
            self.comments = None;
        } else if self.recorder.is_some() || self.voice_draft.is_some() || self.round_rec.is_some() {
            self.cancel_recording();
        } else if self.forward.is_some() {
            self.forward = None;
        } else if self.inline.as_ref().is_some_and(|i| i.results_for.is_some()) {
            self.inline = None;
        } else if self.scheduled_open {
            self.scheduled_open = false;
        } else if self.schedule.is_some() {
            self.schedule = None;
        } else if self.sticker_panel {
            self.sticker_panel = false;
        } else if self.chat_search.is_some() {
            self.chat_search = None;
        } else if !self.picked.is_empty() {
            self.picked.clear();
        } else if self.compose != Compose::New {
            self.cancel_compose();
        } else if let Some(i) = &mut self.info {
            if i.confirm.take().is_none() {
                self.info = None;
            }
        } else if self.topic.is_some() {
            self.topic = None;
        } else {
            self.esc_fallback();
        }
    }

    /// Bottom panel in message selection mode.
    fn selection_bar(&mut self, ui: &mut Ui, peer: PeerId, rect: Rect) {
        glass::glass(ui.painter(), rect, Glass::tinted(rect.height() / 2.0, glass::chrome(), 140).with_shadow().frosted(0.96));
        let close = Rect::from_center_size(pos2(rect.left() + rect.height() / 2.0, rect.center().y), vec2(36.0, 36.0));
        if glass::icon_button(ui, close, "pick-close", Icon::Close, false, true).clicked() {
            self.picked.clear();
            return;
        }
        let n = self.picked.len();
        ui.painter().text(pos2(close.right() + 12.0, rect.center().y), Align2::LEFT_CENTER, lf!("Вибрано: {n}", "Selected: {n}"), FontId::proportional(15.0), ink());
        let all_out = self.picked.iter().all(|id| self.find(peer, *id).is_some_and(|m| m.out));
        let btn = |i: f32| Rect::from_center_size(pos2(rect.right() - 30.0 - i * 46.0, rect.center().y), vec2(38.0, 38.0));
        let protected = self.protected(peer, &self.picked);
        if protected {
            if all_out && glass::icon_button(ui, btn(0.0), "pick-delete", Icon::Trash, false, true).on_hover_text(l!("Видалити", "Delete")).clicked() {
                let ids = std::mem::take(&mut self.picked);
                self.send(Cmd::Delete { peer, ids });
            }
            return;
        }
        if glass::icon_button(ui, btn(0.0), "pick-forward", Icon::Forward, true, true).on_hover_text(l!("Переслати", "Forward")).clicked() {
            let mut ids = std::mem::take(&mut self.picked);
            ids.sort_unstable();
            self.forward = Some(ForwardPicker { from: peer, ids, query: String::new() });
        }
        if glass::icon_button(ui, btn(1.0), "pick-copy", Icon::Copy, false, true).on_hover_text(l!("Копіювати", "Copy")).clicked() {
            let mut msgs: Vec<&Msg> = self.picked.iter().filter_map(|id| self.find(peer, *id)).collect();
            msgs.sort_by_key(|m| m.id);
            let text = msgs.iter().map(|m| format!("{} [{}]\n{}", author(m, &self.me), m.time, m.text)).collect::<Vec<_>>().join("\n\n");
            self.ctx.copy_text(text);
            self.picked.clear();
            self.toast = Some((l!("Скопійовано", "Copied").into(), 0.0, false));
        }
        if all_out && glass::icon_button(ui, btn(2.0), "pick-delete", Icon::Trash, false, true).on_hover_text(l!("Видалити", "Delete")).clicked() {
            let ids = std::mem::take(&mut self.picked);
            self.send(Cmd::Delete { peer, ids });
        }
    }

    /// "Forward to…" window: search and chat list.
    fn forward_view(&mut self, ui: &mut Ui, rect: Rect) {
        let Some(fw) = &mut self.forward else { return };
        // Dim the background and swallow clicks under the window.
        ui.painter().rect_filled(ui.max_rect(), 0.0, Color32::from_black_alpha(90));
        ui.interact(ui.max_rect(), ui.scope_id().with("forward-bg"), Sense::click());
        glass::glass(ui.painter(), rect, Glass::panel(26.0).with_shadow());
        let inner = rect.shrink(16.0);
        let title = lf!("Переслати ({})", "Forward ({})", fw.ids.len());
        ui.painter().text(inner.left_top() + vec2(4.0, 2.0), Align2::LEFT_TOP, title, FontId::proportional(18.0), ink());
        let close = Rect::from_min_size(pos2(inner.right() - 32.0, inner.top() - 4.0), vec2(32.0, 32.0));
        let mut cancel = glass::icon_button(ui, close, "fw-close", Icon::Close, false, true).clicked();
        let field = Rect::from_min_size(inner.left_top() + vec2(0.0, 38.0), vec2(inner.width(), 38.0));
        let resp = glass::text_field(ui, field, &mut fw.query, l!("Кому переслати…", "Forward to…"), false);
        if !resp.has_focus() && fw.query.is_empty() {
            resp.request_focus();
        }
        let list = Rect::from_min_max(pos2(inner.left() - 6.0, field.bottom() + 8.0), pos2(inner.right() + 6.0, inner.bottom()));
        let q = fw.query.trim().to_lowercase();
        let mut chosen = None;
        let mut child = ui.new_child(UiBuilder::new().max_rect(list));
        child.set_clip_rect(list);
        egui::ScrollArea::vertical().id_salt("forward-list").auto_shrink(false).show(&mut child, |ui| {
            ui.spacing_mut().item_spacing.y = 2.0;
            for c in self.chats.iter().filter(|c| c.title.to_lowercase().contains(&q) && c.kind != ChatKind::Channel) {
                let hit = SearchHit { chat: c.title.clone(), kind: c.kind, msg: Msg::placeholder(c.id) };
                if hit_row(ui, &hit, "", self.textures.get(&ImgKey::Avatar(c.id))).clicked() {
                    chosen = Some(c.id);
                }
            }
        });
        if let Some(to) = chosen
            && let Some(fw) = self.forward.take()
        {
            self.send(Cmd::Forward { from: fw.from, ids: fw.ids, to });
            self.open_chat(to);
            cancel = false;
        }
        if cancel {
            self.forward = None;
        }
    }

    /// Scheduled send time picker.
    fn schedule_view(&mut self, ui: &mut Ui, rect: Rect) {
        glass::glass(ui.painter(), rect, Glass::tinted(22.0, glass::chrome(), 150).frosted(1.0).with_shadow());
        ui.interact(rect, ui.scope_id().with("schedule-bg"), Sense::click());
        let inner = rect.shrink(14.0);
        let title = match self.schedule {
            Some((ScheduleFor::Existing(_), _)) => l!("Новий час надсилання", "New send time"),
            _ => l!("Надіслати пізніше", "Send later"),
        };
        ui.painter().text(inner.left_top(), Align2::LEFT_TOP, title, FontId::proportional(15.5), ink());
        let now = jiff::Zoned::now();
        let at = |d: jiff::civil::Date, h: i8, m: i8| d.at(h, m, 0, 0).to_zoned(now.time_zone().clone()).ok().map(|z| z.timestamp().as_second());
        let tomorrow = now.date().tomorrow().unwrap_or(now.date());
        let presets = [
            (l!("Через годину", "In an hour").to_owned(), Some(now.timestamp().as_second() + 3600)),
            (l!("Сьогодні о 21:00", "Today at 21:00").to_owned(), at(now.date(), 21, 0).filter(|t| *t > now.timestamp().as_second() + 60)),
            (l!("Завтра о 9:00", "Tomorrow at 9:00").to_owned(), at(tomorrow, 9, 0)),
        ];
        let mut chosen = None;
        let mut y = inner.top() + 30.0;
        for (i, (label, ts)) in presets.into_iter().enumerate() {
            let Some(ts) = ts else { continue };
            let r = Rect::from_min_size(pos2(inner.left(), y), vec2(inner.width(), 34.0));
            if glass::button(ui, r, ("sched", i), &label, false, true).clicked() {
                chosen = Some(ts);
            }
            y += 40.0;
        }
        let field = Rect::from_min_size(pos2(inner.left(), y + 4.0), vec2(inner.width() - 96.0, 36.0));
        let Some((target, text)) = &mut self.schedule else { return };
        let target = *target;
        glass::text_field(ui, field, text, l!("ГГ:ХХ", "HH:MM"), false);
        // Time without a date: today, or tomorrow if it has already passed.
        let parsed = text.trim().split_once(':').and_then(|(h, m)| Some((h.parse::<i8>().ok()?, m.parse::<i8>().ok()?)));
        let custom = parsed.and_then(|(h, m)| {
            let today = at(now.date(), h, m)?;
            if today > now.timestamp().as_second() + 60 { Some(today) } else { at(tomorrow, h, m) }
        });
        let ok = Rect::from_min_size(pos2(field.right() + 8.0, field.top()), vec2(88.0, 36.0));
        if glass::button(ui, ok, "sched-ok", l!("Гаразд", "OK"), true, custom.is_some()).clicked() {
            chosen = custom;
        }
        if let Some(ts) = chosen {
            self.schedule = None;
            match (target, self.selected) {
                (ScheduleFor::Draft, _) => self.send_draft_with(SendOpts { silent: false, schedule: Some(ts), ..Default::default() }),
                (ScheduleFor::Existing(id), Some(peer)) => self.send(Cmd::Reschedule { peer, id, at: ts }),
                _ => {}
            }
        }
    }

    /// Scheduled messages list: send now, change time, delete.
    fn scheduled_view(&mut self, ui: &mut Ui, peer: PeerId, rect: Rect) {
        let Some(list) = self.scheduled.get(&peer) else { return };
        let rows = list.len() as f32 * 62.0 + 46.0;
        let rect = Rect::from_min_max(pos2(rect.left(), (rect.bottom() - rows).max(rect.top())), rect.max);
        glass::glass(ui.painter(), rect, Glass::tinted(22.0, glass::chrome(), 160).frosted(1.0).with_shadow());
        ui.interact(rect, ui.scope_id().with("scheduled-bg"), Sense::click());
        let inner = rect.shrink2(vec2(12.0, 12.0));
        ui.painter().text(inner.left_top() + vec2(4.0, 0.0), Align2::LEFT_TOP, l!("Заплановані повідомлення", "Scheduled messages"), FontId::proportional(15.0), ink());
        let area = Rect::from_min_max(pos2(inner.left(), inner.top() + 28.0), inner.max);
        let mut cmd = None;
        let mut child = ui.new_child(UiBuilder::new().max_rect(area));
        child.set_clip_rect(area);
        egui::ScrollArea::vertical().id_salt(("scheduled", peer)).auto_shrink(false).show(&mut child, |ui| {
            for m in list {
                let w = ui.available_width();
                let (r, _) = ui.allocate_exact_size(vec2(w, 58.0), Sense::hover());
                let p = ui.painter();
                p.rect_filled(r.shrink(2.0), 14.0, glass::veil(10));
                icons::draw(p, Rect::from_center_size(pos2(r.left() + 22.0, r.center().y), vec2(18.0, 18.0)), Icon::Clock, ACCENT);
                let when = format!("{} {}", m.day, m.time);
                p.text(pos2(r.left() + 42.0, r.top() + 10.0), Align2::LEFT_TOP, when, FontId::proportional(12.5), ACCENT);
                let text = elided(ui, &m.text.replace('\n', " "), 14.0, ink(), w - 42.0 - 140.0);
                ui.painter().galley(pos2(r.left() + 42.0, r.top() + 29.0), text, ink());
                let b = |i: f32| Rect::from_center_size(pos2(r.right() - 24.0 - i * 42.0, r.center().y), vec2(34.0, 34.0));
                if glass::icon_button(ui, b(2.0), ("sch-now", m.id), Icon::Send, false, true).on_hover_text(l!("Надіслати зараз", "Send now")).clicked() {
                    cmd = Some(Cmd::SendScheduledNow { peer, ids: vec![m.id] });
                }
                if glass::icon_button(ui, b(1.0), ("sch-time", m.id), Icon::Clock, false, true).on_hover_text(l!("Змінити час", "Change time")).clicked() {
                    self.schedule = Some((ScheduleFor::Existing(m.id), String::new()));
                }
                if glass::icon_button(ui, b(0.0), ("sch-del", m.id), Icon::Trash, false, true).on_hover_text(l!("Видалити", "Delete")).clicked() {
                    cmd = Some(Cmd::DeleteScheduled { peer, ids: vec![m.id] });
                }
            }
        });
        if let Some(cmd) = cmd {
            self.send(cmd);
        }
        if matches!(self.schedule, Some((ScheduleFor::Existing(_), _))) {
            let panel = Rect::from_min_max(pos2((rect.right() - 300.0).max(rect.left()), rect.top() - 244.0), pos2(rect.right(), rect.top() - 8.0));
            self.schedule_view(ui, panel);
        }
    }

    /// `@bot query` in the input field triggers an inline query after a pause in typing.
    fn update_inline(&mut self, peer: PeerId, now: f64, changed: bool) {
        if changed {
            self.inline_edited_at = now;
        }
        let parsed = self.draft.strip_prefix('@').and_then(|rest| {
            let (bot, query) = rest.split_once(' ')?;
            let ok = bot.len() >= 3 && bot.chars().all(|c| c.is_ascii_alphanumeric() || c == '_');
            ok.then(|| (bot.to_owned(), query.to_owned()))
        });
        let Some((bot, query)) = parsed else {
            self.inline = None;
            return;
        };
        let i = self.inline.get_or_insert_with(|| Inline {
            bot: String::new(),
            query: String::new(),
            requested: None,
            results: Vec::new(),
            query_id: 0,
            results_for: None,
        });
        (i.bot, i.query) = (bot.clone(), query.clone());
        let key = Some((bot.clone(), query.clone()));
        if i.requested != key && now - self.inline_edited_at > SEARCH_DELAY {
            i.requested = key;
            self.send(Cmd::InlineQuery { peer, bot, query });
        }
    }

    fn inline_view(&mut self, ui: &mut Ui, peer: PeerId, rect: Rect) {
        let Some(i) = &self.inline else { return };
        let rows = (i.results.len().max(1) as f32 * 58.0 + 20.0).min(rect.height());
        let rect = Rect::from_min_max(pos2(rect.left(), rect.bottom() - rows), rect.max);
        glass::glass(ui.painter(), rect, Glass::tinted(22.0, glass::chrome(), 160).frosted(1.0).with_shadow());
        ui.interact(rect, ui.scope_id().with("inline-bg"), Sense::click());
        if i.results.is_empty() {
            let text = lf!("@{} нічого не знайшов", "@{} found nothing", i.bot);
            ui.painter().text(rect.center(), Align2::CENTER_CENTER, text, FontId::proportional(14.0), ink_dim());
            return;
        }
        let inner = rect.shrink2(vec2(8.0, 10.0));
        let mut chosen = None;
        let mut child = ui.new_child(UiBuilder::new().max_rect(inner));
        child.set_clip_rect(inner);
        egui::ScrollArea::vertical().id_salt("inline-results").auto_shrink(false).show(&mut child, |ui| {
            for r in &i.results {
                let w = ui.available_width();
                let (row, resp) = ui.allocate_exact_size(vec2(w, 56.0), Sense::click());
                let hover = ui.ctx().animate_bool(resp.id, resp.hovered());
                if hover > 0.0 {
                    ui.painter().rect_filled(row, 14.0, glass::veil((hover * 16.0) as u8));
                }
                let kind = match r.kind.as_str() {
                    "photo" => Icon::Camera,
                    "article" => Icon::File,
                    "location" | "venue" => Icon::Location,
                    _ => Icon::Attach,
                };
                let badge = Rect::from_center_size(pos2(row.left() + 26.0, row.center().y), vec2(38.0, 38.0));
                glass::glass(ui.painter(), badge, Glass::tinted(19.0, ACCENT, 150));
                icons::draw(ui.painter(), Rect::from_center_size(badge.center(), vec2(18.0, 18.0)), kind, ink());
                let title = if r.title.is_empty() { r.kind.clone() } else { r.title.clone() };
                ui.painter().galley(pos2(row.left() + 56.0, row.top() + 9.0), elided(ui, &title, 14.5, ink(), w - 66.0), ink());
                ui.painter().galley(pos2(row.left() + 56.0, row.top() + 30.0), elided(ui, &r.description, 13.0, ink_dim(), w - 66.0), ink_dim());
                if resp.clicked() {
                    chosen = Some(r.id.clone());
                }
            }
        });
        if let Some(result) = chosen
            && let Some(i) = self.inline.take()
        {
            let reply_to = self.reply_target();
            self.send(Cmd::SendInline { peer, query_id: i.query_id, result, reply_to });
            self.draft.clear();
            self.compose = Compose::New;
        }
    }

    /// Ctrl+V with an image or copied files sends them instead of pasting text.
    fn paste_files(&mut self, ui: &mut Ui, peer: PeerId) {
        let pasted = ui.input(|i| {
            i.events.iter().any(|e| matches!(e, egui::Event::Paste(_)))
                || (i.modifiers.command && i.key_pressed(Key::V))
        });
        if !pasted {
            return;
        }
        let Ok(mut clip) = arboard::Clipboard::new() else { return };
        let files: Vec<PathBuf> = match clip.get().file_list() {
            Ok(list) if !list.is_empty() => list,
            _ => match clip.get_image() {
                Ok(img) => save_clipboard_image(&img).into_iter().collect(),
                Err(_) => return,
            },
        };
        if files.is_empty() {
            return;
        }
        ui.ctx().input_mut(|i| i.events.retain(|e| !matches!(e, egui::Event::Paste(_))));
        self.send_files(peer, files, false);
    }

    /// Emoji and sticker panel: tabs on top (emoji, then sets), grid below.
    fn sticker_view(&mut self, ui: &mut Ui, peer: PeerId, rect: Rect) {
        glass::glass(ui.painter(), rect, Glass::tinted(24.0, glass::chrome(), 120).frosted(1.0).with_shadow());
        // Swallow clicks so they don't reach the messages under the panel.
        ui.interact(rect, ui.scope_id().with("sticker-bg"), Sense::click());
        let inner = rect.shrink(10.0);
        let tabs = Rect::from_min_size(inner.min, vec2(inner.width(), 40.0));
        let mut chosen_set = None;
        let mut emoji_clicked = false;
        let mut child = ui.new_child(UiBuilder::new().max_rect(tabs));
        child.set_clip_rect(tabs);
        egui::ScrollArea::horizontal()
            .id_salt("sticker-tabs")
            .scroll_bar_visibility(egui::scroll_area::ScrollBarVisibility::AlwaysHidden)
            .show(&mut child, |ui| {
                ui.horizontal(|ui| {
                    ui.spacing_mut().item_spacing.x = 6.0;
                    let tab = |ui: &mut Ui, active: bool| {
                        let (r, resp) = ui.allocate_exact_size(vec2(40.0, 40.0), Sense::click());
                        if active {
                            glass::glass(ui.painter(), r, Glass::tinted(12.0, ACCENT, 120));
                        } else if resp.hovered() {
                            ui.painter().rect_filled(r, 12.0, glass::veil(18));
                        }
                        (r, resp)
                    };
                    let (r, resp) = tab(ui, self.emoji_tab);
                    icons::draw(ui.painter(), Rect::from_center_size(r.center(), vec2(22.0, 22.0)), Icon::Smile, ink());
                    emoji_clicked = resp.on_hover_text(l!("Емодзі", "Emoji")).clicked();
                    for (i, set) in self.stickers.iter().enumerate() {
                        let (r, resp) = tab(ui, !self.emoji_tab && i == self.sticker_set);
                        let first = set.stickers.first();
                        match first.and_then(|s| self.textures.get(&ImgKey::Sticker(s.0))) {
                            Some(t) => glass::image_contain(ui.painter(), r.shrink(5.0), t),
                            None => {
                                let label = first.map(|s| s.1.clone()).filter(|e| !e.is_empty()).unwrap_or_else(|| glass::initials(&set.title));
                                ui.painter().text(r.center(), Align2::CENTER_CENTER, label, FontId::proportional(20.0), ink());
                            }
                        }
                        if resp.on_hover_text(&set.title).clicked() {
                            chosen_set = Some(i);
                        }
                    }
                    if self.stickers.is_empty() && self.stickers_requested && self.backend.is_some() {
                        ui.add(egui::Spinner::new().color(ink_dim()));
                    }
                });
            });
        if emoji_clicked {
            self.emoji_tab = true;
        }
        if let Some(i) = chosen_set {
            self.emoji_tab = false;
            self.sticker_set = i;
            self.request_sticker_thumbs();
        }
        let body = Rect::from_min_max(pos2(inner.left(), tabs.bottom() + 6.0), inner.max);
        if self.emoji_tab {
            return self.emoji_grid(ui, body);
        }

        let Some(set) = self.stickers.get(self.sticker_set) else {
            ui.painter().text(body.center(), Align2::CENTER_CENTER, l!("Немає встановлених стікерів", "No stickers installed"), FontId::proportional(14.0), ink_dim());
            return;
        };
        ui.painter().text(pos2(body.left() + 4.0, body.top()), Align2::LEFT_TOP, &set.title, FontId::proportional(13.0), ink_dim());
        let grid = Rect::from_min_max(pos2(body.left(), body.top() + 22.0), body.max);
        let mut send = None;
        let mut child = ui.new_child(UiBuilder::new().max_rect(grid));
        child.set_clip_rect(grid);
        egui::ScrollArea::vertical()
            .id_salt(("sticker-grid", set.id))
            .auto_shrink(false)
            .show(&mut child, |ui| {
                let cols = ((grid.width() / 70.0).floor() as usize).max(1);
                let cell = grid.width() / cols as f32 - 2.0;
                for row in set.stickers.chunks(cols) {
                    ui.horizontal(|ui| {
                        ui.spacing_mut().item_spacing.x = 2.0;
                        for (id, emoji) in row {
                            let (r, resp) = ui.allocate_exact_size(vec2(cell, cell), Sense::click());
                            let hover = ui.ctx().animate_bool(resp.id, resp.hovered());
                            if hover > 0.0 {
                                ui.painter().rect_filled(r, 14.0, glass::veil((hover * 22.0) as u8));
                            }
                            let r = r.shrink(6.0 - hover * 2.0);
                            match self.textures.get(&ImgKey::Sticker(*id)) {
                                Some(t) => glass::image_contain(ui.painter(), r, t),
                                None => {
                                    ui.painter().text(r.center(), Align2::CENTER_CENTER, emoji, FontId::proportional(cell * 0.45), ink());
                                }
                            }
                            if resp.clicked() {
                                send = Some(*id);
                            }
                        }
                    });
                }
            });
        if let Some(doc) = send {
            let reply_to = self.reply_target();
            self.compose = Compose::New;
            self.send(Cmd::SendSticker { peer, doc, reply_to });
        }
    }

    /// Emoji grid by category; a click adds the emoji to the input field.
    fn emoji_grid(&mut self, ui: &mut Ui, rect: Rect) {
        let mut picked = None;
        let mut child = ui.new_child(UiBuilder::new().max_rect(rect));
        child.set_clip_rect(rect);
        egui::ScrollArea::vertical().id_salt("emoji-grid").auto_shrink(false).show(&mut child, |ui| {
            let cols = ((rect.width() / 40.0).floor() as usize).max(1);
            let cell = rect.width() / cols as f32 - 1.0;
            for (title, chars) in emoji::CATEGORIES {
                let title = title();
                ui.label(egui::RichText::new(title).size(12.5).color(ink_dim()));
                for row in emoji::list(chars).chunks(cols) {
                    ui.horizontal(|ui| {
                        ui.spacing_mut().item_spacing.x = 1.0;
                        for e in row {
                            let (r, resp) = ui.allocate_exact_size(vec2(cell, cell), Sense::click());
                            if resp.hovered() {
                                ui.painter().rect_filled(r, 10.0, glass::veil(22));
                            }
                            ui.painter().text(r.center(), Align2::CENTER_CENTER, e, FontId::proportional(cell * 0.62), ink());
                            if resp.clicked() {
                                picked = Some(e.clone());
                            }
                        }
                    });
                }
                ui.add_space(6.0);
            }
        });
        if let Some(e) = picked {
            self.draft.push_str(&e);
            self.draft_edited_at = ui.input(|i| i.time);
        }
    }

    /// Highlight while dragging files into the window.
    fn drop_overlay(&mut self, ui: &mut Ui) {
        let (hovering, dropped) = ui.input(|i| (!i.raw.hovered_files.is_empty(), i.raw.dropped_files.clone()));
        let Some(peer) = self.selected else { return };
        if hovering {
            let r = ui.max_rect().shrink(40.0);
            glass::glass(ui.painter(), r, Glass::tinted(36.0, ACCENT, 90).frosted(0.9).with_shadow());
            ui.painter().text(r.center(), Align2::CENTER_CENTER, l!("Відпустіть, щоб надіслати", "Drop to send"), FontId::proportional(24.0), ink());
        }
        let paths: Vec<PathBuf> = dropped.iter().map(|f| f.path().to_path_buf()).filter(|p| !p.as_os_str().is_empty()).collect();
        self.send_files(peer, paths, false);
    }

    /// Strip above the input field: "Reply …" or "Editing".
    fn compose_bar(&mut self, ui: &mut Ui, peer: PeerId, rect: Rect) {
        let (icon, title, id) = match self.compose {
            Compose::Reply(id) => (Icon::Reply, l!("Відповідь", "Reply"), id),
            Compose::Edit(id) => (Icon::Pencil, l!("Редагування", "Editing"), id),
            Compose::New => return,
        };
        glass::glass(ui.painter(), rect, Glass::tinted(rect.height() / 2.0, glass::chrome(), 130).with_shadow().frosted(0.96));
        let p = ui.painter().clone();
        icons::draw(&p, Rect::from_center_size(pos2(rect.left() + 20.0, rect.center().y), vec2(18.0, 18.0)), icon, ACCENT);
        p.rect_filled(Rect::from_min_size(pos2(rect.left() + 36.0, rect.top() + 8.0), vec2(3.0, rect.height() - 16.0)), 2.0, ACCENT);
        let (who, what) = match self.find(peer, id) {
            Some(m) => (if matches!(self.compose, Compose::Edit(_)) { title.to_owned() } else { format!("{title}: {}", author(m, &self.me)) }, quote(m)),
            None => (title.to_owned(), String::new()),
        };
        let w = rect.width() - 100.0;
        let g1 = elided(ui, &who, 12.5, ACCENT, w);
        let g2 = elided(ui, &what, 13.0, ink_dim(), w);
        p.galley(pos2(rect.left() + 46.0, rect.top() + 5.0), g1, ACCENT);
        p.galley(pos2(rect.left() + 46.0, rect.top() + 21.0), g2, ink_dim());
        let close = Rect::from_center_size(pos2(rect.right() - 22.0, rect.center().y), vec2(28.0, 28.0));
        if glass::icon_button(ui, close, "compose-close", Icon::Close, false, true).clicked() {
            self.cancel_compose();
        }
    }

    fn cancel_compose(&mut self) {
        if matches!(self.compose, Compose::Edit(_)) {
            self.draft = std::mem::take(&mut self.edit_stash);
        }
        self.compose = Compose::New;
    }

    /// In-chat search results: a glass panel under the header.
    fn chat_search_view(&mut self, ui: &mut Ui, peer: PeerId, rect: Rect) {
        let Some(s) = &self.chat_search else { return };
        if s.query.trim().chars().count() < 2 {
            return;
        }
        let ready = s.hits_for == s.query.trim();
        let rows = if ready { s.hits.len().max(1) } else { 1 };
        let rect = Rect::from_min_size(rect.min, vec2(rect.width(), (rows as f32 * 58.0 + 20.0).min(rect.height())));
        glass::glass(ui.painter(), rect, Glass::tinted(22.0, glass::chrome(), 150).frosted(1.0).with_shadow());
        ui.interact(rect, ui.scope_id().with("chat-search-bg"), Sense::click());
        if !ready {
            ui.put(Rect::from_center_size(rect.center(), vec2(22.0, 22.0)), egui::Spinner::new().size(22.0).color(ink()));
            return;
        }
        if s.hits.is_empty() {
            ui.painter().text(rect.center(), Align2::CENTER_CENTER, l!("Нічого не знайдено", "Nothing found"), FontId::proportional(14.0), ink_dim());
            return;
        }
        let inner = rect.shrink2(vec2(8.0, 10.0));
        let mut chosen = None;
        let mut child = ui.new_child(UiBuilder::new().max_rect(inner));
        child.set_clip_rect(inner);
        egui::ScrollArea::vertical()
            .id_salt(("chat-search", peer))
            .auto_shrink(false)
            .show(&mut child, |ui| {
                ui.spacing_mut().item_spacing.y = 2.0;
                for hit in &s.hits {
                    let who = author(&hit.msg, &self.me);
                    let hit = SearchHit { chat: who, ..hit.clone() };
                    if hit_row(ui, &hit, &s.hits_for, None).clicked() {
                        chosen = Some(hit);
                    }
                }
            });
        if let Some(hit) = chosen {
            self.jump_to(&hit);
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
        // Wait for the backend to finish writing the cache, but don't hold the window long.
        if let Some(b) = &self.backend {
            let _ = b.done.recv_timeout(std::time::Duration::from_secs(2));
        }
    }

    fn ui(&mut self, ui: &mut Ui, frame: &mut eframe::Frame) {
        self.close_to_tray(ui.ctx(), frame);
        let events: Vec<Ev> = self.backend.as_ref().map(|b| b.rx.try_iter().collect()).unwrap_or_default();
        let account = self.accounts.active;
        self.poll_web_apps();
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
            self.last_activity = now;
        }
        let idle = self.settings.lock_minutes as f64 * 60.0;
        if !self.settings.passcode.is_empty() && idle > 0.0 && now - self.last_activity > idle {
            self.locked = true;
        }
        if self.locked && !self.settings.passcode.is_empty() && matches!(self.screen, Screen::Main) {
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
        self.chatlist_overlays(ui);
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
        self.contact_form_view(ui);
        self.poll_form_view(ui);
        self.chat_edit_view(ui);
        self.privacy_pick_view(ui);
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
) -> egui::Response {
    let w = ui.available_width();
    let (rect, resp) = ui.allocate_exact_size(vec2(w, 66.0), Sense::click());
    let hover = ui.ctx().animate_bool(resp.id, resp.hovered());
    let sel = ui.ctx().animate_bool(resp.id.with("sel"), selected);
    let p = ui.painter();
    if sel > 0.0 {
        glass::glass(p, rect, Glass::tinted(20.0, ACCENT, (sel * 110.0) as u8));
    } else if hover > 0.0 {
        p.rect_filled(rect, 20.0, glass::veil((hover * 14.0) as u8));
    }
    let seed = chat.id.bot_api_dialog_id_unchecked();
    let center = pos2(rect.left() + 34.0, rect.center().y);
    glass::avatar(p, center, 24.0, &chat.title, seed, kind_icon(chat.kind), avatar);
    if chat.presence.online() {
        let dot = center + vec2(17.0, 17.0);
        p.circle_filled(dot, 6.5, glass::chrome());
        p.circle_filled(dot, 4.5, Color32::from_rgb(70, 220, 120));
    }

    let x = rect.left() + 68.0;
    let time = elided(ui, &chat.last_time, 12.0, ink_dim(), 80.0);
    let time_w = time.size().x;
    p.galley(pos2(rect.right() - 12.0 - time_w, rect.top() + 14.0), time, ink_dim());

    let title_icon = match chat.kind {
        ChatKind::Channel => Some(Icon::Channel),
        ChatKind::Bot => Some(Icon::Bot),
        _ => None,
    };
    let title_y = rect.top() + 21.0;
    let mut tx = x;
    if let Some(icon) = title_icon {
        icons::draw(p, Rect::from_center_size(pos2(tx + 7.0, title_y), vec2(15.0, 15.0)), icon, ink_dim());
        tx += 19.0;
    }
    let mute_w = if chat.muted { 18.0 } else { 0.0 };
    let title = elided(ui, &chat.title, 15.0, ink(), rect.right() - tx - time_w - 22.0 - mute_w);
    let title_w = title.size().x;
    p.galley(pos2(tx, rect.top() + 12.0), title, ink());
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
        let b = Rect::from_min_size(pos2(rect.right() - 12.0 - bw, rect.bottom() - 32.0), vec2(bw, 22.0));
        // As in Telegram: muted chats have a grey counter.
        let (tint, alpha) = if chat.muted { (Color32::from_rgb(150, 158, 190), 120) } else { (ACCENT, 230) };
        glass::glass(p, b, Glass::tinted(11.0, tint, alpha));
        p.galley(b.center() - g.size() / 2.0, g, Color32::WHITE);
        bw + 8.0
    } else if chat.pinned {
        icons::draw(p, Rect::from_center_size(pos2(rect.right() - 20.0, rect.bottom() - 21.0), vec2(14.0, 14.0)), Icon::Pin, ink_dim());
        22.0
    } else {
        0.0
    };
    let preview_w = rect.right() - x - badge_w - 14.0;
    let preview_pos = pos2(x, rect.top() + 36.0);
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

/// Date between messages; a click opens the calendar, as in Telegram Desktop.
fn day_chip(ui: &mut Ui, day: &str) -> egui::Response {
    let w = ui.available_width();
    let (r, _) = ui.allocate_exact_size(vec2(w, 40.0), Sense::hover());
    let g = elided(ui, day, 12.5, ink(), 300.0);
    let chip = Rect::from_center_size(r.center(), g.size() + vec2(24.0, 12.0));
    glass::glass(ui.painter(), chip, Glass::tinted(chip.height() / 2.0, Color32::WHITE, 26).frosted(1.0));
    ui.painter().galley(chip.center() - g.size() / 2.0, g, ink());
    let resp = ui.interact(chip, ui.scope_id().with(("day-chip", day)), Sense::click());
    if resp.hovered() {
        ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
    }
    resp.on_hover_text(l!("Перейти до дати", "Jump to date"))
}

/// Message bubble: [photo] + [author] [reply quote] [text] [time].
fn message_menu(ui: &mut Ui, m: &Msg, chat: Option<&ChatInfo>, action: &mut Option<Action>) {
    // Protected content: no copying, saving or forwarding, as in Telegram.
    let protected = m.noforwards || chat.is_some_and(|c| c.noforwards);
    ui.set_min_width(170.0);
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = 2.0;
        for e in emoji::QUICK_REACTIONS.iter().take(6) {
            if ui.add(egui::Button::new(egui::RichText::new(*e).size(18.0)).frame(false)).clicked() {
                *action = Some(Action::React(m.id, (*e).to_owned()));
            }
        }
    });
    ui.separator();
    if glass::menu_button(ui, Icon::Reply, l!("Відповісти", "Reply")).clicked() {
        *action = Some(Action::Reply(m.id));
    }
    if !protected && !m.text.is_empty() && glass::menu_button(ui, Icon::Copy, l!("Копіювати текст", "Copy text")).clicked() {
        *action = Some(Action::Copy(m.text.clone()));
    }
    if !m.out && m.id > 0 && !m.text.is_empty() && glass::menu_button(ui, Icon::Translate, l!("Перекласти", "Translate")).clicked() {
        *action = Some(Action::Translate(m.id));
    }
    if let Some(c) = chat {
        chatview::pin_menu(ui, m, c, action);
    }
    if !protected && (m.photo || m.file.is_some() || m.media.is_some()) && glass::menu_button(ui, Icon::Download, l!("Завантажити", "Download")).clicked() {
        *action = Some(Action::Download(m.id));
    }
    if m.out && !m.photo && m.sticker.is_none() && m.file.is_none() && m.media.is_none() && glass::menu_button(ui, Icon::Pencil, l!("Редагувати", "Edit")).clicked() {
        *action = Some(Action::Edit(m.id));
    }
    if !protected
        && m.media.as_ref().is_some_and(|i| matches!(i.kind, MediaKind::Video | MediaKind::Gif | MediaKind::Round))
        && glass::menu_button(ui, Icon::PlayCircle, l!("Відкрити в програвачі", "Open in player")).clicked()
    {
        *action = Some(Action::OpenExternal(m.id));
    }
    if !protected && glass::menu_button(ui, Icon::Forward, l!("Переслати", "Forward")).clicked() {
        *action = Some(Action::Forward(m.id));
    }
    if glass::menu_button(ui, Icon::Select, l!("Вибрати", "Select")).clicked() {
        *action = Some(Action::Pick(m.id));
    }
    if m.out && chat.is_some_and(|c| c.kind == ChatKind::Group) && glass::menu_button(ui, Icon::Eye, l!("Хто переглянув", "Seen by")).clicked() {
        *action = Some(Action::ReadBy(m.id));
    }
    if m.out && glass::menu_button(ui, Icon::Trash, l!("Видалити", "Delete")).clicked() {
        *action = Some(Action::Delete(m.id));
    }
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

/// Selection mark next to a message and row highlight.
fn pick_mark(ui: &Ui, row: Rect, look: Look) {
    if !look.picking {
        return;
    }
    let p = ui.painter();
    if look.picked {
        p.rect_filled(row, 12.0, rgba(ACCENT, 40));
    }
    let c = pos2(row.left() + 16.0, row.center().y);
    if look.picked {
        p.circle_filled(c, 10.0, ACCENT);
        icons::draw(p, Rect::from_center_size(c, vec2(13.0, 13.0)), Icon::Check, Color32::WHITE);
    } else {
        p.circle_stroke(c, 9.5, egui::Stroke::new(1.5, rgba(ink(), 160)));
    }
    ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
}

/// Bot buttons under the bubble.
fn bot_buttons(ui: &mut Ui, m: &Msg, x: f32, width: f32) -> Option<Action> {
    let mut action = None;
    for (i, row) in m.buttons.iter().enumerate() {
        let (r, _) = ui.allocate_exact_size(vec2(ui.available_width(), 38.0), Sense::hover());
        let w = (width - 4.0 * (row.len() as f32 - 1.0)) / row.len().max(1) as f32;
        for (j, b) in row.iter().enumerate() {
            let br = Rect::from_min_size(pos2(x + j as f32 * (w + 4.0), r.top() + 2.0), vec2(w, 34.0));
            let label = match b.action {
                ButtonAction::Url(_) => format!("{} ↗", b.text),
                _ => b.text.clone(),
            };
            let resp = glass::button(ui, br, ("bot-btn", m.id, i, j), &label, false, true);
            if resp.clicked() {
                action = Some(Action::Button(m.id, i, j));
            }
        }
    }
    action
}

/// Sticker: no bubble, as in Telegram, with the time in a small glass chip.
fn sticker_bubble(ui: &mut Ui, m: &Msg, emoji: &str, tex: Option<&TextureHandle>, look: Look) -> Option<Action> {
    const SIZE: f32 = 160.0;
    let avail = ui.available_width();
    let (row, resp) = ui.allocate_exact_size(vec2(avail, SIZE + 12.0), Sense::click());
    pick_mark(ui, row, look);
    if look.picking && resp.clicked() {
        return Some(Action::Pick(m.id));
    }
    let x = if m.out { row.right() - 10.0 - SIZE } else { row.left() + 10.0 + if look.picking { 30.0 } else { 0.0 } };
    let r = Rect::from_min_size(pos2(x, row.top() + 4.0), vec2(SIZE, SIZE));
    let p = ui.painter();
    match tex {
        Some(t) => glass::image_contain(p, r, t),
        None => {
            p.text(r.center(), Align2::CENTER_CENTER, emoji, FontId::proportional(84.0), ink());
        }
    }
    let meta = p.layout_no_wrap(m.time.clone(), FontId::proportional(11.0), ink());
    let chip = Rect::from_min_size(r.max - meta.size() - vec2(12.0, 8.0), meta.size() + vec2(12.0, 6.0));
    glass::glass(p, chip, Glass::tinted(chip.height() / 2.0, glass::chrome(), 120).frosted(1.0));
    p.galley(chip.min + vec2(6.0, 3.0), meta, ink());
    let mut action = None;
    if !m.reactions.is_empty() {
        action = reaction_chips(ui, m.id, &m.reactions, r.left(), m.out, r.right());
    }
    resp.context_menu(|ui| message_menu(ui, m, look.chat, &mut action));
    if resp.double_clicked() {
        action = Some(Action::Reply(m.id));
    }
    action
}

/// Reactions row under a message.
fn reaction_chips(ui: &mut Ui, id: i32, reactions: &[Reaction], left: f32, out: bool, right: f32) -> Option<Action> {
    let (row, _) = ui.allocate_exact_size(vec2(ui.available_width(), 32.0), Sense::hover());
    let chips: Vec<_> = reactions
        .iter()
        .map(|r| (r, ui.painter().layout_no_wrap(format!("{} {}", r.emoji, r.count), FontId::proportional(14.0), ink())))
        .collect();
    let total: f32 = chips.iter().map(|(_, g)| g.size().x + 22.0).sum();
    let mut x = if out { right - total } else { left };
    let mut action = None;
    for (r, g) in chips {
        let w = g.size().x + 18.0;
        let rect = Rect::from_min_size(pos2(x, row.top() + 2.0), vec2(w, 28.0));
        let resp = ui.interact(rect, ui.scope_id().with(("react", id, &r.emoji)), Sense::click());
        let alpha = if r.mine { 190 } else if resp.hovered() { 60 } else { 34 };
        glass::glass(ui.painter(), rect, Glass::tinted(14.0, if r.mine { ACCENT } else { Color32::WHITE }, alpha));
        ui.painter().galley(rect.center() - g.size() / 2.0, g, ink());
        if resp.clicked() {
            action = Some(Action::React(id, r.emoji.clone()));
        }
        x += w + 4.0;
    }
    action
}

/// Height of the poll / location / contact card (0 means no card).
fn card_height(ui: &Ui, m: &Msg, w: f32, chosen: &[Vec<u8>]) -> f32 {
    if m.media.as_ref().is_some_and(|i| matches!(i.kind, MediaKind::Voice | MediaKind::Audio)) {
        return MEDIA_H;
    }
    if let Some(poll) = &m.poll {
        let q = ui.painter().layout(poll.question.clone(), FontId::new(15.0, egui::FontFamily::Name(format::BOLD.into())), ink(), w);
        let button = poll.multiple && !poll.voted() && !poll.closed && !chosen.is_empty();
        return q.size().y + 22.0 + poll.answers.len() as f32 * 40.0 + 20.0 + if button { 40.0 } else { 0.0 };
    }
    if m.geo.is_some() {
        return 150.0;
    }
    if m.contact.is_some() {
        return 52.0;
    }
    0.0
}

fn draw_card(ui: &mut Ui, m: &Msg, rect: Rect, look: Look) -> Option<Action> {
    let chosen = look.chosen;
    let p = ui.painter().clone();
    let mut action = None;
    if m.media.as_ref().is_some_and(|i| matches!(i.kind, MediaKind::Voice | MediaKind::Audio)) {
        return media_card(ui, m, rect, look.play);
    }
    if let Some(poll) = &m.poll {
        let bold = FontId::new(15.0, egui::FontFamily::Name(format::BOLD.into()));
        let q = p.layout(poll.question.clone(), bold, ink(), rect.width());
        let mut y = rect.top();
        let qh = q.size().y;
        p.galley(pos2(rect.left(), y), q, ink());
        y += qh + 2.0;
        let kind = match (poll.quiz, poll.multiple) {
            (true, _) => l!("Вікторина", "Quiz"),
            (_, true) => l!("Кілька відповідей", "Multiple answers"),
            _ => l!("Опитування", "Poll"),
        };
        p.text(pos2(rect.left(), y), Align2::LEFT_TOP, kind, FontId::proportional(12.0), rgba(ink(), 160));
        y += 20.0;
        let show = poll.voted() || poll.closed;
        for a in &poll.answers {
            let r = Rect::from_min_size(pos2(rect.left(), y), vec2(rect.width(), 36.0));
            if show {
                let pct = if poll.total > 0 { a.voters as f32 / poll.total as f32 } else { 0.0 };
                p.text(pos2(r.left(), r.top() + 2.0), Align2::LEFT_TOP, format!("{:.0}%", pct * 100.0), FontId::proportional(13.0), ink());
                let label = format!("{}{}", a.text, if a.chosen { "  ✓" } else { "" });
                p.galley(pos2(r.left() + 44.0, r.top() + 2.0), elided(ui, &label, 14.0, ink(), r.width() - 44.0), ink());
                let bar = Rect::from_min_size(pos2(r.left() + 44.0, r.bottom() - 9.0), vec2((r.width() - 44.0) * pct.max(0.02), 4.0));
                let col = if poll.quiz && a.correct { Color32::from_rgb(70, 220, 120) } else if a.chosen { ink() } else { rgba(ink(), 140) };
                p.rect_filled(bar, 2.0, col);
            } else {
                let resp = ui.interact(r, ui.scope_id().with(("poll", m.id, &a.option)), Sense::click());
                if resp.hovered() {
                    p.rect_filled(r, 10.0, glass::veil(18));
                    ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
                }
                let c = pos2(r.left() + 12.0, r.center().y);
                let on = chosen.contains(&a.option);
                if on {
                    p.circle_filled(c, 8.0, ink());
                } else {
                    p.circle_stroke(c, 8.0, egui::Stroke::new(1.5, rgba(ink(), 180)));
                }
                p.galley(pos2(r.left() + 30.0, r.center().y - 9.0), elided(ui, &a.text, 14.5, ink(), r.width() - 34.0), ink());
                if resp.clicked() {
                    action = Some(if poll.multiple { Action::TogglePoll(m.id, a.option.clone()) } else { Action::Vote(m.id, vec![a.option.clone()]) });
                }
            }
            y += 40.0;
        }
        if poll.multiple && !show && !chosen.is_empty() {
            let b = Rect::from_min_size(pos2(rect.left(), y), vec2(rect.width(), 34.0));
            if glass::button(ui, b, ("vote", m.id), l!("Голосувати", "Vote"), true, true).clicked() {
                action = Some(Action::Vote(m.id, chosen.to_vec()));
            }
            y += 40.0;
        }
        let votes = match poll.total {
            0 => l!("Ще немає голосів", "No votes yet").to_owned(),
            n => format!("{}{}", lf!("Голосів: {n}", "Votes: {n}"), if poll.closed { l!(" · завершено", " · closed") } else { "" }),
        };
        p.text(pos2(rect.left(), y), Align2::LEFT_TOP, votes, FontId::proportional(12.0), rgba(ink(), 160));
    } else if let Some(g) = &m.geo {
        // Stylized "map": a street grid and a pin; a click opens OpenStreetMap.
        let map = Rect::from_min_size(rect.min, vec2(rect.width(), 100.0));
        let resp = ui.interact(rect, ui.scope_id().with(("geo", m.id)), Sense::click());
        p.rect_filled(map, 12.0, Color32::from_rgb(34, 44, 78));
        let clip = p.with_clip_rect(map.shrink(1.0));
        let stroke = egui::Stroke::new(1.0, glass::veil(30));
        let seed = (g.lat * 1000.0).fract().abs() as f32 * 40.0;
        for k in 0..8 {
            let t = k as f32 * 22.0 + seed;
            clip.line_segment([pos2(map.left() + t, map.top()), pos2(map.left() + t - 30.0, map.bottom())], stroke);
            clip.line_segment([pos2(map.left(), map.top() + t * 0.6), pos2(map.right(), map.top() + t * 0.6 + 14.0)], stroke);
        }
        let pin = Rect::from_center_size(map.center() - vec2(0.0, 6.0), vec2(30.0, 30.0));
        icons::draw(&p, pin, Icon::Location, Color32::from_rgb(255, 110, 130));
        let title = if !g.title.is_empty() { g.title.clone() } else if g.live { l!("Трансляція геопозиції", "Live location").into() } else { l!("Геопозиція", "Location").into() };
        p.galley(pos2(rect.left(), map.bottom() + 6.0), elided(ui, &title, 14.5, ink(), rect.width()), ink());
        let sub = if g.address.is_empty() { format!("{:.5}, {:.5}", g.lat, g.lon) } else { g.address.clone() };
        p.galley(pos2(rect.left(), map.bottom() + 26.0), elided(ui, &sub, 12.5, rgba(ink(), 170), rect.width()), ink());
        if resp.hovered() {
            ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
        }
        if resp.clicked() {
            action = Some(Action::OpenMap(g.lat, g.lon));
        }
    } else if let Some((name, phone)) = &m.contact {
        let resp = ui.interact(rect, ui.scope_id().with(("contact", m.id)), Sense::click());
        glass::avatar(&p, pos2(rect.left() + 24.0, rect.center().y), 22.0, name, name.len() as i64, None, None);
        p.galley(pos2(rect.left() + 56.0, rect.top() + 6.0), elided(ui, name, 15.0, ink(), rect.width() - 60.0), ink());
        p.galley(pos2(rect.left() + 56.0, rect.top() + 28.0), elided(ui, phone, 13.0, rgba(ink(), 180), rect.width() - 60.0), ink());
        if resp.hovered() {
            ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
        }
        if resp.clicked() {
            action = Some(Action::Copy2(phone.clone(), l!("Номер скопійовано", "Number copied")));
        }
    }
    action
}

const MEDIA_H: f32 = 48.0;
/// Player key for recording preview (real ids are positive).
const PREVIEW_ID: i32 = -1;

/// Waveform as bars; `progress` is the played fraction (brighter).
fn draw_wave(p: &egui::Painter, wave: Rect, levels: &[u8], progress: Option<f32>) {
    let n = ((wave.width() / 4.0) as usize).max(8);
    let level = |k: usize| match levels.len() {
        0 => 6.0,
        len => {
            let a = k * len / n;
            let b = ((k + 1) * len / n).max(a + 1).min(len);
            levels[a..b].iter().copied().max().unwrap_or(0) as f32
        }
    };
    for k in 0..n {
        let h = (2.0 + level(k) / 31.0 * wave.height()).min(wave.height());
        let x = wave.left() + k as f32 * 4.0 + 1.0;
        let done = progress.is_some_and(|pr| (k as f32 + 0.5) / n as f32 <= pr);
        let col = if done { ink() } else { rgba(ink(), 100) };
        p.rect_filled(Rect::from_min_max(pos2(x, wave.bottom() - h), pos2(x + 2.4, wave.bottom())), 1.2, col);
    }
}

/// Voice or audio (button, waveform or name, time) or video (button and duration).
fn media_card(ui: &mut Ui, m: &Msg, rect: Rect, play: PlayView) -> Option<Action> {
    let info = m.media.as_ref()?;
    let p = ui.painter().clone();
    let mut action = None;
    let dim = rgba(ink(), 170);

    let btn = Rect::from_min_size(rect.min + vec2(0.0, (rect.height() - 44.0) / 2.0), vec2(44.0, 44.0));
    let resp = ui.interact(btn, ui.scope_id().with(("play", m.id)), Sense::click());
    let tint = if m.out { Color32::WHITE } else { ACCENT };
    glass::glass(&p, btn, Glass::tinted(22.0, tint, if m.out { 70 } else { 180 }));
    if resp.hovered() {
        p.circle_filled(btn.center(), 22.0, glass::veil(20));
        ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
    }
    if play.loading {
        spinner(&p, btn.center(), 11.0);
    } else {
        let icon = if play.playing { Icon::Pause } else { Icon::Play };
        let off = if play.playing { 0.0 } else { 1.5 };
        icons::draw(&p, Rect::from_center_size(btn.center() + vec2(off, 0.0), vec2(18.0, 18.0)), icon, ink());
    }
    if resp.clicked() {
        action = Some(Action::Play(m.id));
    }

    let x0 = btn.right() + 10.0;
    let right = rect.right();
    let time = if play.current { format!("{} / {}", player::clock(play.position), player::clock(info.duration)) } else { player::clock(info.duration) };
    // Always keep room for the speed toggle so the waveform doesn't jump at start.
    let audio = matches!(info.kind, MediaKind::Voice | MediaKind::Audio);
    let chip_w = if audio { 38.0 } else { 0.0 };
    match info.kind {
        MediaKind::Voice => {
            // Waveform: bars, the played part brighter; a click seeks.
            let wave = Rect::from_min_max(pos2(x0, rect.top() + 4.0), pos2(right - chip_w, rect.top() + 26.0));
            draw_wave(&p, wave, &info.waveform, play.current.then_some(play.progress));
            action = seek_area(ui, m.id, wave, play).or(action);
        }
        _ => {
            let title = match info.kind {
                MediaKind::Audio => match (info.performer.as_str(), info.title.as_str()) {
                    ("", "") => l!("Аудіо", "Audio").to_owned(),
                    ("", t) => t.to_owned(),
                    (who, "") => who.to_owned(),
                    (who, t) => format!("{who} — {t}"),
                },
                _ => String::new(),
            };
            let bold = FontId::new(14.5, egui::FontFamily::Name(format::BOLD.into()));
            let mut job = LayoutJob::single_section(title, egui::TextFormat::simple(bold, ink()));
            job.wrap = TextWrapping::truncate_at_width((right - x0 - chip_w).max(10.0));
            p.galley(pos2(x0, rect.top() + 3.0), p.layout_job(job), ink());
            if info.kind == MediaKind::Audio && play.current {
                let bar = Rect::from_min_max(pos2(x0, rect.top() + 24.0), pos2(right - chip_w, rect.top() + 28.0));
                p.rect_filled(bar, 2.0, rgba(ink(), 70));
                p.rect_filled(Rect::from_min_size(bar.min, vec2(bar.width() * play.progress, bar.height())), 2.0, ink());
                action = seek_area(ui, m.id, bar.expand2(vec2(0.0, 6.0)), play).or(action);
            }
        }
    }
    let t = p.text(pos2(x0, rect.bottom() - 2.0), Align2::LEFT_BOTTOM, time, FontId::proportional(12.0), dim);
    if m.media_unread {
        // "Not listened yet" dot, as in Telegram.
        p.circle_filled(pos2(t.right() + 7.0, t.center().y + 0.5), 3.0, if m.out { ink() } else { ACCENT });
    }

    if audio && play.current {
        let chip = Rect::from_min_size(pos2(right - chip_w + 6.0, rect.top() + 6.0), vec2(chip_w - 6.0, 20.0));
        let r = ui.interact(chip, ui.scope_id().with(("speed", m.id)), Sense::click());
        p.rect_filled(chip, 10.0, glass::veil(if r.hovered() { 45 } else { 25 }));
        let s = if play.speed.fract() == 0.0 { format!("{}×", play.speed as i32) } else { format!("{}×", play.speed) };
        p.text(chip.center(), Align2::CENTER_CENTER, s, FontId::proportional(11.5), ink());
        if r.clicked() {
            action = Some(Action::Speed);
        }
    }
    action
}

/// Clickable seek bar for the current audio.
fn seek_area(ui: &mut Ui, id: i32, r: Rect, play: PlayView) -> Option<Action> {
    if !play.current {
        return None;
    }
    let resp = ui.interact(r, ui.scope_id().with(("seek", id)), Sense::click_and_drag());
    if resp.hovered() {
        ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
    }
    // While dragging, seek only on release, so the sound isn't restarted every frame.
    let pos = resp.interact_pointer_pos()?;
    (resp.clicked() || resp.drag_stopped()).then(|| Action::Seek(id, (pos.x - r.left()) / r.width()))
}

/// Open a file in the default application.
fn open_external(path: &std::path::Path) {
    crate::platform::open(path);
}

/// Album layout: how many photos in each row.
fn album_rows(n: usize) -> Vec<usize> {
    match n {
        0..=2 => vec![n],
        3 => vec![1, 2],
        4 => vec![2, 2],
        5 => vec![2, 3],
        6 => vec![3, 3],
        7 => vec![2, 2, 3],
        8 => vec![2, 3, 3],
        9 => vec![3, 3, 3],
        _ => vec![2, 2, 3, 3],
    }
}

/// Album: a grid of photos in one bubble; the caption comes from the message that has one.
fn album_bubble(ui: &mut Ui, msgs: &[&Msg], texs: &[Option<&TextureHandle>], look: Look) -> Option<Action> {
    let first = msgs[0];
    let ids: Vec<i32> = msgs.iter().map(|m| m.id).collect();
    let avail = ui.available_width();
    let w = (avail * 0.72).min(360.0);
    let gap = 3.0;
    let rows = album_rows(msgs.len());
    let heights: Vec<f32> = rows.iter().map(|&c| if c == 1 { w * 0.7 } else { (w / c as f32).min(200.0) }).collect();
    let grid_h: f32 = heights.iter().sum::<f32>() + gap * (rows.len() as f32 - 1.0);
    let cap = msgs.iter().find(|m| !m.text.is_empty()).copied();
    let p = ui.painter().clone();
    let body = cap.map(|c| p.layout_job(format::layout(&c.text, &c.spans, glass::msg_font(), ink(), glass::link(false), w - 20.0, false)));
    let last = msgs[msgs.len() - 1];
    let meta = p.layout_no_wrap(last.time.clone(), FontId::proportional(11.0), rgba(ink(), 170));
    let text_h = body.as_ref().map_or(0.0, |b| b.size().y + meta.size().y + 22.0);
    let size = vec2(w + 8.0, grid_h + 8.0 + text_h);
    let (row, resp) = ui.allocate_exact_size(vec2(avail, size.y + 6.0), Sense::click());
    pick_mark(ui, row, look);
    let x = if first.out { row.right() - 10.0 - size.x } else { row.left() + 10.0 + if look.picking { 30.0 } else { 0.0 } };
    let rect = Rect::from_min_size(pos2(x, row.top() + 3.0), size);
    let g = if first.out { Glass::tinted(18.0, ACCENT, glass::bubble_out_alpha()) } else { Glass::tinted(18.0, glass::bubble_in().0, glass::bubble_in().1) };
    glass::glass(&p, rect, g.frosted(1.0));

    let mut y = rect.top() + 4.0;
    let mut k = 0;
    let mut hit = None;
    for (&cols, &h) in rows.iter().zip(&heights) {
        let cw = (w - gap * (cols as f32 - 1.0)) / cols as f32;
        for c in 0..cols {
            let r = Rect::from_min_size(pos2(rect.left() + 4.0 + c as f32 * (cw + gap), y), vec2(cw, h));
            match texs[k] {
                Some(t) => glass::image_rounded(&p, r, 8.0, t.id(), t.size_vec2()),
                None => {
                    p.rect_filled(r, 8.0, glass::veil(14));
                    icons::draw(&p, Rect::from_center_size(r.center(), vec2(26.0, 26.0)), Icon::Camera, ink_dim());
                }
            }
            if resp.hover_pos().is_some_and(|q| r.contains(q)) {
                hit = Some(msgs[k].id);
                ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
            }
            k += 1;
        }
        y += h + gap;
    }
    if let Some(b) = body {
        p.galley(pos2(rect.left() + 14.0, y + 6.0), b, ink());
        p.galley(rect.max - vec2(14.0, 9.0) - meta.size(), meta, ink());
    } else {
        let chip = Rect::from_min_size(rect.max - meta.size() - vec2(22.0, 16.0), meta.size() + vec2(12.0, 6.0));
        p.rect_filled(chip, chip.height() / 2.0, Color32::from_black_alpha(120));
        // The chip over the photo is dark in both themes, so its text is always light.
        let meta = p.layout_no_wrap(last.time.clone(), FontId::proportional(11.0), glass::ON_DARK);
        p.galley(chip.min + vec2(6.0, 3.0), meta, glass::ON_DARK);
    }
    let main = cap.unwrap_or(first);
    let album_protected = msgs.iter().any(|m| m.noforwards);
    let mut action = None;
    if !main.reactions.is_empty() {
        action = reaction_chips(ui, main.id, &main.reactions, rect.left(), first.out, rect.right());
    }
    if look.picking {
        return resp.clicked().then(|| Action::PickMany(ids.clone())).or(action);
    }
    resp.context_menu(|ui| {
        ui.set_min_width(170.0);
        ui.horizontal(|ui| {
            for e in emoji::QUICK_REACTIONS.iter().take(6) {
                if ui.add(egui::Button::new(egui::RichText::new(*e).size(18.0)).frame(false)).clicked() {
                    action = Some(Action::React(main.id, (*e).to_owned()));
                }
            }
        });
        ui.separator();
        if glass::menu_button(ui, Icon::Reply, l!("Відповісти", "Reply")).clicked() {
            action = Some(Action::Reply(main.id));
        }
        if !look.chat.is_some_and(|c| c.noforwards) && !album_protected && glass::menu_button(ui, Icon::Forward, l!("Переслати альбом", "Forward album")).clicked() {
            action = Some(Action::ForwardMany(ids.clone()));
        }
        if glass::menu_button(ui, Icon::Select, l!("Вибрати", "Select")).clicked() {
            action = Some(Action::PickMany(ids.clone()));
        }
        if first.out && glass::menu_button(ui, Icon::Trash, l!("Видалити альбом", "Delete album")).clicked() {
            action = Some(Action::DeleteMany(ids.clone()));
        }
    });
    if resp.clicked()
        && let Some(id) = hit
    {
        action = Some(Action::View(id));
    }
    if resp.double_clicked() {
        action = Some(Action::Reply(main.id));
    }
    action
}

/// Room for ticks in the time label (≈14 pt at font size 11).
const TICKS_GAP: &str = "     ";

/// Queued message instead of ticks: a clock while sending, a red "!" mark after a failure.
fn draw_pending(p: &egui::Painter, right: egui::Pos2, failed: bool, color: Color32) {
    let c = right + vec2(-7.0, -0.5);
    if failed {
        p.circle_filled(c, 6.0, glass::DANGER);
        let s = egui::Stroke::new(1.6, glass::ON_DARK);
        p.line_segment([c + vec2(0.0, -3.2), c + vec2(0.0, 0.8)], s);
        p.circle_filled(c + vec2(0.0, 3.0), 0.9, glass::ON_DARK);
    } else {
        icons::draw(p, Rect::from_center_size(c, vec2(13.0, 13.0)), Icon::Clock, color);
    }
}

/// Menu of a queued bubble: copy, and retry / delete instead of server actions.
fn pending_menu(ui: &mut Ui, m: &Msg, failed: bool, action: &mut Option<Action>) {
    ui.set_min_width(190.0);
    if failed {
        ui.label(egui::RichText::new(l!("Не надіслано", "Not sent")).color(glass::DANGER).size(12.5));
        if glass::menu_button(ui, Icon::Reload, l!("Повторити", "Retry")).clicked() {
            *action = Some(Action::Retry(m.id));
        }
    } else {
        ui.label(egui::RichText::new(l!("Надсилається…", "Sending…")).color(ink_dim()).size(12.5));
    }
    if glass::menu_button(ui, Icon::Copy, l!("Копіювати", "Copy")).clicked() {
        *action = Some(Action::Copy(m.text.clone()));
    }
    if glass::menu_button(ui, Icon::Trash, l!("Видалити", "Delete")).clicked() {
        *action = Some(Action::Discard(m.id));
    }
}

fn bubble(
    ui: &mut Ui,
    m: &Msg,
    look: Look,
    reply: Option<(String, String)>,
    photo: Option<&TextureHandle>,
    sticker: Option<&TextureHandle>,
) -> Option<Action> {
    const REPLY_H: f32 = 40.0;
    const FILE_H: f32 = 46.0;
    let Look { group: show_sender, revealed, chosen, .. } = look;
    if let Some((_, emoji)) = &m.sticker {
        return sticker_bubble(ui, m, emoji, sticker, look);
    }
    let kind = m.media.as_ref().map(|i| i.kind);
    if kind == Some(MediaKind::Round) {
        return round_bubble(ui, m, photo, look);
    }
    // Video and GIF are drawn like photos: a frame or a thumbnail.
    let video = matches!(kind, Some(MediaKind::Video | MediaKind::Gif));
    let avail = ui.available_width();
    let max_w = (avail * 0.72).min(560.0);
    let pad = vec2(14.0, 9.0);
    let p = ui.painter().clone();

    let img_size = (m.photo || video).then(|| {
        let w = max_w.min(VIDEO_PT);
        let known = m.media.as_ref().map(|i| i.size).filter(|s| s.0 > 0 && s.1 > 0).map(|s| vec2(s.0 as f32, s.1 as f32));
        match known.or(photo.map(|t| t.size_vec2())) {
            Some(s) => vec2(w, (w * s.y / s.x).clamp(110.0, 420.0)),
            None => vec2(w, 220.0),
        }
    });
    let text_w = img_size.map_or(max_w, |s| s.x) - pad.x * 2.0;

    let link = glass::link(m.out);
    let body = (!m.text.is_empty()).then(|| p.layout_job(format::layout(&m.text, &m.spans, glass::msg_font(), ink(), link, text_w, revealed)));
    // Trailing spaces leave room for the ticks drawn on top.
    let meta = format!("{}{}{}{}", chatview::views_label(m, look), if m.edited { l!("ред. ", "edited ") } else { "" }, m.time, if look.ticks.is_some() || look.pending.is_some() { TICKS_GAP } else { "" });
    let meta = p.layout_no_wrap(meta, FontId::proportional(11.0), rgba(ink(), 170));
    let name_color = glass::name_color(m.sender.len() as i64 * 31 + m.sender.chars().next().map_or(0, |c| c as i64));
    let sender = (show_sender && !m.out && !m.sender.is_empty())
        .then(|| elided(ui, &m.sender, 13.0, name_color, text_w));
    let (quote_accent, quote_text) = if m.out { (ink(), rgba(ink(), 200)) } else { (ACCENT, ink_dim()) };
    let reply = reply.map(|(who, what)| (elided(ui, &who, 12.5, quote_accent, text_w - 12.0), elided(ui, &what, 13.0, quote_text, text_w - 12.0)));
    let fwd = m.forwarded.as_ref().map(|from| elided(ui, &lf!("Переслано від {from}", "Forwarded from {from}"), 12.5, quote_accent, text_w));

    // Text block under the photo (or the whole bubble if there is no photo).
    let mut block = vec2(0.0, 0.0);
    if let Some(f) = &fwd {
        block.x = block.x.max(f.size().x);
        block.y += f.size().y + 4.0;
    }
    if let Some(s) = &sender {
        block.x = block.x.max(s.size().x);
        block.y += s.size().y + 2.0;
    }
    if let Some((a, b)) = &reply {
        block.x = block.x.max(a.size().x.max(b.size().x) + 12.0).max(140.0);
        block.y += REPLY_H + 4.0;
    }
    let file = m.file.as_ref().map(|(name, size)| {
        let n = elided(ui, name, 14.5, ink(), text_w - 56.0);
        let s = p.layout_no_wrap(size.clone(), FontId::proportional(12.0), rgba(ink(), 170));
        (n, s)
    });
    if let Some((n, sz)) = &file {
        block.x = block.x.max(n.size().x.max(sz.size().x) + 56.0);
        block.y += FILE_H + 4.0;
    }
    let card_w = text_w.min(320.0);
    let card_h = card_height(ui, m, card_w, chosen);
    if card_h > 0.0 {
        block.x = block.x.max(card_w);
        block.y += card_h + 6.0;
    }
    // The link preview card goes under the text, like in Telegram.
    let web = m.webpage.as_ref().filter(|_| img_size.is_none()).map(|w| suggest::WebCard::layout(ui, w, text_w.min(380.0), photo, quote_accent));
    if let Some(c) = &web {
        block.x = block.x.max(c.width);
        block.y += c.height() + 6.0;
    }
    match &body {
        Some(body) => {
            let last_row_w = body.rows.last().map_or(0.0, |r| r.rect().width());
            let meta_inline = last_row_w + meta.size().x + 10.0 <= text_w && web.is_none();
            block.x = block.x.max(body.size().x);
            block.y += body.size().y;
            if meta_inline {
                block.x = block.x.max(last_row_w + meta.size().x + 10.0);
            } else {
                block.x = block.x.max(meta.size().x);
                block.y += meta.size().y + 2.0;
            }
        }
        // Time goes in the bottom right corner of the media card, next to the duration.
        None if m.media.is_some() => {
            block.x = block.x.max(card_w.min(150.0) + meta.size().x);
            block.y -= 6.0;
        }
        None if img_size.is_none() => {
            block.x = block.x.max(meta.size().x);
            block.y += meta.size().y;
        }
        None => {}
    }
    let has_block = block.y > 0.0;
    let size = match img_size {
        Some(is) => vec2(is.x + 8.0, is.y + 8.0 + if has_block { block.y + pad.y * 2.0 - 4.0 } else { 0.0 }),
        None => block + pad * 2.0,
    };

    let (row, resp) = ui.allocate_exact_size(vec2(avail, size.y + 6.0), Sense::click());
    pick_mark(ui, row, look);
    let x = if m.out { row.right() - 10.0 - size.x } else { row.left() + 10.0 + if look.picking { 30.0 } else { 0.0 } };
    let rect = Rect::from_min_size(pos2(x, row.top() + 3.0), size);
    let g = if m.out {
        Glass::tinted(18.0, ACCENT, glass::bubble_out_alpha()).frosted(1.0)
    } else {
        Glass::tinted(18.0, glass::bubble_in().0, glass::bubble_in().1).frosted(1.0)
    };
    glass::glass(&p, rect, g);

    let mut cursor = rect.min + pad;
    if let Some(is) = img_size {
        let ir = Rect::from_min_size(rect.min + vec2(4.0, 4.0), is);
        match photo {
            Some(t) => glass::image_rounded(&p, ir, 15.0, t.id(), t.size_vec2()),
            None => {
                p.rect_filled(ir, 15.0, glass::veil(14));
                icons::draw(&p, Rect::from_center_size(ir.center(), vec2(30.0, 30.0)), Icon::Camera, ink_dim());
            }
        }
        if let Some(info) = m.media.as_ref().filter(|_| video) {
            video_overlay(&p, ir, info, look.play);
        }
        if !has_block {
            let chip = Rect::from_min_size(ir.max - meta.size() - vec2(18.0, 12.0), meta.size() + vec2(12.0, 6.0));
            p.rect_filled(chip, chip.height() / 2.0, Color32::from_black_alpha(120));
            let on_media = p.layout_no_wrap(meta.text().to_owned(), FontId::proportional(11.0), glass::ON_DARK);
            let at = Rect::from_min_size(chip.min + vec2(6.0, 3.0), on_media.size());
            p.galley(at.min, on_media, glass::ON_DARK);
            chatview::meta_icons(&p, at, m, look, glass::ON_DARK);
        }
        cursor.y = ir.bottom() + pad.y;
    }
    if let Some(f) = fwd {
        let h = f.size().y;
        p.galley(cursor, f, quote_accent);
        cursor.y += h + 4.0;
    }
    if let Some(s) = sender {
        let h = s.size().y;
        p.galley(cursor, s, ink());
        cursor.y += h + 2.0;
    }
    if let Some((a, b)) = reply {
        let r = Rect::from_min_size(cursor, vec2(block.x, REPLY_H));
        p.rect_filled(r, 8.0, glass::veil(16));
        p.rect_filled(Rect::from_min_size(r.min, vec2(3.0, REPLY_H)), 2.0, quote_accent);
        p.galley(r.min + vec2(10.0, 4.0), a, quote_accent);
        p.galley(r.min + vec2(10.0, 21.0), b, quote_text);
        cursor.y += REPLY_H + 4.0;
    }
    let mut file_rect = None;
    let mut body_pos = None;
    if let Some((n, sz)) = file {
        let r = Rect::from_min_size(cursor, vec2(block.x, FILE_H));
        let icon = Rect::from_min_size(r.min + vec2(0.0, 3.0), vec2(40.0, 40.0));
        glass::glass(&p, icon, Glass::tinted(20.0, if m.out { Color32::WHITE } else { ACCENT }, if m.out { 60 } else { 170 }));
        icons::draw(&p, Rect::from_center_size(icon.center(), vec2(19.0, 19.0)), Icon::File, ink());
        p.galley(r.min + vec2(52.0, 4.0), n, ink());
        p.galley(r.min + vec2(52.0, 25.0), sz, ink());
        file_rect = Some(r);
        cursor.y += FILE_H + 4.0;
    }
    let mut card_action = None;
    if card_h > 0.0 {
        card_action = draw_card(ui, m, Rect::from_min_size(cursor, vec2(block.x, card_h)), look);
        cursor.y += card_h + 6.0;
    }
    if let Some(body) = &body {
        p.galley(cursor, body.clone(), ink());
        body_pos = Some(cursor);
    }
    if let Some(c) = web {
        let top = cursor.y + body.as_ref().map_or(0.0, |b| b.size().y + 6.0);
        card_action = c.draw(ui, m, pos2(cursor.x, top), photo, quote_accent).or(card_action);
    }
    if has_block || img_size.is_none() {
        let at = Rect::from_min_size(rect.max - pad - meta.size(), meta.size());
        p.galley(at.min, meta, ink());
        if let Some(failed) = look.pending {
            draw_pending(&p, at.right_center(), failed, rgba(ink(), 200));
        } else {
            chatview::meta_icons(&p, at, m, look, rgba(ink(), 200));
        }
    }

    let mut action = card_action;
    if !m.reactions.is_empty() {
        action = reaction_chips(ui, m.id, &m.reactions, rect.left(), m.out, rect.right()).or(action);
    }
    if !m.buttons.is_empty() {
        action = bot_buttons(ui, m, rect.left(), size.x.max(240.0).min(avail - 20.0)).or(action);
    }
    if look.picking {
        return resp.clicked().then_some(Action::Pick(m.id)).or(action);
    }
    if let Some(failed) = look.pending {
        resp.context_menu(|ui| pending_menu(ui, m, failed, &mut action));
        // A failed bubble retries on click, like the red mark in Telegram Desktop.
        if failed && resp.clicked() {
            action = Some(Action::Retry(m.id));
        }
        return action;
    }
    resp.context_menu(|ui| message_menu(ui, m, look.chat, &mut action));
    // Links and spoilers in the text.
    if let (Some(body), Some(origin), Some(pos)) = (&body, body_pos, resp.hover_pos())
        && Rect::from_min_size(origin, body.size()).contains(pos)
    {
        let ch = body.cursor_from_pos(pos - origin).index.0;
        match format::span_at(&m.spans, format::char_to_byte(&m.text, ch)) {
            Some(span) if span.style == Style::Spoiler && !revealed => {
                ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
                if resp.clicked() {
                    return Some(Action::RevealSpoiler(m.id));
                }
            }
            Some(span) if span.style == Style::Link && span.target.is_some() => {
                ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
                if resp.clicked() {
                    return span.target.clone().map(Action::Open);
                }
            }
            _ => {}
        }
    }
    let media_rect = file_rect.or(img_size.map(|is| Rect::from_min_size(rect.min + vec2(4.0, 4.0), is)));
    if let Some(r) = media_rect {
        let hovered = resp.hover_pos().is_some_and(|p| r.contains(p));
        if hovered {
            ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
        }
        if resp.clicked() && hovered {
            action = match kind {
                _ if m.photo => Some(Action::View(m.id)),
                Some(MediaKind::Video) => Some(Action::Play(m.id)),
                Some(MediaKind::Gif) => None,
                _ => Some(Action::Download(m.id)),
            };
        }
    }
    if resp.double_clicked() {
        action = Some(Action::Reply(m.id));
    }
    action
}

/// Duration and play button over the video frame.
fn video_overlay(p: &egui::Painter, r: Rect, info: &backend::MediaInfo, play: PlayView) {
    let label = match info.kind {
        MediaKind::Gif => "GIF".to_owned(),
        _ if play.current => player::clock(info.duration - play.position),
        _ => player::clock(info.duration),
    };
    let g = p.layout_no_wrap(label, FontId::proportional(11.5), glass::ON_DARK);
    let chip = Rect::from_min_size(r.min + vec2(8.0, 8.0), g.size() + vec2(12.0, 6.0));
    p.rect_filled(chip, chip.height() / 2.0, Color32::from_black_alpha(120));
    p.galley(chip.min + vec2(6.0, 3.0), g, glass::ON_DARK);
    if info.kind == MediaKind::Video && !play.playing {
        let c = Rect::from_center_size(r.center(), vec2(54.0, 54.0));
        glass::glass(p, c, Glass::tinted(27.0, glass::chrome(), 140).frosted(1.0));
        if play.loading {
            spinner(p, c.center(), 13.0);
        } else {
            icons::draw(p, Rect::from_center_size(c.center() + vec2(2.0, 0.0), vec2(22.0, 22.0)), Icon::Play, ink());
        }
    }
    if play.current && info.kind == MediaKind::Video {
        let bar = Rect::from_min_max(pos2(r.left() + 10.0, r.bottom() - 6.0), pos2(r.right() - 10.0, r.bottom() - 3.0));
        p.rect_filled(bar, 1.5, glass::veil(60));
        p.rect_filled(Rect::from_min_size(bar.min, vec2(bar.width() * play.progress, bar.height())), 1.5, ink());
    }
}

/// A spinning ring while something loads.
fn spinner(p: &egui::Painter, c: egui::Pos2, r: f32) {
    let t = p.ctx().input(|i| i.time) as f32 * 5.0;
    let pts: Vec<_> = (0..=24).map(|k| t + k as f32 * 0.18).map(|a| c + vec2(a.cos(), a.sin()) * r).collect();
    p.add(egui::Shape::line(pts, egui::Stroke::new(2.2, ink())));
}

/// Round video message without a bubble: frame in a circle, glass rim, progress around the circle.
fn round_bubble(ui: &mut Ui, m: &Msg, tex: Option<&TextureHandle>, look: Look) -> Option<Action> {
    let size = ROUND_PT;
    let avail = ui.available_width();
    let (row, resp) = ui.allocate_exact_size(vec2(avail, size + 12.0), Sense::click());
    pick_mark(ui, row, look);
    if look.picking && resp.clicked() {
        return Some(Action::Pick(m.id));
    }
    let x = if m.out { row.right() - 10.0 - size } else { row.left() + 10.0 + if look.picking { 30.0 } else { 0.0 } };
    let r = Rect::from_min_size(pos2(x, row.top() + 6.0), vec2(size, size));
    let p = ui.painter();
    glass::glass(p, r.expand(4.0), Glass::tinted(size / 2.0 + 4.0, glass::bubble_in().0, glass::bubble_in().1.max(40)).frosted(1.0).with_shadow());
    match tex {
        Some(t) => glass::image_rounded(p, r, size / 2.0, t.id(), t.size_vec2()),
        None => {
            p.circle_filled(r.center(), size / 2.0, glass::veil(14));
            icons::draw(p, Rect::from_center_size(r.center(), vec2(34.0, 34.0)), Icon::Camera, ink_dim());
        }
    }
    let play = look.play;
    if play.loading {
        spinner(p, r.center(), 16.0);
    }
    if play.current {
        let a0 = -std::f32::consts::FRAC_PI_2;
        let n = (play.progress * 96.0).ceil() as usize;
        let pts: Vec<_> = (0..=n).map(|k| a0 + k as f32 / 96.0 * std::f32::consts::TAU).map(|a| r.center() + vec2(a.cos(), a.sin()) * (size / 2.0 + 1.0)).collect();
        if pts.len() > 1 {
            p.add(egui::Shape::line(pts, egui::Stroke::new(3.0, ink())));
        }
    } else if !play.loading {
        // Muted: a speaker-off icon at the bottom.
        let c = Rect::from_center_size(pos2(r.center().x, r.bottom() - 22.0), vec2(26.0, 26.0));
        p.circle_filled(c.center(), 13.0, Color32::from_black_alpha(170));
        icons::draw(p, c.shrink(5.0), Icon::Muted, glass::ON_DARK);
    }
    let info = m.media.as_ref()?;
    let left = if play.current { info.duration - play.position } else { info.duration };
    let g = p.layout_no_wrap(player::clock(left), FontId::proportional(11.5), ink());
    let dot = if m.media_unread { 10.0 } else { 0.0 };
    let chip = Rect::from_min_size(pos2(r.left() + 4.0, r.bottom() - g.size().y - 6.0), g.size() + vec2(12.0 + dot, 6.0));
    glass::glass(p, chip, Glass::tinted(chip.height() / 2.0, glass::chrome(), 120).frosted(1.0));
    p.galley(chip.min + vec2(6.0, 3.0), g, ink());
    if m.media_unread {
        p.circle_filled(pos2(chip.right() - 9.0, chip.center().y), 3.0, ACCENT);
    }
    let meta = p.layout_no_wrap(m.time.clone(), FontId::proportional(11.0), ink());
    let mchip = Rect::from_min_size(pos2(r.right() - meta.size().x - 16.0, r.bottom() - meta.size().y - 6.0), meta.size() + vec2(12.0, 6.0));
    glass::glass(p, mchip, Glass::tinted(mchip.height() / 2.0, glass::chrome(), 120).frosted(1.0));
    p.galley(mchip.min + vec2(6.0, 3.0), meta, ink());

    let mut action = None;
    if !m.reactions.is_empty() {
        action = reaction_chips(ui, m.id, &m.reactions, r.left(), m.out, r.right());
    }
    let inside = resp.hover_pos().is_some_and(|q| q.distance(r.center()) <= size / 2.0);
    if inside {
        ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
    }
    resp.context_menu(|ui| message_menu(ui, m, look.chat, &mut action));
    if resp.double_clicked() {
        return Some(Action::Reply(m.id));
    }
    if resp.clicked() && inside {
        action = Some(Action::Play(m.id));
    }
    action
}

fn upload_pill(ui: &Ui, u: &Upload, bottom_left: egui::Pos2) -> f32 {
    let pill = Rect::from_min_size(bottom_left - vec2(0.0, 40.0), vec2(280.0, 40.0));
    glass::glass(ui.painter(), pill, Glass::tinted(20.0, glass::chrome(), 150).frosted(1.0).with_shadow());
    let p = ui.painter();
    let frac = if u.total > 0 { (u.done as f32 / u.total as f32).clamp(0.0, 1.0) } else { 0.0 };
    // Circular progress ring.
    let c = pos2(pill.left() + 21.0, pill.center().y);
    p.circle_stroke(c, 10.0, egui::Stroke::new(2.5, glass::veil(40)));
    if frac > 0.0 {
        let n = (frac * 48.0).ceil() as usize + 1;
        let pts = (0..n)
            .map(|i| {
                let a = -std::f32::consts::FRAC_PI_2 + std::f32::consts::TAU * frac * i as f32 / (n - 1) as f32;
                c + 10.0 * vec2(a.cos(), a.sin())
            })
            .collect();
        p.add(egui::Shape::line(pts, egui::Stroke::new(2.5, ACCENT)));
    }
    let pct = format!("{}%", (frac * 100.0).round() as u32);
    let pg = p.layout_no_wrap(pct, FontId::proportional(12.5), ink_dim());
    let pw = pg.size().x;
    p.galley(pos2(pill.right() - 14.0 - pw, pill.center().y - pg.size().y / 2.0), pg, ink_dim());
    let name = elided(ui, &u.name, 13.5, ink(), pill.width() - 60.0 - pw);
    let nh = name.size().y;
    p.galley(pos2(pill.left() + 40.0, pill.center().y - nh / 2.0), name, ink());
    pill.top()
}

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
