//! Chat info column, modelled on the Telegram Desktop profile: avatar and name, description,
//! links, notifications, members, shared media tabs and the leave/clear/delete actions.

use std::collections::HashMap;

use eframe::egui::{self, Align2, Color32, FontId, Rect, Sense, Ui, UiBuilder, pos2, text::LayoutJob, vec2};
use grammers_client::session::types::PeerId;

use super::{App, SEARCH_DELAY, elided, kind_icon};
use crate::backend::{ChatDetails, ChatInfo, ChatKind, Cmd, Ev, ImgKey, MUTE_FOREVER, MediaKind, Member, MemberRole, Msg, SHARED_PAGE, SearchHit, SharedKind};
use crate::format::Style;
use crate::glass::{self, ACCENT, Glass, ink, ink_dim};
use crate::i18n::{l, lf};
use crate::icons::{self, Icon};
use crate::player;

/// Info column width; narrower windows get it as an overlay over the chat.
pub(super) const INFO_W: f32 = 340.0;
/// Below this chat width the column overlays the chat instead of squeezing it.
pub(super) const INFO_MIN_CHAT: f32 = 420.0;

#[derive(Clone, Copy, PartialEq)]
pub(super) enum Tab {
    Members,
    Shared(SharedKind),
    /// Recommended channels (channels only).
    Similar,
    /// Gifts in a person's profile.
    Gifts,
}

/// Destructive action waiting for confirmation.
#[derive(Clone, Copy, PartialEq)]
pub(super) enum Confirm {
    Leave,
    Clear,
    Delete,
    DeleteContact,
    /// Block a user, or stop a bot.
    Block,
}

#[derive(Default)]
struct SharedList {
    msgs: Vec<Msg>,
    loading: bool,
    exhausted: bool,
}

pub(super) struct InfoPanel {
    peer: PeerId,
    details: Option<ChatDetails>,
    tab: Tab,
    members: Vec<Member>,
    members_loaded: bool,
    query: String,
    admins: bool,
    edited_at: f64,
    /// Last members request: (query, admins only).
    requested: (String, bool),
    shared: HashMap<SharedKind, SharedList>,
    /// Similar channels and how many there are in all; `None` until loaded.
    similar: Option<(Vec<ChatInfo>, i32)>,
    /// Gifts and how many there are in all; `None` until loaded.
    gifts: Option<(Vec<crate::backend::Gift>, i32)>,
    pub(super) notify_open: bool,
    pub(super) confirm: Option<Confirm>,
    /// "Also delete for the other side" checkbox in private chats.
    revoke: bool,
}

/// What a click in the panel asks for; applied after drawing, when `self.info` is back in place.
enum Do {
    Close,
    Copy(String),
    OpenUrl(String),
    Member(ChatInfo),
    Photo(i32),
    Jump(Msg),
    Tab(Tab),
    Notify(i32, bool),
    More(SharedKind),
    CreateInvite,
    Confirm(Confirm),
    /// The "Edit" box of a group or channel.
    Edit,
    AddContact,
    Unblock,
}

fn has_members(kind: ChatKind) -> bool {
    matches!(kind, ChatKind::Group | ChatKind::Channel)
}


impl App {
    /// Opens the panel for a chat and requests everything it shows.
    pub(super) fn open_info(&mut self, peer: PeerId) {
        let Some(kind) = self.chats.iter().find(|c| c.id == peer).map(|c| c.kind) else {
            self.info = None;
            return;
        };
        let tab = if kind == ChatKind::Group { Tab::Members } else { Tab::Shared(SharedKind::Media) };
        self.info = Some(InfoPanel {
            peer,
            details: None,
            tab,
            members: Vec::new(),
            members_loaded: false,
            query: String::new(),
            admins: false,
            edited_at: 0.0,
            requested: (String::new(), false),
            shared: HashMap::new(),
            similar: None,
            gifts: None,
            notify_open: false,
            confirm: None,
            revoke: false,
        });
        self.send(Cmd::LoadChatInfo(peer));
        // Asked up front: the tab is shown only when there is something in it.
        if kind == ChatKind::Channel {
            self.send(Cmd::LoadSimilarChannels(peer));
        }
        self.select_info_tab(tab);
    }

    /// The column follows the open chat, like in Telegram Desktop.
    pub(super) fn sync_info(&mut self) {
        let Some(info) = &self.info else { return };
        match self.selected {
            Some(p) if p != info.peer => self.open_info(p),
            None => self.info = None,
            _ => {}
        }
    }

    pub(super) fn select_info_tab(&mut self, tab: Tab) {
        let Some(info) = &mut self.info else { return };
        info.tab = tab;
        let peer = info.peer;
        match tab {
            Tab::Members => {
                if !info.members_loaded && info.requested == (String::new(), false) {
                    self.send(Cmd::LoadMembers { peer, query: String::new(), admins: false });
                }
            }
            Tab::Shared(kind) => {
                let list = info.shared.entry(kind).or_default();
                if list.msgs.is_empty() && !list.loading && !list.exhausted {
                    list.loading = true;
                    self.send(Cmd::LoadShared { peer, kind, before: None });
                }
            }
            Tab::Similar | Tab::Gifts => {}
        }
    }

