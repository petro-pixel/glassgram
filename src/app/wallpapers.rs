//! Chat backgrounds and themes, as in Telegram Desktop: Settings → Chat Settings → "Chat background"
//! (the server's gallery, a picture from a file, or our live background), a chat's own wallpaper and
//! theme from its full info, and "Wallpaper and theme" in a private chat's info to set them.
//! The background is content: drawn under the messages with no glass, the glass chrome stays on top.

use std::sync::Arc;

use eframe::egui::ColorImage;
use std::sync::mpsc::{Receiver, Sender, channel};

use super::*;
use crate::backend::{ChatTheme, ThemeKey, WallPaper};
use crate::wallpaper::{self, Spec as WallSpec};

/// Corners of the chat area's background, as the panels around it.
const RADIUS: f32 = 24.0;
/// A gallery tile, portrait like a phone screen.
const TILE: egui::Vec2 = vec2(92.0, 128.0);
/// A theme tile.
const THEME_TILE: egui::Vec2 = vec2(64.0, 84.0);
/// Drawn backgrounds kept at once (each is a texture of up to `wallpaper::MAX_SIDE`).
const KEEP_DRAWN: usize = 6;

/// A background drawn at one size.
struct Drawn {
    tex: TextureHandle,
    image: Arc<ColorImage>,
    size: [u32; 2],
    id: u64,
    used: f64,
    /// Mean luminance, 0–1.
    lum: f32,
}

/// A finished render: slot key, the size asked for, the image and its mean luminance.
type Job = (String, [u32; 2], Option<(ColorImage, f32)>);

/// Mean luminance of an image, from a sample of its pixels.
fn luminance(image: &ColorImage) -> f32 {
    let (mut sum, mut n) = (0.0f32, 0u32);
    for p in image.pixels.iter().step_by(7) {
        sum += (0.299 * p.r() as f32 + 0.587 * p.g() as f32 + 0.114 * p.b() as f32) / 255.0;
        n += 1;
    }
    if n == 0 { 0.5 } else { sum / n as f32 }
}

/// What a chat's full info says about its looks.
#[derive(Default)]
struct ChatLook {
    wall: Option<WallPaper>,
    theme: Option<ChatTheme>,
}

#[derive(Default)]
pub(super) struct Wallpapers {
    /// The server's gallery (`None` — not loaded).
    list: Option<Vec<WallPaper>>,
    list_asked: bool,
    themes: Option<Vec<ChatTheme>>,
    themes_asked: bool,
    /// Each chat's own wallpaper and theme; asked once per chat per run.
    chats: HashMap<PeerId, ChatLook>,
    asked: HashSet<PeerId>,
    /// Downloaded wallpaper documents.
    files: HashMap<i64, PathBuf>,
    downloading: HashSet<i64>,
    /// Drawn backgrounds by slot (spec code, plus "|tile" for previews).
    drawn: HashMap<String, Drawn>,
    /// Slots being drawn on a thread.
    jobs: HashSet<String>,
    /// Slots whose file couldn't be read: not tried again this run.
    failed: HashSet<String>,
    done: Option<(Sender<Job>, Receiver<Job>)>,
    /// Small fills under the gallery tiles, by colours.
    fills: HashMap<String, TextureHandle>,
    next_id: u64,
    /// The gallery box: `Some(None)` for the global background, `Some(Some(peer))` for a chat.
    pub(super) picker: Option<Option<PeerId>>,
    /// A gallery wallpaper chosen as the global background, waiting for its file.
    pending: Option<WallPaper>,
    /// Pictures picked from a file, already copied to the data folder.
    picked: Option<(Sender<PathBuf>, Receiver<PathBuf>)>,
}

/// UV of a texture cropped to fill `size` (centred).
fn cover_uv(tex: egui::Vec2, size: egui::Vec2) -> Rect {
    let (ta, ra) = (tex.x / tex.y.max(1.0), size.x / size.y.max(1.0));
    if ta > ra {
        let w = ra / ta;
        Rect::from_min_max(pos2((1.0 - w) / 2.0, 0.0), pos2((1.0 + w) / 2.0, 1.0))
    } else {
        let h = ta / ra;
        Rect::from_min_max(pos2(0.0, (1.0 - h) / 2.0), pos2(1.0, (1.0 + h) / 2.0))
    }
}

