//! Replying with a quote, as in Telegram Desktop: a click on the reply bar opens the message,
//! the selected part becomes the quote shown above the reply.

use super::*;
use crate::backend::Quote;

#[derive(Default)]
pub(super) struct Quoting {
    /// The message whose text is open for picking a quote.
    picking: Option<(PeerId, i32)>,
    /// The chosen quote and the message it belongs to; used only while replying to that message.
    chosen: Option<(PeerId, i32, Quote)>,
}

impl Quoting {
    /// The quote for a reply to `id` in `peer`, if one was picked for it.
    pub(super) fn get(&self, peer: PeerId, id: i32) -> Option<&Quote> {
        self.chosen.as_ref().filter(|(p, i, _)| *p == peer && *i == id).map(|(_, _, q)| q)
    }

    pub(super) fn clear(&mut self) {
        *self = Self::default();
    }
}

/// The selected part of `text` (char indices), trimmed, with its offset in UTF-16 units as the API counts it.
pub(super) fn quote_of(text: &str, from: usize, to: usize) -> Option<Quote> {
    let (from, to) = (from.min(to), from.max(to));
    let start = text.char_indices().nth(from).map_or(text.len(), |(b, _)| b);
    let end = text.char_indices().nth(to).map_or(text.len(), |(b, _)| b);
    let part = &text[start..end];
    let lead = part.len() - part.trim_start().len();
    let part = part.trim();
    if part.is_empty() {
        return None;
    }
    let offset = text[..start + lead].encode_utf16().count() as i32;
    Some(Quote { text: part.to_owned(), offset })
}

impl App {
    /// Opens the replied message for picking a quote (not in protected chats: their text can't be copied).
    pub(super) fn open_quote(&mut self, peer: PeerId, id: i32) {
        if self.find(peer, id).is_some_and(|m| !m.text.trim().is_empty()) && !self.protected(peer, &[id]) {
            self.quoting.picking = Some((peer, id));
        }
    }

    /// Text of the reply bar: the quote when there is one.
    pub(super) fn reply_quote(&self, peer: PeerId, id: i32) -> Option<String> {
        self.quoting.get(peer, id).map(|q| q.text.replace('\n', " "))
    }

    pub(super) fn quote_view(&mut self, ui: &mut Ui, peer: PeerId, rect: Rect) {
        let Some((qpeer, id)) = self.quoting.picking else { return };
        let text = self.find(peer, id).map(|m| m.text.clone());
        let esc = ui.input_mut(|i| i.consume_key(egui::Modifiers::NONE, Key::Escape));
        let (Some(text), true, false) = (text, qpeer == peer, esc) else {
            self.quoting.picking = None;
            return;
        };
        let w = rect.width().min(440.0);
        // As tall as the text needs, up to the usual box height; longer texts scroll.
        let text_h = ui.painter().layout(text.clone(), FontId::proportional(glass::msg_font()), ink(), w - 36.0).size().y;
        let panel = Rect::from_center_size(rect.center(), vec2(w, (text_h + 148.0).clamp(200.0, (rect.height() - 100.0).min(420.0))));
        glass::glass(ui.painter(), panel, Glass::panel(24.0).with_shadow());
        ui.interact(panel, ui.scope_id().with("quote-bg"), Sense::click());
        let p = ui.painter().clone();
        p.text(pos2(panel.left() + 18.0, panel.top() + 26.0), Align2::LEFT_CENTER, l!("Цитата", "Quote"), FontId::proportional(16.0), ink());
        let close = Rect::from_center_size(pos2(panel.right() - 26.0, panel.top() + 26.0), vec2(32.0, 32.0));
        if glass::icon_button(ui, close, "quote-close", Icon::Close, false, true).clicked() {
            self.quoting.picking = None;
            return;
        }
        let inner = panel.shrink2(vec2(18.0, 0.0));
        p.text(pos2(inner.left(), panel.top() + 56.0), Align2::LEFT_CENTER, l!("Виділіть частину повідомлення для цитати", "Select a part of the message to quote"), FontId::proportional(12.5), ink_dim());
        let body = Rect::from_min_max(pos2(inner.left(), panel.top() + 72.0), pos2(inner.right(), panel.bottom() - 60.0));
        let mut child = ui.new_child(UiBuilder::new().max_rect(body));
        let mut range = None;
        egui::ScrollArea::vertical().id_salt("quote-text").auto_shrink(false).show(&mut child, |ui| {
            // A read-only field: the text can be selected but not changed.
            let out = egui::TextEdit::multiline(&mut text.as_str())
                .id_salt(("quote-pick", peer, id))
                .font(FontId::proportional(glass::msg_font()))
                .text_color(ink())
                .frame(egui::Frame::NONE)
                .desired_width(ui.available_width())
                .show(ui);
            range = out.cursor_range.filter(|r| !r.is_empty()).map(|r| (r.primary.index.0, r.secondary.index.0));
        });
        let quote = range.and_then(|(a, b)| quote_of(&text, a, b));
        let b = Rect::from_min_max(pos2(inner.left(), panel.bottom() - 52.0), pos2(inner.right(), panel.bottom() - 14.0));
        if glass::button(ui, b, "quote-ok", l!("Цитувати виділене", "Quote selected"), true, quote.is_some()).clicked()
            && let Some(q) = quote
        {
            self.quoting = Quoting { picking: None, chosen: Some((peer, id, q)) };
        }
    }
}

#[cfg(test)]
mod tests {
    use super::quote_of;

    #[test]
    fn quote_offsets_in_utf16() {
        // "👋" is two UTF-16 units, "і" one.
        let q = quote_of("👋 Привіт, світ", 10, 15).unwrap();
        assert_eq!((q.text.as_str(), q.offset), ("світ", 11));
        // Reversed selection and spaces around it.
        let q = quote_of("a  bc  d", 6, 1).unwrap();
        assert_eq!((q.text.as_str(), q.offset), ("bc", 3));
        assert!(quote_of("a   b", 1, 4).is_none());
    }
}
