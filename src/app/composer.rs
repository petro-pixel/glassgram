//! The message field and everything it sends: text, files, stickers, emoji, inline bots.

use super::*;

/// Stickers offered above the field for a typed emoji.
const SUGGEST_MAX: usize = 8;

/// The sticker and emoji panel: sets, the open set, thumbnails asked for.
pub(super) struct StickerPanel {
    /// Installed sticker sets.
    pub(super) sets: Vec<StickerSetInfo>,
    /// The panel is open.
    pub(super) open: bool,
    /// Index of the set shown.
    pub(super) set: usize,
    /// Sticker previews already asked for.
    pub(super) thumbs: HashSet<i64>,
    /// The sets were asked for once this session.
    pub(super) requested: bool,
    /// Emoji tab in the sticker panel.
    pub(super) emoji_tab: bool,
}

impl App {
    pub(super) fn send_draft(&mut self) {
        self.send_draft_with(SendOpts::default());
    }

    pub(super) fn send_draft_with(&mut self, opts: SendOpts) {
        let opts = SendOpts { no_webpage: opts.no_webpage || self.link_preview_off(), ..opts };
        let text = self.draft.trim().to_owned();
        let Some(peer) = self.selected else { return };
        // A media message may lose its caption.
        if text.is_empty() && self.editing_media().is_none() {
            return;
        }
        self.draft.clear();
        match self.compose {
            Compose::Edit(id) => {
                self.compose = Compose::New;
                if let Some(m) = self.history.get_mut(&peer).and_then(|l| l.iter_mut().find(|m| m.id == id)) {
                    let (plain, ents) = format::parse(&text);
                    m.spans = format::from_entities(&plain, &ents, 0);
                    m.text = plain;
                    m.edited = true;
                }
                self.draft = std::mem::take(&mut self.edit_stash);
                self.send_media_edit(peer, id, text);
            }
            // Scheduled messages go straight to the server; the rest through the send queue.
            _ => {
                let reply_to = self.take_reply();
                if opts.schedule.is_some() {
                    self.send(Cmd::Send { peer, text, reply_to, opts });
                } else {
                    self.enqueue(peer, text, reply_to, &opts);
                }
            }
        }
        if opts.schedule.is_some() {
            self.send(Cmd::LoadScheduled(peer));
        }
        // After sending, the server clears the draft itself.
        if self.draft.is_empty() {
            self.draft_synced.clear();
            if let Some(c) = self.chat_mut(peer) {
                c.draft.clear();
            }
        }
    }

    /// What a new message replies to: an explicit reply or the open forum topic
    /// (replying to the topic's first message puts the message into it).
    /// Modifiers that send from the composer (Settings → Chat Settings).
    pub(super) fn send_mods(&self) -> egui::Modifiers {
        if self.settings.ctrl_enter { egui::Modifiers::COMMAND } else { egui::Modifiers::NONE }
    }

    pub(super) fn reply_target(&self) -> Option<ReplyTo> {
        let quote = |peer: PeerId, id: i32| self.quoting.get(peer, id).cloned();
        match (self.compose, self.selected) {
            (Compose::Reply(id), Some(peer)) => Some(ReplyTo { id, peer: None, quote: quote(peer, id) }),
            (Compose::Reply(id), None) => Some(ReplyTo::to(id)),
            (_, peer) => match peer.and_then(|p| self.quoting.elsewhere(p)) {
                Some((from, id)) => Some(ReplyTo { id, peer: Some(from), quote: quote(from, id) }),
                None => self.forum.open.filter(|t| *t != GENERAL_TOPIC).map(ReplyTo::to),
            },
        }
    }

    /// The reply for a message being sent now; the reply bar closes with it.
    pub(super) fn take_reply(&mut self) -> Option<ReplyTo> {
        let reply = self.reply_target();
        let elsewhere = self.selected.is_some_and(|p| self.quoting.elsewhere(p).is_some());
        if matches!(self.compose, Compose::Reply(_)) || elsewhere {
            self.compose = Compose::New;
            self.quoting.clear();
        }
        reply
    }

