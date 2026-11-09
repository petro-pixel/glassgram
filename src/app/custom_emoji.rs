//! Custom emoji in message text: the fallback emoji keeps its place in the layout and the
//! picture is drawn over it once it arrives. Bubbles are free functions, so the pictures are
//! reached through the egui context rather than passed down every call.

use std::sync::{Arc, Mutex};

use super::*;
use crate::format::{Span, Style};
use eframe::egui::{Galley, Painter, text::CCursor, text::LayoutJob};

#[derive(Default)]
struct Shelf {
    /// Pictures by document id. Tiny, so they stay outside the texture budget.
    tex: HashMap<i64, TextureHandle>,
    /// Ids drawn this frame without a picture.
    wanted: HashSet<i64>,
    /// Ids already asked for: they are not asked again.
    asked: HashSet<i64>,
}

/// Shared with the painting code through `ctx.data`.
#[derive(Clone, Default)]
pub(super) struct CustomEmoji(Arc<Mutex<Shelf>>);

fn slot() -> egui::Id {
    egui::Id::unique("custom-emoji-shelf")
}

impl CustomEmoji {
    fn of(ctx: &egui::Context) -> Option<Self> {
        ctx.data(|d| d.get_temp::<Self>(slot()))
    }

    /// Keeps a picture that came for a custom emoji.
    pub(super) fn on_image(&self, key: ImgKey, tex: &TextureHandle) {
        if let ImgKey::Emoji(id) = key
            && let Ok(mut s) = self.0.lock()
        {
            s.tex.insert(id, tex.clone());
        }
    }

    fn texture(&self, id: i64) -> Option<egui::TextureId> {
        let mut s = self.0.lock().ok()?;
        let t = s.tex.get(&id).map(|t| t.id());
        if t.is_none() {
            s.wanted.insert(id);
        }
        t
    }

    /// Once a frame: publishes itself for the painting code and returns ids to ask for.
    fn frame(&self, ctx: &egui::Context) -> Vec<i64> {
        ctx.data_mut(|d| d.insert_temp(slot(), self.clone()));
        let Ok(mut s) = self.0.lock() else { return Vec::new() };
        let wanted = std::mem::take(&mut s.wanted);
        let Shelf { tex, asked, .. } = &mut *s;
        wanted.into_iter().filter(|id| !tex.contains_key(id) && asked.insert(*id)).collect()
    }

    /// An account switch: another account may not see the same documents.
    pub(super) fn clear(&self) {
        if let Ok(mut s) = self.0.lock() {
            *s = Shelf::default();
        }
    }
}

impl App {
    /// Asks the backend for the custom emoji drawn without a picture last frame.
    pub(super) fn custom_emoji_frame(&mut self, ctx: &egui::Context) {
        let ids = self.custom_emoji.frame(ctx);
        // The lock is released before sending: the demo answers inside `send`.
        if !ids.is_empty() {
            self.send(Cmd::LoadCustomEmoji(ids));
        }
    }
}

/// Spans of custom emoji that should show a picture (not under a hidden spoiler).
fn emoji_spans(spans: &[Span], revealed: bool) -> impl Iterator<Item = (&Span, i64)> {
    spans.iter().filter_map(move |s| {
        let id = s.emoji_id()?;
        let hidden = !revealed && spans.iter().any(|o| o.style == Style::Spoiler && o.start < s.end && s.start < o.end);
        (!hidden).then_some((s, id))
    })
}

/// Lays out text whose custom emoji already have pictures: their fallback text turns invisible
/// but keeps its width, so nothing moves when the picture lands.
pub(super) fn galley(p: &Painter, mut job: LayoutJob, spans: &[Span], revealed: bool) -> Arc<Galley> {
    if let Some(shelf) = CustomEmoji::of(p.ctx()) {
        for (s, id) in emoji_spans(spans, revealed) {
            if shelf.texture(id).is_none() {
                continue;
            }
            for sec in &mut job.sections {
                if sec.byte_range.start.0 >= s.start && sec.byte_range.end.0 <= s.end {
                    sec.format.color = Color32::TRANSPARENT;
                }
            }
        }
    }
    p.layout_job(job)
}

/// Draws the pictures of custom emoji over a galley painted at `origin`.
pub(super) fn paint(p: &Painter, galley: &Galley, origin: egui::Pos2, text: &str, spans: &[Span], revealed: bool) {
    let Some(shelf) = CustomEmoji::of(p.ctx()) else { return };
    for (s, id) in emoji_spans(spans, revealed) {
        let Some(tex) = shelf.texture(id) else { continue };
        let (Some(head), Some(tail)) = (text.get(..s.start), text.get(..s.end)) else { continue };
        let a = galley.pos_from_cursor(CCursor::new(head.chars().count()));
        let b = galley.pos_from_cursor(CCursor::new(tail.chars().count()));
        // The fallback emoji's box; a span split across rows is drawn at its start.
        let width = if (a.min.y - b.min.y).abs() < 1.0 { b.min.x - a.min.x } else { a.height() };
        let side = a.height().min(width.max(a.height() * 0.8)) * 1.05;
        let center = origin + vec2(a.min.x + width / 2.0, a.center().y);
        let r = Rect::from_center_size(center, vec2(side, side));
        if p.clip_rect().intersects(r) {
            p.image(tex, r, Rect::from_min_max(pos2(0.0, 0.0), pos2(1.0, 1.0)), Color32::WHITE);
        }
    }
}
