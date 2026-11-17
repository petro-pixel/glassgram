//! Full account data export, as Telegram Desktop's "Export Telegram data": a takeout session
//! (`account.initTakeoutSession` with the chosen kinds of data), every request wrapped in
//! `invokeWithTakeout`, then `account.finishTakeoutSession`. Chats go through the same HTML/JSON
//! writers as the single-chat export (`export.rs`); media through the client's downloader.

use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};

use grammers_client::InvocationError;
use grammers_client::tl::RemoteCall;

use super::*;
use crate::export::{self, Entry, Format};
use crate::i18n::lp;

/// Messages per history request, the server's maximum.
const PAGE_SIZE: i32 = 100;

/// What to export, as the checkboxes of Telegram Desktop's export box.
#[derive(Debug, Clone, PartialEq)]
pub struct TakeoutOptions {
    pub info: bool,
    pub contacts: bool,
    pub sessions: bool,
    pub personal: bool,
    pub bots: bool,
    pub private_groups: bool,
    pub private_channels: bool,
    pub public_groups: bool,
    pub public_channels: bool,
    /// In public groups, only the messages we sent.
    pub only_my: bool,
    pub photos: bool,
    pub videos: bool,
    /// Voice and round video messages.
    pub voice: bool,
    pub files: bool,
    pub stickers: bool,
    /// Size limit for everything but photos, in MB.
    pub max_mb: u32,
    pub format: Format,
    /// The folder the dated export folder goes into.
    pub dir: PathBuf,
}

impl Default for TakeoutOptions {
    fn default() -> Self {
        // Telegram Desktop's defaults: account, contacts, personal chats and photos, HTML.
        Self {
            info: true,
            contacts: true,
            sessions: false,
            personal: true,
            bots: false,
            private_groups: false,
            private_channels: false,
            public_groups: false,
            public_channels: false,
            only_my: true,
            photos: true,
            videos: false,
            voice: false,
            files: false,
            stickers: false,
            max_mb: 8,
            format: Format::Html,
            dir: crate::platform::download_dir(),
        }
    }
}

impl TakeoutOptions {
    pub fn any_media(&self) -> bool {
        self.photos || self.videos || self.voice || self.files || self.stickers
    }

    pub fn any_chats(&self) -> bool {
        self.personal || self.bots || self.private_groups || self.private_channels || self.public_groups || self.public_channels
    }

    /// Whether a chat of this kind is exported.
    fn wants(&self, kind: Category) -> bool {
        match kind {
            Category::Personal => self.personal,
            Category::Bot => self.bots,
            Category::PrivateGroup => self.private_groups,
            Category::PrivateChannel => self.private_channels,
            Category::PublicGroup => self.public_groups,
            Category::PublicChannel => self.public_channels,
        }
    }

    /// Whether a file of this kind and size is saved.
    fn wants_file(&self, kind: FileKind, size: u64) -> bool {
        let fits = size <= self.max_mb as u64 * 1_048_576;
        match kind {
            FileKind::Photo => self.photos,
            FileKind::Video => self.videos && fits,
            FileKind::Voice => self.voice && fits,
            FileKind::Sticker => self.stickers && fits,
            FileKind::File => self.files && fits,
        }
    }
}

/// How far the export got.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct TakeoutProgress {
    /// What is being exported now (a section or a chat name).
    pub step: String,
    pub chats_done: usize,
    pub chats_total: usize,
    pub messages: usize,
    pub files: usize,
    pub bytes: u64,
}

pub enum TakeoutEv {
    /// Telegram asks to wait before the first export (`TAKEOUT_INIT_DELAY_X`): it may start at
    /// `until` (unix seconds).
    Delay { until: i64 },
    Progress(TakeoutProgress),
    Done { dir: PathBuf, progress: TakeoutProgress },
    Cancelled,
    Failed(String),
}

#[derive(Debug, Clone, Copy, PartialEq)]
enum Category {
    Personal,
    Bot,
    PrivateGroup,
    PrivateChannel,
    PublicGroup,
    PublicChannel,
}

#[derive(Debug, Clone, Copy, PartialEq)]
enum FileKind {
    Photo,
    Video,
    Voice,
    Sticker,
    File,
}