    pub(super) fn info_event(&mut self, ev: Ev) {
        match ev {
            Ev::ChatDetails { peer, details } => {
                let muted = details.notify.muted();
                if let Some(c) = self.chat_mut(peer) {
                    c.muted = muted;
                }
                let gifts = details.gifts > 0;
                if let Some(info) = self.info.as_mut().filter(|i| i.peer == peer) {
                    info.details = Some(details);
                }
                // Asked only when the profile has some, so the tab never shows empty.
                if gifts {
                    self.send(Cmd::LoadGifts(peer));
                }
            }
            Ev::Gifts { peer, gifts, total } => {
                if let Some(info) = self.info.as_mut().filter(|i| i.peer == peer) {
                    info.gifts = Some((gifts, total));
                }
            }
            Ev::SimilarChannels { peer, chats, total } => {
                if let Some(info) = self.info.as_mut().filter(|i| i.peer == peer) {
                    info.similar = Some((chats, total));
                }
            }
            Ev::Members { peer, query, admins, list } => {
                if let Some(info) = self.info.as_mut().filter(|i| i.peer == peer && i.requested == (query.clone(), admins)) {
                    info.members = list;
                    info.members_loaded = true;
                }
            }
            Ev::Invite { peer, link } => {
                if let Some(d) = self.info.as_mut().filter(|i| i.peer == peer).and_then(|i| i.details.as_mut()) {
                    d.invite = link;
                }
            }
            Ev::Shared { peer, kind, msgs, older } => {
                if let Some(info) = self.info.as_mut().filter(|i| i.peer == peer) {
                    let list = info.shared.entry(kind).or_default();
                    list.loading = false;
                    list.exhausted = msgs.len() < SHARED_PAGE;
                    if !older {
                        list.msgs.clear();
                    }
                    list.msgs.extend(msgs.into_iter().filter(|m| m.id != 0));
                }
            }
            Ev::Notify { peer, state } => {
                self.notify_changed(peer, state);
                if let Some(c) = self.chat_mut(peer) {
                    c.muted = state.muted();
                }
                if let Some(d) = self.info.as_mut().filter(|i| i.peer == peer).and_then(|i| i.details.as_mut()) {
                    d.notify = state;
                }
            }
            Ev::ChatRemoved(peer) => {
                self.chats.retain(|c| c.id != peer);
                self.history.remove(&peer);
                if self.selected == Some(peer) {
                    self.selected = None;
                }
                if self.info.as_ref().is_some_and(|i| i.peer == peer) {
                    self.info = None;
                }
                self.chats_dirty = true;
                self.toast = Some((l!("Чат прибрано зі списку", "Chat removed from the list").into(), 0.0, false));
            }
            Ev::HistoryCleared(peer) => {
                self.history.insert(peer, Vec::new());
                self.exhausted.insert(peer, true);
                if let Some(c) = self.chat_mut(peer) {
                    c.last_text.clear();
                    c.last_time.clear();
                }
                if let Some(info) = self.info.as_mut().filter(|i| i.peer == peer) {
                    info.shared.clear();
                    if let Some(d) = &mut info.details {
                        d.counts.clear();
                    }
                    let tab = info.tab;
                    self.select_info_tab(tab);
                }
                self.chats_dirty = true;
                self.toast = Some((l!("Історію очищено", "History cleared").into(), 0.0, false));
            }
            _ => {}
        }
    }

    pub(super) fn info_view(&mut self, ui: &mut Ui, rect: Rect) {
        let Some(mut info) = self.info.take() else { return };
        let Some(chat) = self.chats.iter().find(|c| c.id == info.peer).cloned() else { return };
        let now = ui.input(|i| i.time);
        let mut todo = Vec::new();
        glass::glass(ui.painter(), rect, Glass::panel(26.0).with_shadow());
        // Swallow clicks so they do not reach the chat underneath in overlay mode.
        ui.interact(rect, ui.scope_id().with("info-bg"), Sense::click());

        let title = match chat.kind {
            ChatKind::User | ChatKind::Bot => l!("Інформація", "User info"),
            ChatKind::Group => l!("Про групу", "Group info"),
            ChatKind::Channel => l!("Про канал", "Channel info"),
            ChatKind::Saved => l!("Збережене", "Saved Messages"),
        };
        let top = Rect::from_min_size(rect.min, vec2(rect.width(), 56.0));
        ui.painter().text(pos2(top.left() + 20.0, top.center().y), Align2::LEFT_CENTER, title, FontId::proportional(17.0), ink());
        let close = Rect::from_center_size(pos2(top.right() - 30.0, top.center().y), vec2(36.0, 36.0));
        if glass::icon_button(ui, close, "info-close", Icon::Close, false, true).on_hover_text(l!("Закрити", "Close")).clicked() {
            todo.push(Do::Close);
        }
        // Admins edit the group or channel from the pencil, as in Telegram Desktop.
        if matches!(chat.kind, ChatKind::Group | ChatKind::Channel) && info.details.as_ref().is_some_and(|d| d.can_edit) {
            let edit = close.translate(vec2(-44.0, 0.0));
            if glass::icon_button(ui, edit, "info-edit", Icon::Pencil, false, true).on_hover_text(l!("Редагувати", "Edit")).clicked() {
                todo.push(Do::Edit);
            }
        }

        let body = Rect::from_min_max(pos2(rect.left() + 8.0, top.bottom()), pos2(rect.right() - 8.0, rect.bottom() - 10.0));
        let mut child = ui.new_child(UiBuilder::new().max_rect(body));
        child.set_clip_rect(body);
        egui::ScrollArea::vertical().id_salt(("info", info.peer)).auto_shrink(false).show(&mut child, |ui| {
            ui.spacing_mut().item_spacing.y = 2.0;
            self.info_profile(ui, &chat, &info);
            self.info_rows(ui, &chat, &mut info, &mut todo);
            ui.add_space(8.0);
            info_tabs(ui, &chat, &info, &mut todo);
            ui.add_space(6.0);
            match info.tab {
                Tab::Members => self.info_members(ui, &mut info, now, &mut todo),
                Tab::Shared(kind) => self.info_shared(ui, &info, kind, &mut todo),
                Tab::Gifts => {
                    if let Some((gifts, total)) = &info.gifts {
                        self.gift_grid(ui, gifts, *total);
                    }
                }
                Tab::Similar => {
                    let (chats, total) = info.similar.as_ref().map_or((&[][..], 0), |(c, t)| (&c[..], *t));
                    if let Some(c) = self.similar_list(ui, chats, total) {
                        todo.push(Do::Member(c));
                    }
                }
            }
            ui.add_space(10.0);
            let creator = info.details.as_ref().is_some_and(|d| d.creator);
            let blocked = info.details.as_ref().map(|d| d.blocked);
            info_actions(ui, &chat, creator, blocked, &mut todo);
            ui.add_space(8.0);
        });

        // Members search goes to the server after a typing pause.
        let wanted = (info.query.trim().to_owned(), info.admins);
        let search = (wanted != info.requested && now - info.edited_at > SEARCH_DELAY).then(|| {
            info.requested = wanted.clone();
            info.members_loaded = false;
            wanted
        });
        let peer = info.peer;
        self.info = Some(info);
        if let Some((query, admins)) = search {
            self.send(Cmd::LoadMembers { peer, query, admins });
        }
        for d in todo {
            self.info_apply(peer, &chat, d);
        }
    }

