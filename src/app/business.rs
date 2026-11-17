//! Settings → "Telegram Business", as in Telegram Desktop: location, opening hours, quick replies,
//! greeting and away messages, chatbots and chat links; without Premium the same list as a promo.
//! Also a business's address and hours in its profile, and `/shortcut` quick replies in the composer.

use std::collections::HashMap;

use super::*;
use crate::backend::{BusinessCmd, BusinessEv, BusinessInfo, ChatLink, ConnectedBot, MyBusiness, QuickMsg, QuickReply, WEEK, WorkHours};

mod hours;
mod links;
mod messages;

const DAY: i32 = 24 * 60;
/// Address limit, as on the server.
const ADDRESS_MAX: usize = 96;

/// Which page of the section is open.
#[derive(Clone, PartialEq, Eq, Default)]
pub(super) enum Page {
    #[default]
    Main,
    Location,
    Hours,
    Replies,
    /// One shortcut's messages, by name (a new one has no id until its first message).
    Shortcut(String),
    Greeting,
    Away,
    Bots,
    Links,
}

#[derive(Default)]
pub(super) struct Business {
    page: Page,
    /// Our settings; `None` until the server answers.
    my: Option<MyBusiness>,
    quick: Vec<QuickReply>,
    /// Quick replies were asked for this run (the composer asks once, the page on every open).
    quick_asked: bool,
    /// Messages of shortcuts by id.
    msgs: HashMap<i32, Vec<QuickMsg>>,
    links: Option<Vec<ChatLink>>,
    bots: Option<Vec<ConnectedBot>>,
    /// Drafts of the pages; filled from `my` when a page opens or once it arrives.
    address: Option<String>,
    hours: Option<hours::HoursEdit>,
    greeting: Option<messages::GreetEdit>,
    away: Option<messages::AwayEdit>,
    shortcut: messages::ShortcutEdit,
    link_edit: Option<links::LinkEdit>,
    /// The link whose actions are unfolded.
    link_open: Option<String>,
    /// The new shortcut name on the quick replies page; the bot username on the chatbots page.
    new_name: String,
    /// A delete button pressed once: the second press confirms.
    armed: Option<String>,
}

/// A feature row of the main page: icon, title and what it does.
struct Feature {
    page: Page,
    icon: Icon,
    title: &'static str,
    about: &'static str,
}

fn features() -> [Feature; 7] {
    [
        Feature { page: Page::Location, icon: Icon::Location, title: l!("Розташування", "Location"), about: l!("Показуйте розташування свого бізнесу у профілі.", "Display the location of your business on your account.") },
        Feature { page: Page::Hours, icon: Icon::Clock, title: l!("Години роботи", "Opening Hours"), about: l!("Показуйте клієнтам, коли ви працюєте.", "Show to your customers when you are open for business.") },
        Feature {
            page: Page::Replies,
            icon: Icon::Reply,
            title: l!("Швидкі відповіді", "Quick Replies"),
            about: l!("Готові відповіді з форматуванням, щоб відповідати швидше.", "Set up shortcuts with rich text and media to respond to messages faster."),
        },
        Feature {
            page: Page::Greeting,
            icon: Icon::Hand,
            title: l!("Вітальні повідомлення", "Greeting Messages"),
            about: l!("Привітання, які автоматично надсилаються новим клієнтам.", "Create greetings that will be automatically sent to new customers."),
        },
        Feature {
            page: Page::Away,
            icon: Icon::Moon,
            title: l!("Повідомлення про відсутність", "Away Messages"),
            about: l!("Повідомлення, які надсилаються, коли вас немає.", "Define messages that are automatically sent when you are off."),
        },
        Feature { page: Page::Bots, icon: Icon::Bot, title: l!("Чат-боти", "Chatbots"), about: l!("Додайте бота, який відповідатиме клієнтам за вас.", "Add any third-party chatbots that will process customer interactions.") },
        Feature {
            page: Page::Links,
            icon: Icon::Link,
            title: l!("Посилання на чат", "Links to Chat"),
            about: l!("Посилання, що відкривають чат з вами й пропонують перше повідомлення.", "Create links that start a chat with you, suggesting the first message."),
        },
    ]
}

