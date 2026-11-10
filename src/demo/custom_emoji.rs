//! Demo custom emoji: a message whose emoji carry custom emoji entities, and small drawn
//! pictures for them, answered like `messages.getCustomEmojiDocuments` would.

use eframe::egui::{Color32, ColorImage};

use crate::app::App;
use crate::backend::{Ev, ImgKey, Msg};
use crate::format::{Span, Style};

/// (document id, fallback emoji).
const EMOJI: [(i64, &str); 4] = [(9001, "💎"), (9002, "❤"), (9003, "⭐"), (9004, "🔮")];

/// A message with every demo custom emoji in its text.
pub(super) fn message(base: Msg) -> Msg {
    let text = "New emoji for the theme 💎 ❤ ⭐ and a crystal ball 🔮".to_owned();
    let spans = EMOJI
        .iter()
        .filter_map(|(id, e)| {
            let start = text.find(e)?;
            Some(Span { start, end: start + e.len(), style: Style::CustomEmoji, target: Some(id.to_string()) })
        })
        .collect();
    Msg { id: 77, text, spans, ..base }
}

pub(super) fn load(app: &mut App, ids: Vec<i64>) {
    for id in ids {
        if let Some(image) = picture(id) {
            app.push_event(Ev::Image { key: ImgKey::Emoji(id), image });
        }
    }
}

/// A shape in a [-1, 1] square (y down) with its colour at that point.
fn picture(id: i64) -> Option<ColorImage> {
    let shade: fn(f32, f32) -> Option<[f32; 3]> = match id {
        // A cut gem: a flat top and a point below, facets in two tones.
        9001 => |x, y| {
            let inside = if y < -0.25 { y > -0.75 && x.abs() < 0.55 + (y + 0.75) * 0.7 } else { x.abs() / 0.9 + (y + 0.25) / 1.15 < 1.0 };
            inside.then(|| if (x * 3.0).floor() as i32 % 2 == 0 { [0.35, 0.85, 1.0] } else { [0.15, 0.55, 0.95] })
        },
        // The classic heart curve, lighter at the top.
        9002 => |x, y| {
            let (x, y) = (x * 1.25, -y * 1.25 + 0.15);
            ((x * x + y * y - 1.0).powi(3) - x * x * y.powi(3) < 0.0).then(|| [1.0, 0.25 + 0.2 * y.max(0.0), 0.4])
        },
        // A five-pointed star.
        9003 => |x, y| {
            let a = x.atan2(-y).rem_euclid(std::f32::consts::TAU / 5.0) / (std::f32::consts::TAU / 5.0);
            // 0 towards a point, 1 halfway between two.
            let t = a.min(1.0 - a) * 2.0;
            let r = 0.95 - 0.55 * t;
            ((x * x + y * y).sqrt() < r).then(|| [1.0, 0.78 - 0.2 * y, 0.2])
        },
        // A glass orb with a highlight.
        9004 => |x, y| {
            let d = (x * x + y * y).sqrt();
            let glow = (1.0 - ((x + 0.35).powi(2) + (y + 0.35).powi(2)).sqrt() * 2.5).max(0.0);
            (d < 0.9).then(|| [0.55 + 0.45 * glow, 0.35 + 0.5 * glow + 0.2 * d, 0.95])
        },
        _ => return None,
    };
    let side = 96;
    let mut img = ColorImage::filled([side, side], Color32::TRANSPARENT);
    // 3×3 samples per pixel smooth the edges.
    for py in 0..side {
        for px in 0..side {
            let (mut rgb, mut hits) = ([0.0f32; 3], 0);
            for sy in 0..3 {
                for sx in 0..3 {
                    let x = ((px * 3 + sx) as f32 + 0.5) / (side * 3) as f32 * 2.0 - 1.0;
                    let y = ((py * 3 + sy) as f32 + 0.5) / (side * 3) as f32 * 2.0 - 1.0;
                    if let Some(c) = shade(x, y) {
                        hits += 1;
                        (0..3).for_each(|k| rgb[k] += c[k]);
                    }
                }
            }
            if hits > 0 {
                let ch = |k: usize| (rgb[k] / hits as f32 * 255.0).clamp(0.0, 255.0) as u8;
                img.pixels[py * side + px] = Color32::from_rgba_unmultiplied(ch(0), ch(1), ch(2), (hits * 255 / 9) as u8);
            }
        }
    }
    Some(img)
}
