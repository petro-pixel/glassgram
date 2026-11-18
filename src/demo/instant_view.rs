//! Demo Instant View: a "Tech News" post links to an article whose preview has the "Instant View"
//! button; the article shows every kind of block the reader draws.

use eframe::egui::{Color32, ColorImage};
use grammers_client::session::types::PeerId;

use crate::app::App;
use crate::backend::{CardKind, Ev, ImgKey, IvArticle, IvBlock, IvPage, IvText, Msg, WebPageInfo};
use crate::format::{self, Span, Style};

/// The post with the article link.
const POST: i32 = 42;
const URL: &str = "https://telegra.ph/Glass-interfaces-11-15";
/// Picture ids inside the article.
const COVER: i64 = 9_001;
const VIDEO: i64 = 9_002;
const RELATED: i64 = 9_003;

pub(super) fn post(base: Msg) -> Msg {
    let text = format!("How glass interfaces came back, and why they are easier to read this time 👇\n{URL}");
    let spans = format::from_entities(&text, &[], 0);
    let page = WebPageInfo {
        id: 42,
        url: URL.into(),
        site: "Telegraph".into(),
        title: "Glass interfaces, ten years later".into(),
        description: "Blur, refraction and contrast: what changed since the first translucent desktops.".into(),
        photo: true,
        iv: true,
        card: CardKind::Link,
    };
    Msg { id: POST, text, spans, views: Some(5_100), webpage: Some(page), ..base }
}

/// The link card's picture in the bubble.
pub(super) fn images() -> Vec<(ImgKey, ColorImage)> {
    vec![(ImgKey::Photo(PeerId::user_unchecked(4), POST), glass_picture(480, 270, 0.0))]
}

/// Text with one styled stretch: `styled` is found in `text`.
fn styled(text: &str, marks: &[(&str, Style, Option<&str>)]) -> IvText {
    let spans = marks
        .iter()
        .filter_map(|(part, style, target)| {
            let start = text.find(part)?;
            Some(Span { start, end: start + part.len(), style: *style, target: target.map(str::to_owned) })
        })
        .collect();
    IvText { text: text.into(), spans }
}

fn p(text: &str) -> IvBlock {
    IvBlock::Paragraph(IvText::plain(text))
}

