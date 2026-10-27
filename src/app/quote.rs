//! Replies beyond "reply to this message", as in Telegram Desktop: a click on the reply bar opens
//! the message to quote a part of it (with its formatting) or to reply in another chat.

use super::*;
use crate::backend::Quote;
use crate::format::Span;

/// The message open for picking a quote.
struct Picking {
    /// The chat whose composer it was opened from.
    shown_in: PeerId,
    peer: PeerId,
    id: i32,
    text: String,
    spans: Vec<Span>,
}

/// A reply to a message of another chat, waiting in the composer of `to`.
struct Elsewhere {
    to: PeerId,
    from: PeerId,
    id: i32,
    /// Who wrote it and what, for the bar: the other chat's history may be gone by then.
    author: String,
    text: String,
}

#[derive(Default)]
pub(super) struct Quoting {
    picking: Option<Picking>,
    /// The chosen quote and the message it belongs to; used only while replying to that message.
    chosen: Option<(PeerId, i32, Quote)>,
    elsewhere: Option<Elsewhere>,
}

impl Quoting {
    /// The quote for a reply to `id` in `peer`, if one was picked for it.
    pub(super) fn get(&self, peer: PeerId, id: i32) -> Option<&Quote> {
        self.chosen.as_ref().filter(|(p, i, _)| *p == peer && *i == id).map(|(_, _, q)| q)
    }

    /// The other chat's message the composer of `peer` replies to.
    pub(super) fn elsewhere(&self, peer: PeerId) -> Option<(PeerId, i32)> {
        self.elsewhere.as_ref().filter(|e| e.to == peer).map(|e| (e.from, e.id))
    }

    pub(super) fn clear(&mut self) {
        *self = Self::default();
    }
}

/// The selected part of `text` (char indices), trimmed, with its offset in UTF-16 units as the API
/// counts it and the formatting spans cut to it.
pub(super) fn quote_of(text: &str, spans: &[Span], from: usize, to: usize) -> Option<Quote> {
    let (from, to) = (from.min(to), from.max(to));
    let start = text.char_indices().nth(from).map_or(text.len(), |(b, _)| b);
    let end = text.char_indices().nth(to).map_or(text.len(), |(b, _)| b);
    let part = &text[start..end];
    let start = start + (part.len() - part.trim_start().len());
    let part = part.trim();
    if part.is_empty() {
        return None;
    }
    let end = start + part.len();
    let spans = spans
        .iter()
        .filter(|s| s.start < end && s.end > start)
        .map(|s| Span { start: s.start.max(start) - start, end: s.end.min(end) - start, ..s.clone() })
        .collect();
    let offset = text[..start].encode_utf16().count() as i32;
    Some(Quote { text: part.to_owned(), offset, spans })
}

impl App {
    /// The reply bar is shown: a reply, an edit, or a reply to another chat waiting here.
    pub(super) fn compose_bar_shown(&self, peer: PeerId) -> bool {
        self.compose != Compose::New || self.quoting.elsewhere(peer).is_some()
    }

    /// Opens the replied message for picking a quote or another chat.
    pub(super) fn open_quote(&mut self, shown_in: PeerId, peer: PeerId, id: i32) {
        let text = match (self.find(peer, id), &self.quoting.elsewhere) {
            (Some(m), _) => Some((m.text.clone(), m.spans.clone())),
            (None, Some(e)) if e.from == peer && e.id == id => Some((e.text.clone(), Vec::new())),
            _ => None,
        };
        let Some((text, spans)) = text else { return };
        self.quoting.picking = Some(Picking { shown_in, peer, id, text, spans });
    }

    /// Text of the reply bar: the quote when there is one.
    pub(super) fn reply_quote(&self, peer: PeerId, id: i32) -> Option<String> {
        self.quoting.get(peer, id).map(|q| q.text.replace('\n', " "))
    }

