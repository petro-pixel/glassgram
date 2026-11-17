//! Opening hours: the editor in Settings → Telegram Business, and the "Open / Closed" row with the
//! week's table in a business's profile.

use super::*;

/// One day in the editor; open with no intervals means open 24 hours.
#[derive(Clone, Default, PartialEq)]
pub(super) struct DayEdit {
    open: bool,
    /// ("09:00", "18:00"); an end before the start runs past midnight.
    ranges: Vec<(String, String)>,
}

#[derive(Clone, PartialEq)]
pub(in crate::app) struct HoursEdit {
    on: bool,
    tz: String,
    days: [DayEdit; 7],
    tz_open: bool,
    tz_search: String,
}

/// "9", "9:30", "09:30" → minutes; "24:00" is allowed as an end.
fn parse_time(t: &str) -> Option<i32> {
    let t = t.trim();
    let (h, m) = t.split_once(':').unwrap_or((t, "0"));
    let (h, m): (i32, i32) = (h.trim().parse().ok()?, m.trim().parse().ok()?);
    let total = h * 60 + m;
    ((0..60).contains(&m) && (0..=DAY).contains(&total)).then_some(total)
}

impl HoursEdit {
    fn new(h: Option<&WorkHours>, tz: String) -> Self {
        let mut days: [DayEdit; 7] = Default::default();
        if let Some(h) = h {
            for (d, lines) in day_lines(&h.weekly).into_iter().enumerate() {
                days[d].open = !lines.is_empty();
                days[d].ranges = lines
                    .into_iter()
                    .filter_map(|l| match l {
                        Line::FullDay => None,
                        Line::Range(s, e) => Some((clock_text(s), clock_text(e))),
                    })
                    .collect();
            }
        } else {
            // A new schedule starts as weekdays 9 to 18, like a usual shop.
            for day in days.iter_mut().take(5) {
                *day = DayEdit { open: true, ranges: vec![("09:00".into(), "18:00".into())] };
            }
        }
        Self { on: h.is_some(), tz: h.map_or(tz, |h| h.tz.clone()), days, tz_open: false, tz_search: String::new() }
    }

    /// The intervals for the server, or `None` while some time can't be read.
    fn weekly(&self) -> Option<Vec<(i32, i32)>> {
        let mut list = Vec::new();
        for (d, day) in self.days.iter().enumerate() {
            let base = d as i32 * DAY;
            if !day.open {
                continue;
            }
            if day.ranges.is_empty() {
                list.push((base, base + DAY));
            }
            for (s, e) in &day.ranges {
                let (s, mut e) = (parse_time(s)?, parse_time(e)?);
                if s == DAY {
                    return None;
                }
                if e <= s {
                    e += DAY;
                }
                list.push((base + s, base + e));
            }
        }
        Some(normalized(list))
    }
}

/// The zone the system runs in, if the server's list has it; else one with the same offset.
fn system_zone(list: &[crate::backend::Timezone]) -> String {
    let sys = jiff::tz::TimeZone::system();
    let name = sys.iana_name().unwrap_or_default().to_owned();
    if let Some(t) = list.iter().find(|t| t.id == name) {
        return t.id.clone();
    }
    let off = my_offset();
    list.iter().find(|t| t.offset == off).map_or_else(|| "Europe/London".to_owned(), |t| t.id.clone())
}

/// "09:00 – 18:00, 20:00 – 22:00", "Open 24 hours" or "Closed".
fn lines_text(lines: &[Line]) -> String {
    if lines.is_empty() {
        return l!("Зачинено", "Closed").to_owned();
    }
    lines
        .iter()
        .map(|l| match l {
            Line::FullDay => l!("Цілодобово", "Open 24 hours").to_owned(),
            Line::Range(s, e) => format!("{} – {}", clock_text(*s), clock_text(*e)),
        })
        .collect::<Vec<_>>()
        .join(", ")
}

