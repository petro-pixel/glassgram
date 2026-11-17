//! Instant View: the server's parsed copy of an article (`webPage.cached_page`), fetched with
//! `messages.getWebPage` and turned into blocks the UI draws, as in Telegram Desktop.

use std::collections::HashMap;

use grammers_client::media::Photo;
use grammers_client::tl;

use super::{Cmd, DocLoc, Ev, ImgKey, Loader, Worker, PHOTO_PT};
use crate::format::{Span, Style};
use crate::i18n::{l, lf};

/// How many pictures of one page are fetched: long photo reports would flood the loader.
const MAX_PICTURES: usize = 60;
/// Nesting limit for lists, quotes and details inside each other.
const MAX_DEPTH: u8 = 6;

/// Text with formatting, in the same byte-offset spans as message text.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct IvText {
    pub text: String,
    pub spans: Vec<Span>,
}

impl IvText {
    pub fn plain(text: &str) -> Self {
        Self { text: text.to_owned(), spans: Vec::new() }
    }

    pub fn is_empty(&self) -> bool {
        self.text.trim().is_empty()
    }
}

/// A related article at the end of the page.
#[derive(Debug, Clone, PartialEq)]
pub struct IvArticle {
    pub url: String,
    pub title: String,
    pub description: String,
    /// Picture id, arriving as `ImgKey::Page(id)`.
    pub photo: Option<i64>,
}

/// One block of the page; kinds we can't draw are dropped while converting.
#[derive(Debug, Clone, PartialEq)]
pub enum IvBlock {
    Title(IvText),
    Subtitle(IvText),
    /// The small line above the title.
    Kicker(IvText),
    AuthorDate { author: IvText, date: i64 },
    Header(IvText),
    Subheader(IvText),
    Paragraph(IvText),
    Preformatted(String),
    Footer(IvText),
    Divider,
    /// Items with their marker ("•", "1."), each a few blocks.
    List(Vec<(String, Vec<IvBlock>)>),
    Blockquote { text: IvText, caption: IvText },
    Pullquote { text: IvText, caption: IvText },
    /// A photo (or a video's cover when `video`) by id, its size and caption; `url` makes it a link.
    Media { id: i64, size: (i32, i32), video: bool, caption: IvText, credit: IvText, url: Option<String> },
    /// An embedded page or post we can't run: a link card.
    Embed { url: String, caption: IvText },
    Related { title: IvText, articles: Vec<IvArticle> },
}

/// A page ready to draw.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct IvPage {
    pub url: String,
    pub blocks: Vec<IvBlock>,
    pub views: Option<i32>,
    pub rtl: bool,
}

/// `RichText` flattened into text and spans.
pub(super) fn rich(t: &tl::enums::RichText) -> IvText {
    let mut out = IvText::default();
    push_rich(&mut out, t, 0);
    out
}

