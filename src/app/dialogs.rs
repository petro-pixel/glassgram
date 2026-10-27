//! Boxes over the chat: export, forward to, schedule a message, the scheduled list.

use super::*;

impl App {
    /// Export confirmation: with media or text only.
    pub(super) fn export_view(&mut self, ui: &mut Ui) {
        let Some((peer, media)) = &mut self.export_ask else { return };
        let peer = *peer;
        let title = self.chats.iter().find(|c| c.id == peer).map(|c| c.title.clone()).unwrap_or_default();
        let screen = ui.max_rect();
        ui.painter().rect_filled(screen, 0.0, Color32::from_black_alpha(90));
        ui.interact(screen, ui.scope_id().with("export-bg"), Sense::click());
        let rect = Rect::from_center_size(screen.center(), vec2(380.0, 200.0));
        glass::glass(ui.painter(), rect, Glass::panel(24.0).with_shadow());
        let inner = rect.shrink(18.0);
        ui.painter().text(inner.left_top(), Align2::LEFT_TOP, l!("Експорт історії", "Export chat history"), FontId::proportional(17.0), ink());
        ui.painter().galley(inner.left_top() + vec2(0.0, 28.0), elided(ui, &title, 14.0, ink_dim(), inner.width()), ink());
        let mut child = ui.new_child(UiBuilder::new().max_rect(Rect::from_min_size(inner.left_top() + vec2(0.0, 58.0), vec2(inner.width(), 60.0))));
        child.checkbox(media, l!("З фото й файлами (довше)", "Include photos and files (slower)"));
        child.label(egui::RichText::new(l!("HTML і JSON у теці «Завантаження/Glassgram Export»", "HTML and JSON in Downloads/Glassgram Export")).color(ink_dim()).size(12.5));
        let media = *media;
        let half = (inner.width() - 8.0) / 2.0;
        let y = inner.bottom() - 34.0;
        let cancel = glass::button(ui, Rect::from_min_size(pos2(inner.left(), y), vec2(half, 34.0)), "export-cancel", l!("Скасувати", "Cancel"), false, true).clicked();
        let busy = self.exporting.contains(&peer);
        let go = glass::button(ui, Rect::from_min_size(pos2(inner.right() - half, y), vec2(half, 34.0)), "export-go", l!("Експортувати", "Export"), true, !busy).clicked();
        if go && !busy {
            self.exporting.insert(peer);
            self.export_ask = None;
            self.toast = Some((l!("Експорт почався…", "Export started…").into(), 0.0, false));
            self.send(Cmd::ExportChat { peer, title, media });
        } else if cancel || ui.input(|i| i.key_pressed(Key::Escape)) {
            self.export_ask = None;
        }
    }

    /// "Forward to…" window: search and chat list.
    pub(super) fn forward_view(&mut self, ui: &mut Ui, rect: Rect) {
        let Some(fw) = &mut self.forward else { return };
        // Dim the background and swallow clicks under the window.
        ui.painter().rect_filled(ui.max_rect(), 0.0, Color32::from_black_alpha(90));
        ui.interact(ui.max_rect(), ui.scope_id().with("forward-bg"), Sense::click());
        glass::glass(ui.painter(), rect, Glass::panel(26.0).with_shadow());
        let inner = rect.shrink(16.0);
        let title = if fw.reply { l!("Відповісти в…", "Reply in…").to_owned() } else { lf!("Переслати ({})", "Forward ({})", fw.ids.len()) };
        ui.painter().text(inner.left_top() + vec2(4.0, 2.0), Align2::LEFT_TOP, title, FontId::proportional(18.0), ink());
        let close = Rect::from_min_size(pos2(inner.right() - 32.0, inner.top() - 4.0), vec2(32.0, 32.0));
        let mut cancel = glass::icon_button(ui, close, "fw-close", Icon::Close, false, true).clicked();
        let field = Rect::from_min_size(inner.left_top() + vec2(0.0, 38.0), vec2(inner.width(), 38.0));
        let hint = if fw.reply { l!("Пошук чату…", "Search chats…") } else { l!("Кому переслати…", "Forward to…") };
        let resp = glass::text_field(ui, field, &mut fw.query, hint, false);
        if !resp.has_focus() && fw.query.is_empty() {
            resp.request_focus();
        }
        let list = Rect::from_min_max(pos2(inner.left() - 6.0, field.bottom() + 8.0), pos2(inner.right() + 6.0, inner.bottom()));
        let q = fw.query.trim().to_lowercase();
        let mut chosen = None;
        let mut child = ui.new_child(UiBuilder::new().max_rect(list));
        child.set_clip_rect(list);
        egui::ScrollArea::vertical().id_salt("forward-list").auto_shrink(false).show(&mut child, |ui| {
            ui.spacing_mut().item_spacing.y = 2.0;
            for c in self.chats.iter().filter(|c| c.title.to_lowercase().contains(&q) && c.kind != ChatKind::Channel) {
                let hit = SearchHit { chat: c.title.clone(), kind: c.kind, msg: Msg::placeholder(c.id) };
                if hit_row(ui, &hit, "", self.textures.get(&ImgKey::Avatar(c.id))).clicked() {
                    chosen = Some(c.id);
                }
            }
        });
        if let Some(to) = chosen
            && let Some(fw) = self.forward.take()
        {
            match (fw.reply, fw.ids.first()) {
                (true, Some(id)) => self.reply_elsewhere(fw.from, *id, to),
                _ => {
                    self.send(Cmd::Forward { from: fw.from, ids: fw.ids, to });
                    self.open_chat(to);
                }
            }
            cancel = false;
        }
        if cancel {
            self.forward = None;
        }
    }

