//! Quick replies (the shortcut list and each shortcut's messages), and the greeting and away
//! messages, which send the shortcuts "/hello" and "/away" as Telegram Desktop names them.

use super::*;
use crate::backend::{Away, AwaySchedule, Greeting, Recipients};

/// The shortcut the greeting message sends.
pub(super) const GREETING: &str = "hello";
/// The shortcut the away message sends.
pub(super) const AWAY: &str = "away";
const NAME_MAX: usize = 32;
const DATE_FORMAT: &str = "%d.%m.%Y %H:%M";

#[derive(Default)]
pub(in crate::app) struct ShortcutEdit {
    rename: String,
    /// The message being written at the bottom.
    new_text: String,
    /// A message being edited: id and text.
    editing: Option<(i32, String)>,
}

impl ShortcutEdit {
    pub(super) fn named(name: &str) -> Self {
        Self { rename: name.to_owned(), ..Default::default() }
    }
}

#[derive(Clone, PartialEq)]
pub(in crate::app) struct GreetEdit {
    on: bool,
    recipients: Recipients,
    days: i32,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum ScheduleKind {
    Always,
    OutsideHours,
    Custom,
}

#[derive(Clone, PartialEq)]
pub(in crate::app) struct AwayEdit {
    on: bool,
    recipients: Recipients,
    kind: ScheduleKind,
    /// The custom period as typed, "17.11.2026 18:00".
    start: String,
    end: String,
    offline_only: bool,
}

/// Recipients of a new automatic message: every private chat, none excluded.
fn all_chats() -> Recipients {
    Recipients { exclude_selected: true, ..Default::default() }
}

fn date_text(ts: i32) -> String {
    jiff::Timestamp::from_second(ts as i64).map(|t| t.to_zoned(jiff::tz::TimeZone::system()).strftime(DATE_FORMAT).to_string()).unwrap_or_default()
}

fn parse_date(t: &str) -> Option<i32> {
    let dt = jiff::civil::DateTime::strptime(DATE_FORMAT, t.trim()).ok()?;
    Some(dt.to_zoned(jiff::tz::TimeZone::system()).ok()?.timestamp().as_second() as i32)
}

/// Shortcut name rules, as on the server.
fn name_problem(name: &str) -> Option<&'static str> {
    if name.is_empty() {
        return None;
    }
    if !name.chars().all(|c| c.is_alphanumeric() || c == '_') {
        return Some(l!("Лише літери, цифри й підкреслення", "Only letters, digits and underscores"));
    }
    (name.chars().count() > NAME_MAX).then_some(l!("Не більше 32 символів", "At most 32 characters"))
}

/// "All 1-to-1 chats except…" or "Only selected chats…", and the chat types.
fn recipients_editor(ui: &mut Ui, id: &str, r: &mut Recipients) {
    section_title(ui, l!("Отримувачі", "Recipients"));
    let mut all = r.exclude_selected;
    settings_pages::choice(ui, id, &mut all, &[(true, l!("Усі особисті чати, крім…", "All 1-to-1 Chats Except…")), (false, l!("Лише вибрані чати…", "Only Selected Chats…"))], 200.0);
    r.exclude_selected = all;
    section_title(ui, if all { l!("Виключені типи чатів", "Excluded chat types") } else { l!("Включені типи чатів", "Included chat types") });
    let types: [(&str, &mut bool); 4] = [
        (l!("Наявні чати", "Existing Chats"), &mut r.existing),
        (l!("Нові чати", "New Chats"), &mut r.new),
        (l!("Контакти", "Contacts"), &mut r.contacts),
        (l!("Не контакти", "Non-Contacts"), &mut r.non_contacts),
    ];
    for (label, on) in types {
        if toggle_row(ui, label, *on, true) {
            *on = !*on;
        }
    }
    if !r.users.is_empty() {
        let names = r.users.iter().map(|(_, n)| n.as_str()).filter(|n| !n.is_empty()).collect::<Vec<_>>().join(", ");
        let n = r.users.len();
        let text = if all { lp!(n, ["Виключено {n} чат", "Виключено {n} чати", "Виключено {n} чатів"], ["{n} chat excluded", "{n} chats excluded"]) } else { lp!(n, ["Вибрано {n} чат", "Вибрано {n} чати", "Вибрано {n} чатів"], ["{n} chat selected", "{n} chats selected"]) };
        hint(ui, &format!("{text}: {names}"));
    }
}

