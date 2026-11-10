//! The chat content under the glass bars, for the GPU blur.
//!
//! egui paints the history and the bars in one render pass, and a pass can't sample its own target,
//! so the pixels under a bar can't be read back mid-frame. Instead, the history's shapes that lie
//! under a bar are tessellated a second time into a plain coloured mesh, which `blur.rs` paints into
//! a copy of the background before blurring it. Text becomes soft word-sized pills and pictures a
//! colour grid from their 8×8 thumbnail: after the blur that is all the eye can tell apart.

use std::collections::HashMap;
use std::ops::Range;
use std::sync::{LazyLock, Mutex};

use bytemuck::{Pod, Zeroable};
use eframe::egui::{self, epaint, Color32, Context, LayerId, Pos2, Rect, Shape, TextureId, Ui};

/// How far around a bar the blur and the refracting rim reach.
const PAD: f32 = 48.0;
const THUMB: usize = 8;
/// A textured triangle is split into SUBDIV² so the thumbnail shows as a colour grid.
const SUBDIV: usize = 4;
/// Text strokes cover about this much of their line.
const INK_COVER: f32 = 0.45;

#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable)]
pub(super) struct Vert {
    /// In points; the shader divides by the screen size in points.
    pub pos: [f32; 2],
    /// Premultiplied, gamma space, as egui's own vertices.
    pub color: [u8; 4],
}

pub(super) struct Draw {
    pub indices: Range<u32>,
    pub clip: Rect,
}

/// This frame's content mesh.
#[derive(Default)]
pub(super) struct Layer {
    pub verts: Vec<Vert>,
    pub indices: Vec<u32>,
    pub draws: Vec<Draw>,
}

/// Glass panels painted over the content since `ContentMark::end_content`.
struct Capture {
    on: bool,
    zones: Vec<Rect>,
}

static CAPTURE: Mutex<Capture> = Mutex::new(Capture { on: false, zones: Vec::new() });
/// Set while the UI is laid out, taken when the frame is rendered.
pub(super) static LAYER: Mutex<Option<Layer>> = Mutex::new(None);
static THUMBS: LazyLock<Mutex<HashMap<TextureId, [Color32; THUMB * THUMB]>>> = LazyLock::new(Default::default);

/// Called for every GPU glass panel: one painted over the content samples it and widens the capture.
pub(super) fn over_content(rect: Rect) -> bool {
    let mut c = CAPTURE.lock().unwrap_or_else(|e| e.into_inner());
    if c.on {
        c.zones.push(rect);
    }
    c.on
}

/// A tiny copy of a picture, so it shows through the glass in its own colours.
pub struct Thumb(Option<[Color32; THUMB * THUMB]>);

impl Thumb {
    pub fn of(img: &egui::ColorImage) -> Self {
        let [w, h] = img.size;
        if w == 0 || h == 0 || img.pixels.len() < w * h {
            return Self(None);
        }
        let mut grid = [Color32::TRANSPARENT; THUMB * THUMB];
        for (i, cell) in grid.iter_mut().enumerate() {
            let (cx, cy) = (i % THUMB, i / THUMB);
            // Four samples per cell are plenty for something that is blurred anyway.
            let mut acc = [0u32; 4];
            for s in 0..4 {
                let x = ((cx * 2 + s % 2) * 2 + 1) * w / (THUMB * 4);
                let y = ((cy * 2 + s / 2) * 2 + 1) * h / (THUMB * 4);
                let p = img.pixels[y.min(h - 1) * w + x.min(w - 1)].to_array();
                for k in 0..4 {
                    acc[k] += p[k] as u32;
                }
            }
            *cell = Color32::from_rgba_premultiplied((acc[0] / 4) as u8, (acc[1] / 4) as u8, (acc[2] / 4) as u8, (acc[3] / 4) as u8);
        }
        Self(Some(grid))
    }

    /// Ties the copy to the texture made from the picture.
    pub fn remember(self, id: TextureId) {
        let Some(grid) = self.0 else { return };
        let mut map = THUMBS.lock().unwrap_or_else(|e| e.into_inner());
        // Texture ids are never reused, so old entries only cost memory.
        if map.len() > 4096 {
            map.clear();
        }
        map.insert(id, grid);
    }
}

/// The scrolling content's place in its layer's paint list.
pub struct ContentMark {
    layer: LayerId,
    start: usize,
    end: usize,
}

fn next_idx(ctx: &Context, layer: LayerId) -> usize {
    ctx.graphics(|g| g.get(layer).map_or(0, |l| l.next_idx().0))
}

