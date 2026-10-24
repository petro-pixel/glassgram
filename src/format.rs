//! Text formatting: Telegram entities ↔ our own spans, Telegram Desktop style markup
//! (`**bold**`, `__italic__`, `~~strike~~`, `||spoiler||`, `` `code` ``, ```` ```block``` ````,
//! `[label](url)`) and building a `LayoutJob` for egui.

use eframe::egui::{Color32, FontFamily, FontId, Stroke, TextFormat, text::LayoutJob};
use grammers_client::tl;

#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum Style {
    Bold,
    Italic,
    Underline,
    Strike,
    Code,
    Pre,
    Spoiler,
    Quote,
    /// Links, mentions, hashtags, etc.: everything highlighted with the accent.
    Link,
}

/// A formatting span in byte offsets of the `Msg::text` string.
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct Span {
    pub start: usize,
    pub end: usize,
    pub style: Style,
    /// Where a click leads: a URL, `#hashtag` or `@mention`.
    pub target: Option<String>,
}

/// Font family names registered in `install_fonts`.
pub const BOLD: &str = "bold";
pub const ITALIC: &str = "italic";
pub const BOLD_ITALIC: &str = "bold-italic";

/// Converts Telegram UTF-16 offsets to byte offsets.
fn utf16_to_byte(text: &str, units: usize) -> usize {
    let mut n = 0;
    for (i, c) in text.char_indices() {
        if n >= units {
            return i;
        }
        n += c.len_utf16();
    }
    text.len()
}

fn utf16_len(text: &str) -> i32 {
    text.encode_utf16().count() as i32
}

/// Message entities → spans; `shift` is how many bytes were added before the text (media label).
pub fn from_entities(text: &str, entities: &[tl::enums::MessageEntity], shift: usize) -> Vec<Span> {
    use tl::enums::MessageEntity as E;
    let body = &text[shift.min(text.len())..];
    entities
        .iter()
        .filter_map(|e| {
            let (offset, length, style, url) = match e {
                E::Bold(x) => (x.offset, x.length, Style::Bold, None),
                E::Italic(x) => (x.offset, x.length, Style::Italic, None),
                E::Underline(x) => (x.offset, x.length, Style::Underline, None),
                E::Strike(x) => (x.offset, x.length, Style::Strike, None),
                E::Code(x) => (x.offset, x.length, Style::Code, None),
                E::Pre(x) => (x.offset, x.length, Style::Pre, None),
                E::Spoiler(x) => (x.offset, x.length, Style::Spoiler, None),
                E::Blockquote(x) => (x.offset, x.length, Style::Quote, None),
                E::TextUrl(x) => (x.offset, x.length, Style::Link, Some(x.url.clone())),
                E::Url(x) => (x.offset, x.length, Style::Link, Some(String::new())),
                E::Email(x) => (x.offset, x.length, Style::Link, Some("mailto:".into())),
                E::Mention(x) => (x.offset, x.length, Style::Link, Some(String::new())),
                E::MentionName(x) => (x.offset, x.length, Style::Link, None),
                E::Hashtag(x) => (x.offset, x.length, Style::Link, Some(String::new())),
                E::Cashtag(x) => (x.offset, x.length, Style::Link, Some(String::new())),
                E::BotCommand(x) => (x.offset, x.length, Style::Link, Some(String::new())),
                E::Phone(x) => (x.offset, x.length, Style::Link, None),
                _ => return None,
            };
            let start = utf16_to_byte(body, offset.max(0) as usize);
            let end = utf16_to_byte(body, (offset + length).max(0) as usize);
            if start >= end {
                return None;
            }
            // An empty target means "the entity text itself" (URL, @mention, #hashtag).
            let target = url.map(|u| match u.as_str() {
                "" => body[start..end].to_owned(),
                "mailto:" => format!("mailto:{}", &body[start..end]),
                _ => u,
            });
            Some(Span { start: start + shift, end: end + shift, style, target })
        })
        .collect()
}

/// Markup markers: (opener, closer, style). Order matters: ``` before `.
const MARKERS: [(&str, Style); 6] = [
    ("```", Style::Pre),
    ("`", Style::Code),
    ("**", Style::Bold),
    ("__", Style::Italic),
    ("~~", Style::Strike),
    ("||", Style::Spoiler),
];