/// "09:00"; the end of a day stays "24:00".
pub(super) fn clock_text(minutes: i32) -> String {
    let m = if minutes > 0 && minutes % DAY == 0 { DAY } else { minutes.rem_euclid(DAY) };
    format!("{:02}:{:02}", m / 60, m % 60)
}

pub(super) fn day_name(day: usize) -> &'static str {
    [
        l!("Понеділок", "Monday"),
        l!("Вівторок", "Tuesday"),
        l!("Середа", "Wednesday"),
        l!("Четвер", "Thursday"),
        l!("П'ятниця", "Friday"),
        l!("Субота", "Saturday"),
        l!("Неділя", "Sunday"),
    ][day % 7]
}

/// What a day of the week shows.
#[derive(Clone, Debug, PartialEq)]
pub(super) enum Line {
    FullDay,
    /// Minutes from the day's start; the end may pass midnight.
    Range(i32, i32),
}

/// The week's intervals by day of their start: pieces cut at midnight, whole days as "24 hours",
/// and a piece running past midnight joined back onto the day it started.
pub(super) fn day_lines(weekly: &[(i32, i32)]) -> [Vec<Line>; 7] {
    let mut days: [Vec<(i32, i32)>; 7] = Default::default();
    for &(s, e) in weekly {
        let mut s = s;
        while s < e {
            let day_end = (s.div_euclid(DAY) + 1) * DAY;
            let to = e.min(day_end);
            let day = (s.div_euclid(DAY)).rem_euclid(7) as usize;
            days[day].push((s - s.div_euclid(DAY) * DAY, to - s.div_euclid(DAY) * DAY));
            s = to;
        }
    }
    let mut out: [Vec<Line>; 7] = Default::default();
    for (d, pieces) in days.iter_mut().enumerate() {
        pieces.sort();
        out[d] = pieces.iter().map(|&(s, e)| if s == 0 && e == DAY { Line::FullDay } else { Line::Range(s, e) }).collect();
    }
    for d in 0..7 {
        let next = (d + 1) % 7;
        let Some(Line::Range(s, DAY)) = out[d].last().cloned() else { continue };
        if let Some(Line::Range(0, e2)) = out[next].first().cloned() {
            out[next].remove(0);
            if let Some(last) = out[d].last_mut() {
                *last = Line::Range(s, DAY + e2);
            }
        }
    }
    out
}

/// Sorted, overlapping and touching intervals merged, a Sunday-night one carried into Monday.
pub(super) fn normalized(mut list: Vec<(i32, i32)>) -> Vec<(i32, i32)> {
    list.retain(|(s, e)| e > s);
    list.sort();
    let mut out: Vec<(i32, i32)> = Vec::new();
    for (s, e) in list {
        match out.last_mut() {
            Some(last) if s <= last.1 => last.1 = last.1.max(e),
            _ => out.push((s, e)),
        }
    }
    if out.len() > 1 && out[0].0 == 0 && out.last().is_some_and(|l| l.1 == WEEK) {
        let first = out.remove(0);
        if let Some(last) = out.last_mut() {
            last.1 = WEEK + first.1;
        }
    }
    out
}

/// Minute of the week now (Monday 00:00 is 0) at a UTC offset in seconds.
fn week_minute(offset: i32) -> i32 {
    let minutes = (jiff::Timestamp::now().as_second() + offset as i64).div_euclid(60);
    // 1 January 1970 was a Thursday.
    ((minutes + 3 * DAY as i64).rem_euclid(WEEK as i64)) as i32
}

/// Open right now, by our clock when the zone is known, else by the server's word.
pub(super) fn open_now(h: &WorkHours, offset: Option<i32>) -> bool {
    let Some(offset) = offset else { return h.open_now };
    let m = week_minute(offset);
    h.weekly.iter().any(|&(s, e)| (s <= m && m < e) || (s <= m + WEEK && m + WEEK < e))
}

/// Our own UTC offset in seconds.
fn my_offset() -> i32 {
    jiff::Zoned::now().offset().seconds()
}

/// "+02:00" of an offset.
fn offset_text(offset: i32) -> String {
    let sign = if offset < 0 { '−' } else { '+' };
    let a = offset.unsigned_abs() / 60;
    format!("UTC{sign}{:02}:{:02}", a / 60, a % 60)
}