impl ContentMark {
    /// Call before painting the content.
    pub fn begin(ui: &Ui) -> Self {
        let layer = ui.layer_id();
        let start = next_idx(ui.ctx(), layer);
        Self { layer, start, end: start }
    }

    /// Call after the content: the glass painted from here on floats over it.
    pub fn end_content(&mut self, ui: &Ui) {
        self.end = next_idx(ui.ctx(), self.layer);
        let mut c = CAPTURE.lock().unwrap_or_else(|e| e.into_inner());
        c.on = crate::glass::live_blur();
        c.zones.clear();
    }

    /// Call after the glass: captures the content under it for this frame's blur.
    pub fn finish(self, ctx: &Context) {
        let zones: Vec<Rect> = {
            let mut c = CAPTURE.lock().unwrap_or_else(|e| e.into_inner());
            c.on = false;
            std::mem::take(&mut c.zones).into_iter().map(|z| z.expand(PAD)).collect()
        };
        if zones.is_empty() || self.end <= self.start {
            return;
        }
        let under = |r: Rect| zones.iter().any(|z| z.intersects(r));
        let mut shapes = Vec::new();
        ctx.graphics(|g| {
            let Some(list) = g.get(self.layer) else { return };
            for cs in list.all_entries().skip(self.start).take(self.end - self.start) {
                if !under(cs.shape.visual_bounding_rect().intersect(cs.clip_rect)) {
                    continue;
                }
                let mut flat = Vec::new();
                flatten(&cs.shape, &mut flat);
                for shape in flat {
                    if under(shape.visual_bounding_rect().intersect(cs.clip_rect)) {
                        shapes.push(epaint::ClippedShape { clip_rect: cs.clip_rect, shape, transform_after_tessellation: cs.transform_after_tessellation });
                    }
                }
            }
        });
        if shapes.is_empty() {
            return;
        }
        let mut opts = ctx.tessellation_options(|o| *o);
        // Prerasterized discs sample the font atlas, which the content pass doesn't have.
        opts.prerasterized_discs = false;
        let font = ctx.fonts(|f| f.font_image_size());
        let prims = epaint::Tessellator::new(ctx.pixels_per_point(), opts, font, Vec::new()).tessellate_shapes(shapes);
        let thumbs = THUMBS.lock().unwrap_or_else(|e| e.into_inner());
        let mut layer = Layer::default();
        for p in prims {
            let epaint::Primitive::Mesh(mesh) = p.primitive else { continue };
            let first = layer.indices.len() as u32;
            if mesh.texture_id == TextureId::default() {
                push_plain(&mut layer, &mesh);
            } else {
                push_textured(&mut layer, &mesh, thumbs.get(&mesh.texture_id));
            }
            let end = layer.indices.len() as u32;
            match layer.draws.last_mut() {
                Some(d) if d.clip == p.clip_rect && d.indices.end == first => d.indices.end = end,
                _ => layer.draws.push(Draw { indices: first..end, clip: p.clip_rect }),
            }
        }
        if !layer.indices.is_empty() {
            *LAYER.lock().unwrap_or_else(|e| e.into_inner()) = Some(layer);
        }
    }
}

/// Leaves only shapes the content pass can draw: text turns into pills, callbacks go.
fn flatten(shape: &Shape, out: &mut Vec<Shape>) {
    match shape {
        Shape::Vec(v) => v.iter().for_each(|s| flatten(s, out)),
        Shape::Noop | Shape::Callback(_) => {}
        Shape::Text(t) => text_pills(t, out),
        s => out.push(s.clone()),
    }
}

