//! The message field and everything it sends: text, files, stickers, emoji, inline bots.

use super::*;

impl App {
    pub(super) fn send_draft(&mut self) {
        self.send_draft_with(SendOpts::default());
    }

    pub(super) fn send_draft_with(&mut self, opts: SendOpts) {
        let opts = SendOpts { no_webpage: opts.no_webpage || self.link_preview_off(), ..opts };
        let text = self.draft.trim().to_owned();
        let Some(peer) = self.selected else { return };
        if text.is_empty() {
            return;
        }
        self.draft.clear();
        match std::mem::replace(&mut self.compose, Compose::New) {
            Compose::Edit(id) => {
                if let Some(m) = self.history.get_mut(&peer).and_then(|l| l.iter_mut().find(|m| m.id == id)) {
                    let (plain, ents) = format::parse(&text);
                    m.spans = format::from_entities(&plain, &ents, 0);
                    m.text = plain;
                    m.edited = true;
                }
                self.draft = std::mem::take(&mut self.edit_stash);
                self.send(Cmd::Edit { peer, id, text });
            }
            // Scheduled messages go straight to the server; the rest through the send queue.
            Compose::Reply(id) if opts.schedule.is_some() => self.send(Cmd::Send { peer, text, reply_to: Some(id), opts }),
            Compose::Reply(id) => self.enqueue(peer, text, Some(id), &opts),
            Compose::New => {
                let reply_to = self.reply_target();
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

    pub(super) fn reply_target(&self) -> Option<i32> {
        match self.compose {
            Compose::Reply(id) => Some(id),
            _ => self.forum.open.filter(|t| *t != GENERAL_TOPIC),
        }
    }

    pub(super) fn send_files(&mut self, peer: PeerId, paths: Vec<PathBuf>, as_file: bool) {
        if paths.is_empty() {
            return;
        }
        let caption = if self.selected == Some(peer) { std::mem::take(&mut self.draft) } else { String::new() };
        let reply_to = self.reply_target();
        if reply_to.is_some() {
            self.compose = Compose::New;
        }
        self.next_upload += 1;
        let id = self.next_upload;
        let name = match paths.as_slice() {
            [one] => one.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default(),
            many => lf!("Файлів: {}", "Files: {}", many.len()),
        };
        let total = paths.iter().filter_map(|p| std::fs::metadata(p).ok()).map(|m| m.len()).sum();
        self.uploads.push(Upload { id, name, done: 0, total });
        let caption = caption.trim().to_owned();
        self.send(Cmd::SendFiles { id, peer, paths, caption, reply_to, opts: SendOpts::default(), as_file });
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

    pub(super) fn request_sticker_thumbs(&mut self) {
        let Some(set) = self.stickers.get(self.sticker_set) else { return };
        let ids: Vec<i64> = set.stickers.iter().map(|s| s.0).filter(|id| self.sticker_thumbs.insert(*id)).collect();
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
            let reply_to = self.reply_target();
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
                    let (r, resp) = tab(ui, self.emoji_tab);
                    icons::draw(ui.painter(), Rect::from_center_size(r.center(), vec2(22.0, 22.0)), Icon::Smile, ink());
                    emoji_clicked = resp.on_hover_text(l!("Емодзі", "Emoji")).clicked();
                    for (i, set) in self.stickers.iter().enumerate() {
                        let (r, resp) = tab(ui, !self.emoji_tab && i == self.sticker_set);
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
                    if self.stickers.is_empty() && self.stickers_requested && self.backend.is_some() {
                        ui.add(egui::Spinner::new().color(ink_dim()));
                    }
                });
            });
        if emoji_clicked {
            self.emoji_tab = true;
        }
        if let Some(i) = chosen_set {
            self.emoji_tab = false;
            self.sticker_set = i;
            self.request_sticker_thumbs();
        }
        let body = Rect::from_min_max(pos2(inner.left(), tabs.bottom() + 6.0), inner.max);
        if self.emoji_tab {
            return self.emoji_grid(ui, body);
        }

        let Some(set) = self.stickers.get(self.sticker_set) else {
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
            let reply_to = self.reply_target();
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
        let (icon, title, id) = match self.compose {
            Compose::Reply(id) => (Icon::Reply, l!("Відповідь", "Reply"), id),
            Compose::Edit(id) => (Icon::Pencil, l!("Редагування", "Editing"), id),
            Compose::New => return,
        };
        glass::glass(ui.painter(), rect, Glass::tinted(rect.height() / 2.0, glass::chrome(), 130).with_shadow().frosted(0.96));
        let p = ui.painter().clone();
        icons::draw(&p, Rect::from_center_size(pos2(rect.left() + 20.0, rect.center().y), vec2(18.0, 18.0)), icon, ACCENT);
        p.rect_filled(Rect::from_min_size(pos2(rect.left() + 36.0, rect.top() + 8.0), vec2(3.0, rect.height() - 16.0)), 2.0, ACCENT);
        let (who, what) = match self.find(peer, id) {
            Some(m) => (if matches!(self.compose, Compose::Edit(_)) { title.to_owned() } else { format!("{title}: {}", author(m, &self.me)) }, quote(m)),
            None => (title.to_owned(), String::new()),
        };
        let w = rect.width() - 100.0;
        let g1 = elided(ui, &who, 12.5, ACCENT, w);
        let g2 = elided(ui, &what, 13.0, ink_dim(), w);
        p.galley(pos2(rect.left() + 46.0, rect.top() + 5.0), g1, ACCENT);
        p.galley(pos2(rect.left() + 46.0, rect.top() + 21.0), g2, ink_dim());
        let close = Rect::from_center_size(pos2(rect.right() - 22.0, rect.center().y), vec2(28.0, 28.0));
        if glass::icon_button(ui, close, "compose-close", Icon::Close, false, true).clicked() {
            self.cancel_compose();
        }
    }

    pub(super) fn cancel_compose(&mut self) {
        if matches!(self.compose, Compose::Edit(_)) {
            self.draft = std::mem::take(&mut self.edit_stash);
        }
        self.compose = Compose::New;
    }
}

impl App {
    /// Inline bot results, sticker sets and upload progress for the field.
    pub(super) fn composer_event(&mut self, ev: Ev) {
        match ev {
            Ev::Stickers(sets) => {
                self.stickers = sets;
                self.sticker_set = 0;
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
