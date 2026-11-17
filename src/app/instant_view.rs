//! Instant View, as in Telegram Desktop: link previews with a server-side copy of the article get an
//! "Instant View" button, and the article opens in a reading window over the chat.

use super::*;
use crate::backend::{IvArticle, IvBlock, IvPage, IvText};
use crate::format::Span;

/// Widest column of article text: longer lines are hard to read.
const COLUMN_W: f32 = 640.0;
/// Pictures are not drawn taller than this.
const MEDIA_MAX_H: f32 = 460.0;

#[derive(Default)]
pub(super) struct InstantView {
    open: Option<Opened>,
}

struct Opened {
    /// The chat it was opened from: switching chats closes it.
    peer: PeerId,
    url: String,
    /// Site name for the window header, from the link preview.
    site: String,
    /// `None` — waiting for the server.
    page: Option<Result<IvPage, String>>,
}

/// What a click in the article asks for.
enum Click {
    Link(String),
    /// The article itself in the browser (a video we can't play here, the header button).
    Browser,
}

impl App {
    pub(super) fn open_instant_view(&mut self, peer: PeerId, id: i32) {
        let Some(w) = self.find(peer, id).and_then(|m| m.webpage.clone()) else { return };
        self.instant_view.open = Some(Opened { peer, url: w.url.clone(), site: w.site, page: None });
        self.send(Cmd::InstantView { url: w.url });
    }

    pub(super) fn on_instant_view(&mut self, url: String, result: Result<IvPage, String>) {
        if let Some(o) = &mut self.instant_view.open
            && o.url == url
        {
            o.page = Some(result);
        }
    }

    /// The reading window, drawn over the chat.
    pub(super) fn instant_view_window(&mut self, ui: &mut Ui, peer: PeerId, rect: Rect) {
        let Some(o) = &self.instant_view.open else { return };
        if o.peer != peer || ui.input_mut(|i| i.consume_key(egui::Modifiers::NONE, Key::Escape)) {
            self.instant_view.open = None;
            return;
        }
        let panel = Rect::from_center_size(rect.center(), vec2((rect.width() - 32.0).min(COLUMN_W + 56.0), rect.height() - 16.0));
        // Swallows clicks and scrolling meant for the article, so the chat under it stays put.
        ui.interact(panel, ui.scope_id().with("iv-panel"), Sense::click_and_drag());
        // Dense glass: long text reads on it like on paper, the chat only shows through faintly.
        glass::glass(ui.painter(), panel, Glass::tinted(26.0, glass::chrome(), 220).frosted(1.0).with_shadow());
        let p = ui.painter().clone();
        let head_y = panel.top() + 28.0;
        let site = if o.site.is_empty() { l!("Миттєвий перегляд", "Instant View").to_owned() } else { o.site.clone() };
        let title_w = panel.width() - 120.0;
        let title = p.layout_job(one_line(&site, FontId::new(16.0, egui::FontFamily::Name(format::BOLD.into())), ink(), title_w));
        p.galley(pos2(panel.left() + 20.0, head_y - title.size().y / 2.0), title, ink());
        let close = Rect::from_center_size(pos2(panel.right() - 28.0, head_y), vec2(34.0, 34.0));
        let browser = close.translate(vec2(-40.0, 0.0));
        let mut click = None;
        let closed = glass::icon_button(ui, close, "iv-close", Icon::Close, false, true).clicked();
        if glass::icon_button(ui, browser, "iv-browser", Icon::Globe, false, true).on_hover_text(l!("Відкрити в браузері", "Open in browser")).clicked() {
            click = Some(Click::Browser);
        }
        p.hline(panel.x_range().shrink(18.0), panel.top() + 54.0, egui::Stroke::new(1.0, glass::veil(22)));

        let body = Rect::from_min_max(pos2(panel.left() + 6.0, panel.top() + 56.0), pos2(panel.right() - 6.0, panel.bottom() - 12.0));
        let mut child = ui.new_child(UiBuilder::new().max_rect(body));
        egui::ScrollArea::vertical().id_salt(("iv", &o.url)).auto_shrink(false).show(&mut child, |ui| {
            let side = ((ui.available_width() - COLUMN_W) / 2.0).max(16.0);
            ui.add_space(12.0);
            ui.horizontal(|ui| {
                ui.add_space(side);
                ui.vertical(|ui| {
                    ui.set_width(ui.available_width() - side);
                    ui.spacing_mut().item_spacing.y = 0.0;
                    match &o.page {
                        None => {
                            ui.add_space(60.0);
                            ui.vertical_centered(|ui| ui.add(egui::Spinner::new().color(ink())));
                        }
                        Some(Err(e)) => {
                            ui.add_space(40.0);
                            ui.label(egui::RichText::new(e).size(14.5).color(glass::danger()));
                        }
                        Some(Ok(page)) => {
                            let r = Reader { tex: &self.textures, depth: 0 };
                            click = r.blocks(ui, &page.blocks).or(click.take());
                            if let Some(n) = page.views.filter(|n| *n > 0) {
                                ui.add_space(16.0);
                                let views = lp!(n, ["{n} перегляд", "{n} перегляди", "{n} переглядів"], ["{n} view", "{n} views"]);
                                ui.label(egui::RichText::new(views).size(12.5).color(ink_dim()));
                            }
                        }
                    }
                    ui.add_space(24.0);
                });
            });
        });
        let page_url = o.url.clone();
        if closed {
            self.instant_view.open = None;
        }
        match click {
            Some(Click::Link(url)) => self.open_link(&url),
            Some(Click::Browser) => self.open_link(&page_url),
            None => {}
        }
    }
}

