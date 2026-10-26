//! The open chat: message history with its header, overlays and the selection bar.

use super::*;

/// History scrolling: measured row heights, staying at the bottom, messages that came below, the jump target.
pub(super) struct Scroll {
    /// Measured heights of history rows (keyed by the row's first message), for virtualization.
    pub(super) row_h: RefCell<HashMap<(PeerId, i32), f32>>,
    /// History is scrolled to the end (as of last frame).
    pub(super) at_bottom: bool,
    /// Scroll down on the next frame.
    pub(super) to_bottom: bool,
    /// New messages that arrived while history was scrolled up.
    pub(super) unseen_below: usize,
    /// (chat, message, when the jump started)
    pub(super) jump: Option<(PeerId, i32, f64)>,
    /// The open chat had unread messages: where reading stopped and, once found, the first unread one.
    pub(super) unread: Option<Unread>,
}

#[derive(Clone, Copy)]
pub(super) struct Unread {
    peer: PeerId,
    read_in: i32,
    /// The first unread message: the "Unread messages" divider goes above it.
    first: Option<i32>,
    /// The older page that reaches it was asked for.
    asked: bool,
}

/// Where the parts of the bottom row are this frame.
#[derive(Clone, Copy)]
struct Bottom {
    attach: Rect,
    sticker: Rect,
    scheduled: Rect,
    send: Rect,
    input: Rect,
    /// Top of the field and the bars above it: panels open upwards from here.
    above: f32,
}

impl App {
    /// Opening a chat with unread messages starts at the first of them, as in Telegram Desktop.
    pub(super) fn unread_on_open(&mut self, peer: PeerId) {
        let read_in = self.chats.iter().find(|c| c.id == peer).filter(|c| c.unread > 0 && c.read_in > 0).map(|c| c.read_in);
        self.scroll.unread = read_in.map(|read_in| Unread { peer, read_in, first: None, asked: false });
    }

    /// A history page came: jump to the first unread message, or ask for the page that has it.
    pub(super) fn unread_after_history(&mut self, peer: PeerId) {
        let Some(u) = self.scroll.unread.filter(|u| u.peer == peer && u.first.is_none()) else { return };
        let Some(list) = self.history.get(&peer) else { return };
        match list.iter().find(|m| !m.out && m.id > u.read_in).map(|m| m.id) {
            // Seen only if the page goes back far enough: otherwise an older unread one may be missing.
            Some(id) if list.first().is_some_and(|m| m.id <= u.read_in) || u.asked => {
                self.scroll.unread = Some(Unread { first: Some(id), ..u });
                self.jump_in_chat(peer, id);
            }
            _ if !u.asked && list.first().is_some_and(|m| m.id > u.read_in) => {
                self.scroll.unread = Some(Unread { asked: true, ..u });
                self.send(Cmd::OpenAt { peer, id: u.read_in });
            }
            _ => {}
        }
    }

    /// The first unread message of this chat, for the divider.
    fn unread_divider(&self, peer: PeerId) -> Option<i32> {
        self.scroll.unread.filter(|u| u.peer == peer).and_then(|u| u.first)
    }

    pub(super) fn chat_area(&mut self, ui: &mut Ui, rect: Rect) {
        let Some(peer) = self.selected else {
            let pill = Rect::from_center_size(rect.center(), vec2(230.0, 44.0));
            glass::glass(ui.painter(), pill, Glass::panel(22.0));
            ui.painter().text(pill.center(), Align2::CENTER_CENTER, l!("Оберіть чат зліва", "Select a chat on the left"), FontId::proportional(15.0), ink());
            return;
        };
        let Some(chat) = self.chats.iter().find(|c| c.id == peer).cloned() else { return };
        if chat.forum && self.forum.open.is_none() {
            self.forum_view(ui, peer, &chat, rect);
            return;
        }
        let topic = self.forum.open.filter(|_| chat.forum);
        let source = self.saved.source.filter(|_| chat.kind == ChatKind::Saved);
        let group = matches!(chat.kind, ChatKind::Group);

        let now = ui.input(|i| i.time);
        if self.scroll.jump.is_some_and(|(p, _, start)| p != peer || now - start > JUMP_FLASH) {
            self.scroll.jump = None;
        }
        self.chat_history(ui, peer, &chat, rect, topic, source, group, now);
        let header = self.chat_header(ui, peer, &chat, rect, topic, group, now);
        self.chat_bottom(ui, peer, &chat, rect, header, now);
    }