    /// System file picker in a separate thread, so as not to block the UI.
    pub(super) fn pick_files(&self, peer: PeerId, as_file: bool) {
        let (tx, ctx) = (self.picked_tx.clone(), self.ctx.clone());
        let title = if as_file { l!("Надіслати без стиснення", "Send without compression") } else { l!("Надіслати файли", "Send files") };
        std::thread::spawn(move || {
            let paths = rfd::FileDialog::new().set_title(title).pick_files().unwrap_or_default();
            if !paths.is_empty() {
                let _ = tx.send((peer, paths, as_file));
            }
            ctx.request_repaint();
        });
    }

    /// Installed stickers for the emoji typed alone in the field (Settings → "Suggest stickers by emoji").
    fn suggested_stickers(&mut self) -> Vec<(i64, String)> {
        // Variation selectors differ between keyboards and sticker sets.
        let plain = |s: &str| s.chars().filter(|c| *c != '\u{fe0f}').collect::<String>();
        let typed = plain(self.draft.trim());
        if !self.settings.suggest_stickers || matches!(self.compose, Compose::Edit(_)) || typed.is_empty() || typed.chars().count() > 4 || typed.chars().any(|c| c.is_alphanumeric() || c.is_whitespace()) {
            return Vec::new();
        }
        if !self.stickers.requested {
            self.stickers.requested = true;
            self.send(Cmd::LoadStickers);
        }
        let mut seen = HashSet::new();
        self.stickers.sets.iter().flat_map(|s| &s.stickers).filter(|(id, e)| plain(e) == typed && seen.insert(*id)).take(SUGGEST_MAX).cloned().collect()
    }

    /// A row of stickers above the field for the typed emoji; a click sends one instead of the text.
    pub(super) fn sticker_suggest_view(&mut self, ui: &mut Ui, peer: PeerId, rect: Rect) {
        let ids = self.suggested_stickers();
        if ids.is_empty() {
            return;
        }
        let want: Vec<i64> = ids.iter().map(|(id, _)| *id).filter(|id| self.stickers.thumbs.insert(*id)).collect();
        if !want.is_empty() {
            self.send(Cmd::LoadStickerThumbs(want));
        }
        let cell = 64.0;
        let w = (ids.len() as f32 * cell + 16.0).min(rect.width());
        let panel = Rect::from_min_size(pos2(rect.left(), rect.bottom() - cell - 16.0), vec2(w, cell + 16.0));
        glass::glass(ui.painter(), panel, Glass::tinted(22.0, glass::chrome(), 160).frosted(1.0).with_shadow());
        ui.interact(panel, ui.scope_id().with("sticker-suggest-bg"), Sense::click());
        let mut send = None;
        for (k, (id, emoji)) in ids.iter().enumerate() {
            let r = Rect::from_min_size(pos2(panel.left() + 8.0 + k as f32 * cell, panel.top() + 8.0), vec2(cell, cell));
            if r.right() > panel.right() {
                break;
            }
            let resp = ui.interact(r, ui.scope_id().with(("sticker-suggest", *id)), Sense::click());
            let hover = ui.ctx().animate_bool(resp.id, resp.hovered());
            if hover > 0.0 {
                ui.painter().rect_filled(r, 14.0, glass::veil((hover * 22.0) as u8));
            }
            match self.textures.get(&ImgKey::Sticker(*id)) {
                Some(t) => glass::image_contain(ui.painter(), r.shrink(6.0 - hover * 2.0), t),
                None => {
                    ui.painter().text(r.center(), Align2::CENTER_CENTER, emoji, FontId::proportional(cell * 0.45), ink());
                }
            }
            if resp.on_hover_cursor(egui::CursorIcon::PointingHand).clicked() {
                send = Some(*id);
            }
        }
        if let Some(doc) = send {
            self.draft.clear();
            let reply_to = self.take_reply();
            self.send(Cmd::SendSticker { peer, doc, reply_to });
        }
    }

    pub(super) fn request_sticker_thumbs(&mut self) {
        let Some(set) = self.stickers.sets.get(self.stickers.set) else { return };
        let ids: Vec<i64> = set.stickers.iter().map(|s| s.0).filter(|id| self.stickers.thumbs.insert(*id)).collect();
        if !ids.is_empty() {
            self.send(Cmd::LoadStickerThumbs(ids));
        }
    }