fn one_line(text: &str, font: FontId, color: Color32, width: f32) -> LayoutJob {
    let mut job = LayoutJob::simple(text.to_owned(), font, color, width);
    job.wrap = TextWrapping { max_rows: 1, break_anywhere: true, overflow_character: Some('…'), max_width: width };
    job
}

/// Draws blocks top to bottom; borrows only what drawing needs, so `App` stays free for the click.
struct Reader<'a> {
    tex: &'a textures::Textures,
    /// Nesting of lists, for the indent.
    depth: u8,
}

impl Reader<'_> {
    fn blocks(&self, ui: &mut Ui, blocks: &[IvBlock]) -> Option<Click> {
        let mut click = None;
        for (i, b) in blocks.iter().enumerate() {
            ui.push_id(i, |ui| {
                click = self.block(ui, b).or(click.take());
            });
        }
        click
    }

    fn block(&self, ui: &mut Ui, b: &IvBlock) -> Option<Click> {
        let bold = Some(Style::Bold);
        match b {
            IvBlock::Kicker(t) => {
                ui.add_space(8.0);
                text(ui, t, 12.5, glass::link(false), bold)
            }
            IvBlock::Title(t) => {
                ui.add_space(8.0);
                text(ui, t, 24.0, ink(), bold)
            }
            IvBlock::Subtitle(t) => {
                ui.add_space(8.0);
                text(ui, t, 17.0, ink_dim(), None)
            }
            IvBlock::AuthorDate { author, date } => {
                ui.add_space(10.0);
                let mut line = author.clone();
                let when = day(*date);
                if !when.is_empty() {
                    if !line.is_empty() {
                        line.text.push_str(" • ");
                    }
                    line.text.push_str(&when);
                }
                text(ui, &line, 12.5, ink_dim(), None)
            }
            IvBlock::Header(t) => {
                ui.add_space(20.0);
                text(ui, t, 20.0, ink(), bold)
            }
            IvBlock::Subheader(t) => {
                ui.add_space(16.0);
                text(ui, t, 17.0, ink(), bold)
            }
            IvBlock::Paragraph(t) => {
                ui.add_space(12.0);
                text(ui, t, 15.0, ink(), None)
            }
            IvBlock::Footer(t) => {
                ui.add_space(12.0);
                text(ui, t, 12.5, ink_dim(), None)
            }
            IvBlock::Preformatted(code) => {
                ui.add_space(12.0);
                let w = ui.available_width();
                let job = format::layout(code, &[Span { start: 0, end: code.len(), style: Style::Pre, target: None }], 14.0, ink(), glass::link(false), w - 24.0, true);
                let g = ui.painter().layout_job(job);
                let (r, _) = ui.allocate_exact_size(vec2(w, g.size().y + 20.0), Sense::hover());
                ui.painter().rect_filled(r, 12.0, glass::veil(14));
                ui.painter().galley(r.min + vec2(12.0, 10.0), g, ink());
                None
            }
            IvBlock::Divider => {
                ui.add_space(16.0);
                let (r, _) = ui.allocate_exact_size(vec2(ui.available_width(), 4.0), Sense::hover());
                let x = r.center().x;
                ui.painter().hline((x - 60.0)..=(x + 60.0), r.center().y, egui::Stroke::new(1.0, glass::veil(40)));
                None
            }
            IvBlock::List(items) => self.list(ui, items),
            IvBlock::Blockquote { text: t, caption } => {
                ui.add_space(14.0);
                quote(ui, |ui| {
                    let c = text(ui, t, 15.0, ink(), Some(Style::Italic));
                    caption_line(ui, caption).or(c)
                })
            }
            IvBlock::Pullquote { text: t, caption } => {
                ui.add_space(16.0);
                ui.horizontal(|ui| {
                    ui.add_space(24.0);
                    ui.vertical(|ui| {
                        ui.set_width(ui.available_width() - 24.0);
                        let c = text(ui, t, 17.0, ink(), Some(Style::Italic));
                        caption_line(ui, caption).or(c)
                    })
                    .inner
                })
                .inner
            }
            IvBlock::Media { id, size, video, caption, credit, url } => {
                ui.add_space(14.0);
                let mut click = self.media(ui, *id, *size, *video, url.as_deref());
                click = caption_line(ui, caption).or(click);
                if !credit.is_empty() {
                    ui.add_space(2.0);
                    click = text(ui, credit, 11.5, ink_dim(), None).or(click);
                }
                click
            }
            IvBlock::Embed { url, caption } => {
                ui.add_space(14.0);
                let click = link_row(ui, url, url, "");
                caption_line(ui, caption).or(click)
            }
            IvBlock::Related { title, articles } => self.related(ui, title, articles),
        }
    }

    fn list(&self, ui: &mut Ui, items: &[(String, Vec<IvBlock>)]) -> Option<Click> {
        let mut click = None;
        let font = FontId::proportional(15.0);
        let mark_w = items.iter().map(|(m, _)| m.chars().count()).max().unwrap_or(1) as f32 * 9.0 + 12.0;
        let inner = Reader { tex: self.tex, depth: self.depth + 1 };
        for (i, (mark, blocks)) in items.iter().enumerate() {
            ui.push_id(("li", i), |ui| {
                let top = ui.cursor().top();
                let left = ui.cursor().left();
                ui.horizontal_top(|ui| {
                    ui.add_space(mark_w);
                    ui.vertical(|ui| {
                        ui.set_width(ui.available_width());
                        click = inner.blocks(ui, blocks).or(click.take());
                    });
                });
                // The marker sits on the first line, which starts after the item's top spacing.
                let first_line = if matches!(blocks.first(), Some(IvBlock::Paragraph(_))) { 12.0 } else { 0.0 };
                ui.painter().text(pos2(left + mark_w - 8.0, top + first_line), Align2::RIGHT_TOP, mark, font.clone(), ink_dim());
            });
        }
        click
    }

    fn media(&self, ui: &mut Ui, id: i64, size: (i32, i32), video: bool, url: Option<&str>) -> Option<Click> {
        let w = ui.available_width();
        let ratio = size.1.max(1) as f32 / size.0.max(1) as f32;
        let h = (w * ratio).min(MEDIA_MAX_H);
        let shown_w = (h / ratio).min(w);
        let (row, _) = ui.allocate_exact_size(vec2(w, h), Sense::hover());
        let r = Rect::from_center_size(row.center(), vec2(shown_w, h));
        let resp = ui.interact(r, ui.scope_id().with(("iv-media", id)), if video || url.is_some() { Sense::click() } else { Sense::hover() });
        let p = ui.painter();
        match self.tex.get(&ImgKey::Page(id)) {
            Some(t) => glass::image_rounded(p, r, 12.0, t.id(), t.size_vec2()),
            None => {
                p.rect_filled(r, 12.0, glass::veil(14));
                icons::draw(p, Rect::from_center_size(r.center(), vec2(28.0, 28.0)), Icon::Image, ink_dim());
            }
        }
        if video {
            let c = Rect::from_center_size(r.center(), vec2(52.0, 52.0));
            p.circle_filled(c.center(), 26.0, Color32::from_black_alpha(130));
            icons::draw(p, c.shrink(14.0), Icon::Play, glass::ON_DARK);
        }
        if resp.hovered() && (video || url.is_some()) {
            ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
        }
        if video {
            return resp.on_hover_text(l!("Відкрити в браузері", "Open in browser")).clicked().then_some(Click::Browser);
        }
        url.filter(|_| resp.clicked()).map(|u| Click::Link(u.to_owned()))
    }

    fn related(&self, ui: &mut Ui, title: &IvText, articles: &[IvArticle]) -> Option<Click> {
        ui.add_space(24.0);
        let title = if title.is_empty() { IvText::plain(l!("Схожі статті", "Related articles")) } else { title.clone() };
        let mut click = text(ui, &title, 17.0, ink(), Some(Style::Bold));
        ui.add_space(4.0);
        for (i, a) in articles.iter().enumerate() {
            ui.push_id(("rel", i), |ui| {
                ui.add_space(6.0);
                let thumb = a.photo.and_then(|id| self.tex.get(&ImgKey::Page(id)));
                click = related_row(ui, a, thumb).or(click.take());
            });
        }
        click
    }
}

