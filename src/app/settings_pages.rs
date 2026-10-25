//! Settings structured as in Telegram Desktop: a main page with the own profile and a list of
//! sections, each section is a page with a back button.

use std::path::PathBuf;

use super::*;
use crate::backend::{Profile, UsernameState};

/// Bio limit without Premium.
const BIO_MAX: usize = 70;
const NAME_MAX: usize = 64;

/// Fields of the "My account" page; filled from `App::profile` when the page opens.
pub(super) struct ProfileEdit {
    first: String,
    last: String,
    bio: String,
    username: String,
    edited_at: f64,
    /// Username already sent to `account.checkUsername`.
    requested: String,
    /// The answer and for which name.
    checked: Option<(String, UsernameState)>,
}

impl ProfileEdit {
    fn from(p: &Profile) -> Self {
        Self {
            first: p.first.clone(),
            last: p.last.clone(),
            bio: p.bio.clone(),
            username: p.username.clone(),
            edited_at: 0.0,
            requested: String::new(),
            checked: None,
        }
    }
}

/// Local username rules (as on the server), so obvious mistakes don't need a request.
fn username_problem(name: &str) -> Option<&'static str> {
    if name.is_empty() {
        return None;
    }
    if !name.chars().all(|c| c.is_ascii_alphanumeric() || c == '_') {
        return Some(l!("Лише латинські літери, цифри й підкреслення", "Only Latin letters, digits and underscores"));
    }
    if !name.starts_with(|c: char| c.is_ascii_alphabetic()) {
        return Some(l!("Має починатися з літери", "Must start with a letter"));
    }
    if name.ends_with('_') {
        return Some(l!("Не може закінчуватися підкресленням", "Can't end with an underscore"));
    }
    match name.len() {
        0..5 => Some(l!("Щонайменше 5 символів", "At least 5 characters")),
        33.. => Some(l!("Не більше 32 символів", "At most 32 characters")),
        _ => None,
    }
}

impl SettingsTab {
    fn title(self) -> &'static str {
        match self {
            SettingsTab::Main => l!("Налаштування", "Settings"),
            SettingsTab::Account => l!("Мій акаунт", "My Account"),
            SettingsTab::Notifications => l!("Сповіщення", "Notifications"),
            SettingsTab::Privacy => l!("Конфіденційність", "Privacy and Security"),
            SettingsTab::Chats => l!("Налаштування чатів", "Chat Settings"),
            SettingsTab::Folders => l!("Папки", "Folders"),
            SettingsTab::Advanced => l!("Додатково", "Advanced"),
            SettingsTab::Devices => l!("Пристрої", "Devices"),
            SettingsTab::Language => "Мова · Language",
        }
    }

    fn icon(self) -> Icon {
        match self {
            SettingsTab::Main => Icon::Gear,
            SettingsTab::Account => Icon::User,
            SettingsTab::Notifications => Icon::Bell,
            SettingsTab::Privacy => Icon::Lock,
            SettingsTab::Chats => Icon::Chat,
            SettingsTab::Folders => Icon::Folder,
            SettingsTab::Advanced => Icon::Gear,
            SettingsTab::Devices => Icon::Laptop,
            SettingsTab::Language => Icon::Globe,
        }
    }
}

/// Section order on the main page, as in Telegram Desktop.
const SECTIONS: [SettingsTab; 8] = [
    SettingsTab::Account,
    SettingsTab::Notifications,
    SettingsTab::Privacy,
    SettingsTab::Chats,
    SettingsTab::Folders,
    SettingsTab::Advanced,
    SettingsTab::Devices,
    SettingsTab::Language,
];

/// Row of the section list: icon, title and a chevron.
fn nav_row(ui: &mut Ui, tab: SettingsTab, value: &str) -> egui::Response {
    let (r, resp) = ui.allocate_exact_size(vec2(ui.available_width(), 44.0), Sense::click());
    let p = ui.painter();
    if resp.hovered() {
        p.rect_filled(r, 14.0, glass::veil(16));
        ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
    }
    icons::draw(p, Rect::from_center_size(pos2(r.left() + 22.0, r.center().y), vec2(22.0, 22.0)), tab.icon(), ink_dim());
    p.text(pos2(r.left() + 48.0, r.center().y), Align2::LEFT_CENTER, tab.title(), FontId::proportional(14.5), ink());
    icons::draw(p, Rect::from_center_size(pos2(r.right() - 14.0, r.center().y), vec2(16.0, 16.0)), Icon::ChevronRight, ink_dim());
    if !value.is_empty() {
        p.text(pos2(r.right() - 30.0, r.center().y), Align2::RIGHT_CENTER, value, FontId::proportional(13.0), ink_dim());
    }
    resp
}