    /// The message list, scrolling under the floating header and field.
    #[allow(clippy::too_many_arguments)]
    fn chat_history(&mut self, ui: &mut Ui, peer: PeerId, chat: &ChatInfo, rect: Rect, topic: Option<i32>, source: Option<PeerId>, group: bool, now: f64) {
        // In channels views replace ticks; in "Saved Messages" there is nobody to read.
        let ticks = |m: &Msg| (m.out && !matches!(chat.kind, ChatKind::Channel | ChatKind::Saved)).then_some(m.id <= chat.read_out);
        let bar = if self.compose == Compose::New { 0.0 } else { COMPOSE_BAR_H + 8.0 };
        // Messages scroll under the floating glass header and input field.
        let mut load_older = None;
        let mut action = None;
        let (mut ad_seen, mut ad_action) = (None, None);
        let mut open_calendar = None;
        let mut child = ui.new_child(UiBuilder::new().max_rect(rect));
        child.set_clip_rect(rect);
        let pin_h = self.pin_bar_h(peer) + self.saved_bar_h(peer);
        let scroll_bottom = std::mem::take(&mut self.scroll.to_bottom);
        let scrolled = egui::ScrollArea::vertical()
            .id_salt(("history", peer))
            .auto_shrink(false)
            .stick_to_bottom(true)
            .show(&mut child, |ui| {
                ui.add_space(HEADER_H + 16.0 + pin_h);
                match self.history.get(&peer) {
                    None => {
                        ui.add_space(40.0);
                        ui.vertical_centered(|ui| ui.add(egui::Spinner::new().color(ink())));
                    }
                    Some(all) => {
                        // In a forum, show only the open topic.
                        // Saved Messages likewise show only the picked source.
                        let msgs: Vec<&Msg> = all.iter().filter(|m| topic.is_none_or(|t| m.topic_id() == t) && source.is_none_or(|s| m.saved_from == Some(s))).collect();
                        let exhausted = match (topic, source) {
                            (Some(t), _) => self.forum.exhausted.contains(&(peer, t)),
                            (_, Some(s)) => self.saved.exhausted.contains(&s),
                            _ => self.exhausted.get(&peer).copied().unwrap_or(false),
                        };
                        if !msgs.is_empty() && !exhausted {
                            let w = ui.available_width();
                            let (r, _) = ui.allocate_exact_size(vec2(w, 36.0), Sense::hover());
                            let b = Rect::from_center_size(r.center(), vec2(200.0, 32.0));
                            let label = if self.loading_older { l!("Завантаження…", "Loading…") } else { l!("Раніші повідомлення", "Earlier messages") };
                            if glass::button(ui, b, ("older", peer), label, false, !self.loading_older).clicked() {
                                load_older = Some(msgs[0].id);
                            }
                        }
                        let mut last_day = "";
                        let unread_from = self.unread_divider(peer);
                        let jump = self.scroll.jump.filter(|j| j.0 == peer);
                        let mut heights = self.scroll.row_h.borrow_mut();
                        let spacing = ui.spacing().item_spacing.y;
                        let mut i = 0;
                        while i < msgs.len() {
                            let m = msgs[i];
                            // Album: consecutive photos with the same grouped_id form one row.
                            let mut end = i + 1;
                            if m.photo && m.group.is_some() {
                                while end < msgs.len() && msgs[end].photo && msgs[end].group == m.group {
                                    end += 1;
                                }
                            }
                            let album = &msgs[i..end];
                            i = end;
                            let new_day = m.day != last_day;
                            last_day = &m.day;
                            // Virtualization: a row far from the screen and already measured is just empty space.
                            // The height may be stale, but it's remeasured as soon as the row comes close.
                            let top = ui.cursor().top();
                            let jumping = jump.is_some_and(|j| album.iter().any(|x| x.id == j.1));
                            if !jumping
                                && let Some(&h) = heights.get(&(peer, m.id))
                                && !ui.is_rect_visible(Rect::from_x_y_ranges(ui.max_rect().x_range(), top..=top + h).expand(VIRTUAL_MARGIN))
                            {
                                ui.allocate_space(vec2(ui.available_width(), (h - spacing).max(0.0)));
                                continue;
                            }
                            if new_day && day_chip(ui, &m.day).clicked() {
                                open_calendar = Some(m.ts);
                            }
                            if unread_from == Some(m.id) {
                                unread_bar(ui);
                            }
                            if album.len() > 1 {
                                let ids: Vec<i32> = album.iter().map(|m| m.id).collect();
                                let look = Look {
                                    chosen: &[],
                                    group,
                                    revealed: false,
                                    picking: !self.picked.is_empty(),
                                    picked: ids.iter().all(|id| self.picked.contains(id)),
                                    play: PlayView::default(),
                                    ticks: ticks(m),
                                    chat: Some(chat),
                                    pending: None,
                                };
                                let texs: Vec<_> = album.iter().map(|m| self.textures.get(&ImgKey::Photo(peer, m.id))).collect();
                                if let Some(a) = album_bubble(ui, album, &texs, look) {
                                    action = Some(a);
                                }
                                heights.insert((peer, m.id), ui.cursor().top() - top);
                                continue;
                            }
                            if let Some(a) = self.message_row(ui, peer, chat, &msgs, m, topic, group, jump, now) {
                                action = Some(a);
                            }
                            heights.insert((peer, m.id), ui.cursor().top() - top);
                        }
                        // The ad goes after the newest post, as in Telegram Desktop.
                        if topic.is_none()
                            && !msgs.is_empty()
                            && let Some(ad) = self.ads.first(peer)
                        {
                            let tex = ad.photo.and_then(|id| self.textures.get(&ImgKey::Sponsored(id)));
                            let (visible, a) = sponsored::card(ui, ad, tex);
                            if visible {
                                ad_seen = Some(ad.random_id.clone());
                            }
                            ad_action = a;
                        }
                        if msgs.is_empty() {
                            ui.add_space(60.0);
                            ui.vertical_centered(|ui| ui.label(egui::RichText::new(l!("Тут поки порожньо", "Nothing here yet")).color(ink_dim())));
                        }
                    }
                }
                ui.add_space(self.composer_h + 26.0 + bar);
                if scroll_bottom {
                    ui.scroll_to_cursor(Some(Align::BOTTOM));
                }
            });
        self.track_bottom(&scrolled);
        if let Some(before) = load_older {
            self.loading_older = true;
            match (topic, source) {
                (Some(t), _) => self.send(Cmd::OpenTopic { peer, topic: t, before: Some(before) }),
                (_, Some(s)) => self.send(Cmd::LoadSavedHistory { saved: peer, source: s, before: Some(before) }),
                _ => self.send(Cmd::LoadOlder { peer, before }),
            }
        }
        if let Some(a) = action {
            self.apply(peer, a);
        }
        if let Some(ts) = open_calendar {
            self.open_calendar(peer, Some(ts));
        }
        if let Some(id) = ad_seen {
            self.ad_seen(&id);
        }
        if let Some(a) = ad_action {
            self.ad_action(peer, a);
        }

    }

