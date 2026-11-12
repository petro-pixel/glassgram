//! The photo editor's picture work off the UI thread: decoding with the camera's EXIF turn applied,
//! text drawn on the CPU (so the preview and the result are one picture) and the final render:
//! turn, mirror, crop, then the strokes and text over the photo.

use std::path::{Path, PathBuf};
use eframe::egui::ColorImage;
use image::{DynamicImage, ImageDecoder, RgbaImage};
use vello_cpu::kurbo::{self, BezPath, Cap, Circle, Join, RoundedRect, Shape as _};
use vello_cpu::peniko::Color;
use vello_cpu::{Pixmap, RenderContext, Resources};
use super::{Edit, Stroke, TextItem, TextStyle};

/// Long side of the result: the upload compresses photos to the same size anyway.
const OUT_PX: u32 = 2560;
/// Long side of the editor's preview.
const PREVIEW_PX: u32 = 1600;

/// Opens a picture upright: phones store portrait photos sideways with an EXIF turn.
fn open(path: &Path) -> Result<DynamicImage, String> {
    let mut dec = image::ImageReader::open(path).map_err(|e| e.to_string())?.with_guessed_format().map_err(|e| e.to_string())?.into_decoder().map_err(|e| e.to_string())?;
    let orientation = dec.orientation().unwrap_or(image::metadata::Orientation::NoTransforms);
    let mut img = DynamicImage::from_decoder(dec).map_err(|e| e.to_string())?;
    img.apply_orientation(orientation);
    Ok(img)
}

/// The preview the editor works on.
pub(super) fn preview(path: &Path) -> Result<RgbaImage, String> {
    let img = open(path)?;
    Ok(if img.width().max(img.height()) > PREVIEW_PX { img.thumbnail(PREVIEW_PX, PREVIEW_PX) } else { img }.to_rgba8())
}

/// `rot` quarter turns clockwise, then a mirror: the order `Edit` keeps them in.
pub(super) fn turn(img: &RgbaImage, rot: u8, flip: bool) -> RgbaImage {
    use image::imageops;
    let turned = match rot % 4 {
        1 => imageops::rotate90(img),
        2 => imageops::rotate180(img),
        3 => imageops::rotate270(img),
        _ => img.clone(),
    };
    if flip { imageops::flip_horizontal(&turned) } else { turned }
}

/// Renders the edit of `path` into a new temporary file: PNG when the picture has transparency, JPEG otherwise.
pub(super) fn render(path: &Path, edit: &Edit) -> Result<PathBuf, String> {
    let src = open(path)?;
    let alpha = src.color().has_alpha();
    let img = turn(&src.to_rgba8(), edit.rot, edit.flip);
    drop(src);
    let (w, h) = img.dimensions();
    let [x0, y0, x1, y1] = edit.crop;
    let px = |v: f32, n: u32| ((v.clamp(0.0, 1.0) * n as f32).round() as u32).min(n);
    let (cx, cy) = (px(x0, w).min(w - 1), px(y0, h).min(h - 1));
    let (cw, ch) = (px(x1, w).saturating_sub(cx).max(1), px(y1, h).saturating_sub(cy).max(1));
    let mut out = DynamicImage::ImageRgba8(image::imageops::crop_imm(&img, cx, cy, cw, ch).to_image());
    drop(img);
    if cw.max(ch) > OUT_PX {
        out = out.resize(OUT_PX, OUT_PX, image::imageops::FilterType::Triangle);
    }
    let mut rgba = out.to_rgba8();
    if !edit.strokes.is_empty() || edit.texts.iter().any(|t| !t.text.trim().is_empty()) {
        overlay(&mut rgba, edit);
    }
    let stem = path.file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_default();
    let stem = crate::platform::safe_file_name(&stem);
    let file = crate::platform::temp_file(&format!("{stem}.{}", if alpha { "png" } else { "jpg" }));
    if alpha {
        rgba.save_with_format(&file, image::ImageFormat::Png).map_err(|e| e.to_string())?;
    } else {
        let rgb = DynamicImage::ImageRgba8(rgba).to_rgb8();
        let f = std::fs::File::create(&file).map_err(|e| e.to_string())?;
        image::codecs::jpeg::JpegEncoder::new_with_quality(std::io::BufWriter::new(f), 92).encode_image(&rgb).map_err(|e| e.to_string())?;
    }
    Ok(file)
}