/// Parses markup from the input field. Returns plain text and entities for Telegram.
pub fn parse(input: &str) -> (String, Vec<tl::enums::MessageEntity>) {
    let mut out = String::new();
    let mut ents = Vec::new();
    parse_into(input, &mut out, &mut ents);
    (out, ents)
}

fn entity(style: Style, offset: i32, length: i32, url: Option<String>) -> tl::enums::MessageEntity {
    use tl::types as t;
    match (style, url) {
        (Style::Link, Some(url)) => t::MessageEntityTextUrl { offset, length, url }.into(),
        (Style::Bold, _) => t::MessageEntityBold { offset, length }.into(),
        (Style::Italic, _) => t::MessageEntityItalic { offset, length }.into(),
        (Style::Underline, _) => t::MessageEntityUnderline { offset, length }.into(),
        (Style::Strike, _) => t::MessageEntityStrike { offset, length }.into(),
        (Style::Code, _) => t::MessageEntityCode { offset, length }.into(),
        (Style::Pre, _) => t::MessageEntityPre { offset, length, language: String::new() }.into(),
        (Style::Spoiler, _) => t::MessageEntitySpoiler { offset, length }.into(),
        (Style::Quote, _) => t::MessageEntityBlockquote { collapsed: false, offset, length }.into(),
        (Style::Link, None) => t::MessageEntityUnknown { offset, length }.into(),
    }
}

fn parse_into(input: &str, out: &mut String, ents: &mut Vec<tl::enums::MessageEntity>) {
    let mut rest = input;
    'outer: while !rest.is_empty() {
        // [label](url)
        // The label runs to the first `]`, and `(` must follow right after it: otherwise «[тег] … [x](url)»
        // would swallow all the text between the brackets.
        if let Some(after) = rest.strip_prefix('[')
            && let Some(close) = after.find(']')
            && after[close + 1..].starts_with('(')
            && let Some(end) = after[close + 2..].find(')')
        {
            let (label, url) = (&after[..close], &after[close + 2..close + 2 + end]);
            if !label.is_empty() && !url.is_empty() && !url.contains(char::is_whitespace) {
                let offset = utf16_len(out);
                parse_into(label, out, ents);
                ents.push(entity(Style::Link, offset, utf16_len(out) - offset, Some(url.to_owned())));
                rest = &after[close + 3 + end..];
                continue;
            }
        }
        for (marker, style) in MARKERS {
            let Some(after) = rest.strip_prefix(marker) else { continue };
            let Some(close) = after.find(marker).filter(|&c| c > 0) else { continue };
            let inner = &after[..close];
            // Code and code blocks are not parsed further, the rest recursively.
            let code = matches!(style, Style::Code | Style::Pre);
            if inner.trim().is_empty() {
                continue;
            }
            let inner = if style == Style::Pre { inner.strip_prefix('\n').unwrap_or(inner) } else { inner };
            let inner = if style == Style::Pre { inner.strip_suffix('\n').unwrap_or(inner) } else { inner };
            let offset = utf16_len(out);
            if code {
                out.push_str(inner);
            } else {
                parse_into(inner, out, ents);
            }
            ents.push(entity(style, offset, utf16_len(out) - offset, None));
            rest = &after[close + marker.len()..];
            continue 'outer;
        }
        let c = rest.chars().next().unwrap();
        out.push(c);
        rest = &rest[c.len_utf8()..];
    }
}

/// Reverse conversion for editing: text + spans → markup.
pub fn to_markup(text: &str, spans: &[Span]) -> String {
    let marker = |s: &Span| match s.style {
        Style::Bold => Some(("**", "**".to_owned())),
        Style::Italic => Some(("__", "__".to_owned())),
        Style::Strike => Some(("~~", "~~".to_owned())),
        Style::Spoiler => Some(("||", "||".to_owned())),
        Style::Code => Some(("`", "`".to_owned())),
        Style::Pre => Some(("```\n", "\n```".to_owned())),
        Style::Link => match &s.target {
            Some(url) if url != &text[s.start..s.end] && !url.starts_with('#') && !url.starts_with('@') => {
                Some(("[", format!("]({url})")))
            }
            _ => None,
        },
        Style::Underline | Style::Quote => None,
    };
    let mut inserts: Vec<(usize, bool, String)> = Vec::new();
    for s in spans {
        if let Some((open, close)) = marker(s) {
            inserts.push((s.start, true, open.to_owned()));
            inserts.push((s.end, false, close));
        }
    }
    // Closers come before openers at the same position.
    inserts.sort_by_key(|(pos, open, _)| (*pos, *open));
    let mut out = String::with_capacity(text.len() + inserts.len() * 2);
    let mut last = 0;
    for (pos, _, m) in inserts {
        out.push_str(&text[last..pos]);
        out.push_str(&m);
        last = pos;
    }
    out.push_str(&text[last..]);
    out
}

