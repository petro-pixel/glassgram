//! A simple photo editor for the "Send files" box, laid out like Telegram Desktop's: crop with
//! handles and aspect presets, turning and mirroring, freehand drawing and text. The edit is kept
//! as data over the original file, so opening the editor again continues it; "Done" renders it on
//! a thread into a temporary file that takes the photo's place in the box.

use super::*;
use eframe::egui::{ColorImage, Pos2, Response, Vec2};
use image::RgbaImage;

mod render;

/// Drawing colours. They are the picture's content, not UI tokens, so they stay the same in both themes.
const PALETTE: [[u8; 3]; 8] = [[255, 255, 255], [0, 0, 0], [235, 70, 70], [255, 149, 0], [255, 214, 10], [52, 199, 89], [10, 132, 255], [175, 82, 222]];
/// Brush and text sizes as fractions of the picture's long side.
const BRUSH: (f32, f32) = (0.004, 0.04);
const TEXT: (f32, f32) = (0.03, 0.2);

#[derive(Clone, Copy, PartialEq, Default)]
pub(super) enum TextStyle {
    #[default]
    Plain,
    /// Letters with a contrasting outline: readable on any part of the photo.
    Outline,
    /// Letters on a rounded plate of the colour.
    Chip,
}

#[derive(Clone, Copy, PartialEq, Default)]
pub(super) enum Font {
    #[default]
    Bold,
    Regular,
    Italic,
}

impl Font {
    fn data(self) -> &'static [u8] {
        match self {
            Font::Bold => format::UBUNTU_BOLD,
            Font::Regular => format::UBUNTU_REGULAR,
            Font::Italic => format::UBUNTU_ITALIC,
        }
    }

    fn family(self) -> egui::FontFamily {
        match self {
            Font::Bold => egui::FontFamily::Name(format::BOLD.into()),
            Font::Regular => egui::FontFamily::Proportional,
            Font::Italic => egui::FontFamily::Name(format::ITALIC.into()),
        }
    }
}

/// A freehand line. Points are fractions of the turned picture, so cropping never moves them.
#[derive(Clone)]
pub(super) struct Stroke {
    points: Vec<[f32; 2]>,
    color: [u8; 3],
    width: f32,
}

#[derive(Clone)]
pub(super) struct TextItem {
    text: String,
    /// Centre, as a fraction of the turned picture.
    pos: [f32; 2],
    /// Font size as a fraction of the picture's long side.
    size: f32,
    color: [u8; 3],
    style: TextStyle,
    font: Font,
}

/// Everything done to the photo. Turns come first, then the mirror; crop, strokes and text live in
/// the turned picture's coordinates (0..1) and are turned along with it.
#[derive(Clone)]
pub(super) struct Edit {
    /// Quarter turns clockwise.
    rot: u8,
    flip: bool,
    /// Left, top, right, bottom.
    crop: [f32; 4],
    strokes: Vec<Stroke>,
    texts: Vec<TextItem>,
}

impl Default for Edit {
    fn default() -> Self {
        Self { rot: 0, flip: false, crop: [0.0, 0.0, 1.0, 1.0], strokes: Vec::new(), texts: Vec::new() }
    }
}

impl Edit {
    fn changed(&self) -> bool {
        self.rot % 4 != 0 || self.flip || self.crop != [0.0, 0.0, 1.0, 1.0] || !self.strokes.is_empty() || self.texts.iter().any(|t| !t.text.trim().is_empty())
    }

    /// Moves every point as the picture moves under it.
    fn map_points(&mut self, f: impl Fn([f32; 2]) -> [f32; 2]) {
        let [x0, y0, x1, y1] = self.crop;
        let (a, b) = (f([x0, y0]), f([x1, y1]));
        self.crop = [a[0].min(b[0]), a[1].min(b[1]), a[0].max(b[0]), a[1].max(b[1])];
        for p in self.strokes.iter_mut().flat_map(|s| s.points.iter_mut()) {
            *p = f(*p);
        }
        for t in &mut self.texts {
            t.pos = f(t.pos);
        }
    }

    pub(super) fn rotate_left(&mut self) {
        // Turning the mirrored picture left is a right turn of the original under the mirror.
        self.rot = (self.rot + if self.flip { 1 } else { 3 }) % 4;
        self.map_points(|[x, y]| [y, 1.0 - x]);
    }

    fn mirror(&mut self) {
        self.flip = !self.flip;
        self.map_points(|[x, y]| [1.0 - x, y]);
    }
}

#[derive(Clone, Copy, PartialEq)]
enum Mode {
    Crop,
    Draw,
    Text,
}

#[derive(Clone, Copy, PartialEq)]
enum Aspect {
    Free,
    Original,
    Ratio(f32, f32),
}

const ASPECTS: [Aspect; 6] = [Aspect::Free, Aspect::Original, Aspect::Ratio(1.0, 1.0), Aspect::Ratio(3.0, 2.0), Aspect::Ratio(4.0, 3.0), Aspect::Ratio(16.0, 9.0)];

impl Aspect {
    fn label(self) -> String {
        match self {
            Aspect::Free => l!("Вільно", "Free").to_owned(),
            Aspect::Original => l!("Оригінал", "Original").to_owned(),
            Aspect::Ratio(w, h) => format!("{w}:{h}"),
        }
    }
}

