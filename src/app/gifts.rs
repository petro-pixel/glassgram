//! "Gifts" tab in a profile: a grid of gift stickers with their price or collectible number;
//! the sender and their note on hover.

use super::*;
use crate::backend::Gift;

const COLS: usize = 3;

impl App {
    pub(super) fn gift_grid(&self, ui: &mut Ui, gifts: &[Gift], total: i32) {
        let w = ui.available_width();
        let side = ((w - 8.0 * (COLS as f32 - 1.0)) / COLS as f32).floor();
        for row in gifts.chunks(COLS) {
            let (r, _) = ui.allocate_exact_size(vec2(w, side + 8.0), Sense::hover());
            for (k, g) in row.iter().enumerate() {
                let cell = Rect::from_min_size(pos2(r.left() + k as f32 * (side + 8.0), r.top()), vec2(side, side));
                let resp = ui.interact(cell, ui.scope_id().with(("gift", g.date, k)), Sense::hover());
                let p = ui.painter();
                p.rect_filled(cell, 16.0, glass::veil(if resp.hovered() { 26 } else { 14 }));
                let pic = Rect::from_center_size(cell.center() - vec2(0.0, 10.0), vec2(side * 0.6, side * 0.6));
                match g.sticker.and_then(|id| self.textures.get(&ImgKey::Sticker(id))) {
                    Some(tex) => {
                        p.image(tex.id(), pic, Rect::from_min_max(pos2(0.0, 0.0), pos2(1.0, 1.0)), Color32::WHITE);
                    }
                    None => {
                        p.text(pic.center(), Align2::CENTER_CENTER, "🎁", FontId::proportional(side * 0.32), ink());
                    }
                }
                let label = if g.title.is_empty() { format!("⭐ {}", g.stars) } else { g.title.clone() };
                let gl = elided(ui, &label, 12.0, ink_dim(), side - 12.0);
                p.galley(pos2(cell.center().x - gl.size().x / 2.0, cell.bottom() - 22.0), gl, ink_dim());
                let mut tip = String::new();
                if !g.from.is_empty() {
                    tip.push_str(&lf!("Від: {}", "From: {}", g.from));
                }
                if !g.note.is_empty() {
                    tip.push_str(&format!("\n{}", g.note));
                }
                if !tip.is_empty() {
                    resp.on_hover_text(tip.trim_start());
                }
            }
        }
        let more = total - gifts.len() as i32;
        if more > 0 {
            ui.add_space(6.0);
            ui.label(egui::RichText::new(lf!("І ще {more} у Telegram", "And {more} more in Telegram")).color(ink_dim()).size(12.5));
        }
    }
}