    /// One message (not an album) with its reply quote, comments button and the jump highlight.
    #[allow(clippy::too_many_arguments)]
    fn message_row(&self, ui: &mut Ui, peer: PeerId, chat: &ChatInfo, msgs: &[&Msg], m: &Msg, topic: Option<i32>, group: bool, jump: Option<(PeerId, i32, f64)>, now: f64) -> Option<Action> {
        // In channels views replace ticks; in "Saved Messages" there is nobody to read.
        let ticks = |m: &Msg| (m.out && !matches!(chat.kind, ChatKind::Channel | ChatKind::Saved)).then_some(m.id <= chat.read_out);
        let mut action = None;
        // In a forum, a "reply" to the topic root is just a message in the topic, no quote.
        let reply = m.reply_to.filter(|r| topic.is_none_or(|t| *r != t)).map(|id| match msgs.iter().find(|x| x.id == id) {
            Some(r) => (author(r, &self.me), quote(r)),
            None => (l!("Відповідь", "Reply").to_owned(), l!("Повідомлення", "Message").to_owned()),
        });
        let kind = m.media.as_ref().map(|i| i.kind);
        let animated = self.settings.autoplay && m.video_sticker
            || matches!(kind, Some(MediaKind::Gif | MediaKind::Round))
            || (kind == Some(MediaKind::Video) && self.playback.player.is(peer, m.id));
        let clip = self.playback.clips.get(&(peer, m.id)).and_then(|c| c.tex.as_ref());
        let photo = clip.or(self.textures.get(&ImgKey::Photo(peer, m.id)));
        let sticker = clip.or(m.sticker.as_ref().and_then(|s| self.textures.get(&ImgKey::Sticker(s.0))));
        let revealed = self.revealed.contains(&(peer, m.id));
        let bubble_top = ui.cursor().top();
        let chosen = self.multi_vote.get(&(peer, m.id)).map_or(&[][..], |v| v.as_slice());
        let look = Look {
            chosen,
            group,
            revealed,
            picking: !self.picked.is_empty(),
            picked: self.picked.contains(&m.id),
            play: if m.media.is_some() { self.play_view(peer, m.id) } else { PlayView::default() },
            ticks: ticks(m).filter(|_| m.id > 0),
            chat: Some(chat),
            pending: self.pending_state(m.id),
        };
        if let Some(a) = bubble(ui, m, look, reply, photo, sticker) {
            action = Some(a);
        }
        if let Some(n) = m.comments {
            let w = (ui.available_width() * 0.72).min(340.0);
            let (row, _) = ui.allocate_exact_size(vec2(ui.available_width(), 36.0), Sense::hover());
            let b = Rect::from_min_size(pos2(row.left() + 10.0, row.top() + 2.0), vec2(w, 32.0));
            if glass::button(ui, b, ("comments", m.id), &format!("💬  {}", comments_label(n)), false, true).clicked() {
                action = Some(Action::Comments(m.id));
            }
        }
        if animated && ui.is_rect_visible(Rect::from_x_y_ranges(ui.max_rect().x_range(), bubble_top..=ui.cursor().top())) {
            self.playback.clip_wanted.borrow_mut().push((peer, m.id));
        }
        if let Some((_, id, start)) = jump
            && id == m.id
        {
            let row = Rect::from_x_y_ranges(ui.max_rect().x_range(), bubble_top..=ui.cursor().top());
            let age = now - start;
            // Several frames in a row: content height may change while photos load.
            if age < 0.3 {
                ui.scroll_to_rect(row, Some(Align::Center));
            }
            let a = (1.0 - age / JUMP_FLASH).clamp(0.0, 1.0) as f32;
            ui.painter().rect_filled(row.shrink2(vec2(4.0, 0.0)), 14.0, rgba(ACCENT, (a * 70.0) as u8));
        }
        action
    }