enum Drag {
    /// Crop edges being pulled (-1 left/top, 1 right/bottom, 0 untouched), or the whole frame moved.
    Crop { sx: i8, sy: i8, start: [f32; 4], from: [f32; 2] },
    Stroke,
    Text { index: usize, offset: [f32; 2] },
}

/// What the editor ended with this frame.
pub(super) enum Outcome {
    Cancel,
    Done { file: PathBuf, edit: Edit },
    Failed(String),
}

/// Where the shown part of the picture lands on screen.
struct View {
    rect: Rect,
    /// The part shown, in the turned picture's coordinates.
    region: Rect,
}

impl View {
    fn fit(canvas: Rect, region: Rect, size: Vec2) -> Self {
        let px = vec2(region.width() * size.x, region.height() * size.y);
        let scale = (canvas.width() / px.x).min(canvas.height() / px.y).max(0.01);
        Self { rect: Rect::from_center_size(canvas.center(), px * scale), region }
    }

    fn to_screen(&self, p: [f32; 2]) -> Pos2 {
        self.rect.min + (vec2(p[0], p[1]) - self.region.min.to_vec2()) / self.region.size() * self.rect.size()
    }

    fn to_norm(&self, s: Pos2) -> [f32; 2] {
        let v = self.region.min + (s - self.rect.min) / self.rect.size() * self.region.size();
        [v.x, v.y]
    }

    /// The picture's long side on screen.
    fn long(&self) -> f32 {
        (self.rect.width() / self.region.width()).max(self.rect.height() / self.region.height())
    }
}

pub(super) struct PhotoEditor {
    /// The file in the send box the result replaces, and the original the edit applies to.
    pub(super) target: PathBuf,
    source: PathBuf,
    loading: Option<mpsc::Receiver<Result<RgbaImage, String>>>,
    base: Option<RgbaImage>,
    /// `base` turned and mirrored as the edit says, and the turn it was made for.
    tex: Option<(TextureHandle, u8, bool)>,
    edit: Edit,
    undo: Vec<Edit>,
    mode: Mode,
    aspect: Aspect,
    color: [u8; 3],
    brush: f32,
    style: TextStyle,
    font: Font,
    /// The text being edited; its field takes focus once.
    selected: Option<usize>,
    focus: bool,
    /// The text whose typing has an undo step already.
    typing: Option<usize>,
    drag: Option<Drag>,
    /// Text pictures by their look and on-screen size.
    text_tex: HashMap<u64, TextureHandle>,
    saving: Option<mpsc::Receiver<Result<PathBuf, String>>>,
}

impl PhotoEditor {
    pub(super) fn open(ctx: &egui::Context, target: PathBuf, source: PathBuf, edit: Option<Edit>) -> Self {
        let (tx, rx) = mpsc::channel();
        let (path, ctx) = (source.clone(), ctx.clone());
        std::thread::spawn(move || {
            let _ = tx.send(render::preview(&path));
            ctx.request_repaint();
        });
        Self {
            target,
            source,
            loading: Some(rx),
            base: None,
            tex: None,
            edit: edit.unwrap_or_default(),
            undo: Vec::new(),
            mode: Mode::Crop,
            aspect: Aspect::Free,
            color: PALETTE[2],
            brush: 0.012,
            style: TextStyle::Outline,
            font: Font::Bold,
            selected: None,
            focus: false,
            typing: None,
            drag: None,
            text_tex: HashMap::new(),
            saving: None,
        }
    }

    fn push_undo(&mut self) {
        self.undo.push(self.edit.clone());
        if self.undo.len() > 100 {
            self.undo.remove(0);
        }
    }

    fn undo(&mut self) {
        if let Some(e) = self.undo.pop() {
            self.edit = e;
            self.selected = None;
            self.typing = None;
        }
    }

    /// The turned preview's size in pixels.
    fn size(&self) -> Vec2 {
        self.tex.as_ref().map_or(vec2(1.0, 1.0), |(t, _, _)| t.size_vec2())
    }

    /// Fits the crop to an aspect, as large as the picture allows around the frame's centre.
    fn apply_aspect(&mut self) {
        let size = self.size();
        let [x0, y0, x1, y1] = self.edit.crop;
        let ratio = match self.aspect {
            Aspect::Free => return,
            Aspect::Original => size.x / size.y,
            // A portrait frame keeps its orientation: 4:3 becomes 3:4.
            Aspect::Ratio(w, h) if (y1 - y0) * size.y > (x1 - x0) * size.x => h / w,
            Aspect::Ratio(w, h) => w / h,
        };
        let (mut w, mut h) = (size.x, size.x / ratio);
        if h > size.y {
            (w, h) = (size.y * ratio, size.y);
        }
        let (nw, nh) = (w / size.x, h / size.y);
        let (cx, cy) = (((x0 + x1) / 2.0).clamp(nw / 2.0, 1.0 - nw / 2.0), ((y0 + y1) / 2.0).clamp(nh / 2.0, 1.0 - nh / 2.0));
        self.edit.crop = [cx - nw / 2.0, cy - nh / 2.0, cx + nw / 2.0, cy + nh / 2.0];
    }