/// What a message attachment is, for the media checkboxes.
fn file_kind(media: &tl::enums::MessageMedia) -> Option<(FileKind, u64)> {
    match media {
        tl::enums::MessageMedia::Photo(p) => {
            let size = match &p.photo {
                Some(tl::enums::Photo::Photo(ph)) => ph.sizes.iter().map(photo_size).max().unwrap_or(0),
                _ => return None,
            };
            Some((FileKind::Photo, size))
        }
        tl::enums::MessageMedia::Document(d) => {
            let Some(tl::enums::Document::Document(doc)) = &d.document else { return None };
            let mut kind = FileKind::File;
            for a in &doc.attributes {
                match a {
                    tl::enums::DocumentAttribute::Sticker(_) => kind = FileKind::Sticker,
                    tl::enums::DocumentAttribute::Video(v) if v.round_message => kind = FileKind::Voice,
                    tl::enums::DocumentAttribute::Video(_) if kind == FileKind::File => kind = FileKind::Video,
                    tl::enums::DocumentAttribute::Audio(a) if a.voice => kind = FileKind::Voice,
                    _ => {}
                }
            }
            Some((kind, doc.size.max(0) as u64))
        }
        _ => None,
    }
}

fn photo_size(s: &tl::enums::PhotoSize) -> u64 {
    match s {
        tl::enums::PhotoSize::Size(s) => s.size.max(0) as u64,
        tl::enums::PhotoSize::Progressive(s) => s.sizes.iter().copied().max().unwrap_or(0).max(0) as u64,
        _ => 0,
    }
}

/// The label written instead of a file that was not saved.
fn media_label(media: &tl::enums::MessageMedia) -> &'static str {
    match file_kind(media).map(|k| k.0) {
        Some(FileKind::Photo) => l!("Фото", "Photo"),
        Some(FileKind::Video) => l!("Відео", "Video"),
        Some(FileKind::Voice) => l!("Голосове повідомлення", "Voice message"),
        Some(FileKind::Sticker) => l!("Стікер", "Sticker"),
        Some(FileKind::File) => l!("Файл", "File"),
        None => l!("Вкладення", "Attachment"),
    }
}

/// A chat to export.
struct Chat {
    title: String,
    input: tl::enums::InputPeer,
    kind: Category,
}

/// Key of a peer in the name map: (0 user, 1 chat, 2 channel, id).
fn peer_key(p: &tl::enums::Peer) -> (u8, i64) {
    match p {
        tl::enums::Peer::User(u) => (0, u.user_id),
        tl::enums::Peer::Chat(c) => (1, c.chat_id),
        tl::enums::Peer::Channel(c) => (2, c.channel_id),
    }
}

fn user_name(u: &tl::types::User) -> String {
    let name = format!("{} {}", u.first_name.as_deref().unwrap_or_default(), u.last_name.as_deref().unwrap_or_default());
    let name = name.trim();
    if name.is_empty() { l!("Видалений акаунт", "Deleted Account").to_owned() } else { name.to_owned() }
}

/// One running export.
struct Job {
    client: Client,
    id: i64,
    out: Emitter,
    o: TakeoutOptions,
    cancel: Arc<AtomicBool>,
    dir: PathBuf,
    p: TakeoutProgress,
    me: i64,
    my_name: String,
    names: HashMap<(u8, i64), String>,
    /// Sections for the front page: (title, path without extension, note).
    index: Vec<(String, String, String)>,
}

/// Why the export stopped early.
enum Stop {
    Cancelled,
    Failed(String),
}

impl From<InvocationError> for Stop {
    fn from(e: InvocationError) -> Self {
        Stop::Failed(lf!("Експорт: {e}", "Export: {e}"))
    }
}

impl From<String> for Stop {
    fn from(e: String) -> Self {
        Stop::Failed(e)
    }
}

impl Job {
    /// A request inside the takeout session: Telegram lets it through with higher limits.
    async fn call<R: RemoteCall>(&self, query: R) -> Result<R::Return, InvocationError> {
        self.client.invoke(&tl::functions::InvokeWithTakeout { takeout_id: self.id, query }).await
    }