    /// The glass header: avatar or back arrow, title and status, search and calendar. Returns its rect.
    #[allow(clippy::too_many_arguments)]
    fn chat_header(&mut self, ui: &mut Ui, peer: PeerId, chat: &ChatInfo, rect: Rect, topic: Option<i32>, group: bool, now: f64) -> Rect {
        // Header.
        let header = Rect::from_min_size(rect.min, vec2(rect.width(), HEADER_H));
        glass::glass(ui.painter(), header, Glass::tinted(HEADER_H / 2.0, glass::chrome(), 110).with_shadow().frosted(0.96));
        let p = ui.painter().clone();
        let open_topic = topic.and_then(|t| self.forum.topics.get(&peer)?.iter().find(|x| x.id == t).cloned());
        if topic.is_some() {
            // In a topic, a back-to-topic-list button replaces the avatar.
            let back = Rect::from_center_size(pos2(header.left() + 34.0, header.center().y), vec2(40.0, 40.0));
            if glass::icon_button(ui, back, "topic-back", Icon::ChevronLeft, false, true).on_hover_text(l!("До списку тем", "Back to topics")).clicked() {
                self.forum.open = None;
                self.compose = Compose::New;
            }
        } else {
            let tex = self.textures.get(&ImgKey::Avatar(peer));
            glass::avatar(&p, pos2(header.left() + 34.0, header.center().y), 21.0, &chat.title, peer.bot_api_dialog_id_unchecked(), kind_icon(chat.kind), tex);
        }
        let search_btn = Rect::from_center_size(pos2(header.right() - 32.0, header.center().y), vec2(38.0, 38.0));
        let searching = self.chat_search.is_some();
        if glass::icon_button(ui, search_btn, "chat-search", Icon::Search, searching, true).on_hover_text(l!("Пошук у чаті", "Search in chat")).clicked() {
            self.chat_search = if searching { None } else { Some(Search::default()) };
        }
        let text_x = header.left() + 66.0;
        if self.chat_search.is_none() {
            // Click on the title opens chat info, as in Telegram Desktop.
            let left = if topic.is_some() { header.left() + 60.0 } else { header.left() + 8.0 };
            let area = Rect::from_min_max(pos2(left, header.top()), pos2(search_btn.left() - 6.0, header.bottom()));
            let resp = ui.interact(area, ui.scope_id().with(("chat-title", peer)), Sense::click());
            if resp.hovered() {
                ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
            }
            if resp.on_hover_text(l!("Інформація про чат", "Chat info")).clicked() {
                if self.info.is_some() {
                    self.info = None;
                } else {
                    self.open_info(peer);
                }
            }
        }
        if self.chat_search.is_some() {
            let cal = search_btn.translate(vec2(-44.0, 0.0));
            if glass::icon_button(ui, cal, "chat-calendar", Icon::Calendar, self.calendar.is_some(), true).on_hover_text(l!("Перейти до дати", "Jump to date")).clicked() {
                self.open_calendar(peer, None);
            }
        }
        if let Some(s) = &mut self.chat_search {
            let field = Rect::from_min_max(pos2(text_x - 4.0, header.top() + 11.0), pos2(search_btn.left() - 54.0, header.bottom() - 11.0));
            let hint = if s.picking { l!("Ім'я учасника", "Member name") } else { l!("Пошук у чаті", "Search in chat") };
            let resp = glass::search_field(ui, field, &mut s.query, hint);
            if !resp.has_focus() && s.query.is_empty() && s.edited_at == 0.0 {
                resp.request_focus();
                s.edited_at = now;
            }
            if resp.changed() {
                s.edited_at = now;
            }
            if let Some(query) = s.due(now) {
                s.requested = s.key();
                let (kind, from) = (s.kind, s.from.as_ref().map(|f| f.0));
                self.send(Cmd::Search { peer: Some(peer), query, kind, from });
            }
            self.search_members_due(peer, now);
        } else {
            let title_w = search_btn.left() - text_x - 12.0;
            let title = open_topic.as_ref().map_or(chat.title.as_str(), |t| t.title.as_str());
            p.galley(pos2(text_x, header.center().y - 20.0), elided(ui, title, 16.5, ink(), title_w), ink());
            let typing = self.typing.get(&peer).filter(|(_, until)| *until > now);
            let (sub, color) = match typing {
                Some((who, _)) if group && !who.is_empty() => (lf!("{who} друкує…", "{who} is typing…"), ACCENT),
                Some(_) => (l!("друкує…", "typing…").to_owned(), ACCENT),
                None if topic.is_some() => (lf!("тема · {}", "topic · {}", chat.title), ink_dim()),
                None => match chat.kind {
                    ChatKind::User => match chat.presence.text() {
                        Some(t) => (t, if chat.presence.online() { ACCENT } else { ink_dim() }),
                        None => (l!("особистий чат", "private chat").to_owned(), ink_dim()),
                    },
                    ChatKind::Bot => (l!("бот", "bot").to_owned(), ink_dim()),
                    ChatKind::Group => (l!("група", "group").to_owned(), ink_dim()),
                    ChatKind::Channel => (l!("канал", "channel").to_owned(), ink_dim()),
                    ChatKind::Saved => (l!("ваші нотатки", "your notes").to_owned(), ink_dim()),
                },
            };
            p.text(pos2(text_x, header.center().y + 11.0), Align2::LEFT_CENTER, sub, FontId::proportional(12.5), color);
        }

        header
    }