    /// The pixel ratio a corner drag keeps, if any.
    fn locked_ratio(&self) -> Option<f32> {
        let [x0, y0, x1, y1] = self.edit.crop;
        let size = self.size();
        (self.aspect != Aspect::Free).then(|| ((x1 - x0) * size.x) / ((y1 - y0) * size.y).max(1e-3))
    }

    fn set_color(&mut self, c: [u8; 3]) {
        self.color = c;
        self.restyle(|t| t.color = c);
    }

    /// Changes the selected text's look along with the tool's.
    fn restyle(&mut self, f: impl Fn(&mut TextItem)) {
        if self.mode == Mode::Text && self.selected.is_some_and(|i| i < self.edit.texts.len()) {
            self.push_undo();
            if let Some(t) = self.selected.and_then(|i| self.edit.texts.get_mut(i)) {
                f(t);
            }
        }
    }

    fn add_text(&mut self, pos: [f32; 2]) {
        self.drop_empty();
        self.push_undo();
        self.edit.texts.push(TextItem { text: String::new(), pos, size: 0.07, color: self.color, style: self.style, font: self.font });
        self.selected = Some(self.edit.texts.len() - 1);
        self.focus = true;
        self.typing = self.selected;
    }

    /// A text left empty disappears when another one is picked.
    fn drop_empty(&mut self) {
        if let Some(i) = self.selected.take().filter(|&i| self.edit.texts.get(i).is_some_and(|t| t.text.trim().is_empty())) {
            self.edit.texts.remove(i);
        }
        self.typing = None;
    }

    /// Selects text `i`, dropping the empty one selected before; returns where `i` ended up.
    fn select(&mut self, i: usize) -> usize {
        if self.selected == Some(i) {
            return i;
        }
        let before = self.edit.texts.len();
        let old = self.selected;
        self.drop_empty();
        let i = if self.edit.texts.len() < before && old.is_some_and(|o| o < i) { i - 1 } else { i };
        self.selected = Some(i);
        // The tools show the picked text's look.
        if let Some(t) = self.edit.texts.get(i) {
            (self.color, self.style, self.font) = (t.color, t.style, t.font);
        }
        i
    }

    /// The editor over the whole window. Returns how it ended, once it has.
    pub(super) fn view(&mut self, ui: &mut Ui) -> Option<Outcome> {
        if let Some(r) = self.loading.as_ref().and_then(|rx| rx.try_recv().ok()) {
            self.loading = None;
            match r {
                Ok(img) => self.base = Some(img),
                Err(e) => return Some(Outcome::Failed(e)),
            }
        }
        if let Some(r) = self.saving.as_ref().and_then(|rx| rx.try_recv().ok()) {
            self.saving = None;
            return Some(match r {
                Ok(file) => Outcome::Done { file, edit: self.edit.clone() },
                Err(e) => Outcome::Failed(e),
            });
        }
        if let Some(base) = &self.base
            && self.tex.as_ref().is_none_or(|(_, r, f)| (*r, *f) != (self.edit.rot % 4, self.edit.flip))
        {
            let img = render::turn(base, self.edit.rot, self.edit.flip);
            let ci = ColorImage::from_rgba_unmultiplied([img.width() as usize, img.height() as usize], img.as_raw());
            self.tex = Some((ui.ctx().load_texture("photo-editor", ci, TextureOptions::LINEAR), self.edit.rot % 4, self.edit.flip));
        }
        let screen = ui.max_rect();
        ui.painter().rect_filled(screen, 0.0, Color32::from_black_alpha(215));
        ui.interact(screen, ui.scope_id().with("photo-editor-bg"), Sense::click());
        let tabs = Rect::from_center_size(pos2(screen.center().x, screen.top() + 38.0), vec2(3.0 * 112.0 + 8.0, 44.0));
        self.tabs(ui, tabs);
        let bar = Rect::from_center_size(pos2(screen.center().x, screen.bottom() - 46.0), vec2((screen.width() - 32.0).min(780.0), 60.0));
        let mut canvas = Rect::from_min_max(pos2(screen.left() + 24.0, tabs.bottom() + 16.0), pos2(screen.right() - 24.0, bar.top() - 16.0));
        if self.mode == Mode::Text && self.selected.is_some() {
            let extra = Rect::from_center_size(pos2(screen.center().x, bar.top() - 34.0), vec2(bar.width().min(560.0), 52.0));
            canvas.max.y = extra.top() - 12.0;
            self.text_bar(ui, extra);
        }
        let outcome = self.bar(ui, bar);
        if self.tex.is_some() {
            self.canvas(ui, canvas);
        } else {
            bubbles::spinner(ui.painter(), canvas.center(), 14.0);
        }
        outcome.or_else(|| self.keys(ui))
    }

    /// Esc, Enter and Ctrl+Z, unless a text field has them.
    fn keys(&mut self, ui: &mut Ui) -> Option<Outcome> {
        if ui.memory(|m| m.focused().is_some()) {
            return None;
        }
        if ui.input_mut(|i| i.consume_key(egui::Modifiers::COMMAND, Key::Z)) {
            self.undo();
        }
        if ui.input_mut(|i| i.consume_key(egui::Modifiers::NONE, Key::Escape)) {
            return Some(Outcome::Cancel);
        }
        if ui.input_mut(|i| i.consume_key(egui::Modifiers::NONE, Key::Enter)) {
            return self.done(ui.ctx());
        }
        None
    }

