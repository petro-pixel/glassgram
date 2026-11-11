//! "QR code" box for a public username, as in Telegram Desktop 7.3.2: https://t.me/<name> on a
//! choice of gradient designs, to copy the link, save the picture or copy it as an image.
//! The picture is drawn on the CPU (`vello_cpu`), so the preview and the saved file are one image.

use super::*;
use crate::draw::text_path;
use crate::platform;
use eframe::egui::{ColorImage, Pos2, Vec2};
use vello_cpu::kurbo::{self, Affine, BezPath, Circle, RoundedRect, Shape as _};
use vello_cpu::peniko::{Color, Fill, Gradient};
use vello_cpu::{Pixmap, RenderContext, Resources};

/// A design: the background gradient (top-left to bottom-right) and the code's own gradient.
struct Design {
    bg: [[u8; 3]; 3],
    ink: [[u8; 3]; 2],
}

/// The designs of Telegram Desktop's box, close enough: blue, green, orange, pink, ocean, night.
const DESIGNS: [Design; 6] = [
    Design { bg: [[0x6c, 0xb8, 0xf7], [0x4a, 0x84, 0xf0], [0x7a, 0x62, 0xec]], ink: [[0x2a, 0x6b, 0xdc], [0x5b, 0x46, 0xc9]] },
    Design { bg: [[0xb4, 0xe0, 0x6e], [0x5c, 0xc4, 0x8a], [0x2f, 0xa8, 0xa0]], ink: [[0x3a, 0x9a, 0x4c], [0x1d, 0x80, 0x82]] },
    Design { bg: [[0xfb, 0xd1, 0x6a], [0xf6, 0x95, 0x4e], [0xec, 0x5f, 0x5a]], ink: [[0xe0, 0x7a, 0x22], [0xc9, 0x3e, 0x3e]] },
    Design { bg: [[0xf7, 0xa1, 0xc8], [0xe0, 0x6f, 0xc4], [0x9a, 0x62, 0xe6]], ink: [[0xd2, 0x4a, 0x9a], [0x7d, 0x45, 0xc9]] },
    Design { bg: [[0x6f, 0xe3, 0xe0], [0x3c, 0xae, 0xe0], [0x3a, 0x6c, 0xd6]], ink: [[0x14, 0x9a, 0xb8], [0x2a, 0x56, 0xb8]] },
    Design { bg: [[0x4b, 0x52, 0x7a], [0x2c, 0x31, 0x52], [0x17, 0x1a, 0x30]], ink: [[0x2e, 0x35, 0x5c], [0x14, 0x17, 0x2e]] },
];

/// Picture size in points; drawn `SCALE` times larger, for the file and a sharp preview.
const W: f64 = 320.0;
const H: f64 = 400.0;
const SCALE: f64 = 3.0;

/// Whose code the box shows.
struct Open {
    /// Without "@"; empty — no public username yet.
    username: String,
    /// Our own profile: offer to set a username when there is none.
    own: bool,
}

/// The rendered picture for one username and design.
struct Shot {
    username: String,
    design: usize,
    size: [usize; 2],
    /// RGBA, not premultiplied.
    rgba: Vec<u8>,
    tex: TextureHandle,
}

#[derive(Default)]
pub(super) struct UserQr {
    /// `None` — the box is closed.
    open: Option<Open>,
    /// The chosen design, kept between openings.
    design: usize,
    shot: Option<Shot>,
    /// Kept alive: on Linux an image stays in the clipboard only while its owner exists.
    clipboard: Option<arboard::Clipboard>,
}

impl App {
    /// Opens the box for `username` (empty: explain how to get one).
    pub(super) fn open_user_qr(&mut self, username: &str, own: bool) {
        self.user_qr.open = Some(Open { username: username.trim_start_matches('@').to_owned(), own });
    }