    /// The bottom: join / mute bar, Start for bots, or the field with its buttons; then the overlays.
    fn chat_bottom(&mut self, ui: &mut Ui, peer: PeerId, chat: &ChatInfo, rect: Rect, header: Rect, now: f64) {
        // Not a member of a public chat: "Join"; a channel subscriber gets only the notifications toggle.
        if !chat.joined || !chat.can_post {
            let bar = Rect::from_min_max(pos2(rect.left(), rect.bottom() - COMPOSER_H), rect.max);
            let (label, accent) = match (chat.joined, chat.muted) {
                (false, _) => (l!("Приєднатися", "Join"), true),
                (true, true) => (l!("Увімкнути сповіщення", "Unmute"), false),
                (true, false) => (l!("Вимкнути сповіщення", "Mute"), false),
            };
            if glass::button(ui, bar, ("bottom-bar", peer), label, accent, true).clicked() {
                if chat.joined {
                    self.apply_chat(peer, ChatAction::Mute(!chat.muted));
                } else {
                    self.send(Cmd::Join(peer));
                }
            }
            self.composer_h = COMPOSER_H;
            self.chat_overlays(ui, peer, rect, header, bar.top() - 8.0);
            return;
        }
        if self.start_bot_bar(ui, peer, chat.kind, rect) {
            self.composer_h = COMPOSER_H;
            self.chat_overlays(ui, peer, rect, header, rect.bottom() - COMPOSER_H - 8.0);
            return;
        }
        // [📎] [input field] [🙂] [➤]
        let circle = |x: f32| Rect::from_min_size(pos2(x, rect.bottom() - COMPOSER_H), vec2(COMPOSER_H, COMPOSER_H));
        let send_btn = circle(rect.right() - COMPOSER_H);
        let sticker_btn = circle(send_btn.left() - 8.0 - COMPOSER_H);
        let attach_btn = circle(rect.left());
        let n_scheduled = self.scheduled.get(&peer).map_or(0, Vec::len);
        let sched_btn = circle(sticker_btn.left() - 8.0 - COMPOSER_H);
        let input_right = if n_scheduled > 0 { sched_btn.left() } else { sticker_btn.left() } - 8.0;
        // The field grows with the text (Shift+Enter), buttons stay at the bottom.
        let bot_menu = self.bot_menu_width(peer, chat.kind);
        let input_left = attach_btn.right() + 8.0 + bot_menu;
        let rows = {
            let job = LayoutJob::simple(self.draft.clone(), FontId::proportional(glass::AREA_FONT), ink(), (input_right - input_left - 40.0).max(40.0));
            ui.painter().layout_job(job).rows.len().clamp(1, COMPOSER_ROWS)
        };
        let line_h = ui.fonts_mut(|f| f.row_height(&FontId::proportional(glass::AREA_FONT)));
        let input_h = (rows as f32 * line_h + 26.0).max(COMPOSER_H);
        self.composer_h = input_h;
        let input = Rect::from_min_max(pos2(input_left, rect.bottom() - input_h), pos2(input_right, rect.bottom()));
        let mut above = input.top() - 8.0;
        if self.compose != Compose::New {
            let bar_rect = Rect::from_min_max(pos2(rect.left(), above - COMPOSE_BAR_H), pos2(send_btn.right(), above));
            self.compose_bar(ui, peer, bar_rect);
            above = bar_rect.top() - 8.0;
        }
        above = self.link_preview_bar(ui, rect.left(), send_btn.right(), above, now);
        for u in self.uploads.iter().take(3) {
            above = upload_pill(ui, u, pos2(rect.left(), above)) - 6.0;
        }
        if self.uploads.len() > 3 {
            let more = lf!("і ще {}", "and {} more", self.uploads.len() - 3);
            ui.painter().text(pos2(rect.left() + 14.0, above - 8.0), Align2::LEFT_CENTER, more, FontId::proportional(12.5), ink_dim());
            above -= 20.0;
        }
        let b = Bottom { attach: attach_btn, sticker: sticker_btn, scheduled: sched_btn, send: send_btn, input, above };
        self.composer_buttons(ui, peer, rect, header, &b, n_scheduled);
        if self.replacement_bars(ui, peer, rect, header, &b, now) {
            return;
        }
        self.paste_files(ui, peer);
        if bot_menu > 0.0 {
            self.bot_menu_button(ui, circle(attach_btn.right() + 8.0));
        }
        let suggest_rect = Rect::from_min_max(pos2(rect.left(), (above - 300.0).max(header.bottom() + 8.0)), pos2(send_btn.right(), above));
        self.suggest_view(ui, peer, chat.kind, suggest_rect, now);
        glass::glass(ui.painter(), input, Glass::tinted((input.height() / 2.0).min(25.0), glass::chrome(), 110).with_shadow().frosted(0.96));
        let arrow_edit = self.can_edit_by_arrow() && ui.input(|i| i.key_pressed(Key::ArrowUp) && i.modifiers.is_none());
        let send_mods = self.send_mods();
        let (resp, submitted) = glass::text_area(ui, input, &mut self.draft, l!("Повідомлення", "Message"), send_mods);
        // ↑ in an empty composer edits the last own message, as in Telegram Desktop.
        if arrow_edit && resp.has_focus() {
            self.edit_last();
        }
        if resp.changed() {
            self.draft_edited_at = now;
            if !self.draft.is_empty() && now - self.typing_sent_at > TYPING_EVERY && chat.kind != ChatKind::Saved {
                self.typing_sent_at = now;
                self.send(Cmd::Typing(peer));
            }
        }
        if now - self.draft_edited_at > DRAFT_DELAY {
            self.flush_draft();
        }
        self.update_inline(peer, now, resp.changed());
        if self.inline.as_ref().is_some_and(|i| i.results_for.is_some()) {
            let panel = Rect::from_min_max(pos2(rect.left(), (above - 320.0).max(header.bottom() + 8.0)), pos2(rect.right(), above));
            self.inline_view(ui, peer, panel);
        }
        let enter = submitted;
        let can_send = !self.draft.trim().is_empty();
        let editing = matches!(self.compose, Compose::Edit(_));
        // Empty field shows the microphone button, as in Telegram.
        let mic = !can_send && !editing;
        let icon = if editing { Icon::Check } else if mic { Icon::Mic } else { Icon::Send };
        let mut send = glass::icon_button(ui, send_btn, "send", icon, true, can_send || mic);
        if mic {
            send = send.on_hover_text(l!("Записати голосове (права кнопка — відеоповідомлення)", "Record voice (right click — video message)"));
            let mut round = false;
            send.context_menu(|ui| {
                if glass::menu_button(ui, Icon::Video, l!("Записати відеоповідомлення", "Record video message")).clicked() {
                    round = true;
                }
            });
            if round {
                self.start_round(peer);
            }
        }
        if mic && send.clicked() {
            self.start_recording(peer);
        } else if send.clicked() || (enter && can_send) {
            self.send_draft();
            resp.request_focus();
        }
        // Right-click on "send": silent or scheduled, as in Telegram Desktop.
        if can_send && !editing {
            let mut choice = None;
            send.context_menu(|ui| {
                ui.set_min_width(200.0);
                if glass::menu_button(ui, Icon::BellOff, l!("Надіслати без звуку", "Send without sound")).clicked() {
                    choice = Some(false);
                }
                if glass::menu_button(ui, Icon::Clock, l!("Запланувати…", "Schedule…")).clicked() {
                    choice = Some(true);
                }
            });
            match choice {
                Some(false) => self.send_draft_with(SendOpts { silent: true, schedule: None, ..Default::default() }),
                Some(true) => self.schedule = Some((ScheduleFor::Draft, String::new())),
                None => {}
            }
        }
        if self.schedule.is_some() {
            let panel = Rect::from_min_max(pos2((send_btn.right() - 300.0).max(rect.left()), above - 236.0), pos2(send_btn.right(), above));
            self.schedule_view(ui, panel);
        }
        self.chat_overlays(ui, peer, rect, header, above);
    }