    fn check(&self) -> Result<(), Stop> {
        if self.cancel.load(Ordering::Relaxed) { Err(Stop::Cancelled) } else { Ok(()) }
    }

    fn report(&self) {
        self.out.send(Ev::Takeout(TakeoutEv::Progress(self.p.clone())));
    }

    fn step(&mut self, step: &str) {
        self.p.step = step.to_owned();
        self.report();
    }

    fn remember(&mut self, users: &[tl::enums::User], chats: &[tl::enums::Chat]) {
        for u in users {
            if let tl::enums::User::User(u) = u {
                self.names.insert((0, u.id), user_name(u));
            }
        }
        for c in chats {
            match c {
                tl::enums::Chat::Chat(c) => _ = self.names.insert((1, c.id), c.title.clone()),
                tl::enums::Chat::Forbidden(c) => _ = self.names.insert((1, c.id), c.title.clone()),
                tl::enums::Chat::Channel(c) => _ = self.names.insert((2, c.id), c.title.clone()),
                tl::enums::Chat::ChannelForbidden(c) => _ = self.names.insert((2, c.id), c.title.clone()),
                _ => {}
            }
        }
    }

    async fn run(&mut self) -> Result<(), Stop> {
        std::fs::create_dir_all(&self.dir).map_err(|e| e.to_string())?;
        make_private(&self.dir);
        self.account().await?;
        if self.o.contacts {
            self.check()?;
            self.contacts().await?;
        }
        if self.o.sessions {
            self.check()?;
            self.sessions().await?;
        }
        if self.o.any_chats() {
            self.check()?;
            let chats = self.dialogs().await?;
            self.p.chats_total = chats.len();
            for (k, chat) in chats.iter().enumerate() {
                self.check()?;
                self.chat(k, chat).await?;
                self.p.chats_done = k + 1;
            }
        }
        export::write_index(&self.dir, l!("Експортовані дані", "Exported Data"), &self.index, self.o.format)?;
        Ok(())
    }

    /// Who we are (needed for "only my messages" and outgoing names), and the info section.
    async fn account(&mut self) -> Result<(), Stop> {
        self.step(l!("Інформація про акаунт", "Account information"));
        let tl::enums::users::UserFull::Full(full) = self.call(tl::functions::users::GetFullUser { id: tl::enums::InputUser::UserSelf }).await?;
        self.remember(&full.users, &full.chats);
        let tl::enums::UserFull::Full(about) = full.full_user;
        let Some(tl::enums::User::User(me)) = full.users.iter().find(|u| matches!(u, tl::enums::User::User(u) if u.is_self)) else {
            return Err(Stop::Failed(l!("Не вдалося отримати дані акаунта", "Could not load the account").to_owned()));
        };
        self.me = me.id;
        self.my_name = user_name(me);
        if self.o.info {
            let row = vec![
                (l!("Ім'я", "First name"), me.first_name.clone().unwrap_or_default()),
                (l!("Прізвище", "Last name"), me.last_name.clone().unwrap_or_default()),
                (l!("Ім'я користувача", "Username"), me.username.clone().map(|u| format!("@{u}")).unwrap_or_default()),
                (l!("Телефон", "Phone number"), me.phone.clone().map(|p| format!("+{p}")).unwrap_or_default()),
                (l!("Про себе", "Bio"), about.about.unwrap_or_default()),
            ];
            let title = l!("Інформація про акаунт", "Personal information");
            export::write_table(&self.dir, "personal_information", title, &[row], self.o.format)?;
            self.index.push((title.to_owned(), "personal_information".into(), String::new()));
        }
        Ok(())
    }

