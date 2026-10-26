//! Drawing messages in history: bubbles, albums, media cards, stickers, round videos, reactions, bot buttons and their menus.

use super::*;

/// Date between messages; a click opens the calendar, as in Telegram Desktop.
pub(super) fn day_chip(ui: &mut Ui, day: &str) -> egui::Response {
    let w = ui.available_width();
    let (r, _) = ui.allocate_exact_size(vec2(w, 40.0), Sense::hover());
    let g = elided(ui, day, 12.5, ink(), 300.0);
    let chip = Rect::from_center_size(r.center(), g.size() + vec2(24.0, 12.0));
    glass::glass(ui.painter(), chip, Glass::tinted(chip.height() / 2.0, Color32::WHITE, 26).frosted(1.0));
    ui.painter().galley(chip.center() - g.size() / 2.0, g, ink());
    let resp = ui.interact(chip, ui.scope_id().with(("day-chip", day)), Sense::click());
    if resp.hovered() {
        ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
    }
    resp.on_hover_text(l!("Перейти до дати", "Jump to date"))
}

/// Message bubble: [photo] + [author] [reply quote] [text] [time].
pub(super) fn message_menu(ui: &mut Ui, m: &Msg, chat: Option<&ChatInfo>, action: &mut Option<Action>) {
    // Protected content: no copying, saving or forwarding, as in Telegram.
    let protected = m.noforwards || chat.is_some_and(|c| c.noforwards);
    ui.set_min_width(170.0);
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = 2.0;
        for e in emoji::QUICK_REACTIONS.iter().take(6) {
            if ui.add(egui::Button::new(egui::RichText::new(*e).size(18.0)).frame(false)).clicked() {
                *action = Some(Action::React(m.id, (*e).to_owned()));
            }
        }
    });
    ui.separator();
    if glass::menu_button(ui, Icon::Reply, l!("Відповісти", "Reply")).clicked() {
        *action = Some(Action::Reply(m.id));
    }
    if !protected && !m.text.is_empty() && glass::menu_button(ui, Icon::Copy, l!("Копіювати текст", "Copy text")).clicked() {
        *action = Some(Action::Copy(m.text.clone()));
    }
    if !m.out && m.id > 0 && !m.text.is_empty() && glass::menu_button(ui, Icon::Translate, l!("Перекласти", "Translate")).clicked() {
        *action = Some(Action::Translate(m.id));
    }
    if let Some(c) = chat {
        chatview::pin_menu(ui, m, c, action);
    }
    if !protected && (m.photo || m.file.is_some() || m.media.is_some()) && glass::menu_button(ui, Icon::Download, l!("Завантажити", "Download")).clicked() {
        *action = Some(Action::Download(m.id));
    }
    if m.out && !m.photo && m.sticker.is_none() && m.file.is_none() && m.media.is_none() && glass::menu_button(ui, Icon::Pencil, l!("Редагувати", "Edit")).clicked() {
        *action = Some(Action::Edit(m.id));
    }
    if !protected
        && m.media.as_ref().is_some_and(|i| matches!(i.kind, MediaKind::Video | MediaKind::Gif | MediaKind::Round))
        && glass::menu_button(ui, Icon::PlayCircle, l!("Відкрити в програвачі", "Open in player")).clicked()
    {
        *action = Some(Action::OpenExternal(m.id));
    }
    if !protected && glass::menu_button(ui, Icon::Forward, l!("Переслати", "Forward")).clicked() {
        *action = Some(Action::Forward(m.id));
    }
    if glass::menu_button(ui, Icon::Select, l!("Вибрати", "Select")).clicked() {
        *action = Some(Action::Pick(m.id));
    }
    if m.out && chat.is_some_and(|c| c.kind == ChatKind::Group) && glass::menu_button(ui, Icon::Eye, l!("Хто переглянув", "Seen by")).clicked() {
        *action = Some(Action::ReadBy(m.id));
    }
    if m.out && glass::menu_button(ui, Icon::Trash, l!("Видалити", "Delete")).clicked() {
        *action = Some(Action::Delete(m.id));
    }
    // Other people's messages in groups and channels, as in Telegram Desktop.
    let public = chat.is_some_and(|c| matches!(c.kind, ChatKind::Group | ChatKind::Channel));
    if !m.out && m.id > 0 && public && glass::menu_button(ui, Icon::Flag, l!("Поскаржитися", "Report")).clicked() {
        *action = Some(Action::Report(m.id));
    }
}

/// Selection mark next to a message and row highlight.
pub(super) fn pick_mark(ui: &Ui, row: Rect, look: Look) {
    if !look.picking {
        return;
    }
    let p = ui.painter();
    if look.picked {
        p.rect_filled(row, 12.0, rgba(ACCENT, 40));
    }
    let c = pos2(row.left() + 16.0, row.center().y);
    if look.picked {
        p.circle_filled(c, 10.0, ACCENT);
        icons::draw(p, Rect::from_center_size(c, vec2(13.0, 13.0)), Icon::Check, Color32::WHITE);
    } else {
        p.circle_stroke(c, 9.5, egui::Stroke::new(1.5, rgba(ink(), 160)));
    }
    ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
}

/// Bot buttons under the bubble.
pub(super) fn bot_buttons(ui: &mut Ui, m: &Msg, x: f32, width: f32) -> Option<Action> {
    let mut action = None;
    for (i, row) in m.buttons.iter().enumerate() {
        let (r, _) = ui.allocate_exact_size(vec2(ui.available_width(), 38.0), Sense::hover());
        let w = (width - 4.0 * (row.len() as f32 - 1.0)) / row.len().max(1) as f32;
        for (j, b) in row.iter().enumerate() {
            let br = Rect::from_min_size(pos2(x + j as f32 * (w + 4.0), r.top() + 2.0), vec2(w, 34.0));
            let label = match b.action {
                ButtonAction::Url(_) => format!("{} ↗", b.text),
                _ => b.text.clone(),
            };
            let resp = glass::button(ui, br, ("bot-btn", m.id, i, j), &label, false, true);
            if resp.clicked() {
                action = Some(Action::Button(m.id, i, j));
            }
        }
    }
    action
}

