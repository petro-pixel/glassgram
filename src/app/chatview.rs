//! Chat view extras: pinned bar and list, jump buttons (down / mentions / reactions),
//! "seen by" popup, view counters and protected content.

use super::*;

/// Height of the pinned-message bar under the header.
const PIN_BAR_H: f32 = 48.0;
/// Round jump buttons above the composer.
const JUMP_BTN: f32 = 44.0;
/// Room for the eye icon before the view count (≈14 pt at font size 11).
const EYE_GAP: &str = "     ";
/// Distance from the end of history that still counts as "at the bottom".
const BOTTOM_SLACK: f32 = 80.0;

/// View count prefix for the time label of channel posts.
pub(super) fn views_label(m: &Msg, look: Look) -> String {
    match (m.views, look.chat.map(|c| c.kind)) {
        (Some(v), Some(ChatKind::Channel)) => format!("{EYE_GAP}{}   ", compact(v)),
        _ => String::new(),
    }
}

/// 950 → "950", 12 400 → "12.4K", like Telegram.
fn compact(n: i32) -> String {
    let short = |v: f64| {
        let s = format!("{v:.1}");
        s.strip_suffix(".0").map(str::to_owned).unwrap_or(s)
    };
    match n {
        n if n >= 1_000_000 => lf!("{} млн", "{}M", short(n as f64 / 1e6)),
        n if n >= 1_000 => lf!("{} тис.", "{}K", short(n as f64 / 1e3)),
        n => n.to_string(),
    }
}

/// Icons over the time label: ticks on the right, the eye on the left (views).
pub(super) fn meta_icons(p: &egui::Painter, at: Rect, m: &Msg, look: Look, color: Color32) {
    if let Some(read) = look.ticks {
        icons::ticks(p, at.right_center(), read, color);
    }
    if !views_label(m, look).is_empty() {
        icons::draw(p, Rect::from_center_size(pos2(at.left() + 6.5, at.center().y), vec2(12.0, 12.0)), Icon::Eye, color);
    }
}

/// "Pin" / "Unpin" items of the message menu; the pin options follow Telegram Desktop's dialog.
pub(super) fn pin_menu(ui: &mut Ui, m: &Msg, chat: &ChatInfo, action: &mut Option<Action>) {
    if !chat.can_pin || m.id <= 0 {
        return;
    }
    if m.pinned {
        if glass::menu_button(ui, Icon::PinOff, l!("Відкріпити", "Unpin")).clicked() {
            *action = Some(Action::Unpin(m.id));
        }
        return;
    }
    let label = l!("Закріпити", "Pin");
    match chat.kind {
        ChatKind::Saved => {
            if glass::menu_button(ui, Icon::Pin, label).clicked() {
                *action = Some(Action::Pin(m.id, true, false));
            }
        }
        ChatKind::User | ChatKind::Bot => {
            ui.menu_button((glass::icon_atom(Icon::Pin), label), |ui| {
                if ui.button(lf!("Для мене і {}", "For me and {}", chat.title)).clicked() {
                    *action = Some(Action::Pin(m.id, false, false));
                }
                if ui.button(l!("Лише для мене", "Only for me")).clicked() {
                    *action = Some(Action::Pin(m.id, false, true));
                }
            });
        }
        ChatKind::Group | ChatKind::Channel => {
            ui.menu_button((glass::icon_atom(Icon::Pin), label), |ui| {
                if glass::menu_button(ui, Icon::Bell, l!("Сповістити всіх", "Notify everyone")).clicked() {
                    *action = Some(Action::Pin(m.id, false, false));
                }
                if glass::menu_button(ui, Icon::BellOff, l!("Без сповіщення", "Without notification")).clicked() {
                    *action = Some(Action::Pin(m.id, true, false));
                }
            });
        }
    }
}