/// Rich text with clickable links; `extra` styles the whole block (bold headers, italic quotes).
fn text(ui: &mut Ui, t: &IvText, size: f32, color: Color32, extra: Option<Style>) -> Option<Click> {
    let mut spans = t.spans.clone();
    if let Some(style) = extra {
        spans.push(Span { start: 0, end: t.text.len(), style, target: None });
    }
    let job = format::layout(&t.text, &spans, size, color, glass::link(false), ui.available_width(), true);
    let g = ui.painter().layout_job(job);
    let has_links = t.spans.iter().any(|s| s.style == Style::Link && s.target.is_some());
    let (r, resp) = ui.allocate_exact_size(g.size(), if has_links { Sense::click() } else { Sense::hover() });
    ui.painter().galley(r.min, g.clone(), color);
    let pos = resp.hover_pos().filter(|_| has_links)?;
    let ch = g.cursor_from_pos(pos - r.min).index.0;
    let span = format::span_at(&t.spans, format::char_to_byte(&t.text, ch)).filter(|s| s.style == Style::Link)?;
    let target = span.target.clone()?;
    ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
    let resp = resp.on_hover_text(&target);
    resp.clicked().then_some(Click::Link(target))
}

fn caption_line(ui: &mut Ui, t: &IvText) -> Option<Click> {
    if t.is_empty() {
        return None;
    }
    ui.add_space(6.0);
    text(ui, t, 12.5, ink_dim(), None)
}