/// Sticker: no bubble, as in Telegram, with the time in a small glass chip.
pub(super) fn sticker_bubble(ui: &mut Ui, m: &Msg, emoji: &str, tex: Option<&TextureHandle>, look: Look) -> Option<Action> {
    const SIZE: f32 = 160.0;
    let avail = ui.available_width();
    let (row, resp) = ui.allocate_exact_size(vec2(avail, SIZE + 12.0), Sense::click());
    pick_mark(ui, row, look);
    if look.picking && resp.clicked() {
        return Some(Action::Pick(m.id));
    }
    let x = if m.out { row.right() - 10.0 - SIZE } else { row.left() + 10.0 + if look.picking { 30.0 } else { 0.0 } };
    let r = Rect::from_min_size(pos2(x, row.top() + 4.0), vec2(SIZE, SIZE));
    let p = ui.painter();
    match tex {
        Some(t) => glass::image_contain(p, r, t),
        None => {
            p.text(r.center(), Align2::CENTER_CENTER, emoji, FontId::proportional(84.0), ink());
        }
    }
    let meta = p.layout_no_wrap(m.time.clone(), FontId::proportional(11.0), ink());
    let chip = Rect::from_min_size(r.max - meta.size() - vec2(12.0, 8.0), meta.size() + vec2(12.0, 6.0));
    glass::glass(p, chip, Glass::tinted(chip.height() / 2.0, glass::chrome(), 120).frosted(1.0));
    p.galley(chip.min + vec2(6.0, 3.0), meta, ink());
    let mut action = None;
    if !m.reactions.is_empty() {
        action = reaction_chips(ui, m.id, &m.reactions, r.left(), m.out, r.right());
    }
    resp.context_menu(|ui| message_menu(ui, m, look.chat, &mut action));
    if resp.double_clicked() {
        action = Some(Action::Reply(m.id));
    }
    action
}

/// Reactions row under a message.
pub(super) fn reaction_chips(ui: &mut Ui, id: i32, reactions: &[Reaction], left: f32, out: bool, right: f32) -> Option<Action> {
    let (row, _) = ui.allocate_exact_size(vec2(ui.available_width(), 32.0), Sense::hover());
    let chips: Vec<_> = reactions
        .iter()
        .map(|r| (r, ui.painter().layout_no_wrap(format!("{} {}", r.emoji, r.count), FontId::proportional(14.0), ink())))
        .collect();
    let total: f32 = chips.iter().map(|(_, g)| g.size().x + 22.0).sum();
    let mut x = if out { right - total } else { left };
    let mut action = None;
    for (r, g) in chips {
        let w = g.size().x + 18.0;
        let rect = Rect::from_min_size(pos2(x, row.top() + 2.0), vec2(w, 28.0));
        let resp = ui.interact(rect, ui.scope_id().with(("react", id, &r.emoji)), Sense::click());
        let alpha = if r.mine { 190 } else if resp.hovered() { 60 } else { 34 };
        glass::glass(ui.painter(), rect, Glass::tinted(14.0, if r.mine { ACCENT } else { Color32::WHITE }, alpha));
        ui.painter().galley(rect.center() - g.size() / 2.0, g, ink());
        if resp.clicked() {
            action = Some(Action::React(id, r.emoji.clone()));
        }
        x += w + 4.0;
    }
    action
}

/// Height of the poll / location / contact card (0 means no card).
pub(super) fn card_height(ui: &Ui, m: &Msg, w: f32, chosen: &[Vec<u8>]) -> f32 {
    if m.media.as_ref().is_some_and(|i| matches!(i.kind, MediaKind::Voice | MediaKind::Audio)) {
        return MEDIA_H;
    }
    if let Some(poll) = &m.poll {
        let q = ui.painter().layout(poll.question.clone(), FontId::new(15.0, egui::FontFamily::Name(format::BOLD.into())), ink(), w);
        let button = poll.multiple && !poll.voted() && !poll.closed && !chosen.is_empty();
        return q.size().y + 22.0 + poll.answers.len() as f32 * 40.0 + 20.0 + if button { 40.0 } else { 0.0 };
    }
    if m.geo.is_some() {
        return 150.0;
    }
    if m.contact.is_some() {
        return 52.0;
    }
    0.0
}