/// One-line preview for the pinned bar and list: cards and media have no text of their own.
fn preview(m: &Msg) -> String {
    let label = if let Some(poll) = &m.poll {
        format!("📊 {}", poll.question)
    } else if let Some(g) = &m.geo {
        format!("📍 {}", if g.title.is_empty() { l!("Місце", "Location") } else { g.title.as_str() })
    } else if let Some((name, _)) = &m.contact {
        format!("👤 {name}")
    } else if let Some((_, emoji)) = &m.sticker {
        lf!("{emoji} Стікер", "{emoji} Sticker")
    } else if let Some((name, _)) = &m.file {
        format!("📎 {name}")
    } else {
        match m.media.as_ref().map(|i| i.kind) {
            Some(MediaKind::Voice) => l!("🎤 Голосове повідомлення", "🎤 Voice message").to_owned(),
            Some(MediaKind::Round) => l!("📹 Відеоповідомлення", "📹 Video message").to_owned(),
            Some(MediaKind::Video) => l!("🎬 Відео", "🎬 Video").to_owned(),
            Some(MediaKind::Gif) => "GIF".to_owned(),
            Some(MediaKind::Audio) => l!("🎵 Аудіо", "🎵 Audio").to_owned(),
            None => return quote(m),
        }
    };
    if m.text.is_empty() { label } else { format!("{label} · {}", m.text.replace('\n', " ")) }
}

/// Small counter on top of a jump button.
fn badge(p: &egui::Painter, ui: &Ui, center_top: egui::Pos2, n: usize) {
    let label = if n > 999 { "999+".to_owned() } else { n.to_string() };
    let g = elided(ui, &label, 11.5, glass::ON_DARK, 60.0);
    let w = (g.size().x + 12.0).max(20.0);
    let r = Rect::from_center_size(center_top, vec2(w, 20.0));
    glass::glass(p, r, Glass::tinted(10.0, ACCENT, 230));
    p.galley(r.center() - g.size() / 2.0, g, glass::ON_DARK);
}

impl App {
    /// Chat info for the demo backend (mention counters and such).
    pub fn demo_chat(&self, peer: PeerId) -> Option<&ChatInfo> {
        self.chats.iter().find(|c| c.id == peer)
    }

    /// Per-chat state reset and requests made when a chat is opened.
    pub(super) fn open_chat_view(&mut self, peer: PeerId) {
        self.pins_open = false;
        self.read_by = None;
        self.mark_wanted = None;
        self.at_bottom = true;
        self.unseen_below = 0;
        self.send(Cmd::LoadPinned(peer));
        self.open_sponsored(peer);
        let (mentions, reactions) = self.chats.iter().find(|c| c.id == peer).map_or((0, 0), |c| (c.unread_mentions, c.unread_reactions));
        if mentions > 0 {
            self.send(Cmd::LoadUnreadMarks { peer, reactions: false });
        }
        if reactions > 0 {
            self.send(Cmd::LoadUnreadMarks { peer, reactions: true });
        }
    }

