//! Motion: Apple-style springs for transitions, driven by egui's frame time. With "Reduce motion"
//! every spring snaps to its target at once.

use eframe::egui::{self, Id, Rect, Ui, emath::TSTransform, layers::ShapeIdx, pos2, vec2};
use grammers_client::session::types::PeerId;

use crate::glass;

/// A damped spring: `response` is the period of the undamped motion in seconds, `damping` the
/// damping ratio (1 — no overshoot, below 1 — a slight bounce), as in SwiftUI's `.spring`.
#[derive(Clone, Copy)]
pub(super) struct Spring {
    response: f32,
    damping: f32,
}

impl Spring {
    /// Panels and columns: no overshoot, so nothing peeks past an edge (~300 ms).
    pub(super) const SMOOTH: Spring = Spring { response: 0.32, damping: 1.0 };
    /// Small things that move on their own: a hint of bounce (~300 ms).
    pub(super) const SNAPPY: Spring = Spring { response: 0.3, damping: 0.78 };

    fn omega(self) -> f32 {
        std::f32::consts::TAU / self.response
    }

    /// Step response from 0 to 1 at `t` seconds; exactly 1 once settled.
    pub(super) fn curve(self, t: f32) -> f32 {
        if t <= 0.0 {
            return 0.0;
        }
        let (w, z) = (self.omega(), self.damping);
        let x = if z >= 1.0 {
            1.0 - (1.0 + w * t) * (-w * t).exp()
        } else {
            let wd = w * (1.0 - z * z).sqrt();
            1.0 - (-z * w * t).exp() * ((wd * t).cos() + z * w / wd * (wd * t).sin())
        };
        if t >= self.settle() { 1.0 } else { x }
    }

    /// When the motion is within about 1% of rest.
    pub(super) fn settle(self) -> f32 {
        if self.damping >= 1.0 { 6.6 / self.omega() } else { 5.0 / (self.damping * self.omega()) }
    }
}

/// Progress 0 → 1 (possibly a little past 1 on a bouncy spring) of a motion that began at
/// `start` (egui time); keeps frames coming while it runs.
pub(super) fn since(ctx: &egui::Context, start: f64, spring: Spring) -> f32 {
    if glass::calm() {
        return 1.0;
    }
    let t = (ctx.input(|i| i.time) - start) as f32;
    if t < spring.settle() {
        ctx.request_repaint();
    }
    spring.curve(t)
}

/// A value that follows `target` on a spring, keeping its velocity when the target moves mid-way.
/// The state lives in egui's temp data under `id`; `eps` is how close counts as arrived.
pub(super) fn follow(ctx: &egui::Context, id: Id, target: f32, eps: f32, spring: Spring) -> f32 {
    let (mut x, mut v) = ctx.data_mut(|d| *d.get_temp_mut_or_insert_with(id, || (target, 0.0f32)));
    if glass::calm() || !x.is_finite() {
        (x, v) = (target, 0.0);
    } else if x != target || v != 0.0 {
        let (w, z) = (spring.omega(), spring.damping);
        // Fixed small steps keep the integration stable on a slow frame.
        let mut left = ctx.input(|i| i.stable_dt).min(0.1);
        while left > 0.0 {
            let dt = left.min(1.0 / 240.0);
            v += (-w * w * (x - target) - 2.0 * z * w * v) * dt;
            x += v * dt;
            left -= dt;
        }
        if (x - target).abs() < eps && v.abs() < eps * 10.0 {
            (x, v) = (target, 0.0);
        } else {
            ctx.request_repaint();
        }
    }
    ctx.data_mut(|d| d.insert_temp(id, (x, v)));
    x
}

/// How long a new bubble keeps its entry animation on record.
const APPEAR_KEEP: f64 = 1.0;

