//! Message effects, as in Telegram Desktop: a message sent with one shows the effect's emoji by
//! its time and plays the effect's animation over the chat once when it is first shown; a click
//! on the emoji plays it again. The send button's menu offers effects in private chats.
//!
//! Bubbles are free functions, so they reach the effect emoji and report where they drew the
//! badge through the egui context, like custom emoji do.

use std::sync::mpsc::{Receiver, TryRecvError, sync_channel};
use std::sync::{Arc, Mutex};

use super::*;
use crate::backend::Effect;
use eframe::egui::{ColorImage, Galley, Painter};

/// Frames per second of a played effect, and the longest one drawn (Telegram's run 2–3 s).
const FPS: f64 = 30.0;
const MAX_SECS: f64 = 4.0;
/// Side of the animation in points, and its largest size in pixels.
const SIDE_PT: f32 = 240.0;
const SIDE_PX: f32 = 480.0;
/// A message newer than this plays its effect when first shown, like one that just came.
const FRESH_SECS: i64 = 600;
/// Effects played at once.
const MAX_PLAYING: usize = 3;

#[derive(Default)]
pub(super) struct Effects {
    list: Vec<Effect>,
    /// Animation files by effect id.
    files: HashMap<i64, PathBuf>,
    asked: bool,
    /// Messages whose effect already played (or was skipped) this session.
    played: HashSet<(PeerId, i32)>,
    /// A badge was clicked: play that message's effect again.
    replay: Option<(PeerId, i32)>,
    playing: Vec<Play>,
    shelf: Shelf,
}

/// What bubbles see: the emoji of each effect, and where badges were drawn this frame.
#[derive(Clone, Default)]
struct Shelf(Arc<Mutex<Board>>);

#[derive(Default)]
struct Board {
    emoji: HashMap<i64, String>,
    drawn: Vec<Drawn>,
    /// A message has an effect we don't know yet.
    wanted: bool,
}

#[derive(Clone, Copy)]
struct Drawn {
    peer: PeerId,
    id: i32,
    effect: i64,
    badge: Rect,
    out: bool,
}

fn slot() -> egui::Id {
    egui::Id::unique("effects-shelf")
}

fn shelf(ctx: &egui::Context) -> Option<Shelf> {
    ctx.data(|d| d.get_temp::<Shelf>(slot()))
}

/// The emoji of the message's effect, once the effect list is known.
pub(super) fn badge(ctx: &egui::Context, m: &Msg) -> Option<String> {
    let id = m.effect?;
    let s = shelf(ctx)?;
    let mut b = s.0.lock().ok()?;
    let emoji = b.emoji.get(&id).cloned();
    if emoji.is_none() {
        b.wanted = true;
    }
    emoji
}

/// The time line of a bubble with the effect emoji in front, a little larger than the time.
/// Returns the galley and the width the emoji takes.
pub(super) fn meta(p: &Painter, badge: Option<&str>, text: &str, color: Color32) -> (Arc<Galley>, f32) {
    let Some(emoji) = badge else { return (p.layout_no_wrap(text.to_owned(), FontId::proportional(11.0), color), 0.0) };
    let w = p.layout_no_wrap(format!("{emoji} "), FontId::proportional(13.0), color).size().x;
    let mut job = LayoutJob::default();
    job.append(&format!("{emoji} "), 0.0, egui::TextFormat { font_id: FontId::proportional(13.0), color, valign: Align::Center, ..Default::default() });
    job.append(text, 0.0, egui::TextFormat { font_id: FontId::proportional(11.0), color, valign: Align::Center, ..Default::default() });
    (p.layout_job(job), w)
}

/// A bubble drew the effect badge of `m` at `badge`.
pub(super) fn drawn(ctx: &egui::Context, m: &Msg, badge: Rect) {
    let (Some(effect), Some(s)) = (m.effect, shelf(ctx)) else { return };
    if let Ok(mut b) = s.0.lock() {
        b.drawn.push(Drawn { peer: m.peer, id: m.id, effect, badge, out: m.out });
    }
}