    pub(super) fn chat_view_event(&mut self, ev: Ev) {
        match ev {
            Ev::Pinned { peer, msgs } => {
                let ids: HashSet<i32> = msgs.iter().map(|m| m.id).collect();
                if let Some(list) = self.history.get_mut(&peer) {
                    for m in list {
                        m.pinned = ids.contains(&m.id);
                    }
                }
                // Keep the shown message unless it was unpinned; otherwise show the newest.
                if !self.pin_shown.get(&peer).is_some_and(|id| ids.contains(id)) {
                    match msgs.last() {
                        Some(m) => self.pin_shown.insert(peer, m.id),
                        None => self.pin_shown.remove(&peer),
                    };
                }
                if msgs.is_empty() && self.selected == Some(peer) {
                    self.pins_open = false;
                }
                self.pins.insert(peer, msgs);
            }
            Ev::PinnedChanged { peer, ids, pinned } => {
                for id in &ids {
                    if let Some(m) = self.find_mut(peer, *id) {
                        m.pinned = pinned;
                    }
                }
                let found: Vec<Msg> = ids.iter().filter_map(|id| self.find(peer, *id).cloned()).collect();
                let list = self.pins.entry(peer).or_default();
                if pinned {
                    let missing = found.len() < ids.len();
                    for m in found {
                        if !list.iter().any(|x| x.id == m.id) {
                            list.push(m);
                        }
                    }
                    list.sort_by_key(|m| m.id);
                    if let Some(id) = ids.iter().copied().filter(|id| list.iter().any(|m| m.id == *id)).max() {
                        self.pin_shown.insert(peer, id);
                    }
                    self.pins_hidden.remove(&peer);
                    // The message is not in the loaded history — ask for the full list.
                    if missing && self.selected == Some(peer) {
                        self.send(Cmd::LoadPinned(peer));
                    }
                } else {
                    list.retain(|m| !ids.contains(&m.id));
                    let last = list.last().map(|m| m.id);
                    if self.pin_shown.get(&peer).is_some_and(|id| ids.contains(id)) {
                        match last {
                            Some(id) => self.pin_shown.insert(peer, id),
                            None => self.pin_shown.remove(&peer),
                        };
                    }
                }
            }
            Ev::UnreadMarks { peer, reactions, ids } => {
                // The server may have read everything already — sync the counter.
                if ids.is_empty()
                    && let Some(c) = self.chat_mut(peer)
                {
                    *if reactions { &mut c.unread_reactions } else { &mut c.unread_mentions } = 0;
                }
                let wanted = self.mark_wanted == Some((peer, reactions)) && !ids.is_empty();
                self.marks.insert((peer, reactions), ids);
                if wanted {
                    self.mark_wanted = None;
                    self.next_mark(peer, reactions);
                }
            }
            Ev::ReadBy { peer, id, readers } => {
                if let Some(r) = &mut self.read_by
                    && r.0 == peer
                    && r.1 == id
                {
                    r.2 = Some(readers);
                }
            }
            Ev::Views { peer, id, views } => {
                if let Some(m) = self.find_mut(peer, id) {
                    m.views = Some(views);
                }
            }
            _ => {}
        }
    }

    /// Jump to the next unread mention (`false`) or reaction (`true`), oldest first.
    pub(super) fn next_mark(&mut self, peer: PeerId, reactions: bool) {
        let list = self.marks.entry((peer, reactions)).or_default();
        if list.is_empty() {
            // Not loaded yet (or we went through the page) — ask the server and jump on arrival.
            self.mark_wanted = Some((peer, reactions));
            self.send(Cmd::LoadUnreadMarks { peer, reactions });
            return;
        }
        let id = list.remove(0);
        let left = list.len() as i32;
        let mut read_all = false;
        if let Some(c) = self.chat_mut(peer) {
            let n = if reactions { &mut c.unread_reactions } else { &mut c.unread_mentions };
            *n = if left == 0 { 0 } else { (*n - 1).max(left) };
            read_all = *n == 0;
        }
        // The server only clears these per chat — once the last one is seen.
        if read_all {
            self.send(Cmd::ReadMarks { peer, reactions });
        }
        self.jump_in_chat(peer, id);
    }

    fn read_all_marks(&mut self, peer: PeerId, reactions: bool) {
        self.marks.remove(&(peer, reactions));
        if let Some(c) = self.chat_mut(peer) {
            *if reactions { &mut c.unread_reactions } else { &mut c.unread_mentions } = 0;
        }
        self.send(Cmd::ReadMarks { peer, reactions });
    }

    /// Scroll the open chat to a message, loading history around it if needed.
    pub(super) fn jump_in_chat(&mut self, peer: PeerId, id: i32) {
        if self.find(peer, id).is_none() {
            self.send(Cmd::OpenAt { peer, id });
        }
        self.jump = Some((peer, id, self.ctx.input(|i| i.time)));
    }