    /// Attach (with its menu), stickers and the scheduled list, and their panels.
    fn composer_buttons(&mut self, ui: &mut Ui, peer: PeerId, rect: Rect, header: Rect, b: &Bottom, n_scheduled: usize) {
        let Bottom { attach: attach_btn, sticker: sticker_btn, scheduled: sched_btn, send: send_btn, above, .. } = *b;
        let attach = glass::icon_button(ui, attach_btn, "attach", Icon::Attach, false, true).on_hover_text(l!("Надіслати файл", "Send file"));
        if attach.clicked() {
            self.pick_files(peer, false);
        }
        let can_poll = self.chats.iter().any(|c| c.id == peer && matches!(c.kind, ChatKind::Group | ChatKind::Channel | ChatKind::Bot | ChatKind::Saved));
        attach.context_menu(|ui| {
            if glass::menu_button(ui, Icon::File, l!("Надіслати без стиснення", "Send without compression")).clicked() {
                self.pick_files(peer, true);
            }
            // Polls go to groups, channels, bots and Saved Messages, as in Telegram.
            if can_poll && glass::menu_button(ui, Icon::Poll, l!("Опитування", "Poll")).clicked() {
                self.open_poll_form(peer);
            }
        });
        if glass::icon_button(ui, sticker_btn, "stickers", Icon::Smile, self.stickers.open, true).on_hover_text(l!("Стікери", "Stickers")).clicked() {
            self.stickers.open = !self.stickers.open;
            if self.stickers.open && !self.stickers.requested {
                self.stickers.requested = true;
                self.send(Cmd::LoadStickers);
            }
        }
        if n_scheduled > 0 {
            let hint = lf!("Заплановані: {n_scheduled}", "Scheduled: {n_scheduled}");
            if glass::icon_button(ui, sched_btn, "scheduled", Icon::Clock, self.scheduled_open, true).on_hover_text(hint).clicked() {
                self.scheduled_open = !self.scheduled_open;
            }
        }
        if self.stickers.open {
            let panel = Rect::from_min_max(pos2((send_btn.right() - 380.0).max(rect.left()), above - 340.0), pos2(send_btn.right(), above));
            self.sticker_view(ui, peer, panel);
        }
        if self.scheduled_open && n_scheduled > 0 {
            let panel = Rect::from_min_max(pos2(rect.left(), (above - 360.0).max(header.bottom() + 8.0)), pos2(rect.right(), above));
            self.scheduled_view(ui, peer, panel);
        }
    }

