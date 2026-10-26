//! "Jump to date": a month calendar over the chat, as in Telegram Desktop. Opened from the
//! calendar button in chat search or by clicking a date chip in history.

use jiff::civil::Date;
use jiff::tz::TimeZone;

use super::*;

const CELL: f32 = 40.0;
const PAD: f32 = 18.0;
const HEAD_H: f32 = 56.0;
const WEEK_H: f32 = 24.0;

pub(super) struct Calendar {
    peer: PeerId,
    /// First day of the shown month.
    month: Date,
    /// Photos and videos per local day, as Telegram Desktop marks them; months already asked for.
    media: HashMap<Date, i32>,
    asked: HashSet<Date>,
}

fn month_name(m: i8) -> &'static str {
    match m {
        1 => l!("Січень", "January"),
        2 => l!("Лютий", "February"),
        3 => l!("Березень", "March"),
        4 => l!("Квітень", "April"),
        5 => l!("Травень", "May"),
        6 => l!("Червень", "June"),
        7 => l!("Липень", "July"),
        8 => l!("Серпень", "August"),
        9 => l!("Вересень", "September"),
        10 => l!("Жовтень", "October"),
        11 => l!("Листопад", "November"),
        _ => l!("Грудень", "December"),
    }
}

fn weekdays() -> [&'static str; 7] {
    [l!("Пн", "Mo"), l!("Вт", "Tu"), l!("Ср", "We"), l!("Чт", "Th"), l!("Пт", "Fr"), l!("Сб", "Sa"), l!("Нд", "Su")]
}

fn today() -> Date {
    jiff::Zoned::now().date()
}

impl App {
    /// `ts`: a message time to start from (its month); `None` — this month.
    pub(super) fn open_calendar(&mut self, peer: PeerId, ts: Option<i64>) {
        let day = ts.and_then(|t| jiff::Timestamp::from_second(t).ok()).map_or_else(today, |t| t.to_zoned(TimeZone::system()).date());
        self.calendar = Some(Calendar { peer, month: day.first_of_month(), media: HashMap::new(), asked: HashSet::new() });
        self.ask_media_days();
    }

    /// The server answers with the periods before a date, so ask from the end of the shown month.
    fn ask_media_days(&mut self) {
        let Some(c) = &mut self.calendar else { return };
        if !c.asked.insert(c.month) {
            return;
        }
        let end = c.month.checked_add(jiff::Span::new().months(1)).ok().and_then(|d| d.to_zoned(TimeZone::system()).ok());
        if let Some(end) = end {
            let (peer, before) = (c.peer, end.timestamp().as_second() as i32);
            self.send(Cmd::LoadMediaDays { peer, before });
        }
    }

    pub(super) fn media_days(&mut self, peer: PeerId, days: Vec<(i32, i32)>) {
        let Some(c) = self.calendar.as_mut().filter(|c| c.peer == peer) else { return };
        for (ts, n) in days {
            if let Ok(t) = jiff::Timestamp::from_second(ts.into()) {
                *c.media.entry(t.to_zoned(TimeZone::system()).date()).or_default() += n;
            }
        }
    }

    pub(super) fn date_found(&mut self, peer: PeerId, id: Option<i32>) {
        match id {
            Some(id) if self.selected == Some(peer) => self.jump_in_chat(peer, id),
            Some(_) => {}
            None => self.toast = Some((l!("У цьому чаті ще немає повідомлень", "There are no messages in this chat yet").into(), 0.0, false)),
        }
    }