    /// `@bot query` in the input field triggers an inline query after a pause in typing.
    pub(super) fn update_inline(&mut self, peer: PeerId, now: f64, changed: bool) {
        if changed {
            self.inline_edited_at = now;
        }
        let parsed = self.draft.strip_prefix('@').and_then(|rest| {
            let (bot, query) = rest.split_once(' ')?;
            let ok = bot.len() >= 3 && bot.chars().all(|c| c.is_ascii_alphanumeric() || c == '_');
            ok.then(|| (bot.to_owned(), query.to_owned()))
        });
        let Some((bot, query)) = parsed else {
            self.inline = None;
            return;
        };
        let i = self.inline.get_or_insert_with(|| Inline {
            bot: String::new(),
            query: String::new(),
            requested: None,
            results: Vec::new(),
            query_id: 0,
            results_for: None,
        });
        (i.bot, i.query) = (bot.clone(), query.clone());
        let key = Some((bot.clone(), query.clone()));
        if i.requested != key && now - self.inline_edited_at > SEARCH_DELAY {
            i.requested = key;
            self.send(Cmd::InlineQuery { peer, bot, query });
        }
    }

    pub(super) fn inline_view(&mut self, ui: &mut Ui, peer: PeerId, rect: Rect) {
        let Some(i) = &self.inline else { return };
        let rows = (i.results.len().max(1) as f32 * 58.0 + 20.0).min(rect.height());
        let rect = Rect::from_min_max(pos2(rect.left(), rect.bottom() - rows), rect.max);
        glass::glass(ui.painter(), rect, Glass::tinted(22.0, glass::chrome(), 160).frosted(1.0).with_shadow());
        ui.interact(rect, ui.scope_id().with("inline-bg"), Sense::click());
        if i.results.is_empty() {
            let text = lf!("@{} нічого не знайшов", "@{} found nothing", i.bot);
            ui.painter().text(rect.center(), Align2::CENTER_CENTER, text, FontId::proportional(14.0), ink_dim());
            return;
        }
        let inner = rect.shrink2(vec2(8.0, 10.0));
        let mut chosen = None;
        let mut child = ui.new_child(UiBuilder::new().max_rect(inner));
        child.set_clip_rect(inner);
        egui::ScrollArea::vertical().id_salt("inline-results").auto_shrink(false).show(&mut child, |ui| {
            for r in &i.results {
                let w = ui.available_width();
                let (row, resp) = ui.allocate_exact_size(vec2(w, 56.0), Sense::click());
                let hover = ui.ctx().animate_bool(resp.id, resp.hovered());
                if hover > 0.0 {
                    ui.painter().rect_filled(row, 14.0, glass::veil((hover * 16.0) as u8));
                }
                let kind = match r.kind.as_str() {
                    "photo" => Icon::Camera,
                    "article" => Icon::File,
                    "location" | "venue" => Icon::Location,
                    _ => Icon::Attach,
                };
                let badge = Rect::from_center_size(pos2(row.left() + 26.0, row.center().y), vec2(38.0, 38.0));
                glass::glass(ui.painter(), badge, Glass::tinted(19.0, ACCENT, 150));
                icons::draw(ui.painter(), Rect::from_center_size(badge.center(), vec2(18.0, 18.0)), kind, ink());
                let title = if r.title.is_empty() { r.kind.clone() } else { r.title.clone() };
                ui.painter().galley(pos2(row.left() + 56.0, row.top() + 9.0), elided(ui, &title, 14.5, ink(), w - 66.0), ink());
                ui.painter().galley(pos2(row.left() + 56.0, row.top() + 30.0), elided(ui, &r.description, 13.0, ink_dim(), w - 66.0), ink_dim());
                if resp.clicked() {
                    chosen = Some(r.id.clone());
                }
            }
        });
        if let Some(result) = chosen
            && let Some(i) = self.inline.take()
        {
            let reply_to = self.take_reply();
            self.send(Cmd::SendInline { peer, query_id: i.query_id, result, reply_to });
            self.draft.clear();
            self.compose = Compose::New;
        }
    }

