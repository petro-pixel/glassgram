//! Voice transcription and AI summaries of long posts, as in Telegram Desktop. The "→A" button by
//! a voice message's waveform (or beside a round video) asks for the text and shows it under the
//! media, a second click folds it; long posts carry a "Summary" capsule that puts the AI summary
//! in place of the text, and its header goes back to the original.
//!
//! Bubbles are free functions, so they read this state through the egui context, like effects do.

use std::sync::{Arc, Mutex};

use super::*;
use crate::backend::Transcript;
use eframe::egui::{Galley, Painter};

/// Height of the "Summary" capsule and of the summary block's header.
const CAPSULE_H: f32 = 24.0;
const HEADER_H: f32 = 26.0;
/// The demo has no live updates: a pending transcription asks again after this long.
const REASK_SECS: f64 = 1.2;
/// A live transcription still pending after this long may have missed its update: a click asks
/// again (the server just answers with where it is).
const STALE_SECS: f64 = 10.0;

#[derive(Default)]
pub(super) struct AiText {
    shelf: Shelf,
}

#[derive(Clone, Default)]
struct Shelf(Arc<Mutex<Board>>);

#[derive(Default)]
struct Board {
    heard: HashMap<(PeerId, i32), Heard>,
    briefs: HashMap<(PeerId, i32), Summary>,
    /// Demo: pending transcriptions ask again instead of waiting for `updateTranscribedAudio`.
    reask: bool,
}

/// The transcription of one voice or round video message.
struct Heard {
    pending: bool,
    text: String,
    /// Unfolded under the media; a second click on "→A" folds it.
    shown: bool,
    /// When it was asked for or last answered.
    since: f64,
}

enum Summary {
    Loading,
    Ready { text: String, spans: Vec<format::Span>, shown: bool },
}

fn slot() -> egui::Id {
    egui::Id::unique("aitext-shelf")
}

fn with_board<R>(ctx: &egui::Context, f: impl FnOnce(&Board) -> R) -> Option<R> {
    let shelf = ctx.data(|d| d.get_temp::<Shelf>(slot()))?;
    let b = shelf.0.lock().ok()?;
    Some(f(&b))
}

/// Voice and round video messages from the server can be transcribed.
fn can_transcribe(m: &Msg) -> bool {
    m.id > 0 && m.media.as_ref().is_some_and(|i| matches!(i.kind, MediaKind::Voice | MediaKind::Round))
}

/// Room a voice message's card keeps for "→A" right of the waveform.
pub(super) fn button_w(m: &Msg) -> f32 {
    if can_transcribe(m) && m.media.as_ref().is_some_and(|i| i.kind == MediaKind::Voice) { 34.0 } else { 0.0 }
}

/// The "→A" button. `glassy`: it floats over the wallpaper (round videos), not inside a bubble.
pub(super) fn voice_button(ui: &Ui, m: &Msg, r: Rect, glassy: bool) -> Option<Action> {
    if !can_transcribe(m) {
        return None;
    }
    let now = ui.input(|i| i.time);
    let (pending, shown, reask) = with_board(ui.ctx(), |b| match b.heard.get(&(m.peer, m.id)) {
        Some(h) => (h.pending, h.shown, h.pending && b.reask && now - h.since > REASK_SECS),
        None => (false, false, false),
    })
    .unwrap_or_default();
    let p = ui.painter();
    let resp = ui.interact(r, ui.scope_id().with(("transcribe", m.id)), Sense::click());
    if glassy {
        glass::glass(p, r, Glass::tinted(r.height() / 2.0, glass::chrome(), if resp.hovered() { 170 } else { 120 }).frosted(1.0));
    } else {
        p.rect_filled(r, r.height() / 2.0, glass::veil(if resp.hovered() { 45 } else { 25 }));
    }
    if pending {
        bubbles::spinner(p, r.center(), 6.0);
        ui.ctx().request_repaint();
    } else if shown {
        icons::draw(p, Rect::from_center_size(r.center(), vec2(14.0, 14.0)), Icon::ChevronUp, ink());
    } else {
        p.text(r.center(), Align2::CENTER_CENTER, "→A", FontId::new(12.5, egui::FontFamily::Name(format::BOLD.into())), ink());
    }
    if resp.hovered() {
        ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
    }
    let tip = if shown { l!("Згорнути текст", "Hide text") } else { l!("Розшифрувати голосове", "Transcribe voice message") };
    (resp.on_hover_text(tip).clicked() || reask).then_some(Action::Transcribe(m.id))
}

