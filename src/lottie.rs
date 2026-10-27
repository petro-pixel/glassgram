//! Animated stickers (`.tgs`: gzipped Lottie JSON), drawn on the CPU: `velato` reads the
//! animation and walks its frame, `vello_cpu` (already in the build through egui) rasterizes it.
//! Both are pure Rust.
//!
//! A sticker comes from a stranger, so the file is checked before anything is rendered: sizes
//! are capped and repeaters (which multiply the work without bound) are limited.

use std::io::Read;

use velato::model::{ImageAsset, fixed};
use velato::{Composition, RenderSink, Renderer};
use vello_cpu::kurbo::{Affine, BezPath, Shape};
use vello_cpu::{PaintType, Pixmap, RenderContext, Resources};

/// Telegram limits `.tgs` files to 64 KB; anything far bigger is not a sticker.
const MAX_FILE: usize = 1 << 20;
const MAX_JSON: u64 = 8 << 20;
/// Repeaters in one animation and copies in one repeater, beyond which we don't animate it.
const MAX_REPEATERS: usize = 8;
const MAX_COPIES: f64 = 32.0;

pub struct Lottie {
    comp: Composition,
    renderer: Renderer,
    ctx: RenderContext,
    resources: Resources,
    side: u16,
}

impl Lottie {
    /// Reads a `.tgs` file (or plain Lottie JSON) to be drawn as `side`×`side` pixel frames.
    pub fn load(data: &[u8], side: u16) -> Result<Self, String> {
        if data.len() > MAX_FILE {
            return Err("sticker file too large".into());
        }
        let mut json = Vec::new();
        if data.starts_with(&[0x1f, 0x8b]) {
            flate2::read::GzDecoder::new(data).take(MAX_JSON + 1).read_to_end(&mut json).map_err(|e| e.to_string())?;
            if json.len() as u64 > MAX_JSON {
                return Err("sticker too large unpacked".into());
            }
        } else {
            json = data.to_vec();
        }
        let value: serde_json::Value = serde_json::from_slice(&json).map_err(|e| e.to_string())?;
        check(&value)?;
        let comp = Composition::from_json(value).map_err(|e| e.to_string())?;
        if comp.width == 0 || comp.height == 0 || comp.frame_rate <= 0.0 || comp.frames.end <= comp.frames.start {
            return Err("empty animation".into());
        }
        let side = side.clamp(16, 1024);
        Ok(Self { comp, renderer: Renderer::new(), ctx: RenderContext::new(side, side), resources: Resources::new(), side })
    }

    /// Length in seconds.
    pub fn duration(&self) -> f64 {
        (self.comp.frames.end - self.comp.frames.start) / self.comp.frame_rate
    }

    /// RGBA (not premultiplied) of the frame at `secs` from the start.
    pub fn frame(&mut self, secs: f64) -> Vec<u8> {
        let frame = self.comp.frames.start + secs * self.comp.frame_rate;
        // Fit the animation into the square, centred.
        let (w, h) = (self.comp.width as f64, self.comp.height as f64);
        let s = self.side as f64 / w.max(h);
        let at = Affine::translate(((self.side as f64 - w * s) / 2.0, (self.side as f64 - h * s) / 2.0)) * Affine::scale(s);
        self.ctx.reset();
        let mut sink = Sink { ctx: &mut self.ctx };
        self.renderer.append(&self.comp, frame, at, 1.0, &mut sink);
        let mut target = Pixmap::new(self.side, self.side);
        self.ctx.flush();
        self.ctx.render(&mut target, &mut self.resources);
        target.take_unpremultiplied().into_iter().flat_map(|p| [p.r, p.g, p.b, p.a]).collect()
    }
}

/// Refuses animations whose repeaters could make a frame take forever.
fn check(v: &serde_json::Value) -> Result<(), String> {
    let mut repeaters = 0;
    let mut stack = vec![v];
    while let Some(v) = stack.pop() {
        match v {
            serde_json::Value::Object(m) => {
                if m.get("ty").and_then(|t| t.as_str()) == Some("rp") {
                    repeaters += 1;
                    // Copies are a value or keyframes; any number in them counts.
                    let copies = m.get("c").map(numbers).unwrap_or_default();
                    if repeaters > MAX_REPEATERS || copies.iter().any(|c| *c > MAX_COPIES) {
                        return Err("too many repeated shapes".into());
                    }
                }
                stack.extend(m.values());
            }
            serde_json::Value::Array(a) => stack.extend(a),
            _ => {}
        }
    }
    Ok(())
}

