//! Settings → "My Stars", as in Telegram Desktop: the balance on top, then the history with
//! All / Incoming / Outgoing tabs; a click on a row opens its details under it.
//! Only showing: buying Stars and paying wait for the payments security review.

use super::*;
use crate::backend::{StarsDir, StarsTx};

/// One tab's history.
#[derive(Default)]
struct History {
    list: Vec<StarsTx>,
    /// Offset of the next page; empty — everything is loaded.
    next: String,
    /// A first page has come.
    loaded: bool,
    /// A page was asked for and hasn't come yet.
    loading: bool,
}

#[derive(Default)]
pub(super) struct Stars {
    /// Whole Stars and the fraction; `None` until the server answers.
    balance: Option<(i64, i32)>,
    dir: StarsDir,
    /// All, Incoming, Outgoing.
    tabs: [History; 3],
    /// The transaction whose details are open (`StarsTx::id`).
    open: Option<String>,
}

fn slot(dir: StarsDir) -> usize {
    match dir {
        StarsDir::All => 0,
        StarsDir::Incoming => 1,
        StarsDir::Outgoing => 2,
    }
}

fn dir_title(dir: StarsDir) -> &'static str {
    match dir {
        StarsDir::All => l!("Усі", "All"),
        StarsDir::Incoming => l!("Вхідні", "Incoming"),
        StarsDir::Outgoing => l!("Вихідні", "Outgoing"),
    }
}

/// "1 234" with the fraction when there is one.
fn balance_text(stars: i64, nanos: i32) -> String {
    let digits = stars.unsigned_abs().to_string();
    let mut s = String::new();
    for (k, c) in digits.chars().enumerate() {
        if k > 0 && (digits.len() - k) % 3 == 0 {
            s.push('\u{202f}');
        }
        s.push(c);
    }
    if stars < 0 {
        s.insert(0, '−');
    }
    if nanos != 0 {
        s.push('.');
        s.push_str(format!("{:09}", nanos.unsigned_abs()).trim_end_matches('0'));
    }
    s
}

/// Gold of the star, from the folder tag palette so it stays readable in both themes.
fn gold() -> Color32 {
    glass::tag_color(1)
}

fn amount_color(t: &StarsTx) -> Color32 {
    if t.failed {
        ink_dim()
    } else if t.stars < 0 || t.nanos < 0 {
        glass::danger()
    } else {
        glass::tag_color(3)
    }
}

/// What a button in the details box asks for after drawing.
enum DetailsAct {
    Copy(String),
    Open(String),
}

/// Refunded, pending or failed; empty for a completed one.
fn state(t: &StarsTx) -> &'static str {
    if t.failed {
        l!("Не вдалося", "Failed")
    } else if t.pending {
        l!("Очікує", "Pending")
    } else if t.refund {
        l!("Повернено", "Refunded")
    } else {
        ""
    }
}

impl App {
    pub(super) fn open_stars(&mut self) {
        self.stars.open = None;
        self.stars.tabs = Default::default();
        self.load_stars_page(self.stars.dir, String::new());
    }

    fn load_stars_page(&mut self, dir: StarsDir, offset: String) {
        self.stars.tabs[slot(dir)].loading = true;
        self.send(Cmd::LoadStars { dir, offset });
    }

    /// The balance next to "My Stars" on the main settings page, once known.
    pub(super) fn stars_label(&self) -> String {
        self.stars.balance.map(|(s, n)| balance_text(s, n)).unwrap_or_default()
    }

    pub(super) fn on_stars_balance(&mut self, stars: i64, nanos: i32) {
        self.stars.balance = Some((stars, nanos));
    }

    pub(super) fn on_stars_history(&mut self, dir: StarsDir, list: Vec<StarsTx>, next: String, append: bool) {
        let h = &mut self.stars.tabs[slot(dir)];
        if !append {
            h.list.clear();
        }
        h.list.extend(list);
        h.next = next;
        h.loaded = true;
        h.loading = false;
    }