pub(super) fn draw_card(ui: &mut Ui, m: &Msg, rect: Rect, look: Look) -> Option<Action> {
    let chosen = look.chosen;
    let p = ui.painter().clone();
    let mut action = None;
    if m.media.as_ref().is_some_and(|i| matches!(i.kind, MediaKind::Voice | MediaKind::Audio)) {
        return media_card(ui, m, rect, look.play);
    }
    if let Some(poll) = &m.poll {
        let bold = FontId::new(15.0, egui::FontFamily::Name(format::BOLD.into()));
        let q = p.layout(poll.question.clone(), bold, ink(), rect.width());
        let mut y = rect.top();
        let qh = q.size().y;
        p.galley(pos2(rect.left(), y), q, ink());
        y += qh + 2.0;
        let kind = match (poll.quiz, poll.multiple) {
            (true, _) => l!("Вікторина", "Quiz"),
            (_, true) => l!("Кілька відповідей", "Multiple answers"),
            _ => l!("Опитування", "Poll"),
        };
        p.text(pos2(rect.left(), y), Align2::LEFT_TOP, kind, FontId::proportional(12.0), rgba(ink(), 160));
        y += 20.0;
        let show = poll.voted() || poll.closed;
        for a in &poll.answers {
            let r = Rect::from_min_size(pos2(rect.left(), y), vec2(rect.width(), 36.0));
            if show {
                let pct = if poll.total > 0 { a.voters as f32 / poll.total as f32 } else { 0.0 };
                p.text(pos2(r.left(), r.top() + 2.0), Align2::LEFT_TOP, format!("{:.0}%", pct * 100.0), FontId::proportional(13.0), ink());
                let label = format!("{}{}", a.text, if a.chosen { "  ✓" } else { "" });
                p.galley(pos2(r.left() + 44.0, r.top() + 2.0), elided(ui, &label, 14.0, ink(), r.width() - 44.0), ink());
                let bar = Rect::from_min_size(pos2(r.left() + 44.0, r.bottom() - 9.0), vec2((r.width() - 44.0) * pct.max(0.02), 4.0));
                let col = if poll.quiz && a.correct { Color32::from_rgb(70, 220, 120) } else if a.chosen { ink() } else { rgba(ink(), 140) };
                p.rect_filled(bar, 2.0, col);
            } else {
                let resp = ui.interact(r, ui.scope_id().with(("poll", m.id, &a.option)), Sense::click());
                if resp.hovered() {
                    p.rect_filled(r, 10.0, glass::veil(18));
                    ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
                }
                let c = pos2(r.left() + 12.0, r.center().y);
                let on = chosen.contains(&a.option);
                if on {
                    p.circle_filled(c, 8.0, ink());
                } else {
                    p.circle_stroke(c, 8.0, egui::Stroke::new(1.5, rgba(ink(), 180)));
                }
                p.galley(pos2(r.left() + 30.0, r.center().y - 9.0), elided(ui, &a.text, 14.5, ink(), r.width() - 34.0), ink());
                if resp.clicked() {
                    action = Some(if poll.multiple { Action::TogglePoll(m.id, a.option.clone()) } else { Action::Vote(m.id, vec![a.option.clone()]) });
                }
            }
            y += 40.0;
        }
        if poll.multiple && !show && !chosen.is_empty() {
            let b = Rect::from_min_size(pos2(rect.left(), y), vec2(rect.width(), 34.0));
            if glass::button(ui, b, ("vote", m.id), l!("Голосувати", "Vote"), true, true).clicked() {
                action = Some(Action::Vote(m.id, chosen.to_vec()));
            }
            y += 40.0;
        }
        let votes = match poll.total {
            0 => l!("Ще немає голосів", "No votes yet").to_owned(),
            n => format!("{}{}", lf!("Голосів: {n}", "Votes: {n}"), if poll.closed { l!(" · завершено", " · closed") } else { "" }),
        };
        p.text(pos2(rect.left(), y), Align2::LEFT_TOP, votes, FontId::proportional(12.0), rgba(ink(), 160));
    } else if let Some(g) = &m.geo {
        // Stylized "map": a street grid and a pin; a click opens OpenStreetMap.
        let map = Rect::from_min_size(rect.min, vec2(rect.width(), 100.0));
        let resp = ui.interact(rect, ui.scope_id().with(("geo", m.id)), Sense::click());
        p.rect_filled(map, 12.0, Color32::from_rgb(34, 44, 78));
        let clip = p.with_clip_rect(map.shrink(1.0));
        let stroke = egui::Stroke::new(1.0, glass::veil(30));
        let seed = (g.lat * 1000.0).fract().abs() as f32 * 40.0;
        for k in 0..8 {
            let t = k as f32 * 22.0 + seed;
            clip.line_segment([pos2(map.left() + t, map.top()), pos2(map.left() + t - 30.0, map.bottom())], stroke);
            clip.line_segment([pos2(map.left(), map.top() + t * 0.6), pos2(map.right(), map.top() + t * 0.6 + 14.0)], stroke);
        }
        let pin = Rect::from_center_size(map.center() - vec2(0.0, 6.0), vec2(30.0, 30.0));
        icons::draw(&p, pin, Icon::Location, Color32::from_rgb(255, 110, 130));
        let title = if !g.title.is_empty() { g.title.clone() } else if g.live { l!("Трансляція геопозиції", "Live location").into() } else { l!("Геопозиція", "Location").into() };
        p.galley(pos2(rect.left(), map.bottom() + 6.0), elided(ui, &title, 14.5, ink(), rect.width()), ink());
        let sub = if g.address.is_empty() { format!("{:.5}, {:.5}", g.lat, g.lon) } else { g.address.clone() };
        p.galley(pos2(rect.left(), map.bottom() + 26.0), elided(ui, &sub, 12.5, rgba(ink(), 170), rect.width()), ink());
        if resp.hovered() {
            ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
        }
        if resp.clicked() {
            action = Some(Action::OpenMap(g.lat, g.lon));
        }
    } else if let Some((name, phone)) = &m.contact {
        let resp = ui.interact(rect, ui.scope_id().with(("contact", m.id)), Sense::click());
        glass::avatar(&p, pos2(rect.left() + 24.0, rect.center().y), 22.0, name, name.len() as i64, None, None);
        p.galley(pos2(rect.left() + 56.0, rect.top() + 6.0), elided(ui, name, 15.0, ink(), rect.width() - 60.0), ink());
        p.galley(pos2(rect.left() + 56.0, rect.top() + 28.0), elided(ui, phone, 13.0, rgba(ink(), 180), rect.width() - 60.0), ink());
        if resp.hovered() {
            ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
        }
        if resp.clicked() {
            action = Some(Action::Copy2(phone.clone(), l!("Номер скопійовано", "Number copied")));
        }
    }
    action
}

/// Waveform as bars; `progress` is the played fraction (brighter).
pub(super) fn draw_wave(p: &egui::Painter, wave: Rect, levels: &[u8], progress: Option<f32>) {
    let n = ((wave.width() / 4.0) as usize).max(8);
    let level = |k: usize| match levels.len() {
        0 => 6.0,
        len => {
            let a = k * len / n;
            let b = ((k + 1) * len / n).max(a + 1).min(len);
            levels[a..b].iter().copied().max().unwrap_or(0) as f32
        }
    };
    for k in 0..n {
        let h = (2.0 + level(k) / 31.0 * wave.height()).min(wave.height());
        let x = wave.left() + k as f32 * 4.0 + 1.0;
        let done = progress.is_some_and(|pr| (k as f32 + 0.5) / n as f32 <= pr);
        let col = if done { ink() } else { rgba(ink(), 100) };
        p.rect_filled(Rect::from_min_max(pos2(x, wave.bottom() - h), pos2(x + 2.4, wave.bottom())), 1.2, col);
    }
}

