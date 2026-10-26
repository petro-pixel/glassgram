//! "Similar channels" tab in a channel's info column, as in Telegram Desktop.

use super::*;

impl App {
    /// Channel rows; returns the one clicked. `total` above the list count is what Premium shows.
    pub(super) fn similar_list(&self, ui: &mut Ui, chats: &[ChatInfo], total: i32) -> Option<ChatInfo> {
        let mut clicked = None;
        for c in chats {
            let w = ui.available_width();
            let (rect, resp) = ui.allocate_exact_size(vec2(w, 52.0), Sense::click());
            let hover = ui.ctx().animate_bool(resp.id, resp.hovered());
            let p = ui.painter();
            if hover > 0.0 {
                p.rect_filled(rect, 16.0, glass::veil((hover * 14.0) as u8));
            }
            glass::avatar(p, pos2(rect.left() + 28.0, rect.center().y), 19.0, &c.title, c.id.bot_api_dialog_id_unchecked(), kind_icon(c.kind), self.textures.get(&ImgKey::Avatar(c.id)));
            let x = rect.left() + 56.0;
            p.galley(pos2(x, rect.top() + 8.0), elided(ui, &c.title, 14.0, ink(), rect.right() - x - 10.0), ink());
            p.galley(pos2(x, rect.top() + 29.0), elided(ui, &c.last_text, 12.5, ink_dim(), rect.right() - x - 10.0), ink_dim());
            if resp.hovered() {
                ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
            }
            if resp.clicked() {
                clicked = Some(c.clone());
            }
        }
        let more = total - chats.len() as i32;
        if more > 0 {
            ui.add_space(6.0);
            let text = lf!("Ще {more} схожих каналів доступні з Telegram Premium", "{more} more similar channels are available with Telegram Premium");
            ui.add(egui::Label::new(egui::RichText::new(text).color(ink_dim()).size(12.5)).wrap());
        }
        clicked
    }
}
