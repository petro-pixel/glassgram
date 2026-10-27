//! "New chat" (the pencil next to search), as in Telegram Desktop: contacts, a new group
//! (members, then the name and photo), a new channel (name, description and photo, then its
//! type and link, then the first subscribers).

use std::sync::mpsc;

use super::*;
use crate::backend::UsernameState;

/// Most people a new group gets in one request (as in Telegram Desktop; the rest are invited later).
const GROUP_MAX: usize = 199;

/// The "New chat" window: contacts, a new group or a new channel.
pub(super) struct NewChat {
    kind: NewKind,
    query: String,
    edited_at: f64,
    requested: String,
    /// Members of the new group.
    picked: Vec<PeerId>,
    /// Group: members are chosen, now the name.
    named: bool,
    title: String,
    about: String,
    /// The create request is in flight.
    pub(super) busy: bool,
    /// Photo for the new group or channel, its preview, and the file picker's answer.
    photo: Option<PathBuf>,
    photo_tex: Option<egui::TextureHandle>,
    photo_rx: Option<mpsc::Receiver<PathBuf>>,
    /// The channel is created: what is set up next.
    created: Option<(PeerId, After)>,
}

/// The steps after a new channel, as in Telegram Desktop.
enum After {
    /// Public with a `t.me/` link, or private with an invite link.
    Type { public: bool, link: String, edited_at: f64, requested: String, checked: Option<(String, UsernameState)>, invite: Option<String> },
    /// The first subscribers, from contacts.
    Subscribers,
}

impl NewChat {
    fn new(kind: NewKind) -> Self {
        Self {
            kind,
            query: String::new(),
            edited_at: 0.0,
            requested: String::new(),
            picked: Vec::new(),
            named: false,
            title: String::new(),
            about: String::new(),
            busy: false,
            photo: None,
            photo_tex: None,
            photo_rx: None,
            created: None,
        }
    }

    /// Choosing the first subscribers of the new channel: the people list is back.
    fn subscribers(&self) -> bool {
        matches!(self.created, Some((_, After::Subscribers)))
    }
}

/// Channel link rules (as for usernames): 5–32 of a–z, 0–9 and `_`, starting with a letter.
fn link_problem(name: &str) -> Option<&'static str> {
    if name.chars().count() < 5 {
        return Some(l!("Щонайменше 5 символів", "At least 5 characters"));
    }
    if !name.chars().all(|c| c.is_ascii_alphanumeric() || c == '_') || !name.starts_with(|c: char| c.is_ascii_alphabetic()) || name.len() > 32 {
        return Some(l!("Лише латиниця, цифри й _, на початку — літера", "Only a–z, 0–9 and _, starting with a letter"));
    }
    None
}

#[derive(Clone, Copy, PartialEq)]
enum NewKind {
    Contacts,
    Group,
    Channel,
}

impl App {
    /// Pencil button next to search: a menu with "New group", "New channel", "Contacts".
    pub(super) fn new_chat_button(&mut self, ui: &mut Ui, rect: Rect) {
        let open = self.new_chat_menu.is_some();
        // The anchor follows the button when the window is resized.
        if open {
            self.new_chat_menu = Some(rect);
        }
        if glass::icon_button(ui, rect, "new-chat", Icon::Pencil, open, true).on_hover_text(l!("Новий чат", "New chat")).clicked() {
            self.new_chat_menu = if open { None } else { Some(rect) };
        }
    }