/// The effects row on top of the send button's menu; premium ones carry a star.
pub(super) fn strip(ui: &mut Ui, list: &[Effect], premium: bool) -> Option<i64> {
    const CELL: f32 = 34.0;
    let mut picked = None;
    ui.label(egui::RichText::new(l!("Надіслати з ефектом", "Send with effect")).color(ink_dim()).size(12.5));
    let per_row = 6;
    for row in list.chunks(per_row) {
        let (r, _) = ui.allocate_exact_size(vec2(CELL * per_row as f32, CELL), Sense::hover());
        for (k, e) in row.iter().enumerate() {
            let cell = Rect::from_min_size(r.min + vec2(k as f32 * CELL, 0.0), vec2(CELL, CELL));
            let resp = ui.interact(cell, ui.scope_id().with(("effect", e.id)), Sense::click());
            if resp.hovered() {
                ui.painter().rect_filled(cell.shrink(1.0), CELL / 2.0, glass::veil(24));
            }
            ui.painter().text(cell.center(), Align2::CENTER_CENTER, &e.emoji, FontId::proportional(20.0), ink());
            if e.premium && !premium {
                let star = Rect::from_center_size(cell.right_bottom() - vec2(8.0, 8.0), vec2(13.0, 13.0));
                ui.painter().circle_filled(star.center(), 6.5, glass::chrome());
                icons::draw(ui.painter(), star.shrink(2.0), Icon::Star, ink());
            }
            let resp = if e.premium && !premium { resp.on_hover_text(l!("Ефект для Telegram Premium", "A Telegram Premium effect")) } else { resp };
            if resp.clicked() {
                picked = Some(e.id);
            }
        }
    }
    picked
}

/// One effect being played: a thread draws its frames in order, the UI shows each on time.
struct Play {
    peer: PeerId,
    id: i32,
    rx: Receiver<(usize, ColorImage)>,
    /// A frame that came early, waiting for its time.
    next: Option<(usize, ColorImage)>,
    tex: Option<TextureHandle>,
    /// When the first frame arrived.
    started: Option<f64>,
    done: bool,
}

impl Play {
    fn start(peer: PeerId, id: i32, path: PathBuf, side: usize) -> Self {
        let (tx, rx) = sync_channel(2);
        std::thread::spawn(move || {
            let Ok(data) = std::fs::read(&path) else { return };
            let Ok(mut anim) = crate::lottie::Lottie::load(&data, side as u16) else { return };
            let count = ((anim.duration().min(MAX_SECS) * FPS).ceil() as usize).max(1);
            for k in 0..count {
                let img = ColorImage::from_rgba_unmultiplied([side, side], &anim.frame(k as f64 / FPS));
                // The UI stopped the effect.
                if tx.send((k, img)).is_err() {
                    return;
                }
            }
        });
        Self { peer, id, rx, next: None, tex: None, started: None, done: false }
    }

    fn advance(&mut self, ctx: &egui::Context, now: f64) {
        loop {
            if self.next.is_none() {
                match self.rx.try_recv() {
                    Ok(f) => self.next = Some(f),
                    Err(TryRecvError::Empty) => return,
                    Err(TryRecvError::Disconnected) => {
                        self.done = true;
                        return;
                    }
                }
            }
            let started = *self.started.get_or_insert(now);
            let due = ((now - started) * FPS) as usize;
            match self.next.take() {
                Some((k, img)) if k <= due => match &mut self.tex {
                    Some(t) => t.set(img, TextureOptions::LINEAR),
                    None => self.tex = Some(ctx.load_texture(format!("effect-{:?}-{}", self.peer, self.id), img, TextureOptions::LINEAR)),
                },
                early => {
                    self.next = early;
                    return;
                }
            }
        }
    }
}

impl App {
    pub(super) fn effects_event(&mut self, ev: Ev) {
        match ev {
            Ev::Effects(list) => {
                if let Ok(mut b) = self.effects.shelf.0.lock() {
                    b.emoji = list.iter().map(|e| (e.id, e.emoji.clone())).collect();
                }
                self.effects.list = list;
            }
            Ev::EffectFile { id, path } => {
                self.effects.files.insert(id, path);
            }
            _ => {}
        }
    }

    /// Asks for the effect list once: when the send menu opens or a message needs its badge.
    pub(super) fn want_effects(&mut self) {
        if !self.effects.asked {
            self.effects.asked = true;
            self.send(Cmd::LoadEffects);
        }
    }