/// Voice or audio (button, waveform or name, time) or video (button and duration).
pub(super) fn media_card(ui: &mut Ui, m: &Msg, rect: Rect, play: PlayView) -> Option<Action> {
    let info = m.media.as_ref()?;
    let p = ui.painter().clone();
    let mut action = None;
    let dim = rgba(ink(), 170);

    let btn = Rect::from_min_size(rect.min + vec2(0.0, (rect.height() - 44.0) / 2.0), vec2(44.0, 44.0));
    let resp = ui.interact(btn, ui.scope_id().with(("play", m.id)), Sense::click());
    let tint = if m.out { Color32::WHITE } else { ACCENT };
    glass::glass(&p, btn, Glass::tinted(22.0, tint, if m.out { 70 } else { 180 }));
    if resp.hovered() {
        p.circle_filled(btn.center(), 22.0, glass::veil(20));
        ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
    }
    if play.loading {
        spinner(&p, btn.center(), 11.0);
    } else {
        let icon = if play.playing { Icon::Pause } else { Icon::Play };
        let off = if play.playing { 0.0 } else { 1.5 };
        icons::draw(&p, Rect::from_center_size(btn.center() + vec2(off, 0.0), vec2(18.0, 18.0)), icon, ink());
    }
    if resp.clicked() {
        action = Some(Action::Play(m.id));
    }

    let x0 = btn.right() + 10.0;
    let right = rect.right();
    let time = if play.current { format!("{} / {}", player::clock(play.position), player::clock(info.duration)) } else { player::clock(info.duration) };
    // Always keep room for the speed toggle so the waveform doesn't jump at start.
    let audio = matches!(info.kind, MediaKind::Voice | MediaKind::Audio);
    let chip_w = if audio { 38.0 } else { 0.0 };
    match info.kind {
        MediaKind::Voice => {
            // Waveform: bars, the played part brighter; a click seeks.
            let wave = Rect::from_min_max(pos2(x0, rect.top() + 4.0), pos2(right - chip_w, rect.top() + 26.0));
            draw_wave(&p, wave, &info.waveform, play.current.then_some(play.progress));
            action = seek_area(ui, m.id, wave, play).or(action);
        }
        _ => {
            let title = match info.kind {
                MediaKind::Audio => match (info.performer.as_str(), info.title.as_str()) {
                    ("", "") => l!("Аудіо", "Audio").to_owned(),
                    ("", t) => t.to_owned(),
                    (who, "") => who.to_owned(),
                    (who, t) => format!("{who} — {t}"),
                },
                _ => String::new(),
            };
            let bold = FontId::new(14.5, egui::FontFamily::Name(format::BOLD.into()));
            let mut job = LayoutJob::single_section(title, egui::TextFormat::simple(bold, ink()));
            job.wrap = TextWrapping::truncate_at_width((right - x0 - chip_w).max(10.0));
            p.galley(pos2(x0, rect.top() + 3.0), p.layout_job(job), ink());
            if info.kind == MediaKind::Audio && play.current {
                let bar = Rect::from_min_max(pos2(x0, rect.top() + 24.0), pos2(right - chip_w, rect.top() + 28.0));
                p.rect_filled(bar, 2.0, rgba(ink(), 70));
                p.rect_filled(Rect::from_min_size(bar.min, vec2(bar.width() * play.progress, bar.height())), 2.0, ink());
                action = seek_area(ui, m.id, bar.expand2(vec2(0.0, 6.0)), play).or(action);
            }
        }
    }
    let t = p.text(pos2(x0, rect.bottom() - 2.0), Align2::LEFT_BOTTOM, time, FontId::proportional(12.0), dim);
    if m.media_unread {
        // "Not listened yet" dot, as in Telegram.
        p.circle_filled(pos2(t.right() + 7.0, t.center().y + 0.5), 3.0, if m.out { ink() } else { ACCENT });
    }

    if audio && play.current {
        let chip = Rect::from_min_size(pos2(right - chip_w + 6.0, rect.top() + 6.0), vec2(chip_w - 6.0, 20.0));
        let r = ui.interact(chip, ui.scope_id().with(("speed", m.id)), Sense::click());
        p.rect_filled(chip, 10.0, glass::veil(if r.hovered() { 45 } else { 25 }));
        let s = if play.speed.fract() == 0.0 { format!("{}×", play.speed as i32) } else { format!("{}×", play.speed) };
        p.text(chip.center(), Align2::CENTER_CENTER, s, FontId::proportional(11.5), ink());
        if r.clicked() {
            action = Some(Action::Speed);
        }
    }
    action
}

/// Clickable seek bar for the current audio.
pub(super) fn seek_area(ui: &mut Ui, id: i32, r: Rect, play: PlayView) -> Option<Action> {
    if !play.current {
        return None;
    }
    let resp = ui.interact(r, ui.scope_id().with(("seek", id)), Sense::click_and_drag());
    if resp.hovered() {
        ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
    }
    // While dragging, seek only on release, so the sound isn't restarted every frame.
    let pos = resp.interact_pointer_pos()?;
    (resp.clicked() || resp.drag_stopped()).then(|| Action::Seek(id, (pos.x - r.left()) / r.width()))
}


/// Album layout: how many photos in each row.
pub(super) fn album_rows(n: usize) -> Vec<usize> {
    match n {
        0..=2 => vec![n],
        3 => vec![1, 2],
        4 => vec![2, 2],
        5 => vec![2, 3],
        6 => vec![3, 3],
        7 => vec![2, 2, 3],
        8 => vec![2, 3, 3],
        9 => vec![3, 3, 3],
        _ => vec![2, 2, 3, 3],
    }
}