    fn new_chat_menu_view(&mut self, ui: &mut Ui) {
        let Some(anchor) = self.new_chat_menu else { return };
        let screen = ui.max_rect();
        if ui.interact(screen, ui.scope_id().with("new-chat-bg"), Sense::click()).clicked() {
            self.new_chat_menu = None;
            return;
        }
        let items = [(NewKind::Group, Icon::Users, l!("Нова група", "New group")), (NewKind::Channel, Icon::Channel, l!("Новий канал", "New channel")), (NewKind::Contacts, Icon::User, l!("Контакти", "Contacts"))];
        let rect = Rect::from_min_size(pos2(anchor.right() - 220.0, anchor.bottom() + 8.0), vec2(220.0, items.len() as f32 * 40.0 + 16.0));
        glass::glass(ui.painter(), rect, Glass::panel(20.0).with_shadow());
        ui.interact(rect, ui.scope_id().with("new-chat-panel"), Sense::click());
        let mut pick = None;
        for (k, (kind, icon, label)) in items.into_iter().enumerate() {
            let r = Rect::from_min_size(pos2(rect.left() + 8.0, rect.top() + 8.0 + k as f32 * 40.0), vec2(rect.width() - 16.0, 40.0));
            let resp = ui.interact(r, ui.scope_id().with(("new-chat-item", k)), Sense::click());
            let p = ui.painter();
            if resp.hovered() {
                p.rect_filled(r, 12.0, glass::veil(16));
                ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
            }
            icons::draw(p, Rect::from_center_size(pos2(r.left() + 20.0, r.center().y), vec2(20.0, 20.0)), icon, ink());
            p.text(pos2(r.left() + 44.0, r.center().y), Align2::LEFT_CENTER, label, FontId::proportional(14.5), ink());
            if resp.clicked() {
                pick = Some(kind);
            }
        }
        if let Some(kind) = pick {
            self.new_chat_menu = None;
            if kind != NewKind::Channel && self.contacts.is_empty() {
                self.send(Cmd::LoadContacts);
            }
            self.new_chat = Some(NewChat::new(kind));
        } else if ui.input(|i| i.key_pressed(Key::Escape)) {
            self.new_chat_menu = None;
        }
    }

    /// "New chat" menu and windows, above everything else.
    pub(super) fn new_chat_overlays(&mut self, ui: &mut Ui) {
        if self.new_chat.is_some() {
            self.new_chat_view(ui);
        }
        if self.new_chat_menu.is_some() {
            self.new_chat_menu_view(ui);
        }
    }

    pub(super) fn new_chat_event(&mut self, ev: Ev) {
        match ev {
            Ev::Contacts(list) => self.contacts = list,
            Ev::ContactResults { query, chats } => self.contact_hits = (query, chats),
            Ev::ChatCreated(info) => {
                let id = info.id;
                let channel = info.kind == ChatKind::Channel;
                if !self.chats.iter().any(|c| c.id == id) {
                    self.chats.insert(0, info);
                    sort_chats(&mut self.chats);
                }
                self.folder = None;
                self.open_chat(id);
                let Some(nc) = &mut self.new_chat else { return };
                if let Some(path) = nc.photo.take() {
                    self.send(Cmd::SetChatPhoto { peer: id, path });
                }
                let Some(nc) = &mut self.new_chat else { return };
                if channel && nc.kind == NewKind::Channel {
                    nc.busy = false;
                    nc.picked.clear();
                    nc.created = Some((id, After::Type { public: false, link: String::new(), edited_at: 0.0, requested: String::new(), checked: None, invite: None }));
                    self.send(Cmd::ExportInvite(id));
                } else {
                    self.new_chat = None;
                }
            }
            Ev::ChannelUsernameCheck { peer, name, state } => {
                if let Some(NewChat { created: Some((p, After::Type { checked, .. })), .. }) = &mut self.new_chat
                    && *p == peer
                {
                    *checked = Some((name, state));
                }
            }
            _ => {}
        }
    }

    /// The private link of the channel just created.
    pub(super) fn new_channel_invite(&mut self, peer: PeerId, link: &str) {
        if let Some(NewChat { created: Some((p, After::Type { invite, .. })), .. }) = &mut self.new_chat
            && *p == peer
        {
            *invite = Some(link.to_owned());
        }
    }

