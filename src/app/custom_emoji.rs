//! Custom emoji in message text: the fallback emoji keeps its place in the layout and the
//! picture is drawn over it once it arrives. Bubbles are free functions, so the pictures are
//! reached through the egui context rather than passed down every call.
//!
//! Animated (`.tgs`) emoji play: a thread draws the whole loop once at text size, the frames are
//! kept per document id and one small texture per emoji shows the frame of the moment. Only
//! emoji painted on screen are played, and only so many at a time; the rest keep their still.

use std::path::PathBuf;
use std::sync::mpsc::{Receiver, TryRecvError, sync_channel};
use std::sync::{Arc, Mutex};

use super::*;
use crate::format::{Span, Style};
use eframe::egui::{ColorImage, Galley, Painter, text::CCursor, text::LayoutJob};

#[derive(Default)]
struct Shelf {
    /// Pictures by document id. Tiny, so they stay outside the texture budget.
    tex: HashMap<i64, TextureHandle>,
    /// Ids drawn this frame without a picture.
    wanted: HashSet<i64>,
    /// Ids already asked for: they are not asked again.
    asked: HashSet<i64>,
    /// `.tgs` files of animated emoji by document id.
    files: HashMap<i64, PathBuf>,
    /// Emoji being played.
    anims: HashMap<i64, Anim>,
    /// Animated emoji painted on screen this frame.
    seen: HashSet<i64>,
    /// Files that could not be drawn: they stay still.
    broken: HashSet<i64>,
    /// Playing is allowed (autoplay and looping on, "reduce motion" off).
    motion: bool,
}

/// Frames per second of a played emoji, and the longest loop drawn (Telegram's are up to 3 s).
const FPS: f64 = 30.0;
const MAX_SECS: f64 = 3.0;
/// Emoji played at once; beyond it the others stay still. 24 loops of 90 frames at 48 px ≈ 20 MB.
const MAX_ANIMS: usize = 24;
/// Frames of an emoji off screen this long are dropped.
const KEEP_SECS: f64 = 10.0;

struct Anim {
    /// The drawing thread's single answer: the whole loop.
    rx: Option<Receiver<Vec<Arc<ColorImage>>>>,
    frames: Vec<Arc<ColorImage>>,
    tex: Option<TextureHandle>,
    shown: usize,
    started: f64,
    seen_at: f64,
}

impl Anim {
    fn start(path: PathBuf, side: usize, now: f64) -> Self {
        let (tx, rx) = sync_channel(1);
        std::thread::spawn(move || {
            let Ok(data) = std::fs::read(&path) else { return };
            let Ok(mut anim) = crate::lottie::Lottie::load(&data, side as u16) else { return };
            let count = ((anim.duration().min(MAX_SECS) * FPS).ceil() as usize).max(1);
            let frames = (0..count)
                .map(|k| Arc::new(ColorImage::from_rgba_unmultiplied([side, side], &anim.frame(k as f64 / FPS))))
                .collect();
            let _ = tx.send(frames);
        });
        Self { rx: Some(rx), frames: Vec::new(), tex: None, shown: usize::MAX, started: now, seen_at: now }
    }

    /// Shows the frame of the moment; `false`: the file could not be drawn.
    fn advance(&mut self, ctx: &egui::Context, id: i64, now: f64) -> bool {
        if let Some(rx) = &self.rx {
            match rx.try_recv() {
                Ok(frames) => {
                    self.frames = frames;
                    self.rx = None;
                    self.started = now;
                }
                Err(TryRecvError::Empty) => return true,
                Err(TryRecvError::Disconnected) => return false,
            }
        }
        if self.frames.is_empty() {
            return false;
        }
        let k = ((now - self.started) * FPS) as usize % self.frames.len();
        if k != self.shown {
            self.shown = k;
            let img = self.frames[k].clone();
            match &mut self.tex {
                Some(t) => t.set(img, TextureOptions::LINEAR),
                None => self.tex = Some(ctx.load_texture(format!("emoji-anim{id}"), img, TextureOptions::LINEAR)),
            }
        }
        true
    }
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