    /// Scheduled send time picker.
    pub(super) fn schedule_view(&mut self, ui: &mut Ui, rect: Rect) {
        glass::glass(ui.painter(), rect, Glass::tinted(22.0, glass::chrome(), 150).frosted(1.0).with_shadow());
        ui.interact(rect, ui.scope_id().with("schedule-bg"), Sense::click());
        let inner = rect.shrink(14.0);
        let title = match self.schedule {
            Some((ScheduleFor::Existing(_), _)) => l!("Новий час надсилання", "New send time"),
            _ => l!("Надіслати пізніше", "Send later"),
        };
        ui.painter().text(inner.left_top(), Align2::LEFT_TOP, title, FontId::proportional(15.5), ink());
        let now = jiff::Zoned::now();
        let at = |d: jiff::civil::Date, h: i8, m: i8| d.at(h, m, 0, 0).to_zoned(now.time_zone().clone()).ok().map(|z| z.timestamp().as_second());
        let tomorrow = now.date().tomorrow().unwrap_or(now.date());
        let presets = [
            (l!("Через годину", "In an hour").to_owned(), Some(now.timestamp().as_second() + 3600)),
            (l!("Сьогодні о 21:00", "Today at 21:00").to_owned(), at(now.date(), 21, 0).filter(|t| *t > now.timestamp().as_second() + 60)),
            (l!("Завтра о 9:00", "Tomorrow at 9:00").to_owned(), at(tomorrow, 9, 0)),
        ];
        let mut chosen = None;
        let mut y = inner.top() + 30.0;
        for (i, (label, ts)) in presets.into_iter().enumerate() {
            let Some(ts) = ts else { continue };
            let r = Rect::from_min_size(pos2(inner.left(), y), vec2(inner.width(), 34.0));
            if glass::button(ui, r, ("sched", i), &label, false, true).clicked() {
                chosen = Some(ts);
            }
            y += 40.0;
        }
        let field = Rect::from_min_size(pos2(inner.left(), y + 4.0), vec2(inner.width() - 96.0, 36.0));
        let Some((target, text)) = &mut self.schedule else { return };
        let target = *target;
        glass::text_field(ui, field, text, l!("ГГ:ХХ", "HH:MM"), false);
        // Time without a date: today, or tomorrow if it has already passed.
        let parsed = text.trim().split_once(':').and_then(|(h, m)| Some((h.parse::<i8>().ok()?, m.parse::<i8>().ok()?)));
        let custom = parsed.and_then(|(h, m)| {
            let today = at(now.date(), h, m)?;
            if today > now.timestamp().as_second() + 60 { Some(today) } else { at(tomorrow, h, m) }
        });
        let ok = Rect::from_min_size(pos2(field.right() + 8.0, field.top()), vec2(88.0, 36.0));
        if glass::button(ui, ok, "sched-ok", l!("Гаразд", "OK"), true, custom.is_some()).clicked() {
            chosen = custom;
        }
        if let Some(ts) = chosen {
            self.schedule = None;
            match (target, self.selected) {
                (ScheduleFor::Draft, _) => self.send_draft_with(SendOpts { silent: false, schedule: Some(ts), ..Default::default() }),
                (ScheduleFor::Existing(id), Some(peer)) => self.send(Cmd::Reschedule { peer, id, at: ts }),
                _ => {}
            }
        }
    }