fn push_rich(out: &mut IvText, t: &tl::enums::RichText, depth: u8) {
    use tl::enums::RichText as R;
    if depth > 32 {
        return;
    }
    let start = out.text.len();
    let (inner, style, target): (Option<&tl::enums::RichText>, Option<Style>, Option<String>) = match t {
        R::TextEmpty => return,
        R::TextPlain(x) => {
            out.text.push_str(&x.text);
            return;
        }
        R::TextConcat(x) => {
            for t in &x.texts {
                push_rich(out, t, depth + 1);
            }
            return;
        }
        R::TextMath(x) => {
            out.text.push_str(&x.source);
            out.spans.push(Span { start, end: out.text.len(), style: Style::Code, target: None });
            return;
        }
        R::TextCustomEmoji(x) => {
            out.text.push_str(&x.alt);
            return;
        }
        // Inline pictures need their document; the text flows on without them.
        R::TextImage(_) => return,
        R::TextBold(x) => (Some(&x.text), Some(Style::Bold), None),
        // Marked text is highlighted in Telegram; bold is the nearest style we have.
        R::TextMarked(x) => (Some(&x.text), Some(Style::Bold), None),
        R::TextItalic(x) => (Some(&x.text), Some(Style::Italic), None),
        R::TextUnderline(x) => (Some(&x.text), Some(Style::Underline), None),
        R::TextStrike(x) => (Some(&x.text), Some(Style::Strike), None),
        R::TextFixed(x) => (Some(&x.text), Some(Style::Code), None),
        R::TextSpoiler(x) => (Some(&x.text), Some(Style::Spoiler), None),
        R::TextUrl(x) => (Some(&x.text), Some(Style::Link), Some(x.url.clone())),
        R::TextEmail(x) => (Some(&x.text), Some(Style::Link), Some(format!("mailto:{}", x.email))),
        R::TextAutoUrl(x) => (Some(&x.text), Some(Style::Link), None),
        R::TextSubscript(x) => (Some(&x.text), None, None),
        R::TextSuperscript(x) => (Some(&x.text), None, None),
        R::TextPhone(x) => (Some(&x.text), None, None),
        R::TextAnchor(x) => (Some(&x.text), None, None),
        R::TextMention(x) => (Some(&x.text), None, None),
        R::TextHashtag(x) => (Some(&x.text), None, None),
        R::TextBotCommand(x) => (Some(&x.text), None, None),
        R::TextCashtag(x) => (Some(&x.text), None, None),
        R::TextAutoEmail(x) => (Some(&x.text), None, None),
        R::TextAutoPhone(x) => (Some(&x.text), None, None),
        R::TextBankCard(x) => (Some(&x.text), None, None),
        R::TextMentionName(x) => (Some(&x.text), None, None),
        R::TextDate(x) => (Some(&x.text), None, None),
        R::TextDiff(x) => (Some(&x.text), None, None),
        R::TextButton(x) => (Some(&x.text), None, None),
        R::TextTonAddress(x) => (Some(&x.text), None, None),
    };
    if let Some(inner) = inner {
        push_rich(out, inner, depth + 1);
    }
    // An auto-detected link points at its own text.
    let target = match (style, target) {
        (Some(Style::Link), None) => Some(out.text[start..].to_owned()),
        (_, t) => t,
    };
    if let Some(style) = style
        && out.text.len() > start
    {
        out.spans.push(Span { start, end: out.text.len(), style, target });
    }
}

/// The page's photos and documents by id, for blocks that refer to them.
struct Media<'a> {
    photos: HashMap<i64, &'a tl::types::Photo>,
    docs: HashMap<i64, &'a tl::types::Document>,
}

impl Media<'_> {
    fn photo_size(&self, id: i64) -> Option<(i32, i32)> {
        let p = self.photos.get(&id)?;
        p.sizes.iter().filter_map(size_of).max_by_key(|(w, h)| w * h)
    }

    fn video_size(&self, id: i64) -> (i32, i32) {
        self.docs
            .get(&id)
            .and_then(|d| {
                d.attributes.iter().find_map(|a| match a {
                    tl::enums::DocumentAttribute::Video(v) => Some((v.w, v.h)),
                    _ => None,
                })
            })
            .unwrap_or((640, 360))
    }
}

fn size_of(s: &tl::enums::PhotoSize) -> Option<(i32, i32)> {
    match s {
        tl::enums::PhotoSize::Size(s) => Some((s.w, s.h)),
        tl::enums::PhotoSize::Progressive(s) => Some((s.w, s.h)),
        tl::enums::PhotoSize::PhotoCachedSize(s) => Some((s.w, s.h)),
        _ => None,
    }
}

fn caption(c: &tl::enums::PageCaption) -> (IvText, IvText) {
    let tl::enums::PageCaption::Caption(c) = c;
    (rich(&c.text), rich(&c.credit))
}

/// Converts the page; unknown or empty blocks are skipped.
pub(super) fn convert(page: &tl::types::Page) -> IvPage {
    let media = Media {
        photos: page.photos.iter().filter_map(|p| match p {
            tl::enums::Photo::Photo(p) => Some((p.id, p)),
            _ => None,
        }).collect(),
        docs: page.documents.iter().filter_map(|d| match d {
            tl::enums::Document::Document(d) => Some((d.id, d)),
            _ => None,
        }).collect(),
    };
    let mut blocks = Vec::new();
    for b in &page.blocks {
        push_block(&mut blocks, b, &media, 0);
    }
    IvPage { url: page.url.clone(), blocks, views: page.views, rtl: page.rtl }
}

