//! Birthdays, as in Telegram: a reminder above the chat list on contacts' birthdays, the birthday
//! in a user's info and our own in My Account.

use super::*;
use crate::backend::Birthday;

/// Today's reminder and our own birthday in My Account.
#[derive(Default)]
pub(super) struct Birthdays {
    /// Contacts whose birthday is today.
    pub(super) today: Vec<(ChatInfo, Birthday)>,
    /// The day the reminder was hidden.
    pub(super) dismissed: Option<jiff::civil::Date>,
    /// Our own birthday being edited.
    pub(super) edit: Option<Birthday>,
}

pub(super) enum BirthdayAction {
    Open(ChatInfo),
    Dismiss,
}

/// Month as it reads in a date ("5 березня" / "March 5").
fn month(m: u8) -> &'static str {
    match m {
        1 => l!("січня", "January"),
        2 => l!("лютого", "February"),
        3 => l!("березня", "March"),
        4 => l!("квітня", "April"),
        5 => l!("травня", "May"),
        6 => l!("червня", "June"),
        7 => l!("липня", "July"),
        8 => l!("серпня", "August"),
        9 => l!("вересня", "September"),
        10 => l!("жовтня", "October"),
        11 => l!("листопада", "November"),
        _ => l!("грудня", "December"),
    }
}

/// "5 березня 1990" / "March 5, 1990".
pub(super) fn birthday_label(b: Birthday) -> String {
    let (day, month) = (b.day, month(b.month));
    match b.year {
        Some(year) => lf!("{day} {month} {year}", "{month} {day}, {year}"),
        None => lf!("{day} {month}", "{month} {day}"),
    }
}

impl App {
    /// The reminder row at the top of the main list; `None` when there is nothing to show.
    pub(super) fn birthday_banner(&self, ui: &mut Ui) -> Option<BirthdayAction> {
        let today = jiff::Zoned::now().date();
        if self.birthdays.today.is_empty() || self.birthdays.dismissed == Some(today) {
            return None;
        }
        let names: Vec<&str> = self.birthdays.today.iter().map(|(c, _)| c.title.as_str()).collect();
        let text = lf!("Сьогодні день народження: {}", "Birthday today: {}", names.join(", "));
        let w = ui.available_width();
        let (row, resp) = ui.allocate_exact_size(vec2(w, 52.0), Sense::click());
        let card = row.shrink2(vec2(4.0, 2.0));
        let p = ui.painter();
        glass::glass(p, card, Glass::tinted(16.0, ACCENT, if resp.hovered() { 70 } else { 45 }));
        p.text(pos2(card.left() + 24.0, card.center().y), Align2::CENTER_CENTER, "🎂", FontId::proportional(20.0), ink());
        p.galley(pos2(card.left() + 44.0, card.top() + 8.0), elided(ui, &text, 13.5, ink(), card.width() - 90.0), ink());
        let hint = l!("Привітайте — натисніть, щоб відкрити чат", "Send your wishes — click to open the chat");
        p.galley(pos2(card.left() + 44.0, card.top() + 27.0), elided(ui, hint, 12.0, ink_dim(), card.width() - 90.0), ink_dim());
        if resp.hovered() {
            ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
        }
        let x = Rect::from_center_size(pos2(card.right() - 22.0, card.center().y), vec2(32.0, 32.0));
        if glass::icon_button(ui, x, "birthday-dismiss", Icon::Close, false, true).on_hover_text(l!("Сховати до завтра", "Hide until tomorrow")).clicked() {
            return Some(BirthdayAction::Dismiss);
        }
        resp.clicked().then(|| BirthdayAction::Open(self.birthdays.today[0].0.clone()))
    }

    pub(super) fn birthday_action(&mut self, a: BirthdayAction) {
        match a {
            BirthdayAction::Dismiss => self.birthdays.dismissed = Some(jiff::Zoned::now().date()),
            BirthdayAction::Open(c) => {
                let id = c.id;
                if !self.chats.iter().any(|x| x.id == id) {
                    self.chats.push(c);
                }
                if self.backend.is_none() {
                    self.history.entry(id).or_default();
                }
                self.open_chat(id);
            }
        }
    }
}

/// What the picker starts from: the saved birthday or January 1.
pub(super) fn shown(saved: Option<Birthday>) -> Birthday {
    saved.unwrap_or(Birthday { day: 1, month: 1, year: None })
}

/// Birthday picker on the My Account page: day, month, optional year. Returns the command to send.
pub(super) fn birthday_section(ui: &mut Ui, edit: &mut Option<Birthday>, saved: Option<Birthday>) -> Option<Cmd> {
    section_title(ui, l!("День народження", "Birthday"));
    let mut cmd = None;
    let edit = edit.get_or_insert(shown(saved));
    ui.horizontal(|ui| {
        egui::ComboBox::from_id_salt("bday-day").width(56.0).selected_text(edit.day.to_string()).show_ui(ui, |ui| {
            for d in 1..=31u8 {
                ui.selectable_value(&mut edit.day, d, d.to_string());
            }
        });
        egui::ComboBox::from_id_salt("bday-month").width(110.0).selected_text(month(edit.month)).show_ui(ui, |ui| {
            for m in 1..=12u8 {
                ui.selectable_value(&mut edit.month, m, month(m));
            }
        });
        let this_year = jiff::Zoned::now().year() as u16;
        let year_text = edit.year.map_or_else(|| l!("рік не вказано", "no year").to_owned(), |y| y.to_string());
        egui::ComboBox::from_id_salt("bday-year").width(110.0).selected_text(year_text).show_ui(ui, |ui| {
            ui.selectable_value(&mut edit.year, None, l!("рік не вказано", "no year"));
            for y in (this_year - 100..=this_year).rev() {
                ui.selectable_value(&mut edit.year, Some(y), y.to_string());
            }
        });
    });
    let current = *edit;
    let hint = match saved {
        Some(b) => lf!("Зараз: {}. Хто бачить, налаштовується в «Конфіденційності».", "Now: {}. Who sees it is set in Privacy and Security.", birthday_label(b)),
        None => l!("Не вказано", "Not set").to_owned(),
    };
    ui.label(egui::RichText::new(hint).color(ink_dim()).size(12.5));
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = 6.0;
        let (r, _) = ui.allocate_exact_size(vec2(160.0, 34.0), Sense::hover());
        if glass::button(ui, r, "bday-save", l!("Зберегти", "Save"), true, saved != Some(current)).clicked() {
            cmd = Some(Cmd::SetBirthday(Some(current)));
        }
        let (r, _) = ui.allocate_exact_size(vec2(160.0, 34.0), Sense::hover());
        if glass::button(ui, r, "bday-remove", l!("Прибрати", "Remove"), false, saved.is_some()).clicked() {
            cmd = Some(Cmd::SetBirthday(None));
        }
    });
    cmd
}