    fn done(&mut self, ctx: &egui::Context) -> Option<Outcome> {
        if self.saving.is_some() || self.tex.is_none() {
            return None;
        }
        self.drop_empty();
        if !self.edit.changed() {
            return Some(Outcome::Cancel);
        }
        let (tx, rx) = mpsc::channel();
        let (path, edit, ctx) = (self.source.clone(), self.edit.clone(), ctx.clone());
        std::thread::spawn(move || {
            // The editor was closed meanwhile: nobody will send the file.
            if let Err(mpsc::SendError(Ok(file))) = tx.send(render::render(&path, &edit)) {
                let _ = std::fs::remove_file(file);
            }
            ctx.request_repaint();
        });
        self.saving = Some(rx);
        None
    }

    /// Crop / Draw / Text, a segmented capsule at the top.
    fn tabs(&mut self, ui: &mut Ui, rect: Rect) {
        glass::glass(ui.painter(), rect, Glass::panel(rect.height() / 2.0).with_shadow());
        let modes = [(Mode::Crop, Icon::Crop, l!("Обрізати", "Crop")), (Mode::Draw, Icon::Brush, l!("Малювати", "Draw")), (Mode::Text, Icon::Text, l!("Текст", "Text"))];
        for (k, (mode, icon, label)) in modes.into_iter().enumerate() {
            let r = Rect::from_min_size(pos2(rect.left() + 4.0 + k as f32 * 112.0, rect.top() + 4.0), vec2(112.0, rect.height() - 8.0));
            let resp = ui.interact(r, ui.scope_id().with(("photo-editor-tab", k)), Sense::click());
            let on = self.mode == mode;
            if on {
                glass::glass(ui.painter(), r, glass::accent_glass(r, true, 0.0));
            } else if resp.hovered() {
                ui.painter().rect_filled(r, r.height() / 2.0, glass::veil(16));
            }
            let color = if on { glass::ON_DARK } else { ink() };
            let g = ui.painter().layout_no_wrap(label.to_owned(), FontId::proportional(14.0), color);
            let x = r.center().x - (g.size().x + 24.0) / 2.0;
            icons::draw(ui.painter(), Rect::from_min_size(pos2(x, r.center().y - 9.0), vec2(18.0, 18.0)), icon, color);
            ui.painter().galley(pos2(x + 24.0, r.center().y - g.size().y / 2.0), g, color);
            if resp.clicked() && !on {
                self.drop_empty();
                self.mode = mode;
                self.drag = None;
            }
            if resp.hovered() {
                ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
            }
        }
    }

    /// The bottom bar: Cancel, the mode's tools, Done.
    fn bar(&mut self, ui: &mut Ui, rect: Rect) -> Option<Outcome> {
        glass::glass(ui.painter(), rect, Glass::panel(rect.height() / 2.0).with_shadow());
        let side = 44.0;
        let cancel = Rect::from_center_size(pos2(rect.left() + 8.0 + side / 2.0, rect.center().y), Vec2::splat(side));
        let done = Rect::from_center_size(pos2(rect.right() - 8.0 - side / 2.0, rect.center().y), Vec2::splat(side));
        let mut out = None;
        if glass::icon_button(ui, cancel, "photo-editor-cancel", Icon::Close, false, true).on_hover_text(l!("Скасувати", "Cancel")).clicked() {
            out = Some(Outcome::Cancel);
        }
        if self.saving.is_some() {
            glass::glass(ui.painter(), done, glass::accent_glass(done, false, 0.0));
            bubbles::spinner(ui.painter(), done.center(), 8.0);
            ui.ctx().request_repaint();
        } else if glass::icon_button(ui, done, "photo-editor-done", Icon::Check, true, self.tex.is_some()).on_hover_text(l!("Готово", "Done")).clicked() {
            out = self.done(ui.ctx());
        }
        let tools = Rect::from_min_max(pos2(cancel.right() + 12.0, rect.top()), pos2(done.left() - 12.0, rect.bottom()));
        let mut child = ui.new_child(UiBuilder::new().max_rect(tools).layout(egui::Layout::left_to_right(Align::Center)));
        child.set_clip_rect(tools);
        child.spacing_mut().item_spacing.x = 6.0;
        match self.mode {
            Mode::Crop => self.crop_tools(&mut child),
            Mode::Draw => self.draw_tools(&mut child),
            Mode::Text => self.text_tools(&mut child),
        }
        out
    }

    fn tool(ui: &mut Ui, id: &str, icon: Icon, hint: &str, enabled: bool) -> bool {
        let (r, _) = ui.allocate_exact_size(vec2(40.0, 40.0), Sense::hover());
        glass::icon_button(ui, r, ("photo-editor", id), icon, false, enabled).on_hover_text(hint).clicked()
    }