fn rounded_image(p: &egui::Painter, rect: Rect, radius: f32, tex: egui::TextureId, uv: Rect, tint: Color32) {
    p.add(egui::epaint::RectShape::filled(rect, radius, tint).with_texture(tex, uv));
}

/// The colour of a fill's middle, for a theme tile's accents and dark pattern lines.
fn mean(colors: &[u32]) -> Color32 {
    if colors.is_empty() {
        return Color32::GRAY;
    }
    let n = colors.len() as u32;
    let ch = |s: u32| ((colors.iter().map(|c| (c >> s) & 0xff).sum::<u32>()) / n) as u8;
    Color32::from_rgb(ch(16), ch(8), ch(0))
}

impl Wallpapers {
    fn ready(&mut self) -> &(Sender<Job>, Receiver<Job>) {
        self.done.get_or_insert_with(channel)
    }

    /// Takes finished renders into textures; old ones go when too many are kept.
    fn poll(&mut self, ctx: &egui::Context, now: f64) {
        let finished: Vec<Job> = self.ready().1.try_iter().collect();
        for (slot, size, image) in finished {
            self.jobs.remove(&slot);
            let Some((image, lum)) = image else {
                self.failed.insert(slot);
                continue;
            };
            self.next_id += 1;
            let image = Arc::new(image);
            let tex = ctx.load_texture(format!("wall-{}", self.next_id), (*image).clone(), TextureOptions::LINEAR);
            self.drawn.insert(slot, Drawn { tex, image, size, id: self.next_id, used: now, lum });
        }
        while self.drawn.len() > KEEP_DRAWN {
            let Some(oldest) = self.drawn.iter().min_by(|a, b| a.1.used.total_cmp(&b.1.used)).map(|(k, _)| k.clone()) else { break };
            self.drawn.remove(&oldest);
        }
    }

    /// The drawing of `spec` for `slot`, started on a thread when missing or far from `size` px.
    fn drawn_for(&mut self, ctx: &egui::Context, slot: String, spec: &WallSpec, size: [u32; 2], now: f64) -> Option<&Drawn> {
        let off = |d: &Drawn| {
            let r = |a: u32, b: u32| (a as f32 / b.max(1) as f32 - 1.0).abs();
            r(d.size[0], size[0]) > 0.12 || r(d.size[1], size[1]) > 0.12
        };
        if self.drawn.get(&slot).is_none_or(off) && !self.failed.contains(&slot) && self.jobs.insert(slot.clone()) {
            let (tx, ctx, spec, key) = (self.ready().0.clone(), ctx.clone(), spec.clone(), slot.clone());
            std::thread::spawn(move || {
                let image = wallpaper::render(&spec, size[0], size[1]).map(|i| {
                    let lum = luminance(&i);
                    (i, lum)
                });
                let _ = tx.send((key, size, image));
                ctx.request_repaint();
            });
        }
        let d = self.drawn.get_mut(&slot)?;
        d.used = now;
        Some(d)
    }

    /// A small texture of a fill, for gallery and theme tiles.
    fn fill_tex(&mut self, ctx: &egui::Context, colors: &[u32], rotation: i32) -> TextureHandle {
        let key = format!("{rotation}:{colors:?}");
        self.fills
            .entry(key)
            .or_insert_with(|| {
                let (w, h) = (24, 32);
                let px = wallpaper::fill(colors, rotation, w, h);
                let rgba: Vec<u8> = px.iter().flat_map(|p| [p[0], p[1], p[2], 255]).collect();
                ctx.load_texture("wall-fill", ColorImage::from_rgba_unmultiplied([w as usize, h as usize], &rgba), TextureOptions::LINEAR)
            })
            .clone()
    }

    /// A theme with its wallpapers: one known only by key is filled in from the list.
    fn full_theme(&self, t: &ChatTheme) -> ChatTheme {
        let listed = self.themes.as_ref().and_then(|l| l.iter().find(|x| x.key == t.key));
        match listed {
            Some(l) if t.light.is_none() && t.dark.is_none() => l.clone(),
            _ => t.clone(),
        }
    }
}