    /// Chat or any of the messages has protected content (no forwarding, copying, saving).
    pub(super) fn protected(&self, peer: PeerId, ids: &[i32]) -> bool {
        self.chats.iter().any(|c| c.id == peer && c.noforwards) || ids.iter().any(|id| self.find(peer, *id).is_some_and(|m| m.noforwards))
    }

    /// Handles chat-view actions and blocks forbidden ones; returns the rest for `apply`.
    pub(super) fn chat_view_action(&mut self, peer: PeerId, action: Action) -> Option<Action> {
        let blocked = match &action {
            Action::Copy(_) => self.protected(peer, &[]),
            Action::Forward(id) | Action::Download(id) | Action::OpenExternal(id) => self.protected(peer, &[*id]),
            Action::ForwardMany(ids) => self.protected(peer, ids),
            _ => false,
        };
        if blocked {
            self.toast = Some((l!("У цьому чаті заборонено копіювати й пересилати", "Copying and forwarding are restricted in this chat").into(), 0.0, true));
            return None;
        }
        match action {
            Action::Pin(id, silent, one_side) => self.send(Cmd::PinMessage { peer, id, unpin: false, silent, one_side }),
            Action::Unpin(id) => self.send(Cmd::PinMessage { peer, id, unpin: true, silent: true, one_side: false }),
            Action::ReadBy(id) => {
                self.read_by = Some((peer, id, None));
                self.send(Cmd::LoadReadBy { peer, id });
            }
            Action::Translate(id) => self.open_translate(peer, id),
            a => return Some(a),
        }
        None
    }

    /// Space the pinned bar takes under the header.
    pub(super) fn pin_bar_h(&self, peer: PeerId) -> f32 {
        let shown = self.pins.get(&peer).is_some_and(|l| !l.is_empty()) && !self.pins_hidden.contains(&peer);
        if shown { PIN_BAR_H + 8.0 } else { 0.0 }
    }

    /// Remember whether history is scrolled to the end (for the "down" button).
    pub(super) fn track_bottom<R>(&mut self, out: &egui::scroll_area::ScrollAreaOutput<R>) {
        let max = (out.content_size.y - out.inner_rect.height()).max(0.0);
        self.at_bottom = max - out.state.clamped_offset().y < BOTTOM_SLACK;
        if self.at_bottom {
            self.unseen_below = 0;
        }
    }

    /// Pinned bar, jump buttons and popups over the chat; Esc closes our popups first.
    pub(super) fn chat_view_overlays(&mut self, ui: &mut Ui, peer: PeerId, rect: Rect, header: Rect, above: f32) {
        let Some(chat) = self.chats.iter().find(|c| c.id == peer).cloned() else { return };
        // Forum topics have their own pins; the chat-wide bar would be misleading there.
        let in_topic = chat.forum && self.topic.is_some();
        if self.pin_bar_h(peer) > 0.0 && !in_topic {
            let bar = Rect::from_min_size(pos2(rect.left(), header.bottom() + 8.0), vec2(rect.width(), PIN_BAR_H));
            self.pinned_bar(ui, peer, &chat, bar);
            if self.pins_open {
                let w = rect.width().min(420.0);
                let panel = Rect::from_min_max(pos2(rect.right() - w, bar.bottom() + 8.0), pos2(rect.right(), (bar.bottom() + 420.0).min(above - 8.0)));
                self.pins_list_view(ui, peer, &chat, panel);
            }
        }
        let saved_h = self.saved_bar_h(peer);
        if saved_h > 0.0 {
            let bar = Rect::from_min_size(pos2(rect.left(), header.bottom() + 8.0 + self.pin_bar_h(peer)), vec2(rect.width(), saved_h - 8.0));
            self.saved_bar(ui, peer, bar);
        }
        self.jump_buttons(ui, peer, &chat, rect, above);
        if self.read_by.as_ref().is_some_and(|r| r.0 != peer) {
            self.read_by = None;
        }
        if self.read_by.is_some() {
            let panel = Rect::from_center_size(rect.center(), vec2(rect.width().min(320.0), (rect.height() - 120.0).min(360.0)));
            self.read_by_view(ui, panel);
        }
        self.sponsored_overlay(ui, peer, rect);
        self.calendar_view(ui, peer, rect);
        self.translate_view(ui, peer, rect);
        if self.viewer.is_none() && (self.read_by.is_some() || self.pins_open) && ui.input_mut(|i| i.consume_key(egui::Modifiers::NONE, Key::Escape)) {
            if self.read_by.is_some() {
                self.read_by = None;
            } else {
                self.pins_open = false;
            }
        }
    }