    /// Bars that take the place of the field: selected messages, a recording, a voice draft.
    /// Returns `true` when one was drawn (the overlays are drawn too).
    fn replacement_bars(&mut self, ui: &mut Ui, peer: PeerId, rect: Rect, header: Rect, b: &Bottom, now: f64) -> bool {
        let Bottom { send: send_btn, input, above, .. } = *b;
        if !self.picked.is_empty() {
            let bar = Rect::from_min_max(pos2(rect.left(), send_btn.top()), send_btn.max);
            self.selection_bar(ui, peer, bar);
            self.chat_overlays(ui, peer, rect, header, above);
            return true;
        }
        if self.rec.voice.as_ref().is_some_and(|(p, _)| *p != peer) {
            self.cancel_recording();
        }
        if self.rec.draft.as_ref().is_some_and(|(p, _)| *p != peer) || self.rec.round.as_ref().is_some_and(|(p, _)| *p != peer) {
            self.cancel_recording();
        }
        if self.rec.round.is_some() {
            self.round_bar(ui, input, rect, now);
            if glass::icon_button(ui, send_btn, "send", Icon::Send, true, true).on_hover_text(l!("Надіслати відеоповідомлення", "Send video message")).clicked() {
                self.send_round();
            }
            self.chat_overlays(ui, peer, rect, header, above);
            return true;
        }
        if self.rec.draft.is_some() {
            self.voice_draft_bar(ui, input);
            if glass::icon_button(ui, send_btn, "send", Icon::Send, true, true).on_hover_text(l!("Надіслати голосове", "Send voice message")).clicked() {
                self.send_recording();
            }
            self.chat_overlays(ui, peer, rect, header, above);
            return true;
        }
        if self.rec.voice.is_some() {
            self.recording_bar(ui, input, now);
            if glass::icon_button(ui, send_btn, "send", Icon::Send, true, true).on_hover_text(l!("Надіслати голосове", "Send voice message")).clicked() {
                self.send_recording();
            }
            self.chat_overlays(ui, peer, rect, header, above);
            return true;
        }
        false
    }

