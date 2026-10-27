//! Source tabs in Saved Messages: "All" and one capsule per chat the messages were saved from,
//! as in Telegram Desktop. Picking a source shows only its messages. After them come the reaction
//! tags: a reaction on a saved message is its tag, and picking one shows only those messages.

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
    /// Reaction tags, most used first.
    pub(super) tags: Vec<backend::SavedTag>,
    /// The tag picked (its emoji); combines with the source.
    pub(super) tag: Option<String>,
    /// A tag being renamed: (emoji, typed name).
    renaming: Option<(String, String)>,
}

/// Longest tag name Telegram accepts.
const TAG_TITLE_MAX: usize = 12;

/// Height of the strip under the chat header.
const SAVED_BAR_H: f32 = 40.0;

impl App {
    /// Space the strip takes above the history; 0 outside Saved Messages or with one source.
    pub(super) fn saved_bar_h(&self, peer: PeerId) -> f32 {
        let saved = self.chats.iter().any(|c| c.id == peer && c.kind == ChatKind::Saved);
        if saved && (self.saved.sources.len() > 1 || !self.saved.tags.is_empty()) { SAVED_BAR_H + 8.0 } else { 0.0 }
    }

    /// Called when a chat opens: Saved Messages asks for its sources and starts on "All".
    pub(super) fn saved_reset(&mut self, peer: PeerId) {
        self.saved.source = None;
        self.saved.tag = None;
        self.saved.renaming = None;
        if self.chats.iter().any(|c| c.id == peer && c.kind == ChatKind::Saved) {
            self.send(Cmd::LoadSavedDialogs);
            self.send(Cmd::LoadSavedTags);
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

    /// The open chat's tag filter: only in Saved Messages.
    pub(super) fn saved_tag(&self, chat: &ChatInfo) -> Option<String> {
        self.saved.tag.clone().filter(|_| chat.kind == ChatKind::Saved)
    }

    pub(super) fn select_saved_tag(&mut self, saved: PeerId, tag: Option<String>) {
        if let Some(emoji) = &tag {
            self.send(Cmd::LoadSavedTagged { saved, emoji: emoji.clone() });
        }
        self.saved.tag = tag;
        self.scroll.to_bottom = true;
    }

    pub(super) fn on_saved_tags(&mut self, tags: Vec<backend::SavedTag>) {
        // A tag removed from its last message disappears, and so does its filter.
        if self.saved.tag.as_ref().is_some_and(|t| !tags.iter().any(|x| &x.emoji == t)) {
            self.saved.tag = None;
        }
        self.saved.tags = tags;
    }

    pub(super) fn on_saved_tags_changed(&mut self) {
        if self.selected.is_some_and(|p| self.chats.iter().any(|c| c.id == p && c.kind == ChatKind::Saved)) {
            self.send(Cmd::LoadSavedTags);
        }
    }

    /// Tagged messages join the history; copies already there get their reactions refreshed.
    pub(super) fn on_saved_tagged(&mut self, msgs: Vec<Msg>) {
        let Some(saved) = self.chats.iter().find(|c| c.kind == ChatKind::Saved).map(|c| c.id) else { return };
        let list = self.history.entry(saved).or_default();
        for m in msgs {
            match list.iter_mut().find(|x| x.id == m.id) {
                Some(x) => {
                    x.reactions = m.reactions;
                    x.saved_from = x.saved_from.or(m.saved_from);
                }
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
        let (mut tag_picked, mut rename) = (None, None);
        let mut child = ui.new_child(UiBuilder::new().max_rect(rect.shrink2(vec2(6.0, 4.0))));
        egui::ScrollArea::horizontal().id_salt("saved-sources").auto_shrink(false).scroll_bar_visibility(egui::scroll_area::ScrollBarVisibility::AlwaysHidden).show(&mut child, |ui| {
            ui.horizontal(|ui| {
                ui.spacing_mut().item_spacing.x = 4.0;
                if self.source_chip(ui, None, l!("Усі", "All"), self.saved.source.is_none()).clicked() {
                    picked = Some(None);
                }
                if self.saved.sources.len() > 1 {
                    for c in &self.saved.sources {
                        let title = self.source_title(saved, c);
                        if self.source_chip(ui, Some(c), &title, self.saved.source == Some(c.id)).clicked() {
                            picked = Some(Some(c.id));
                        }
                    }
                }
                if !self.saved.tags.is_empty() {
                    ui.add_space(4.0);
                    let (r, _) = ui.allocate_exact_size(vec2(1.0, SAVED_BAR_H - 16.0), Sense::hover());
                    ui.painter().rect_filled(r, 0.5, glass::veil(40));
                    ui.add_space(4.0);
                }
                for t in &self.saved.tags {
                    let on = self.saved.tag.as_ref() == Some(&t.emoji);
                    let resp = tag_chip(ui, t, on);
                    if resp.clicked() {
                        tag_picked = Some(if on { None } else { Some(t.emoji.clone()) });
                    }
                    resp.context_menu(|ui| {
                        if glass::menu_button(ui, Icon::Pencil, l!("Перейменувати тег", "Rename tag")).clicked() {
                            rename = Some((t.emoji.clone(), t.title.clone()));
                            ui.close();
                        }
                    });
                }
            });
        });
        if let Some(source) = picked.filter(|s| *s != self.saved.source) {
            self.select_saved_source(saved, source);
        }
        if let Some(tag) = tag_picked {
            self.select_saved_tag(saved, tag);
        }
        if rename.is_some() {
            self.saved.renaming = rename;
        }
    }

    /// "Rename tag" box over everything; Enter saves, Esc closes.
    pub(super) fn tag_rename_view(&mut self, ui: &mut Ui) {
        let Some((emoji, title)) = &mut self.saved.renaming else { return };
        let screen = ui.max_rect();
        ui.painter().rect_filled(screen, 0.0, Color32::from_black_alpha(90));
        ui.interact(screen, ui.scope_id().with("tag-rename-bg"), Sense::click());
        let rect = Rect::from_center_size(screen.center(), vec2(340.0, 160.0));
        glass::glass(ui.painter(), rect, Glass::panel(24.0).with_shadow());
        let inner = rect.shrink(18.0);
        ui.painter().text(inner.left_top(), Align2::LEFT_TOP, lf!("Назва тегу {emoji}", "Name of the {emoji} tag"), FontId::proportional(17.0), ink());
        let field = Rect::from_min_size(pos2(inner.left(), inner.top() + 36.0), vec2(inner.width(), 38.0));
        let resp = glass::text_field(ui, field, title, l!("Без назви", "No name"), false);
        if !ui.memory(|m| m.focused().is_some()) {
            resp.request_focus();
        }
        let enter = resp.lost_focus() && ui.input(|i| i.key_pressed(Key::Enter));
        let half = (inner.width() - 8.0) / 2.0;
        let by = inner.bottom() - 36.0;
        let esc = ui.input_mut(|i| i.consume_key(egui::Modifiers::NONE, Key::Escape));
        let cancel = glass::button(ui, Rect::from_min_size(pos2(inner.left(), by), vec2(half, 36.0)), "tag-cancel", l!("Скасувати", "Cancel"), false, true).clicked();
        let save = glass::button(ui, Rect::from_min_size(pos2(inner.left() + half + 8.0, by), vec2(half, 36.0)), "tag-save", l!("Зберегти", "Save"), true, true).clicked();
        if save || enter {
            let (emoji, title) = (emoji.clone(), title.trim().chars().take(TAG_TITLE_MAX).collect());
            self.send(Cmd::RenameSavedTag { emoji, title });
        }
        if save || enter || cancel || esc {
            self.saved.renaming = None;
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

/// A tag capsule: the emoji, its name if it has one, and how many messages carry it.
fn tag_chip(ui: &mut Ui, t: &backend::SavedTag, on: bool) -> egui::Response {
    let h = SAVED_BAR_H - 8.0;
    let label = if t.title.is_empty() { format!("{}  {}", t.emoji, t.count) } else { format!("{} {}  {}", t.emoji, t.title, t.count) };
    let text = elided(ui, &label, 13.0, ink(), 160.0);
    let (r, resp) = ui.allocate_exact_size(vec2(text.size().x + 24.0, h), Sense::click());
    let p = ui.painter();
    if on {
        glass::glass(p, r, Glass::tinted(h / 2.0, ACCENT, 70));
    } else {
        p.rect_filled(r, h / 2.0, glass::veil(if resp.hovered() { 22 } else { 10 }));
    }
    p.galley(pos2(r.left() + 12.0, r.center().y - text.size().y / 2.0), text, ink());
    if resp.hovered() {
        ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
    }
    resp
}