    /// "Reply in another chat" chose `to`: the reply moves into that chat's composer.
    pub(super) fn reply_elsewhere(&mut self, from: PeerId, id: i32, to: PeerId) {
        let chosen = self.quoting.chosen.take().filter(|(p, i, _)| *p == from && *i == id);
        let (author, text) = self.find(from, id).map(|m| (author(m, &self.me), quote(m))).unwrap_or_default();
        self.open_chat(to);
        self.quoting = Quoting { picking: None, chosen, elsewhere: None };
        if to == from {
            self.compose = Compose::Reply(id);
        } else {
            self.quoting.elsewhere = Some(Elsewhere { to, from, id, author, text });
        }
    }

    /// The bar above the field for a reply to another chat's message.
    pub(super) fn elsewhere_bar(&mut self, ui: &mut Ui, peer: PeerId, rect: Rect) {
        let Some(e) = self.quoting.elsewhere.as_ref().filter(|e| e.to == peer) else { return };
        let (from, id) = (e.from, e.id);
        let chat = self.chats.iter().find(|c| c.id == from).map(|c| c.title.clone()).unwrap_or_default();
        let who = lf!("Відповідь у «{chat}»: {}", "Reply in «{chat}»: {}", e.author);
        let what = self.reply_quote(from, id).unwrap_or_else(|| e.text.clone());
        glass::glass(ui.painter(), rect, Glass::tinted(rect.height() / 2.0, glass::chrome(), 130).with_shadow().frosted(0.96));
        let p = ui.painter().clone();
        icons::draw(&p, Rect::from_center_size(pos2(rect.left() + 20.0, rect.center().y), vec2(18.0, 18.0)), Icon::Reply, ACCENT);
        p.rect_filled(Rect::from_min_size(pos2(rect.left() + 36.0, rect.top() + 8.0), vec2(3.0, rect.height() - 16.0)), 2.0, ACCENT);
        let w = rect.width() - 100.0;
        p.galley(pos2(rect.left() + 46.0, rect.top() + 5.0), elided(ui, &who, 12.5, ACCENT, w), ACCENT);
        p.galley(pos2(rect.left() + 46.0, rect.top() + 21.0), elided(ui, &what, 13.0, ink_dim(), w), ink_dim());
        let close = Rect::from_center_size(pos2(rect.right() - 22.0, rect.center().y), vec2(28.0, 28.0));
        let body = Rect::from_min_max(rect.min, pos2(close.left() - 4.0, rect.bottom()));
        let resp = ui.interact(body, ui.scope_id().with("elsewhere-quote"), Sense::click());
        if resp.on_hover_text(l!("Натисніть, щоб процитувати частину", "Click to quote a part")).on_hover_cursor(egui::CursorIcon::PointingHand).clicked() {
            self.open_quote(peer, from, id);
        }
        if glass::icon_button(ui, close, "elsewhere-close", Icon::Close, false, true).clicked() {
            self.quoting.clear();
        }
    }