    /// A round photo button for the new group or channel; the picked image shows in it.
    fn new_chat_photo(&mut self, ui: &mut Ui, rect: Rect) {
        let Some(nc) = &mut self.new_chat else { return };
        if let Some(path) = nc.photo_rx.as_ref().and_then(|rx| rx.try_recv().ok()) {
            // A small preview; the full file is uploaded after the chat is created.
            nc.photo_tex = image::open(&path).ok().map(|img| {
                let img = img.thumbnail(160, 160).to_rgba8();
                let color = egui::ColorImage::from_rgba_unmultiplied([img.width() as usize, img.height() as usize], img.as_raw());
                ui.ctx().load_texture("new-chat-photo", color, egui::TextureOptions::LINEAR)
            });
            nc.photo = Some(path);
        }
        let resp = ui.interact(rect, ui.scope_id().with("new-chat-photo"), Sense::click());
        let p = ui.painter();
        match &nc.photo_tex {
            Some(t) => glass::avatar(p, rect.center(), rect.width() / 2.0, "", 0, None, Some(t)),
            None => {
                glass::glass(p, rect, Glass::tinted(rect.width() / 2.0, ACCENT, if resp.hovered() { 200 } else { 160 }));
                icons::draw(p, Rect::from_center_size(rect.center(), vec2(22.0, 22.0)), Icon::Camera, glass::ON_DARK);
            }
        }
        if resp.on_hover_text(l!("Вибрати фото", "Set a photo")).on_hover_cursor(egui::CursorIcon::PointingHand).clicked() {
            let (tx, rx) = mpsc::channel();
            nc.photo_rx = Some(rx);
            let ctx = ui.ctx().clone();
            let title = l!("Фото чату", "Chat photo");
            std::thread::spawn(move || {
                if let Some(path) = rfd::FileDialog::new().set_title(title).add_filter("Images", &["jpg", "jpeg", "png", "webp"]).pick_file() {
                    let _ = tx.send(path);
                }
                ctx.request_repaint();
            });
        }
    }

    /// After a new channel: its type and link, then the first subscribers. `true` closes the window.
    fn new_channel_steps(&mut self, ui: &mut Ui, inner: Rect, buttons: Rect, now: f64) -> bool {
        let Some(NewChat { created: Some((peer, after)), picked, .. }) = &mut self.new_chat else { return false };
        let peer = *peer;
        let w = (buttons.width() - 8.0) / 2.0;
        let left = Rect::from_min_size(buttons.min, vec2(w, buttons.height()));
        let right = Rect::from_min_size(pos2(buttons.right() - w, buttons.top()), vec2(w, buttons.height()));
        let After::Type { public, link, edited_at, requested, checked, invite } = after else {
            // Subscribers: the people list above picks them.
            if glass::button(ui, left, "channel-skip", l!("Пропустити", "Skip"), false, true).clicked() {
                return true;
            }
            if glass::button(ui, right, "channel-add", l!("Додати", "Add"), true, !picked.is_empty()).clicked() {
                let users = std::mem::take(picked);
                self.send(Cmd::InviteToChannel { peer, users });
                return true;
            }
            return false;
        };
        let mut y = inner.top() + 44.0;
        let half = (inner.width() - 8.0) / 2.0;
        for (k, (is_public, label)) in [(true, l!("Публічний", "Public")), (false, l!("Приватний", "Private"))].into_iter().enumerate() {
            let r = Rect::from_min_size(pos2(inner.left() + k as f32 * (half + 8.0), y), vec2(half, 36.0));
            if glass::button(ui, r, ("channel-type", k), label, *public == is_public, true).clicked() {
                *public = is_public;
            }
        }
        y += 48.0;
        let about = if *public {
            l!("Публічний канал можна знайти в пошуку, приєднатися може будь-хто.", "Public channels can be found in search, anyone can join them.")
        } else {
            l!("До приватного каналу можна приєднатися лише за посиланням-запрошенням.", "Private channels can only be joined via an invite link.")
        };
        let g = ui.painter().layout(about.to_owned(), FontId::proportional(12.5), ink_dim(), inner.width() - 8.0);
        let gh = g.size().y;
        ui.painter().galley(pos2(inner.left() + 4.0, y), g, ink_dim());
        y += gh + 14.0;
        let mut next = false;
        if *public {
            let field = Rect::from_min_size(pos2(inner.left(), y), vec2(inner.width(), 40.0));
            if glass::text_field(ui, field, link, "t.me/…", false).changed() {
                *edited_at = now;
            }
            let name = link.trim().trim_start_matches("t.me/").trim_start_matches('@').to_owned();
            let problem = link_problem(&name);
            let state = checked.as_ref().filter(|(n, _)| *n == name).map(|(_, s)| *s);
            let (status, color) = match (problem, state) {
                (Some(why), _) if !name.is_empty() => (why.to_owned(), glass::DANGER),
                (_, _) if name.is_empty() => (String::new(), ink_dim()),
                (None, Some(UsernameState::Free)) => (lf!("t.me/{name} вільне", "t.me/{name} is available"), ACCENT),
                (None, Some(UsernameState::Taken)) => (l!("Це посилання вже зайняте", "This link is already taken").to_owned(), glass::DANGER),
                (None, Some(UsernameState::Invalid)) => (l!("Некоректне посилання", "This link is invalid").to_owned(), glass::DANGER),
                _ => (l!("Перевіряємо…", "Checking…").to_owned(), ink_dim()),
            };
            ui.painter().text(pos2(inner.left() + 4.0, field.bottom() + 8.0), Align2::LEFT_TOP, status, FontId::proportional(12.5), color);
            // Server check after a pause in typing, like the username in "My account".
            if problem.is_none() && name != *requested && now - *edited_at > SEARCH_DELAY {
                *requested = name.clone();
                self.send(Cmd::CheckChannelUsername { peer, name });
            } else if glass::button(ui, right, "channel-next", l!("Далі", "Next"), true, problem.is_none() && state == Some(UsernameState::Free)).clicked() {
                self.send(Cmd::SetChannelUsername { peer, name });
                next = true;
            }
        } else {
            let link = invite.clone();
            let r = Rect::from_min_size(pos2(inner.left(), y), vec2(inner.width(), 40.0));
            glass::glass(ui.painter(), r, Glass::tinted(20.0, glass::chrome(), 110));
            let shown = link.clone().unwrap_or_else(|| l!("Створюємо посилання…", "Creating the link…").to_owned());
            ui.painter().galley(pos2(r.left() + 14.0, r.center().y - 9.0), elided(ui, &shown, 14.0, ink(), r.width() - 28.0), ink());
            let copy = Rect::from_min_size(pos2(inner.left(), r.bottom() + 8.0), vec2(inner.width(), 34.0));
            if glass::button(ui, copy, "channel-copy-link", l!("Копіювати посилання", "Copy link"), false, link.is_some()).clicked()
                && let Some(link) = link
            {
                ui.ctx().copy_text(link);
                self.toast = Some((l!("Посилання скопійовано", "Link copied").into(), 0.0, false));
            }
            next = glass::button(ui, right, "channel-next", l!("Далі", "Next"), true, true).clicked();
        }
        if glass::button(ui, left, "channel-later", l!("Пізніше", "Later"), false, true).clicked() {
            return true;
        }
        if next {
            if self.contacts.is_empty() {
                self.send(Cmd::LoadContacts);
            }
            if let Some(NewChat { created: Some((_, after)), .. }) = &mut self.new_chat {
                *after = After::Subscribers;
            }
        }
        false
    }

