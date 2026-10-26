//! In-chat search panel: type filters, the sender filter of groups and the results.

use super::*;
use crate::backend::Mentionable;

impl App {
    /// While picking a sender, the field searches the group's members (after a pause in typing).
    pub(super) fn search_members_due(&mut self, peer: PeerId, now: f64) {
        let Some(s) = self.chat_search.as_mut().filter(|s| s.picking) else { return };
        let q = s.query.trim().to_owned();
        if s.members_asked.as_ref() != Some(&q) && now - s.edited_at > SEARCH_DELAY {
            s.members_asked = Some(q.clone());
            self.send(Cmd::LoadMentions { peer, query: q });
        }
    }

    /// In-chat search results: a glass panel under the header.
    pub(super) fn chat_search_view(&mut self, ui: &mut Ui, peer: PeerId, rect: Rect) {
        let Some(s) = &self.chat_search else { return };
        const FILTERS_H: f32 = 44.0;
        let group = self.chats.iter().any(|c| c.id == peer && c.kind == ChatKind::Group);
        let members: Vec<Mentionable> = match &self.suggest.members {
            Some((p, _, list)) if *p == peer && s.picking => list.clone(),
            _ => Vec::new(),
        };
        let rows = if s.picking {
            members.len().max(1)
        } else if !s.active() {
            0
        } else if s.ready() {
            s.hits.len().max(1)
        } else {
            1
        };
        let body_h = if rows == 0 { 0.0 } else { rows as f32 * 58.0 + 12.0 };
        let full = Rect::from_min_size(rect.min, vec2(rect.width(), (FILTERS_H + body_h + 8.0).min(rect.height())));
        glass::glass(ui.painter(), full, Glass::tinted(22.0, glass::chrome(), 150).frosted(1.0).with_shadow());
        ui.interact(full, ui.scope_id().with("chat-search-bg"), Sense::click());
        // Sender (groups), then the types as in the profile's shared media; a second click clears.
        // The server rejects a type filter together with a sender, so they replace each other.
        let strip = Rect::from_min_size(full.min + vec2(10.0, 8.0), vec2(full.width() - 20.0, 32.0));
        let mut kind = s.kind;
        let mut from_click = false;
        ui.scope_builder(UiBuilder::new().max_rect(strip), |ui| {
            ui.horizontal(|ui| {
                ui.spacing_mut().item_spacing.x = 6.0;
                if group {
                    let label = match &s.from {
                        Some((_, name)) => lf!("Від: {name} ✕", "From: {name} ✕"),
                        None => l!("Від кого…", "From…").to_owned(),
                    };
                    from_click = info::chip(ui, &label, s.from.is_some() || s.picking).clicked();
                }
                for k in SharedKind::ALL {
                    if info::chip(ui, k.title(), kind == Some(k)).clicked() {
                        kind = if kind == Some(k) { None } else { Some(k) };
                    }
                }
            });
        });
        let Some(s) = self.chat_search.as_mut() else { return };
        if s.kind != kind {
            s.kind = kind;
            if kind.is_some() {
                s.from = None;
            }
            if s.picking {
                s.picking = false;
                s.query = std::mem::take(&mut s.stash);
            }
            s.edited_at = 0.0;
        }
        if from_click {
            if s.from.take().is_some() || s.picking {
                s.picking = false;
                s.query = std::mem::take(&mut s.stash);
            } else {
                s.picking = true;
                s.stash = std::mem::take(&mut s.query);
                s.members_asked = None;
            }
            s.edited_at = 0.0;
        }
        if rows == 0 {
            return;
        }
        let rect = Rect::from_min_max(pos2(full.left(), strip.bottom() + 4.0), full.max);
        if s.picking {
            if let Some(m) = self.member_pick(ui, peer, rect, &members) {
                let s = self.chat_search.as_mut().expect("search is open");
                s.from = Some((m.id, m.name));
                s.kind = None;
                s.picking = false;
                s.query = std::mem::take(&mut s.stash);
                s.edited_at = 0.0;
            }
            return;
        }
        let s = &*s;
        if !s.ready() {
            ui.put(Rect::from_center_size(rect.center(), vec2(22.0, 22.0)), egui::Spinner::new().size(22.0).color(ink()));
            return;
        }
        if s.hits.is_empty() {
            ui.painter().text(rect.center(), Align2::CENTER_CENTER, l!("Нічого не знайдено", "Nothing found"), FontId::proportional(14.0), ink_dim());
            return;
        }
        let inner = rect.shrink2(vec2(8.0, 6.0));
        let mut chosen = None;
        let mut child = ui.new_child(UiBuilder::new().max_rect(inner));
        child.set_clip_rect(inner);
        let s = self.chat_search.as_ref().expect("search is open");
        egui::ScrollArea::vertical()
            .id_salt(("chat-search", peer))
            .auto_shrink(false)
            .show(&mut child, |ui| {
                ui.spacing_mut().item_spacing.y = 2.0;
                for hit in &s.hits {
                    let who = author(&hit.msg, &self.me);
                    let hit = SearchHit { chat: who, ..hit.clone() };
                    if hit_row(ui, &hit, &s.hits_for.0, None).clicked() {
                        chosen = Some(hit);
                    }
                }
            });
        if let Some(hit) = chosen {
            self.jump_to(&hit);
        }
    }

    /// Group members matching the field; returns the one clicked.
    fn member_pick(&self, ui: &mut Ui, peer: PeerId, rect: Rect, members: &[Mentionable]) -> Option<Mentionable> {
        if members.is_empty() {
            let asked = self.chat_search.as_ref().is_some_and(|s| s.members_asked.is_some());
            let text = if asked { l!("Нікого не знайдено", "Nobody found") } else { l!("Завантаження…", "Loading…") };
            ui.painter().text(rect.center(), Align2::CENTER_CENTER, text, FontId::proportional(14.0), ink_dim());
            return None;
        }
        let inner = rect.shrink2(vec2(8.0, 6.0));
        let mut chosen = None;
        let mut child = ui.new_child(UiBuilder::new().max_rect(inner));
        child.set_clip_rect(inner);
        egui::ScrollArea::vertical().id_salt(("search-from", peer)).auto_shrink(false).show(&mut child, |ui| {
            ui.spacing_mut().item_spacing.y = 2.0;
            for m in members {
                let w = ui.available_width();
                let (r, resp) = ui.allocate_exact_size(vec2(w, 56.0), Sense::click());
                let hover = ui.ctx().animate_bool(resp.id, resp.hovered());
                let p = ui.painter();
                if hover > 0.0 {
                    p.rect_filled(r, 16.0, glass::veil((hover * 14.0) as u8));
                }
                let kind = if m.bot { ChatKind::Bot } else { ChatKind::User };
                glass::avatar(p, pos2(r.left() + 28.0, r.center().y), 19.0, &m.name, m.id.bot_api_dialog_id_unchecked(), kind_icon(kind), self.textures.get(&ImgKey::Avatar(m.id)));
                let x = r.left() + 56.0;
                p.galley(pos2(x, r.top() + 9.0), elided(ui, &m.name, 14.0, ink(), r.right() - x - 10.0), ink());
                if !m.username.is_empty() {
                    p.galley(pos2(x, r.top() + 30.0), elided(ui, &format!("@{}", m.username), 12.5, ink_dim(), r.right() - x - 10.0), ink_dim());
                }
                if resp.hovered() {
                    ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
                }
                if resp.clicked() {
                    chosen = Some(m.clone());
                }
            }
        });
        chosen
    }
}