    pub(super) fn user_qr_view(&mut self, ui: &mut Ui) {
        let Some(open) = &self.user_qr.open else { return };
        let (username, own) = (open.username.clone(), open.own);
        let screen = ui.max_rect();
        ui.painter().rect_filled(screen, 0.0, Color32::from_black_alpha(90));
        let bg = ui.interact(screen, ui.scope_id().with("user-qr-bg"), Sense::click());
        let esc = ui.input_mut(|i| i.consume_key(egui::Modifiers::NONE, Key::Escape));
        if username.is_empty() {
            self.user_qr_none(ui, screen, own, esc || bg.clicked());
            return;
        }
        // The preview shrinks on small windows; the rest of the box is fixed.
        let preview_h = (screen.height() - 270.0).clamp(160.0, 300.0);
        let preview = vec2(preview_h * (W / H) as f32, preview_h);
        let panel = Rect::from_center_size(screen.center(), vec2(400.0, preview_h + 222.0));
        glass::glass(ui.painter(), panel, Glass::panel(24.0).with_shadow());
        ui.interact(panel, ui.scope_id().with("user-qr-panel"), Sense::click());
        let inner = panel.shrink(18.0);
        ui.painter().text(inner.left_top(), Align2::LEFT_TOP, l!("QR-код", "QR code"), FontId::proportional(17.0), ink());
        let close = Rect::from_center_size(pos2(panel.right() - 30.0, panel.top() + 28.0), vec2(34.0, 34.0));
        if glass::icon_button(ui, close, "user-qr-close", Icon::Close, false, true).clicked() || esc || bg.clicked() {
            self.user_qr.open = None;
            return;
        }

        let q = &mut self.user_qr;
        if !q.shot.as_ref().is_some_and(|s| s.username == username && s.design == q.design) {
            q.shot = render(&username, &DESIGNS[q.design % DESIGNS.len()]).map(|(size, rgba)| {
                let image = ColorImage::from_rgba_unmultiplied(size, &rgba);
                let tex = ui.ctx().load_texture("user-qr", image, TextureOptions::LINEAR);
                Shot { username: username.clone(), design: q.design, size, rgba, tex }
            });
        }
        let pic = Rect::from_min_size(pos2(panel.center().x - preview.x / 2.0, inner.top() + 40.0), preview);
        if let Some(s) = &q.shot {
            egui::Image::new((s.tex.id(), preview)).corner_radius(18.0).paint_at(ui, pic);
        }

        // Swatches: each design's gradient in a circle, the chosen one ringed.
        let n = DESIGNS.len() as f32;
        let (d, gap) = (32.0, 12.0);
        let row_y = pic.bottom() + 16.0 + d / 2.0;
        let left = panel.center().x - (n * d + (n - 1.0) * gap) / 2.0 + d / 2.0;
        for (k, design) in DESIGNS.iter().enumerate() {
            let c = pos2(left + k as f32 * (d + gap), row_y);
            let r = Rect::from_center_size(c, vec2(d + 6.0, d + 6.0));
            let resp = ui.interact(r, ui.scope_id().with(("user-qr-design", k)), Sense::click());
            let p = ui.painter();
            let [a, _, b] = design.bg.map(|[r, g, b]| Color32::from_rgb(r, g, b));
            p.circle_filled(c, d / 2.0, a);
            // Lower-right half in the gradient's end colour.
            let half: Vec<Pos2> = (0..=16).map(|i| c + Vec2::angled(-std::f32::consts::FRAC_PI_4 + i as f32 * std::f32::consts::PI / 16.0) * (d / 2.0)).collect();
            p.add(egui::Shape::convex_polygon(half, b, egui::Stroke::NONE));
            if k == q.design {
                p.circle_stroke(c, d / 2.0 + 3.0, egui::Stroke::new(2.0, ink()));
            }
            if resp.hovered() {
                ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
            }
            if resp.on_hover_text(design_name(k)).clicked() {
                q.design = k;
            }
        }

        let link = format!("https://t.me/{username}");
        let half = (inner.width() - 8.0) / 2.0;
        let y = row_y + d / 2.0 + 16.0;
        let copy_link = Rect::from_min_size(pos2(inner.left(), y), vec2(half, 36.0));
        let copy_image = Rect::from_min_size(pos2(inner.right() - half, y), vec2(half, 36.0));
        let save = Rect::from_min_size(pos2(inner.left(), y + 44.0), vec2(inner.width(), 36.0));
        let ready = q.shot.is_some();
        if glass::button(ui, copy_link, "user-qr-link", l!("Копіювати посилання", "Copy link"), false, true).clicked() {
            ui.ctx().copy_text(link);
            self.toast = Some((l!("Посилання скопійовано", "Link copied").into(), 0.0, false));
        }
        if glass::button(ui, copy_image, "user-qr-copy", l!("Копіювати зображення", "Copy image"), false, ready).clicked() {
            let done = self.user_qr.copy_image();
            self.toast = Some(match done {
                Ok(()) => (l!("Зображення скопійовано", "Image copied").into(), 0.0, false),
                Err(e) => (e, 0.0, true),
            });
        }
        if glass::button(ui, save, "user-qr-save", l!("Зберегти як зображення", "Save as image"), true, ready).clicked() {
            let done = self.user_qr.save_png();
            self.toast = Some(match done {
                Ok(path) => (lf!("Збережено: {}", "Saved: {}", path.display()), 0.0, false),
                Err(e) => (e, 0.0, true),
            });
        }
    }