    fn info_apply(&mut self, peer: PeerId, chat: &ChatInfo, d: Do) {
        match d {
            Do::Close => self.info = None,
            Do::Copy(text) => {
                self.ctx.copy_text(text);
                self.toast = Some((l!("Скопійовано", "Copied").into(), 0.0, false));
            }
            Do::OpenUrl(url) => self.open_link(&url),
            Do::Member(c) => {
                let id = c.id;
                if !self.chats.iter().any(|x| x.id == id) {
                    self.chats.push(c);
                }
                // Demo answers synchronously and has no history for new people.
                if self.backend.is_none() {
                    self.history.entry(id).or_default();
                }
                self.open_chat(id);
            }
            Do::Photo(id) => self.open_viewer(peer, id),
            Do::Jump(msg) => self.jump_to(&SearchHit { chat: chat.title.clone(), kind: chat.kind, msg }),
            Do::Tab(tab) => self.select_info_tab(tab),
            Do::Notify(mute_until, silent) => {
                if let Some(i) = &mut self.info {
                    i.notify_open = false;
                }
                self.send(Cmd::SetNotify { peer, mute_until, silent });
            }
            Do::More(kind) => {
                let before = self.info.as_mut().and_then(|i| {
                    let list = i.shared.get_mut(&kind)?;
                    list.loading = true;
                    list.msgs.last().map(|m| m.id)
                });
                self.send(Cmd::LoadShared { peer, kind, before });
            }
            Do::CreateInvite => self.send(Cmd::ExportInvite(peer)),
            Do::Edit => {
                let d = self.info.as_ref().and_then(|i| i.details.clone()).unwrap_or_default();
                self.open_chat_edit(peer, d.about, d.rights, d.slowmode);
            }
            Do::AddContact => self.open_contact_form(Some((peer, &chat.title))),
            Do::Unblock => self.send(Cmd::Block { peer, blocked: false }),
            Do::Confirm(c) => {
                if let Some(i) = &mut self.info {
                    i.confirm = Some(c);
                    i.revoke = false;
                }
            }
        }
    }

    /// Big avatar, name and the status line.
    fn info_profile(&self, ui: &mut Ui, chat: &ChatInfo, info: &InfoPanel) {
        let w = ui.available_width();
        let (r, _) = ui.allocate_exact_size(vec2(w, 150.0), Sense::hover());
        let p = ui.painter();
        let tex = self.textures.get(&ImgKey::Avatar(chat.id));
        glass::avatar(p, pos2(r.center().x, r.top() + 46.0), 42.0, &chat.title, chat.id.bot_api_dialog_id_unchecked(), kind_icon(chat.kind), tex);
        p.galley(pos2(r.center().x, r.top() + 98.0) - vec2(elided(ui, &chat.title, 17.0, ink(), w - 24.0).size().x / 2.0, 0.0), elided(ui, &chat.title, 17.0, ink(), w - 24.0), ink());
        let d = info.details.as_ref();
        let (sub, color) = match chat.kind {
            ChatKind::User => match chat.presence.text() {
                Some(t) => (t, if chat.presence.online() { ACCENT } else { ink_dim() }),
                None => (l!("особистий чат", "private chat").to_owned(), ink_dim()),
            },
            ChatKind::Bot => (l!("бот", "bot").to_owned(), ink_dim()),
            ChatKind::Saved => (l!("ваші нотатки", "your notes").to_owned(), ink_dim()),
            ChatKind::Group => match (d.and_then(|d| d.members), d.and_then(|d| d.online).filter(|n| *n > 0)) {
                (Some(n), Some(o)) => (lf!("учасників: {n}, у мережі: {o}", "{n} members, {o} online"), ink_dim()),
                (Some(n), None) => (lf!("учасників: {n}", "{n} members"), ink_dim()),
                _ => (l!("група", "group").to_owned(), ink_dim()),
            },
            ChatKind::Channel => match d.and_then(|d| d.members) {
                Some(n) => (lf!("підписників: {n}", "{n} subscribers"), ink_dim()),
                None => (l!("канал", "channel").to_owned(), ink_dim()),
            },
        };
        p.text(pos2(r.center().x, r.top() + 132.0), Align2::CENTER_CENTER, sub, FontId::proportional(12.5), color);
    }