impl App {
    pub(super) fn wallpapers_event(&mut self, ev: Ev) {
        let w = &mut self.wallpapers;
        match ev {
            Ev::WallPapers(list) => w.list = Some(list),
            Ev::ChatThemes(list) => w.themes = Some(list),
            Ev::WallPaperFile { doc, path } => {
                w.downloading.remove(&doc);
                w.files.insert(doc, path.clone());
                if let Some(p) = w.pending.take_if(|p| p.doc == doc) {
                    self.settings.wallpaper = p.spec(Some(path));
                    self.settings.save();
                }
            }
            Ev::ChatWallPaper { peer, wall } => w.chats.entry(peer).or_default().wall = wall,
            Ev::ChatTheme { peer, theme } => {
                let known = theme.is_some();
                w.chats.entry(peer).or_default().theme = theme;
                if known {
                    self.ask_themes();
                }
            }
            _ => {}
        }
    }

    fn ask_themes(&mut self) {
        if !std::mem::replace(&mut self.wallpapers.themes_asked, true) {
            self.send(Cmd::LoadChatThemes);
        }
    }

    /// What to draw for a server wallpaper: its file once downloaded, its fill until then.
    fn wall_spec(&mut self, w: &WallPaper) -> WallSpec {
        if w.doc == 0 {
            return w.spec(None);
        }
        if let Some(path) = self.wallpapers.files.get(&w.doc) {
            return w.spec(Some(path.clone()));
        }
        // Downloaded in an earlier run.
        let file = w.file();
        if file.is_file() {
            self.wallpapers.files.insert(w.doc, file.clone());
            return w.spec(Some(file));
        }
        if self.wallpapers.downloading.insert(w.doc) {
            self.send(Cmd::DownloadWallPaper(w.clone()));
        }
        w.spec(None)
    }

    /// A theme's wallpaper for the current light or dark look.
    fn theme_wall(&self, t: &ChatTheme) -> Option<WallPaper> {
        let t = self.wallpapers.full_theme(t);
        if glass::is_light() { t.light.or(t.dark) } else { t.dark.or(t.light) }
    }

    /// The chat's own wallpaper, else its theme's, else the global one.
    fn chat_wall_spec(&mut self, peer: Option<PeerId>) -> WallSpec {
        let Some(peer) = peer else { return self.settings.wallpaper.clone() };
        let kind = self.chats.iter().find(|c| c.id == peer).map(|c| c.kind);
        if kind.is_some_and(|k| k != ChatKind::Saved) && self.wallpapers.asked.insert(peer) {
            self.send(Cmd::LoadChatWallPaper(peer));
        }
        let look = self.wallpapers.chats.get(&peer);
        let wall = look.and_then(|l| l.wall.clone());
        let theme = look.and_then(|l| l.theme.clone());
        if let Some(w) = wall.or_else(|| theme.and_then(|t| self.theme_wall(&t))) {
            return self.wall_spec(&w);
        }
        self.settings.wallpaper.clone()
    }

    /// The background under a chat (or the empty chat area): content, below everything else in it.
    pub(super) fn wallpaper_bg(&mut self, ui: &mut Ui, peer: Option<PeerId>, rect: Rect) {
        let now = ui.input(|i| i.time);
        self.wallpapers.poll(&self.ctx, now);
        let spec = self.chat_wall_spec(peer);
        if spec == WallSpec::Live {
            return;
        }
        let ppp = ui.ctx().pixels_per_point();
        let size = [(rect.width() * ppp) as u32, (rect.height() * ppp) as u32];
        let ctx = self.ctx.clone();
        // A pattern still downloading shows its fill meanwhile.
        let take = |d: &Drawn| (d.tex.clone(), d.image.clone(), d.id, d.lum);
        let shown = match self.wallpapers.drawn_for(&ctx, spec.code(), &spec, size, now) {
            Some(d) => Some(take(d)),
            None => spec.fill().filter(|f| *f != spec).and_then(|f| self.wallpapers.drawn_for(&ctx, f.code(), &f, size, now).map(take)),
        };
        if let Some((tex, image, id, lum)) = shown {
            let veil = glass::wallpaper_veil(lum);
            glass::wallpaper(ui.painter(), rect, RADIUS, &tex, crate::blur::WallLayer { rect, radius: RADIUS, image, id, veil });
        }
    }