    fn crop_tools(&mut self, ui: &mut Ui) {
        if Self::tool(ui, "rotate", Icon::RotateCcw, l!("Повернути", "Rotate"), true) {
            self.push_undo();
            self.edit.rotate_left();
        }
        if Self::tool(ui, "flip", Icon::Flip, l!("Віддзеркалити", "Flip"), true) {
            self.push_undo();
            self.edit.mirror();
        }
        ui.add_space(6.0);
        for a in ASPECTS {
            if info::chip(ui, &a.label(), self.aspect == a).clicked() {
                self.push_undo();
                self.aspect = a;
                self.apply_aspect();
            }
        }
        ui.add_space(6.0);
        let reset = self.edit.crop != [0.0, 0.0, 1.0, 1.0] || self.edit.rot % 4 != 0 || self.edit.flip;
        if Self::tool(ui, "reset", Icon::Reload, l!("Скинути", "Reset"), reset) {
            self.push_undo();
            // Turn back to the original, keeping what was drawn in place on the photo.
            while self.edit.rot % 4 != 0 || self.edit.flip {
                if self.edit.flip {
                    self.edit.mirror();
                } else {
                    self.edit.rotate_left();
                }
            }
            self.edit.crop = [0.0, 0.0, 1.0, 1.0];
            self.aspect = Aspect::Free;
        }
    }

    fn swatches(&mut self, ui: &mut Ui) {
        for c in PALETTE {
            let (r, resp) = ui.allocate_exact_size(vec2(28.0, 40.0), Sense::click());
            let p = ui.painter();
            if self.color == c {
                p.circle_stroke(r.center(), 13.0, egui::Stroke::new(2.0, ink()));
            }
            // A thin ring keeps white and black visible on either theme's glass.
            p.circle_filled(r.center(), 9.5, Color32::from_rgb(c[0], c[1], c[2]));
            p.circle_stroke(r.center(), 9.5, egui::Stroke::new(1.0, glass::veil(60)));
            if resp.hovered() {
                ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
            }
            if resp.clicked() {
                self.set_color(c);
            }
        }
    }

    fn draw_tools(&mut self, ui: &mut Ui) {
        self.swatches(ui);
        ui.add_space(8.0);
        slider(ui, &mut self.brush, BRUSH, 120.0);
        let dot = 3.0 + 11.0 * (self.brush - BRUSH.0) / (BRUSH.1 - BRUSH.0);
        let (r, _) = ui.allocate_exact_size(vec2(30.0, 40.0), Sense::hover());
        ui.painter().circle_filled(r.center(), dot, Color32::from_rgb(self.color[0], self.color[1], self.color[2]));
        ui.add_space(4.0);
        let can = !self.undo.is_empty();
        if Self::tool(ui, "undo", Icon::Undo, l!("Скасувати дію", "Undo"), can) {
            self.undo();
        }
    }

    fn text_tools(&mut self, ui: &mut Ui) {
        if Self::tool(ui, "add-text", Icon::Plus, l!("Додати текст", "Add text"), true) {
            let [x0, y0, x1, y1] = self.edit.crop;
            self.add_text([(x0 + x1) / 2.0, (y0 + y1) / 2.0]);
        }
        self.swatches(ui);
        ui.add_space(4.0);
        let styles = [(TextStyle::Plain, l!("Звичайний", "Plain")), (TextStyle::Outline, l!("Контур", "Outline")), (TextStyle::Chip, l!("Плашка", "Chip"))];
        for (s, label) in styles {
            if info::chip(ui, label, self.style == s).clicked() {
                self.style = s;
                self.restyle(|t| t.style = s);
            }
        }
        ui.add_space(4.0);
        for f in [Font::Bold, Font::Regular, Font::Italic] {
            if font_chip(ui, f, self.font == f).clicked() {
                self.font = f;
                self.restyle(|t| t.font = f);
            }
        }
    }

    /// The selected text's field, size and delete, above the bar.
    fn text_bar(&mut self, ui: &mut Ui, rect: Rect) {
        let Some(i) = self.selected.filter(|&i| i < self.edit.texts.len()) else { return };
        glass::glass(ui.painter(), rect, Glass::panel(rect.height() / 2.0).with_shadow());
        let inner = rect.shrink2(vec2(8.0, 6.0));
        let del = Rect::from_center_size(pos2(inner.right() - 20.0, inner.center().y), vec2(40.0, 40.0));
        let size = Rect::from_min_max(pos2(del.left() - 136.0, inner.top()), pos2(del.left() - 8.0, inner.bottom()));
        let field = Rect::from_min_max(inner.min, pos2(size.left() - 8.0, inner.bottom()));
        let mut text = self.edit.texts[i].text.clone();
        let resp = glass::text_field(ui, field, &mut text, l!("Текст", "Text"), false);
        if self.focus {
            self.focus = false;
            resp.request_focus();
        }
        if text != self.edit.texts[i].text {
            if self.typing != Some(i) {
                self.push_undo();
                self.typing = Some(i);
            }
            self.edit.texts[i].text = text;
        }
        let mut child = ui.new_child(UiBuilder::new().max_rect(size).layout(egui::Layout::left_to_right(Align::Center)));
        let mut s = self.edit.texts[i].size;
        if slider(&mut child, &mut s, TEXT, size.width()) {
            self.edit.texts[i].size = s;
        }
        if glass::icon_button(ui, del, "photo-editor-del-text", Icon::Trash, false, true).on_hover_text(l!("Видалити", "Delete")).clicked() {
            self.push_undo();
            self.edit.texts.remove(i);
            self.selected = None;
            self.typing = None;
        }
    }