    async fn contacts(&mut self) -> Result<(), Stop> {
        self.step(l!("Контакти", "Contacts"));
        let rows: Vec<Vec<(&str, String)>> = match self.call(tl::functions::contacts::GetContacts { hash: 0 }).await? {
            tl::enums::contacts::Contacts::Contacts(c) => c
                .users
                .iter()
                .filter_map(|u| match u {
                    tl::enums::User::User(u) => Some(vec![
                        (l!("Ім'я", "Name"), user_name(u)),
                        (l!("Телефон", "Phone number"), u.phone.clone().map(|p| format!("+{p}")).unwrap_or_default()),
                        (l!("Ім'я користувача", "Username"), u.username.clone().map(|n| format!("@{n}")).unwrap_or_default()),
                    ]),
                    _ => None,
                })
                .collect(),
            tl::enums::contacts::Contacts::NotModified => Vec::new(),
        };
        let title = l!("Контакти", "Contacts");
        export::write_table(&self.dir, "contacts", title, &rows, self.o.format)?;
        self.index.push((title.to_owned(), "contacts".into(), lp!(rows.len(), ["{n} контакт", "{n} контакти", "{n} контактів"], ["{n} contact", "{n} contacts"])));
        Ok(())
    }

    async fn sessions(&mut self) -> Result<(), Stop> {
        self.step(l!("Активні сеанси", "Active sessions"));
        let tl::enums::account::Authorizations::Authorizations(a) = self.call(tl::functions::account::GetAuthorizations {}).await?;
        let rows: Vec<Vec<(&str, String)>> = a
            .authorizations
            .iter()
            .map(|tl::enums::Authorization::Authorization(s)| {
                vec![
                    (l!("Пристрій", "Device"), format!("{} {}", s.device_model, s.system_version).trim().to_owned()),
                    (l!("Застосунок", "Application"), format!("{} {}", s.app_name, s.app_version).trim().to_owned()),
                    (l!("IP-адреса", "IP address"), s.ip.clone()),
                    (l!("Країна", "Country"), s.country.clone()),
                    (l!("Остання активність", "Last active"), export::date(s.date_active as i64)),
                    (l!("Створено", "Created"), export::date(s.date_created as i64)),
                ]
            })
            .collect();
        let title = l!("Активні сеанси", "Active sessions");
        export::write_table(&self.dir, "sessions", title, &rows, self.o.format)?;
        self.index.push((title.to_owned(), "sessions".into(), lp!(rows.len(), ["{n} сеанс", "{n} сеанси", "{n} сеансів"], ["{n} session", "{n} sessions"])));
        let tl::enums::account::WebAuthorizations::Authorizations(w) = self.call(tl::functions::account::GetWebAuthorizations {}).await?;
        if !w.authorizations.is_empty() {
            let rows: Vec<Vec<(&str, String)>> = w
                .authorizations
                .iter()
                .map(|tl::enums::WebAuthorization::Authorization(s)| {
                    vec![
                        (l!("Сайт", "Website"), s.domain.clone()),
                        (l!("Браузер", "Browser"), format!("{} {}", s.browser, s.platform).trim().to_owned()),
                        (l!("IP-адреса", "IP address"), s.ip.clone()),
                        (l!("Остання активність", "Last active"), export::date(s.date_active as i64)),
                    ]
                })
                .collect();
            let title = l!("Сеанси на сайтах", "Web sessions");
            export::write_table(&self.dir, "web_sessions", title, &rows, self.o.format)?;
            self.index.push((title.to_owned(), "web_sessions".into(), String::new()));
        }
        Ok(())
    }