/// Pick-one row of capsule buttons.
fn choice<T: PartialEq + Copy>(ui: &mut Ui, id: &str, value: &mut T, options: &[(T, &str)], width: f32) {
    ui.horizontal_wrapped(|ui| {
        ui.spacing_mut().item_spacing = vec2(6.0, 6.0);
        for (k, (v, label)) in options.iter().enumerate() {
            let (r, _) = ui.allocate_exact_size(vec2(width, 32.0), Sense::hover());
            if glass::button(ui, r, (id, k), label, *value == *v, true).clicked() {
                *value = *v;
            }
        }
    });
}

fn hint(ui: &mut Ui, text: &str) {
    ui.label(egui::RichText::new(text).color(ink_dim()).size(12.5));
}

fn field(ui: &mut Ui, text: &mut String, placeholder: &str) -> egui::Response {
    let r = ui.allocate_exact_size(vec2(ui.available_width(), 36.0), Sense::hover()).0;
    glass::text_field(ui, r, text, placeholder, false)
}

impl App {
    /// Switches the settings page and asks the backend for what the page shows.
    pub(super) fn open_settings_page(&mut self, tab: SettingsTab) {
        self.settings_tab = tab;
        match tab {
            SettingsTab::Privacy => self.send(Cmd::LoadPrivacy),
            SettingsTab::Devices => self.send(Cmd::LoadSessions),
            SettingsTab::Notifications => self.load_notify_exceptions(),
            SettingsTab::Account => {
                self.profile_edit = Some(ProfileEdit::from(&self.profile));
                self.send(Cmd::LoadProfile);
            }
            _ => {}
        }
    }

    pub(super) fn settings_page(&mut self, ui: &mut Ui, tab: SettingsTab) {
        match tab {
            SettingsTab::Main => self.settings_main(ui),
            SettingsTab::Account => self.account_page(ui),
            SettingsTab::Notifications => self.notifications_page(ui),
            SettingsTab::Privacy => self.privacy_tab(ui),
            SettingsTab::Chats => self.chats_page(ui),
            SettingsTab::Folders => self.folders_page(ui),
            SettingsTab::Advanced => self.advanced_page(ui),
            SettingsTab::Devices => self.devices_tab(ui),
            SettingsTab::Language => self.language_page(ui),
        }
    }

    pub fn demo_profile(&self) -> Profile {
        self.profile.clone()
    }