    /// Opens a chat with a person from contacts or search.
    fn open_person(&mut self, person: ChatInfo) {
        let id = person.id;
        if !self.chats.iter().any(|c| c.id == id) {
            self.chats.push(person);
        }
        self.new_chat = None;
        self.open_chat(id);
    }

    /// People for the window: matching contacts, plus global results that are not contacts.
    fn people(&self, query: &str) -> (Vec<&ChatInfo>, Vec<&ChatInfo>) {
        let q = query.trim().to_lowercase();
        let mine: Vec<&ChatInfo> = self.contacts.iter().filter(|c| q.is_empty() || c.title.to_lowercase().contains(&q) || c.last_text.to_lowercase().contains(&q)).collect();
        let global = if !q.is_empty() && self.contact_hits.0.to_lowercase() == q {
            self.contact_hits.1.iter().filter(|c| !self.contacts.iter().any(|x| x.id == c.id) && c.kind != ChatKind::Saved).collect()
        } else {
            Vec::new()
        };
        (mine, global)
    }

    /// Search and the list of people: contacts to open, or members and subscribers to pick.
    fn people_picker(&mut self, ui: &mut Ui, inner: Rect, top: f32, buttons: Rect, now: f64) -> (Option<ChatInfo>, Option<PeerId>) {
        let Some(nc) = &mut self.new_chat else { return (None, None) };
        let mut open = None;
        let mut toggle = None;
        let field = Rect::from_min_size(pos2(inner.left(), top), vec2(inner.width(), 38.0));
        let r = glass::search_field(ui, field, &mut nc.query, l!("Пошук за іменем чи @username", "Search by name or @username"));
        if r.changed() {
            nc.edited_at = now;
        }
        if !r.has_focus() && nc.query.is_empty() && !ui.memory(|m| m.focused().is_some()) {
            r.request_focus();
        }
        let q = nc.query.trim().to_owned();
        let search = (q.chars().count() >= 2 && q != nc.requested && now - nc.edited_at > SEARCH_DELAY).then(|| q.clone());
        if let Some(q) = &search {
            nc.requested = q.clone();
        }
        // Groups have Cancel / Next under the list, contacts the "New contact" button.
        let bottom = buttons.top() - 10.0;
        let body = Rect::from_min_max(pos2(inner.left() - 6.0, field.bottom() + 10.0), pos2(inner.right() + 6.0, bottom));
        let mut child = ui.new_child(UiBuilder::new().max_rect(body));
        child.set_clip_rect(body);
        let picked = nc.picked.clone();
        let group = nc.kind == NewKind::Group || nc.subscribers();
        let query = nc.query.clone();
        let (mine, global) = self.people(&query);
        egui::ScrollArea::vertical().id_salt("new-chat-people").auto_shrink(false).show(&mut child, |ui| {
            ui.spacing_mut().item_spacing.y = 2.0;
            if mine.is_empty() && global.is_empty() {
                ui.add_space(24.0);
                ui.vertical_centered(|ui| {
                    if self.contacts.is_empty() && query.trim().is_empty() {
                        ui.add(egui::Spinner::new().color(ink_dim()));
                    } else {
                        ui.label(egui::RichText::new(l!("Нікого не знайдено", "No one found")).color(ink_dim()));
                    }
                });
            }
            let mut row = |ui: &mut Ui, c: &ChatInfo| {
                let mark = group.then(|| picked.contains(&c.id));
                if person_row(ui, c, self.textures.get(&ImgKey::Avatar(c.id)), mark).clicked() {
                    if group {
                        toggle = Some(c.id);
                    } else {
                        open = Some(c.clone());
                    }
                }
            };
            for c in &mine {
                row(ui, c);
            }
            if !global.is_empty() {
                section_title(ui, l!("Глобальний пошук", "Global search"));
                for c in &global {
                    row(ui, c);
                }
            }
        });
        if let Some(q) = search {
            self.send(Cmd::SearchContacts(q));
        }
        (open, toggle)
    }