/// Album: a grid of photos in one bubble; the caption comes from the message that has one.
pub(super) fn album_bubble(ui: &mut Ui, msgs: &[&Msg], texs: &[Option<&TextureHandle>], look: Look) -> Option<Action> {
    let first = msgs[0];
    let ids: Vec<i32> = msgs.iter().map(|m| m.id).collect();
    let avail = ui.available_width();
    let w = (avail * 0.72).min(360.0);
    let gap = 3.0;
    let rows = album_rows(msgs.len());
    let heights: Vec<f32> = rows.iter().map(|&c| if c == 1 { w * 0.7 } else { (w / c as f32).min(200.0) }).collect();
    let grid_h: f32 = heights.iter().sum::<f32>() + gap * (rows.len() as f32 - 1.0);
    let cap = msgs.iter().find(|m| !m.text.is_empty()).copied();
    let p = ui.painter().clone();
    let body = cap.map(|c| p.layout_job(format::layout(&c.text, &c.spans, glass::msg_font(), ink(), glass::link(false), w - 20.0, false)));
    let last = msgs[msgs.len() - 1];
    let meta = p.layout_no_wrap(last.time.clone(), FontId::proportional(11.0), rgba(ink(), 170));
    let text_h = body.as_ref().map_or(0.0, |b| b.size().y + meta.size().y + 22.0);
    let size = vec2(w + 8.0, grid_h + 8.0 + text_h);
    let (row, resp) = ui.allocate_exact_size(vec2(avail, size.y + 6.0), Sense::click());
    pick_mark(ui, row, look);
    let x = if first.out { row.right() - 10.0 - size.x } else { row.left() + 10.0 + if look.picking { 30.0 } else { 0.0 } };
    let rect = Rect::from_min_size(pos2(x, row.top() + 3.0), size);
    let g = if first.out { Glass::tinted(18.0, ACCENT, glass::bubble_out_alpha()) } else { Glass::tinted(18.0, glass::bubble_in().0, glass::bubble_in().1) };
    glass::glass(&p, rect, g.frosted(1.0));

    let mut y = rect.top() + 4.0;
    let mut k = 0;
    let mut hit = None;
    for (&cols, &h) in rows.iter().zip(&heights) {
        let cw = (w - gap * (cols as f32 - 1.0)) / cols as f32;
        for c in 0..cols {
            let r = Rect::from_min_size(pos2(rect.left() + 4.0 + c as f32 * (cw + gap), y), vec2(cw, h));
            match texs[k] {
                Some(t) => glass::image_rounded(&p, r, 8.0, t.id(), t.size_vec2()),
                None => {
                    p.rect_filled(r, 8.0, glass::veil(14));
                    icons::draw(&p, Rect::from_center_size(r.center(), vec2(26.0, 26.0)), Icon::Camera, ink_dim());
                }
            }
            if resp.hover_pos().is_some_and(|q| r.contains(q)) {
                hit = Some(msgs[k].id);
                ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
            }
            k += 1;
        }
        y += h + gap;
    }
    if let Some(b) = body {
        p.galley(pos2(rect.left() + 14.0, y + 6.0), b, ink());
        p.galley(rect.max - vec2(14.0, 9.0) - meta.size(), meta, ink());
    } else {
        let chip = Rect::from_min_size(rect.max - meta.size() - vec2(22.0, 16.0), meta.size() + vec2(12.0, 6.0));
        p.rect_filled(chip, chip.height() / 2.0, Color32::from_black_alpha(120));
        // The chip over the photo is dark in both themes, so its text is always light.
        let meta = p.layout_no_wrap(last.time.clone(), FontId::proportional(11.0), glass::ON_DARK);
        p.galley(chip.min + vec2(6.0, 3.0), meta, glass::ON_DARK);
    }
    let main = cap.unwrap_or(first);
    let album_protected = msgs.iter().any(|m| m.noforwards);
    let mut action = None;
    if !main.reactions.is_empty() {
        action = reaction_chips(ui, main.id, &main.reactions, rect.left(), first.out, rect.right());
    }
    if look.picking {
        return resp.clicked().then(|| Action::PickMany(ids.clone())).or(action);
    }
    resp.context_menu(|ui| {
        ui.set_min_width(170.0);
        ui.horizontal(|ui| {
            for e in emoji::QUICK_REACTIONS.iter().take(6) {
                if ui.add(egui::Button::new(egui::RichText::new(*e).size(18.0)).frame(false)).clicked() {
                    action = Some(Action::React(main.id, (*e).to_owned()));
                }
            }
        });
        ui.separator();
        if glass::menu_button(ui, Icon::Reply, l!("Відповісти", "Reply")).clicked() {
            action = Some(Action::Reply(main.id));
        }
        if !look.chat.is_some_and(|c| c.noforwards) && !album_protected && glass::menu_button(ui, Icon::Forward, l!("Переслати альбом", "Forward album")).clicked() {
            action = Some(Action::ForwardMany(ids.clone()));
        }
        if glass::menu_button(ui, Icon::Select, l!("Вибрати", "Select")).clicked() {
            action = Some(Action::PickMany(ids.clone()));
        }
        if first.out && glass::menu_button(ui, Icon::Trash, l!("Видалити альбом", "Delete album")).clicked() {
            action = Some(Action::DeleteMany(ids.clone()));
        }
    });
    if resp.clicked()
        && let Some(id) = hit
    {
        action = Some(Action::View(id));
    }
    if resp.double_clicked() {
        action = Some(Action::Reply(main.id));
    }
    action
}

/// Queued message instead of ticks: a clock while sending, a red "!" mark after a failure.
pub(super) fn draw_pending(p: &egui::Painter, right: egui::Pos2, failed: bool, color: Color32) {
    let c = right + vec2(-7.0, -0.5);
    if failed {
        p.circle_filled(c, 6.0, glass::DANGER);
        let s = egui::Stroke::new(1.6, glass::ON_DARK);
        p.line_segment([c + vec2(0.0, -3.2), c + vec2(0.0, 0.8)], s);
        p.circle_filled(c + vec2(0.0, 3.0), 0.9, glass::ON_DARK);
    } else {
        icons::draw(p, Rect::from_center_size(c, vec2(13.0, 13.0)), Icon::Clock, color);
    }
}

/// Menu of a queued bubble: copy, and retry / delete instead of server actions.
pub(super) fn pending_menu(ui: &mut Ui, m: &Msg, failed: bool, action: &mut Option<Action>) {
    ui.set_min_width(190.0);
    if failed {
        ui.label(egui::RichText::new(l!("Не надіслано", "Not sent")).color(glass::DANGER).size(12.5));
        if glass::menu_button(ui, Icon::Reload, l!("Повторити", "Retry")).clicked() {
            *action = Some(Action::Retry(m.id));
        }
    } else {
        ui.label(egui::RichText::new(l!("Надсилається…", "Sending…")).color(ink_dim()).size(12.5));
    }
    if glass::menu_button(ui, Icon::Copy, l!("Копіювати", "Copy")).clicked() {
        *action = Some(Action::Copy(m.text.clone()));
    }
    if glass::menu_button(ui, Icon::Trash, l!("Видалити", "Delete")).clicked() {
        *action = Some(Action::Discard(m.id));
    }
}

