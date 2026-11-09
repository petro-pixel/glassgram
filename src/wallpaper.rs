//! Chat backgrounds drawn on the CPU, off the UI thread: Telegram's fills (one colour, a two-colour
//! linear gradient, the three/four-colour "freeform" one), pictures, and pattern wallpapers — a
//! gzipped SVG mask over a fill at an intensity (negative in dark themes: the pattern shows the fill
//! through black). The result is one opaque image at the chat's aspect.

use std::io::Read as _;
use std::path::PathBuf;

use eframe::egui::ColorImage;
use vello_cpu::kurbo::{self, Affine, BezPath, Shape as _};
use vello_cpu::{PaintType, Pixmap, RenderContext, Resources};

/// Telegram's default background fill, used when a pattern comes without colours.
pub const DEFAULT_COLORS: [u32; 4] = [0xdbddbb, 0x6ba587, 0xd5d88d, 0x88b884];
/// Unpacked pattern files above this are refused: a pattern is a few hundred KB.
const MAX_SVG: u64 = 8 << 20;
/// Shapes beyond this are skipped, so a hostile file can't make a render take forever.
const MAX_SHAPES: usize = 20_000;
/// The longest side we render a background at.
pub const MAX_SIDE: u32 = 1600;

/// What lies under a chat's messages.
#[derive(Clone, Debug, PartialEq, Default)]
pub enum Spec {
    /// Our live background (no wallpaper).
    #[default]
    Live,
    /// Fill colours (0xRRGGBB, 1–4) and the two-colour gradient's rotation in degrees.
    Fill { colors: Vec<u32>, rotation: i32 },
    /// A picture: a server wallpaper or a local file, cropped to fill.
    Image { path: PathBuf },
    /// A pattern file over a fill; intensity is −100…100.
    Pattern { path: PathBuf, colors: Vec<u32>, rotation: i32, intensity: i32 },
}

fn hex_list(colors: &[u32]) -> String {
    colors.iter().map(|c| format!("{c:06x}")).collect::<Vec<_>>().join(",")
}

fn parse_hex(v: &str) -> Vec<u32> {
    v.split(',').filter_map(|c| u32::from_str_radix(c.trim(), 16).ok()).map(|c| c & 0xff_ffff).take(4).collect()
}

impl Spec {
    /// The settings line: `fill:<rot>:<colours>`, `image:<path>`, `pattern:<rot>:<intensity>:<colours>:<path>`.
    /// The path goes last, so a `C:\` in it splits nothing.
    pub fn code(&self) -> String {
        match self {
            Spec::Live => String::new(),
            Spec::Fill { colors, rotation } => format!("fill:{rotation}:{}", hex_list(colors)),
            Spec::Image { path } => format!("image:{}", path.display()),
            Spec::Pattern { path, colors, rotation, intensity } => format!("pattern:{rotation}:{intensity}:{}:{}", hex_list(colors), path.display()),
        }
    }

    pub fn parse(v: &str) -> Self {
        let parts: Vec<&str> = v.splitn(5, ':').collect();
        let num = |s: &str| s.parse::<i32>().unwrap_or(0);
        match parts[..] {
            ["fill", rot, colors] if !parse_hex(colors).is_empty() => Spec::Fill { colors: parse_hex(colors), rotation: num(rot) },
            ["image", ..] => Spec::Image { path: PathBuf::from(&v["image:".len()..]) },
            ["pattern", rot, intensity, colors, path] => {
                Spec::Pattern { path: PathBuf::from(path), colors: parse_hex(colors), rotation: num(rot), intensity: num(intensity).clamp(-100, 100) }
            }
            _ => Spec::Live,
        }
    }

    /// The fill under a pattern, or the whole of a fill: shown at once while a file still downloads.
    pub fn fill(&self) -> Option<Spec> {
        match self {
            Spec::Fill { .. } => Some(self.clone()),
            Spec::Pattern { colors, rotation, .. } => Some(Spec::Fill { colors: colors.clone(), rotation: *rotation }),
            _ => None,
        }
    }
}