fn numbers(v: &serde_json::Value) -> Vec<f64> {
    match v {
        serde_json::Value::Number(n) => n.as_f64().into_iter().collect(),
        serde_json::Value::Array(a) => a.iter().flat_map(numbers).collect(),
        serde_json::Value::Object(m) => m.values().flat_map(numbers).collect(),
        _ => Vec::new(),
    }
}

/// velato's drawing calls onto a `vello_cpu` context.
struct Sink<'a> {
    ctx: &'a mut RenderContext,
}

fn path(shape: &impl Shape) -> BezPath {
    shape.to_path(0.1)
}

impl RenderSink for Sink<'_> {
    fn push_layer(&mut self, blend: impl Into<vello_cpu::peniko::BlendMode>, alpha: f32, transform: Affine, shape: &impl Shape) {
        self.ctx.set_transform(transform);
        self.ctx.push_layer(Some(&path(shape)), Some(blend.into()), Some(alpha), None, None);
    }

    fn push_clip_layer(&mut self, transform: Affine, shape: &impl Shape) {
        self.ctx.set_transform(transform);
        self.ctx.push_clip_layer(&path(shape));
    }

    fn pop_layer(&mut self) {
        self.ctx.pop_layer();
    }

    fn draw(&mut self, stroke: Option<&fixed::Stroke>, transform: Affine, brush: &fixed::Brush, shape: &impl Shape) {
        let paint: PaintType = match brush {
            vello_cpu::peniko::Brush::Solid(c) => PaintType::Solid(*c),
            vello_cpu::peniko::Brush::Gradient(g) => PaintType::Gradient(g.clone()),
            // Stickers have no raster images.
            vello_cpu::peniko::Brush::Image(_) => return,
        };
        self.ctx.set_transform(transform);
        self.ctx.set_paint(paint);
        match stroke {
            Some(s) => {
                self.ctx.set_stroke(s.clone());
                self.ctx.stroke_path(&path(shape));
            }
            None => self.ctx.fill_path(&path(shape)),
        }
    }

    fn draw_image(&mut self, _image: &ImageAsset, _transform: Affine, _alpha: f64) {}
}

#[cfg(test)]
mod tests {
    use super::Lottie;

    /// A one-second red square, scaling up: the smallest real Lottie file.
    const SQUARE: &str = r#"{"v":"5.5.2","fr":30,"ip":0,"op":30,"w":100,"h":100,"layers":[{"ty":4,"ind":1,"ip":0,"op":30,"st":0,
        "ks":{"o":{"a":0,"k":100},"r":{"a":0,"k":0},"p":{"a":0,"k":[50,50,0]},"a":{"a":0,"k":[0,0,0]},"s":{"a":0,"k":[100,100,100]}},
        "shapes":[{"ty":"rc","p":{"a":0,"k":[0,0]},"s":{"a":0,"k":[60,60]},"r":{"a":0,"k":0}},{"ty":"fl","c":{"a":0,"k":[1,0,0,1]},"o":{"a":0,"k":100}}]}]}"#;

    #[test]
    fn draws_a_frame() {
        let mut l = Lottie::load(SQUARE.as_bytes(), 32).expect("a valid animation");
        assert!((l.duration() - 1.0).abs() < 1e-6);
        let px = l.frame(0.5);
        assert_eq!(px.len(), 32 * 32 * 4);
        // The middle is red and opaque, the corner empty.
        let mid = (16 * 32 + 16) * 4;
        assert!(px[mid] > 200 && px[mid + 1] < 40 && px[mid + 3] > 200, "{:?}", &px[mid..mid + 4]);
        assert_eq!(px[3], 0);
    }

    #[test]
    fn refuses_runaway_repeaters() {
        let evil = SQUARE.replace(r#"{"ty":"fl""#, r#"{"ty":"rp","c":{"a":0,"k":100000}},{"ty":"fl""#);
        assert!(Lottie::load(evil.as_bytes(), 32).is_err());
    }
}