    /// A preview of a background in `rect`; the live one as a plain tile with our colours.
    fn spec_preview(&mut self, ui: &Ui, rect: Rect, spec: &WallSpec) {
        let p = ui.painter();
        if *spec == WallSpec::Live {
            let tex = self.wallpapers.fill_tex(&self.ctx, &if glass::is_light() { [0xeef1ff, 0xd6ddf8, 0xc9b8ff, 0x9fd8f0] } else { [0x10122c, 0x281c6e, 0x060816, 0x0c3a52] }, 0);
            rounded_image(p, rect, 12.0, tex.id(), Rect::from_min_max(pos2(0.0, 0.0), pos2(1.0, 1.0)), Color32::WHITE);
            p.text(rect.center(), Align2::CENTER_CENTER, l!("Живий", "Live"), FontId::proportional(12.5), ink());
            return;
        }
        let ppp = ui.ctx().pixels_per_point();
        let size = [(rect.width() * ppp) as u32, (rect.height() * ppp) as u32];
        let (ctx, now) = (self.ctx.clone(), ui.input(|i| i.time));
        match self.wallpapers.drawn_for(&ctx, format!("{}|tile", spec.code()), spec, size, now) {
            Some(d) => rounded_image(p, rect, 12.0, d.tex.id(), Rect::from_min_max(pos2(0.0, 0.0), pos2(1.0, 1.0)), Color32::WHITE),
            None => {
                p.rect_filled(rect, 12.0, glass::veil(20));
            }
        }
    }

    /// A gallery tile of a server wallpaper: its fill and thumbnail.
    fn wall_tile(&mut self, ui: &mut Ui, w: &WallPaper, selected: bool, id: impl std::hash::Hash) -> bool {
        let (rect, resp) = ui.allocate_exact_size(TILE, Sense::click());
        let p = ui.painter().clone();
        let full = Rect::from_min_max(pos2(0.0, 0.0), pos2(1.0, 1.0));
        let thumb = self.textures.get(&ImgKey::Wall(w.doc)).cloned();
        if w.pattern || w.doc == 0 {
            let colors = if w.colors.is_empty() { wallpaper::DEFAULT_COLORS.to_vec() } else { w.colors.clone() };
            if w.intensity < 0 {
                p.rect_filled(rect, 12.0, Color32::BLACK);
            } else {
                let fill = self.wallpapers.fill_tex(&self.ctx, &colors, w.rotation);
                rounded_image(&p, rect, 12.0, fill.id(), full, Color32::WHITE);
            }
            if let Some(t) = thumb.filter(|_| w.doc != 0) {
                let k = (w.intensity.unsigned_abs().min(100) as f32 / 100.0).max(0.25);
                // A pattern's thumbnail is its shapes: dark on a light fill, the fill's colour on black.
                let tint = if w.intensity < 0 { mean(&colors).gamma_multiply(k) } else { Color32::from_black_alpha((k * 160.0) as u8) };
                rounded_image(&p, rect, 12.0, t.id(), cover_uv(t.size_vec2(), rect.size()), tint);
            }
        } else if let Some(t) = thumb {
            rounded_image(&p, rect, 12.0, t.id(), cover_uv(t.size_vec2(), rect.size()), Color32::WHITE);
        } else {
            p.rect_filled(rect, 12.0, glass::veil(20));
        }
        self.tile_frame(ui, rect, &resp, selected, id);
        resp.clicked()
    }

    /// Hover and the selection ring around a tile.
    fn tile_frame(&self, ui: &Ui, rect: Rect, resp: &egui::Response, selected: bool, _id: impl std::hash::Hash) {
        let p = ui.painter();
        if selected {
            p.rect_stroke(rect.expand(2.0), 14.0, egui::Stroke::new(2.5, ACCENT), egui::StrokeKind::Outside);
        } else if resp.hovered() {
            p.rect_stroke(rect, 12.0, egui::Stroke::new(1.5, glass::veil(120)), egui::StrokeKind::Inside);
        }
        if resp.hovered() {
            ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
        }
    }

    /// A tile with a word on it: "Live", "None".
    fn word_tile(&mut self, ui: &mut Ui, size: egui::Vec2, word: &str, selected: bool, live: bool) -> bool {
        let (rect, resp) = ui.allocate_exact_size(size, Sense::click());
        if live {
            self.spec_preview(ui, rect, &WallSpec::Live);
        } else {
            ui.painter().rect_filled(rect, 12.0, glass::veil(22));
            ui.painter().text(rect.center(), Align2::CENTER_CENTER, word, FontId::proportional(12.5), ink());
        }
        self.tile_frame(ui, rect, &resp, selected, word);
        resp.clicked()
    }