/// Builds a `LayoutJob` taking spans into account. `revealed`: whether spoilers are shown.
pub fn layout(text: &str, spans: &[Span], size: f32, color: Color32, link: Color32, wrap: f32, revealed: bool) -> LayoutJob {
    let mut job = LayoutJob::default();
    job.wrap.max_width = wrap;
    let mut cuts: Vec<usize> = vec![0, text.len()];
    for s in spans {
        cuts.push(s.start.min(text.len()));
        cuts.push(s.end.min(text.len()));
    }
    cuts.sort_unstable();
    cuts.dedup();
    for w in cuts.windows(2) {
        let (a, b) = (w[0], w[1]);
        if a >= b || !text.is_char_boundary(a) || !text.is_char_boundary(b) {
            continue;
        }
        let has = |st: Style| spans.iter().any(|s| s.style == st && s.start <= a && s.end >= b);
        let mono = has(Style::Code) || has(Style::Pre);
        let italic = has(Style::Italic) || has(Style::Quote);
        let family = match (mono, has(Style::Bold), italic) {
            (true, ..) => FontFamily::Monospace,
            (false, true, true) => FontFamily::Name(BOLD_ITALIC.into()),
            (false, true, false) => FontFamily::Name(BOLD.into()),
            (false, false, true) => FontFamily::Name(ITALIC.into()),
            (false, false, false) => FontFamily::Proportional,
        };
        let mut f = TextFormat::simple(FontId::new(if mono { size - 1.0 } else { size }, family), color);
        if has(Style::Link) {
            f.color = link;
        }
        if mono {
            let light = crate::glass::is_light();
            f.color = if light { Color32::from_rgb(178, 70, 20) } else { Color32::from_rgb(255, 196, 140) };
            f.background = Color32::from_black_alpha(if light { 16 } else { 70 });
        }
        if has(Style::Underline) {
            f.underline = Stroke::new(1.0, f.color);
        }
        if has(Style::Strike) {
            f.strikethrough = Stroke::new(1.0, f.color);
        }
        if has(Style::Spoiler) && !revealed {
            f.color = Color32::TRANSPARENT;
            f.background = crate::glass::veil(70);
            f.underline = Stroke::NONE;
            f.strikethrough = Stroke::NONE;
        }
        job.append(&text[a..b], 0.0, f);
    }
    job
}

/// The span under a byte position (for clicks on links and spoilers).
pub fn span_at(spans: &[Span], byte: usize) -> Option<&Span> {
    spans
        .iter()
        .filter(|s| s.start <= byte && byte < s.end)
        .find(|s| matches!(s.style, Style::Link | Style::Spoiler))
}

/// Byte position of a character by its index.
pub fn char_to_byte(text: &str, chars: usize) -> usize {
    text.char_indices().nth(chars).map_or(text.len(), |(i, _)| i)
}

