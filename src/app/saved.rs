//! Source tabs in Saved Messages: "All" and one capsule per chat the messages were saved from,
//! as in Telegram Desktop. Picking a source shows only its messages.

use super::*;

/// Saved Messages split by source.
#[derive(Default)]
pub(super) struct Saved {
    /// Where messages came from, newest first.
    pub(super) sources: Vec<ChatInfo>,
    /// The source picked; `None` shows all.
    pub(super) source: Option<PeerId>,
    /// Sources whose history is fully loaded.
    pub(super) exhausted: HashSet<PeerId>,
}

/// Height of the strip under the chat header.
const SAVED_BAR_H: f32 = 40.0;

impl App {
    /// Space the strip takes above the history; 0 outside Saved Messages or with one source.
    pub(super) fn saved_bar_h(&self, peer: PeerId) -> f32 {
        let saved = self.chats.iter().any(|c| c.id == peer && c.kind == ChatKind::Saved);
        if saved && self.saved.sources.len() > 1 { SAVED_BAR_H + 8.0 } else { 0.0 }
    }

    /// Called when a chat opens: Saved Messages asks for its sources and starts on "All".
    pub(super) fn saved_reset(&mut self, peer: PeerId) {
        self.saved.source = None;
        if self.chats.iter().any(|c| c.id == peer && c.kind == ChatKind::Saved) {
            self.send(Cmd::LoadSavedDialogs);
        }
    }

    pub(super) fn select_saved_source(&mut self, saved: PeerId, source: Option<PeerId>) {
        self.saved.source = source;
        if let Some(source) = source {
            self.send(Cmd::LoadSavedHistory { saved, source, before: None });
        }
        self.scroll.to_bottom = true;
    }

    /// A page of one source goes into the Saved Messages history, like topics into a forum.
    pub(super) fn on_saved_history(&mut self, source: PeerId, msgs: Vec<Msg>, older: bool) {
        if older {
            self.loading_older = false;
        }
        if msgs.len() < backend::PAGE {
            self.saved.exhausted.insert(source);
        }
        let Some(saved) = self.chats.iter().find(|c| c.kind == ChatKind::Saved).map(|c| c.id) else { return };
        let list = self.history.entry(saved).or_default();
        for m in msgs {
            match list.iter_mut().find(|x| x.id == m.id) {
                // Cached copies don't know their source.
                Some(x) => x.saved_from = m.saved_from,
                None => list.push(m),
            }
        }
        list.sort_by_key(|m| m.id);
    }

    /// Our own notes are saved "from" ourselves.
    fn source_title(&self, saved: PeerId, c: &ChatInfo) -> String {
        if c.id == saved { l!("Мої нотатки", "My Notes").to_owned() } else { c.title.clone() }
    }

    pub(super) fn saved_bar(&mut self, ui: &mut Ui, saved: PeerId, rect: Rect) {
        glass::glass(ui.painter(), rect, Glass::panel(rect.height() / 2.0));
        let mut picked = None;
        let mut child = ui.new_child(UiBuilder::new().max_rect(rect.shrink2(vec2(6.0, 4.0))));
        egui::ScrollArea::horizontal().id_salt("saved-sources").auto_shrink(false).scroll_bar_visibility(egui::scroll_area::ScrollBarVisibility::AlwaysHidden).show(&mut child, |ui| {
            ui.horizontal(|ui| {
                ui.spacing_mut().item_spacing.x = 4.0;
                if self.source_chip(ui, None, l!("Усі", "All"), self.saved.source.is_none()).clicked() {
                    picked = Some(None);
                }
                for c in &self.saved.sources {
                    let title = self.source_title(saved, c);
                    if self.source_chip(ui, Some(c), &title, self.saved.source == Some(c.id)).clicked() {
                        picked = Some(Some(c.id));
                    }
                }
            });
        });
        if let Some(source) = picked.filter(|s| *s != self.saved.source) {
            self.select_saved_source(saved, source);
        }
    }

    fn source_chip(&self, ui: &mut Ui, chat: Option<&ChatInfo>, title: &str, on: bool) -> egui::Response {
        let h = SAVED_BAR_H - 8.0;
        let text = elided(ui, title, 13.0, ink(), 140.0);
        let pad = if chat.is_some() { 4.0 + h - 4.0 + 6.0 } else { 14.0 };
        let w = pad + text.size().x + 14.0;
        let (r, resp) = ui.allocate_exact_size(vec2(w, h), Sense::click());
        let p = ui.painter();
        if on {
            glass::glass(p, r, Glass::tinted(h / 2.0, ACCENT, 70));
        } else if resp.hovered() {
            p.rect_filled(r, h / 2.0, glass::veil(14));
        }
        if let Some(c) = chat {
            let rad = (h - 8.0) / 2.0;
            glass::avatar(p, pos2(r.left() + 4.0 + rad, r.center().y), rad, &c.title, c.id.bot_api_dialog_id_unchecked(), kind_icon(c.kind), self.textures.get(&ImgKey::Avatar(c.id)));
        }
        p.galley(pos2(r.left() + pad, r.center().y - text.size().y / 2.0), text, ink());
        if resp.hovered() {
            ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
        }
        resp
    }
}