    /// Ctrl+V with an image or copied files sends them instead of pasting text.
    pub(super) fn paste_files(&mut self, ui: &mut Ui, peer: PeerId) {
        let pasted = ui.input(|i| {
            i.events.iter().any(|e| matches!(e, egui::Event::Paste(_)))
                || (i.modifiers.command && i.key_pressed(Key::V))
        });
        if !pasted {
            return;
        }
        let Ok(mut clip) = arboard::Clipboard::new() else { return };
        let files: Vec<PathBuf> = match clip.get().file_list() {
            Ok(list) if !list.is_empty() => list,
            _ => match clip.get_image() {
                Ok(img) => save_clipboard_image(&img).into_iter().collect(),
                Err(_) => return,
            },
        };
        if files.is_empty() {
            return;
        }
        ui.ctx().input_mut(|i| i.events.retain(|e| !matches!(e, egui::Event::Paste(_))));
        self.send_files(peer, files, false);
    }

    /// Emoji and sticker panel: tabs on top (emoji, then sets), grid below.
    pub(super) fn sticker_view(&mut self, ui: &mut Ui, peer: PeerId, rect: Rect) {
        glass::glass(ui.painter(), rect, Glass::tinted(24.0, glass::chrome(), 120).frosted(1.0).with_shadow());
        // Swallow clicks so they don't reach the messages under the panel.
        ui.interact(rect, ui.scope_id().with("sticker-bg"), Sense::click());
        let inner = rect.shrink(10.0);
        let tabs = Rect::from_min_size(inner.min, vec2(inner.width(), 40.0));
        let mut chosen_set = None;
        let mut emoji_clicked = false;
        let mut child = ui.new_child(UiBuilder::new().max_rect(tabs));
        child.set_clip_rect(tabs);
        egui::ScrollArea::horizontal()
            .id_salt("sticker-tabs")
            .scroll_bar_visibility(egui::scroll_area::ScrollBarVisibility::AlwaysHidden)
            .show(&mut child, |ui| {
                ui.horizontal(|ui| {
                    ui.spacing_mut().item_spacing.x = 6.0;
                    let tab = |ui: &mut Ui, active: bool| {
                        let (r, resp) = ui.allocate_exact_size(vec2(40.0, 40.0), Sense::click());
                        if active {
                            glass::glass(ui.painter(), r, Glass::tinted(12.0, ACCENT, 120));
                        } else if resp.hovered() {
                            ui.painter().rect_filled(r, 12.0, glass::veil(18));
                        }
                        (r, resp)
                    };
                    let (r, resp) = tab(ui, self.stickers.emoji_tab);
                    icons::draw(ui.painter(), Rect::from_center_size(r.center(), vec2(22.0, 22.0)), Icon::Smile, ink());
                    emoji_clicked = resp.on_hover_text(l!("Емодзі", "Emoji")).clicked();
                    for (i, set) in self.stickers.sets.iter().enumerate() {
                        let (r, resp) = tab(ui, !self.stickers.emoji_tab && i == self.stickers.set);
                        let first = set.stickers.first();
                        match first.and_then(|s| self.textures.get(&ImgKey::Sticker(s.0))) {
                            Some(t) => glass::image_contain(ui.painter(), r.shrink(5.0), t),
                            None => {
                                let label = first.map(|s| s.1.clone()).filter(|e| !e.is_empty()).unwrap_or_else(|| glass::initials(&set.title));
                                ui.painter().text(r.center(), Align2::CENTER_CENTER, label, FontId::proportional(20.0), ink());
                            }
                        }
                        if resp.on_hover_text(&set.title).clicked() {
                            chosen_set = Some(i);
                        }
                    }
                    if self.stickers.sets.is_empty() && self.stickers.requested && self.backend.is_some() {
                        ui.add(egui::Spinner::new().color(ink_dim()));
                    }
                });
            });
        if emoji_clicked {
            self.stickers.emoji_tab = true;
        }
        if let Some(i) = chosen_set {
            self.stickers.emoji_tab = false;
            self.stickers.set = i;
            self.request_sticker_thumbs();
        }
        let body = Rect::from_min_max(pos2(inner.left(), tabs.bottom() + 6.0), inner.max);
        if self.stickers.emoji_tab {
            return self.emoji_grid(ui, body);
        }

        let Some(set) = self.stickers.sets.get(self.stickers.set) else {
            ui.painter().text(body.center(), Align2::CENTER_CENTER, l!("Немає встановлених стікерів", "No stickers installed"), FontId::proportional(14.0), ink_dim());
            return;
        };
        ui.painter().text(pos2(body.left() + 4.0, body.top()), Align2::LEFT_TOP, &set.title, FontId::proportional(13.0), ink_dim());
        let grid = Rect::from_min_max(pos2(body.left(), body.top() + 22.0), body.max);
        let mut send = None;
        let mut child = ui.new_child(UiBuilder::new().max_rect(grid));
        child.set_clip_rect(grid);
        egui::ScrollArea::vertical()
            .id_salt(("sticker-grid", set.id))
            .auto_shrink(false)
            .show(&mut child, |ui| {
                let cols = ((grid.width() / 70.0).floor() as usize).max(1);
                let cell = grid.width() / cols as f32 - 2.0;
                for row in set.stickers.chunks(cols) {
                    ui.horizontal(|ui| {
                        ui.spacing_mut().item_spacing.x = 2.0;
                        for (id, emoji) in row {
                            let (r, resp) = ui.allocate_exact_size(vec2(cell, cell), Sense::click());
                            let hover = ui.ctx().animate_bool(resp.id, resp.hovered());
                            if hover > 0.0 {
                                ui.painter().rect_filled(r, 14.0, glass::veil((hover * 22.0) as u8));
                            }
                            let r = r.shrink(6.0 - hover * 2.0);
                            match self.textures.get(&ImgKey::Sticker(*id)) {
                                Some(t) => glass::image_contain(ui.painter(), r, t),
                                None => {
                                    ui.painter().text(r.center(), Align2::CENTER_CENTER, emoji, FontId::proportional(cell * 0.45), ink());
                                }
                            }
                            if resp.clicked() {
                                send = Some(*id);
                            }
                        }
                    });
                }
            });
        if let Some(doc) = send {
            let reply_to = self.take_reply();
            self.compose = Compose::New;
            self.send(Cmd::SendSticker { peer, doc, reply_to });
        }
    }