/// Draws `spec` at `w`×`h` pixels (an opaque image); `None` for the live background or a file we can't read.
pub fn render(spec: &Spec, w: u32, h: u32) -> Option<ColorImage> {
    let (w, h) = fit(w, h);
    match spec {
        Spec::Live => None,
        // Smooth: a quarter of the size, the GPU's linear filter stretches it.
        Spec::Fill { colors, rotation } => {
            let (fw, fh) = ((w / 4).max(8), (h / 4).max(8));
            Some(to_image(fw, fh, &fill(colors, *rotation, fw, fh)))
        }
        Spec::Image { path } => {
            let bytes = std::fs::read(path).ok()?;
            let img = image::load_from_memory(&bytes).ok()?;
            let rgba = img.resize_to_fill(w, h, image::imageops::FilterType::Triangle).to_rgba8();
            // Transparent pictures lie on black, as everything under them is ours to paint.
            let px: Vec<[u8; 3]> = rgba.pixels().map(|p| {
                let a = p[3] as u32;
                [(p[0] as u32 * a / 255) as u8, (p[1] as u32 * a / 255) as u8, (p[2] as u32 * a / 255) as u8]
            }).collect();
            Some(to_image(rgba.width(), rgba.height(), &px))
        }
        Spec::Pattern { path, colors, rotation, intensity } => {
            let colors = if colors.is_empty() { &DEFAULT_COLORS[..] } else { &colors[..] };
            let base = fill(colors, *rotation, w, h);
            let svg = read_pattern(&std::fs::read(path).ok()?)?;
            let (mask, tw) = pattern_mask(&svg, h)?;
            let k = (*intensity).clamp(-100, 100) as f32 / 100.0;
            let shift = (tw as i64 - w as i64) / 2;
            let px = base.iter().enumerate().map(|(i, c)| {
                let (x, y) = (i as u32 % w, i as u32 / w);
                let mx = (x as i64 + shift).rem_euclid(tw as i64) as u32;
                let a = mask[(y * tw + mx) as usize] as f32 / 255.0;
                let ch = |v: u8| {
                    let v = v as f32 / 255.0;
                    // Positive: the pattern darkens the fill as a soft-light black would (v → v²);
                    // negative: black, with the fill showing through the pattern.
                    let out = if k >= 0.0 { v + (v * v - v) * a * k } else { v * a * -k };
                    (out.clamp(0.0, 1.0) * 255.0).round() as u8
                };
                [ch(c[0]), ch(c[1]), ch(c[2])]
            }).collect::<Vec<_>>();
            Some(to_image(w, h, &px))
        }
    }
}

/// The size capped at `MAX_SIDE`, keeping the aspect.
fn fit(w: u32, h: u32) -> (u32, u32) {
    let (w, h) = (w.max(1), h.max(1));
    let s = (MAX_SIDE as f32 / w.max(h) as f32).min(1.0);
    (((w as f32 * s).round() as u32).max(1), ((h as f32 * s).round() as u32).max(1))
}

fn to_image(w: u32, h: u32, px: &[[u8; 3]]) -> ColorImage {
    let rgba: Vec<u8> = px.iter().flat_map(|p| [p[0], p[1], p[2], 255]).collect();
    ColorImage::from_rgba_unmultiplied([w as usize, h as usize], &rgba)
}

fn rgb(c: u32) -> [f32; 3] {
    [((c >> 16) & 0xff) as f32, ((c >> 8) & 0xff) as f32, (c & 0xff) as f32]
}

/// Where the colours of a freeform gradient sit (Telegram's first phase).
const POINTS: [(f32, f32); 4] = [(0.80, 0.10), (0.35, 0.25), (0.20, 0.90), (0.65, 0.75)];

/// Telegram's fills: one colour, a linear gradient turned by `rotation`, or freeform for three or four.
pub fn fill(colors: &[u32], rotation: i32, w: u32, h: u32) -> Vec<[u8; 3]> {
    let cols: Vec<[f32; 3]> = if colors.is_empty() { DEFAULT_COLORS.iter().map(|c| rgb(*c)).collect() } else { colors.iter().map(|c| rgb(*c)).collect() };
    let n = (w * h) as usize;
    let to = |c: [f32; 3]| [c[0].round() as u8, c[1].round() as u8, c[2].round() as u8];
    match cols.len() {
        1 => vec![to(cols[0]); n],
        2 => {
            // 0° runs from the top colour to the bottom one; turning is clockwise.
            let a = (rotation as f32).to_radians();
            let d = (-a.sin(), a.cos());
            let reach = 0.5 * (d.0.abs() + d.1.abs());
            (0..n).map(|i| {
                let (x, y) = ((i as u32 % w) as f32 / w as f32 - 0.5, (i as u32 / w) as f32 / h as f32 - 0.5);
                let t = (0.5 + (x * d.0 + y * d.1) / (2.0 * reach)).clamp(0.0, 1.0);
                to([0, 1, 2].map(|k| cols[0][k] + (cols[1][k] - cols[0][k]) * t))
            }).collect()
        }
        _ => (0..n).map(|i| {
            let (x, y) = ((i as u32 % w) as f32 / w as f32, (i as u32 / w) as f32 / h as f32);
            let (mut acc, mut total) = ([0.0f32; 3], 0.0f32);
            for (c, (px, py)) in cols.iter().zip(POINTS) {
                let d = (0.9 - ((x - px).powi(2) + (y - py).powi(2)).sqrt()).max(0.0);
                let wgt = d.powi(4);
                total += wgt;
                for k in 0..3 {
                    acc[k] += c[k] * wgt;
                }
            }
            if total <= 0.0 { to(cols[0]) } else { to(acc.map(|v| v / total)) }
        }).collect(),
    }
}