    pub(super) fn calendar_view(&mut self, ui: &mut Ui, peer: PeerId, rect: Rect) {
        let Some(cal) = &self.calendar else { return };
        if cal.peer != peer || ui.input_mut(|i| i.consume_key(egui::Modifiers::NONE, Key::Escape)) {
            self.calendar = None;
            return;
        }
        let month = cal.month;
        let size = vec2(7.0 * CELL + PAD * 2.0, HEAD_H + WEEK_H + 6.0 * CELL + PAD);
        let panel = Rect::from_center_size(rect.center(), size);
        glass::glass(ui.painter(), panel, Glass::panel(24.0).with_shadow());
        let p = ui.painter().clone();

        // Header: ‹ Month year › and close.
        let cy = panel.top() + HEAD_H / 2.0;
        let title = format!("{} {}", month_name(month.month()), month.year());
        p.text(pos2(panel.center().x - 18.0, cy), Align2::CENTER_CENTER, title, FontId::proportional(16.0), ink());
        let btn = |x: f32| Rect::from_center_size(pos2(x, cy), vec2(34.0, 34.0));
        let mut shift = 0;
        if glass::icon_button(ui, btn(panel.left() + PAD + 17.0), "cal-prev", Icon::ChevronLeft, false, true).on_hover_text(l!("Попередній місяць", "Previous month")).clicked() {
            shift = -1;
        }
        let has_next = month < today().first_of_month();
        if glass::icon_button(ui, btn(panel.right() - PAD - 17.0 - 40.0), "cal-next", Icon::ChevronRight, false, has_next).on_hover_text(l!("Наступний місяць", "Next month")).clicked() {
            shift = 1;
        }
        if glass::icon_button(ui, btn(panel.right() - PAD - 17.0), "cal-close", Icon::Close, false, true).clicked() {
            self.calendar = None;
            return;
        }

        let grid = pos2(panel.left() + PAD, panel.top() + HEAD_H);
        for (k, name) in weekdays().iter().enumerate() {
            p.text(pos2(grid.x + (k as f32 + 0.5) * CELL, grid.y + WEEK_H / 2.0), Align2::CENTER_CENTER, *name, FontId::proportional(12.0), ink_dim());
        }
        let offset = month.weekday().to_monday_zero_offset() as i32;
        let today = today();
        let mut picked = None;
        for d in 1..=month.days_in_month() {
            let cell = offset + d as i32 - 1;
            let (row, col) = (cell / 7, cell % 7);
            let r = Rect::from_min_size(pos2(grid.x + col as f32 * CELL, grid.y + WEEK_H + row as f32 * CELL), vec2(CELL, CELL)).shrink(2.0);
            let date = month.with().day(d).build().unwrap_or(month);
            let future = date > today;
            let resp = ui.interact(r, ui.scope_id().with(("cal-day", date)), if future { Sense::hover() } else { Sense::click() });
            if resp.hovered() && !future {
                p.rect_filled(r, r.height() / 2.0, glass::veil(24));
                ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
            }
            if date == today {
                p.rect_stroke(r, r.height() / 2.0, egui::Stroke::new(1.5, ACCENT), egui::StrokeKind::Inside);
            }
            let color = if future { glass::rgba(ink(), 70) } else { ink() };
            p.text(r.center(), Align2::CENTER_CENTER, d.to_string(), FontId::proportional(14.0), color);
            // A dot under days with photos or videos.
            if let Some(n) = cal.media.get(&date) {
                p.circle_filled(pos2(r.center().x, r.bottom() - 5.0), 2.5, ACCENT);
                if resp.hovered() {
                    resp.clone().on_hover_text(lf!("Фото й відео: {n}", "Photos and videos: {n}"));
                }
            }
            if resp.clicked() {
                picked = Some(date);
            }
        }

        if shift != 0
            && let Some(c) = &mut self.calendar
            && let Ok(m) = c.month.checked_add(jiff::Span::new().months(shift))
        {
            c.month = m;
            self.ask_media_days();
        }
        if let Some(date) = picked {
            self.calendar = None;
            match date.to_zoned(TimeZone::system()) {
                Ok(z) => self.send(Cmd::JumpToDate { peer, date: z.timestamp().as_second() as i32 }),
                Err(e) => self.toast = Some((e.to_string(), 0.0, true)),
            }
        }
    }
}