    /// Emoji grid by category; a click adds the emoji to the input field.
    pub(super) fn emoji_grid(&mut self, ui: &mut Ui, rect: Rect) {
        let mut picked = None;
        let mut child = ui.new_child(UiBuilder::new().max_rect(rect));
        child.set_clip_rect(rect);
        egui::ScrollArea::vertical().id_salt("emoji-grid").auto_shrink(false).show(&mut child, |ui| {
            let cols = ((rect.width() / 40.0).floor() as usize).max(1);
            let cell = rect.width() / cols as f32 - 1.0;
            for (title, chars) in emoji::CATEGORIES {
                let title = title();
                ui.label(egui::RichText::new(title).size(12.5).color(ink_dim()));
                for row in emoji::list(chars).chunks(cols) {
                    ui.horizontal(|ui| {
                        ui.spacing_mut().item_spacing.x = 1.0;
                        for e in row {
                            let (r, resp) = ui.allocate_exact_size(vec2(cell, cell), Sense::click());
                            if resp.hovered() {
                                ui.painter().rect_filled(r, 10.0, glass::veil(22));
                            }
                            ui.painter().text(r.center(), Align2::CENTER_CENTER, e, FontId::proportional(cell * 0.62), ink());
                            if resp.clicked() {
                                picked = Some(e.clone());
                            }
                        }
                    });
                }
                ui.add_space(6.0);
            }
        });
        if let Some(e) = picked {
            self.draft.push_str(&e);
            self.draft_edited_at = ui.input(|i| i.time);
        }
    }

    /// Highlight while dragging files into the window.
    pub(super) fn drop_overlay(&mut self, ui: &mut Ui) {
        let (hovering, dropped) = ui.input(|i| (!i.raw.hovered_files.is_empty(), i.raw.dropped_files.clone()));
        let Some(peer) = self.selected else { return };
        if hovering {
            let r = ui.max_rect().shrink(40.0);
            glass::glass(ui.painter(), r, Glass::tinted(36.0, ACCENT, 90).frosted(0.9).with_shadow());
            ui.painter().text(r.center(), Align2::CENTER_CENTER, l!("Відпустіть, щоб надіслати", "Drop to send"), FontId::proportional(24.0), ink());
        }
        let paths: Vec<PathBuf> = dropped.iter().map(|f| f.path().to_path_buf()).filter(|p| !p.as_os_str().is_empty()).collect();
        self.send_files(peer, paths, false);
    }