    /// The picture with everything on it, and the mode's handling of the pointer.
    fn canvas(&mut self, ui: &mut Ui, canvas: Rect) {
        let size = self.size();
        let [x0, y0, x1, y1] = self.edit.crop;
        let crop = Rect::from_min_max(pos2(x0, y0), pos2(x1, y1));
        let view = View::fit(canvas, if self.mode == Mode::Crop { Rect::from_min_max(Pos2::ZERO, pos2(1.0, 1.0)) } else { crop }, size);
        let sense = if self.mode == Mode::Text { Sense::click_and_drag() } else { Sense::drag() };
        let resp = ui.interact(view.rect, ui.scope_id().with("photo-editor-canvas"), sense);
        let boxes = self.text_boxes(ui, &view);
        match self.mode {
            Mode::Crop => self.crop_input(ui, &resp, &view),
            Mode::Draw => self.draw_input(&resp, &view),
            Mode::Text => self.text_input(&resp, &view, &boxes),
        }
        let p = ui.painter_at(view.rect);
        if let Some((tex, _, _)) = &self.tex {
            p.image(tex.id(), view.rect, view.region, Color32::WHITE);
        }
        let long = view.long();
        for s in &self.edit.strokes {
            let c = Color32::from_rgb(s.color[0], s.color[1], s.color[2]);
            let w = (s.width * long).max(1.0);
            let pts: Vec<Pos2> = s.points.iter().map(|q| view.to_screen(*q)).collect();
            if pts.len() > 1 {
                p.add(egui::Shape::line(pts.clone(), egui::Stroke::new(w, c)));
            }
            // Round joins and caps, as the result draws them.
            for q in pts {
                p.circle_filled(q, w / 2.0, c);
            }
        }
        let ppp = ui.ctx().pixels_per_point();
        for (i, r) in boxes.iter().enumerate() {
            let t = &self.edit.texts[i];
            match self.text_tex.get(&text_key(t, t.size * long * ppp)) {
                Some(tex) if !t.text.is_empty() => {
                    p.image(tex.id(), *r, Rect::from_min_max(Pos2::ZERO, pos2(1.0, 1.0)), Color32::WHITE);
                }
                _ => {
                    p.text(r.center(), Align2::CENTER_CENTER, l!("Текст", "Text"), FontId::new((t.size * long).max(8.0), t.font.family()), rgba(glass::ON_DARK, 150));
                }
            }
            if self.mode == Mode::Text && self.selected == Some(i) {
                p.rect_stroke(r.expand(4.0), 8.0, egui::Stroke::new(1.5, rgba(glass::ON_DARK, 220)), egui::StrokeKind::Outside);
            }
        }
        if self.mode == Mode::Crop {
            // The handles stand a little past the picture's edge.
            crop_frame(&ui.painter_at(view.rect.expand(8.0)), &view, crop);
        }
    }

    /// Where each text sits on screen; renders the pictures of texts that changed.
    fn text_boxes(&mut self, ui: &Ui, view: &View) -> Vec<Rect> {
        let ppp = ui.ctx().pixels_per_point();
        let long = view.long();
        let mut used = HashSet::new();
        let mut boxes = Vec::new();
        for t in &self.edit.texts {
            let px = t.size * long * ppp;
            let key = text_key(t, px);
            used.insert(key);
            let shown = if t.text.is_empty() { TextItem { text: l!("Текст", "Text").to_owned(), ..t.clone() } } else { t.clone() };
            if !t.text.is_empty() && !self.text_tex.contains_key(&key)
                && let Some(img) = render::text_image(t, px as f64)
            {
                self.text_tex.insert(key, ui.ctx().load_texture("photo-editor-text", img, TextureOptions::LINEAR));
            }
            let sz = match (t.text.is_empty(), self.text_tex.get(&key)) {
                (false, Some(tex)) => tex.size_vec2() / ppp,
                _ => render::layout(&shown, (t.size * long) as f64).map_or(Vec2::splat(24.0), |l| vec2(l.size.width as f32, l.size.height as f32)),
            };
            boxes.push(Rect::from_center_size(view.to_screen(t.pos), sz));
        }
        self.text_tex.retain(|k, _| used.contains(k));
        boxes
    }