/// A two-line row with an icon and a chevron.
fn feature_row(ui: &mut Ui, icon: Icon, title: &str, about: &str, chevron: bool) -> egui::Response {
    let w = ui.available_width();
    let job = egui::text::LayoutJob::simple(about.to_owned(), FontId::proportional(12.5), ink_dim(), w - 80.0);
    let g = ui.painter().layout_job(job);
    let (r, resp) = ui.allocate_exact_size(vec2(w, g.size().y + 36.0), if chevron { Sense::click() } else { Sense::hover() });
    let p = ui.painter();
    if chevron && resp.hovered() {
        p.rect_filled(r, 14.0, glass::veil(16));
        ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
    }
    let badge = Rect::from_center_size(pos2(r.left() + 22.0, r.top() + 22.0), vec2(30.0, 30.0));
    glass::glass(p, badge, Glass::tinted(15.0, ACCENT, 120));
    icons::draw(p, Rect::from_center_size(badge.center(), vec2(17.0, 17.0)), icon, ink());
    p.text(pos2(r.left() + 48.0, r.top() + 10.0), Align2::LEFT_TOP, title, FontId::proportional(14.5), ink());
    p.galley(pos2(r.left() + 48.0, r.top() + 30.0), g, ink_dim());
    if chevron {
        icons::draw(p, Rect::from_center_size(pos2(r.right() - 14.0, r.center().y), vec2(16.0, 16.0)), Icon::ChevronRight, ink_dim());
    }
    resp
}

/// A row with a label and an on/off switch; returns whether it was clicked.
pub(super) fn toggle_row(ui: &mut Ui, label: &str, on: bool, enabled: bool) -> bool {
    let (r, resp) = ui.allocate_exact_size(vec2(ui.available_width(), 40.0), if enabled { Sense::click() } else { Sense::hover() });
    let p = ui.painter();
    if enabled && resp.hovered() {
        p.rect_filled(r, 12.0, glass::veil(14));
        ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
    }
    p.text(pos2(r.left() + 12.0, r.center().y), Align2::LEFT_CENTER, label, FontId::proportional(14.0), if enabled { ink() } else { ink_dim() });
    let s = Rect::from_center_size(pos2(r.right() - 30.0, r.center().y), vec2(40.0, 22.0));
    p.rect_filled(s, 11.0, if on { ACCENT } else { glass::veil(50) });
    let x = if on { s.right() - 11.0 } else { s.left() + 11.0 };
    p.circle_filled(pos2(x, s.center().y), 8.5, glass::ON_DARK);
    resp.clicked()
}

pub(super) fn hint(ui: &mut Ui, text: &str) {
    ui.add(egui::Label::new(egui::RichText::new(text).color(ink_dim()).size(12.5)).wrap());
}

/// A full-width capsule button.
pub(super) fn wide_button(ui: &mut Ui, id: &str, label: &str, accent: bool, enabled: bool) -> bool {
    let r = ui.allocate_exact_size(vec2(ui.available_width(), 36.0), Sense::hover()).0;
    glass::button(ui, r, id, label, accent, enabled).clicked()
}

pub(super) fn field(ui: &mut Ui, text: &mut String, placeholder: &str) -> egui::Response {
    let r = ui.allocate_exact_size(vec2(ui.available_width(), 36.0), Sense::hover()).0;
    glass::text_field(ui, r, text, placeholder, false)
}

impl App {
    pub(super) fn business_event(&mut self, ev: BusinessEv) {
        let b = &mut self.business;
        match ev {
            BusinessEv::Loaded(my) => b.my = Some(my),
            BusinessEv::QuickReplies(mut list) => {
                // Previews read as plain text; the markup only matters when editing.
                for q in &mut list {
                    q.top = format::parse(&q.top).0;
                }
                b.msgs.retain(|id, _| list.iter().any(|q| q.id == *id));
                b.quick = list;
            }
            BusinessEv::QuickMessages { shortcut, msgs } => {
                b.msgs.insert(shortcut, msgs);
            }
            BusinessEv::Links(list) => b.links = Some(list),
            BusinessEv::Bots(list) => b.bots = Some(list),
        }
    }

