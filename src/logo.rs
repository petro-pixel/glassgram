//! App logo (window, tray): a glass circle with a chat bubble.
//! Drawn with signed distances and custom anti-aliasing: no files and no rasterizer.

use image::{Rgba, RgbaImage};

/// Logo of `size`×`size`; `badge` is a red unread dot in the corner.
pub fn render(size: u32, badge: bool) -> RgbaImage {
    let s = size as f32;
    let aa = 1.0 / s; // anti-aliasing width in logo units
    RgbaImage::from_fn(size, size, |x, y| {
        // 0..1 coordinates at the pixel centre.
        let (u, v) = ((x as f32 + 0.5) / s, (y as f32 + 0.5) / s);
        let mut px = [0.0f32; 4];

        // Circle with a diagonal gradient.
        let d = ((u - 0.5).powi(2) + (v - 0.5).powi(2)).sqrt() - 0.47;
        let t = (u + v) / 2.0;
        let base = mix([96.0, 128.0, 255.0], [184.0, 104.0, 255.0], t);
        blend(&mut px, base, coverage(d, aa));
        // Specular highlight at the top, like the UI's glass panels.
        let hl = ((u - 0.5).powi(2) / 0.16 + (v - 0.27).powi(2) / 0.03).sqrt() - 1.0;
        blend(&mut px, [255.0, 255.0, 255.0], coverage(hl * 0.15, aa) * 0.22 * coverage(d, aa));
        // Rim.
        blend(&mut px, [255.0, 255.0, 255.0], coverage(d.abs() - 0.012, aa) * 0.45);

        // Bubble: a rounded rectangle and a tail at the bottom left.
        let bubble = rounded_rect(u - 0.5, v - 0.47, 0.22, 0.155, 0.11);
        let tail = triangle((u, v), (0.33, 0.56), (0.30, 0.72), (0.46, 0.60));
        let shape = bubble.min(tail);
        blend(&mut px, [255.0, 255.0, 255.0], coverage(shape, aa) * 0.96);
        // Three “typing” dots in the bubble, in the circle's colour.
        for cx in [0.40, 0.50, 0.60] {
            let dot = ((u - cx).powi(2) + (v - 0.47).powi(2)).sqrt() - 0.032;
            blend(&mut px, mix([96.0, 128.0, 255.0], [184.0, 104.0, 255.0], cx), coverage(dot, aa));
        }

        if badge {
            let b = ((u - 0.78).powi(2) + (v - 0.22).powi(2)).sqrt() - 0.17;
            // White outline so the dot reads on any panel.
            blend(&mut px, [255.0, 255.0, 255.0], coverage(b - 0.035, aa));
            blend(&mut px, [255.0, 72.0, 92.0], coverage(b, aa));
        }
        Rgba([px[0] as u8, px[1] as u8, px[2] as u8, (px[3] * 255.0) as u8])
    })
}

/// Pixel coverage of a shape with signed distance `d`.
fn coverage(d: f32, aa: f32) -> f32 {
    (0.5 - d / aa).clamp(0.0, 1.0)
}

fn mix(a: [f32; 3], b: [f32; 3], t: f32) -> [f32; 3] {
    let t = t.clamp(0.0, 1.0);
    [a[0] + (b[0] - a[0]) * t, a[1] + (b[1] - a[1]) * t, a[2] + (b[2] - a[2]) * t]
}

/// Blends a colour on top (premultiplied logic on a non-premultiplied pixel).
fn blend(px: &mut [f32; 4], c: [f32; 3], a: f32) {
    if a <= 0.0 {
        return;
    }
    let out_a = a + px[3] * (1.0 - a);
    for i in 0..3 {
        px[i] = (c[i] * a + px[i] * px[3] * (1.0 - a)) / out_a.max(1e-6);
    }
    px[3] = out_a;
}

fn rounded_rect(x: f32, y: f32, hw: f32, hh: f32, r: f32) -> f32 {
    let (qx, qy) = (x.abs() - hw + r, y.abs() - hh + r);
    (qx.max(0.0).powi(2) + qy.max(0.0).powi(2)).sqrt() + qx.max(qy).min(0.0) - r
}

/// Signed distance to a triangle (negative inside).
fn triangle(p: (f32, f32), a: (f32, f32), b: (f32, f32), c: (f32, f32)) -> f32 {
    let edge = |p: (f32, f32), a: (f32, f32), b: (f32, f32)| {
        let (ex, ey) = (b.0 - a.0, b.1 - a.1);
        let (wx, wy) = (p.0 - a.0, p.1 - a.1);
        let t = ((wx * ex + wy * ey) / (ex * ex + ey * ey)).clamp(0.0, 1.0);
        let (dx, dy) = (wx - ex * t, wy - ey * t);
        ((dx * dx + dy * dy).sqrt(), ex * wy - ey * wx)
    };
    let (d0, s0) = edge(p, a, b);
    let (d1, s1) = edge(p, b, c);
    let (d2, s2) = edge(p, c, a);
    let d = d0.min(d1).min(d2);
    let inside = (s0 >= 0.0 && s1 >= 0.0 && s2 >= 0.0) || (s0 <= 0.0 && s1 <= 0.0 && s2 <= 0.0);
    if inside { -d } else { d }
}