    /// Every dialog of the chosen kinds, the main list and the archive.
    async fn dialogs(&mut self) -> Result<Vec<Chat>, Stop> {
        self.step(l!("Список чатів", "Chat list"));
        let mut chats: Vec<Chat> = Vec::new();
        let mut seen = HashSet::new();
        for folder in [None, Some(1)] {
            let (mut offset_date, mut offset_id, mut offset_peer) = (0, 0, tl::enums::InputPeer::Empty);
            loop {
                self.check()?;
                let req = tl::functions::messages::GetDialogs { exclude_pinned: false, folder_id: folder, offset_date, offset_id, offset_peer: offset_peer.clone(), limit: PAGE_SIZE, hash: 0 };
                let (dialogs, messages, users, chats_raw, last) = match self.call(req).await? {
                    tl::enums::messages::Dialogs::Dialogs(d) => (d.dialogs, d.messages, d.users, d.chats, true),
                    tl::enums::messages::Dialogs::Slice(d) => {
                        let last = d.dialogs.len() < PAGE_SIZE as usize;
                        (d.dialogs, d.messages, d.users, d.chats, last)
                    }
                    tl::enums::messages::Dialogs::NotModified(_) => break,
                };
                self.remember(&users, &chats_raw);
                let mut next = None;
                for d in &dialogs {
                    let tl::enums::Dialog::Dialog(d) = d else { continue };
                    let Some((input, kind)) = classify(&d.peer, &users, &chats_raw, self.me) else { continue };
                    next = Some((d.top_message, input.clone(), peer_key(&d.peer)));
                    if seen.insert(peer_key(&d.peer)) && self.o.wants(kind) {
                        let title = self.names.get(&peer_key(&d.peer)).cloned().unwrap_or_default();
                        chats.push(Chat { title, input, kind });
                    }
                }
                let Some((top, input, key)) = next.filter(|_| !last) else { break };
                // The next page starts after the last dialog's top message.
                offset_date = messages.iter().find_map(|m| match m {
                    tl::enums::Message::Message(m) if m.id == top && peer_key(&m.peer_id) == key => Some(m.date),
                    tl::enums::Message::Service(m) if m.id == top && peer_key(&m.peer_id) == key => Some(m.date),
                    _ => None,
                }).unwrap_or(0);
                offset_id = top;
                offset_peer = input;
            }
        }
        Ok(chats)
    }

    /// One chat's history, newest page first, written oldest first.
    async fn chat(&mut self, k: usize, chat: &Chat) -> Result<(), Stop> {
        self.step(&chat.title);
        let rel = format!("chats/chat_{:03}", k + 1);
        let dir = self.dir.join(&rel);
        let files = dir.join("files");
        if self.o.any_media() {
            std::fs::create_dir_all(&files).map_err(|e| e.to_string())?;
        } else {
            std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
        }
        let only_my = self.o.only_my && chat.kind == Category::PublicGroup;
        let mut entries = Vec::new();
        let mut offset_id = 0;
        loop {
            self.check()?;
            let res = if only_my {
                self.call(tl::functions::messages::Search {
                    peer: chat.input.clone(),
                    q: String::new(),
                    from_id: Some(tl::enums::InputPeer::PeerSelf),
                    saved_peer_id: None,
                    saved_reaction: None,
                    top_msg_id: None,
                    filter: tl::enums::MessagesFilter::InputMessagesFilterEmpty,
                    min_date: 0,
                    max_date: 0,
                    offset_id,
                    add_offset: 0,
                    limit: PAGE_SIZE,
                    max_id: 0,
                    min_id: 0,
                    hash: 0,
                })
                .await?
            } else {
                self.call(tl::functions::messages::GetHistory { peer: chat.input.clone(), offset_id, offset_date: 0, add_offset: 0, limit: PAGE_SIZE, max_id: 0, min_id: 0, hash: 0 }).await?
            };
            let (messages, users, chats) = match res {
                tl::enums::messages::Messages::Messages(m) => (m.messages, m.users, m.chats),
                tl::enums::messages::Messages::Slice(m) => (m.messages, m.users, m.chats),
                tl::enums::messages::Messages::ChannelMessages(m) => (m.messages, m.users, m.chats),
                tl::enums::messages::Messages::NotModified(_) => break,
            };
            self.remember(&users, &chats);
            let Some(last) = messages.iter().filter_map(msg_id).min() else { break };
            for m in messages {
                self.check()?;
                if let Some(e) = self.entry(chat, m, &files).await {
                    entries.push(e);
                    self.p.messages += 1;
                }
            }
            self.report();
            offset_id = last;
        }
        entries.reverse();
        export::write_chat(&dir, &chat.title, &entries, self.o.format)?;
        let note = lp!(entries.len(), ["{n} повідомлення", "{n} повідомлення", "{n} повідомлень"], ["{n} message", "{n} messages"]);
        self.index.push((chat.title.clone(), format!("{rel}/{}", if self.o.format.html() { "messages" } else { "result" }), note));
        Ok(())
    }