/// A block with the accent bar on the left, as quotes are drawn in bubbles.
fn quote(ui: &mut Ui, add: impl FnOnce(&mut Ui) -> Option<Click>) -> Option<Click> {
    let top = ui.cursor().top();
    let left = ui.cursor().left();
    let click = ui
        .horizontal_top(|ui| {
            ui.add_space(16.0);
            ui.vertical(|ui| {
                ui.set_width(ui.available_width());
                add(ui)
            })
            .inner
        })
        .inner;
    let bottom = ui.min_rect().bottom();
    ui.painter().rect_filled(Rect::from_min_max(pos2(left, top), pos2(left + 3.0, bottom)), 2.0, ACCENT);
    click
}

/// An embedded page as a link card.
fn link_row(ui: &mut Ui, url: &str, title: &str, desc: &str) -> Option<Click> {
    let w = ui.available_width();
    let font = FontId::new(14.0, egui::FontFamily::Name(format::BOLD.into()));
    let t = ui.painter().layout_job(one_line(title, font, ink(), w - 56.0));
    let d = (!desc.is_empty()).then(|| ui.painter().layout_job(one_line(desc, FontId::proportional(12.5), ink_dim(), w - 56.0)));
    let h = (t.size().y + d.as_ref().map_or(0.0, |d| d.size().y + 2.0) + 20.0).max(48.0);
    let (r, resp) = ui.allocate_exact_size(vec2(w, h), Sense::click());
    let p = ui.painter();
    p.rect_filled(r, 14.0, glass::veil(if resp.hovered() { 22 } else { 12 }));
    icons::draw(p, Rect::from_center_size(pos2(r.left() + 24.0, r.center().y), vec2(20.0, 20.0)), Icon::Link, ACCENT);
    let mut y = r.center().y - (h - 20.0) / 2.0;
    let th = t.size().y;
    p.galley(pos2(r.left() + 46.0, y), t, ink());
    y += th + 2.0;
    if let Some(d) = d {
        p.galley(pos2(r.left() + 46.0, y), d, ink_dim());
    }
    if resp.hovered() {
        ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
    }
    resp.on_hover_text(url).clicked().then(|| Click::Link(url.to_owned()))
}