/// The unfolded text of a transcription, laid out `width` wide; a partial one ends with "…".
pub(super) fn transcript(p: &Painter, m: &Msg, width: f32) -> Option<Arc<Galley>> {
    if !can_transcribe(m) {
        return None;
    }
    let text = with_board(p.ctx(), |b| {
        b.heard.get(&(m.peer, m.id)).filter(|h| h.shown && !h.text.is_empty()).map(|h| if h.pending { format!("{}…", h.text) } else { h.text.clone() })
    })??;
    Some(p.layout(text, FontId::proportional(glass::msg_font() - 1.0), ink(), width))
}

/// What a long post shows for its summary.
pub(super) enum Brief {
    /// The "Summary" capsule above the text, spinning while it loads.
    Offer { loading: bool },
    /// The summary in place of the text.
    Shown(String, Vec<format::Span>),
}

/// A long post the server can summarize.
pub(super) fn brief(ctx: &egui::Context, m: &Msg) -> Option<Brief> {
    if m.id <= 0 || m.summary_lang.is_none() || m.text.is_empty() {
        return None;
    }
    let state = with_board(ctx, |b| match b.briefs.get(&(m.peer, m.id)) {
        Some(Summary::Loading) => Brief::Offer { loading: true },
        Some(Summary::Ready { text, spans, shown: true }) => Brief::Shown(text.clone(), spans.clone()),
        _ => Brief::Offer { loading: false },
    });
    Some(state.unwrap_or(Brief::Offer { loading: false }))
}

impl Brief {
    /// Space it takes above the text.
    pub(super) fn head_h(&self) -> f32 {
        match self {
            Brief::Offer { .. } => CAPSULE_H + 6.0,
            Brief::Shown(..) => HEADER_H,
        }
    }

    /// The summary to lay out instead of the post's text.
    pub(super) fn text(&self) -> Option<(&str, &[format::Span])> {
        match self {
            Brief::Shown(text, spans) => Some((text, spans)),
            Brief::Offer { .. } => None,
        }
    }

    /// The capsule, or the summary block's frame and header over `body_h` of text, `width` wide.
    pub(super) fn draw(&self, ui: &Ui, m: &Msg, at: egui::Pos2, width: f32, body_h: f32) -> Option<Action> {
        let p = ui.painter();
        let accent = if m.out { ink() } else { ACCENT };
        let label = FontId::new(12.5, egui::FontFamily::Name(format::BOLD.into()));
        let (hit, tip) = match self {
            Brief::Offer { loading } => {
                let g = p.layout_no_wrap(l!("Підсумок", "Summary").to_owned(), label, accent);
                let r = Rect::from_min_size(at, vec2(g.size().x + 40.0, CAPSULE_H));
                let hovered = ui.rect_contains_pointer(r);
                p.rect_filled(r, CAPSULE_H / 2.0, rgba(accent, if hovered { 50 } else { 28 }));
                let icon = Rect::from_center_size(pos2(r.left() + 16.0, r.center().y), vec2(14.0, 14.0));
                if *loading {
                    bubbles::spinner(p, icon.center(), 5.5);
                    ui.ctx().request_repaint();
                } else {
                    icons::draw(p, icon, Icon::Sparkles, accent);
                }
                p.galley(pos2(r.left() + 28.0, r.center().y - g.size().y / 2.0), g, accent);
                (r, l!("Короткий підсумок від ШІ", "Short AI summary"))
            }
            Brief::Shown(..) => {
                // A quote-like block: a light veil and a stripe, not glass on the bubble's glass.
                let frame = Rect::from_min_size(at - vec2(8.0, 2.0), vec2(width + 14.0, HEADER_H + body_h + 6.0));
                p.rect_filled(frame, 10.0, rgba(accent, 22));
                glass::stripe(p, Rect::from_min_size(frame.min, vec2(3.0, frame.height())), &[accent]);
                icons::draw(p, Rect::from_min_size(at + vec2(0.0, 3.0), vec2(14.0, 14.0)), Icon::Sparkles, accent);
                p.text(at + vec2(20.0, 10.0), Align2::LEFT_CENTER, l!("Підсумок від ШІ", "AI summary"), label, accent);
                let back = p.text(pos2(at.x + width, at.y + 10.0), Align2::RIGHT_CENTER, l!("Показати оригінал", "Show original"), FontId::proportional(12.0), glass::link(m.out));
                (Rect::from_min_max(at, pos2(at.x + width, at.y + HEADER_H)).union(back), l!("Повернути повний текст", "Back to the full text"))
            }
        };
        let resp = ui.interact(hit, ui.scope_id().with(("summary", m.id)), Sense::click());
        if resp.hovered() {
            ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
        }
        resp.on_hover_text(tip).clicked().then_some(Action::Summary(m.id))
    }
}