    /// A message for the export, its attachment saved if the media choices allow it.
    async fn entry(&mut self, chat: &Chat, m: tl::enums::Message, files: &Path) -> Option<Entry> {
        let (id, date, from_id, out, text, reply, media) = match m {
            tl::enums::Message::Message(m) => (m.id, m.date, m.from_id, m.out, m.message, m.reply_to, m.media),
            tl::enums::Message::Service(m) => (m.id, m.date, m.from_id, m.out, String::new(), m.reply_to, None),
            tl::enums::Message::Empty(_) => return None,
        };
        let from = match &from_id {
            Some(p) => self.names.get(&peer_key(p)).cloned().unwrap_or_default(),
            None if out => self.my_name.clone(),
            None => chat.title.clone(),
        };
        let reply_to = match reply {
            Some(tl::enums::MessageReplyHeader::Header(h)) => h.reply_to_msg_id,
            _ => None,
        };
        let mut file = None;
        let mut label = None;
        if let Some(md) = media {
            label = Some(media_label(&md));
            if let Some((kind, size)) = file_kind(&md)
                && self.o.wants_file(kind, size)
            {
                file = self.save(id, md, files, size).await;
            }
        } else if text.is_empty() {
            label = Some(l!("Службове повідомлення", "Service message"));
        }
        Some(Entry { id, date: export::date(date as i64), from, text, reply_to, media: if file.is_none() { label } else { None }, file })
    }

    async fn save(&mut self, id: i32, md: tl::enums::MessageMedia, files: &Path, size: u64) -> Option<String> {
        let media = Media::from_raw(md)?;
        let name = match &media {
            Media::Photo(_) => format!("photo_{id}.jpg"),
            Media::Document(d) => format!("{id}_{}", crate::platform::safe_file_name(d.name().unwrap_or_default())),
            Media::Sticker(s) => format!("{id}_{}", crate::platform::safe_file_name(s.document.name().unwrap_or("sticker.webp"))),
            _ => return None,
        };
        let path = files.join(&name);
        if self.client.download_media(&media, &path).await.is_err() {
            let _ = std::fs::remove_file(&path);
            return None;
        }
        self.p.files += 1;
        self.p.bytes += std::fs::metadata(&path).map(|m| m.len()).unwrap_or(size);
        Some(format!("files/{name}"))
    }
}

fn msg_id(m: &tl::enums::Message) -> Option<i32> {
    match m {
        tl::enums::Message::Message(m) => Some(m.id),
        tl::enums::Message::Service(m) => Some(m.id),
        tl::enums::Message::Empty(_) => None,
    }
}

/// The input peer and export category of a dialog; `None` for chats we can't address.
fn classify(peer: &tl::enums::Peer, users: &[tl::enums::User], chats: &[tl::enums::Chat], me: i64) -> Option<(tl::enums::InputPeer, Category)> {
    match peer {
        tl::enums::Peer::User(p) => {
            let u = users.iter().find_map(|u| match u {
                tl::enums::User::User(u) if u.id == p.user_id => Some(u),
                _ => None,
            })?;
            let input = if u.id == me { tl::enums::InputPeer::PeerSelf } else { tl::types::InputPeerUser { user_id: u.id, access_hash: u.access_hash? }.into() };
            Some((input, if u.bot { Category::Bot } else { Category::Personal }))
        }
        tl::enums::Peer::Chat(p) => Some((tl::types::InputPeerChat { chat_id: p.chat_id }.into(), Category::PrivateGroup)),
        tl::enums::Peer::Channel(p) => {
            let c = chats.iter().find_map(|c| match c {
                tl::enums::Chat::Channel(c) if c.id == p.channel_id => Some(c),
                _ => None,
            })?;
            let public = c.username.is_some() || c.usernames.as_ref().is_some_and(|u| !u.is_empty());
            let kind = match (c.megagroup, public) {
                (true, true) => Category::PublicGroup,
                (true, false) => Category::PrivateGroup,
                (false, true) => Category::PublicChannel,
                (false, false) => Category::PrivateChannel,
            };
            Some((tl::types::InputPeerChannel { channel_id: c.id, access_hash: c.access_hash? }.into(), kind))
        }
    }
}