/// A soft pill over the x-height of every word, in the word's colour.
fn text_pills(t: &epaint::TextShape, out: &mut Vec<Shape>) {
    for row in &t.galley.rows {
        let origin = t.pos + row.pos.to_vec2();
        let mut word: Option<(f32, f32, f32, f32, Color32)> = None;
        let mut flush = |w: &mut Option<(f32, f32, f32, f32, Color32)>| {
            if let Some((x0, x1, base, ascent, color)) = w.take() {
                let h = ascent * 0.62;
                let r = Rect::from_min_max(Pos2::new(x0, base - ascent * 0.67), Pos2::new(x1, base - ascent * 0.67 + h));
                // Soft edges: a hard pill reads as a dash where the clear rim shows it.
                let pill = epaint::RectShape::filled(r, h / 2.0, color.gamma_multiply(INK_COVER * t.opacity_factor)).with_blur_width(h);
                out.push(Shape::Rect(pill));
            }
        };
        for g in &row.row.glyphs {
            if g.chr.is_whitespace() || g.is_color {
                flush(&mut word);
                continue;
            }
            let mut color = row.row.visuals.mesh.vertices.get(g.first_vertex as usize).map_or(t.fallback_color, |v| v.color);
            if color == Color32::PLACEHOLDER {
                color = t.fallback_color;
            }
            if let Some(c) = t.override_text_color {
                color = c;
            }
            let (x0, x1, base) = (origin.x + g.pos.x, origin.x + g.pos.x + g.advance_width, origin.y + g.pos.y);
            match &mut word {
                Some(w) if w.4 == color && (w.2 - base).abs() < 0.5 => w.1 = x1,
                _ => {
                    flush(&mut word);
                    word = Some((x0, x1, base, g.font_ascent, color));
                }
            }
        }
        flush(&mut word);
    }
}

fn push_plain(layer: &mut Layer, mesh: &epaint::Mesh) {
    let base = layer.verts.len() as u32;
    layer.verts.extend(mesh.vertices.iter().map(|v| Vert { pos: [v.pos.x, v.pos.y], color: v.color.to_array() }));
    layer.indices.extend(mesh.indices.iter().map(|i| base + i));
}

/// Bilinear sample of a thumbnail; an unknown picture reads as mid grey.
fn sample(thumb: Option<&[Color32; THUMB * THUMB]>, uv: Pos2) -> [f32; 4] {
    let Some(t) = thumb else { return [0.5, 0.5, 0.5, 1.0] };
    let n = THUMB as f32;
    let (x, y) = ((uv.x * n - 0.5).clamp(0.0, n - 1.0), (uv.y * n - 0.5).clamp(0.0, n - 1.0));
    let (x0, y0) = (x.floor() as usize, y.floor() as usize);
    let (x1, y1) = ((x0 + 1).min(THUMB - 1), (y0 + 1).min(THUMB - 1));
    let (fx, fy) = (x.fract(), y.fract());
    let px = |x: usize, y: usize| t[y * THUMB + x].to_array().map(|c| c as f32 / 255.0);
    let (a, b, c, d) = (px(x0, y0), px(x1, y0), px(x0, y1), px(x1, y1));
    std::array::from_fn(|k| (a[k] * (1.0 - fx) + b[k] * fx) * (1.0 - fy) + (c[k] * (1.0 - fx) + d[k] * fx) * fy)
}

/// Pictures: every triangle split into a small grid tinted from the thumbnail.
fn push_textured(layer: &mut Layer, mesh: &epaint::Mesh, thumb: Option<&[Color32; THUMB * THUMB]>) {
    let n = SUBDIV;
    for tri in mesh.indices.chunks_exact(3) {
        let [a, b, c] = [tri[0], tri[1], tri[2]].map(|i| mesh.vertices.get(i as usize).copied().unwrap_or_default());
        let base = layer.verts.len() as u32;
        // Barycentric grid: row i has n + 1 - i points.
        for i in 0..=n {
            for j in 0..=n - i {
                let (u, v) = (i as f32 / n as f32, j as f32 / n as f32);
                let w = 1.0 - u - v;
                let pos = a.pos.to_vec2() * w + b.pos.to_vec2() * u + c.pos.to_vec2() * v;
                let uv = a.uv.to_vec2() * w + b.uv.to_vec2() * u + c.uv.to_vec2() * v;
                let tint: [f32; 4] = std::array::from_fn(|k| {
                    let ch = |x: Color32| x.to_array()[k] as f32;
                    (ch(a.color) * w + ch(b.color) * u + ch(c.color) * v) / 255.0
                });
                let s = sample(thumb, uv.to_pos2());
                let color = std::array::from_fn(|k| (tint[k] * s[k] * 255.0).round().clamp(0.0, 255.0) as u8);
                layer.verts.push(Vert { pos: [pos.x, pos.y], color });
            }
        }
        let row_start = |i: usize| base + (0..i).map(|r| (n + 1 - r) as u32).sum::<u32>();
        for i in 0..n {
            for j in 0..n - i {
                let (p, q, r) = (row_start(i) + j as u32, row_start(i) + j as u32 + 1, row_start(i + 1) + j as u32);
                layer.indices.extend([p, r, q]);
                if j + 1 < n - i {
                    let s = row_start(i + 1) + j as u32 + 1;
                    layer.indices.extend([q, r, s]);
                }
            }
        }
    }
}