    /// Description, username, phone, invite link and notifications.
    fn info_rows(&self, ui: &mut Ui, chat: &ChatInfo, info: &mut InfoPanel, todo: &mut Vec<Do>) {
        let Some(d) = info.details.clone() else {
            ui.add_space(8.0);
            ui.vertical_centered(|ui| ui.add(egui::Spinner::new().color(ink())));
            return;
        };
        let user = matches!(chat.kind, ChatKind::User | ChatKind::Bot);
        if !d.about.is_empty() {
            let caption = if user { l!("Про себе", "Bio") } else { l!("Опис", "Description") };
            if info_row(ui, Icon::Info, &d.about, caption).clicked() {
                todo.push(Do::Copy(d.about.clone()));
            }
        }
        if !d.username.is_empty() {
            let (text, caption) = if user && !d.usernames.is_empty() {
                // Extra and collectible usernames, as "also @a, @b" in Telegram Desktop.
                let also = d.usernames.iter().map(|u| format!("@{u}")).collect::<Vec<_>>().join(", ");
                (format!("@{}", d.username), lf!("Ім'я користувача · також {also}", "Username · also {also}"))
            } else if user {
                (format!("@{}", d.username), l!("Ім'я користувача", "Username").to_owned())
            } else {
                (format!("https://t.me/{}", d.username), l!("Посилання", "Link").to_owned())
            };
            if info_row(ui, Icon::At, &text, &caption).clicked() {
                todo.push(Do::Copy(text));
            }
        }
        if !d.phone.is_empty() && info_row(ui, Icon::Phone, &d.phone, l!("Телефон", "Phone")).clicked() {
            todo.push(Do::Copy(d.phone.clone()));
        }
        if let Some(c) = &d.channel {
            let text = if c.last_text.is_empty() { c.title.clone() } else { format!("{} · {}", c.title, c.last_text) };
            if info_row(ui, Icon::Megaphone, &text, l!("Особистий канал", "Personal channel")).clicked() {
                todo.push(Do::Member(c.clone()));
            }
        }
        if let Some(b) = d.birthday {
            let today = b.is_on(jiff::Zoned::now().date());
            let text = if today { format!("🎂 {}", super::birthdays::birthday_label(b)) } else { super::birthdays::birthday_label(b) };
            info_row(ui, Icon::Cake, &text, if today { l!("Сьогодні день народження", "Birthday today") } else { l!("День народження", "Birthday") });
        }
        if d.username.is_empty() && !user && chat.kind != ChatKind::Saved {
            if !d.invite.is_empty() {
                if info_row(ui, Icon::Link, &d.invite, l!("Посилання-запрошення", "Invite link")).clicked() {
                    todo.push(Do::Copy(d.invite.clone()));
                }
            } else if d.admin {
                let w = ui.available_width();
                let (r, _) = ui.allocate_exact_size(vec2(w, 44.0), Sense::hover());
                let b = Rect::from_center_size(r.center(), vec2(w - 24.0, 36.0));
                if glass::button(ui, b, "info-invite", l!("Створити посилання-запрошення", "Create invite link"), false, true).clicked() {
                    todo.push(Do::CreateInvite);
                }
            }
        }
        if chat.kind == ChatKind::Saved {
            return;
        }
        let icon = if d.notify.muted() { Icon::Muted } else { Icon::Bell };
        if info_row(ui, icon, l!("Сповіщення", "Notifications"), &d.notify.label()).clicked() {
            info.notify_open = !info.notify_open;
        }
        if info.notify_open {
            let now = jiff::Timestamp::now().as_second() as i32;
            let silent = d.notify.silent;
            let muted = d.notify.muted();
            let mut opt = |ui: &mut Ui, label: &str, on: bool, d: Do| {
                if menu_row(ui, label, on).clicked() {
                    todo.push(d);
                }
            };
            opt(ui, l!("Увімкнути", "Unmute"), !muted, Do::Notify(0, silent));
            opt(ui, l!("Вимкнути на 1 годину", "Mute for 1 hour"), false, Do::Notify(now + 3600, silent));
            opt(ui, l!("Вимкнути на 8 годин", "Mute for 8 hours"), false, Do::Notify(now + 8 * 3600, silent));
            opt(ui, l!("Вимкнути на 2 дні", "Mute for 2 days"), false, Do::Notify(now + 2 * 86400, silent));
            opt(ui, l!("Вимкнути назавжди", "Mute forever"), false, Do::Notify(MUTE_FOREVER, silent));
            // Keep the current mute deadline, flip only the sound.
            opt(ui, l!("Без звуку", "Silent"), silent, Do::Notify(if muted { d.notify.mute_until } else { 0 }, !silent));
        }
    }

    fn info_members(&self, ui: &mut Ui, info: &mut InfoPanel, now: f64, todo: &mut Vec<Do>) {
        if info.details.as_ref().is_some_and(|d| !d.can_view_members) {
            empty(ui, l!("Список учасників бачать лише адміни", "Only admins can see the member list"));
            return;
        }
        let w = ui.available_width();
        let (r, _) = ui.allocate_exact_size(vec2(w, 40.0), Sense::hover());
        let field = Rect::from_min_max(pos2(r.left() + 6.0, r.top() + 2.0), pos2(r.right() - 6.0, r.bottom() - 2.0));
        if glass::search_field(ui, field, &mut info.query, l!("Пошук учасників", "Search members")).changed() {
            info.edited_at = now;
        }
        ui.horizontal(|ui| {
            ui.add_space(6.0);
            for (admins, label) in [(false, l!("Усі", "All")), (true, l!("Адміни", "Admins"))] {
                if chip(ui, label, info.admins == admins).clicked() && info.admins != admins {
                    info.admins = admins;
                    info.edited_at = 0.0;
                }
            }
        });
        ui.add_space(4.0);
        if !info.members_loaded {
            ui.add_space(12.0);
            ui.vertical_centered(|ui| ui.add(egui::Spinner::new().color(ink())));
            return;
        }
        if info.members.is_empty() {
            empty(ui, l!("Нікого не знайдено", "Nobody found"));
        }
        for m in &info.members {
            if self.member_row(ui, m).clicked() && m.chat.kind != ChatKind::Saved {
                todo.push(Do::Member(m.chat.clone()));
            }
        }
    }