/// The dated folder of a new export inside the chosen one, as Telegram Desktop names it.
pub fn export_dir(base: &Path) -> PathBuf {
    let stamp = jiff::Zoned::now().strftime("%Y-%m-%d").to_string();
    base.join(crate::platform::safe_file_name(&format!("Telegram Desktop Export {stamp}")))
}

/// The takeout flags for the chosen data.
fn init_request(o: &TakeoutOptions) -> tl::functions::account::InitTakeoutSession {
    let files = o.any_media() && o.any_chats();
    tl::functions::account::InitTakeoutSession {
        contacts: o.contacts,
        message_users: o.personal || o.bots,
        message_chats: o.private_groups,
        message_megagroups: o.private_groups || o.public_groups,
        message_channels: o.private_channels || o.public_channels,
        files,
        file_max_size: files.then(|| o.max_mb.max(1) as i64 * 1_048_576),
    }
}

async fn run(client: Client, out: Emitter, o: TakeoutOptions, cancel: Arc<AtomicBool>) {
    let id = match client.invoke(&init_request(&o)).await {
        Ok(tl::enums::account::Takeout::Takeout(t)) => t.id,
        Err(InvocationError::Rpc(e)) if e.is("TAKEOUT_INIT_DELAY") => {
            // Telegram makes a new account owner wait (usually 24 hours) and notifies the other sessions.
            let wait = e.value.map_or(86_400, |v| v as i64);
            out.send(Ev::Takeout(TakeoutEv::Delay { until: jiff::Timestamp::now().as_second() + wait }));
            return;
        }
        Err(e) => {
            out.send(Ev::Takeout(TakeoutEv::Failed(lf!("Експорт: {e}", "Export: {e}"))));
            return;
        }
    };
    let dir = export_dir(&o.dir);
    let mut job = Job { client: client.clone(), id, out: out.clone(), o, cancel, dir, p: TakeoutProgress::default(), me: 0, my_name: String::new(), names: HashMap::new(), index: Vec::new() };
    let result = job.run().await;
    let finish = tl::functions::account::FinishTakeoutSession { success: result.is_ok() };
    let _ = job.call(finish).await;
    out.send(Ev::Takeout(match result {
        Ok(()) => TakeoutEv::Done { dir: job.dir, progress: job.p },
        Err(Stop::Cancelled) => TakeoutEv::Cancelled,
        Err(Stop::Failed(e)) => TakeoutEv::Failed(e),
    }));
}

impl Worker {
    /// Starting and cancelling the full account export.
    pub(super) async fn takeout_cmd(&mut self, cmd: Cmd) -> Result<(), String> {
        match cmd {
            Cmd::StartTakeout(o) => {
                let client = self.client()?;
                let cancel = Arc::new(AtomicBool::new(false));
                if let Some(old) = self.takeout.replace(cancel.clone()) {
                    old.store(true, Ordering::Relaxed);
                }
                // Hours of history and files: in the background, so other commands go on.
                tokio::spawn(run(client, self.out.clone(), o, cancel));
                Ok(())
            }
            Cmd::CancelTakeout => {
                if let Some(c) = self.takeout.take() {
                    c.store(true, Ordering::Relaxed);
                }
                Ok(())
            }
            _ => Ok(()),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn flags_follow_the_choice() {
        let o = TakeoutOptions { public_groups: true, photos: true, max_mb: 4, ..Default::default() };
        let r = init_request(&o);
        assert!(r.contacts && r.message_users && r.message_megagroups && !r.message_chats && !r.message_channels);
        assert_eq!(r.file_max_size, Some(4 * 1_048_576));
        let none = TakeoutOptions { personal: false, ..TakeoutOptions { photos: true, ..Default::default() } };
        assert!(!init_request(&none).files);
    }

    #[test]
    fn size_limit_spares_photos() {
        let o = TakeoutOptions { photos: true, files: true, max_mb: 1, ..Default::default() };
        assert!(o.wants_file(FileKind::Photo, 50_000_000));
        assert!(!o.wants_file(FileKind::File, 2_000_000));
        assert!(o.wants_file(FileKind::File, 500_000));
        assert!(!o.wants_file(FileKind::Video, 10));
    }
}