    /// Keeps the file of an animated emoji, to play it once its still picture is shown.
    pub(super) fn on_file(&self, id: i64, path: PathBuf) {
        if let Ok(mut s) = self.0.lock() {
            s.files.insert(id, path);
        }
    }

    fn texture(&self, id: i64) -> Option<egui::TextureId> {
        let mut s = self.0.lock().ok()?;
        let still = s.tex.get(&id).map(|t| t.id());
        if still.is_none() {
            s.wanted.insert(id);
        }
        let playing = s.anims.get(&id).filter(|_| s.motion).and_then(|a| a.tex.as_ref()).map(|t| t.id());
        playing.or(still)
    }

    /// The emoji is painted on screen: an animated one gets played.
    fn seen(&self, id: i64) {
        if let Ok(mut s) = self.0.lock()
            && s.files.contains_key(&id)
        {
            s.seen.insert(id);
        }
    }

    /// Once a frame: publishes itself for the painting code and returns ids to ask for.
    fn frame(&self, ctx: &egui::Context, motion: bool) -> Vec<i64> {
        ctx.data_mut(|d| d.insert_temp(slot(), self.clone()));
        let Ok(mut s) = self.0.lock() else { return Vec::new() };
        s.motion = motion;
        let seen = std::mem::take(&mut s.seen);
        if motion {
            let now = ctx.input(|i| i.time);
            // Text size (15 pt, drawn a little larger) in pixels.
            let side = ((24.0 * ctx.pixels_per_point()).ceil() as usize).clamp(16, 64);
            let Shelf { tex, files, anims, broken, .. } = &mut *s;
            for id in seen {
                // The still picture first: it also means the file is on disk.
                if broken.contains(&id) || !tex.contains_key(&id) {
                    continue;
                }
                if !anims.contains_key(&id) {
                    if anims.len() >= MAX_ANIMS {
                        continue;
                    }
                    let Some(path) = files.get(&id) else { continue };
                    anims.insert(id, Anim::start(path.clone(), side, now));
                }
                if let Some(a) = anims.get_mut(&id) {
                    a.seen_at = now;
                    if !a.advance(ctx, id, now) {
                        anims.remove(&id);
                        broken.insert(id);
                    }
                }
            }
            anims.retain(|_, a| now - a.seen_at < KEEP_SECS);
        } else {
            s.anims.clear();
        }
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
        let s = &self.settings;
        let motion = s.autoplay && s.loop_stickers && !s.reduce_motion;
        let ids = self.custom_emoji.frame(ctx, motion);
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
    let Some(shelf) = CustomEmoji::of(p.ctx()) else { return p.layout_job(job) };
    let hide: Vec<(usize, usize)> = emoji_spans(spans, revealed).filter(|(_, id)| shelf.texture(*id).is_some()).map(|(s, _)| (s.start, s.end)).collect();
    if hide.is_empty() {
        return p.layout_job(job);
    }
    // `LayoutJob::append` merges neighbours of the same format, so the emoji usually share a
    // section with the text around them: cut the sections at the emoji edges.
    let mut out = Vec::with_capacity(job.sections.len() + hide.len() * 2);
    for sec in std::mem::take(&mut job.sections) {
        let (a, b) = (sec.byte_range.start.0, sec.byte_range.end.0);
        let mut cuts: Vec<usize> = hide.iter().flat_map(|h| [h.0, h.1]).filter(|c| *c > a && *c < b).collect();
        cuts.extend([a, b]);
        cuts.sort_unstable();
        cuts.dedup();
        for (i, w) in cuts.windows(2).enumerate() {
            let mut piece = sec.clone();
            piece.byte_range = egui::text::ByteIndex(w[0])..egui::text::ByteIndex(w[1]);
            if i > 0 {
                piece.leading_space = 0.0;
            }
            if hide.iter().any(|h| h.0 <= w[0] && w[1] <= h.1) {
                piece.format.color = Color32::TRANSPARENT;
            }
            out.push(piece);
        }
    }
    job.sections = out;
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
            shelf.seen(id);
            p.image(tex, r, Rect::from_min_max(pos2(0.0, 0.0), pos2(1.0, 1.0)), Color32::WHITE);
        }
    }
}
