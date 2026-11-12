//! Fact checks under messages, as in Telegram Desktop: a "Fact check" block under the text with
//! the checkers' clarification and their country; a long one folds to a few lines. Messages the
//! server marks `need_check` ask `messages.getFactCheck` once they are on screen, in batches per
//! chat, and the answer is kept per message for the session.
//!
//! Bubbles are free functions, so they read this state through the egui context, like effects do.

use std::sync::{Arc, Mutex};

use super::*;
use crate::backend::{FACT_CHECK_BATCH, FactCheck};
use eframe::egui::{Galley, Painter};

/// A folded fact check shows this many lines.
const FOLDED_ROWS: usize = 4;
const HEADER_H: f32 = 20.0;
const PAD: f32 = 6.0;

#[derive(Default)]
pub(super) struct FactChecks {
    shelf: Shelf,
}

#[derive(Clone, Default)]
struct Shelf(Arc<Mutex<Board>>);

#[derive(Default)]
struct Board {
    /// Fresh fact checks from `messages.getFactCheck`, over what the message arrived with.
    got: HashMap<(PeerId, i32), FactCheck>,
    /// Asked once per session: the answer, even an empty one, is final.
    asked: HashSet<(PeerId, i32)>,
    /// Shown this frame and still stale.
    wanted: Vec<(PeerId, i32)>,
    /// Unfolded long fact checks.
    open: HashSet<(PeerId, i32)>,
}

fn slot() -> egui::Id {
    egui::Id::unique("factcheck-shelf")
}

fn shelf(ctx: &egui::Context) -> Option<Shelf> {
    ctx.data(|d| d.get_temp::<Shelf>(slot()))
}

/// The fact check laid out for a bubble.
pub(super) struct Block {
    body: Arc<Galley>,
    footer: Arc<Galley>,
    /// "Show more" / "Show less" for a long one.
    toggle: Option<&'static str>,
    pub(super) width: f32,
}

impl Block {
    /// The fact check of `m` with a text, `width` wide at most.
    pub(super) fn layout(p: &Painter, m: &Msg, width: f32) -> Option<Self> {
        let key = (m.peer, m.id);
        let (check, open) = match shelf(p.ctx()).as_ref().and_then(|s| s.0.lock().ok()) {
            Some(b) => (b.got.get(&key).cloned().or_else(|| m.factcheck.clone()), b.open.contains(&key)),
            None => (m.factcheck.clone(), false),
        };
        let check = check.filter(|c| !c.text.is_empty())?;
        let inner = width - 14.0;
        let font = glass::msg_font() - 1.0;
        let job = |rows: usize| {
            let mut job = format::layout(&check.text, &check.spans, font, ink(), glass::link(m.out), inner, true);
            job.wrap.max_rows = rows;
            p.layout_job(job)
        };
        let full = job(usize::MAX);
        let long = full.rows.len() > FOLDED_ROWS + 1;
        let body = if long && !open { job(FOLDED_ROWS) } else { full };
        let toggle = long.then(|| if open { l!("Згорнути", "Show less") } else { l!("Показати більше", "Show more") });
        let note = if check.country.is_empty() {
            l!("Це роз'яснення надали фактчекери.", "This clarification was provided by fact checkers.").to_owned()
        } else {
            let country = check.country.to_uppercase();
            lf!("Це роз'яснення надали фактчекери ({country}).", "This clarification was provided by fact checkers in {country}.")
        };
        let footer = p.layout(note, FontId::proportional(11.5), rgba(ink(), 150), inner);
        let natural = body.size().x.max(footer.size().x).max(150.0) + 14.0;
        Some(Self { body, footer, toggle, width: natural.min(width) })
    }

    pub(super) fn height(&self) -> f32 {
        PAD + HEADER_H + self.body.size().y + 4.0 + self.footer.size().y + PAD
    }

    /// The block at `at`, `width` wide: a quote-like frame with the header, text and the country.
    pub(super) fn draw(self, ui: &Ui, m: &Msg, at: egui::Pos2, width: f32) {
        let p = ui.painter();
        let accent = if m.out { ink() } else { ACCENT };
        let frame = Rect::from_min_size(at, vec2(width, self.height()));
        // A light veil and a stripe, like a quote: no glass on the bubble's glass.
        p.rect_filled(frame, 8.0, rgba(accent, 22));
        glass::stripe(p, Rect::from_min_size(frame.min, vec2(3.0, frame.height())), &[accent]);
        let x = at.x + 10.0;
        let head = pos2(x, at.y + PAD + HEADER_H / 2.0);
        p.text(head, Align2::LEFT_CENTER, l!("Перевірка фактів", "Fact check"), FontId::new(13.0, egui::FontFamily::Name(format::BOLD.into())), accent);
        let body_at = pos2(x, at.y + PAD + HEADER_H);
        let body_h = self.body.size().y;
        p.galley(body_at, self.body, ink());
        p.galley(pos2(x, body_at.y + body_h + 4.0), self.footer, rgba(ink(), 150));
        let Some(label) = self.toggle else { return };
        let r = p.text(pos2(frame.right() - 8.0, head.y), Align2::RIGHT_CENTER, label, FontId::proportional(12.0), glass::link(m.out));
        let resp = ui.interact(r.expand(4.0), ui.scope_id().with(("factcheck", m.id)), Sense::click());
        if resp.hovered() {
            ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
        }
        if resp.clicked()
            && let Some(s) = shelf(ui.ctx())
            && let Ok(mut b) = s.0.lock()
        {
            let key = (m.peer, m.id);
            if !b.open.remove(&key) {
                b.open.insert(key);
            }
        }
    }
}

/// A drawn bubble: a stale fact check on screen asks for its text after this frame.
pub(super) fn want(ui: &Ui, m: &Msg, rect: Rect) {
    if m.id <= 0 || !m.factcheck.as_ref().is_some_and(|c| c.need_check) || !ui.is_rect_visible(rect) {
        return;
    }
    if let Some(s) = shelf(ui.ctx())
        && let Ok(mut b) = s.0.lock()
        && !b.asked.contains(&(m.peer, m.id))
    {
        b.wanted.push((m.peer, m.id));
    }
}

impl App {
    /// After the history is drawn: asks for the fact checks its bubbles wanted.
    pub(super) fn fact_check_frame(&mut self, peer: PeerId) {
        self.ctx.data_mut(|d| d.insert_temp(slot(), self.fact_checks.shelf.clone()));
        let ids = {
            let Ok(mut b) = self.fact_checks.shelf.0.lock() else { return };
            let wanted = std::mem::take(&mut b.wanted);
            let mut ids: Vec<i32> = wanted.into_iter().filter(|k| k.0 == peer && b.asked.insert(*k)).map(|k| k.1).collect();
            ids.sort_unstable();
            ids
        };
        // The lock is gone: the demo answers inside `send`.
        for batch in ids.chunks(FACT_CHECK_BATCH) {
            self.send(Cmd::LoadFactChecks { peer, ids: batch.to_vec() });
        }
    }

    pub(super) fn on_fact_checks(&mut self, peer: PeerId, checks: Vec<(i32, FactCheck)>) {
        if let Ok(mut b) = self.fact_checks.shelf.0.lock() {
            b.got.extend(checks.into_iter().map(|(id, c)| ((peer, id), c)));
        }
        self.ctx.request_repaint();
    }
}