/// Draws the strokes and text over the cropped picture.
fn overlay(rgba: &mut RgbaImage, edit: &Edit) {
    let (ow, oh) = rgba.dimensions();
    let [x0, y0, x1, y1] = edit.crop.map(|v| v as f64);
    // Positions are fractions of the whole turned picture; the output shows only the crop.
    let (tw, th) = (ow as f64 / (x1 - x0).max(1e-6), oh as f64 / (y1 - y0).max(1e-6));
    let long = tw.max(th);
    let map = |p: [f32; 2]| kurbo::Point::new((p[0] as f64 - x0) * tw, (p[1] as f64 - y0) * th);
    let mut ctx = RenderContext::new(ow as u16, oh as u16);
    for s in &edit.strokes {
        stroke(&mut ctx, s, &map, long);
    }
    for t in edit.texts.iter().filter(|t| !t.text.trim().is_empty()) {
        if let Some(lay) = layout(t, t.size as f64 * long) {
            paint_text(&mut ctx, &lay, t, map(t.pos));
        }
    }
    let mut target = Pixmap::new(ow as u16, oh as u16);
    ctx.flush();
    ctx.render(&mut target, &mut Resources::new());
    for (dst, o) in rgba.pixels_mut().zip(target.take_unpremultiplied()) {
        let a = o.a as u32;
        if a == 0 {
            continue;
        }
        let mix = |s: u8, d: u8| ((s as u32 * a + d as u32 * (255 - a)) / 255) as u8;
        dst.0 = [mix(o.r, dst.0[0]), mix(o.g, dst.0[1]), mix(o.b, dst.0[2]), (a + dst.0[3] as u32 * (255 - a) / 255) as u8];
    }
}

fn rgb([r, g, b]: [u8; 3]) -> Color {
    Color::from_rgb8(r, g, b)
}

fn stroke(ctx: &mut RenderContext, s: &Stroke, map: &impl Fn([f32; 2]) -> kurbo::Point, long: f64) {
    let w = (s.width as f64 * long).max(1.0);
    ctx.set_paint(rgb(s.color));
    let Some((first, rest)) = s.points.split_first() else { return };
    if rest.is_empty() {
        ctx.fill_path(&Circle::new(map(*first), w / 2.0).to_path(0.1));
        return;
    }
    let mut path = BezPath::new();
    path.move_to(map(*first));
    for p in rest {
        path.line_to(map(*p));
    }
    ctx.set_stroke(kurbo::Stroke::new(w).with_caps(Cap::Round).with_join(Join::Round));
    ctx.stroke_path(&path);
}

/// Text laid out at a font size in pixels: the lines' outlines and the box around them.
pub(super) struct Layout {
    lines: Vec<(BezPath, f64)>,
    text_w: f64,
    px: f64,
    /// The whole box, padding included: what a chip fills and a click hits.
    pub(super) size: kurbo::Size,
}

const LINE: f64 = 1.22;

fn pad(px: f64) -> f64 {
    px * 0.32
}

pub(super) fn layout(t: &TextItem, px: f64) -> Option<Layout> {
    let lines = t.text.split('\n').map(|l| crate::draw::text_path_in(t.font.data(), l, px)).collect::<Option<Vec<_>>>()?;
    let text_w = lines.iter().map(|l| l.1).fold(0.0, f64::max);
    let size = kurbo::Size::new(text_w + 2.0 * pad(px), lines.len() as f64 * px * LINE + 2.0 * pad(px) * 0.6);
    Some(Layout { lines, text_w, px, size })
}

