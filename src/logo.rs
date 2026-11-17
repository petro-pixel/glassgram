//! App logo (window, tray): a molten glass drop over gold and white dots, rendered in Blender.
//! The picture is built into the binary; only the unread badge is drawn on top.

use std::sync::OnceLock;

use image::{imageops, Rgba, RgbaImage};

/// The same picture the packages install (`packaging/icons`), so every place shows one icon.
const ICON: &[u8] = include_bytes!("../packaging/icons/glassgram-512.png");

fn source() -> &'static RgbaImage {
    static IMG: OnceLock<RgbaImage> = OnceLock::new();
    // A broken file is a build mistake; an empty icon is better than a crash at start-up.
    IMG.get_or_init(|| image::load_from_memory(ICON).map(|i| i.to_rgba8()).unwrap_or_else(|_| RgbaImage::new(1, 1)))
}

/// Logo of `size`×`size`; `badge` is a red unread dot in the corner.
pub fn render(size: u32, badge: bool) -> RgbaImage {
    let mut img = imageops::resize(source(), size, size, imageops::FilterType::Lanczos3);
    if !badge {
        return img;
    }
    let s = size as f32;
    let aa = 1.0 / s; // anti-aliasing width in logo units
    for (x, y, p) in img.enumerate_pixels_mut() {
        let (u, v) = ((x as f32 + 0.5) / s, (y as f32 + 0.5) / s);
        let mut px = [p[0] as f32, p[1] as f32, p[2] as f32, p[3] as f32 / 255.0];
        let b = ((u - 0.78).powi(2) + (v - 0.22).powi(2)).sqrt() - 0.17;
        // White outline so the dot reads on any panel.
        blend(&mut px, [255.0, 255.0, 255.0], coverage(b - 0.035, aa));
        blend(&mut px, [255.0, 72.0, 92.0], coverage(b, aa));
        *p = Rgba([px[0] as u8, px[1] as u8, px[2] as u8, (px[3] * 255.0) as u8]);
    }
    img
}

/// Pixel coverage of a shape with signed distance `d`.
fn coverage(d: f32, aa: f32) -> f32 {
    (0.5 - d / aa).clamp(0.0, 1.0)
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

#[cfg(test)]
mod tests {
    #[test]
    fn built_in_icon_decodes() {
        let img = super::render(64, true);
        assert_eq!(img.dimensions(), (64, 64));
        assert_eq!(img.get_pixel(0, 63)[3], 0, "the plate keeps its transparent corners");
        assert_eq!(img.get_pixel(32, 32)[3], 255, "the middle is the opaque picture");
    }
}