    pub(super) fn stars_page(&mut self, ui: &mut Ui) {
        let mut load = None;
        let mut act = None;
        egui::ScrollArea::vertical().id_salt("stars-body").auto_shrink(false).show(ui, |ui| {
            self.stars_balance(ui);
            ui.add_space(12.0);
            ui.horizontal(|ui| {
                ui.spacing_mut().item_spacing.x = 6.0;
                for dir in [StarsDir::All, StarsDir::Incoming, StarsDir::Outgoing] {
                    if info::chip(ui, dir_title(dir), self.stars.dir == dir).clicked() && self.stars.dir != dir {
                        self.stars.dir = dir;
                        self.stars.open = None;
                        let h = &self.stars.tabs[slot(dir)];
                        if !h.loaded && !h.loading {
                            load = Some((dir, String::new()));
                        }
                    }
                }
            });
            ui.add_space(8.0);
            let h = &self.stars.tabs[slot(self.stars.dir)];
            if !h.loaded {
                ui.add(egui::Spinner::new().color(ink_dim()));
                return;
            }
            if h.list.is_empty() {
                hint(ui, l!("Тут з'являтимуться ваші операції із зірками.", "Your Stars transactions will appear here."));
                return;
            }
            let mut toggle = None;
            for t in &h.list {
                let open = self.stars.open.as_deref() == Some(t.id.as_str());
                if stars_row(ui, t, open).clicked() {
                    toggle = Some(t.id.clone());
                }
                if open {
                    act = stars_details(ui, t);
                }
                ui.add_space(4.0);
            }
            if !h.next.is_empty() {
                let r = ui.allocate_exact_size(vec2(ui.available_width(), 36.0), Sense::hover()).0;
                let label = if h.loading { l!("Завантаження…", "Loading…") } else { l!("Показати ще", "Show more") };
                if glass::button(ui, r, "stars-more", label, false, !h.loading).clicked() {
                    load = Some((self.stars.dir, h.next.clone()));
                }
            }
            if let Some(id) = toggle {
                self.stars.open = if self.stars.open.as_ref() == Some(&id) { None } else { Some(id) };
            }
        });
        if let Some((dir, offset)) = load {
            self.load_stars_page(dir, offset);
        }
        match act {
            Some(DetailsAct::Copy(id)) => {
                self.ctx.copy_text(id);
                self.toast = Some((l!("ID скопійовано", "ID copied").into(), 0.0, false));
            }
            Some(DetailsAct::Open(url)) => self.open_link(&url),
            None => {}
        }
    }

    /// The big star and the number, as at the top of the page in Telegram Desktop.
    fn stars_balance(&self, ui: &mut Ui) {
        let (card, _) = ui.allocate_exact_size(vec2(ui.available_width(), 128.0), Sense::hover());
        let p = ui.painter();
        p.rect_filled(card, 20.0, glass::veil(10));
        let star = Rect::from_center_size(pos2(card.center().x, card.top() + 34.0), vec2(40.0, 40.0));
        icons::draw(p, star, Icon::Star, gold());
        let text = self.stars.balance.map_or_else(|| "…".to_owned(), |(s, n)| balance_text(s, n));
        let bold = FontId::new(26.0, egui::FontFamily::Name(format::BOLD.into()));
        p.text(pos2(card.center().x, card.top() + 78.0), Align2::CENTER_CENTER, text, bold, ink());
        p.text(pos2(card.center().x, card.top() + 106.0), Align2::CENTER_CENTER, l!("Ваш баланс", "Your balance"), FontId::proportional(12.5), ink_dim());
        hint(ui, l!("Зірки купують цифрові товари в ботах і мініапках, дарують подарунки й платять за реакції. Поповнення поки що доступне лише в офіційних застосунках.", "Stars buy digital goods in bots and mini apps, pay for gifts and reactions. Top-up is available only in the official apps for now."));
    }
}