/// Transitions that need memory beyond one frame.
#[derive(Default)]
pub(super) struct Motion {
    /// Messages that arrived while their chat was open: (chat, id, egui time).
    appear: Vec<(PeerId, i32, f64)>,
    /// Our message just left the send queue: its server copy replaces a bubble already shown.
    quiet_out: Option<(PeerId, f64)>,
    /// The folder the list shows, the direction of the last switch (−1 left, 1 right) and its start.
    pub(super) folder: Option<Option<i32>>,
    pub(super) folder_dir: f32,
    pub(super) folder_at: f64,
}

impl Motion {
    /// A message was added at the bottom of an open chat; `out` ones replacing a queued bubble are skipped.
    pub(super) fn arrived(&mut self, peer: PeerId, id: i32, out: bool, queued: bool, now: f64) {
        self.appear.retain(|a| now - a.2 < APPEAR_KEEP);
        let echo = out && id > 0 && (queued || self.quiet_out.is_some_and(|(p, t)| p == peer && now - t < 3.0));
        if !echo {
            self.appear.push((peer, id, now));
        } else if let Some(a) = self.appear.iter_mut().rev().find(|a| a.0 == peer && a.1 < 0) {
            // The server copy takes over the queued bubble's entry, which may still be running.
            a.1 = id;
        }
    }

    pub(super) fn sent(&mut self, peer: PeerId, now: f64) {
        self.quiet_out = Some((peer, now));
    }

    /// When the message's entry began, while it is still animating.
    pub(super) fn appearing(&self, peer: PeerId, id: i32, now: f64) -> Option<f64> {
        if glass::calm() {
            return None;
        }
        let settle = Spring::SNAPPY.settle() as f64;
        self.appear.iter().find(|a| a.0 == peer && a.1 == id && now - a.2 < settle).map(|a| a.2)
    }
}

/// A bubble being drawn with its entry animation.
pub(super) struct Entry {
    first: ShapeIdx,
    opacity: f32,
    t: f32,
}

/// Starts a new bubble's entry: what is painted until `entry_end` fades in.
pub(super) fn entry_begin(ui: &mut Ui, start: f64) -> Entry {
    let t = since(ui.ctx(), start, Spring::SNAPPY);
    let first = ui.ctx().graphics_mut(|g| g.entry(ui.layer_id()).next_idx());
    let opacity = ui.opacity();
    ui.multiply_opacity((t * 1.6).clamp(0.0, 1.0));
    Entry { first, opacity, t }
}

/// Rises and grows the bubble from its tail corner, as in Telegram, on a bouncy spring.
pub(super) fn entry_end(ui: &mut Ui, e: Entry, row: Rect, out: bool) {
    ui.set_opacity(e.opacity);
    let s = 0.9 + 0.1 * e.t;
    let anchor = pos2(if out { row.right() } else { row.left() }, row.bottom());
    let lift = vec2(0.0, (1.0 - e.t) * 14.0);
    let tf = TSTransform::new(anchor.to_vec2() * (1.0 - s) + lift, s);
    ui.ctx().graphics_mut(|g| {
        let list = g.entry(ui.layer_id());
        let end = list.next_idx();
        list.transform_range(e.first, end, tf);
        // The GPU blur keeps the rect it was made with and ignores opacity: leave it out until the bubble lands.
        for k in e.first.0..end.0 {
            list.mutate_shape(ShapeIdx(k), |c| {
                if matches!(c.shape, egui::Shape::Callback(_)) {
                    c.shape = egui::Shape::Noop;
                }
            });
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn springs_start_at_zero_and_settle_at_one() {
        for s in [Spring::SMOOTH, Spring::SNAPPY] {
            assert_eq!(s.curve(0.0), 0.0);
            assert_eq!(s.curve(s.settle()), 1.0);
            assert!((s.curve(s.settle() - 0.001) - 1.0).abs() < 0.02);
            // Within the design range of 150–350 ms.
            assert!((0.15..=0.4).contains(&s.settle()), "{}", s.settle());
        }
        // The smooth spring never overshoots.
        assert!((0..100).all(|k| Spring::SMOOTH.curve(k as f32 * 0.005) <= 1.0));
    }
}