/// "Only selected" with nothing selected would send to no one.
fn recipients_ok(r: &Recipients) -> bool {
    r.exclude_selected || r.existing || r.new || r.contacts || r.non_contacts || !r.users.is_empty()
}

impl App {
    pub(in crate::app) fn replies_page(&mut self, ui: &mut Ui) {
        hint(ui, l!("Готові відповіді з форматуванням. Щоб надіслати таку в особистому чаті, введіть / і назву.", "Set up shortcuts with rich text to respond to messages faster. In a private chat, type / and the shortcut's name to send it."));
        ui.add_space(8.0);
        let mut open = None;
        let b = &mut self.business;
        ui.horizontal(|ui| {
            let r = ui.allocate_exact_size(vec2(ui.available_width() - 96.0, 36.0), Sense::hover()).0;
            glass::text_field(ui, r, &mut b.new_name, l!("Назва нової відповіді", "New shortcut name"), false);
            let name = b.new_name.trim().trim_start_matches('/').to_owned();
            let r = ui.allocate_exact_size(vec2(88.0, 36.0), Sense::hover()).0;
            if glass::button(ui, r, "quick-add", l!("Додати", "Add"), true, !name.is_empty() && name_problem(&name).is_none()).clicked() {
                open = Some(Page::Shortcut(name));
            }
        });
        if let Some(problem) = name_problem(self.business.new_name.trim().trim_start_matches('/')) {
            ui.label(egui::RichText::new(problem).color(glass::danger()).size(12.5));
        }
        ui.add_space(8.0);
        if self.business.quick.is_empty() {
            hint(ui, l!("Швидких відповідей ще немає.", "No quick replies yet."));
        }
        for q in &self.business.quick {
            let n = q.count;
            let count = lp!(n, ["{n} повідомлення", "{n} повідомлення", "{n} повідомлень"], ["{n} message", "{n} messages"]);
            let about = if q.top.is_empty() { count } else { format!("{} · {count}", q.top.replace('\n', " ")) };
            if shortcut_row(ui, &q.name, &about).clicked() {
                open = Some(Page::Shortcut(q.name.clone()));
            }
        }
        if let Some(page) = open {
            self.open_business_page(page);
        }
    }

    pub(in crate::app) fn shortcut_page(&mut self, ui: &mut Ui, name: &str) {
        let id = self.business.quick.iter().find(|q| q.name == name).map(|q| q.id);
        let mut cmd = None;
        let special = name == GREETING || name == AWAY;
        let b = &mut self.business;
        match id.map(|id| b.msgs.get(&id)) {
            Some(None) => {
                ui.add(egui::Spinner::new().color(ink_dim()));
            }
            None => hint(ui, l!("Повідомлень ще немає. Додайте перше нижче.", "No messages yet. Add the first one below.")),
            Some(Some(list)) => {
                let shortcut = id.unwrap_or_default();
                for m in list {
                    if let Some(c) = message_box(ui, shortcut, m, &mut b.shortcut.editing) {
                        cmd = Some(c);
                    }
                    ui.add_space(6.0);
                }
            }
        }
        ui.add_space(6.0);
        let r = ui.allocate_exact_size(vec2(ui.available_width(), 72.0), Sense::hover()).0;
        let (_, submitted) = glass::text_area(ui, r, &mut b.shortcut.new_text, l!("Нове повідомлення…", "New message…"), egui::Modifiers::CTRL);
        ui.add_space(6.0);
        let text = b.shortcut.new_text.trim().to_owned();
        if (wide_button(ui, "quick-send", l!("Додати повідомлення", "Add Message"), true, !text.is_empty()) || submitted) && !text.is_empty() {
            cmd = Some(BusinessCmd::AddQuickMessage { shortcut: name.to_owned(), text });
            b.shortcut.new_text.clear();
        }
        hint(ui, l!("Форматування як у полі повідомлення: **жирний**, __курсив__, `код`. Ctrl+Enter — додати.", "Formatting as in the message field: **bold**, __italic__, `code`. Ctrl+Enter adds it."));
        if let Some(id) = id {
            ui.add_space(10.0);
            if !special {
                section_title(ui, l!("Назва", "Shortcut"));
                ui.horizontal(|ui| {
                    let r = ui.allocate_exact_size(vec2(ui.available_width() - 144.0, 36.0), Sense::hover()).0;
                    glass::text_field(ui, r, &mut b.shortcut.rename, l!("Назва", "Shortcut"), false);
                    let new = b.shortcut.rename.trim().trim_start_matches('/').to_owned();
                    let r = ui.allocate_exact_size(vec2(136.0, 36.0), Sense::hover()).0;
                    if glass::button(ui, r, "quick-rename", l!("Перейменувати", "Rename"), false, !new.is_empty() && new != name && name_problem(&new).is_none()).clicked() {
                        cmd = Some(BusinessCmd::RenameQuickReply { shortcut: id, name: new });
                    }
                });
            }
            ui.add_space(6.0);
            let key = format!("quick-{id}");
            let label = if b.armed.as_deref() == Some(key.as_str()) { l!("Точно видалити?", "Delete for sure?") } else { l!("Видалити швидку відповідь", "Delete Quick Reply") };
            if info::danger_row(ui, label).clicked() {
                if b.armed.as_deref() == Some(key.as_str()) {
                    cmd = Some(BusinessCmd::DeleteQuickReply(id));
                    b.armed = None;
                } else {
                    b.armed = Some(key);
                }
            }
        }
        // A rename moves the page to the new name; a delete leaves it.
        if let Some(BusinessCmd::RenameQuickReply { name, .. }) = &cmd {
            self.business.page = Page::Shortcut(name.clone());
        }
        let deleted = matches!(cmd, Some(BusinessCmd::DeleteQuickReply(_)));
        if let Some(cmd) = cmd {
            self.send(Cmd::Business(cmd));
        }
        if deleted {
            self.business_back();
        }
    }