/// Ubuntu font (Ubuntu Font Licence, `assets/fonts/UFL.txt`), embedded in the program.
/// The default egui fonts stay as fallbacks, for emoji and symbols missing from Ubuntu.
pub fn install_fonts(ctx: &eframe::egui::Context) {
    use eframe::egui::{FontData, FontDefinitions};
    use std::sync::Arc;
    const FACES: [(&str, &[u8]); 4] = [
        ("ubuntu", include_bytes!("../assets/fonts/Ubuntu-Regular.ttf")),
        (BOLD, include_bytes!("../assets/fonts/Ubuntu-Bold.ttf")),
        (ITALIC, include_bytes!("../assets/fonts/Ubuntu-Italic.ttf")),
        (BOLD_ITALIC, include_bytes!("../assets/fonts/Ubuntu-BoldItalic.ttf")),
    ];
    let mut fonts = FontDefinitions::default();
    let fallback = fonts.families.get(&FontFamily::Proportional).cloned().unwrap_or_default();
    for (name, bytes) in FACES {
        fonts.font_data.insert(name.into(), Arc::new(FontData::from_static(bytes)));
        let chain = std::iter::once(name.to_owned()).chain(fallback.iter().cloned()).collect();
        let family = if name == "ubuntu" { FontFamily::Proportional } else { FontFamily::Name(name.into()) };
        fonts.families.insert(family, chain);
    }
    ctx.set_fonts(fonts);
}

#[cfg(test)]
mod tests {
    use super::*;

    fn styles(input: &str) -> (String, Vec<(i32, i32, &'static str)>) {
        use tl::enums::MessageEntity as E;
        let (text, ents) = parse(input);
        let ents = ents
            .iter()
            .map(|e| match e {
                E::Bold(x) => (x.offset, x.length, "b"),
                E::Italic(x) => (x.offset, x.length, "i"),
                E::Strike(x) => (x.offset, x.length, "s"),
                E::Spoiler(x) => (x.offset, x.length, "sp"),
                E::Code(x) => (x.offset, x.length, "c"),
                E::Pre(x) => (x.offset, x.length, "pre"),
                E::TextUrl(x) => (x.offset, x.length, "url"),
                _ => (0, 0, "?"),
            })
            .collect();
        (text, ents)
    }

    #[test]
    fn bracketed_text_before_link_stays_text() {
        let (text, ents) = super::parse("[тег] **ж** [посилання](https://example.com)");
        assert_eq!(text, "[тег] ж посилання");
        assert_eq!(ents.len(), 2);
        assert!(matches!(&ents[1], tl::enums::MessageEntity::TextUrl(u) if u.offset == 8 && u.length == 9));
    }

    #[test]
    fn plain_text_untouched() {
        assert_eq!(styles("snake_case і 2*3*4 #тег"), ("snake_case і 2*3*4 #тег".into(), vec![]));
        assert_eq!(styles("** не жирне"), ("** не жирне".into(), vec![]));
    }

    #[test]
    fn markers() {
        assert_eq!(styles("так **жирно** і __курсив__"), ("так жирно і курсив".into(), vec![(4, 5, "b"), (12, 6, "i")]));
        assert_eq!(styles("`a**b**`"), ("a**b**".into(), vec![(0, 6, "c")]));
        assert_eq!(styles("```\nfn x()\n```"), ("fn x()".into(), vec![(0, 6, "pre")]));
        assert_eq!(styles("||таємниця|| ~~ні~~"), ("таємниця ні".into(), vec![(0, 8, "sp"), (9, 2, "s")]));
        assert_eq!(styles("**жир __курс__**"), ("жир курс".into(), vec![(4, 4, "i"), (0, 8, "b")]));
    }

    #[test]
    fn links_and_utf16() {
        assert_eq!(styles("🦀 [сайт](https://x.ua)"), ("🦀 сайт".into(), vec![(3, 4, "url")]));
        assert_eq!(styles("[не](посилання з пробілом)"), ("[не](посилання з пробілом)".into(), vec![]));
    }

    #[test]
    fn entities_roundtrip() {
        let (text, ents) = parse("🦀 **жир** [сайт](https://x.ua) `код`");
        let spans = from_entities(&text, &ents, 0);
        assert_eq!(&text[spans[0].start..spans[0].end], "жир");
        assert_eq!(&text[spans[1].start..spans[1].end], "сайт");
        assert_eq!(spans[1].target.as_deref(), Some("https://x.ua"));
        assert_eq!(to_markup(&text, &spans), "🦀 **жир** [сайт](https://x.ua) `код`");
    }

    #[test]
    fn shifted_entities() {
        let text = "📎 Файл\nпідпис";
        let shift = "📎 Файл\n".len();
        let ents = vec![tl::types::MessageEntityBold { offset: 0, length: 6 }.into()];
        let spans = from_entities(text, &ents, shift);
        assert_eq!(&text[spans[0].start..spans[0].end], "підпис");
    }
}