    /// A theme tile: its wallpaper's fill with the emoji, or the gift's name.
    fn theme_tile(&mut self, ui: &mut Ui, t: &ChatTheme, selected: bool) -> bool {
        let (rect, resp) = ui.allocate_exact_size(THEME_TILE, Sense::click());
        let p = ui.painter().clone();
        let wall = self.theme_wall(t);
        let (colors, rotation) = wall.as_ref().filter(|w| !w.colors.is_empty()).map_or_else(
            || {
                let a = if glass::is_light() { t.accent.0 } else { t.accent.1 };
                (vec![if a == 0 { 0x6ba587 } else { a }], 0)
            },
            |w| (w.colors.clone(), w.rotation),
        );
        let fill = self.wallpapers.fill_tex(&self.ctx, &colors, rotation);
        rounded_image(&p, rect, 12.0, fill.id(), Rect::from_min_max(pos2(0.0, 0.0), pos2(1.0, 1.0)), Color32::WHITE);
        if t.emoticon.is_empty() {
            // A gift theme: its name on a dark chip, readable on any fill.
            let chip = Rect::from_min_max(pos2(rect.left() + 4.0, rect.bottom() - 26.0), pos2(rect.right() - 4.0, rect.bottom() - 4.0));
            p.rect_filled(chip, 11.0, Color32::from_black_alpha(140));
            let g = elided(ui, &t.title, 11.0, glass::ON_DARK, chip.width() - 8.0);
            p.galley(pos2(chip.center().x - g.size().x / 2.0, chip.center().y - g.size().y / 2.0), g, glass::ON_DARK);
            p.text(pos2(rect.center().x, rect.top() + 28.0), Align2::CENTER_CENTER, "🎁", FontId::proportional(24.0), glass::ON_DARK);
        } else {
            p.text(rect.center(), Align2::CENTER_CENTER, &t.emoticon, FontId::proportional(28.0), glass::ON_DARK);
        }
        self.tile_frame(ui, rect, &resp, selected, &t.title);
        resp.on_hover_text(&t.title).clicked()
    }

    /// Settings → Chat Settings → "Chat background": the current one and where to choose another.
    pub(super) fn background_section(&mut self, ui: &mut Ui) {
        section_title(ui, l!("Фон чату", "Chat background"));
        let take: Vec<PathBuf> = self.wallpapers.picked.as_ref().map(|(_, rx)| rx.try_iter().collect()).unwrap_or_default();
        if let Some(path) = take.into_iter().last() {
            self.settings.wallpaper = WallSpec::Image { path };
        }
        let spec = self.settings.wallpaper.clone();
        ui.horizontal(|ui| {
            ui.add_space(12.0);
            let (rect, _) = ui.allocate_exact_size(vec2(72.0, 100.0), Sense::hover());
            self.spec_preview(ui, rect, &spec);
            ui.add_space(12.0);
            ui.vertical(|ui| {
                let w = (ui.available_width() - 12.0).min(260.0);
                let row = |ui: &mut Ui, id: &str, label: &str| {
                    let (r, _) = ui.allocate_exact_size(vec2(w, 32.0), Sense::hover());
                    glass::button(ui, r, id, label, false, true).clicked()
                };
                if row(ui, "wall-gallery", l!("Обрати з галереї", "Choose from gallery")) {
                    self.open_wallpapers(None);
                }
                if row(ui, "wall-file", l!("Обрати з файлу", "Choose from file")) {
                    self.pick_wallpaper_file();
                }
                if spec != WallSpec::Live && row(ui, "wall-live", l!("Живий фон Glassgram", "Glassgram live background")) {
                    self.settings.wallpaper = WallSpec::Live;
                }
            });
        });
        if self.settings.reduce_motion && spec == WallSpec::Live {
            hint_text(ui, l!("Живий фон стоїть на місці: увімкнено «Зменшити рух».", "The live background stands still: \"Reduce motion\" is on."));
        }
    }

    /// Opens the gallery box, asking for what it shows.
    pub(super) fn open_wallpapers(&mut self, peer: Option<PeerId>) {
        self.wallpapers.picker = Some(peer);
        if !std::mem::replace(&mut self.wallpapers.list_asked, true) {
            self.send(Cmd::LoadWallPapers);
        }
        if peer.is_some() {
            self.ask_themes();
        }
    }