    fn crop_input(&mut self, ui: &Ui, resp: &Response, view: &View) {
        let [x0, y0, x1, y1] = self.edit.crop;
        let (a, b) = (view.to_screen([x0, y0]), view.to_screen([x1, y1]));
        let free = self.aspect == Aspect::Free;
        let grab = |pos: Pos2| -> (i8, i8) {
            let near = |v: f32, lo: f32, hi: f32| if (v - lo).abs() < 16.0 { -1 } else if (v - hi).abs() < 16.0 { 1 } else { 0 };
            let (sx, sy) = (near(pos.x, a.x, b.x), near(pos.y, a.y, b.y));
            let inside = |v: f32, lo: f32, hi: f32| v > lo - 16.0 && v < hi + 16.0;
            match (sx, sy) {
                (0, 0) => (0, 0),
                // With a fixed aspect only the corners resize.
                (sx, sy) if sx != 0 && sy != 0 => (sx, sy),
                (sx, 0) if free && inside(pos.y, a.y, b.y) => (sx, 0),
                (0, sy) if free && inside(pos.x, a.x, b.x) => (0, sy),
                _ => (0, 0),
            }
        };
        if let Some(h) = resp.hover_pos() {
            ui.ctx().set_cursor_icon(match grab(h) {
                (-1, -1) | (1, 1) => egui::CursorIcon::ResizeNwSe,
                (1, -1) | (-1, 1) => egui::CursorIcon::ResizeNeSw,
                (_, 0) if grab(h).0 != 0 => egui::CursorIcon::ResizeHorizontal,
                (0, _) if grab(h).1 != 0 => egui::CursorIcon::ResizeVertical,
                _ => egui::CursorIcon::Move,
            });
        }
        if resp.drag_started()
            && let Some(pos) = resp.interact_pointer_pos()
        {
            self.push_undo();
            let (sx, sy) = grab(pos);
            self.drag = Some(Drag::Crop { sx, sy, start: self.edit.crop, from: view.to_norm(pos) });
        }
        if resp.drag_stopped() {
            self.drag = None;
        }
        let (Some(Drag::Crop { sx, sy, start, from }), Some(pos)) = (&self.drag, resp.interact_pointer_pos()) else { return };
        let (sx, sy, start, from) = (*sx, *sy, *start, *from);
        let now = view.to_norm(pos);
        let size = self.size();
        // At least 48 screen points either way.
        let min = [48.0 / view.rect.width(), 48.0 / view.rect.height()];
        let [mut x0, mut y0, mut x1, mut y1] = start;
        if (sx, sy) == (0, 0) {
            let dx = (now[0] - from[0]).clamp(-x0, 1.0 - x1);
            let dy = (now[1] - from[1]).clamp(-y0, 1.0 - y1);
            self.edit.crop = [x0 + dx, y0 + dy, x1 + dx, y1 + dy];
            return;
        }
        let ratio = self.locked_ratio();
        let p = [now[0].clamp(0.0, 1.0), now[1].clamp(0.0, 1.0)];
        match sx {
            -1 => x0 = p[0].min(x1 - min[0]),
            1 => x1 = p[0].max(x0 + min[0]),
            _ => {}
        }
        match sy {
            -1 => y0 = p[1].min(y1 - min[1]),
            1 => y1 = p[1].max(y0 + min[1]),
            _ => {}
        }
        if let Some(r) = ratio {
            // The height follows the width; if that leaves the picture, the width gives way.
            let mut h = (x1 - x0) * size.x / r / size.y;
            let room = if sy < 0 { y1 } else { 1.0 - y0 };
            if h > room {
                h = room;
                let w = h * size.y * r / size.x;
                if sx < 0 { x0 = x1 - w } else { x1 = x0 + w }
            }
            if sy < 0 { y0 = y1 - h } else { y1 = y0 + h }
        }
        self.edit.crop = [x0, y0, x1, y1];
    }

    fn draw_input(&mut self, resp: &Response, view: &View) {
        if resp.hovered() {
            resp.ctx.set_cursor_icon(egui::CursorIcon::Crosshair);
        }
        if resp.drag_started()
            && let Some(pos) = resp.interact_pointer_pos()
        {
            self.push_undo();
            self.edit.strokes.push(Stroke { points: vec![view.to_norm(pos)], color: self.color, width: self.brush });
            self.drag = Some(Drag::Stroke);
        }
        if resp.drag_stopped() {
            self.drag = None;
        }
        if let (Some(Drag::Stroke), Some(pos), Some(s)) = (&self.drag, resp.interact_pointer_pos(), self.edit.strokes.last_mut()) {
            let q = view.to_norm(pos);
            // Points closer than a screen pixel and a half add nothing but weight.
            if s.points.last().is_none_or(|l| ((q[0] - l[0]) * view.rect.width() / view.region.width()).hypot((q[1] - l[1]) * view.rect.height() / view.region.height()) > 1.5) {
                s.points.push(q);
            }
        }
    }

    fn text_input(&mut self, resp: &Response, view: &View, boxes: &[Rect]) {
        let hit = |pos: Pos2| boxes.iter().rposition(|r| r.expand(6.0).contains(pos));
        if resp.drag_started()
            && let Some(pos) = resp.interact_pointer_pos()
            && let Some(i) = hit(pos)
        {
            let i = self.select(i);
            if i < self.edit.texts.len() {
                self.push_undo();
                let n = view.to_norm(pos);
                let t = self.edit.texts[i].pos;
                self.drag = Some(Drag::Text { index: i, offset: [t[0] - n[0], t[1] - n[1]] });
            }
        }
        if resp.drag_stopped() {
            self.drag = None;
        }
        if let (Some(Drag::Text { index, offset }), Some(pos)) = (&self.drag, resp.interact_pointer_pos()) {
            let n = view.to_norm(pos);
            if let Some(t) = self.edit.texts.get_mut(*index) {
                t.pos = [(n[0] + offset[0]).clamp(0.0, 1.0), (n[1] + offset[1]).clamp(0.0, 1.0)];
            }
        }
        if resp.clicked()
            && let Some(pos) = resp.interact_pointer_pos()
        {
            match hit(pos) {
                Some(i) => {
                    self.select(i);
                    self.focus = true;
                }
                // A click past the texts deselects; with nothing selected it starts a new text.
                None if self.selected.is_some() => self.drop_empty(),
                None => self.add_text(view.to_norm(pos)),
            }
        }
        if resp.hovered() && resp.hover_pos().is_some_and(|p| hit(p).is_some()) {
            resp.ctx.set_cursor_icon(egui::CursorIcon::Grab);
        } else if resp.hovered() {
            resp.ctx.set_cursor_icon(egui::CursorIcon::Text);
        }
    }
}