    fn pinned_bar(&mut self, ui: &mut Ui, peer: PeerId, chat: &ChatInfo, rect: Rect) {
        let Some(list) = self.pins.get(&peer).filter(|l| !l.is_empty()) else { return };
        let shown = self.pin_shown.get(&peer).copied();
        let idx = list.iter().position(|m| Some(m.id) == shown).unwrap_or(list.len() - 1);
        let m = list[idx].clone();
        let len = list.len();
        let p = ui.painter().clone();
        glass::glass(&p, rect, Glass::tinted(rect.height() / 2.0, glass::chrome(), 110).with_shadow().frosted(0.96));

        // Position indicator: up to four segments, the newest at the bottom, like Telegram Desktop.
        let n = len.min(4);
        let cur = if len <= 4 { idx } else { idx * n / len };
        let track = Rect::from_min_max(pos2(rect.left() + 18.0, rect.top() + 10.0), pos2(rect.left() + 21.0, rect.bottom() - 10.0));
        let gap = 2.0;
        let seg_h = (track.height() - gap * (n as f32 - 1.0)) / n as f32;
        for k in 0..n {
            let r = Rect::from_min_size(pos2(track.left(), track.top() + k as f32 * (seg_h + gap)), vec2(track.width(), seg_h));
            p.rect_filled(r, 1.5, if k == cur { ACCENT } else { rgba(ACCENT, 90) });
        }

        let close = Rect::from_center_size(pos2(rect.right() - 26.0, rect.center().y), vec2(34.0, 34.0));
        let list_btn = Rect::from_center_size(pos2(close.left() - 22.0, rect.center().y), vec2(34.0, 34.0));
        let mut x = track.right() + 10.0;
        if let Some(t) = self.textures.get(&ImgKey::Photo(peer, m.id)) {
            let thumb = Rect::from_min_size(pos2(x, rect.center().y - 16.0), vec2(32.0, 32.0));
            glass::image_rounded(&p, thumb, 8.0, t.id(), t.size_vec2());
            x = thumb.right() + 10.0;
        }
        let text_w = (if len > 1 { list_btn.left() } else { close.left() } - x - 8.0).max(20.0);
        let title = if len > 1 { lf!("Закріплене повідомлення #{}", "Pinned message #{}", idx + 1) } else { l!("Закріплене повідомлення", "Pinned message").to_owned() };
        p.galley(pos2(x, rect.center().y - 16.0), elided(ui, &title, 13.0, ACCENT, text_w), ACCENT);
        p.galley(pos2(x, rect.center().y + 2.0), elided(ui, &preview(&m), 13.5, ink(), text_w), ink());

        // Click jumps to the shown message and moves the bar to the previous one (cycling).
        let body = Rect::from_min_max(rect.min, pos2(x + text_w, rect.bottom()));
        let resp = ui.interact(body, ui.scope_id().with(("pin-bar", peer)), Sense::click());
        if resp.hovered() {
            ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
        }
        if resp.clicked() {
            let prev = list[if idx == 0 { len - 1 } else { idx - 1 }].id;
            self.pin_shown.insert(peer, prev);
            self.jump_in_chat(peer, m.id);
        }
        if len > 1 && glass::icon_button(ui, list_btn, ("pin-list", peer), Icon::List, self.pins_open, true).on_hover_text(l!("Усі закріплені", "All pinned messages")).clicked() {
            self.pins_open = !self.pins_open;
        }
        let hint = if chat.can_pin { l!("Відкріпити", "Unpin") } else { l!("Сховати", "Hide") };
        if glass::icon_button(ui, close, ("pin-close", peer), Icon::Close, false, true).on_hover_text(hint).clicked() {
            if chat.can_pin {
                self.send(Cmd::PinMessage { peer, id: m.id, unpin: true, silent: true, one_side: false });
            } else {
                self.pins_hidden.insert(peer);
                self.pins_open = false;
            }
        }
    }