    /// Strip above the input field: "Reply …" or "Editing".
    pub(super) fn compose_bar(&mut self, ui: &mut Ui, peer: PeerId, rect: Rect) {
        let quoted = match self.compose {
            Compose::Reply(id) => self.reply_quote(peer, id),
            _ => None,
        };
        let (icon, title, id) = match self.compose {
            Compose::Reply(id) if quoted.is_some() => (Icon::Reply, l!("Відповідь із цитатою", "Reply with quote"), id),
            Compose::Reply(id) => (Icon::Reply, l!("Відповідь", "Reply"), id),
            Compose::Edit(id) => (Icon::Pencil, l!("Редагування", "Editing"), id),
            Compose::New => return self.elsewhere_bar(ui, peer, rect),
        };
        glass::glass(ui.painter(), rect, Glass::tinted(rect.height() / 2.0, glass::chrome(), 130).with_shadow().frosted(0.96));
        let p = ui.painter().clone();
        icons::draw(&p, Rect::from_center_size(pos2(rect.left() + 20.0, rect.center().y), vec2(18.0, 18.0)), icon, ACCENT);
        p.rect_filled(Rect::from_min_size(pos2(rect.left() + 36.0, rect.top() + 8.0), vec2(3.0, rect.height() - 16.0)), 2.0, ACCENT);
        let (who, what) = match self.find(peer, id) {
            Some(m) => (if matches!(self.compose, Compose::Edit(_)) { title.to_owned() } else { format!("{title}: {}", author(m, &self.me)) }, quoted.clone().unwrap_or_else(|| quote(m))),
            None => (title.to_owned(), String::new()),
        };
        let close = Rect::from_center_size(pos2(rect.right() - 22.0, rect.center().y), vec2(28.0, 28.0));
        // Editing a photo or file: a button to replace it, and the new file's name under the title.
        let replaced = self.editing_media().is_some().then(|| self.replace_button(ui, Rect::from_center_size(pos2(close.left() - 22.0, rect.center().y), vec2(34.0, 34.0)))).flatten();
        let what = replaced.unwrap_or(what);
        let w = rect.width() - 100.0;
        let g1 = elided(ui, &who, 12.5, ACCENT, w);
        let g2 = elided(ui, &what, 13.0, ink_dim(), w);
        p.galley(pos2(rect.left() + 46.0, rect.top() + 5.0), g1, ACCENT);
        p.galley(pos2(rect.left() + 46.0, rect.top() + 21.0), g2, ink_dim());
        // As in Telegram Desktop, a click on a reply opens the message to quote a part of it.
        if let Compose::Reply(id) = self.compose {
            let body = Rect::from_min_max(rect.min, pos2(close.left() - 4.0, rect.bottom()));
            let resp = ui.interact(body, ui.scope_id().with("compose-quote"), Sense::click());
            if resp.on_hover_text(l!("Натисніть, щоб процитувати частину", "Click to quote a part")).on_hover_cursor(egui::CursorIcon::PointingHand).clicked() {
                self.open_quote(peer, peer, id);
            }
        }
        if glass::icon_button(ui, close, "compose-close", Icon::Close, false, true).clicked() {
            self.cancel_compose();
        }
    }

    pub(super) fn cancel_compose(&mut self) {
        if matches!(self.compose, Compose::Edit(_)) {
            self.draft = std::mem::take(&mut self.edit_stash);
        }
        self.clear_replacement();
        self.compose = Compose::New;
        self.quoting.clear();
    }
}

impl App {
    /// Inline bot results, sticker sets and upload progress for the field.
    pub(super) fn composer_event(&mut self, ev: Ev) {
        match ev {
            Ev::Stickers(sets) => {
                self.stickers.sets = sets;
                self.stickers.set = 0;
                self.request_sticker_thumbs();
            }
            Ev::UploadProgress { id, done, total } => {
                if let Some(u) = self.uploads.iter_mut().find(|u| u.id == id) {
                    (u.done, u.total) = (done, total);
                }
            }
            Ev::UploadDone(id) => self.uploads.retain(|u| u.id != id),
            Ev::InlineResults { peer, bot, query, query_id, results } => {
                if self.selected == Some(peer)
                    && let Some(i) = &mut self.inline
                    && i.bot == bot
                    && i.query == query
                {
                    i.results = results;
                    i.query_id = query_id;
                    i.results_for = Some((bot, query));
                }
            }
            _ => {}
        }
    }
}