fn push_block(out: &mut Vec<IvBlock>, b: &tl::enums::PageBlock, media: &Media, depth: u8) {
    use tl::enums::PageBlock as B;
    if depth > MAX_DEPTH {
        return;
    }
    let text = |t: &tl::enums::RichText, f: fn(IvText) -> IvBlock| {
        let t = rich(t);
        (!t.is_empty()).then(|| f(t))
    };
    let block = match b {
        B::Title(x) => text(&x.text, IvBlock::Title),
        B::Subtitle(x) => text(&x.text, IvBlock::Subtitle),
        B::Kicker(x) => text(&x.text, IvBlock::Kicker),
        B::AuthorDate(x) => Some(IvBlock::AuthorDate { author: rich(&x.author), date: x.published_date as i64 }),
        B::Header(x) => text(&x.text, IvBlock::Header),
        B::Heading1(x) => text(&x.text, IvBlock::Header),
        B::Heading2(x) => text(&x.text, IvBlock::Header),
        B::Subheader(x) => text(&x.text, IvBlock::Subheader),
        B::Heading3(x) => text(&x.text, IvBlock::Subheader),
        B::Heading4(x) => text(&x.text, IvBlock::Subheader),
        B::Heading5(x) => text(&x.text, IvBlock::Subheader),
        B::Heading6(x) => text(&x.text, IvBlock::Subheader),
        B::Paragraph(x) => text(&x.text, IvBlock::Paragraph),
        B::Thinking(x) => text(&x.text, IvBlock::Paragraph),
        B::Footer(x) => text(&x.text, IvBlock::Footer),
        B::Preformatted(x) => Some(IvBlock::Preformatted(rich(&x.text).text)),
        B::Math(x) => Some(IvBlock::Preformatted(x.source.clone())),
        B::Divider => Some(IvBlock::Divider),
        B::List(x) => Some(list(x, media, depth)),
        B::OrderedList(x) => Some(ordered_list(x, media, depth)),
        B::Blockquote(x) => Some(IvBlock::Blockquote { text: rich(&x.text), caption: rich(&x.caption) }),
        B::BlockquoteBlocks(x) => {
            // Paragraphs of a quote joined into one, so it keeps a single bar.
            let mut t = IvText::default();
            for b in children(&x.blocks, media, depth) {
                if let IvBlock::Paragraph(p) = b {
                    if !t.text.is_empty() {
                        t.text.push_str("\n\n");
                    }
                    let shift = t.text.len();
                    t.text.push_str(&p.text);
                    t.spans.extend(p.spans.into_iter().map(|s| Span { start: s.start + shift, end: s.end + shift, ..s }));
                }
            }
            Some(IvBlock::Blockquote { text: t, caption: rich(&x.caption) })
        }
        B::Pullquote(x) => Some(IvBlock::Pullquote { text: rich(&x.text), caption: rich(&x.caption) }),
        B::Photo(x) => media.photo_size(x.photo_id).map(|size| {
            let (caption, credit) = caption(&x.caption);
            IvBlock::Media { id: x.photo_id, size, video: false, caption, credit, url: x.url.clone() }
        }),
        B::Video(x) => {
            let (caption, credit) = caption(&x.caption);
            Some(IvBlock::Media { id: x.video_id, size: media.video_size(x.video_id), video: true, caption, credit, url: None })
        }
        B::Cover(x) => {
            push_block(out, &x.cover, media, depth + 1);
            None
        }
        B::Collage(x) => {
            out.extend(children(&x.items, media, depth));
            caption_block(&x.caption)
        }
        B::Slideshow(x) => {
            out.extend(children(&x.items, media, depth));
            caption_block(&x.caption)
        }
        B::Details(x) => {
            out.extend(text(&x.title, IvBlock::Subheader));
            out.extend(children(&x.blocks, media, depth));
            None
        }
        B::Embed(x) => x.url.clone().filter(|u| !u.is_empty()).map(|url| IvBlock::Embed { url, caption: caption(&x.caption).0 }),
        B::EmbedPost(x) => {
            if !x.author.is_empty() {
                out.push(IvBlock::Subheader(IvText::plain(&x.author)));
            }
            out.extend(children(&x.blocks, media, depth));
            (!x.url.is_empty()).then(|| IvBlock::Embed { url: x.url.clone(), caption: caption(&x.caption).0 })
        }
        B::RelatedArticles(x) => {
            let articles: Vec<IvArticle> = x.articles.iter().map(|a| {
                let tl::enums::PageRelatedArticle::Article(a) = a;
                IvArticle {
                    url: a.url.clone(),
                    title: a.title.clone().unwrap_or_else(|| a.url.clone()),
                    description: a.description.clone().unwrap_or_default(),
                    photo: a.photo_id.filter(|id| media.photos.contains_key(id)),
                }
            }).collect();
            (!articles.is_empty()).then(|| IvBlock::Related { title: rich(&x.title), articles })
        }
        B::Table(x) => {
            table(out, x);
            None
        }
        // Anchors, channels, audio, maps, buttons, documents and anything newer are skipped.
        _ => None,
    };
    out.extend(block);
}