/// A pattern file is gzipped SVG (`.tgv`); plain SVG is taken as it is.
pub fn read_pattern(bytes: &[u8]) -> Option<String> {
    if bytes.starts_with(&[0x1f, 0x8b]) {
        let mut text = String::new();
        flate2::read::GzDecoder::new(bytes).take(MAX_SVG + 1).read_to_string(&mut text).ok()?;
        (text.len() as u64 <= MAX_SVG).then_some(text)
    } else {
        (bytes.len() as u64 <= MAX_SVG).then(|| String::from_utf8_lossy(bytes).into_owned())
    }
}

/// The pattern's coverage (0–255 per pixel) at `height` pixels, and its width.
pub fn pattern_mask(svg: &str, height: u32) -> Option<(Vec<u8>, u32)> {
    let doc = roxmltree::Document::parse(svg).ok()?;
    let root = doc.root_element();
    let num = |k: &str| root.attribute(k).and_then(|v| v.trim_end_matches("px").parse::<f64>().ok());
    let vb: Vec<f64> = root.attribute("viewBox").unwrap_or_default().split([' ', ',']).filter_map(|v| v.parse().ok()).collect();
    let (vx, vy, vw, vh) = match vb[..] {
        [x, y, w, h] if w > 0.0 && h > 0.0 => (x, y, w, h),
        _ => (0.0, 0.0, num("width").filter(|w| *w > 0.0)?, num("height").filter(|h| *h > 0.0)?),
    };
    let height = height.clamp(8, MAX_SIDE);
    let width = ((height as f64 * vw / vh).round() as u32).clamp(8, 4096);
    let mut ctx = RenderContext::new(width as u16, height as u16);
    ctx.set_paint(PaintType::Solid(vello_cpu::peniko::Color::BLACK));
    let base = Affine::scale_non_uniform(width as f64 / vw, height as f64 / vh) * Affine::translate((-vx, -vy));
    let mut shapes = 0;
    draw_node(&mut ctx, root, base, &mut shapes);
    let mut target = Pixmap::new(width as u16, height as u16);
    ctx.flush();
    ctx.render(&mut target, &mut Resources::new());
    Some((target.take_unpremultiplied().into_iter().map(|p| p.a).collect(), width))
}

fn draw_node(ctx: &mut RenderContext, node: roxmltree::Node, at: Affine, shapes: &mut usize) {
    for n in node.children().filter(|n| n.is_element()) {
        if *shapes >= MAX_SHAPES {
            return;
        }
        let at = at * n.attribute("transform").map(transform).unwrap_or(Affine::IDENTITY);
        let num = |k: &str| n.attribute(k).and_then(|v| v.trim_end_matches("px").parse::<f64>().ok()).unwrap_or(0.0);
        let path: Option<BezPath> = match n.tag_name().name() {
            "g" | "svg" => {
                draw_node(ctx, n, at, shapes);
                None
            }
            "path" => n.attribute("d").and_then(|d| BezPath::from_svg(d).ok()),
            "circle" => Some(kurbo::Circle::new((num("cx"), num("cy")), num("r")).to_path(0.1)),
            "ellipse" => Some(kurbo::Ellipse::new((num("cx"), num("cy")), (num("rx"), num("ry")), 0.0).to_path(0.1)),
            "rect" => Some(kurbo::Rect::new(num("x"), num("y"), num("x") + num("width"), num("y") + num("height")).to_rounded_rect(num("rx").max(num("ry"))).to_path(0.1)),
            tag @ ("polygon" | "polyline") => {
                let v: Vec<f64> = n.attribute("points").unwrap_or_default().split([' ', ',']).filter_map(|x| x.parse().ok()).collect();
                let mut p = BezPath::new();
                for (i, xy) in v.chunks_exact(2).enumerate() {
                    if i == 0 { p.move_to((xy[0], xy[1])) } else { p.line_to((xy[0], xy[1])) }
                }
                if tag == "polygon" {
                    p.close_path();
                }
                Some(p)
            }
            // Definitions, clips and masks aren't drawn by themselves.
            _ => None,
        };
        if let Some(p) = path.filter(|_| n.attribute("fill") != Some("none")) {
            *shapes += 1;
            ctx.set_transform(at);
            ctx.fill_path(&p);
        }
    }
}