pub(super) fn bubble(
    ui: &mut Ui,
    m: &Msg,
    look: Look,
    reply: Option<(String, String)>,
    photo: Option<&TextureHandle>,
    sticker: Option<&TextureHandle>,
) -> Option<Action> {
    const REPLY_H: f32 = 40.0;
    const FILE_H: f32 = 46.0;
    let Look { group: show_sender, revealed, chosen, .. } = look;
    if let Some((_, emoji)) = &m.sticker {
        return sticker_bubble(ui, m, emoji, sticker, look);
    }
    let kind = m.media.as_ref().map(|i| i.kind);
    if kind == Some(MediaKind::Round) {
        return round_bubble(ui, m, photo, look);
    }
    // Video and GIF are drawn like photos: a frame or a thumbnail.
    let video = matches!(kind, Some(MediaKind::Video | MediaKind::Gif));
    let avail = ui.available_width();
    let max_w = (avail * 0.72).min(560.0);
    let pad = vec2(14.0, 9.0);
    let p = ui.painter().clone();

    let img_size = (m.photo || video).then(|| {
        let w = max_w.min(VIDEO_PT);
        let known = m.media.as_ref().map(|i| i.size).filter(|s| s.0 > 0 && s.1 > 0).map(|s| vec2(s.0 as f32, s.1 as f32));
        match known.or(photo.map(|t| t.size_vec2())) {
            Some(s) => vec2(w, (w * s.y / s.x).clamp(110.0, 420.0)),
            None => vec2(w, 220.0),
        }
    });
    let text_w = img_size.map_or(max_w, |s| s.x) - pad.x * 2.0;

    let link = glass::link(m.out);
    let body = (!m.text.is_empty()).then(|| p.layout_job(format::layout(&m.text, &m.spans, glass::msg_font(), ink(), link, text_w, revealed)));
    // Trailing spaces leave room for the ticks drawn on top.
    let meta = format!("{}{}{}{}", chatview::views_label(m, look), if m.edited { l!("ред. ", "edited ") } else { "" }, m.time, if look.ticks.is_some() || look.pending.is_some() { TICKS_GAP } else { "" });
    let meta = p.layout_no_wrap(meta, FontId::proportional(11.0), rgba(ink(), 170));
    let name_color = glass::name_color(m.sender.len() as i64 * 31 + m.sender.chars().next().map_or(0, |c| c as i64));
    let sender = (show_sender && !m.out && !m.sender.is_empty())
        .then(|| elided(ui, &m.sender, 13.0, name_color, text_w));
    let (quote_accent, quote_text) = if m.out { (ink(), rgba(ink(), 200)) } else { (ACCENT, ink_dim()) };
    let reply = reply.map(|(who, what)| (elided(ui, &who, 12.5, quote_accent, text_w - 12.0), elided(ui, &what, 13.0, quote_text, text_w - 12.0)));
    let fwd = m.forwarded.as_ref().map(|from| elided(ui, &lf!("Переслано від {from}", "Forwarded from {from}"), 12.5, quote_accent, text_w));

    // Text block under the photo (or the whole bubble if there is no photo).
    let mut block = vec2(0.0, 0.0);
    if let Some(f) = &fwd {
        block.x = block.x.max(f.size().x);
        block.y += f.size().y + 4.0;
    }
    if let Some(s) = &sender {
        block.x = block.x.max(s.size().x);
        block.y += s.size().y + 2.0;
    }
    if let Some((a, b)) = &reply {
        block.x = block.x.max(a.size().x.max(b.size().x) + 12.0).max(140.0);
        block.y += REPLY_H + 4.0;
    }
    let file = m.file.as_ref().map(|(name, size)| {
        let n = elided(ui, name, 14.5, ink(), text_w - 56.0);
        let s = p.layout_no_wrap(size.clone(), FontId::proportional(12.0), rgba(ink(), 170));
        (n, s)
    });
    if let Some((n, sz)) = &file {
        block.x = block.x.max(n.size().x.max(sz.size().x) + 56.0);
        block.y += FILE_H + 4.0;
    }
    let card_w = text_w.min(320.0);
    let card_h = card_height(ui, m, card_w, chosen);
    if card_h > 0.0 {
        block.x = block.x.max(card_w);
        block.y += card_h + 6.0;
    }
    // The link preview card goes under the text, like in Telegram.
    let web = m.webpage.as_ref().filter(|_| img_size.is_none()).map(|w| suggest::WebCard::layout(ui, w, text_w.min(380.0), photo, quote_accent));
    if let Some(c) = &web {
        block.x = block.x.max(c.width);
        block.y += c.height() + 6.0;
    }
    match &body {
        Some(body) => {
            let last_row_w = body.rows.last().map_or(0.0, |r| r.rect().width());
            let meta_inline = last_row_w + meta.size().x + 10.0 <= text_w && web.is_none();
            block.x = block.x.max(body.size().x);
            block.y += body.size().y;
            if meta_inline {
                block.x = block.x.max(last_row_w + meta.size().x + 10.0);
            } else {
                block.x = block.x.max(meta.size().x);
                block.y += meta.size().y + 2.0;
            }
        }
        // Time goes in the bottom right corner of the media card, next to the duration.
        None if m.media.is_some() => {
            block.x = block.x.max(card_w.min(150.0) + meta.size().x);
            block.y -= 6.0;
        }
        None if img_size.is_none() => {
            block.x = block.x.max(meta.size().x);
            block.y += meta.size().y;
        }
        None => {}
    }
    let has_block = block.y > 0.0;
    let size = match img_size {
        Some(is) => vec2(is.x + 8.0, is.y + 8.0 + if has_block { block.y + pad.y * 2.0 - 4.0 } else { 0.0 }),
        None => block + pad * 2.0,
    };

    let (row, resp) = ui.allocate_exact_size(vec2(avail, size.y + 6.0), Sense::click());
    pick_mark(ui, row, look);
    let x = if m.out { row.right() - 10.0 - size.x } else { row.left() + 10.0 + if look.picking { 30.0 } else { 0.0 } };
    let rect = Rect::from_min_size(pos2(x, row.top() + 3.0), size);
    let g = if m.out {
        Glass::tinted(18.0, ACCENT, glass::bubble_out_alpha()).frosted(1.0)
    } else {
        Glass::tinted(18.0, glass::bubble_in().0, glass::bubble_in().1).frosted(1.0)
    };
    glass::glass(&p, rect, g);

    let mut cursor = rect.min + pad;
    if let Some(is) = img_size {
        let ir = Rect::from_min_size(rect.min + vec2(4.0, 4.0), is);
        match photo {
            Some(t) => glass::image_rounded(&p, ir, 15.0, t.id(), t.size_vec2()),
            None => {
                p.rect_filled(ir, 15.0, glass::veil(14));
                icons::draw(&p, Rect::from_center_size(ir.center(), vec2(30.0, 30.0)), Icon::Camera, ink_dim());
            }
        }
        if let Some(info) = m.media.as_ref().filter(|_| video) {
            video_overlay(&p, ir, info, look.play);
        }
        if !has_block {
            let chip = Rect::from_min_size(ir.max - meta.size() - vec2(18.0, 12.0), meta.size() + vec2(12.0, 6.0));
            p.rect_filled(chip, chip.height() / 2.0, Color32::from_black_alpha(120));
            let on_media = p.layout_no_wrap(meta.text().to_owned(), FontId::proportional(11.0), glass::ON_DARK);
            let at = Rect::from_min_size(chip.min + vec2(6.0, 3.0), on_media.size());
            p.galley(at.min, on_media, glass::ON_DARK);
            chatview::meta_icons(&p, at, m, look, glass::ON_DARK);
        }
        cursor.y = ir.bottom() + pad.y;
    }
    if let Some(f) = fwd {
        let h = f.size().y;
        p.galley(cursor, f, quote_accent);
        cursor.y += h + 4.0;
    }
    if let Some(s) = sender {
        let h = s.size().y;
        p.galley(cursor, s, ink());
        cursor.y += h + 2.0;
    }
    if let Some((a, b)) = reply {
        let r = Rect::from_min_size(cursor, vec2(block.x, REPLY_H));
        p.rect_filled(r, 8.0, glass::veil(16));
        p.rect_filled(Rect::from_min_size(r.min, vec2(3.0, REPLY_H)), 2.0, quote_accent);
        p.galley(r.min + vec2(10.0, 4.0), a, quote_accent);
        p.galley(r.min + vec2(10.0, 21.0), b, quote_text);
        cursor.y += REPLY_H + 4.0;
    }
    let mut file_rect = None;
    let mut body_pos = None;
    if let Some((n, sz)) = file {
        let r = Rect::from_min_size(cursor, vec2(block.x, FILE_H));
        let icon = Rect::from_min_size(r.min + vec2(0.0, 3.0), vec2(40.0, 40.0));
        glass::glass(&p, icon, Glass::tinted(20.0, if m.out { Color32::WHITE } else { ACCENT }, if m.out { 60 } else { 170 }));
        icons::draw(&p, Rect::from_center_size(icon.center(), vec2(19.0, 19.0)), Icon::File, ink());
        p.galley(r.min + vec2(52.0, 4.0), n, ink());
        p.galley(r.min + vec2(52.0, 25.0), sz, ink());
        file_rect = Some(r);
        cursor.y += FILE_H + 4.0;
    }
    let mut card_action = None;
    if card_h > 0.0 {
        card_action = draw_card(ui, m, Rect::from_min_size(cursor, vec2(block.x, card_h)), look);
        cursor.y += card_h + 6.0;
    }
    if let Some(body) = &body {
        p.galley(cursor, body.clone(), ink());
        body_pos = Some(cursor);
    }
    if let Some(c) = web {
        let top = cursor.y + body.as_ref().map_or(0.0, |b| b.size().y + 6.0);
        card_action = c.draw(ui, m, pos2(cursor.x, top), photo, quote_accent).or(card_action);
    }
    if has_block || img_size.is_none() {
        let at = Rect::from_min_size(rect.max - pad - meta.size(), meta.size());
        p.galley(at.min, meta, ink());
        if let Some(failed) = look.pending {
            draw_pending(&p, at.right_center(), failed, rgba(ink(), 200));
        } else {
            chatview::meta_icons(&p, at, m, look, rgba(ink(), 200));
        }
    }

    let mut action = card_action;
    if !m.reactions.is_empty() {
        action = reaction_chips(ui, m.id, &m.reactions, rect.left(), m.out, rect.right()).or(action);
    }
    if !m.buttons.is_empty() {
        action = bot_buttons(ui, m, rect.left(), size.x.max(240.0).min(avail - 20.0)).or(action);
    }
    if look.picking {
        return resp.clicked().then_some(Action::Pick(m.id)).or(action);
    }
    if let Some(failed) = look.pending {
        resp.context_menu(|ui| pending_menu(ui, m, failed, &mut action));
        // A failed bubble retries on click, like the red mark in Telegram Desktop.
        if failed && resp.clicked() {
            action = Some(Action::Retry(m.id));
        }
        return action;
    }
    resp.context_menu(|ui| message_menu(ui, m, look.chat, &mut action));
    // Links and spoilers in the text.
    if let (Some(body), Some(origin), Some(pos)) = (&body, body_pos, resp.hover_pos())
        && Rect::from_min_size(origin, body.size()).contains(pos)
    {
        let ch = body.cursor_from_pos(pos - origin).index.0;
        match format::span_at(&m.spans, format::char_to_byte(&m.text, ch)) {
            Some(span) if span.style == Style::Spoiler && !revealed => {
                ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
                if resp.clicked() {
                    return Some(Action::RevealSpoiler(m.id));
                }
            }
            Some(span) if span.style == Style::Link && span.target.is_some() => {
                ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
                let target = span.target.clone().unwrap_or_default();
                let text = m.text.get(span.start..span.end).unwrap_or_default();
                let hidden = format::hidden_link(text, &target);
                // The real address under the pointer, as in a browser's status bar.
                if hidden {
                    egui::Tooltip::always_open(ui.ctx().clone(), ui.layer_id(), ui.scope_id().with(("link-target", m.id)), egui::PopupAnchor::Pointer).show(|ui| ui.label(&target));
                }
                if resp.clicked() {
                    return Some(if hidden { Action::OpenHidden(target) } else { Action::Open(target) });
                }
            }
            _ => {}
        }
    }
    let media_rect = file_rect.or(img_size.map(|is| Rect::from_min_size(rect.min + vec2(4.0, 4.0), is)));
    if let Some(r) = media_rect {
        let hovered = resp.hover_pos().is_some_and(|p| r.contains(p));
        if hovered {
            ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
        }
        if resp.clicked() && hovered {
            action = match kind {
                _ if m.photo => Some(Action::View(m.id)),
                Some(MediaKind::Video) => Some(Action::Play(m.id)),
                Some(MediaKind::Gif) => None,
                _ => Some(Action::Download(m.id)),
            };
        }
    }
    if resp.double_clicked() {
        action = Some(Action::Reply(m.id));
    }
    action
}