    pub(super) fn quote_view(&mut self, ui: &mut Ui, peer: PeerId, rect: Rect) {
        let Some(pick) = &self.quoting.picking else { return };
        if pick.shown_in != peer || ui.input_mut(|i| i.consume_key(egui::Modifiers::NONE, Key::Escape)) {
            self.quoting.picking = None;
            return;
        }
        let (from, id, text, spans) = (pick.peer, pick.id, pick.text.clone(), pick.spans.clone());
        // Protected text can't be copied, so it can't be quoted either.
        let protected = self.protected(from, &[id]);
        let quotable = !text.trim().is_empty() && !protected;
        let w = rect.width().min(440.0);
        // As tall as the text needs, up to the usual box height; longer texts scroll.
        let text_h = if quotable { ui.painter().layout(text.clone(), FontId::proportional(glass::msg_font()), ink(), w - 36.0).size().y + 96.0 } else { 56.0 };
        let buttons = if quotable { 92.0 } else { 52.0 };
        let panel = Rect::from_center_size(rect.center(), vec2(w, (text_h + buttons).clamp(110.0, (rect.height() - 100.0).min(460.0))));
        glass::glass(ui.painter(), panel, Glass::panel(24.0).with_shadow());
        ui.interact(panel, ui.scope_id().with("quote-bg"), Sense::click());
        let p = ui.painter().clone();
        p.text(pos2(panel.left() + 18.0, panel.top() + 26.0), Align2::LEFT_CENTER, l!("Відповідь", "Reply"), FontId::proportional(16.0), ink());
        let close = Rect::from_center_size(pos2(panel.right() - 26.0, panel.top() + 26.0), vec2(32.0, 32.0));
        if glass::icon_button(ui, close, "quote-close", Icon::Close, false, true).clicked() {
            self.quoting.picking = None;
            return;
        }
        let inner = panel.shrink2(vec2(18.0, 0.0));
        let mut quote = None;
        if quotable {
            p.text(pos2(inner.left(), panel.top() + 56.0), Align2::LEFT_CENTER, l!("Виділіть частину повідомлення для цитати", "Select a part of the message to quote"), FontId::proportional(12.5), ink_dim());
            let body = Rect::from_min_max(pos2(inner.left(), panel.top() + 72.0), pos2(inner.right(), panel.bottom() - buttons - 8.0));
            let mut child = ui.new_child(UiBuilder::new().max_rect(body));
            let mut range = None;
            egui::ScrollArea::vertical().id_salt("quote-text").auto_shrink(false).show(&mut child, |ui| {
                // A read-only field: the text can be selected but not changed.
                let out = egui::TextEdit::multiline(&mut text.as_str())
                    .id_salt(("quote-pick", from, id))
                    .font(FontId::proportional(glass::msg_font()))
                    .text_color(ink())
                    .frame(egui::Frame::NONE)
                    .desired_width(ui.available_width())
                    .show(ui);
                range = out.cursor_range.filter(|r| !r.is_empty()).map(|r| (r.primary.index.0, r.secondary.index.0));
            });
            quote = range.and_then(|(a, b)| quote_of(&text, &spans, a, b));
        }
        let row = |k: f32| Rect::from_min_max(pos2(inner.left(), panel.bottom() - 52.0 - k * 40.0), pos2(inner.right(), panel.bottom() - 14.0 - k * 40.0));
        if quotable
            && glass::button(ui, row(1.0), "quote-ok", l!("Цитувати виділене", "Quote selected"), true, quote.is_some()).clicked()
            && let Some(q) = quote
        {
            self.quoting.picking = None;
            self.quoting.chosen = Some((from, id, q));
        }
        // Like forwarding, a reply elsewhere would carry the message out of a protected chat.
        if glass::button(ui, row(0.0), "quote-elsewhere", l!("Відповісти в іншому чаті", "Reply in another chat"), false, !protected).clicked() {
            self.quoting.picking = None;
            self.forward = Some(ForwardPicker { from, ids: vec![id], query: String::new(), reply: true });
        }
    }
}

#[cfg(test)]
mod tests {
    use super::quote_of;
    use crate::format::{Span, Style};

    #[test]
    fn quote_offsets_in_utf16() {
        // "👋" is two UTF-16 units, "і" one.
        let q = quote_of("👋 Привіт, світ", &[], 10, 15).unwrap();
        assert_eq!((q.text.as_str(), q.offset), ("світ", 11));
        // Reversed selection and spaces around it.
        let q = quote_of("a  bc  d", &[], 6, 1).unwrap();
        assert_eq!((q.text.as_str(), q.offset), ("bc", 3));
        assert!(quote_of("a   b", &[], 1, 4).is_none());
    }

    #[test]
    fn quote_keeps_its_part_of_the_formatting() {
        let bold = |start, end| Span { start, end, style: Style::Bold, target: None };
        // "one two three": bold over "two thr", the quote is "two three" (bytes 4..13).
        let q = quote_of("one two three", &[bold(4, 11), bold(0, 3)], 4, 13).unwrap();
        assert_eq!(q.spans, vec![bold(0, 7)]);
        // The backend sends it as entities of the quote text (offsets in UTF-16 from the quote start).
        let (plain, ents) = crate::format::parse(&crate::format::to_markup(&q.text, &q.spans));
        assert_eq!(plain, q.text);
        assert!(matches!(ents.as_slice(), [grammers_client::tl::enums::MessageEntity::Bold(b)] if b.offset == 0 && b.length == 7));
    }
}