    pub(super) fn open_business(&mut self) {
        self.business.page = Page::Main;
        self.send(Cmd::LoadProfile);
        // Our full info answers without Premium too; quick replies need it.
        self.send(Cmd::Business(BusinessCmd::Load));
        if self.profile.premium {
            self.load_quick_replies();
        }
    }

    fn load_quick_replies(&mut self) {
        self.business.quick_asked = true;
        self.send(Cmd::Business(BusinessCmd::LoadQuickReplies));
    }

    fn open_business_page(&mut self, page: Page) {
        let b = &mut self.business;
        b.armed = None;
        b.new_name.clear();
        match &page {
            Page::Location => b.address = None,
            Page::Hours => b.hours = None,
            Page::Greeting | Page::Away => {
                b.greeting = None;
                b.away = None;
                self.load_quick_replies();
            }
            Page::Links => {
                b.link_edit = None;
                b.link_open = None;
                self.send(Cmd::Business(BusinessCmd::LoadLinks));
            }
            Page::Bots => self.send(Cmd::Business(BusinessCmd::LoadBots)),
            Page::Replies => self.load_quick_replies(),
            Page::Shortcut(name) => {
                self.business.shortcut = messages::ShortcutEdit::named(name);
                if let Some(q) = self.business.quick.iter().find(|q| &q.name == name) {
                    let id = q.id;
                    self.send(Cmd::Business(BusinessCmd::LoadQuickMessages(id)));
                }
            }
            Page::Main => {}
        }
        self.business.page = page;
    }

    /// The title of the settings window while the section is open.
    pub(super) fn business_title(&self) -> String {
        match &self.business.page {
            Page::Shortcut(name) => format!("/{name}"),
            page => features().into_iter().find(|f| &f.page == page).map_or(l!("Telegram Business", "Telegram Business"), |f| f.title).to_owned(),
        }
    }

    /// Back from an inner page; `false` when already on the section's first page.
    pub(super) fn business_back(&mut self) -> bool {
        let to = match &self.business.page {
            Page::Main => return false,
            Page::Shortcut(name) if name == messages::GREETING => Page::Greeting,
            Page::Shortcut(name) if name == messages::AWAY => Page::Away,
            Page::Shortcut(_) => Page::Replies,
            _ => Page::Main,
        };
        self.open_business_page(to);
        true
    }

    pub(super) fn business_page(&mut self, ui: &mut Ui) {
        let page = self.business.page.clone();
        egui::ScrollArea::vertical().id_salt(("business", self.business_title())).auto_shrink(false).show(ui, |ui| match page {
            Page::Main => self.business_main(ui),
            Page::Location => self.location_page(ui),
            Page::Hours => self.hours_page(ui),
            Page::Replies => self.replies_page(ui),
            Page::Shortcut(name) => self.shortcut_page(ui, &name),
            Page::Greeting => self.greeting_page(ui),
            Page::Away => self.away_page(ui),
            Page::Bots => self.bots_page(ui),
            Page::Links => self.chat_links_page(ui),
        });
    }

    fn business_main(&mut self, ui: &mut Ui) {
        let premium = self.profile.premium;
        ui.add_space(8.0);
        let (top, _) = ui.allocate_exact_size(vec2(ui.available_width(), 74.0), Sense::hover());
        let badge = Rect::from_center_size(pos2(top.center().x, top.top() + 30.0), vec2(56.0, 56.0));
        glass::glass(ui.painter(), badge, Glass::tinted(28.0, ACCENT, 150));
        icons::draw(ui.painter(), Rect::from_center_size(badge.center(), vec2(30.0, 30.0)), Icon::Briefcase, ink());
        ui.vertical_centered(|ui| {
            ui.label(egui::RichText::new("Telegram Business").size(20.0).color(ink()));
            let sub = if premium {
                l!("Розвивайте свій бізнес з цими ексклюзивними можливостями.", "Boost your business with these exclusive features.")
            } else {
                l!("Перетворіть свій акаунт на сторінку бізнесу з цими можливостями. Вони доступні з Telegram Premium.", "Turn your account into a business page with these additional features. They are available with Telegram Premium.")
            };
            ui.add(egui::Label::new(egui::RichText::new(sub).color(ink_dim()).size(13.0)).wrap());
        });
        ui.add_space(10.0);
        let mut open = None;
        for f in features() {
            if feature_row(ui, f.icon, f.title, f.about, premium).clicked() {
                open = Some(f.page);
            }
        }
        ui.add_space(10.0);
        if !premium {
            if wide_button(ui, "business-premium", l!("Підписатися на Telegram Premium", "Subscribe to Telegram Premium"), true, true) {
                self.open_link("https://t.me/PremiumBot");
            }
            ui.add_space(6.0);
            hint(ui, l!("Відкриється @PremiumBot, через який оформлюється підписка.", "Opens @PremiumBot, where the subscription is bought."));
        }
        if let Some(page) = open {
            self.open_business_page(page);
        }
    }