    fn member_row(&self, ui: &mut Ui, m: &Member) -> egui::Response {
        let w = ui.available_width();
        let (rect, resp) = ui.allocate_exact_size(vec2(w, 52.0), Sense::click());
        let hover = ui.ctx().animate_bool(resp.id, resp.hovered());
        let p = ui.painter();
        if hover > 0.0 {
            p.rect_filled(rect, 16.0, glass::veil((hover * 14.0) as u8));
        }
        let c = &m.chat;
        glass::avatar(p, pos2(rect.left() + 28.0, rect.center().y), 19.0, &c.title, c.id.bot_api_dialog_id_unchecked(), kind_icon(c.kind), self.textures.get(&ImgKey::Avatar(c.id)));
        let role = match m.role {
            _ if !m.rank.is_empty() => m.rank.clone(),
            MemberRole::Owner => l!("власник", "owner").to_owned(),
            MemberRole::Admin => l!("адмін", "admin").to_owned(),
            MemberRole::Member => String::new(),
        };
        let role = elided(ui, &role, 12.0, ink_dim(), 100.0);
        let role_w = role.size().x;
        p.galley(pos2(rect.right() - 10.0 - role_w, rect.top() + 10.0), role, ink_dim());
        let x = rect.left() + 56.0;
        p.galley(pos2(x, rect.top() + 8.0), elided(ui, &c.title, 14.0, ink(), rect.right() - x - role_w - 18.0), ink());
        let (sub, color) = match c.presence.text() {
            Some(t) => (t, if c.presence.online() { ACCENT } else { ink_dim() }),
            None if c.kind == ChatKind::Bot => (l!("бот", "bot").to_owned(), ink_dim()),
            None => (String::new(), ink_dim()),
        };
        p.galley(pos2(x, rect.top() + 29.0), elided(ui, &sub, 12.5, color, rect.right() - x - 10.0), color);
        if resp.hovered() {
            ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
        }
        resp
    }

    fn info_shared(&self, ui: &mut Ui, info: &InfoPanel, kind: SharedKind, todo: &mut Vec<Do>) {
        let Some(list) = info.shared.get(&kind) else { return };
        if list.msgs.is_empty() {
            if list.loading {
                ui.add_space(12.0);
                ui.vertical_centered(|ui| ui.add(egui::Spinner::new().color(ink())));
            } else {
                empty(ui, l!("Тут поки порожньо", "Nothing here yet"));
            }
            return;
        }
        match kind {
            SharedKind::Media | SharedKind::Gifs => self.media_grid(ui, info.peer, &list.msgs, todo),
            _ => {
                for m in &list.msgs {
                    let resp = shared_row(ui, m, kind);
                    if resp.clicked() {
                        match link_of(m).filter(|_| kind == SharedKind::Links) {
                            Some(url) => todo.push(Do::OpenUrl(url)),
                            None => todo.push(Do::Jump(m.clone())),
                        }
                    }
                    resp.context_menu(|ui| {
                        if ui.button(l!("Перейти до повідомлення", "Go to message")).clicked() {
                            todo.push(Do::Jump(m.clone()));
                        }
                    });
                }
            }
        }
        if !list.exhausted {
            let w = ui.available_width();
            let (r, _) = ui.allocate_exact_size(vec2(w, 44.0), Sense::hover());
            let b = Rect::from_center_size(r.center(), vec2(180.0, 34.0));
            let label = if list.loading { l!("Завантаження…", "Loading…") } else { l!("Показати ще", "Show more") };
            if glass::button(ui, b, ("info-more", kind), label, false, !list.loading).clicked() {
                todo.push(Do::More(kind));
            }
        }
    }

    /// Square thumbnails, three per row; a click opens the photo or jumps to the video.
    fn media_grid(&self, ui: &mut Ui, peer: PeerId, msgs: &[Msg], todo: &mut Vec<Do>) {
        const COLS: usize = 3;
        const GAP: f32 = 4.0;
        let w = ui.available_width();
        let side = ((w - GAP * (COLS as f32 - 1.0)) / COLS as f32).floor();
        for row in msgs.chunks(COLS) {
            let (r, _) = ui.allocate_exact_size(vec2(w, side + GAP), Sense::hover());
            for (i, m) in row.iter().enumerate() {
                let cell = Rect::from_min_size(pos2(r.left() + i as f32 * (side + GAP), r.top()), vec2(side, side));
                let resp = ui.interact(cell, ui.scope_id().with(("shared-cell", m.id)), Sense::click());
                let p = ui.painter();
                match self.textures.get(&ImgKey::Photo(peer, m.id)) {
                    Some(t) => glass::image_rounded(p, cell, 10.0, t.id(), t.size_vec2()),
                    None => {
                        p.rect_filled(cell, 10.0, glass::veil(22));
                        let icon = if m.photo { Icon::Camera } else { Icon::Play };
                        icons::draw(p, Rect::from_center_size(cell.center(), vec2(24.0, 24.0)), icon, ink_dim());
                    }
                }
                if let Some(info) = &m.media {
                    // Dark chip so the label reads over any picture.
                    let label = if info.kind == MediaKind::Gif { "GIF".to_owned() } else { player::clock(info.duration) };
                    let g = p.layout_no_wrap(label, FontId::proportional(11.0), glass::ON_DARK);
                    let chip = Rect::from_min_size(cell.min + vec2(6.0, 6.0), g.size() + vec2(10.0, 4.0));
                    p.rect_filled(chip, chip.height() / 2.0, Color32::from_black_alpha(130));
                    p.galley(chip.min + vec2(5.0, 2.0), g, glass::ON_DARK);
                }
                if resp.hovered() {
                    p.rect_filled(cell, 10.0, glass::veil(24));
                    ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
                }
                if resp.clicked() {
                    todo.push(if m.photo { Do::Photo(m.id) } else { Do::Jump(m.clone()) });
                }
            }
        }
    }

    /// The profile shows "Unblock" right after blocking, without reloading.
    pub(super) fn info_blocked(&mut self, peer: PeerId, blocked: bool) {
        if let Some(d) = self.info.as_mut().filter(|i| i.peer == peer).and_then(|i| i.details.as_mut()) {
            d.blocked = blocked;
        }
    }

