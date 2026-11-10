//! Formatting in the message field, as in Telegram Desktop: selected text gets a glass bar above it
//! (bold, italic, strikethrough, monospace, spoiler, link, clear formatting), and the same actions
//! are on Ctrl+B / Ctrl+I / Ctrl+Shift+X / Ctrl+Shift+M / Ctrl+Shift+P / Ctrl+K / Ctrl+Shift+N.
//! They write the usual markup, which the field itself shows styled (`glass::rich_area`).

use std::ops::Range;

use eframe::egui::text::{CCursor, CCursorRange};
use eframe::egui::{Modifiers, Vec2};

use super::*;

#[derive(Default)]
pub(super) struct RichText {
    /// The field the bar belongs to, for the global Ctrl+K to know when to step aside.
    edit: Option<egui::Id>,
    /// Where the bar was last drawn: the pointer over it keeps it open while the click takes the
    /// focus from the field.
    bar: Option<Rect>,
    /// The selection the bar stands over, kept for that same frame.
    sel: Option<Rect>,
}

#[derive(Clone, Copy, PartialEq, Debug)]
enum Fmt {
    Bold,
    Italic,
    Strike,
    Mono,
    Spoiler,
    Link,
    Clear,
}

const BAR: [(Fmt, Icon); 7] = [
    (Fmt::Bold, Icon::Bold),
    (Fmt::Italic, Icon::Italic),
    (Fmt::Strike, Icon::Strike),
    (Fmt::Mono, Icon::Code),
    (Fmt::Spoiler, Icon::EyeOff),
    (Fmt::Link, Icon::Link),
    (Fmt::Clear, Icon::ClearFormat),
];

const BTN: f32 = 34.0;

impl Fmt {
    fn name(self) -> &'static str {
        match self {
            Fmt::Bold => l!("Жирний (Ctrl+B)", "Bold (Ctrl+B)"),
            Fmt::Italic => l!("Курсив (Ctrl+I)", "Italic (Ctrl+I)"),
            Fmt::Strike => l!("Закреслений (Ctrl+Shift+X)", "Strikethrough (Ctrl+Shift+X)"),
            Fmt::Mono => l!("Моноширинний (Ctrl+Shift+M)", "Monospace (Ctrl+Shift+M)"),
            Fmt::Spoiler => l!("Спойлер (Ctrl+Shift+P)", "Spoiler (Ctrl+Shift+P)"),
            Fmt::Link => l!("Посилання (Ctrl+K)", "Link (Ctrl+K)"),
            Fmt::Clear => l!("Без форматування (Ctrl+Shift+N)", "Clear formatting (Ctrl+Shift+N)"),
        }
    }
}

impl App {
    /// Whether the message field has text selected, so Ctrl+K means "link" there, not "search".
    pub(super) fn composer_selection(&self, ctx: &egui::Context) -> bool {
        self.richtext.edit.is_some_and(|id| ctx.memory(|m| m.has_focus(id)) && glass::selection_rect(ctx, id).is_some())
    }

    /// The formatting shortcuts; runs before the field so the change shows this frame.
    pub(super) fn format_keys(&mut self, ui: &Ui, hint: &str) {
        let id = ui.scope_id().with(("edit", hint));
        self.richtext.edit = Some(id);
        if !ui.memory(|m| m.has_focus(id)) {
            return;
        }
        let fmt = ui.input_mut(|i| {
            let shift = Modifiers::COMMAND | Modifiers::SHIFT;
            // Shift variants first: `consume_key` ignores an extra Shift.
            [(shift, Key::X, Fmt::Strike), (shift, Key::M, Fmt::Mono), (shift, Key::P, Fmt::Spoiler), (shift, Key::N, Fmt::Clear)]
                .into_iter()
                .chain([(Modifiers::COMMAND, Key::B, Fmt::Bold), (Modifiers::COMMAND, Key::I, Fmt::Italic), (Modifiers::COMMAND, Key::K, Fmt::Link)])
                .find(|(m, k, _)| i.consume_key(*m, *k))
                .map(|(.., f)| f)
        });
        if let Some(f) = fmt {
            self.format_selection(ui.ctx(), id, f);
        }
    }

    /// The bar over the selected text; runs after the field, which records where the selection is.
    pub(super) fn format_bar(&mut self, ui: &mut Ui, area: Rect) {
        let Some(id) = self.richtext.edit else { return };
        let over_bar = self.richtext.bar.is_some_and(|r| ui.rect_contains_pointer(r.expand(4.0)));
        let sel = glass::selection_rect(ui.ctx(), id).or(self.richtext.sel.filter(|_| over_bar));
        let Some(sel) = sel.filter(|_| ui.memory(|m| m.has_focus(id)) || over_bar) else {
            self.richtext.bar = None;
            self.richtext.sel = None;
            return;
        };
        self.richtext.sel = Some(sel);
        let size = vec2(BAR.len() as f32 * BTN + 8.0, BTN + 8.0);
        let x = sel.center().x.clamp(area.left() + size.x / 2.0, (area.right() - size.x / 2.0).max(area.left() + size.x / 2.0));
        let bar = Rect::from_center_size(pos2(x, sel.top() - 10.0 - size.y / 2.0), size);
        self.richtext.bar = Some(bar);
        glass::glass(ui.painter(), bar, Glass::tinted(size.y / 2.0, glass::chrome(), 200).with_shadow().frosted(0.96));
        // Clicks on the bar must not reach the messages under it.
        ui.interact(bar, id.with("format-bar"), Sense::click());
        for (n, (fmt, icon)) in BAR.into_iter().enumerate() {
            let r = Rect::from_min_size(bar.min + vec2(4.0 + n as f32 * BTN, 4.0), Vec2::splat(BTN));
            let resp = ui.interact(r, id.with(("format", n)), Sense::click()).on_hover_text(fmt.name());
            if resp.hovered() {
                ui.painter().rect_filled(r.shrink(1.0), BTN / 2.0, glass::veil(40));
            }
            icons::draw(ui.painter(), Rect::from_center_size(r.center(), Vec2::splat(18.0)), icon, ink());
            if resp.clicked() {
                self.format_selection(ui.ctx(), id, fmt);
            }
        }
    }