/// The text's look and size, for its cached picture.
fn text_key(t: &TextItem, px: f32) -> u64 {
    use std::hash::{Hash, Hasher};
    let mut h = std::collections::hash_map::DefaultHasher::new();
    (&t.text, t.style as u8, t.font as u8, t.color, px.round() as u32).hash(&mut h);
    h.finish()
}

/// Darkens what the crop cuts off and draws the frame: thirds, edges and corner handles.
fn crop_frame(p: &egui::Painter, view: &View, crop: Rect) {
    let r = Rect::from_min_max(view.to_screen([crop.min.x, crop.min.y]), view.to_screen([crop.max.x, crop.max.y]));
    let full = view.rect;
    let shade = Color32::from_black_alpha(150);
    for s in [
        Rect::from_min_max(full.min, pos2(full.right(), r.top())),
        Rect::from_min_max(pos2(full.left(), r.bottom()), full.max),
        Rect::from_min_max(pos2(full.left(), r.top()), pos2(r.left(), r.bottom())),
        Rect::from_min_max(pos2(r.right(), r.top()), pos2(full.right(), r.bottom())),
    ] {
        p.rect_filled(s, 0.0, shade);
    }
    let thin = egui::Stroke::new(1.0, rgba(glass::ON_DARK, 90));
    for k in 1..3 {
        let f = k as f32 / 3.0;
        p.line_segment([pos2(r.left() + r.width() * f, r.top()), pos2(r.left() + r.width() * f, r.bottom())], thin);
        p.line_segment([pos2(r.left(), r.top() + r.height() * f), pos2(r.right(), r.top() + r.height() * f)], thin);
    }
    p.rect_stroke(r, 0.0, egui::Stroke::new(1.5, glass::ON_DARK), egui::StrokeKind::Inside);
    let (len, w) = (20.0_f32.min(r.width() / 2.0).min(r.height() / 2.0), 4.0);
    let thick = egui::Stroke::new(w, glass::ON_DARK);
    for (c, dx, dy) in [(r.left_top(), 1.0, 1.0), (r.right_top(), -1.0, 1.0), (r.left_bottom(), 1.0, -1.0), (r.right_bottom(), -1.0, -1.0)] {
        // Just outside the frame, so they never hide the picture's edge.
        let c = c - vec2(dx, dy) * (w / 2.0);
        p.line_segment([c - vec2(dx * w / 2.0, 0.0), c + vec2(dx * len, 0.0)], thick);
        p.line_segment([c, c + vec2(0.0, dy * len)], thick);
    }
}

/// A horizontal slider over `range`; returns whether it moved.
fn slider(ui: &mut Ui, value: &mut f32, range: (f32, f32), width: f32) -> bool {
    let (r, resp) = ui.allocate_exact_size(vec2(width, 40.0), Sense::click_and_drag());
    let line = Rect::from_center_size(r.center(), vec2(r.width() - 16.0, 4.0));
    let mut moved = false;
    if let Some(pos) = resp.interact_pointer_pos().filter(|_| resp.dragged() || resp.clicked()) {
        let f = ((pos.x - line.left()) / line.width()).clamp(0.0, 1.0);
        *value = range.0 + (range.1 - range.0) * f;
        moved = true;
    }
    let f = ((*value - range.0) / (range.1 - range.0)).clamp(0.0, 1.0);
    let p = ui.painter();
    p.rect_filled(line, 2.0, glass::veil(40));
    let x = line.left() + line.width() * f;
    p.rect_filled(Rect::from_min_max(line.min, pos2(x, line.bottom())), 2.0, ACCENT);
    p.circle_filled(pos2(x, line.center().y), 8.0, ink());
    if resp.hovered() {
        ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
    }
    moved
}

/// A capsule showing "Aa" in a font.
fn font_chip(ui: &mut Ui, font: Font, selected: bool) -> Response {
    let (rect, resp) = ui.allocate_exact_size(vec2(40.0, 32.0), Sense::click());
    let p = ui.painter();
    if selected {
        glass::glass(p, rect, Glass::tinted(16.0, ACCENT, 110));
    } else {
        p.rect_filled(rect, 16.0, glass::veil(if resp.hovered() { 24 } else { 10 }));
    }
    p.text(rect.center(), Align2::CENTER_CENTER, "Aa", FontId::new(14.0, font.family()), ink());
    if resp.hovered() {
        ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
    }
    resp.on_hover_text(match font {
        Font::Bold => l!("Жирний", "Bold"),
        Font::Regular => l!("Звичайний шрифт", "Regular"),
        Font::Italic => l!("Курсив", "Italic"),
    })
}

impl App {
    /// "Sample photo" in the attach menu of the demo, so the send box and the editor can be tried without files.
    pub(super) fn demo_photo_item(&mut self, ui: &mut Ui, peer: PeerId) {
        if self.demo && glass::menu_button(ui, Icon::Image, l!("Зразок фото", "Sample photo")).clicked() {
            match demo::sample_photo() {
                Ok(path) => self.send_files(peer, vec![path], false),
                Err(e) => self.toast = Some((e, 0.0, true)),
            }
        }
    }
}