    /// Confirmation for leave / clear / delete, over the whole window.
    pub(super) fn info_confirm_view(&mut self, ui: &mut Ui) {
        let Some(info) = &mut self.info else { return };
        let Some(what) = info.confirm else { return };
        let peer = info.peer;
        let Some(chat) = self.chats.iter().find(|c| c.id == peer) else { return };
        let t = chat.title.clone();
        let kind = chat.kind;
        let creator = info.details.as_ref().is_some_and(|d| d.creator);
        let (title, body, action) = match what {
            Confirm::Leave if kind == ChatKind::Channel => (l!("Вийти з каналу", "Leave channel"), lf!("Ви справді хочете вийти з «{t}»?", "Are you sure you want to leave “{t}”?"), l!("Вийти", "Leave")),
            Confirm::Leave => (l!("Вийти з групи", "Leave group"), lf!("Ви справді хочете вийти з «{t}»?", "Are you sure you want to leave “{t}”?"), l!("Вийти", "Leave")),
            Confirm::Clear => (l!("Очистити історію", "Clear history"), lf!("Видалити всі повідомлення в «{t}»? Цю дію не можна скасувати.", "Delete all messages in “{t}”? This cannot be undone."), l!("Очистити", "Clear")),
            Confirm::Delete if creator && kind == ChatKind::Channel => (l!("Видалити канал", "Delete channel"), lf!("Ви справді хочете видалити «{t}»? Усіх підписників буде видалено, а всі повідомлення — втрачено.", "Are you sure you want to delete “{t}”? All subscribers will be removed and all messages will be lost."), l!("Видалити", "Delete")),
            Confirm::Delete if creator && kind == ChatKind::Group => (l!("Видалити групу", "Delete group"), lf!("Ви справді хочете видалити «{t}»? Усіх учасників буде видалено, а всі повідомлення — втрачено.", "Are you sure you want to delete “{t}”? All members will be removed and all messages will be lost."), l!("Видалити", "Delete")),
            Confirm::Delete if kind == ChatKind::Group => (l!("Видалити й вийти", "Delete and leave"), lf!("Вийти з «{t}» і видалити чат зі списку?", "Leave “{t}” and delete the chat from the list?"), l!("Видалити", "Delete")),
            Confirm::Delete => (l!("Видалити чат", "Delete chat"), lf!("Видалити чат з «{t}»? Цю дію не можна скасувати.", "Delete the chat with “{t}”? This cannot be undone."), l!("Видалити", "Delete")),
            Confirm::DeleteContact => (l!("Видалити контакт", "Delete contact"), lf!("Видалити «{t}» з ваших контактів?", "Delete “{t}” from your contacts?"), l!("Видалити", "Delete")),
            Confirm::Block if kind == ChatKind::Bot => (l!("Зупинити бота", "Stop bot"), lf!("Зупинити «{t}»? Бот більше не зможе вам писати.", "Stop “{t}”? The bot won't be able to message you."), l!("Зупинити", "Stop")),
            Confirm::Block => (l!("Заблокувати", "Block user"), lf!("Заблокувати «{t}»? Цей користувач не зможе писати вам і дзвонити.", "Block “{t}”? They won't be able to message or call you."), l!("Заблокувати", "Block")),
        };
        // Only private chats with a person can be wiped on both sides.
        let can_revoke = kind == ChatKind::User && matches!(what, Confirm::Clear | Confirm::Delete);
        let screen = ui.max_rect();
        ui.painter().rect_filled(screen, 0.0, Color32::from_black_alpha(90));
        ui.interact(screen, ui.scope_id().with("info-confirm-bg"), Sense::click());
        let rect = Rect::from_center_size(screen.center(), vec2(380.0, if can_revoke { 210.0 } else { 180.0 }));
        glass::glass(ui.painter(), rect, Glass::panel(24.0).with_shadow());
        let inner = rect.shrink(18.0);
        ui.painter().text(inner.left_top(), Align2::LEFT_TOP, title, FontId::proportional(17.0), ink());
        let job = LayoutJob::simple(body, FontId::proportional(14.0), ink_dim(), inner.width());
        let g = ui.painter().layout_job(job);
        ui.painter().galley(inner.left_top() + vec2(0.0, 30.0), g, ink_dim());
        if can_revoke {
            let mut child = ui.new_child(UiBuilder::new().max_rect(Rect::from_min_size(inner.left_top() + vec2(0.0, 100.0), vec2(inner.width(), 28.0))));
            let label = if what == Confirm::Clear { lf!("Також очистити для {t}", "Also clear for {t}") } else { lf!("Також видалити для {t}", "Also delete for {t}") };
            child.checkbox(&mut info.revoke, label);
        }
        // The owner's delete always goes for everyone.
        let revoke = (info.revoke && can_revoke) || (creator && what == Confirm::Delete && matches!(kind, ChatKind::Group | ChatKind::Channel));
        let half = (inner.width() - 8.0) / 2.0;
        let y = inner.bottom() - 34.0;
        let cancel = glass::button(ui, Rect::from_min_size(pos2(inner.left(), y), vec2(half, 34.0)), "info-confirm-cancel", l!("Скасувати", "Cancel"), false, true).clicked();
        let go = glass::button(ui, Rect::from_min_size(pos2(inner.right() - half, y), vec2(half, 34.0)), "info-confirm-go", action, true, true).clicked();
        if go {
            info.confirm = None;
            self.send(match what {
                Confirm::Leave => Cmd::LeaveChat(peer),
                Confirm::Clear => Cmd::ClearHistory { peer, revoke },
                Confirm::Delete => Cmd::DeleteChat { peer, revoke },
                Confirm::DeleteContact => Cmd::DeleteContact(peer),
                Confirm::Block => Cmd::Block { peer, blocked: true },
            });
        } else if cancel {
            info.confirm = None;
        }
    }
}