fn page() -> IvPage {
    let t = IvText::plain;
    let blocks = vec![
        IvBlock::Kicker(t("DESIGN")),
        IvBlock::Title(t("Glass interfaces, ten years later")),
        IvBlock::Subtitle(t("Blur, refraction and contrast: what changed since the first translucent desktops")),
        IvBlock::AuthorDate { author: t("Glassgram team"), date: 1_763_200_000 },
        IvBlock::Media { id: COVER, size: (1280, 720), video: false, caption: t("A glass panel over a busy wallpaper: the text stays readable."), credit: t("Illustration: Glassgram"), url: None },
        IvBlock::Paragraph(styled(
            "Translucent windows first arrived on desktops almost twenty years ago and quietly went away: text on top of them was hard to read, and the effect cost too much on the hardware of the time. Today GPUs blur a frame in well under a millisecond, and the design guidelines learned where glass belongs.",
            &[("almost twenty years ago", Style::Italic, None), ("well under a millisecond", Style::Bold, None)],
        )),
        IvBlock::Header(t("Where glass goes")),
        p("Glass is a control layer floating above the content, not the content itself. The rule of thumb is simple:"),
        IvBlock::List(vec![
            ("•".into(), vec![p("headers, toolbars, the message field and menus are glass;")]),
            ("•".into(), vec![p("message text, photos and long lists stay solid;")]),
            ("•".into(), vec![p("never more than one layer of glass on glass.")]),
        ]),
        IvBlock::Blockquote {
            text: t("Text on glass must be readable on any background. If the background is busy, put a dark chip under the text."),
            caption: t("Glassgram design code"),
        },
        IvBlock::Subheader(t("How the blur is made")),
        IvBlock::Paragraph(styled(
            "The window is drawn once without the glass, blurred on the GPU in two passes, and the panels sample the blurred frame. The shader is in glass.wgsl, and the details are in the egui documentation.",
            &[("glass.wgsl", Style::Code, None), ("the egui documentation", Style::Link, Some("https://docs.rs/egui"))],
        )),
        IvBlock::Preformatted("let blurred = blur.run(frame, radius);\npanel.sample(blurred, refraction);".into()),
        IvBlock::Pullquote { text: t("The best glass is the one you don't notice until you need it."), caption: t("— a designer we asked") },
        IvBlock::Header(t("Three settings worth knowing")),
        IvBlock::List(vec![
            ("1.".into(), vec![p("Reduce transparency makes every panel opaque.")]),
            ("2.".into(), vec![p("Increase contrast adds a clear border and darker text.")]),
            ("3.".into(), vec![p("Interface scale keeps the glass sharp on any screen.")]),
        ]),
        IvBlock::Divider,
        IvBlock::Media { id: VIDEO, size: (640, 360), video: true, caption: t("Refraction at the edge of a panel, slowed down."), credit: IvText::default(), url: None },
        IvBlock::Embed { url: "https://codeberg.org/Lonami/grammers".into(), caption: t("The MTProto library under Glassgram") },
        IvBlock::Related {
            title: t("Related articles"),
            articles: vec![
                IvArticle { url: "https://telegra.ph/Liquid-glass-in-Rust-11-01".into(), title: "Liquid glass in Rust".into(), description: "A blur shader for wgpu in two hundred lines.".into(), photo: Some(RELATED) },
                IvArticle { url: "https://telegra.ph/Readable-text-on-busy-backgrounds-10-20".into(), title: "Readable text on busy backgrounds".into(), description: "Chips, shadows and contrast ratios that work.".into(), photo: None },
            ],
        },
        IvBlock::Footer(t("Made with Telegraph")),
    ];
    IvPage { url: URL.into(), blocks, views: Some(12_400), rtl: false }
}

/// Answers `Cmd::InstantView`: the demo article with its pictures, or the usual refusal.
pub(super) fn open(app: &mut App, url: String) {
    if url != URL {
        let result = Err(crate::i18n::l!("Для цієї сторінки немає миттєвого перегляду", "This page has no Instant View").into());
        return app.push_event(Ev::InstantView { url, result });
    }
    for (id, w, h, hue) in [(COVER, 640, 360, 0.0), (VIDEO, 640, 360, 2.2), (RELATED, 128, 128, 4.0)] {
        app.push_event(Ev::Image { key: ImgKey::Page(id), image: glass_picture(w, h, hue) });
    }
    app.push_event(Ev::InstantView { url, result: Ok(page()) });
}

/// Soft colour waves with a lighter panel in the middle: stands in for the article's pictures.
fn glass_picture(w: usize, h: usize, hue: f32) -> ColorImage {
    let mut img = ColorImage::filled([w, h], Color32::BLACK);
    for y in 0..h {
        for x in 0..w {
            let (fx, fy) = (x as f32 / w as f32, y as f32 / h as f32);
            let wave = |k: f32| ((fx * 5.0 + fy * 3.0 + hue + k).sin() * 0.5 + 0.5) * 255.0;
            let panel = (0.25..0.75).contains(&fx) && (0.3..0.7).contains(&fy);
            let lift = if panel { 60.0 } else { 0.0 };
            let r = (wave(0.0) * 0.6 + 40.0 + lift).min(255.0);
            let g = (wave(2.0) * 0.5 + 50.0 + lift).min(255.0);
            let b = (wave(4.0) * 0.4 + 110.0 + lift).min(255.0);
            img.pixels[y * w + x] = Color32::from_rgb(r as u8, g as u8, b as u8);
        }
    }
    img
}