    /// List of all pinned messages, newest first.
    fn pins_list_view(&mut self, ui: &mut Ui, peer: PeerId, chat: &ChatInfo, rect: Rect) {
        let Some(list) = self.pins.get(&peer).cloned() else { return };
        glass::glass(ui.painter(), rect, Glass::panel(24.0).with_shadow());
        let title = lf!("Закріплені повідомлення: {}", "Pinned messages: {}", list.len());
        ui.painter().text(pos2(rect.left() + 18.0, rect.top() + 26.0), Align2::LEFT_CENTER, title, FontId::proportional(15.0), ink());
        let close = Rect::from_center_size(pos2(rect.right() - 26.0, rect.top() + 26.0), vec2(32.0, 32.0));
        if glass::icon_button(ui, close, "pins-close", Icon::Close, false, true).clicked() {
            self.pins_open = false;
        }
        let footer = if chat.can_pin { 52.0 } else { 0.0 };
        let body = Rect::from_min_max(pos2(rect.left() + 8.0, rect.top() + 50.0), pos2(rect.right() - 8.0, rect.bottom() - 8.0 - footer));
        let mut chosen = None;
        let mut child = ui.new_child(UiBuilder::new().max_rect(body));
        egui::ScrollArea::vertical().id_salt(("pins", peer)).auto_shrink(false).show(&mut child, |ui| {
            for m in list.iter().rev() {
                let w = ui.available_width();
                let (r, resp) = ui.allocate_exact_size(vec2(w, 52.0), Sense::click());
                if resp.hovered() {
                    ui.painter().rect_filled(r, 14.0, glass::veil(18));
                    ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
                }
                let head = format!("{} · {}", author(m, &self.me), m.time);
                ui.painter().galley(r.min + vec2(12.0, 7.0), elided(ui, &head, 12.5, ACCENT, w - 24.0), ACCENT);
                ui.painter().galley(r.min + vec2(12.0, 26.0), elided(ui, &preview(m), 14.0, ink(), w - 24.0), ink());
                if resp.clicked() {
                    chosen = Some(m.id);
                }
            }
        });
        if chat.can_pin {
            let b = Rect::from_min_max(pos2(rect.left() + 14.0, rect.bottom() - 52.0), pos2(rect.right() - 14.0, rect.bottom() - 12.0));
            if glass::button(ui, b, ("unpin-all", peer), l!("Відкріпити всі", "Unpin all messages"), false, true).clicked() {
                self.send(Cmd::UnpinAll(peer));
                self.pins_open = false;
            }
        }
        if let Some(id) = chosen {
            self.pin_shown.insert(peer, id);
            self.pins_open = false;
            self.jump_in_chat(peer, id);
        }
    }