/// Duration and play button over the video frame.
pub(super) fn video_overlay(p: &egui::Painter, r: Rect, info: &backend::MediaInfo, play: PlayView) {
    let label = match info.kind {
        MediaKind::Gif => "GIF".to_owned(),
        _ if play.current => player::clock(info.duration - play.position),
        _ => player::clock(info.duration),
    };
    let g = p.layout_no_wrap(label, FontId::proportional(11.5), glass::ON_DARK);
    let chip = Rect::from_min_size(r.min + vec2(8.0, 8.0), g.size() + vec2(12.0, 6.0));
    p.rect_filled(chip, chip.height() / 2.0, Color32::from_black_alpha(120));
    p.galley(chip.min + vec2(6.0, 3.0), g, glass::ON_DARK);
    if info.kind == MediaKind::Video && !play.playing {
        let c = Rect::from_center_size(r.center(), vec2(54.0, 54.0));
        glass::glass(p, c, Glass::tinted(27.0, glass::chrome(), 140).frosted(1.0));
        if play.loading {
            spinner(p, c.center(), 13.0);
        } else {
            icons::draw(p, Rect::from_center_size(c.center() + vec2(2.0, 0.0), vec2(22.0, 22.0)), Icon::Play, ink());
        }
    }
    if play.current && info.kind == MediaKind::Video {
        let bar = Rect::from_min_max(pos2(r.left() + 10.0, r.bottom() - 6.0), pos2(r.right() - 10.0, r.bottom() - 3.0));
        p.rect_filled(bar, 1.5, glass::veil(60));
        p.rect_filled(Rect::from_min_size(bar.min, vec2(bar.width() * play.progress, bar.height())), 1.5, ink());
    }
}