    /// Panels over the chat (search, forwarding) and Esc handling.
    pub(super) fn chat_overlays(&mut self, ui: &mut Ui, peer: PeerId, rect: Rect, header: Rect, above: f32) {
        self.chat_view_overlays(ui, peer, rect, header, above);
        if self.comments.as_ref().is_some_and(|c| c.peer != peer) {
            self.comments = None;
        }
        if self.comments.is_some() {
            let w = rect.width().min(440.0);
            let panel = Rect::from_min_max(pos2(rect.right() - w, header.bottom() + 8.0), rect.max);
            self.comments_view(ui, panel);
        }
        if self.chat_search.is_some() {
            let panel = Rect::from_min_max(pos2(rect.left(), header.bottom() + 8.0), pos2(rect.right(), (header.bottom() + 340.0).min(above - 8.0)));
            self.chat_search_view(ui, peer, panel);
        }
        if self.forward.is_some() {
            let w = rect.width().min(380.0);
            let panel = Rect::from_center_size(rect.center(), vec2(w, (rect.height() - 120.0).min(480.0)));
            self.forward_view(ui, panel);
        }
        let esc = ui.input(|i| i.key_pressed(Key::Escape));
        // Windows above the chat close themselves on this Esc.
        if !esc || self.modal_open() {
            return;
        }
        if self.viewer.is_some() {
            // Closes the viewer itself.
        } else if self.comments.is_some() {
            self.comments = None;
        } else if self.rec.voice.is_some() || self.rec.draft.is_some() || self.rec.round.is_some() {
            self.cancel_recording();
        } else if self.forward.is_some() {
            self.forward = None;
        } else if self.inline.as_ref().is_some_and(|i| i.results_for.is_some()) {
            self.inline = None;
        } else if self.scheduled_open {
            self.scheduled_open = false;
        } else if self.schedule.is_some() {
            self.schedule = None;
        } else if self.stickers.open {
            self.stickers.open = false;
        } else if self.chat_search.is_some() {
            self.chat_search = None;
        } else if !self.picked.is_empty() {
            self.picked.clear();
        } else if self.compose != Compose::New {
            self.cancel_compose();
        } else if let Some(i) = &mut self.info {
            if i.confirm.take().is_none() {
                self.info = None;
            }
        } else if self.forum.open.is_some() {
            self.forum.open = None;
        } else {
            self.esc_fallback();
        }
    }

    /// Bottom panel in message selection mode.
    pub(super) fn selection_bar(&mut self, ui: &mut Ui, peer: PeerId, rect: Rect) {
        glass::glass(ui.painter(), rect, Glass::tinted(rect.height() / 2.0, glass::chrome(), 140).with_shadow().frosted(0.96));
        let close = Rect::from_center_size(pos2(rect.left() + rect.height() / 2.0, rect.center().y), vec2(36.0, 36.0));
        if glass::icon_button(ui, close, "pick-close", Icon::Close, false, true).clicked() {
            self.picked.clear();
            return;
        }
        let n = self.picked.len();
        ui.painter().text(pos2(close.right() + 12.0, rect.center().y), Align2::LEFT_CENTER, lf!("Вибрано: {n}", "Selected: {n}"), FontId::proportional(15.0), ink());
        let all_out = self.picked.iter().all(|id| self.find(peer, *id).is_some_and(|m| m.out));
        let btn = |i: f32| Rect::from_center_size(pos2(rect.right() - 30.0 - i * 46.0, rect.center().y), vec2(38.0, 38.0));
        let protected = self.protected(peer, &self.picked);
        if protected {
            if all_out && glass::icon_button(ui, btn(0.0), "pick-delete", Icon::Trash, false, true).on_hover_text(l!("Видалити", "Delete")).clicked() {
                let ids = std::mem::take(&mut self.picked);
                self.send(Cmd::Delete { peer, ids });
            }
            return;
        }
        if glass::icon_button(ui, btn(0.0), "pick-forward", Icon::Forward, true, true).on_hover_text(l!("Переслати", "Forward")).clicked() {
            let mut ids = std::mem::take(&mut self.picked);
            ids.sort_unstable();
            self.forward = Some(ForwardPicker { from: peer, ids, query: String::new() });
        }
        if glass::icon_button(ui, btn(1.0), "pick-copy", Icon::Copy, false, true).on_hover_text(l!("Копіювати", "Copy")).clicked() {
            let mut msgs: Vec<&Msg> = self.picked.iter().filter_map(|id| self.find(peer, *id)).collect();
            msgs.sort_by_key(|m| m.id);
            let text = msgs.iter().map(|m| format!("{} [{}]\n{}", author(m, &self.me), m.time, m.text)).collect::<Vec<_>>().join("\n\n");
            self.ctx.copy_text(text);
            self.picked.clear();
            self.toast = Some((l!("Скопійовано", "Copied").into(), 0.0, false));
        }
        if all_out && glass::icon_button(ui, btn(2.0), "pick-delete", Icon::Trash, false, true).on_hover_text(l!("Видалити", "Delete")).clicked() {
            let ids = std::mem::take(&mut self.picked);
            self.send(Cmd::Delete { peer, ids });
        }
    }
}

/// "Unread messages" across the history, above the first message that came while we were away.
fn unread_bar(ui: &mut Ui) {
    let w = ui.available_width();
    let (r, _) = ui.allocate_exact_size(vec2(w, 30.0), Sense::hover());
    let band = Rect::from_center_size(r.center(), vec2(w, 24.0));
    ui.painter().rect_filled(band, 0.0, glass::veil(16));
    ui.painter().text(band.center(), Align2::CENTER_CENTER, l!("Непрочитані повідомлення", "Unread messages"), FontId::proportional(12.5), ink_dim());
}