    fn location_page(&mut self, ui: &mut Ui) {
        let saved = self.business.my.as_ref().map(|m| m.address.clone());
        let Some(saved) = saved else {
            ui.add(egui::Spinner::new().color(ink_dim()));
            return;
        };
        let draft = self.business.address.get_or_insert_with(|| saved.clone());
        hint(ui, l!("Якщо у вас є магазин чи офіс, куди можуть прийти клієнти, вкажіть його адресу.", "If you have a physical store that customers can visit, share its location."));
        ui.add_space(8.0);
        field(ui, draft, l!("Введіть адресу", "Enter Address"));
        if draft.chars().count() > ADDRESS_MAX {
            *draft = draft.chars().take(ADDRESS_MAX).collect();
        }
        ui.add_space(8.0);
        let changed = draft.trim() != saved;
        let text = draft.clone();
        let mut cmd = None;
        if wide_button(ui, "business-location-save", l!("Зберегти", "Save"), true, changed && !text.trim().is_empty()) {
            cmd = Some(BusinessCmd::SetLocation(text));
        }
        if !saved.is_empty() {
            ui.add_space(6.0);
            if info::danger_row(ui, l!("Видалити розташування", "Remove Location")).clicked() {
                self.business.address = Some(String::new());
                cmd = Some(BusinessCmd::SetLocation(String::new()));
            }
        }
        if let Some(cmd) = cmd {
            self.send(Cmd::Business(cmd));
        }
    }

    /// Address and opening hours in a business's profile.
    pub(super) fn business_rows(&self, ui: &mut Ui, info: &BusinessInfo) {
        if !info.address.is_empty() && info::info_row(ui, Icon::Location, &info.address, l!("Розташування", "Location")).clicked() {
            ui.ctx().copy_text(info.address.clone());
        }
        if let Some(h) = &info.hours {
            hours::profile_hours(ui, h, info.offset);
        }
    }

    /// Quick replies matching what follows "/" in a private chat; asks for them once.
    pub(super) fn quick_reply_items(&mut self, query: &str) -> Vec<QuickReply> {
        if !self.profile.premium {
            return Vec::new();
        }
        if !self.business.quick_asked {
            self.load_quick_replies();
        }
        let q = query.to_lowercase();
        self.business.quick.iter().filter(|r| r.name.to_lowercase().starts_with(&q)).cloned().collect()
    }

    pub(super) fn send_quick_reply(&mut self, peer: PeerId, shortcut: i32) {
        self.draft.clear();
        self.send(Cmd::Business(BusinessCmd::SendQuickReply { peer, shortcut }));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn days_and_overnight() {
        // Monday 9–18, Friday 22:00 – Saturday 02:00, Sunday all day running into Monday 01:00.
        let weekly = normalized(vec![(9 * 60, 18 * 60), (4 * DAY + 22 * 60, 5 * DAY + 2 * 60), (6 * DAY, WEEK), (0, 60)]);
        assert_eq!(weekly, vec![(9 * 60, 18 * 60), (4 * DAY + 22 * 60, 5 * DAY + 2 * 60), (6 * DAY, WEEK + 60)]);
        let lines = day_lines(&weekly);
        assert_eq!(lines[0], vec![Line::Range(0, 60), Line::Range(9 * 60, 18 * 60)]);
        assert_eq!(lines[4], vec![Line::Range(22 * 60, DAY + 2 * 60)]);
        assert!(lines[5].is_empty());
        assert_eq!(lines[6], vec![Line::FullDay]);
        assert_eq!(clock_text(DAY), "24:00");
        assert_eq!(clock_text(DAY + 90), "01:30");
    }
}