    /// The row that leads to "/hello" or "/away", or offers to create it.
    fn special_shortcut(&mut self, ui: &mut Ui, name: &str, create: &str) -> Option<i32> {
        let q = self.business.quick.iter().find(|q| q.name == name).cloned();
        let open = match &q {
            Some(q) => {
                let n = q.count;
                let count = lp!(n, ["{n} повідомлення", "{n} повідомлення", "{n} повідомлень"], ["{n} message", "{n} messages"]);
                let about = if q.top.is_empty() { count } else { format!("{} · {count}", q.top.replace('\n', " ")) };
                shortcut_row(ui, name, &about).clicked()
            }
            None => wide_button(ui, name, create, false, true),
        };
        if open {
            self.open_business_page(Page::Shortcut(name.to_owned()));
        }
        q.map(|q| q.id)
    }

    pub(in crate::app) fn greeting_page(&mut self, ui: &mut Ui) {
        let Some(my) = self.business.my.clone() else {
            ui.add(egui::Spinner::new().color(ink_dim()));
            return;
        };
        hint(ui, l!("Привітайте клієнтів, коли вони пишуть вам уперше або після тривалої перерви.", "Greet customers when they message you for the first time or after a period of no activity."));
        ui.add_space(8.0);
        let shortcut = self.special_shortcut(ui, GREETING, l!("Створити вітальне повідомлення", "Create a Greeting Message"));
        ui.add_space(6.0);
        let saved = my.greeting.clone();
        let edit = self.business.greeting.get_or_insert_with(|| match &saved {
            Some(g) => GreetEdit { on: true, recipients: g.recipients.clone(), days: g.days },
            None => GreetEdit { on: false, recipients: all_chats(), days: 7 },
        });
        if toggle_row(ui, l!("Надсилати вітальне повідомлення", "Send Greeting Message"), edit.on, shortcut.is_some() || edit.on) {
            edit.on = !edit.on;
        }
        if edit.on {
            recipients_editor(ui, "greet-recipients", &mut edit.recipients);
            section_title(ui, l!("Період без активності", "Period of No Activity"));
            let labels: Vec<(i32, String)> = [7, 14, 21, 28].into_iter().map(|n| (n, lp!(n, ["{n} день", "{n} дні", "{n} днів"], ["{n} day", "{n} days"]))).collect();
            let options: Vec<(i32, &str)> = labels.iter().map(|(n, l)| (*n, l.as_str())).collect();
            settings_pages::choice(ui, "greet-days", &mut edit.days, &options, 86.0);
            hint(ui, l!("Привітання надійде, якщо в чаті не було повідомлень стільки днів.", "The greeting goes out if the chat had no messages for this long."));
        }
        ui.add_space(10.0);
        let wanted = match (edit.on, shortcut) {
            (true, Some(id)) => Some(Greeting { shortcut: id, recipients: edit.recipients.clone(), days: edit.days }),
            _ => None,
        };
        let ready = wanted != saved && (wanted.is_some() || !edit.on) && recipients_ok(&edit.recipients);
        if wide_button(ui, "greet-save", l!("Зберегти", "Save"), true, ready) {
            self.send(Cmd::Business(BusinessCmd::SetGreeting(wanted)));
        }
    }