    /// No public username: say what it is for and, for us, where to set one.
    fn user_qr_none(&mut self, ui: &mut Ui, screen: Rect, own: bool, close: bool) {
        let panel = Rect::from_center_size(screen.center(), vec2(360.0, if own { 200.0 } else { 160.0 }));
        glass::glass(ui.painter(), panel, Glass::panel(24.0).with_shadow());
        ui.interact(panel, ui.scope_id().with("user-qr-panel"), Sense::click());
        let inner = panel.shrink(18.0);
        let p = ui.painter();
        p.text(inner.left_top(), Align2::LEFT_TOP, l!("QR-код", "QR code"), FontId::proportional(17.0), ink());
        let text = if own {
            l!(
                "У вас ще немає імені користувача. Задайте його, і вас знайдуть за посиланням t.me та QR-кодом без номера телефону.",
                "You don't have a username yet. Set one so people can find you by a t.me link and a QR code without your phone number."
            )
        } else {
            l!("У цього чату немає публічного імені, тож немає й QR-коду.", "This chat has no public username, so it has no QR code.")
        };
        let g = p.layout(text.to_owned(), FontId::proportional(14.0), ink_dim(), inner.width());
        p.galley(pos2(inner.left(), inner.top() + 36.0), g, ink_dim());
        let by = inner.bottom() - 36.0;
        if own {
            let half = (inner.width() - 8.0) / 2.0;
            let cancel = glass::button(ui, Rect::from_min_size(pos2(inner.left(), by), vec2(half, 36.0)), "user-qr-cancel", l!("Скасувати", "Cancel"), false, true).clicked();
            let set = Rect::from_min_size(pos2(inner.right() - half, by), vec2(half, 36.0));
            if glass::button(ui, set, "user-qr-set", l!("Задати ім'я", "Set username"), true, true).clicked() {
                self.user_qr.open = None;
                self.settings_open = true;
                self.open_settings_page(SettingsTab::Account);
            } else if cancel || close {
                self.user_qr.open = None;
            }
        } else {
            let ok = glass::button(ui, Rect::from_min_size(pos2(inner.left(), by), vec2(inner.width(), 36.0)), "user-qr-ok", l!("Зрозуміло", "OK"), false, true).clicked();
            if ok || close {
                self.user_qr.open = None;
            }
        }
    }
}

impl UserQr {
    pub(super) fn is_open(&self) -> bool {
        self.open.is_some()
    }

    /// The picture as a PNG in the downloads folder, under a name that doesn't overwrite another.
    fn save_png(&self) -> Result<PathBuf, String> {
        let s = self.shot.as_ref().ok_or_else(|| l!("Зображення ще не готове", "The image isn't ready yet").to_owned())?;
        let dir = platform::download_dir();
        std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
        let stem = platform::safe_file_name(&format!("{}-qr", s.username));
        let path = (0..1000)
            .map(|k| dir.join(if k == 0 { format!("{stem}.png") } else { format!("{stem} ({k}).png") }))
            .find(|p| !p.exists())
            .ok_or_else(|| l!("Не вдалося підібрати ім'я файлу", "Couldn't pick a file name").to_owned())?;
        image::save_buffer(&path, &s.rgba, s.size[0] as u32, s.size[1] as u32, image::ExtendedColorType::Rgba8).map_err(|e| e.to_string())?;
        Ok(path)
    }