    fn format_selection(&mut self, ctx: &egui::Context, id: egui::Id, fmt: Fmt) {
        let Some(mut st) = egui::TextEdit::load_state(ctx, id) else { return };
        let Some(range) = st.cursor.char_range().filter(|r| !r.is_empty()) else { return };
        let chars = range.as_sorted_char_range();
        let (text, sel) = apply(&self.draft, chars.start.0..chars.end.0, fmt);
        self.draft = text;
        st.cursor.set_char_range(Some(CCursorRange::two(CCursor::new(sel.start), CCursor::new(sel.end))));
        st.store(ctx, id);
        ctx.memory_mut(|m| m.request_focus(id));
        self.draft_edited_at = ctx.input(|i| i.time);
    }
}

/// Applies `fmt` to the characters `sel` of `text`: wraps them in the markup, or unwraps them when
/// they already have it, as the buttons in Telegram Desktop toggle. Returns the new text and what
/// stays selected (for a link: the cursor where the address goes).
fn apply(text: &str, sel: Range<usize>, fmt: Fmt) -> (String, Range<usize>) {
    let (a, b) = (format::char_to_byte(text, sel.start), format::char_to_byte(text, sel.end));
    let inner = &text[a..b];
    let count = |s: &str| s.chars().count();
    let marker = match fmt {
        Fmt::Bold => "**",
        Fmt::Italic => "__",
        Fmt::Strike => "~~",
        Fmt::Spoiler => "||",
        Fmt::Mono if inner.contains('\n') => "```",
        Fmt::Mono => "`",
        Fmt::Link => {
            let out = format!("{}[{inner}](){}", &text[..a], &text[b..]);
            let at = sel.end + 3;
            return (out, at..at);
        }
        Fmt::Clear => return clear(text, a, b),
    };
    let m = count(marker);
    if text[..a].ends_with(marker) && text[b..].starts_with(marker) {
        let out = format!("{}{inner}{}", &text[..a - marker.len()], &text[b + marker.len()..]);
        return (out, sel.start - m..sel.end - m);
    }
    if let Some(bare) = inner.strip_prefix(marker).and_then(|s| s.strip_suffix(marker)) {
        let out = format!("{}{bare}{}", &text[..a], &text[b..]);
        return (out, sel.start..sel.start + count(bare));
    }
    (format!("{}{marker}{inner}{marker}{}", &text[..a], &text[b..]), sel.start + m..sel.end + m)
}

/// Removes the markers inside the selection, and around it when it is exactly a styled stretch.
fn clear(text: &str, mut a: usize, mut b: usize) -> (String, Range<usize>) {
    let (_, marks) = format::markup_ranges(text);
    // Grow over markers that touch the selection on both sides, as many layers as there are.
    while let (Some(open), Some(close)) = (marks.iter().find(|m| m.1 == a), marks.iter().find(|m| m.0 == b)) {
        (a, b) = (open.0, close.1);
    }
    let mut out = String::with_capacity(text.len());
    let mut last = a;
    out.push_str(&text[..a]);
    for &(x, y) in marks.iter().filter(|(x, y)| *x >= a && *y <= b) {
        if x < last {
            continue;
        }
        out.push_str(&text[last..x]);
        last = y;
    }
    out.push_str(&text[last..b]);
    let start = text[..a].chars().count();
    let end = out.chars().count();
    out.push_str(&text[b..]);
    (out, start..end)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn toggles() {
        assert_eq!(apply("say hi now", 4..6, Fmt::Bold), ("say **hi** now".to_owned(), 6..8));
        // The same button again takes the markup off: selected inside it, or with it.
        assert_eq!(apply("say **hi** now", 6..8, Fmt::Bold), ("say hi now".to_owned(), 4..6));
        assert_eq!(apply("say **hi** now", 4..10, Fmt::Bold), ("say hi now".to_owned(), 4..6));
        assert_eq!(apply("a\nb", 0..3, Fmt::Mono).0, "```a\nb```");
        assert_eq!(apply("привіт світ", 7..11, Fmt::Italic).0, "привіт __світ__");
        assert_eq!(apply("see docs", 4..8, Fmt::Link), ("see [docs]()".to_owned(), 11..11));
    }

    #[test]
    fn clears() {
        assert_eq!(clear("x **__hi__** y", 6, 8), ("x hi y".to_owned(), 2..4));
        assert_eq!(clear("**a** and ~~b~~", 0, 15), ("a and b".to_owned(), 0..7));
        // Unrelated markup outside the selection stays.
        assert_eq!(clear("**a** b", 6, 7), ("**a** b".to_owned(), 6..7));
    }
}