/// Black or white, whichever reads on `c`.
pub(super) fn contrast(c: [u8; 3]) -> [u8; 3] {
    let lum = 0.299 * c[0] as f32 + 0.587 * c[1] as f32 + 0.114 * c[2] as f32;
    if lum > 160.0 { [0, 0, 0] } else { [255, 255, 255] }
}

fn paint_text(ctx: &mut RenderContext, lay: &Layout, t: &TextItem, center: kurbo::Point) {
    let px = lay.px;
    let origin = center - kurbo::Vec2::new(lay.size.width / 2.0, lay.size.height / 2.0);
    let fill = match t.style {
        TextStyle::Chip => {
            ctx.set_paint(rgb(t.color));
            ctx.fill_path(&RoundedRect::from_rect(kurbo::Rect::from_origin_size(origin, lay.size), px * 0.36).to_path(0.1));
            contrast(t.color)
        }
        _ => t.color,
    };
    let mut path = BezPath::new();
    for (k, (line, w)) in lay.lines.iter().enumerate() {
        let x = origin.x + pad(px) + (lay.text_w - w) / 2.0;
        let base = origin.y + pad(px) * 0.6 + k as f64 * px * LINE + px * 0.95;
        path.extend(kurbo::Affine::translate((x, base)) * line.clone());
    }
    if t.style == TextStyle::Outline {
        ctx.set_paint(rgb(contrast(t.color)));
        ctx.set_stroke(kurbo::Stroke::new(px * 0.16).with_join(Join::Round));
        ctx.stroke_path(&path);
    }
    ctx.set_paint(rgb(fill));
    ctx.fill_path(&path);
}

/// One text item as a picture for the preview, `px` being its font size in screen pixels.
pub(super) fn text_image(t: &TextItem, px: f64) -> Option<ColorImage> {
    let lay = layout(t, px.clamp(4.0, 600.0))?;
    let (w, h) = (lay.size.width.ceil().clamp(1.0, 4000.0) as u16, lay.size.height.ceil().clamp(1.0, 4000.0) as u16);
    let mut ctx = RenderContext::new(w, h);
    paint_text(&mut ctx, &lay, t, kurbo::Point::new(lay.size.width / 2.0, lay.size.height / 2.0));
    let mut target = Pixmap::new(w, h);
    ctx.flush();
    ctx.render(&mut target, &mut Resources::new());
    let rgba: Vec<u8> = target.take_unpremultiplied().into_iter().flat_map(|p| [p.r, p.g, p.b, p.a]).collect();
    Some(ColorImage::from_rgba_unmultiplied([w as usize, h as usize], &rgba))
}

#[cfg(test)]
mod tests {
    use super::*;
    use super::super::Font;

    #[test]
    fn renders_an_edit() {
        let path = crate::platform::temp_file("edit-test.png");
        RgbaImage::from_pixel(400, 300, image::Rgba([20, 40, 60, 255])).save(&path).expect("save");
        let mut edit = Edit::default();
        edit.rotate_left();
        edit.crop = [0.25, 0.0, 0.75, 1.0];
        edit.strokes.push(Stroke { points: vec![[0.3, 0.3], [0.6, 0.6]], color: [255, 0, 0], width: 0.05 });
        edit.texts.push(TextItem { text: "Hi\nthere".into(), pos: [0.5, 0.5], size: 0.1, color: [255, 255, 255], style: TextStyle::Chip, font: Font::Bold });
        let out = render(&path, &edit).expect("render");
        let img = image::open(&out).expect("open").to_rgba8();
        // Turned to 300×400, then half the width kept.
        assert_eq!(img.dimensions(), (150, 400));
        let _ = std::fs::remove_file(path);
        let _ = std::fs::remove_file(out);
    }
}