    fn copy_image(&mut self) -> Result<(), String> {
        let s = self.shot.as_ref().ok_or_else(|| l!("Зображення ще не готове", "The image isn't ready yet").to_owned())?;
        if self.clipboard.is_none() {
            self.clipboard = Some(arboard::Clipboard::new().map_err(|e| e.to_string())?);
        }
        let clip = self.clipboard.as_mut().ok_or_else(|| "clipboard".to_owned())?;
        let image = arboard::ImageData { width: s.size[0], height: s.size[1], bytes: std::borrow::Cow::Borrowed(&s.rgba) };
        clip.set_image(image).map_err(|e| e.to_string())
    }
}

fn design_name(k: usize) -> &'static str {
    match k {
        0 => l!("Синій", "Blue"),
        1 => l!("Зелений", "Green"),
        2 => l!("Помаранчевий", "Orange"),
        3 => l!("Рожевий", "Pink"),
        4 => l!("Океан", "Ocean"),
        _ => l!("Ніч", "Night"),
    }
}

fn rgb([r, g, b]: [u8; 3]) -> Color {
    Color::from_rgb8(r, g, b)
}

/// Draws the card: the gradient background, a white plate with the code and the logo in its middle,
/// and "@USERNAME" under it. Returns the size in pixels and RGBA (not premultiplied).
fn render(username: &str, design: &Design) -> Option<([usize; 2], Vec<u8>)> {
    // Level H survives the logo covering the middle of the code.
    let code = qrcode::QrCode::with_error_correction_level(format!("https://t.me/{username}").as_bytes(), qrcode::EcLevel::H).ok()?;
    let (w, h) = ((W * SCALE) as u16, (H * SCALE) as u16);
    let mut ctx = RenderContext::new(w, h);
    ctx.set_transform(Affine::scale(SCALE));

    ctx.set_paint(Gradient::new_linear((0.0, 0.0), (W, H)).with_stops(design.bg.map(rgb)));
    ctx.fill_rect(&kurbo::Rect::new(0.0, 0.0, W, H));
    let card = kurbo::Rect::new(32.0, 40.0, W - 32.0, H - 40.0);
    ctx.set_paint(Color::from_rgba8(0, 0, 0, 46));
    ctx.fill_blurred_rounded_rect(&(card + kurbo::Vec2::new(0.0, 6.0)), 28.0, 10.0, false);
    ctx.set_paint(Color::from_rgb8(255, 255, 255));
    ctx.fill_path(&RoundedRect::from_rect(card, 28.0).to_path(0.05));

    let area = kurbo::Rect::from_origin_size((card.x0 + 22.0, card.y0 + 22.0), (card.width() - 44.0, card.width() - 44.0));
    ctx.set_paint(Gradient::new_linear((area.x0, area.y0), (area.x1, area.y1)).with_stops(design.ink.map(rgb)));
    let logo = modules(&mut ctx, &code, area);

    // Telegram's paper plane on a disc in the design's colours.
    let r = logo / 2.0 * 0.86;
    let c = area.center();
    ctx.set_paint(Gradient::new_linear((c.x - r, c.y - r), (c.x + r, c.y + r)).with_stops(design.bg.map(rgb)));
    ctx.fill_path(&Circle::new(c, r).to_path(0.05));
    ctx.set_paint(Color::from_rgb8(255, 255, 255));
    ctx.fill_path(&(Affine::translate((c.x, c.y)) * Affine::scale(r / 12.0) * Affine::translate((-12.0, -12.0)) * plane()));

    // The caption fits the plate: long usernames get a smaller size.
    let caption = format!("@{}", username.to_uppercase());
    let mut size = 22.0;
    let (mut path, mut width) = text_path(&caption, size)?;
    let room = card.width() - 32.0;
    if width > room {
        size *= room / width;
        (path, width) = text_path(&caption, size)?;
    }
    let base = area.y1 + (card.y1 - area.y1) / 2.0 + size * 0.36;
    ctx.set_paint(Gradient::new_linear((card.x0, 0.0), (card.x1, 0.0)).with_stops(design.ink.map(rgb)));
    ctx.fill_path(&(Affine::translate((card.center().x - width / 2.0, base)) * path));

    let mut target = Pixmap::new(w, h);
    ctx.flush();
    ctx.render(&mut target, &mut Resources::new());
    let rgba = target.take_unpremultiplied().into_iter().flat_map(|p| [p.r, p.g, p.b, p.a]).collect();
    Some(([w as usize, h as usize], rgba))
}