    pub(in crate::app) fn away_page(&mut self, ui: &mut Ui) {
        let Some(my) = self.business.my.clone() else {
            ui.add(egui::Spinner::new().color(ink_dim()));
            return;
        };
        hint(ui, l!("Автоматично відповідайте, коли вас немає на зв'язку.", "Automatically reply with a message when you are away."));
        ui.add_space(8.0);
        let shortcut = self.special_shortcut(ui, AWAY, l!("Створити повідомлення про відсутність", "Create an Away Message"));
        ui.add_space(6.0);
        let saved = my.away.clone();
        let has_hours = my.hours.is_some();
        let edit = self.business.away.get_or_insert_with(|| away_edit(saved.as_ref()));
        if toggle_row(ui, l!("Надсилати повідомлення про відсутність", "Send Away Message"), edit.on, shortcut.is_some() || edit.on) {
            edit.on = !edit.on;
        }
        if edit.on {
            section_title(ui, l!("Розклад", "Schedule"));
            let options = [(ScheduleKind::Always, l!("Завжди", "Always Send")), (ScheduleKind::OutsideHours, l!("Поза годинами роботи", "Outside of Business Hours")), (ScheduleKind::Custom, l!("Свій розклад", "Custom Schedule"))];
            settings_pages::choice(ui, "away-kind", &mut edit.kind, &options, 200.0);
            if edit.kind == ScheduleKind::OutsideHours && !has_hours {
                hint(ui, l!("Спершу вкажіть години роботи.", "Set your opening hours first."));
            }
            if edit.kind == ScheduleKind::Custom {
                field(ui, &mut edit.start, l!("Початок: 17.11.2026 18:00", "Start: 17.11.2026 18:00"));
                ui.add_space(4.0);
                field(ui, &mut edit.end, l!("Кінець: 24.11.2026 09:00", "End: 24.11.2026 09:00"));
            }
            ui.add_space(6.0);
            if toggle_row(ui, l!("Лише коли я не в мережі", "Only if Offline"), edit.offline_only, true) {
                edit.offline_only = !edit.offline_only;
            }
            recipients_editor(ui, "away-recipients", &mut edit.recipients);
        }
        ui.add_space(10.0);
        let schedule = match edit.kind {
            ScheduleKind::Always => Some(AwaySchedule::Always),
            ScheduleKind::OutsideHours => Some(AwaySchedule::OutsideHours),
            ScheduleKind::Custom => match (parse_date(&edit.start), parse_date(&edit.end)) {
                (Some(s), Some(e)) if e > s => Some(AwaySchedule::Custom(s, e)),
                _ => None,
            },
        };
        if edit.on && schedule.is_none() {
            ui.label(egui::RichText::new(l!("Дата вводиться як 17.11.2026 18:00, кінець — після початку", "Dates look like 17.11.2026 18:00, the end after the start")).color(glass::danger()).size(12.5));
        }
        let wanted = match (edit.on, shortcut, schedule) {
            (true, Some(id), Some(schedule)) => Some(Away { shortcut: id, offline_only: edit.offline_only, schedule, recipients: edit.recipients.clone() }),
            _ => None,
        };
        let ready = wanted != saved && (wanted.is_some() || !edit.on) && recipients_ok(&edit.recipients);
        if wide_button(ui, "away-save", l!("Зберегти", "Save"), true, ready) {
            self.send(Cmd::Business(BusinessCmd::SetAway(wanted)));
        }
    }
}

fn away_edit(saved: Option<&Away>) -> AwayEdit {
    let now = jiff::Timestamp::now().as_second() as i32;
    let Some(a) = saved else {
        return AwayEdit { on: false, recipients: all_chats(), kind: ScheduleKind::OutsideHours, start: date_text(now), end: date_text(now + 7 * 86_400), offline_only: false };
    };
    let (kind, start, end) = match a.schedule {
        AwaySchedule::Always => (ScheduleKind::Always, now, now + 7 * 86_400),
        AwaySchedule::OutsideHours => (ScheduleKind::OutsideHours, now, now + 7 * 86_400),
        AwaySchedule::Custom(s, e) => (ScheduleKind::Custom, s, e),
    };
    AwayEdit { on: true, recipients: a.recipients.clone(), kind, start: date_text(start), end: date_text(end), offline_only: a.offline_only }
}