/// Everything Telegram keeps about one transaction, under its row.
fn stars_details(ui: &mut Ui, t: &StarsTx) -> Option<DetailsAct> {
    let mut act = None;
    let rows: Vec<(&str, String)> = [
        (l!("Сума", "Amount"), format!("{} ⭐", t.amount())),
        (l!("Від / кому", "From / to"), t.source.clone()),
        (l!("Дата", "Date"), t.when.clone()),
        (l!("Стан", "Status"), if state(t).is_empty() { l!("Виконано", "Completed").to_owned() } else { state(t).to_owned() }),
        (l!("Опис", "Description"), t.description.clone()),
        (l!("ID операції", "Transaction ID"), t.id.clone()),
    ]
    .into_iter()
    .filter(|(_, v)| !v.is_empty())
    .collect();
    egui::Frame::new().fill(glass::veil(6)).corner_radius(14.0).inner_margin(egui::Margin::symmetric(14, 10)).show(ui, |ui| {
        ui.set_width(ui.available_width());
        egui::Grid::new(("stars-details", &t.id)).num_columns(2).spacing(vec2(16.0, 6.0)).show(ui, |ui| {
            for (k, v) in &rows {
                ui.label(egui::RichText::new(*k).color(ink_dim()).size(12.5));
                ui.add(egui::Label::new(egui::RichText::new(v).color(ink()).size(13.5)).wrap());
                ui.end_row();
            }
        });
        ui.add_space(6.0);
        ui.horizontal(|ui| {
            let r = ui.allocate_exact_size(vec2(150.0, 32.0), Sense::hover()).0;
            if glass::button(ui, r, ("stars-copy", &t.id), l!("Копіювати ID", "Copy ID"), false, true).clicked() {
                act = Some(DetailsAct::Copy(t.id.clone()));
            }
            if !t.url.is_empty() {
                let r = ui.allocate_exact_size(vec2(170.0, 32.0), Sense::hover()).0;
                if glass::button(ui, r, ("stars-url", &t.id), l!("Переглянути в блокчейні", "View in blockchain"), false, true).clicked() {
                    act = Some(DetailsAct::Open(t.url.clone()));
                }
            }
        });
    });
    act
}

/// Source and title on the left, the signed amount with a star on the right.
fn stars_row(ui: &mut Ui, t: &StarsTx, open: bool) -> egui::Response {
    let (row, resp) = ui.allocate_exact_size(vec2(ui.available_width(), 58.0), Sense::click());
    let p = ui.painter();
    p.rect_filled(row, 14.0, glass::veil(if open || resp.hovered() { 18 } else { 10 }));
    if resp.hovered() {
        ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
    }
    let amount = t.amount();
    let font = FontId::new(15.0, egui::FontFamily::Name(format::BOLD.into()));
    let ag = p.layout_no_wrap(amount, font, amount_color(t));
    let star = Rect::from_center_size(pos2(row.right() - 22.0, row.center().y - 6.0), vec2(18.0, 18.0));
    icons::draw(p, star, Icon::Star, gold());
    let ax = star.left() - 6.0 - ag.size().x;
    let aw = ag.size().x;
    p.galley(pos2(ax, star.center().y - ag.size().y / 2.0), ag, ink());
    let st = state(t);
    if !st.is_empty() {
        p.text(pos2(row.right() - 12.0, row.bottom() - 12.0), Align2::RIGHT_CENTER, st, FontId::proportional(11.0), ink_dim());
    }
    let x = row.left() + 12.0;
    let w = row.width() - aw - 60.0;
    let (top, sub) = match (t.title.is_empty(), t.source.is_empty()) {
        (false, false) => (t.title.clone(), format!("{} · {}", t.source, t.when)),
        (true, false) => (t.source.clone(), t.when.clone()),
        (false, true) => (t.title.clone(), t.when.clone()),
        (true, true) => (l!("Операція", "Transaction").to_owned(), t.when.clone()),
    };
    p.galley(pos2(x, row.top() + 9.0), elided(ui, &top, 14.5, ink(), w), ink());
    p.galley(pos2(x, row.top() + 32.0), elided(ui, &sub, 12.5, ink_dim(), w), ink());
    resp
}

fn hint(ui: &mut Ui, text: &str) {
    ui.label(egui::RichText::new(text).color(ink_dim()).size(12.5));
}
