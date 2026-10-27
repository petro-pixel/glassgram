//! Channel post comments: the panel with the discussion group's messages and its own field.

use super::*;

impl App {
    /// Comments panel: bubbles from the discussion group and a field for your own comment.
    pub(super) fn comments_view(&mut self, ui: &mut Ui, rect: Rect) {
        let Some(c) = &self.comments else { return };
        ui.interact(rect, ui.scope_id().with("comments-bg"), Sense::click());
        glass::glass(ui.painter(), rect, Glass::panel(24.0).with_shadow());
        let inner = rect.shrink(14.0);
        let title = match (c.discussion.is_some(), c.msgs.len()) {
            (false, _) => l!("Коментарі", "Comments").to_owned(),
            (true, 0) => l!("Коментарів ще немає", "No comments yet").to_owned(),
            (true, n) => comments_label(n as i32),
        };
        ui.painter().text(inner.left_top() + vec2(4.0, 4.0), Align2::LEFT_TOP, title, FontId::proportional(17.0), ink());
        let close = Rect::from_min_size(pos2(inner.right() - 32.0, inner.top() - 2.0), vec2(32.0, 32.0));
        let closed = glass::icon_button(ui, close, "comments-close", Icon::Close, false, true).clicked();
        let field = Rect::from_min_max(pos2(inner.left(), inner.bottom() - 40.0), pos2(inner.right() - 48.0, inner.bottom()));
        let send = Rect::from_min_size(pos2(inner.right() - 40.0, inner.bottom() - 40.0), vec2(40.0, 40.0));
        let list = Rect::from_min_max(pos2(inner.left() - 6.0, inner.top() + 40.0), pos2(inner.right() + 6.0, field.top() - 8.0));
        let mut child = ui.new_child(UiBuilder::new().max_rect(list));
        child.set_clip_rect(list);
        let mut action = None;
        let mut older = None;
        egui::ScrollArea::vertical().id_salt(("comments", c.peer, c.post)).auto_shrink(false).stick_to_bottom(true).show(&mut child, |ui| {
            if c.discussion.is_none() {
                ui.add_space(30.0);
                ui.vertical_centered(|ui| ui.add(egui::Spinner::new().color(ink())));
                return;
            }
            if !c.exhausted && !c.msgs.is_empty() {
                let w = ui.available_width();
                let (r, _) = ui.allocate_exact_size(vec2(w, 36.0), Sense::hover());
                let b = Rect::from_center_size(r.center(), vec2(200.0, 32.0));
                let label = if c.loading { l!("Завантаження…", "Loading…") } else { l!("Раніші коментарі", "Earlier comments") };
                if glass::button(ui, b, ("older-comments", c.post), label, false, !c.loading).clicked() {
                    older = c.msgs.first().map(|m| m.id);
                }
            }
            let root = c.discussion.map(|d| d.1);
            for m in &c.msgs {
                let reply = m.reply_to.filter(|r| Some(*r) != root).map(|id| match c.msgs.iter().find(|x| x.id == id) {
                    Some(r) => (author(r, &self.me), quote(r)),
                    None => (l!("Відповідь", "Reply").to_owned(), l!("Коментар", "Comment").to_owned()),
                });
                let photo = self.textures.get(&ImgKey::Photo(m.peer, m.id));
                let sticker = m.sticker.as_ref().and_then(|s| self.textures.get(&ImgKey::Sticker(s.0)));
                let look = Look { chosen: &[], group: true, revealed: false, picking: false, picked: false, play: PlayView::default(), ticks: None, chat: None, pending: None };
                if let Some(a) = bubble(ui, m, look, reply, photo, sticker) {
                    action = Some(a);
                }
            }
        });
        let Some(c) = self.comments.as_mut() else { return };
        if let Some(before) = older {
            c.loading = true;
            let (peer, id) = (c.peer, c.post);
            self.send(Cmd::OpenComments { peer, id, before: Some(before) });
        }
        let send_mods = self.send_mods();
        let Some(c) = self.comments.as_mut() else { return };
        let ready = c.discussion.is_some();
        let (resp, submitted) = glass::text_area(ui, field, &mut c.draft, l!("Коментар…", "Comment…"), send_mods);
        let can = ready && !c.draft.trim().is_empty();
        let clicked = glass::icon_button(ui, send, "comment-send", Icon::Send, true, can).clicked();
        if (clicked || submitted) && can
            && let Some((disc, root)) = c.discussion
        {
            let text = std::mem::take(&mut c.draft).trim().to_owned();
            self.send(Cmd::Send { peer: disc, text, reply_to: Some(ReplyTo::to(root)), opts: SendOpts::default() });
            resp.request_focus();
        }
        // Actions on a comment (copy, react, download) happen in the discussion group.
        if let Some(a) = action
            && let Some((disc, _)) = self.comments.as_ref().and_then(|c| c.discussion)
        {
            match a {
                Action::Reply(_) | Action::Edit(_) | Action::Pick(_) | Action::Forward(_) | Action::Delete(_) | Action::Pin(..) | Action::Unpin(_) | Action::ReadBy(_) | Action::Translate(_) | Action::Report(_) => {}
                a => self.apply(disc, a),
            }
        }
        if closed {
            self.comments = None;
        }
    }
}

impl App {
    /// A page of comments for the open post.
    pub(super) fn comments_event(&mut self, ev: Ev) {
        match ev {
            Ev::Comments { peer, post, discussion, root, msgs, older } => {
                if let Some(c) = self.comments.as_mut().filter(|c| c.peer == peer && c.post == post) {
                    c.discussion = Some((discussion, root));
                    c.loading = false;
                    // Incomplete page: there are no earlier comments.
                    c.exhausted = msgs.len() < backend::PAGE;
                    if older {
                        let mut merged: Vec<Msg> = msgs.into_iter().filter(|m| !c.msgs.iter().any(|x| x.id == m.id)).collect();
                        merged.append(&mut c.msgs);
                        c.msgs = merged;
                    } else {
                        c.msgs = msgs;
                    }
                }
            }
            _ => {}
        }
    }
}