/// A related article: its picture on the right, title and description on the left.
fn related_row(ui: &mut Ui, a: &IvArticle, thumb: Option<&TextureHandle>) -> Option<Click> {
    let w = ui.available_width();
    let pic = if thumb.is_some() || a.photo.is_some() { 64.0 } else { 0.0 };
    let text_w = w - 24.0 - if pic > 0.0 { pic + 12.0 } else { 0.0 };
    let bold = FontId::new(14.5, egui::FontFamily::Name(format::BOLD.into()));
    let mut job = LayoutJob::simple(a.title.clone(), bold, ink(), text_w);
    job.wrap = TextWrapping { max_rows: 2, break_anywhere: false, overflow_character: Some('…'), max_width: text_w };
    let t = ui.painter().layout_job(job);
    let mut job = LayoutJob::simple(a.description.clone(), FontId::proportional(13.0), ink_dim(), text_w);
    job.wrap = TextWrapping { max_rows: 2, break_anywhere: false, overflow_character: Some('…'), max_width: text_w };
    let d = (!a.description.is_empty()).then(|| ui.painter().layout_job(job));
    let h = (t.size().y + d.as_ref().map_or(0.0, |d| d.size().y + 2.0) + 20.0).max(pic + 16.0);
    let (r, resp) = ui.allocate_exact_size(vec2(w, h), Sense::click());
    let p = ui.painter();
    p.rect_filled(r, 14.0, glass::veil(if resp.hovered() { 22 } else { 12 }));
    let th = t.size().y;
    p.galley(pos2(r.left() + 12.0, r.top() + 10.0), t, ink());
    if let Some(d) = d {
        p.galley(pos2(r.left() + 12.0, r.top() + 12.0 + th), d, ink_dim());
    }
    if pic > 0.0 {
        let pr = Rect::from_min_size(pos2(r.right() - 8.0 - pic, r.center().y - pic / 2.0), vec2(pic, pic));
        match thumb {
            Some(t) => glass::image_rounded(p, pr, 10.0, t.id(), t.size_vec2()),
            None => {
                p.rect_filled(pr, 10.0, glass::veil(16));
            }
        }
    }
    if resp.hovered() {
        ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
    }
    resp.on_hover_text(&a.url).clicked().then(|| Click::Link(a.url.clone()))
}

/// The publication date in the reader's time zone.
fn day(ts: i64) -> String {
    if ts <= 0 {
        return String::new();
    }
    jiff::Timestamp::from_second(ts).map(|t| t.to_zoned(jiff::tz::TimeZone::system()).strftime("%d.%m.%Y").to_string()).unwrap_or_default()
}