/// An SVG `transform` list: matrix, translate, scale, rotate.
fn transform(v: &str) -> Affine {
    let mut at = Affine::IDENTITY;
    for part in v.split(')') {
        let Some((name, args)) = part.split_once('(') else { continue };
        let a: Vec<f64> = args.split([' ', ',']).filter_map(|x| x.parse().ok()).collect();
        let t = match (name.trim().trim_start_matches(','), &a[..]) {
            ("matrix", [a, b, c, d, e, f]) => Affine::new([*a, *b, *c, *d, *e, *f]),
            ("translate", [x]) => Affine::translate((*x, 0.0)),
            ("translate", [x, y]) => Affine::translate((*x, *y)),
            ("scale", [s]) => Affine::scale(*s),
            ("scale", [x, y]) => Affine::scale_non_uniform(*x, *y),
            ("rotate", [r]) => Affine::rotate(r.to_radians()),
            ("rotate", [r, x, y]) => Affine::translate((*x, *y)) * Affine::rotate(r.to_radians()) * Affine::translate((-x, -y)),
            _ => Affine::IDENTITY,
        };
        at *= t;
    }
    at
}

/// A made-up pattern for the demo: scattered stars, rings and dots, as plain SVG.
pub fn demo_pattern() -> String {
    let mut shapes = String::new();
    let mut seed = 7u32;
    let mut next = || {
        seed = seed.wrapping_mul(1_103_515_245).wrapping_add(12345);
        (seed >> 8) % 1000
    };
    for _ in 0..90 {
        let (x, y, r) = (next() as f32 * 0.72, next() as f32 * 1.48, 8.0 + next() as f32 / 40.0);
        match next() % 3 {
            0 => shapes.push_str(&format!("<circle cx=\"{x}\" cy=\"{y}\" r=\"{}\"/>", r * 0.5)),
            1 => shapes.push_str(&format!("<path d=\"M{} {y}a{r} {r} 0 1 0 {} 0a{r} {r} 0 1 0 {} 0zM{} {y}a{} {} 0 1 1 {} 0a{} {} 0 1 1 {} 0z\"/>", x - r, 2.0 * r, -2.0 * r, x - r * 0.6, r * 0.6, r * 0.6, 1.2 * r, r * 0.6, r * 0.6, -1.2 * r)),
            _ => shapes.push_str(&format!("<polygon points=\"{x},{} {},{} {},{} {},{} {},{}\" transform=\"rotate({} {x} {y})\"/>", y - r, x + r * 0.95, y - r * 0.3, x + r * 0.6, y + r * 0.8, x - r * 0.6, y + r * 0.8, x - r * 0.95, y - r * 0.3, next() % 72)),
        }
    }
    format!("<svg xmlns=\"http://www.w3.org/2000/svg\" viewBox=\"0 0 720 1480\">{shapes}</svg>")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn spec_round_trip() {
        let specs = [
            Spec::Live,
            Spec::Fill { colors: vec![0x112233], rotation: 0 },
            Spec::Fill { colors: vec![0xdbddbb, 0x6ba587, 0xd5d88d, 0x88b884], rotation: 45 },
            Spec::Image { path: PathBuf::from("/tmp/a:b.jpg") },
            Spec::Pattern { path: PathBuf::from("C:\\Users\\x\\w1.tgv"), colors: vec![0xff0000, 0x00ff00], rotation: 90, intensity: -50 },
        ];
        for s in specs {
            assert_eq!(Spec::parse(&s.code()), s);
        }
        assert_eq!(Spec::parse("junk"), Spec::Live);
    }

    #[test]
    fn fills_and_pattern() {
        assert_eq!(fill(&[0x102030], 0, 4, 4)[5], [0x10, 0x20, 0x30]);
        let lin = fill(&[0x000000, 0xffffff], 0, 2, 10);
        assert!(lin[0][0] < 30 && lin[19][0] > 225, "top dark, bottom light");
        let free = fill(&DEFAULT_COLORS, 0, 16, 16);
        assert_eq!(free.len(), 256);
        let (mask, w) = pattern_mask(&demo_pattern(), 148).unwrap();
        assert_eq!(w, 72);
        assert!(mask.iter().any(|a| *a > 200) && mask.iter().any(|a| *a == 0));
        assert!(transform("translate(10 20) scale(2)") == Affine::translate((10.0, 20.0)) * Affine::scale(2.0));
    }
}