    pub(super) fn settings_title(&self) -> &'static str {
        self.settings_tab.title()
    }

    fn profile_name(&self) -> String {
        let full = format!("{} {}", self.profile.first, self.profile.last).trim().to_owned();
        if full.is_empty() { self.me.clone() } else { full }
    }

    /// Main page: profile card, sections, interface scale (as in Telegram Desktop).
    fn settings_main(&mut self, ui: &mut Ui) {
        let mut open = None;
        let name = self.profile_name();
        let lang = self.settings.lang.name();
        let devices = if self.sessions.is_empty() { String::new() } else { self.sessions.len().to_string() };
        let avatar = self.profile.id.and_then(|id| self.textures.get(&ImgKey::Avatar(id)));
        let s = &mut self.settings;
        egui::ScrollArea::vertical().id_salt("settings-main").auto_shrink(false).show(ui, |ui| {
            let (card, resp) = ui.allocate_exact_size(vec2(ui.available_width(), 76.0), Sense::click());
            let p = ui.painter();
            if resp.hovered() {
                p.rect_filled(card, 18.0, glass::veil(16));
                ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
            }
            let seed = self.profile.id.map_or(7, |id| id.bot_api_dialog_id_unchecked());
            glass::avatar(p, pos2(card.left() + 38.0, card.center().y), 28.0, &name, seed, None, avatar);
            let x = card.left() + 80.0;
            p.galley(pos2(x, card.top() + 14.0), elided(ui, &name, 17.0, ink(), card.width() - 100.0), ink());
            let mut sub = self.profile.phone.clone();
            if !self.profile.username.is_empty() {
                if !sub.is_empty() {
                    sub.push_str(" · ");
                }
                sub.push('@');
                sub.push_str(&self.profile.username);
            }
            p.galley(pos2(x, card.top() + 42.0), elided(ui, &sub, 13.0, ink_dim(), card.width() - 100.0), ink());
            if resp.clicked() {
                open = Some(SettingsTab::Account);
            }
            ui.add_space(8.0);
            for tab in SECTIONS {
                let value = match tab {
                    SettingsTab::Language => lang.as_str(),
                    SettingsTab::Devices => devices.as_str(),
                    _ => "",
                };
                if nav_row(ui, tab, value).clicked() {
                    open = Some(tab);
                }
            }
            ui.add_space(10.0);
            section_title(ui, l!("Масштаб інтерфейсу", "Default interface scale"));
            let scales: Vec<(f32, String)> = settings::SCALES.iter().map(|k| (*k, format!("{:.0}%", k * 100.0))).collect();
            let options: Vec<(f32, &str)> = scales.iter().map(|(k, l)| (*k, l.as_str())).collect();
            choice(ui, "scale", &mut s.scale, &options, 58.0);
        });
        if let Some(tab) = open {
            self.open_settings_page(tab);
        }
    }

    pub(super) fn on_profile(&mut self, p: Profile) {
        // Fields the user hasn't touched follow the server.
        if let Some(e) = &mut self.profile_edit {
            let old = &self.profile;
            for (field, was, now) in [(&mut e.first, &old.first, &p.first), (&mut e.last, &old.last, &p.last), (&mut e.bio, &old.bio, &p.bio), (&mut e.username, &old.username, &p.username)] {
                if field == was {
                    *field = now.clone();
                }
            }
        }
        let name = format!("{} {}", p.first, p.last).trim().to_owned();
        if !name.is_empty() && name != self.me {
            self.me = name.clone();
            self.accounts.set_name(self.accounts.active, &name);
            self.accounts.save();
        }
        self.profile = p;
    }

    pub(super) fn on_username_check(&mut self, name: String, state: UsernameState) {
        if let Some(e) = &mut self.profile_edit {
            e.checked = Some((name, state));
        }
    }

    fn pick_profile_photo(&self) {
        let (tx, ctx) = (self.photo_tx.clone(), self.ctx.clone());
        let title = l!("Фото профілю", "Profile photo");
        std::thread::spawn(move || {
            let file = rfd::FileDialog::new().set_title(title).add_filter("Images", &["jpg", "jpeg", "png", "webp"]).pick_file();
            if let Some(path) = file {
                let _ = tx.send(path);
            }
            ctx.request_repaint();
        });
    }

    pub(super) fn take_profile_photos(&mut self) {
        let picked: Vec<PathBuf> = self.photo_rx.try_iter().collect();
        for path in picked {
            self.send(Cmd::SetProfilePhoto(path));
        }
    }

    /// "My account": photo, name, bio, username, phone.
    fn account_page(&mut self, ui: &mut Ui) {
        let now = ui.input(|i| i.time);
        let Some(e) = &mut self.profile_edit else { return };
        let p = &self.profile;
        let avatar = p.id.and_then(|id| self.textures.get(&ImgKey::Avatar(id)));
        let mut pick_photo = false;
        let mut save = None;
        let mut save_username = None;
        egui::ScrollArea::vertical().id_salt("settings-account").auto_shrink(false).show(ui, |ui| {
            let (row, _) = ui.allocate_exact_size(vec2(ui.available_width(), 96.0), Sense::hover());
            let name = format!("{} {}", e.first, e.last);
            let seed = p.id.map_or(7, |id| id.bot_api_dialog_id_unchecked());
            glass::avatar(ui.painter(), pos2(row.left() + 48.0, row.center().y), 40.0, name.trim(), seed, None, avatar);
            let b = Rect::from_min_size(pos2(row.left() + 108.0, row.center().y - 18.0), vec2(170.0, 36.0));
            pick_photo = glass::button(ui, b, "profile-photo", l!("Змінити фото", "Set photo"), false, true).clicked();

            section_title(ui, l!("Ім'я", "Name"));
            field(ui, &mut e.first, l!("Ім'я (обов'язково)", "First name (required)"));
            ui.add_space(4.0);
            field(ui, &mut e.last, l!("Прізвище (необов'язково)", "Last name (optional)"));
            for f in [&mut e.first, &mut e.last] {
                if f.chars().count() > NAME_MAX {
                    *f = f.chars().take(NAME_MAX).collect();
                }
            }
            section_title(ui, l!("Біо", "Bio"));
            field(ui, &mut e.bio, l!("Кілька слів про себе", "A few words about you"));
            let left = BIO_MAX as i64 - e.bio.chars().count() as i64;
            if left < 0 {
                ui.label(egui::RichText::new(lf!("Задовге на {}", "Too long by {}", -left)).color(glass::DANGER).size(12.5));
            } else {
                hint(ui, &lf!("Будь-які деталі, наприклад вік, рід занять чи місто. Лишилось: {left}", "Any details such as age, occupation or city. Left: {left}"));
            }
            ui.add_space(8.0);
            let changed = (e.first.trim(), e.last.trim(), e.bio.trim()) != (p.first.as_str(), p.last.as_str(), p.bio.as_str());
            let ok = changed && !e.first.trim().is_empty() && left >= 0;
            let r = ui.allocate_exact_size(vec2(ui.available_width(), 36.0), Sense::hover()).0;
            if glass::button(ui, r, "profile-save", l!("Зберегти", "Save"), true, ok).clicked() && ok {
                save = Some((e.first.trim().to_owned(), e.last.trim().to_owned(), e.bio.trim().to_owned()));
            }

            section_title(ui, l!("Ім'я користувача", "Username"));
            if field(ui, &mut e.username, "@username").changed() {
                e.edited_at = now;
            }
            let name = e.username.trim().trim_start_matches('@').to_owned();
            let problem = username_problem(&name);
            let same = name.eq_ignore_ascii_case(&p.username);
            let state = e.checked.as_ref().filter(|(n, _)| *n == name).map(|(_, s)| *s);
            let (status, color) = match (problem, state) {
                _ if same => (String::new(), ink_dim()),
                (Some(why), _) => (why.to_owned(), glass::DANGER),
                (None, _) if name.is_empty() => (l!("Ім'я користувача буде прибрано", "The username will be removed").to_owned(), ink_dim()),
                (None, Some(UsernameState::Free)) => (lf!("@{name} вільне", "@{name} is available"), ACCENT),
                (None, Some(UsernameState::Taken)) => (l!("Це ім'я вже зайняте", "This username is already taken").to_owned(), glass::DANGER),
                (None, Some(UsernameState::Invalid)) => (l!("Некоректне ім'я користувача", "This username is invalid").to_owned(), glass::DANGER),
                (None, None) => (l!("Перевіряємо…", "Checking…").to_owned(), ink_dim()),
            };
            if !status.is_empty() {
                ui.label(egui::RichText::new(status).color(color).size(12.5));
            }
            hint(ui, &lf!(
                "Інші знайдуть вас за цим іменем без номера телефону: t.me/{name}",
                "People can find you by this name without your phone number: t.me/{name}"
            ));
            // Server check after a pause in typing, like the search.
            if !same && problem.is_none() && !name.is_empty() && name != e.requested && now - e.edited_at > SEARCH_DELAY {
                e.requested = name.clone();
                save_username = Some(Err(name.clone()));
            }
            ui.add_space(8.0);
            let can = !same && problem.is_none() && (name.is_empty() || state == Some(UsernameState::Free));
            let r = ui.allocate_exact_size(vec2(ui.available_width(), 36.0), Sense::hover()).0;
            if glass::button(ui, r, "username-save", l!("Зберегти ім'я користувача", "Save username"), true, can).clicked() && can {
                save_username = Some(Ok(name));
            }

            if !p.phone.is_empty() {
                section_title(ui, l!("Телефон", "Phone number"));
                ui.label(egui::RichText::new(&p.phone).color(ink()).size(14.5));
                hint(ui, l!("Номер змінюється в мобільному застосунку Telegram", "The number is changed in the Telegram mobile app"));
            }
        });
        if pick_photo {
            self.pick_profile_photo();
        }
        if let Some((first, last, bio)) = save {
            self.send(Cmd::SaveProfile { first, last, bio });
        }
        match save_username {
            Some(Ok(name)) => self.send(Cmd::SetUsername(name)),
            Some(Err(name)) => self.send(Cmd::CheckUsername(name)),
            None => {}
        }
    }

    fn notifications_page(&mut self, ui: &mut Ui) {
        egui::ScrollArea::vertical().id_salt("settings-notify").auto_shrink(false).show(ui, |ui| {
            section_title(ui, l!("Сповіщення на робочому столі", "Desktop notifications"));
            ui.checkbox(&mut self.settings.notifications, l!("Системні сповіщення про нові повідомлення", "System notifications for new messages"));
            hint(ui, l!("Те саме перемикає дзвіночок над списком чатів і меню в треї.", "The bell above the chat list and the tray menu toggle the same."));
            hint(ui, l!("Чати без звуку сповіщень не показують; перемикач — у меню чату.", "Muted chats don't notify; the toggle is in the chat menu."));
            ui.add_space(10.0);
            self.notify_scopes(ui);
        });
    }

    fn chats_page(&mut self, ui: &mut Ui) {
        let s = &mut self.settings;
        egui::ScrollArea::vertical().id_salt("settings-chats").auto_shrink(false).show(ui, |ui| {
            section_title(ui, l!("Тема", "Theme"));
            let themes = [(settings::Theme::System, l!("Як у системі", "System")), (settings::Theme::Dark, l!("Темна", "Dark")), (settings::Theme::Light, l!("Світла", "Light"))];
            choice(ui, "theme", &mut s.theme, &themes, 122.0);
            ui.add_space(10.0);
            section_title(ui, l!("Медіа", "Media"));
            ui.checkbox(&mut s.autoplay, l!("Автовідтворення GIF, кружечків і відео-стікерів", "Autoplay GIFs, video messages and video stickers"));
        });
    }

    fn folders_page(&mut self, ui: &mut Ui) {
        let mut edit = None;
        let mut delete = None;
        let mut new = false;
        egui::ScrollArea::vertical().id_salt("settings-folders").auto_shrink(false).show(ui, |ui| {
            hint(ui, l!("Папки впорядковують чати. Ctrl+1…9 — перехід.", "Folders sort your chats. Ctrl+1…9 to switch."));
            section_title(ui, l!("Вигляд папок", "Folders view"));
            ui.horizontal(|ui| {
                ui.spacing_mut().item_spacing.x = 6.0;
                for (side, label) in [(false, l!("Вкладки зверху", "Tabs on top")), (true, l!("Колонка зліва", "Column on the left"))] {
                    let (r, _) = ui.allocate_exact_size(vec2(160.0, 32.0), Sense::hover());
                    if glass::button(ui, r, ("folders-side", side), label, self.settings.folders_side == side, true).clicked() {
                        self.settings.folders_side = side;
                    }
                }
            });
            ui.add_space(10.0);
            section_title(ui, l!("Мої папки", "My folders"));
            if self.folders.is_empty() {
                hint(ui, l!("Папок ще немає", "No folders yet"));
            }
            for f in &self.folders {
                let (row, _) = ui.allocate_exact_size(vec2(ui.available_width(), 44.0), Sense::hover());
                let p = ui.painter();
                p.rect_filled(row, 14.0, glass::veil(10));
                icons::draw(p, Rect::from_center_size(pos2(row.left() + 22.0, row.center().y), vec2(20.0, 20.0)), Icon::Folder, ink_dim());
                let title = if f.emoticon.is_empty() { f.title.clone() } else { format!("{} {}", f.emoticon, f.title) };
                p.galley(pos2(row.left() + 48.0, row.center().y - 9.0), elided(ui, &title, 14.5, ink(), row.width() - 140.0), ink());
                let b = |k: f32| Rect::from_center_size(pos2(row.right() - 22.0 - k * 40.0, row.center().y), vec2(36.0, 36.0));
                if glass::icon_button(ui, b(1.0), ("folder-edit", f.id), Icon::Pencil, false, true).on_hover_text(l!("Редагувати", "Edit")).clicked() {
                    edit = Some(f.id);
                }
                if glass::icon_button(ui, b(0.0), ("folder-del", f.id), Icon::Trash, false, true).on_hover_text(l!("Видалити", "Delete")).clicked() {
                    delete = Some(f.id);
                }
                ui.add_space(4.0);
            }
            ui.add_space(6.0);
            let r = ui.allocate_exact_size(vec2(ui.available_width(), 36.0), Sense::hover()).0;
            new = glass::button(ui, r, "folder-create", l!("Створити папку", "Create new folder"), false, true).clicked();
        });
        if new {
            self.new_folder();
        }
        if let Some(f) = edit.and_then(|id| self.folders.iter().find(|f| f.id == id)) {
            self.folder_edit = Some(FolderEdit { folder: f.clone(), new: false, query: String::new() });
        }
        if let Some(id) = delete {
            self.send(Cmd::DeleteFolder(id));
        }
    }

    /// "Advanced": connection (proxy), camera and media tools.
    fn advanced_page(&mut self, ui: &mut Ui) {
        let cameras = self.cameras.get_or_insert_with(camera::list).clone();
        let (can_play, can_record) = (self.can_play, self.can_record);
        let s = &mut self.settings;
        egui::ScrollArea::vertical().id_salt("settings-advanced").auto_shrink(false).show(ui, |ui| {
            section_title(ui, l!("Проксі SOCKS5", "SOCKS5 proxy"));
            let r = ui.allocate_exact_size(vec2(ui.available_width(), 36.0), Sense::hover()).0;
            glass::text_field(ui, r, &mut s.proxy, "socks5://127.0.0.1:1080", false);
            match settings::proxy_error(&s.proxy) {
                Some(e) => ui.label(egui::RichText::new(e).color(glass::DANGER).size(12.5)),
                None => ui.label(egui::RichText::new(l!("Діє після перезапуску програми", "Takes effect after restart")).color(ink_dim()).size(12.5)),
            };
            ui.add_space(10.0);
            section_title(ui, l!("Камера для відеоповідомлень", "Camera for video messages"));
            if cameras.is_empty() {
                hint(ui, l!("Камер не знайдено", "No cameras found"));
            }
            for (k, c) in cameras.iter().enumerate() {
                // Without an explicit choice the first camera records.
                let on = s.camera == c.id || (s.camera.is_empty() && k == 0);
                let label = if c.name.is_empty() { c.id.clone() } else { c.name.clone() };
                let r = ui.allocate_exact_size(vec2(ui.available_width(), 32.0), Sense::hover()).0;
                if glass::button(ui, r, ("camera", k), &label, on, true).clicked() {
                    s.camera = c.id.clone();
                }
            }
            ui.add_space(10.0);
            section_title(ui, l!("Медіа", "Media"));
            let yes_no = |ok: bool, what: &str| format!("{} {what}", if ok { "✓" } else { "✗" });
            hint(ui, &yes_no(can_play, l!("ffplay — звук голосових, аудіо й відео", "ffplay — sound for voice, audio and video")));
            hint(ui, &yes_no(can_record, l!("ffmpeg — відео в чаті й запис голосових", "ffmpeg — in-chat video and voice recording")));
        });
    }

    fn language_page(&mut self, ui: &mut Ui) {
        let s = &mut self.settings;
        egui::ScrollArea::vertical().id_salt("settings-lang").auto_shrink(false).show(ui, |ui| {
            // Ukrainian and English are built in; the rest come from localisation files.
            for (k, lang) in crate::i18n::available().into_iter().enumerate() {
                let r = ui.allocate_exact_size(vec2(ui.available_width(), 40.0), Sense::hover()).0;
                if glass::button(ui, r, ("lang", k), &lang.name(), s.lang == lang, true).clicked() {
                    s.lang = lang;
                }
            }
            ui.add_space(8.0);
            hint(ui, l!("Інші мови — файли перекладу в теці locales поруч із налаштуваннями.", "Other languages are translation files in the locales folder next to the settings."));
        });
    }
}

#[cfg(test)]
mod tests {
    use super::username_problem;

    #[test]
    fn username_rules() {
        assert!(username_problem("").is_none());
        assert!(username_problem("glass_gram5").is_none());
        assert!(username_problem("abcd").is_some());
        assert!(username_problem("5glass").is_some());
        assert!(username_problem("glass_").is_some());
        assert!(username_problem("скло_грам").is_some());
        assert!(username_problem(&"a".repeat(33)).is_some());
    }
}