    fn new_chat_view(&mut self, ui: &mut Ui) {
        let Some(nc) = &mut self.new_chat else { return };
        let now = ui.input(|i| i.time);
        let screen = ui.max_rect();
        ui.painter().rect_filled(screen, 0.0, Color32::from_black_alpha(90));
        ui.interact(screen, ui.scope_id().with("new-chat-modal-bg"), Sense::click());
        let tall = nc.kind != NewKind::Channel || nc.subscribers();
        let short = if nc.created.is_some() { 330.0 } else { 300.0 };
        let rect = Rect::from_center_size(screen.center(), vec2(screen.width().min(440.0) - 20.0, if tall { (screen.height() - 60.0).min(620.0) } else { short }));
        glass::glass(ui.painter(), rect, Glass::panel(26.0).with_shadow());
        let inner = rect.shrink(18.0);
        let title = match (nc.kind, nc.named) {
            _ if nc.subscribers() => l!("Додати підписників", "Add subscribers").to_owned(),
            _ if nc.created.is_some() => l!("Тип каналу", "Channel type").to_owned(),
            (NewKind::Contacts, _) => l!("Контакти", "Contacts").to_owned(),
            (NewKind::Group, false) => l!("Нова група", "New group").to_owned(),
            (NewKind::Group, true) => l!("Назва групи", "Group name").to_owned(),
            (NewKind::Channel, _) => l!("Новий канал", "New channel").to_owned(),
        };
        ui.painter().text(inner.left_top() + vec2(4.0, 2.0), Align2::LEFT_TOP, &title, FontId::proportional(17.0), ink());
        if nc.kind == NewKind::Group && !nc.named {
            let n = nc.picked.len();
            let sub = if n == 0 { l!("Виберіть учасників", "Select members").to_owned() } else { lf!("Вибрано: {n} з {GROUP_MAX}", "Selected: {n} of {GROUP_MAX}") };
            ui.painter().text(inner.left_top() + vec2(4.0, 24.0), Align2::LEFT_TOP, sub, FontId::proportional(12.5), ink_dim());
        }
        let close = Rect::from_min_size(pos2(inner.right() - 32.0, inner.top() - 4.0), vec2(32.0, 32.0));
        let mut done = glass::icon_button(ui, close, "new-chat-close", Icon::Close, false, true).clicked();
        let buttons = Rect::from_min_max(pos2(inner.left(), inner.bottom() - 38.0), inner.max);
        let top = inner.top() + 48.0;

        // Contacts and the first group step show search and a list of people.
        let picking = nc.kind == NewKind::Contacts || (nc.kind == NewKind::Group && !nc.named) || nc.subscribers();
        let (open, toggle) = if picking { self.people_picker(ui, inner, top, buttons, now) } else { (None, None) };
        let Some(nc) = &mut self.new_chat else { return };
        if let Some(id) = toggle {
            if let Some(i) = nc.picked.iter().position(|p| *p == id) {
                nc.picked.remove(i);
            } else if nc.picked.len() < GROUP_MAX {
                nc.picked.push(id);
            }
        }

        let mut cmd = None;
        if nc.created.is_some() {
            if self.new_channel_steps(ui, inner, buttons, now) || done || ui.input(|i| i.key_pressed(Key::Escape)) {
                self.new_chat = None;
            }
            return;
        }
        let photo = Rect::from_min_size(pos2(inner.left(), top - 8.0), vec2(56.0, 56.0));
        match (nc.kind, nc.named) {
            (NewKind::Contacts, _) => {
                if glass::button(ui, buttons, "new-contact", l!("Новий контакт", "New contact"), false, true).clicked() {
                    self.new_chat = None;
                    self.open_contact_form(None);
                    return;
                }
            }
            (NewKind::Group, false) => {
                let w = (buttons.width() - 8.0) / 2.0;
                if glass::button(ui, Rect::from_min_size(buttons.min, vec2(w, buttons.height())), "group-cancel", l!("Скасувати", "Cancel"), false, true).clicked() {
                    done = true;
                }
                let next = Rect::from_min_size(pos2(buttons.right() - w, buttons.top()), vec2(w, buttons.height()));
                if glass::button(ui, next, "group-next", l!("Далі", "Next"), true, !nc.picked.is_empty()).clicked() {
                    nc.named = true;
                }
            }
            (NewKind::Group, true) => {
                let field = Rect::from_min_max(pos2(photo.right() + 12.0, top), pos2(inner.right(), top + 40.0));
                let r = glass::text_field(ui, field, &mut nc.title, l!("Назва групи", "Group name"), false);
                if nc.title.is_empty() && !r.has_focus() {
                    r.request_focus();
                }
                let n = nc.picked.len();
                let p = ui.painter();
                p.text(pos2(inner.left() + 4.0, photo.bottom() + 12.0), Align2::LEFT_TOP, lf!("Учасників: {n}", "Members: {n}"), FontId::proportional(12.5), ink_dim());
                let mut y = photo.bottom() + 36.0;
                for id in &nc.picked {
                    if y > buttons.top() - 44.0 {
                        break;
                    }
                    let c = self.contacts.iter().chain(&self.contact_hits.1).find(|c| c.id == *id);
                    let name = c.map(|c| c.title.as_str()).unwrap_or("…");
                    let seed = id.bot_api_dialog_id_unchecked();
                    glass::avatar(p, pos2(inner.left() + 18.0, y + 18.0), 16.0, name, seed, None, self.textures.get(&ImgKey::Avatar(*id)));
                    p.galley(pos2(inner.left() + 44.0, y + 9.0), elided(ui, name, 14.5, ink(), inner.width() - 50.0), ink());
                    y += 40.0;
                }
                let w = (buttons.width() - 8.0) / 2.0;
                if glass::button(ui, Rect::from_min_size(buttons.min, vec2(w, buttons.height())), "group-back", l!("Назад", "Back"), false, !nc.busy).clicked() {
                    nc.named = false;
                }
                let ok = !nc.title.trim().is_empty() && !nc.busy;
                let create = Rect::from_min_size(pos2(buttons.right() - w, buttons.top()), vec2(w, buttons.height()));
                let enter = r.lost_focus() && ui.input(|i| i.key_pressed(Key::Enter));
                if (glass::button(ui, create, "group-create", l!("Створити", "Create"), true, ok).clicked() || enter) && ok {
                    nc.busy = true;
                    cmd = Some(Cmd::CreateGroup { title: nc.title.trim().to_owned(), users: nc.picked.clone() });
                }
            }
            (NewKind::Channel, _) => {
                let field = Rect::from_min_max(pos2(photo.right() + 12.0, top), pos2(inner.right(), top + 40.0));
                let r = glass::text_field(ui, field, &mut nc.title, l!("Назва каналу", "Channel name"), false);
                if nc.title.is_empty() && !r.has_focus() && !ui.memory(|m| m.focused().is_some()) {
                    r.request_focus();
                }
                let about = Rect::from_min_size(pos2(inner.left(), photo.bottom() + 12.0), vec2(inner.width(), 40.0));
                glass::text_field(ui, about, &mut nc.about, l!("Опис (необов'язково)", "Description (optional)"), false);
                ui.painter().text(
                    pos2(inner.left() + 4.0, about.bottom() + 12.0),
                    Align2::LEFT_TOP,
                    l!("Канал — щоб розповідати новини необмеженій аудиторії.", "Channels are a tool for broadcasting your messages to unlimited audiences."),
                    FontId::proportional(12.5),
                    ink_dim(),
                );
                let ok = !nc.title.trim().is_empty() && !nc.busy;
                if glass::button(ui, buttons, "channel-create", l!("Створити канал", "Create channel"), true, ok).clicked() && ok {
                    nc.busy = true;
                    cmd = Some(Cmd::CreateChannel { title: nc.title.trim().to_owned(), about: nc.about.trim().to_owned() });
                }
            }
        }
        let naming = nc.kind == NewKind::Channel || (nc.kind == NewKind::Group && nc.named);
        if naming {
            self.new_chat_photo(ui, photo);
        }
        if let Some(cmd) = cmd {
            self.send(cmd);
        }
        if let Some(person) = open {
            self.open_person(person);
            return;
        }
        if done || ui.input(|i| i.key_pressed(Key::Escape)) {
            self.new_chat = None;
        }
    }
}

