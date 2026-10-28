//! Photos and videos sent under a spoiler: a blurred copy of the picture with drifting dots on
//! top, until a click reveals it (the same `revealed` set as text spoilers).

use super::*;
use eframe::egui::ColorImage;

/// Long side of the blurred copy: scaled up to the bubble it reads as a heavy blur.
const BLUR_PX: usize = 24;

#[derive(Default)]
pub(super) struct Spoilers {
    /// Blurred copies of hidden pictures. Tiny, so they stay outside the texture budget.
    blurred: HashMap<(PeerId, i32), TextureHandle>,
}

impl Spoilers {
    pub(super) fn forget(&mut self, key: (PeerId, i32)) {
        self.blurred.remove(&key);
    }
}

impl App {
    /// Every picture that arrives: a hidden one keeps a blurred copy for its cover.
    pub(super) fn on_spoiler_image(&mut self, key: ImgKey, image: &ColorImage) {
        let ImgKey::Photo(peer, id) = key else { return };
        let hidden = self.history.get(&peer).is_some_and(|list| list.iter().any(|m| m.id == id && m.spoiler));
        if hidden && !self.spoilers.blurred.contains_key(&(peer, id)) {
            let tex = self.ctx.load_texture(format!("spoiler-{peer:?}-{id}"), blurred(image), TextureOptions::LINEAR);
            self.spoilers.blurred.insert((peer, id), tex);
        }
    }

    /// The media of `m` is still under its spoiler.
    pub(super) fn media_hidden(&self, peer: PeerId, m: &Msg) -> bool {
        m.spoiler && !self.revealed.contains(&(peer, m.id))
    }

    /// The picture to draw for `m`: its blurred copy while it is hidden.
    pub(super) fn media_tex(&self, peer: PeerId, m: &Msg) -> Option<&TextureHandle> {
        if self.media_hidden(peer, m) {
            return self.spoilers.blurred.get(&(peer, m.id));
        }
        self.textures.get(&ImgKey::Photo(peer, m.id))
    }
}

/// A small, box-blurred copy of `image`.
pub(super) fn blurred(image: &ColorImage) -> ColorImage {
    let [w, h] = image.size;
    if w == 0 || h == 0 {
        return ColorImage::filled([1, 1], Color32::BLACK);
    }
    let k = BLUR_PX as f32 / w.max(h) as f32;
    let (bw, bh) = (((w as f32 * k).round() as usize).max(1), ((h as f32 * k).round() as usize).max(1));
    // Averaging whole areas instead of sampling keeps it free of aliasing.
    let mut acc = vec![[0u32; 4]; bw * bh];
    let mut count = vec![0u32; bw * bh];
    for y in 0..h {
        let row = y * bh / h * bw;
        for x in 0..w {
            let c = image.pixels[y * w + x];
            let a = &mut acc[row + x * bw / w];
            for (s, v) in a.iter_mut().zip(c.to_array()) {
                *s += u32::from(v);
            }
            count[row + x * bw / w] += 1;
        }
    }
    let small: Vec<[u32; 4]> = acc.iter().zip(&count).map(|(a, n)| a.map(|s| s / (*n).max(1))).collect();
    let mut pixels = Vec::with_capacity(bw * bh);
    for y in 0..bh {
        for x in 0..bw {
            let mut sum = [0u32; 4];
            let mut n = 0;
            for yy in y.saturating_sub(1)..(y + 2).min(bh) {
                for xx in x.saturating_sub(1)..(x + 2).min(bw) {
                    for (s, v) in sum.iter_mut().zip(small[yy * bw + xx]) {
                        *s += v;
                    }
                    n += 1;
                }
            }
            let [r, g, b, a] = sum.map(|s| (s / n) as u8);
            pixels.push(Color32::from_rgba_premultiplied(r, g, b, a));
        }
    }
    ColorImage::new([bw, bh], pixels)
}

/// A number in 0..1 from an index, stable between frames.
fn hash(i: u32) -> f32 {
    let mut x = i.wrapping_mul(0x9e37_79b9) ^ 0x85eb_ca6b;
    x ^= x >> 15;
    x = x.wrapping_mul(0x2c1b_3c6d);
    x ^= x >> 12;
    (x & 0xffff) as f32 / 65535.0
}

/// The cover over hidden media: dimmed, with twinkling dots drifting slowly, like Telegram's.
pub(super) fn cover(p: &egui::Painter, r: Rect, radius: f32, time: f64) {
    p.rect_filled(r, radius, Color32::from_black_alpha(70));
    // Dots stay off the rounded corners.
    let area = r.shrink(radius * 0.3);
    let t = time as f32;
    let n = ((area.area() / 110.0) as usize).clamp(30, 900);
    for i in 0..n as u32 {
        let h = |k: u32| hash(i * 8 + k);
        let speed = 0.01 + h(2) * 0.03;
        let angle = h(3) * std::f32::consts::TAU;
        let x = (h(0) + t * speed * angle.cos()).rem_euclid(1.0);
        let y = (h(1) + t * speed * angle.sin() * area.width() / area.height()).rem_euclid(1.0);
        let life = (t * (0.6 + h(4)) + h(5) * std::f32::consts::TAU).sin() * 0.5 + 0.5;
        let at = area.min + vec2(x * area.width(), y * area.height());
        p.circle_filled(at, 0.6 + h(6) * 0.7, Color32::from_white_alpha((life * 230.0) as u8));
    }
    p.ctx().request_repaint_after(Duration::from_millis(40));
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn blur_keeps_the_aspect_and_the_average() {
        let img = ColorImage::filled([400, 200], Color32::from_rgb(200, 100, 50));
        let b = blurred(&img);
        assert_eq!(b.size, [BLUR_PX, BLUR_PX / 2]);
        assert!(b.pixels.iter().all(|c| *c == Color32::from_rgb(200, 100, 50)));
        assert_eq!(blurred(&ColorImage::filled([0, 0], Color32::WHITE)).size, [1, 1]);
    }
}