    /// Effects for the send button's menu: in private chats with someone else, as in Telegram Desktop.
    pub(super) fn send_effects(&mut self, peer: PeerId) -> Vec<Effect> {
        if !self.chats.iter().any(|c| c.id == peer && c.kind == ChatKind::User) {
            return Vec::new();
        }
        self.want_effects();
        self.effects.list.clone()
    }

    /// The draft goes with the effect picked in the send menu.
    pub(super) fn send_with_effect(&mut self, effect: i64) {
        let premium = self.effects.list.iter().any(|e| e.id == effect && e.premium);
        if premium && !self.profile.premium {
            self.toast = Some((l!("Цей ефект доступний з Telegram Premium", "This effect needs Telegram Premium").into(), 0.0, true));
            return;
        }
        self.send_draft_with(SendOpts { effect: Some(effect), ..Default::default() });
    }

    /// A click on the badge of message `id`.
    pub(super) fn replay_effect(&mut self, peer: PeerId, id: i32) {
        self.effects.replay = Some((peer, id));
    }

    /// After the history is drawn: starts the effects of messages shown for the first time (or
    /// clicked) and paints the ones playing over the chat, clipped to it.
    pub(super) fn effects_frame(&mut self, ui: &Ui, peer: PeerId, clip: Rect, now: f64) {
        let ctx = ui.ctx().clone();
        ctx.data_mut(|d| d.insert_temp(slot(), self.effects.shelf.clone()));
        let (drawn, wanted) = match self.effects.shelf.0.lock() {
            Ok(mut b) => (std::mem::take(&mut b.drawn), std::mem::take(&mut b.wanted)),
            Err(_) => return,
        };
        if wanted {
            self.want_effects();
        }
        let unix = jiff::Timestamp::now().as_second();
        let read_in = self.chats.iter().find(|c| c.id == peer).map_or(0, |c| c.read_in);
        let side = (SIDE_PT * ctx.pixels_per_point()).min(SIDE_PX) as usize;
        let replay = self.effects.replay.take();
        for d in drawn.iter().filter(|d| d.peer == peer) {
            let first = self.effects.played.insert((peer, d.id));
            let clicked = replay == Some((peer, d.id));
            // Old messages keep still; new and unread ones play, as when they arrive.
            let fresh = self.find(peer, d.id).is_some_and(|m| unix - m.ts < FRESH_SECS || (!m.out && m.id > read_in));
            if !(clicked || (first && fresh && !self.settings.reduce_motion)) {
                continue;
            }
            let Some(path) = self.effects.files.get(&d.effect).cloned() else {
                // Not here yet: a later frame tries again.
                if !clicked {
                    self.effects.played.remove(&(peer, d.id));
                }
                self.want_effects();
                continue;
            };
            self.effects.playing.retain(|p| (p.peer, p.id) != (peer, d.id));
            if self.effects.playing.len() >= MAX_PLAYING {
                self.effects.playing.remove(0);
            }
            self.effects.playing.push(Play::start(peer, d.id, path, side));
        }
        let p = ui.painter().with_clip_rect(clip);
        for play in &mut self.effects.playing {
            play.advance(&ctx, now);
            let (Some(tex), Some(d)) = (&play.tex, drawn.iter().find(|d| (d.peer, d.id) == (play.peer, play.id))) else { continue };
            // The animation flies out from the badge across the chat: leftwards from our messages
            // on the right, mirrored for incoming ones on the left.
            let s = SIDE_PT;
            let c = d.badge.center();
            let (r, uv) = if d.out {
                (Rect::from_min_size(pos2(c.x - s * 0.8, c.y - s * 0.5), vec2(s, s)), Rect::from_min_max(pos2(0.0, 0.0), pos2(1.0, 1.0)))
            } else {
                (Rect::from_min_size(pos2(c.x - s * 0.2, c.y - s * 0.5), vec2(s, s)), Rect::from_min_max(pos2(1.0, 0.0), pos2(0.0, 1.0)))
            };
            p.image(tex.id(), r, uv, Color32::WHITE);
        }
        self.effects.playing.retain(|p| !(p.done && p.next.is_none()));
        if !self.effects.playing.is_empty() {
            ctx.request_repaint_after(Duration::from_secs_f64(1.0 / FPS));
        }
    }
}