impl App {
    pub(in crate::app) fn hours_page(&mut self, ui: &mut Ui) {
        let Some(my) = self.business.my.clone() else {
            ui.add(egui::Spinner::new().color(ink_dim()));
            return;
        };
        let edit = self.business.hours.get_or_insert_with(|| HoursEdit::new(my.hours.as_ref(), system_zone(&my.timezones)));
        hint(ui, l!("Увімкніть, щоб показувати у профілі графік роботи, а клієнтам — чи ви зараз працюєте.", "Turn this on to show your opening hours schedule to your customers."));
        ui.add_space(6.0);
        if toggle_row(ui, l!("Показувати години роботи", "Show Business Hours"), edit.on, true) {
            edit.on = !edit.on;
        }
        if edit.on {
            ui.add_space(4.0);
            for d in 0..7 {
                day_editor(ui, d, &mut edit.days[d]);
            }
            ui.add_space(8.0);
            tz_picker(ui, edit, &my.timezones);
        }
        ui.add_space(10.0);
        let weekly = edit.weekly();
        let wanted = if edit.on { weekly.clone().map(|weekly| Some(WorkHours { tz: edit.tz.clone(), open_now: false, weekly })) } else { Some(None) };
        let saved = my.hours.clone().map(|h| WorkHours { open_now: false, ..h });
        if weekly.is_none() && edit.on {
            ui.label(egui::RichText::new(l!("Час вводиться як 09:00", "Times look like 09:00")).color(glass::danger()).size(12.5));
        }
        let ready = wanted.as_ref().is_some_and(|w| *w != saved);
        if wide_button(ui, "business-hours-save", l!("Зберегти", "Save"), true, ready)
            && let Some(w) = wanted
        {
            self.send(Cmd::Business(BusinessCmd::SetHours(w)));
        }
    }
}

/// A day's switch, and its intervals when open.
fn day_editor(ui: &mut Ui, d: usize, day: &mut DayEdit) {
    if toggle_row(ui, day_name(d), day.open, true) {
        day.open = !day.open;
    }
    if !day.open {
        return;
    }
    if day.ranges.is_empty() {
        ui.horizontal(|ui| {
            ui.add_space(12.0);
            ui.label(egui::RichText::new(l!("Цілодобово", "Open 24 hours")).color(ink_dim()).size(12.5));
        });
    }
    let mut remove = None;
    for (k, (s, e)) in day.ranges.iter_mut().enumerate() {
        let (row, _) = ui.allocate_exact_size(vec2(ui.available_width(), 38.0), Sense::hover());
        let w = 86.0;
        let a = Rect::from_min_size(pos2(row.left() + 12.0, row.top() + 2.0), vec2(w, 34.0));
        let b = a.translate(vec2(w + 26.0, 0.0));
        glass::text_field(ui, a, s, l!("Відкриття", "Opening"), false);
        ui.painter().text(pos2(a.right() + 13.0, a.center().y), Align2::CENTER_CENTER, "–", FontId::proportional(14.0), ink_dim());
        glass::text_field(ui, b, e, l!("Закриття", "Closing"), false);
        let x = Rect::from_center_size(pos2(b.right() + 26.0, b.center().y), vec2(32.0, 32.0));
        if glass::icon_button(ui, x, ("hours-remove", d, k), Icon::Close, false, true).on_hover_text(l!("Прибрати", "Remove")).clicked() {
            remove = Some(k);
        }
    }
    if let Some(k) = remove {
        day.ranges.remove(k);
    }
    ui.horizontal(|ui| {
        ui.add_space(12.0);
        let r = ui.allocate_exact_size(vec2(200.0, 30.0), Sense::hover()).0;
        if glass::button(ui, r, ("hours-add", d), l!("Додати проміжок", "Add a Set of Hours"), false, true).clicked() {
            let from = day.ranges.last().and_then(|(_, e)| parse_time(e)).map_or(9 * 60, |e| (e + 60).min(DAY - 60));
            day.ranges.push((clock_text(from), clock_text((from + 120).min(DAY))));
        }
    });
    ui.add_space(4.0);
}

fn tz_picker(ui: &mut Ui, edit: &mut HoursEdit, list: &[crate::backend::Timezone]) {
    let name = list.iter().find(|t| t.id == edit.tz).map_or_else(|| edit.tz.clone(), |t| t.name.clone());
    if info::info_row(ui, Icon::Globe, &name, l!("Часовий пояс", "Time Zone")).clicked() {
        edit.tz_open = !edit.tz_open;
    }
    if !edit.tz_open {
        return;
    }
    let r = ui.allocate_exact_size(vec2(ui.available_width(), 34.0), Sense::hover()).0;
    glass::search_field(ui, r, &mut edit.tz_search, l!("Пошук", "Search"));
    let q = edit.tz_search.to_lowercase();
    egui::ScrollArea::vertical().id_salt("tz-list").max_height(220.0).show(ui, |ui| {
        for t in list.iter().filter(|t| q.is_empty() || t.name.to_lowercase().contains(&q) || t.id.to_lowercase().contains(&q)) {
            if info::menu_row(ui, &t.name, t.id == edit.tz).clicked() {
                edit.tz = t.id.clone();
                edit.tz_open = false;
            }
        }
    });
}