/// Rounded modules that flow into their neighbours, with rounded finder squares; the middle is left
/// empty for the logo. Returns the side of that empty square.
fn modules(ctx: &mut RenderContext, code: &qrcode::QrCode, area: kurbo::Rect) -> f64 {
    let n = code.width();
    let colors = code.to_colors();
    let cell = area.width() / n as f64;
    // An odd number of modules, about a quarter of the side.
    let hole = ((n as f64 * 0.24) as usize) | 1;
    let lo = (n - hole) / 2;
    let empty = |x: usize, y: usize| (x < 8 && y < 8) || (x + 8 >= n && y < 8) || (x < 8 && y + 8 >= n) || ((lo..lo + hole).contains(&x) && (lo..lo + hole).contains(&y));
    let dark = |x: usize, y: usize| x < n && y < n && colors[y * n + x] == qrcode::Color::Dark && !empty(x, y);
    let at = |x: usize, y: usize| kurbo::Point::new(area.x0 + (x as f64 + 0.5) * cell, area.y0 + (y as f64 + 0.5) * cell);
    let mut path = BezPath::new();
    let r = cell * 0.5;
    for y in 0..n {
        for x in 0..n {
            if !dark(x, y) {
                continue;
            }
            let c = at(x, y);
            path.extend(Circle::new(c, r).path_elements(0.02));
            // Bridges to the right and down neighbours, so runs read as one rounded shape.
            if dark(x + 1, y) {
                path.extend(kurbo::Rect::new(c.x, c.y - r, c.x + cell, c.y + r).path_elements(0.02));
            }
            if dark(x, y + 1) {
                path.extend(kurbo::Rect::new(c.x - r, c.y, c.x + r, c.y + cell).path_elements(0.02));
            }
        }
    }
    ctx.fill_path(&path);
    // Finder squares: a rounded ring and a rounded dot. Rounder corners look nicer but some
    // scanners find the code's corners from them and then misread it.
    let mut ring = BezPath::new();
    for (fx, fy) in [(0, 0), (n - 7, 0), (0, n - 7)] {
        let o = kurbo::Point::new(area.x0 + fx as f64 * cell, area.y0 + fy as f64 * cell);
        let sq = |inset: f64, radius: f64| RoundedRect::from_rect(kurbo::Rect::from_origin_size(o + (inset * cell, inset * cell), ((7.0 - 2.0 * inset) * cell, (7.0 - 2.0 * inset) * cell)), radius * cell);
        ring.extend(sq(0.0, 0.8).path_elements(0.02));
        ring.extend(sq(1.0, 0.5).path_elements(0.02));
        ring.extend(sq(2.0, 0.4).path_elements(0.02));
    }
    ctx.set_fill_rule(Fill::EvenOdd);
    ctx.fill_path(&ring);
    ctx.set_fill_rule(Fill::NonZero);
    hole as f64 * cell
}

/// Telegram's paper plane on a 24×24 grid.
fn plane() -> BezPath {
    let pts = [(4.6, 11.4), (18.4, 6.0), (16.2, 18.0), (12.2, 15.1), (10.0, 17.2), (9.9, 13.7), (15.4, 8.7), (8.4, 12.9)];
    let mut p = BezPath::new();
    p.move_to(pts[0]);
    for q in &pts[1..] {
        p.line_to(*q);
    }
    p.close_path();
    p
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn renders_every_design() {
        for d in &DESIGNS {
            let ([w, h], rgba) = render("glassgram_demo", d).expect("render");
            assert_eq!((w, h), ((W * SCALE) as usize, (H * SCALE) as usize));
            assert_eq!(rgba.len(), w * h * 4);
            // The plate is white just inside its top edge.
            let i = ((50.0 * SCALE) as usize * w + w / 2) * 4;
            assert_eq!(&rgba[i..i + 3], &[255, 255, 255]);
        }
    }

    #[test]
    fn long_usernames_fit() {
        // The longest username Telegram allows still makes a level-H code and a caption.
        assert!(render(&"a".repeat(32), &DESIGNS[0]).is_some());
        let (_, w) = text_path("@GLASSGRAM", 22.0).expect("font");
        assert!(w > 50.0 && w < 200.0);
    }
}