/// Capsule tabs: members first (groups and channels), then the shared media kinds.
fn info_tabs(ui: &mut Ui, chat: &ChatInfo, info: &InfoPanel, todo: &mut Vec<Do>) {
    let count = |k: SharedKind| info.details.as_ref().and_then(|d| d.counts.iter().find(|c| c.0 == k)).map(|c| c.1);
    ui.horizontal_wrapped(|ui| {
        ui.spacing_mut().item_spacing = vec2(6.0, 6.0);
        ui.add_space(6.0);
        if has_members(chat.kind) && info.details.as_ref().is_none_or(|d| d.can_view_members) {
            let label = match info.details.as_ref().and_then(|d| d.members) {
                Some(n) => lf!("Учасники {n}", "Members {n}"),
                None => l!("Учасники", "Members").to_owned(),
            };
            if chip(ui, &label, info.tab == Tab::Members).clicked() {
                todo.push(Do::Tab(Tab::Members));
            }
        }
        for k in SharedKind::ALL {
            let label = match count(k) {
                Some(n) if n > 0 => format!("{} {n}", k.title()),
                _ => k.title().to_owned(),
            };
            if chip(ui, &label, info.tab == Tab::Shared(k)).clicked() {
                todo.push(Do::Tab(Tab::Shared(k)));
            }
        }
        if let Some((_, total)) = info.gifts.as_ref().filter(|(g, _)| !g.is_empty()) {
            if chip(ui, &lf!("Подарунки {total}", "Gifts {total}"), info.tab == Tab::Gifts).clicked() {
                todo.push(Do::Tab(Tab::Gifts));
            }
        }
        if let Some((_, total)) = info.similar.as_ref().filter(|(c, _)| !c.is_empty()) {
            if chip(ui, &lf!("Схожі канали {total}", "Similar channels {total}"), info.tab == Tab::Similar).clicked() {
                todo.push(Do::Tab(Tab::Similar));
            }
        }
    });
}

/// Leave / clear / delete in the danger colour, as at the bottom of the Telegram Desktop profile.
/// The owner gets "Delete" instead of "Leave", so the group or channel isn't left without one.
/// Private chats also get the contact and block rows, in Telegram Desktop's order;
/// `blocked` is `None` until the profile has loaded.
fn info_actions(ui: &mut Ui, chat: &ChatInfo, creator: bool, blocked: Option<bool>, todo: &mut Vec<Do>) {
    if !chat.joined {
        return;
    }
    let act = |ui: &mut Ui, todo: &mut Vec<Do>, label: &str, c: Confirm| {
        if danger_row(ui, label).clicked() {
            todo.push(Do::Confirm(c));
        }
    };
    match chat.kind {
        ChatKind::User | ChatKind::Bot => {
            if chat.kind == ChatKind::User {
                if chat.contact {
                    act(ui, todo, l!("Видалити контакт", "Delete contact"), Confirm::DeleteContact);
                } else if action_row(ui, l!("Додати в контакти", "Add to contacts"), ACCENT).clicked() {
                    todo.push(Do::AddContact);
                }
            }
            act(ui, todo, l!("Очистити історію", "Clear history"), Confirm::Clear);
            act(ui, todo, l!("Видалити чат", "Delete chat"), Confirm::Delete);
            let bot = chat.kind == ChatKind::Bot;
            match blocked {
                Some(true) => {
                    let label = if bot { l!("Перезапустити бота", "Restart bot") } else { l!("Розблокувати", "Unblock user") };
                    if action_row(ui, label, ACCENT).clicked() {
                        todo.push(Do::Unblock);
                    }
                }
                Some(false) => act(ui, todo, if bot { l!("Зупинити бота", "Stop bot") } else { l!("Заблокувати", "Block user") }, Confirm::Block),
                None => {}
            }
        }
        ChatKind::Saved => act(ui, todo, l!("Очистити історію", "Clear history"), Confirm::Clear),
        ChatKind::Group => {
            act(ui, todo, l!("Очистити історію", "Clear history"), Confirm::Clear);
            if creator {
                act(ui, todo, l!("Видалити групу", "Delete group"), Confirm::Delete);
            } else {
                act(ui, todo, l!("Вийти з групи", "Leave group"), Confirm::Leave);
            }
        }
        ChatKind::Channel if creator => act(ui, todo, l!("Видалити канал", "Delete channel"), Confirm::Delete),
        ChatKind::Channel => act(ui, todo, l!("Вийти з каналу", "Leave channel"), Confirm::Leave),
    }
}

/// Icon, wrapped value and a caption under it; click copies.
fn info_row(ui: &mut Ui, icon: Icon, text: &str, caption: &str) -> egui::Response {
    let w = ui.available_width();
    let x = 52.0;
    let job = LayoutJob::simple(text.to_owned(), FontId::proportional(14.0), ink(), w - x - 12.0);
    let g = ui.painter().layout_job(job);
    let h = g.size().y + 36.0;
    let (rect, resp) = ui.allocate_exact_size(vec2(w, h.max(52.0)), Sense::click());
    let hover = ui.ctx().animate_bool(resp.id, resp.hovered());
    let p = ui.painter();
    if hover > 0.0 {
        p.rect_filled(rect, 16.0, glass::veil((hover * 14.0) as u8));
    }
    icons::draw(p, Rect::from_center_size(pos2(rect.left() + 26.0, rect.top() + 22.0), vec2(20.0, 20.0)), icon, ink_dim());
    p.galley(pos2(rect.left() + x, rect.top() + 9.0), g.clone(), ink());
    p.text(pos2(rect.left() + x, rect.top() + 13.0 + g.size().y), Align2::LEFT_TOP, caption, FontId::proportional(12.5), ink_dim());
    if resp.hovered() {
        ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
    }
    resp
}