/// A person in the "New chat" list: avatar, name, status; a check mark when picking group members.
fn person_row(ui: &mut Ui, c: &ChatInfo, tex: Option<&egui::TextureHandle>, mark: Option<bool>) -> egui::Response {
    let (rect, resp) = ui.allocate_exact_size(vec2(ui.available_width(), 52.0), Sense::click());
    let hover = ui.ctx().animate_bool(resp.id, resp.hovered());
    let p = ui.painter();
    if hover > 0.0 {
        p.rect_filled(rect, 16.0, glass::veil((hover * 14.0) as u8));
    }
    let center = pos2(rect.left() + 30.0, rect.center().y);
    glass::avatar(p, center, 20.0, &c.title, c.id.bot_api_dialog_id_unchecked(), None, tex);
    let x = rect.left() + 60.0;
    let right = if mark.is_some() { 40.0 } else { 12.0 };
    p.galley(pos2(x, rect.top() + 8.0), elided(ui, &c.title, 14.5, ink(), rect.right() - x - right), ink());
    let status = c.presence.text().unwrap_or_else(|| c.last_text.clone());
    let color = if c.presence.online() { ACCENT } else { ink_dim() };
    p.galley(pos2(x, rect.top() + 28.0), elided(ui, &status, 12.5, color, rect.right() - x - right), color);
    if let Some(on) = mark {
        let r = Rect::from_center_size(pos2(rect.right() - 22.0, rect.center().y), vec2(22.0, 22.0));
        if on {
            p.circle_filled(r.center(), 11.0, ACCENT);
            icons::draw(p, r.shrink(4.0), Icon::Check, glass::ON_DARK);
        } else {
            p.circle_stroke(r.center(), 10.0, egui::Stroke::new(1.5, ink_dim()));
        }
    }
    if resp.hovered() {
        ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
    }
    resp
}