/// The plain text of a list item or table cell, as a paragraph unless empty.
fn paragraph(t: &tl::enums::RichText) -> Option<IvBlock> {
    let t = rich(t);
    (!t.is_empty()).then_some(IvBlock::Paragraph(t))
}

fn list(x: &tl::types::PageBlockList, media: &Media, depth: u8) -> IvBlock {
    let items = x.items.iter().map(|it| {
        let blocks = match it {
            tl::enums::PageListItem::Text(t) => paragraph(&t.text).into_iter().collect(),
            tl::enums::PageListItem::Blocks(t) => children(&t.blocks, media, depth),
        };
        ("•".to_owned(), blocks)
    });
    IvBlock::List(items.collect())
}

fn ordered_list(x: &tl::types::PageBlockOrderedList, media: &Media, depth: u8) -> IvBlock {
    let first = x.start.unwrap_or(1);
    let n = x.items.len() as i32;
    let items = x.items.iter().enumerate().map(|(i, it)| {
        let i = i as i32;
        let auto = if x.reversed { first + n - 1 - i } else { first + i };
        let (num, blocks) = match it {
            tl::enums::PageListOrderedItem::Text(t) => (t.num.clone(), paragraph(&t.text).into_iter().collect()),
            tl::enums::PageListOrderedItem::Blocks(t) => (t.num.clone(), children(&t.blocks, media, depth)),
        };
        (format!("{}.", num.unwrap_or_else(|| auto.to_string())), blocks)
    });
    IvBlock::List(items.collect())
}

/// No grid yet: the title, then each row as a line with its cells separated.
fn table(out: &mut Vec<IvBlock>, x: &tl::types::PageBlockTable) {
    let title = rich(&x.title);
    if !title.is_empty() {
        out.push(IvBlock::Subheader(title));
    }
    for row in &x.rows {
        let tl::enums::PageTableRow::Row(row) = row;
        let cells: Vec<String> = row.cells.iter().map(|c| {
            let tl::enums::PageTableCell::Cell(c) = c;
            c.text.as_ref().map(|t| rich(t).text).unwrap_or_default()
        }).collect();
        let line = cells.join("  ·  ");
        if !line.trim().is_empty() {
            out.push(IvBlock::Paragraph(IvText::plain(&line)));
        }
    }
}

fn children(blocks: &[tl::enums::PageBlock], media: &Media, depth: u8) -> Vec<IvBlock> {
    let mut out = Vec::new();
    for b in blocks {
        push_block(&mut out, b, media, depth + 1);
    }
    out
}

fn caption_block(c: &tl::enums::PageCaption) -> Option<IvBlock> {
    let (text, _) = caption(c);
    (!text.is_empty()).then(|| IvBlock::Footer(text))
}

/// Ids of pictures the blocks show, in order, and whether each is a video's cover.
fn pictures(blocks: &[IvBlock], out: &mut Vec<(i64, bool)>) {
    for b in blocks {
        match b {
            IvBlock::Media { id, video, .. } => out.push((*id, *video)),
            IvBlock::List(items) => items.iter().for_each(|(_, b)| pictures(b, out)),
            IvBlock::Related { articles, .. } => out.extend(articles.iter().filter_map(|a| a.photo.map(|id| (id, false)))),
            _ => {}
        }
    }
}