    /// Scheduled messages list: send now, change time, delete.
    pub(super) fn scheduled_view(&mut self, ui: &mut Ui, peer: PeerId, rect: Rect) {
        let Some(list) = self.scheduled.get(&peer) else { return };
        let rows = list.len() as f32 * 62.0 + 46.0;
        let rect = Rect::from_min_max(pos2(rect.left(), (rect.bottom() - rows).max(rect.top())), rect.max);
        glass::glass(ui.painter(), rect, Glass::tinted(22.0, glass::chrome(), 160).frosted(1.0).with_shadow());
        ui.interact(rect, ui.scope_id().with("scheduled-bg"), Sense::click());
        let inner = rect.shrink2(vec2(12.0, 12.0));
        ui.painter().text(inner.left_top() + vec2(4.0, 0.0), Align2::LEFT_TOP, l!("Заплановані повідомлення", "Scheduled messages"), FontId::proportional(15.0), ink());
        let area = Rect::from_min_max(pos2(inner.left(), inner.top() + 28.0), inner.max);
        let mut cmd = None;
        let mut child = ui.new_child(UiBuilder::new().max_rect(area));
        child.set_clip_rect(area);
        egui::ScrollArea::vertical().id_salt(("scheduled", peer)).auto_shrink(false).show(&mut child, |ui| {
            for m in list {
                let w = ui.available_width();
                let (r, _) = ui.allocate_exact_size(vec2(w, 58.0), Sense::hover());
                let p = ui.painter();
                p.rect_filled(r.shrink(2.0), 14.0, glass::veil(10));
                icons::draw(p, Rect::from_center_size(pos2(r.left() + 22.0, r.center().y), vec2(18.0, 18.0)), Icon::Clock, ACCENT);
                let when = format!("{} {}", m.day, m.time);
                p.text(pos2(r.left() + 42.0, r.top() + 10.0), Align2::LEFT_TOP, when, FontId::proportional(12.5), ACCENT);
                let text = elided(ui, &m.text.replace('\n', " "), 14.0, ink(), w - 42.0 - 140.0);
                ui.painter().galley(pos2(r.left() + 42.0, r.top() + 29.0), text, ink());
                let b = |i: f32| Rect::from_center_size(pos2(r.right() - 24.0 - i * 42.0, r.center().y), vec2(34.0, 34.0));
                if glass::icon_button(ui, b(2.0), ("sch-now", m.id), Icon::Send, false, true).on_hover_text(l!("Надіслати зараз", "Send now")).clicked() {
                    cmd = Some(Cmd::SendScheduledNow { peer, ids: vec![m.id] });
                }
                if glass::icon_button(ui, b(1.0), ("sch-time", m.id), Icon::Clock, false, true).on_hover_text(l!("Змінити час", "Change time")).clicked() {
                    self.schedule = Some((ScheduleFor::Existing(m.id), String::new()));
                }
                if glass::icon_button(ui, b(0.0), ("sch-del", m.id), Icon::Trash, false, true).on_hover_text(l!("Видалити", "Delete")).clicked() {
                    cmd = Some(Cmd::DeleteScheduled { peer, ids: vec![m.id] });
                }
            }
        });
        if let Some(cmd) = cmd {
            self.send(cmd);
        }
        if matches!(self.schedule, Some((ScheduleFor::Existing(_), _))) {
            let panel = Rect::from_min_max(pos2((rect.right() - 300.0).max(rect.left()), rect.top() - 244.0), pos2(rect.right(), rect.top() - 8.0));
            self.schedule_view(ui, panel);
        }
    }
}

impl App {
    /// Export progress and the scheduled list.
    pub(super) fn dialogs_event(&mut self, ev: Ev) {
        match ev {
            Ev::Scheduled { peer, msgs } => {
                if msgs.is_empty() && self.selected == Some(peer) {
                    self.scheduled_open = false;
                }
                self.scheduled.insert(peer, msgs);
            }
            Ev::ExportProgress { peer, done } => {
                let t = self.chats.iter().find(|c| c.id == peer).map(|c| c.title.clone()).unwrap_or_default();
                self.toast = Some((lf!("Експорт «{t}»: {done} повідомлень…", "Exporting “{t}”: {done} messages…"), 0.0, false));
            }
            Ev::Exported { peer, dir, count } => {
                self.exporting.remove(&peer);
                self.toast = Some((lf!("Експортовано {count} повідомлень", "Exported {count} messages"), 0.0, false));
                if count > 0 {
                    crate::platform::open(&dir);
                }
            }
            _ => {}
        }
    }
}
