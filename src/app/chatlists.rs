//! Shared folders by link, as in Telegram Desktop: the box that adds a folder from `t.me/addlist/…`,
//! adds chats that appeared in it since, or leaves it together with the ticked chats.

use super::*;
use crate::backend::{Chatlist, ChatlistAction};

/// The open box: what the server offered and which chats are ticked.
pub(super) struct ChatlistBox {
    list: Chatlist,
    ticked: Vec<bool>,
}

/// The slug of a `t.me/addlist/<slug>` link.
pub(super) fn tme_addlist(url: &str) -> Option<String> {
    let rest = url.split_once("://").map_or(url, |(_, r)| r);
    let rest = rest.strip_prefix("t.me/").or_else(|| rest.strip_prefix("telegram.me/"))?;
    let slug = rest.strip_prefix("addlist/")?.split(['/', '?', '#']).next()?;
    (!slug.is_empty()).then(|| slug.to_owned())
}

impl App {
    pub(super) fn on_chatlist(&mut self, list: Chatlist) {
        if list.chats.is_empty() && list.already > 0 {
            self.toast = Some((l!("Усі чати цієї папки вже у вас є", "You already have every chat of this folder").into(), 0.0, false));
            return;
        }
        // Leaving keeps the chats unless ticked, as in Telegram Desktop; adding ticks everything.
        let tick = !matches!(list.action, ChatlistAction::Leave(_));
        self.chatlist_box = Some(ChatlistBox { ticked: vec![tick; list.chats.len()], list });
    }

    /// Deleting a shared folder goes through the leave box, so its chats can be left too.
    pub(super) fn delete_folder(&mut self, id: i32) {
        if self.folders.iter().any(|f| f.id == id && f.shared) {
            self.send(Cmd::AskLeaveChatlist(id));
        } else {
            self.send(Cmd::DeleteFolder(id));
        }
    }

    pub(super) fn chatlist_view(&mut self, ui: &mut Ui) {
        let Some(b) = &mut self.chatlist_box else { return };
        let screen = ui.max_rect();
        ui.painter().rect_filled(screen, 0.0, Color32::from_black_alpha(90));
        ui.interact(screen, ui.scope_id().with("chatlist-bg"), Sense::click());
        let h = (150.0 + b.list.chats.len() as f32 * 50.0).clamp(240.0, (screen.height() - 60.0).min(600.0));
        let rect = Rect::from_center_size(screen.center(), vec2(screen.width().min(420.0) - 20.0, h));
        glass::glass(ui.painter(), rect, Glass::panel(26.0).with_shadow());
        let inner = rect.shrink(18.0);
        let name = if b.list.emoticon.is_empty() { b.list.title.clone() } else { format!("{} {}", b.list.emoticon, b.list.title) };
        let (title, about, main) = match &b.list.action {
            ChatlistAction::Join(_) if b.list.already > 0 => (
                l!("Додати чати до папки", "Add chats to folder").to_owned(),
                lf!("У папці «{name}» є нові чати.", "The folder «{name}» has new chats."),
                l!("Приєднатися", "Join chats"),
            ),
            ChatlistAction::Join(_) => (
                lf!("Додати папку «{name}»", "Add folder «{name}»"),
                l!("Папка з'явиться у вашому списку разом з вибраними чатами.", "The folder will appear in your list with the chosen chats.").to_owned(),
                l!("Додати папку", "Add folder"),
            ),
            ChatlistAction::Update(_) => (
                l!("Нові чати в папці", "New chats in folder").to_owned(),
                lf!("Власник папки «{name}» додав чати.", "The owner of «{name}» added chats."),
                l!("Приєднатися", "Join chats"),
            ),
            ChatlistAction::Leave(_) => (
                lf!("Видалити папку «{name}»", "Delete folder «{name}»"),
                l!("Позначте чати, з яких також вийти.", "Tick the chats to leave as well.").to_owned(),
                l!("Видалити", "Delete"),
            ),
        };
        let p = ui.painter();
        p.galley(inner.min, elided(ui, &title, 17.0, ink(), inner.width()), ink());
        let wrap = p.layout(about, FontId::proportional(12.5), ink_dim(), inner.width());
        let about_h = wrap.size().y;
        p.galley(inner.min + vec2(0.0, 30.0), wrap, ink_dim());

        let buttons = Rect::from_min_max(pos2(inner.left(), inner.bottom() - 36.0), inner.max);
        let body = Rect::from_min_max(pos2(inner.left() - 4.0, inner.top() + 38.0 + about_h), pos2(inner.right() + 4.0, buttons.top() - 10.0));
        let mut child = ui.new_child(UiBuilder::new().max_rect(body));
        child.set_clip_rect(body);
        egui::ScrollArea::vertical().id_salt("chatlist").auto_shrink(false).show(&mut child, |ui| {
            for (k, (id, name)) in b.list.chats.iter().enumerate() {
                let (row, resp) = ui.allocate_exact_size(vec2(ui.available_width() - 8.0, 46.0), Sense::click());
                if resp.clicked() {
                    b.ticked[k] = !b.ticked[k];
                }
                let p = ui.painter();
                if resp.hovered() {
                    p.rect_filled(row, 12.0, glass::veil(12));
                }
                glass::avatar(p, pos2(row.left() + 26.0, row.center().y), 18.0, name, id.bot_api_dialog_id_unchecked(), None, self.textures.get(&ImgKey::Avatar(*id)));
                p.galley(pos2(row.left() + 54.0, row.center().y - 9.0), elided(ui, name, 14.5, ink(), row.width() - 100.0), ink());
                let mark = Rect::from_center_size(pos2(row.right() - 20.0, row.center().y), vec2(22.0, 22.0));
                if b.ticked[k] {
                    p.circle_filled(mark.center(), 11.0, ACCENT);
                    icons::draw(p, mark.shrink(4.0), Icon::Check, glass::ON_DARK);
                } else {
                    p.circle_stroke(mark.center(), 10.0, egui::Stroke::new(1.5, ink_dim()));
                }
            }
        });

        let half = (buttons.width() - 8.0) / 2.0;
        let esc = ui.input_mut(|i| i.consume_key(egui::Modifiers::NONE, Key::Escape));
        let cancel = glass::button(ui, Rect::from_min_size(buttons.min, vec2(half, buttons.height())), "chatlist-cancel", l!("Скасувати", "Cancel"), false, true).clicked();
        // Adding needs at least one chat; leaving works with none ticked.
        let ready = matches!(b.list.action, ChatlistAction::Leave(_)) || b.ticked.iter().any(|t| *t);
        let ok = glass::button(ui, Rect::from_min_size(pos2(buttons.right() - half, buttons.top()), vec2(half, buttons.height())), "chatlist-ok", main, true, ready).clicked();
        if cancel || esc {
            self.chatlist_box = None;
        } else if ok && ready {
            let Some(b) = self.chatlist_box.take() else { return };
            let peers = b.list.chats.iter().zip(&b.ticked).filter(|(_, t)| **t).map(|(c, _)| c.0).collect();
            self.send(Cmd::ApplyChatlist { action: b.list.action, peers });
        }
    }
}

#[cfg(test)]
mod tests {
    use super::tme_addlist;

    #[test]
    fn parses_addlist_links() {
        assert_eq!(tme_addlist("https://t.me/addlist/AbC-12_x"), Some("AbC-12_x".into()));
        assert_eq!(tme_addlist("t.me/addlist/slug?x=1"), Some("slug".into()));
        assert_eq!(tme_addlist("https://t.me/addlist/"), None);
        assert_eq!(tme_addlist("https://t.me/somechannel"), None);
    }
}
