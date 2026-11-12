//! Helpers for pictures drawn on the CPU with `vello_cpu`, outside the egui frame: the username
//! QR card and the taskbar unread badge.

use vello_cpu::kurbo::{self, BezPath};

/// `text` in Ubuntu Bold as one path with its baseline at y = 0, and its advance width.
pub fn text_path(text: &str, size: f64) -> Option<(BezPath, f64)> {
    text_path_in(crate::format::UBUNTU_BOLD, text, size)
}

/// `text_path` in another font (a TrueType file).
pub fn text_path_in(font: &[u8], text: &str, size: f64) -> Option<(BezPath, f64)> {
    struct Pen<'a> {
        path: &'a mut BezPath,
        x: f64,
        s: f64,
    }
    impl Pen<'_> {
        fn p(&self, x: f32, y: f32) -> kurbo::Point {
            kurbo::Point::new(self.x + x as f64 * self.s, -(y as f64) * self.s)
        }
    }
    impl ttf_parser::OutlineBuilder for Pen<'_> {
        fn move_to(&mut self, x: f32, y: f32) {
            let p = self.p(x, y);
            self.path.move_to(p);
        }
        fn line_to(&mut self, x: f32, y: f32) {
            let p = self.p(x, y);
            self.path.line_to(p);
        }
        fn quad_to(&mut self, x1: f32, y1: f32, x: f32, y: f32) {
            let (a, b) = (self.p(x1, y1), self.p(x, y));
            self.path.quad_to(a, b);
        }
        fn curve_to(&mut self, x1: f32, y1: f32, x2: f32, y2: f32, x: f32, y: f32) {
            let (a, b, c) = (self.p(x1, y1), self.p(x2, y2), self.p(x, y));
            self.path.curve_to(a, b, c);
        }
        fn close(&mut self) {
            self.path.close_path();
        }
    }
    let face = ttf_parser::Face::parse(font, 0).ok()?;
    let s = size / face.units_per_em() as f64;
    let mut path = BezPath::new();
    let mut x = 0.0;
    for ch in text.chars() {
        // Letters Ubuntu lacks (CJK, emoji) are skipped rather than drawn as boxes.
        let Some(id) = face.glyph_index(ch) else { continue };
        face.outline_glyph(id, &mut Pen { path: &mut path, x, s });
        x += face.glyph_hor_advance(id).unwrap_or(0) as f64 * s;
    }
    Some((path, x))
}