    /// A picture from disk, copied into the data folder so it outlives the original.
    fn pick_wallpaper_file(&mut self) {
        let tx = self.wallpapers.picked.get_or_insert_with(channel).0.clone();
        let ctx = self.ctx.clone();
        let title = l!("Фон чату", "Chat background");
        std::thread::spawn(move || {
            let Some(file) = rfd::FileDialog::new().set_title(title).add_filter("Images", &["jpg", "jpeg", "png", "webp"]).pick_file() else { return };
            let Ok(bytes) = std::fs::read(&file) else { return };
            if image::load_from_memory(&bytes).is_err() {
                return;
            }
            let dir = backend::data_dir().join("wallpapers");
            let _ = std::fs::create_dir_all(&dir);
            // A fresh name each time, so a cached drawing of the previous picture isn't reused.
            if let Ok(old) = std::fs::read_dir(&dir) {
                for e in old.flatten().filter(|e| e.file_name().to_string_lossy().starts_with("custom-")) {
                    let _ = std::fs::remove_file(e.path());
                }
            }
            let ext = file.extension().and_then(|e| e.to_str()).unwrap_or("jpg").to_lowercase();
            let stamp = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or_default();
            let path = dir.join(format!("custom-{stamp}.{ext}"));
            if std::fs::write(&path, &bytes).is_ok() {
                let _ = tx.send(path);
                ctx.request_repaint();
            }
        });
    }

    /// The info row of a private chat: its theme and wallpaper, opening the box.
    pub(super) fn wallpaper_label(&self, peer: PeerId) -> String {
        let look = self.wallpapers.chats.get(&peer);
        let theme = look.and_then(|l| l.theme.as_ref()).map(|t| self.wallpapers.full_theme(t).title);
        let own = look.is_some_and(|l| l.wall.is_some());
        match (theme, own) {
            (Some(t), true) => lf!("Тема {t} · свій фон", "Theme {t} · own wallpaper"),
            (Some(t), false) => lf!("Тема {t}", "Theme {t}"),
            (None, true) => l!("Свій фон", "Own wallpaper").to_owned(),
            (None, false) => l!("Як усюди", "Same as everywhere").to_owned(),
        }
    }