/// "/name" in bold over a line about it.
fn shortcut_row(ui: &mut Ui, name: &str, about: &str) -> egui::Response {
    let (r, resp) = ui.allocate_exact_size(vec2(ui.available_width(), 52.0), Sense::click());
    let p = ui.painter();
    p.rect_filled(r, 14.0, glass::veil(if resp.hovered() { 18 } else { 8 }));
    if resp.hovered() {
        ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
    }
    let bold = FontId::new(14.5, egui::FontFamily::Name(format::BOLD.into()));
    p.text(pos2(r.left() + 12.0, r.top() + 9.0), Align2::LEFT_TOP, format!("/{name}"), bold, ink());
    p.galley(pos2(r.left() + 12.0, r.top() + 30.0), elided(ui, about, 12.5, ink_dim(), r.width() - 48.0), ink_dim());
    icons::draw(p, Rect::from_center_size(pos2(r.right() - 16.0, r.center().y), vec2(16.0, 16.0)), Icon::ChevronRight, ink_dim());
    resp
}

/// One message of a shortcut with edit and delete buttons, or its editor.
fn message_box(ui: &mut Ui, shortcut: i32, m: &QuickMsg, editing: &mut Option<(i32, String)>) -> Option<BusinessCmd> {
    let mut cmd = None;
    egui::Frame::new().fill(glass::veil(10)).corner_radius(14.0).inner_margin(egui::Margin::symmetric(12, 10)).show(ui, |ui| {
        ui.set_width(ui.available_width());
        if let Some((_, text)) = editing.as_mut().filter(|(id, _)| *id == m.id) {
            let r = ui.allocate_exact_size(vec2(ui.available_width(), 72.0), Sense::hover()).0;
            let (_, submitted) = glass::text_area(ui, r, text, l!("Текст", "Text"), egui::Modifiers::CTRL);
            ui.add_space(6.0);
            let mut done = false;
            ui.horizontal(|ui| {
                let r = ui.allocate_exact_size(vec2(110.0, 32.0), Sense::hover()).0;
                if (glass::button(ui, r, ("quick-save", m.id), l!("Зберегти", "Save"), true, !text.trim().is_empty()).clicked() || submitted) && !text.trim().is_empty() {
                    cmd = Some(BusinessCmd::EditQuickMessage { shortcut, id: m.id, text: text.trim().to_owned() });
                    done = true;
                }
                let r = ui.allocate_exact_size(vec2(110.0, 32.0), Sense::hover()).0;
                if glass::button(ui, r, ("quick-cancel", m.id), l!("Скасувати", "Cancel"), false, true).clicked() {
                    done = true;
                }
            });
            if done {
                *editing = None;
            }
            return;
        }
        // The list shows the text as it reads; markup stays for editing.
        let plain = format::parse(&m.text).0;
        let text = if plain.is_empty() && m.media { l!("📎 Вкладення", "📎 Attachment").to_owned() } else if m.media { format!("📎 {plain}") } else { plain };
        ui.horizontal(|ui| {
            let w = ui.available_width() - 80.0;
            ui.allocate_ui(vec2(w, 0.0), |ui| {
                ui.set_width(w);
                ui.add(egui::Label::new(egui::RichText::new(text).color(ink()).size(14.0)).wrap());
            });
            let (r, _) = ui.allocate_exact_size(vec2(76.0, 32.0), Sense::hover());
            let edit = Rect::from_min_size(r.min, vec2(34.0, 32.0));
            let del = edit.translate(vec2(40.0, 0.0));
            if glass::icon_button(ui, edit, ("quick-edit", m.id), Icon::Pencil, false, true).on_hover_text(l!("Змінити", "Edit")).clicked() {
                *editing = Some((m.id, m.text.clone()));
            }
            if glass::icon_button(ui, del, ("quick-del", m.id), Icon::Trash, false, true).on_hover_text(l!("Видалити", "Delete")).clicked() {
                cmd = Some(BusinessCmd::DeleteQuickMessage { shortcut, id: m.id });
            }
        });
    });
    cmd
}