    /// Round buttons over the bottom-right corner: reactions, mentions, then "down" (Telegram's order).
    fn jump_buttons(&mut self, ui: &mut Ui, peer: PeerId, chat: &ChatInfo, rect: Rect, above: f32) {
        let x = rect.right() - 8.0 - JUMP_BTN / 2.0;
        let mut y = above - 8.0 - JUMP_BTN / 2.0;
        let p = ui.painter().clone();
        let mut btn = |ui: &mut Ui, id: &str, icon: Icon, n: usize, hint: &str| {
            let r = Rect::from_center_size(pos2(x, y), vec2(JUMP_BTN, JUMP_BTN));
            y -= JUMP_BTN + 10.0;
            let resp = glass::icon_button(ui, r, (id, peer), icon, false, true).on_hover_text(hint);
            if n > 0 {
                badge(&p, ui, pos2(r.center().x, r.top()), n);
            }
            resp
        };
        if !self.at_bottom && btn(ui, "jump-down", Icon::ChevronDown, self.unseen_below, l!("До останніх повідомлень", "Scroll to the latest messages")).clicked() {
            self.scroll_bottom = true;
            self.unseen_below = 0;
        }
        for (reactions, n) in [(false, chat.unread_mentions), (true, chat.unread_reactions)] {
            if n <= 0 {
                continue;
            }
            let (id, icon, hint) = if reactions {
                ("jump-reaction", Icon::Heart, l!("Непрочитані реакції", "Unread reactions"))
            } else {
                ("jump-mention", Icon::At, l!("Непрочитані згадки", "Unread mentions"))
            };
            let resp = btn(ui, id, icon, n as usize, hint);
            if resp.clicked() {
                self.next_mark(peer, reactions);
            }
            let mut read = false;
            resp.context_menu(|ui| {
                if glass::menu_button(ui, Icon::CheckCheck, l!("Позначити всі прочитаними", "Mark all as read")).clicked() {
                    read = true;
                }
            });
            if read {
                self.read_all_marks(peer, reactions);
            }
        }
    }

    /// "Seen by" popup for an outgoing group message.
    fn read_by_view(&mut self, ui: &mut Ui, rect: Rect) {
        let Some((_, _, readers)) = &self.read_by else { return };
        let readers = readers.clone();
        glass::glass(ui.painter(), rect, Glass::panel(24.0).with_shadow());
        let title = match &readers {
            Some(r) if !r.is_empty() => lf!("Переглянули: {}", "Seen by {}", r.len()),
            _ => l!("Переглянули", "Seen by").to_owned(),
        };
        ui.painter().text(pos2(rect.left() + 18.0, rect.top() + 26.0), Align2::LEFT_CENTER, title, FontId::proportional(15.0), ink());
        let close = Rect::from_center_size(pos2(rect.right() - 26.0, rect.top() + 26.0), vec2(32.0, 32.0));
        if glass::icon_button(ui, close, "read-by-close", Icon::Close, false, true).clicked() {
            self.read_by = None;
            return;
        }
        let body = Rect::from_min_max(pos2(rect.left() + 8.0, rect.top() + 50.0), pos2(rect.right() - 8.0, rect.bottom() - 8.0));
        let mut child = ui.new_child(UiBuilder::new().max_rect(body));
        match readers {
            None => {
                child.add_space(24.0);
                child.vertical_centered(|ui| ui.add(egui::Spinner::new().color(ink())));
            }
            Some(list) if list.is_empty() => {
                child.add_space(24.0);
                child.vertical_centered(|ui| ui.label(egui::RichText::new(l!("Ще ніхто не переглянув", "Nobody has seen it yet")).color(ink_dim())));
            }
            Some(list) => {
                egui::ScrollArea::vertical().id_salt("read-by").auto_shrink(false).show(&mut child, |ui| {
                    for (i, (name, when)) in list.iter().enumerate() {
                        let w = ui.available_width();
                        let (r, _) = ui.allocate_exact_size(vec2(w, 46.0), Sense::hover());
                        glass::avatar(ui.painter(), pos2(r.left() + 26.0, r.center().y), 16.0, name, i as i64 * 7 + name.len() as i64, None, None);
                        ui.painter().galley(pos2(r.left() + 52.0, r.top() + 5.0), elided(ui, name, 14.0, ink(), w - 64.0), ink());
                        ui.painter().text(pos2(r.left() + 52.0, r.top() + 33.0), Align2::LEFT_CENTER, when, FontId::proportional(12.0), ink_dim());
                    }
                });
            }
        }
    }
}