/// A spinning ring while something loads.
pub(super) fn spinner(p: &egui::Painter, c: egui::Pos2, r: f32) {
    let t = p.ctx().input(|i| i.time) as f32 * 5.0;
    let pts: Vec<_> = (0..=24).map(|k| t + k as f32 * 0.18).map(|a| c + vec2(a.cos(), a.sin()) * r).collect();
    p.add(egui::Shape::line(pts, egui::Stroke::new(2.2, ink())));
}

/// Round video message without a bubble: frame in a circle, glass rim, progress around the circle.
pub(super) fn round_bubble(ui: &mut Ui, m: &Msg, tex: Option<&TextureHandle>, look: Look) -> Option<Action> {
    let size = ROUND_PT;
    let avail = ui.available_width();
    let (row, resp) = ui.allocate_exact_size(vec2(avail, size + 12.0), Sense::click());
    pick_mark(ui, row, look);
    if look.picking && resp.clicked() {
        return Some(Action::Pick(m.id));
    }
    let x = if m.out { row.right() - 10.0 - size } else { row.left() + 10.0 + if look.picking { 30.0 } else { 0.0 } };
    let r = Rect::from_min_size(pos2(x, row.top() + 6.0), vec2(size, size));
    let p = ui.painter();
    glass::glass(p, r.expand(4.0), Glass::tinted(size / 2.0 + 4.0, glass::bubble_in().0, glass::bubble_in().1.max(40)).frosted(1.0).with_shadow());
    match tex {
        Some(t) => glass::image_rounded(p, r, size / 2.0, t.id(), t.size_vec2()),
        None => {
            p.circle_filled(r.center(), size / 2.0, glass::veil(14));
            icons::draw(p, Rect::from_center_size(r.center(), vec2(34.0, 34.0)), Icon::Camera, ink_dim());
        }
    }
    let play = look.play;
    if play.loading {
        spinner(p, r.center(), 16.0);
    }
    if play.current {
        let a0 = -std::f32::consts::FRAC_PI_2;
        let n = (play.progress * 96.0).ceil() as usize;
        let pts: Vec<_> = (0..=n).map(|k| a0 + k as f32 / 96.0 * std::f32::consts::TAU).map(|a| r.center() + vec2(a.cos(), a.sin()) * (size / 2.0 + 1.0)).collect();
        if pts.len() > 1 {
            p.add(egui::Shape::line(pts, egui::Stroke::new(3.0, ink())));
        }
    } else if !play.loading {
        // Muted: a speaker-off icon at the bottom.
        let c = Rect::from_center_size(pos2(r.center().x, r.bottom() - 22.0), vec2(26.0, 26.0));
        p.circle_filled(c.center(), 13.0, Color32::from_black_alpha(170));
        icons::draw(p, c.shrink(5.0), Icon::Muted, glass::ON_DARK);
    }
    let info = m.media.as_ref()?;
    let left = if play.current { info.duration - play.position } else { info.duration };
    let g = p.layout_no_wrap(player::clock(left), FontId::proportional(11.5), ink());
    let dot = if m.media_unread { 10.0 } else { 0.0 };
    let chip = Rect::from_min_size(pos2(r.left() + 4.0, r.bottom() - g.size().y - 6.0), g.size() + vec2(12.0 + dot, 6.0));
    glass::glass(p, chip, Glass::tinted(chip.height() / 2.0, glass::chrome(), 120).frosted(1.0));
    p.galley(chip.min + vec2(6.0, 3.0), g, ink());
    if m.media_unread {
        p.circle_filled(pos2(chip.right() - 9.0, chip.center().y), 3.0, ACCENT);
    }
    let meta = p.layout_no_wrap(m.time.clone(), FontId::proportional(11.0), ink());
    let mchip = Rect::from_min_size(pos2(r.right() - meta.size().x - 16.0, r.bottom() - meta.size().y - 6.0), meta.size() + vec2(12.0, 6.0));
    glass::glass(p, mchip, Glass::tinted(mchip.height() / 2.0, glass::chrome(), 120).frosted(1.0));
    p.galley(mchip.min + vec2(6.0, 3.0), meta, ink());

    let mut action = None;
    if !m.reactions.is_empty() {
        action = reaction_chips(ui, m.id, &m.reactions, r.left(), m.out, r.right());
    }
    let inside = resp.hover_pos().is_some_and(|q| q.distance(r.center()) <= size / 2.0);
    if inside {
        ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
    }
    resp.context_menu(|ui| message_menu(ui, m, look.chat, &mut action));
    if resp.double_clicked() {
        return Some(Action::Reply(m.id));
    }
    if resp.clicked() && inside {
        action = Some(Action::Play(m.id));
    }
    action
}

pub(super) fn upload_pill(ui: &Ui, u: &Upload, bottom_left: egui::Pos2) -> f32 {
    let pill = Rect::from_min_size(bottom_left - vec2(0.0, 40.0), vec2(280.0, 40.0));
    glass::glass(ui.painter(), pill, Glass::tinted(20.0, glass::chrome(), 150).frosted(1.0).with_shadow());
    let p = ui.painter();
    let frac = if u.total > 0 { (u.done as f32 / u.total as f32).clamp(0.0, 1.0) } else { 0.0 };
    // Circular progress ring.
    let c = pos2(pill.left() + 21.0, pill.center().y);
    p.circle_stroke(c, 10.0, egui::Stroke::new(2.5, glass::veil(40)));
    if frac > 0.0 {
        let n = (frac * 48.0).ceil() as usize + 1;
        let pts = (0..n)
            .map(|i| {
                let a = -std::f32::consts::FRAC_PI_2 + std::f32::consts::TAU * frac * i as f32 / (n - 1) as f32;
                c + 10.0 * vec2(a.cos(), a.sin())
            })
            .collect();
        p.add(egui::Shape::line(pts, egui::Stroke::new(2.5, ACCENT)));
    }
    let pct = format!("{}%", (frac * 100.0).round() as u32);
    let pg = p.layout_no_wrap(pct, FontId::proportional(12.5), ink_dim());
    let pw = pg.size().x;
    p.galley(pos2(pill.right() - 14.0 - pw, pill.center().y - pg.size().y / 2.0), pg, ink_dim());
    let name = elided(ui, &u.name, 13.5, ink(), pill.width() - 60.0 - pw);
    let nh = name.size().y;
    p.galley(pos2(pill.left() + 40.0, pill.center().y - nh / 2.0), name, ink());
    pill.top()
}