/// Option row in the notifications list, with a check mark when active.
fn menu_row(ui: &mut Ui, label: &str, on: bool) -> egui::Response {
    let w = ui.available_width();
    let (rect, resp) = ui.allocate_exact_size(vec2(w, 36.0), Sense::click());
    let hover = ui.ctx().animate_bool(resp.id, resp.hovered());
    let p = ui.painter();
    if hover > 0.0 {
        p.rect_filled(rect.shrink2(vec2(8.0, 0.0)), 12.0, glass::veil((hover * 14.0) as u8));
    }
    p.text(pos2(rect.left() + 52.0, rect.center().y), Align2::LEFT_CENTER, label, FontId::proportional(13.5), ink());
    if on {
        icons::draw(p, Rect::from_center_size(pos2(rect.right() - 26.0, rect.center().y), vec2(16.0, 16.0)), Icon::Check, ACCENT);
    }
    if resp.hovered() {
        ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
    }
    resp
}

fn danger_row(ui: &mut Ui, label: &str) -> egui::Response {
    action_row(ui, label, glass::danger())
}

fn action_row(ui: &mut Ui, label: &str, color: Color32) -> egui::Response {
    let w = ui.available_width();
    let (rect, resp) = ui.allocate_exact_size(vec2(w, 40.0), Sense::click());
    let hover = ui.ctx().animate_bool(resp.id, resp.hovered());
    let p = ui.painter();
    if hover > 0.0 {
        p.rect_filled(rect, 14.0, glass::veil((hover * 14.0) as u8));
    }
    p.text(pos2(rect.left() + 18.0, rect.center().y), Align2::LEFT_CENTER, label, FontId::proportional(14.5), color);
    if resp.hovered() {
        ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
    }
    resp
}

/// Capsule toggle (radius = height / 2).
pub(super) fn chip(ui: &mut Ui, label: &str, selected: bool) -> egui::Response {
    let g = ui.painter().layout_no_wrap(label.to_owned(), FontId::proportional(13.0), ink());
    let (rect, resp) = ui.allocate_exact_size(vec2(g.size().x + 24.0, 32.0), Sense::click());
    let hover = ui.ctx().animate_bool(resp.id, resp.hovered());
    let sel = ui.ctx().animate_bool(resp.id.with("sel"), selected);
    let p = ui.painter();
    if sel > 0.0 {
        glass::glass(p, rect, Glass::tinted(16.0, ACCENT, (sel * 110.0) as u8));
    } else {
        p.rect_filled(rect, 16.0, glass::veil((10.0 + hover * 14.0) as u8));
    }
    p.galley(rect.center() - g.size() / 2.0, g, ink());
    if resp.hovered() {
        ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
    }
    resp
}

fn empty(ui: &mut Ui, text: &str) {
    ui.add_space(16.0);
    ui.vertical_centered(|ui| ui.label(egui::RichText::new(text).color(ink_dim()).size(13.5)));
    ui.add_space(8.0);
}

/// First web link of a message: an entity target, or else a bare `http` word.
fn link_of(m: &Msg) -> Option<String> {
    m.spans
        .iter()
        .filter(|s| s.style == Style::Link)
        .filter_map(|s| s.target.clone())
        .find(|t| t.starts_with("http") || t.starts_with("t.me/") || t.contains('.') && !t.starts_with('@') && !t.starts_with('#'))
        .or_else(|| m.text.split_whitespace().find(|w| w.starts_with("http")).map(str::to_owned))
}

/// List row for files, links and voice messages.
fn shared_row(ui: &mut Ui, m: &Msg, kind: SharedKind) -> egui::Response {
    let w = ui.available_width();
    let (rect, resp) = ui.allocate_exact_size(vec2(w, 56.0), Sense::click());
    let hover = ui.ctx().animate_bool(resp.id, resp.hovered());
    let p = ui.painter();
    if hover > 0.0 {
        p.rect_filled(rect, 16.0, glass::veil((hover * 14.0) as u8));
    }
    let icon = match kind {
        SharedKind::Links => Icon::Link,
        SharedKind::Voice => Icon::Mic,
        _ => Icon::File,
    };
    let c = pos2(rect.left() + 28.0, rect.center().y);
    glass::glass(p, Rect::from_center_size(c, vec2(40.0, 40.0)), Glass::tinted(20.0, ACCENT, 90));
    icons::draw(p, Rect::from_center_size(c, vec2(20.0, 20.0)), icon, ink());
    let when = format!("{} {}", m.day, m.time);
    let (title, sub) = match kind {
        SharedKind::Links => {
            let url = link_of(m).unwrap_or_default();
            let host = url.trim_start_matches("https://").trim_start_matches("http://").split('/').next().unwrap_or_default().to_owned();
            (if host.is_empty() { url.clone() } else { host }, if url.is_empty() { m.text.replace('\n', " ") } else { url })
        }
        SharedKind::Voice => {
            let who = if m.out { l!("Ви", "You").to_owned() } else if m.sender.is_empty() { l!("Голосове", "Voice message").to_owned() } else { m.sender.clone() };
            let dur = m.media.as_ref().map(|i| player::clock(i.duration)).unwrap_or_default();
            (who, format!("{dur} · {when}"))
        }
        _ => match &m.file {
            Some((name, size)) => (name.clone(), format!("{size} · {when}")),
            None => (m.media.as_ref().map(|i| i.title.clone()).filter(|t| !t.is_empty()).unwrap_or_else(|| l!("Файл", "File").to_owned()), when),
        },
    };
    let x = rect.left() + 58.0;
    p.galley(pos2(x, rect.top() + 9.0), elided(ui, &title, 14.0, ink(), rect.right() - x - 10.0), ink());
    p.galley(pos2(x, rect.top() + 30.0), elided(ui, &sub, 12.5, ink_dim(), rect.right() - x - 10.0), ink_dim());
    if resp.hovered() {
        ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
    }
    resp
}