/// Starts downloading the page's pictures: photos by their best size, videos by their cover.
fn load_pictures(loader: &Loader, page: &tl::types::Page, iv: &IvPage) {
    let mut ids = Vec::new();
    pictures(&iv.blocks, &mut ids);
    for (id, video) in ids.into_iter().take(MAX_PICTURES) {
        if video {
            let doc = page.documents.iter().find_map(|d| match d {
                tl::enums::Document::Document(d) if d.id == id => Some(d),
                _ => None,
            });
            let thumb = doc.and_then(|d| {
                d.thumbs.iter().flatten().filter_map(|t| match t {
                    tl::enums::PhotoSize::Size(s) => Some((s.w, s.r#type.clone())),
                    tl::enums::PhotoSize::Progressive(s) => Some((s.w, s.r#type.clone())),
                    _ => None,
                }).max_by_key(|t| t.0)
            });
            if let (Some(doc), Some((_, thumb))) = (doc, thumb) {
                loader.fetch(ImgKey::Page(id), format!("iv{id}.jpg"), PHOTO_PT, DocLoc { doc: doc.clone(), thumb });
            }
        } else if let Some(photo) = page.photos.iter().find(|p| matches!(p, tl::enums::Photo::Photo(p) if p.id == id)) {
            loader.page_photo(ImgKey::Page(id), format!("iv{id}.jpg"), Photo::from_raw(photo.clone()));
        }
    }
}

impl Worker {
    pub(super) async fn instant_view_cmd(&mut self, cmd: Cmd) -> Result<(), String> {
        let Cmd::InstantView { url } = cmd else { return Ok(()) };
        let client = self.client()?;
        let (out, loader) = (self.out.clone(), self.loader.clone());
        tokio::spawn(async move {
            let req = tl::functions::messages::GetWebPage { url: url.clone(), hash: 0 };
            let result = match client.invoke(&req).await {
                Ok(tl::enums::messages::WebPage::Page(w)) => match w.webpage {
                    tl::enums::WebPage::Page(tl::types::WebPage { cached_page: Some(tl::enums::Page::Page(page)), .. }) => {
                        let iv = convert(&page);
                        if let Some(loader) = &loader {
                            load_pictures(loader, &page, &iv);
                        }
                        Ok(iv)
                    }
                    tl::enums::WebPage::Pending(_) => Err(l!("Сторінку ще готують, спробуйте трохи пізніше", "The page is still being prepared, try again a bit later").to_owned()),
                    _ => Err(l!("Для цієї сторінки немає миттєвого перегляду", "This page has no Instant View").to_owned()),
                },
                Err(e) => Err(lf!("Не вдалося завантажити статтю: {e}", "Could not load the article: {e}")),
            };
            out.send(Ev::InstantView { url, result });
        });
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn plain(s: &str) -> tl::enums::RichText {
        tl::types::TextPlain { text: s.into() }.into()
    }

    #[test]
    fn rich_text_spans() {
        let link: tl::enums::RichText = tl::types::TextUrl { text: plain("the docs"), url: "https://docs.rs".into(), webpage_id: 0 }.into();
        let t: tl::enums::RichText = tl::types::TextConcat { texts: vec![plain("Read "), tl::types::TextBold { text: link }.into(), plain(" now")] }.into();
        let r = rich(&t);
        assert_eq!(r.text, "Read the docs now");
        assert!(r.spans.iter().any(|s| s.style == Style::Link && s.target.as_deref() == Some("https://docs.rs") && &r.text[s.start..s.end] == "the docs"));
        assert!(r.spans.iter().any(|s| s.style == Style::Bold && (s.start, s.end) == (5, 13)));
    }

    #[test]
    fn blocks_and_unknown() {
        let caption = tl::types::PageCaption { text: plain("c"), credit: tl::enums::RichText::TextEmpty }.into();
        let item = tl::types::PageListOrderedItemText { checkbox: false, checked: false, num: None, text: plain("one"), value: None, r#type: None }.into();
        let page = tl::types::Page {
            part: false,
            rtl: false,
            v2: true,
            url: "https://telegra.ph/a".into(),
            blocks: vec![
                tl::types::PageBlockTitle { text: plain("Title") }.into(),
                tl::enums::PageBlock::Unsupported,
                tl::types::PageBlockParagraph { text: tl::enums::RichText::TextEmpty }.into(),
                tl::types::PageBlockOrderedList { reversed: false, items: vec![item], start: Some(3), r#type: None }.into(),
                tl::types::PageBlockPhoto { spoiler: false, photo_id: 7, caption, url: None, webpage_id: None }.into(),
            ],
            photos: Vec::new(),
            documents: Vec::new(),
            views: Some(5),
        };
        let iv = convert(&page);
        // The empty paragraph, the unsupported block and the photo missing from `photos` are dropped.
        assert_eq!(iv.blocks.len(), 2);
        assert_eq!(iv.blocks[0], IvBlock::Title(IvText::plain("Title")));
        assert!(matches!(&iv.blocks[1], IvBlock::List(items) if items[0].0 == "3."));
        assert_eq!(iv.views, Some(5));
    }
}