/// "Open" or "Closed" with today's hours; a click unfolds the week, in their time or ours.
pub(super) fn profile_hours(ui: &mut Ui, h: &WorkHours, offset: Option<i32>) {
    let id = ui.scope_id().with("business-week");
    let (mut unfolded, mut mine) = ui.data(|d| d.get_temp::<(bool, bool)>(id)).unwrap_or_default();
    let mine_shown = mine && offset.is_some_and(|o| o != my_offset());
    let shift = if mine_shown { offset.map_or(0, |o| (my_offset() - o) / 60) } else { 0 };
    let shown: Vec<(i32, i32)> = h.weekly.iter().map(|&(s, e)| (s + shift, e + shift)).collect();
    let lines = day_lines(&shown);
    // Without the zone, our own day is the best guess for "today".
    let zone = if mine_shown { my_offset() } else { offset.unwrap_or_else(my_offset) };
    let today = (week_minute(zone) / DAY) as usize;
    let open = open_now(h, offset);
    let status = if open { l!("Відчинено", "Open") } else { l!("Зачинено", "Closed") };
    let caption = lf!("Години роботи · сьогодні {}", "Business hours · today {}", lines_text(&lines[today]));
    let w = ui.available_width();
    let (r, resp) = ui.allocate_exact_size(vec2(w, 52.0), Sense::click());
    let p = ui.painter();
    if resp.hovered() {
        p.rect_filled(r, 16.0, glass::veil(14));
        ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
    }
    icons::draw(p, Rect::from_center_size(pos2(r.left() + 26.0, r.top() + 22.0), vec2(20.0, 20.0)), Icon::Clock, ink_dim());
    let color = if open { glass::positive() } else { glass::danger() };
    p.text(pos2(r.left() + 52.0, r.top() + 9.0), Align2::LEFT_TOP, status, FontId::proportional(14.0), color);
    let chevron = if unfolded { Icon::ChevronUp } else { Icon::ChevronDown };
    icons::draw(p, Rect::from_center_size(pos2(r.right() - 20.0, r.top() + 18.0), vec2(16.0, 16.0)), chevron, ink_dim());
    p.galley(pos2(r.left() + 52.0, r.top() + 29.0), elided(ui, &caption, 12.5, ink_dim(), w - 64.0), ink_dim());
    if resp.clicked() {
        unfolded = !unfolded;
    }
    if unfolded {
        if offset.is_some_and(|o| o != my_offset()) {
            ui.horizontal(|ui| {
                ui.add_space(52.0);
                ui.spacing_mut().item_spacing.x = 6.0;
                if info::chip(ui, l!("Місцевий час", "Local time"), !mine).clicked() {
                    mine = false;
                }
                if info::chip(ui, l!("Мій час", "My time"), mine).clicked() {
                    mine = true;
                }
            });
            ui.add_space(4.0);
        }
        for (d, day) in lines.iter().enumerate() {
            let (r, _) = ui.allocate_exact_size(vec2(ui.available_width(), 26.0), Sense::hover());
            let color = if d == today { ink() } else { ink_dim() };
            let font = if d == today { FontId::new(13.5, egui::FontFamily::Name(format::BOLD.into())) } else { FontId::proportional(13.5) };
            let p = ui.painter();
            p.text(pos2(r.left() + 52.0, r.center().y), Align2::LEFT_CENTER, day_name(d), font.clone(), color);
            p.text(pos2(r.right() - 12.0, r.center().y), Align2::RIGHT_CENTER, lines_text(day), font, color);
        }
        if let Some(o) = offset {
            let (r, _) = ui.allocate_exact_size(vec2(ui.available_width(), 22.0), Sense::hover());
            let zone = if mine_shown { offset_text(my_offset()) } else { format!("{} · {}", h.tz, offset_text(o)) };
            ui.painter().text(pos2(r.left() + 52.0, r.center().y), Align2::LEFT_CENTER, zone, FontId::proportional(11.5), ink_dim());
        }
        ui.add_space(4.0);
    }
    ui.data_mut(|d| d.insert_temp(id, (unfolded, mine)));
}