impl App {
    fn ai_board(&self) -> Option<std::sync::MutexGuard<'_, Board>> {
        self.ctx.data_mut(|d| d.insert_temp(slot(), self.ai_text.shelf.clone()));
        self.ai_text.shelf.0.lock().ok()
    }

    /// "→A": asks for the text, then folds and unfolds it.
    pub(super) fn transcribe(&mut self, peer: PeerId, id: i32) {
        let now = self.ctx.input(|i| i.time);
        let demo = self.demo;
        let ask = {
            let Some(mut b) = self.ai_board() else { return };
            b.reask = demo;
            match b.heard.get_mut(&(peer, id)) {
                // The live update normally finishes it; a click asks again only if it seems lost.
                Some(h) if h.pending => {
                    let again = demo || now - h.since > STALE_SECS;
                    if again {
                        h.since = now;
                    }
                    again
                }
                Some(h) => {
                    h.shown = !h.shown;
                    false
                }
                None => {
                    b.heard.insert((peer, id), Heard { pending: true, text: String::new(), shown: true, since: now });
                    true
                }
            }
        };
        // The lock is gone: the demo answers inside `send`.
        if ask {
            self.send(Cmd::Transcribe { peer, id });
        }
    }

    pub(super) fn on_transcribed(&mut self, peer: PeerId, id: i32, result: Result<Transcript, String>) {
        let now = self.ctx.input(|i| i.time);
        let t = match result {
            Ok(t) => t,
            Err(e) => {
                if let Some(mut b) = self.ai_board() {
                    b.heard.remove(&(peer, id));
                }
                self.toast = Some((e, 0.0, true));
                return;
            }
        };
        if let Some(mut b) = self.ai_board()
            // Updates for messages we never asked about (another device did) are not shown.
            && let Some(h) = b.heard.get_mut(&(peer, id))
        {
            h.pending = t.pending;
            if !t.text.is_empty() || !t.pending {
                h.text = t.text;
            }
            h.since = now;
        }
        if let Some((left, until)) = t.trial {
            self.toast = Some((trial_note(left, until), 0.0, false));
        }
    }

    /// "Summary" asks for it once, then switches between it and the original.
    pub(super) fn summary(&mut self, peer: PeerId, id: i32) {
        let ask = {
            let Some(mut b) = self.ai_board() else { return };
            match b.briefs.get_mut(&(peer, id)) {
                Some(Summary::Loading) => false,
                Some(Summary::Ready { shown, .. }) => {
                    *shown = !*shown;
                    false
                }
                None => {
                    b.briefs.insert((peer, id), Summary::Loading);
                    true
                }
            }
        };
        if ask {
            self.send(Cmd::Summarize { peer, id });
        }
    }

    pub(super) fn on_summarized(&mut self, peer: PeerId, id: i32, result: Result<(String, Vec<format::Span>), String>) {
        let Some(mut b) = self.ai_board() else { return };
        match result {
            Ok((text, spans)) => {
                b.briefs.insert((peer, id), Summary::Ready { text, spans, shown: true });
            }
            Err(e) => {
                b.briefs.remove(&(peer, id));
                drop(b);
                self.toast = Some((e, 0.0, true));
            }
        }
    }

    /// Demo: the partial text came, so the next ask gets the whole one.
    pub fn demo_transcribing(&self, peer: PeerId, id: i32) -> bool {
        self.ai_text.shelf.0.lock().is_ok_and(|b| b.heard.get(&(peer, id)).is_some_and(|h| h.pending && !h.text.is_empty()))
    }
}

/// Without Premium the server counts free transcriptions: how many are left, or when more come.
fn trial_note(left: i32, until: i64) -> String {
    if left > 0 || until <= 0 {
        return lp!(
            left,
            ["Залишилася {n} безкоштовна розшифровка", "Залишилося {n} безкоштовні розшифровки", "Залишилося {n} безкоштовних розшифровок"],
            ["{n} free transcription left", "{n} free transcriptions left"]
        );
    }
    let when = jiff::Timestamp::from_second(until).map(|t| t.to_zoned(jiff::tz::TimeZone::system()).strftime("%d.%m %H:%M").to_string()).unwrap_or_default();
    lf!("Безкоштовні розшифровки скінчилися до {when}", "No free transcriptions left until {when}")
}