    /// The gallery box: themes (for a chat) and wallpapers.
    pub(super) fn wallpaper_picker_view(&mut self, ui: &mut Ui) {
        let Some(peer) = self.wallpapers.picker else { return };
        let screen = ui.max_rect();
        ui.painter().rect_filled(screen, 0.0, Color32::from_black_alpha(90));
        ui.interact(screen, ui.scope_id().with("wall-picker-bg"), Sense::click());
        let rect = Rect::from_center_size(screen.center(), vec2(screen.width().min(560.0) - 20.0, (screen.height() - 60.0).min(620.0)));
        glass::glass(ui.painter(), rect, Glass::panel(26.0).with_shadow());
        let inner = rect.shrink(18.0);
        let title = if peer.is_some() { l!("Фон і тема чату", "Chat wallpaper and theme") } else { l!("Фон чату", "Chat background") };
        ui.painter().text(pos2(inner.left() + 4.0, inner.top() + 12.0), Align2::LEFT_CENTER, title, FontId::proportional(17.0), ink());
        let close = Rect::from_min_size(pos2(inner.right() - 32.0, inner.top() - 4.0), vec2(32.0, 32.0));
        let mut done = glass::icon_button(ui, close, "wall-close", Icon::Close, false, true).clicked() || ui.input(|i| i.key_pressed(Key::Escape));
        let footer = if peer.is_none() { 44.0 } else { 0.0 };
        let body = Rect::from_min_max(pos2(inner.left(), inner.top() + 40.0), pos2(inner.right(), inner.bottom() - footer));
        let mut child = ui.new_child(UiBuilder::new().max_rect(body));
        child.set_clip_rect(body);
        let private = peer.is_some_and(|p| self.chats.iter().any(|c| c.id == p && c.kind == ChatKind::User));
        let mut chose_wall: Option<Option<WallPaper>> = None;
        let mut chose_theme: Option<Option<ThemeKey>> = None;
        let mut live = false;
        egui::ScrollArea::vertical().id_salt("wall-picker").auto_shrink(false).show(&mut child, |ui| {
            egui::Frame::NONE.inner_margin(egui::Margin { left: 8, right: 4, top: 0, bottom: 8 }).show(ui, |ui| {
            if let Some(peer) = peer.filter(|_| private) {
                section_title(ui, l!("Тема", "Theme"));
                let current = self.wallpapers.chats.get(&peer).and_then(|l| l.theme.as_ref()).and_then(|t| t.key.clone());
                let themes = self.wallpapers.themes.clone();
                ui.horizontal_wrapped(|ui| {
                    ui.spacing_mut().item_spacing = vec2(8.0, 8.0);
                    if self.word_tile(ui, THEME_TILE, l!("Без теми", "No theme"), current.is_none(), false) {
                        chose_theme = Some(None);
                    }
                    for t in themes.iter().flatten() {
                        if self.theme_tile(ui, t, t.key == current) {
                            chose_theme = Some(t.key.clone());
                        }
                    }
                    if themes.is_none() {
                        ui.add(egui::Spinner::new().color(ink()));
                    }
                });
                ui.add_space(6.0);
            }
            section_title(ui, l!("Фони", "Wallpapers"));
            let own = peer.and_then(|p| self.wallpapers.chats.get(&p)).and_then(|l| l.wall.as_ref()).map(|w| w.id);
            let global = self.settings.wallpaper.clone();
            let list = self.wallpapers.list.clone();
            ui.horizontal_wrapped(|ui| {
                ui.spacing_mut().item_spacing = vec2(8.0, 8.0);
                match peer {
                    None if self.word_tile(ui, TILE, "", global == WallSpec::Live, true) => live = true,
                    Some(_) if private && self.word_tile(ui, TILE, l!("Як усюди", "Same as everywhere"), own.is_none(), false) => chose_wall = Some(None),
                    _ => {}
                }
                for w in list.iter().flatten() {
                    let selected = match peer {
                        Some(_) => own == Some(w.id),
                        None => match &self.wallpapers.pending {
                            Some(p) => p.id == w.id,
                            None if w.doc == 0 => w.spec(None) == global,
                            None => self.wallpapers.files.get(&w.doc).is_some_and(|p| w.spec(Some(p.clone())) == global),
                        },
                    };
                    if self.wall_tile(ui, w, selected, w.id) && (peer.is_none() || private) {
                        chose_wall = Some(Some(w.clone()));
                    }
                }
                if list.is_none() {
                    ui.add(egui::Spinner::new().color(ink()));
                }
            });
            if peer.is_some() && !private {
                hint_text(ui, l!("Свій фон і тему можна задати лише в особистому чаті.", "A chat's own wallpaper and theme can be set only in a private chat."));
            }
            });
        });
        if peer.is_none() {
            let b = Rect::from_min_size(pos2(inner.left(), inner.bottom() - 34.0), vec2(inner.width(), 34.0));
            if glass::button(ui, b, "wall-from-file", l!("Обрати з файлу", "Choose from file"), false, true).clicked() {
                self.pick_wallpaper_file();
                done = true;
            }
        }
        if live {
            self.wallpapers.pending = None;
            self.settings.wallpaper = WallSpec::Live;
            self.settings.save();
        }
        match (peer, chose_wall) {
            (None, Some(Some(w))) => {
                let spec = self.wall_spec(&w);
                if w.doc != 0 && !self.wallpapers.files.contains_key(&w.doc) {
                    // Applied when the file arrives; the tile shows it as chosen meanwhile.
                    self.wallpapers.pending = Some(w);
                } else {
                    self.wallpapers.pending = None;
                    self.settings.wallpaper = spec;
                    self.settings.save();
                }
            }
            (Some(peer), Some(wall)) => self.send(Cmd::SetChatWallPaper { peer, wall }),
            _ => {}
        }
        if let (Some(peer), Some(theme)) = (peer, chose_theme) {
            self.send(Cmd::SetChatTheme { peer, theme });
        }
        if done {
            self.wallpapers.picker = None;
        }
    }
}

fn hint_text(ui: &mut Ui, text: &str) {
    ui.horizontal(|ui| {
        ui.add_space(12.0);
        ui.label(egui::RichText::new(text).color(ink_dim()).size(12.5));
    });
}
