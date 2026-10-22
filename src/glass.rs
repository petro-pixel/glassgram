//! «Liquid glass»: напівпрозорі панелі з градієнтною заливкою, спекулярним ободом,
//! внутрішнім «заломленням» на дальньому краю та м'якою тінню поверх живого фону.

use std::f32::consts::{FRAC_PI_2, PI, TAU};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use eframe::egui::{
    self, Color32, CornerRadius, FontId, Mesh, Painter, Pos2, Rect, Response, Sense, Shape,
    Stroke, StrokeKind, TextEdit, TextureHandle, TextureId, Ui, Vec2, pos2, vec2,
    epaint::{ColorMode, PathShape, PathStroke, Shadow, Vertex, WHITE_UV},
};

use crate::icons::{self, Icon};

pub const ACCENT: Color32 = Color32::from_rgb(92, 124, 255);
/// Світлий текст поверх темного — для переглядача фото, що завжди темний.
pub const ON_DARK: Color32 = Color32::from_rgb(244, 246, 255);

static LIGHT: AtomicBool = AtomicBool::new(false);

/// Перемикає світлу тему для всіх кольорів нижче.
pub fn set_light(on: bool) {
    LIGHT.store(on, Ordering::Relaxed);
}

pub fn is_light() -> bool {
    LIGHT.load(Ordering::Relaxed)
}

pub fn ink() -> Color32 {
    if is_light() { Color32::from_rgb(22, 26, 46) } else { ON_DARK }
}

pub fn ink_dim() -> Color32 {
    if is_light() { Color32::from_rgb(88, 96, 126) } else { Color32::from_rgb(178, 186, 214) }
}

/// Тон щільних панелей (поле вводу, меню, чипи).
pub fn chrome() -> Color32 {
    if is_light() { Color32::from_rgb(250, 251, 255) } else { Color32::from_rgb(28, 32, 70) }
}

/// Легке підсвічування (наведення, заглушки): світле на темному, темне на світлому.
pub fn veil(alpha: u8) -> Color32 {
    if is_light() { Color32::from_black_alpha((alpha as f32 * 0.6) as u8) } else { Color32::from_white_alpha(alpha) }
}

/// Тон і густина вхідних бульбашок.
pub fn bubble_in() -> (Color32, u8) {
    if is_light() { (Color32::WHITE, 190) } else { (Color32::from_rgb(200, 210, 255), 28) }
}

/// Густина акцентного тону вихідних бульбашок.
pub fn bubble_out_alpha() -> u8 {
    if is_light() { 70 } else { 140 }
}

/// Колір посилань у бульбашці.
pub fn link(out: bool) -> Color32 {
    match (is_light(), out) {
        (true, _) => Color32::from_rgb(28, 92, 210),
        (false, true) => Color32::from_rgb(200, 225, 255),
        (false, false) => Color32::from_rgb(130, 190, 255),
    }
}

/// Напрям «джерела світла» (зверху зліва).
fn light() -> Vec2 {
    vec2(-0.45, -0.9).normalized()
}

#[derive(Clone, Copy)]
pub struct Glass {
    pub radius: f32,
    /// Колір тонування; альфа задає «густину» скла.
    pub tint: Color32,
    pub shadow: bool,
    /// Сила верхнього відблиску (0..1).
    pub shine: f32,
    /// Непрозорість шару справжнього розмиття (0 — без нього).
    pub blur: f32,
}

static GPU: AtomicBool = AtomicBool::new(false);

/// Вмикає GPU-розмиття (викликається, якщо wgpu-ресурси зареєстровано).
pub fn set_gpu(enabled: bool) {
    GPU.store(enabled, Ordering::Relaxed);
}

fn gpu() -> bool {
    GPU.load(Ordering::Relaxed)
}

impl Glass {
    pub fn panel(radius: f32) -> Self {
        // На світлому фоні скло має бути густішим, інакше панелі зливаються з ним.
        let alpha = if is_light() { 125 } else { 22 };
        Self { radius, tint: Color32::from_rgba_unmultiplied(255, 255, 255, alpha), shadow: true, shine: 1.0, blur: 1.0 }
    }

    pub fn tinted(radius: f32, rgb: Color32, alpha: u8) -> Self {
        Self {
            radius,
            tint: Color32::from_rgba_unmultiplied(rgb.r(), rgb.g(), rgb.b(), alpha),
            shadow: false,
            shine: 0.8,
            blur: 0.0,
        }
    }

    pub fn frosted(mut self, opacity: f32) -> Self {
        self.blur = opacity;
        self
    }

    pub fn with_shadow(mut self) -> Self {
        self.shadow = true;
        self
    }
}

pub fn rgba(c: Color32, a: u8) -> Color32 {
    Color32::from_rgba_unmultiplied(c.r(), c.g(), c.b(), a)
}

fn mix(a: Color32, b: Color32, t: f32) -> Color32 {
    let t = t.clamp(0.0, 1.0);
    let l = |x: u8, y: u8| (x as f32 + (y as f32 - x as f32) * t).round() as u8;
    Color32::from_rgba_premultiplied(l(a.r(), b.r()), l(a.g(), b.g()), l(a.b(), b.b()), l(a.a(), b.a()))
}

/// Точки заокругленого прямокутника за годинниковою стрілкою (екранні координати).
/// Кількість сегментів дуги залежить від радіуса у фізичних пікселях (`ppp`), щоб хорда
/// відхилялася від кола не більше ніж на 0.1 px — кути лишаються гладкими за будь-якого масштабу.
pub fn rounded_points(rect: Rect, r: f32, ppp: f32) -> Vec<Pos2> {
    let r = r.min(rect.width() / 2.0).min(rect.height() / 2.0).max(0.0);
    if r * ppp < 0.5 {
        return vec![rect.left_top(), rect.right_top(), rect.right_bottom(), rect.left_bottom()];
    }
    let step = 2.0 * (1.0 - (0.1 / (r * ppp)).min(1.0)).acos();
    let n = ((FRAC_PI_2 / step).ceil() as usize).clamp(2, 96);
    let corners = [
        (pos2(rect.right() - r, rect.top() + r), -FRAC_PI_2),
        (pos2(rect.right() - r, rect.bottom() - r), 0.0),
        (pos2(rect.left() + r, rect.bottom() - r), FRAC_PI_2),
        (pos2(rect.left() + r, rect.top() + r), PI),
    ];
    let mut pts = Vec::with_capacity(4 * (n + 1));
    for (c, start) in corners {
        for i in 0..=n {
            let a = start + FRAC_PI_2 * i as f32 / n as f32;
            pts.push(c + r * vec2(a.cos(), a.sin()));
        }
    }
    // На стиках дуг (коли r дорівнює половині сторони) точки збігаються — прибираємо дублікати.
    pts.dedup_by(|a, b| a.distance(*b) < 1e-3);
    if pts.len() > 1 && pts[0].distance(pts[pts.len() - 1]) < 1e-3 {
        pts.pop();
    }
    pts
}

/// Опуклий меш зі згладженим краєм: внутрішнє кільце з кольором `attr`, зовнішнє —
/// прозоре, на відстані одного фізичного пікселя (як feathering у тесселяторі egui).
/// `attr` повертає колір і UV для точки.
fn aa_fan(points: &[Pos2], center: Pos2, ppp: f32, tex: TextureId, attr: impl Fn(Pos2) -> (Color32, Pos2)) -> Mesh {
    let mut mesh = Mesh::with_texture(tex);
    let n = points.len();
    if n < 3 {
        return mesh;
    }
    let half = 0.5 / ppp;
    let vertex = |pos: Pos2, color: Color32, uv: Pos2| Vertex { pos, uv, color };
    let (c, uv) = attr(center);
    mesh.vertices.push(vertex(center, c, uv));
    for i in 0..n {
        let (prev, cur, next) = (points[(i + n - 1) % n], points[i], points[(i + 1) % n]);
        // Зовнішня нормаль для обходу за годинниковою стрілкою при осі y донизу.
        let normal = |a: Pos2, b: Pos2| {
            let d = (b - a).normalized();
            vec2(d.y, -d.x)
        };
        let nrm = (normal(prev, cur) + normal(cur, next)).normalized();
        let inner = cur - nrm * half;
        let outer = cur + nrm * half;
        let (ci, ui) = attr(inner);
        let (_, uo) = attr(outer);
        mesh.vertices.push(vertex(inner, ci, ui));
        mesh.vertices.push(vertex(outer, Color32::TRANSPARENT, uo));
    }
    let n = n as u32;
    for i in 0..n {
        let j = (i + 1) % n;
        let (ia, oa, ib, ob) = (1 + 2 * i, 2 + 2 * i, 1 + 2 * j, 2 + 2 * j);
        mesh.add_triangle(0, ia, ib);
        mesh.add_triangle(ia, oa, ib);
        mesh.add_triangle(ib, oa, ob);
    }
    mesh
}

/// Опуклий згладжений меш з кольором, що залежить від позиції.
fn fan(points: &[Pos2], center: Pos2, ppp: f32, color: impl Fn(Pos2) -> Color32) -> Mesh {
    aa_fan(points, center, ppp, TextureId::default(), |q| (color(q), WHITE_UV))
}

/// Радіальна пляма (еліпс) від `inner` у центрі до прозорого краю.
fn radial(center: Pos2, radii: Vec2, inner: Color32) -> Mesh {
    let mut mesh = Mesh::default();
    const SEG: u32 = 48;
    mesh.colored_vertex(center, inner);
    for ring in [0.55_f32, 1.0] {
        let col = if ring < 1.0 { mix(Color32::TRANSPARENT, inner, 0.45) } else { Color32::TRANSPARENT };
        for i in 0..SEG {
            let a = TAU * i as f32 / SEG as f32;
            mesh.colored_vertex(center + vec2(a.cos() * radii.x, a.sin() * radii.y) * ring, col);
        }
    }
    for i in 0..SEG {
        let j = (i + 1) % SEG;
        mesh.add_triangle(0, 1 + i, 1 + j);
        let (a, b, c, d) = (1 + i, 1 + j, 1 + SEG + i, 1 + SEG + j);
        mesh.add_triangle(a, c, b);
        mesh.add_triangle(b, c, d);
    }
    mesh
}

/// Нормаль межі заокругленого прямокутника в точці `p`.
fn edge_normal(rect: Rect, r: f32, p: Pos2) -> Vec2 {
    let inner = rect.shrink(r.min(rect.width() / 2.0).min(rect.height() / 2.0) - 0.01);
    let n = p - inner.clamp(p);
    if n.length_sq() < 1e-6 { vec2(0.0, -1.0) } else { n.normalized() }
}

/// Живий фон: темний градієнт і кольорові плями, що повільно пливуть.
pub fn background(p: &Painter, rect: Rect, t: f64) {
    if gpu() {
        p.add(crate::blur::background_callback(rect, (t % 100_000.0) as f32, is_light()));
        return;
    }
    let (top, bottom) = if is_light() {
        (Color32::from_rgb(238, 241, 255), Color32::from_rgb(214, 221, 248))
    } else {
        (Color32::from_rgb(16, 18, 44), Color32::from_rgb(6, 8, 22))
    };
    let corners = [rect.left_top(), rect.right_top(), rect.right_bottom(), rect.left_bottom()];
    p.add(Shape::mesh(fan(&corners, rect.center(), p.pixels_per_point(), |q| {
        mix(top, bottom, (q.y - rect.top()) / rect.height())
    })));

    let t = t as f32;
    let s = rect.size().min_elem();
    let blobs = [
        (Color32::from_rgb(120, 80, 255), 0.62, 0.07, 0.0),
        (Color32::from_rgb(0, 190, 255), 0.55, 0.05, 2.1),
        (Color32::from_rgb(255, 70, 160), 0.48, 0.06, 4.0),
        (Color32::from_rgb(255, 150, 60), 0.36, 0.08, 5.3),
        (Color32::from_rgb(40, 230, 180), 0.40, 0.045, 1.2),
    ];
    for (col, size, speed, phase) in blobs {
        let c = rect.center()
            + vec2(
                (t * speed + phase).sin() * rect.width() * 0.38,
                (t * speed * 0.8 + phase * 1.7).cos() * rect.height() * 0.34,
            );
        let r = s * size * (1.0 + 0.08 * (t * speed * 3.0 + phase).sin());
        p.add(Shape::mesh(radial(c, vec2(r * 1.15, r), rgba(col, if is_light() { 90 } else { 150 }))));
    }
}

/// Основна функція: малює скляну поверхню.
pub fn glass(p: &Painter, rect: Rect, g: Glass) {
    let r = g.radius.min(rect.width() / 2.0).min(rect.height() / 2.0);
    if g.shadow {
        let shadow = Shadow { offset: [0, 12], blur: 40, spread: 0, color: Color32::from_black_alpha(if is_light() { 34 } else { 70 }) };
        p.add(shadow.as_shape(rect, CornerRadius::same(r.min(255.0) as u8)));
    }
    if g.blur > 0.0 && gpu() {
        p.add(crate::blur::panel_callback(rect, r, g.blur, 16.0));
    }

    let ppp = p.pixels_per_point();
    let pts = rounded_points(rect, r, ppp);
    let (tr, tg, tb, ta) = g.tint.to_srgba_unmultiplied().into();
    let lit = Color32::from_rgba_unmultiplied(
        tr.saturating_add((255 - tr) / 2),
        tg.saturating_add((255 - tg) / 2),
        tb.saturating_add((255 - tb) / 2),
        ta.saturating_add((30.0 * g.shine) as u8),
    );
    let deep = Color32::from_rgba_unmultiplied(tr, tg, tb, ta);
    p.add(Shape::mesh(fan(&pts, rect.center(), ppp, |q| {
        let k = 0.3 * (q.x - rect.left()) / rect.width() + 0.7 * (q.y - rect.top()) / rect.height();
        mix(lit, deep, k)
    })));

    // Спекулярний відблиск у верхній частині.
    if g.shine > 0.0 && rect.height() > 18.0 {
        let gh = (rect.height() * 0.22).min(34.0);
        let center = pos2(rect.center().x, rect.top() + gh * 0.75);
        let radii = vec2(rect.width() * 0.42 - r * 0.2, gh * 0.7);
        p.add(Shape::mesh(radial(center, radii, Color32::from_white_alpha((22.0 * g.shine) as u8))));
    }

    // Внутрішнє «заломлення»: світлий ореол на протилежному від світла краю.
    let inset = rect.shrink(2.0);
    let ir = (r - 2.0).max(0.0);
    p.add(PathShape::closed_line(
        rounded_points(inset, ir, ppp),
        PathStroke {
            width: 3.0,
            color: ColorMode::UV(Arc::new(move |_, q| {
                let d = edge_normal(inset, ir, q).dot(-light()).max(0.0);
                Color32::from_white_alpha((d.powi(2) * 34.0) as u8)
            })),
            kind: StrokeKind::Inside,
        },
    ));

    // Обід зі спекулярним бліком.
    p.add(PathShape::closed_line(
        pts,
        PathStroke {
            width: 1.2,
            color: ColorMode::UV(Arc::new(move |_, q| {
                let n = edge_normal(rect, r, q);
                let front = n.dot(light()).max(0.0).powi(2);
                let back = n.dot(-light()).max(0.0).powi(3);
                Color32::from_white_alpha((30.0 + 190.0 * front + 70.0 * back).min(255.0) as u8)
            })),
            kind: StrokeKind::Inside,
        },
    ));
}

/// Вміст кнопки: текст або векторна іконка.
pub enum Label<'a> {
    Text(&'a str),
    Icon(Icon),
}

/// Скляна кнопка з текстом.
pub fn button(ui: &mut Ui, rect: Rect, id: impl std::hash::Hash + std::fmt::Debug, label: &str, accent: bool, enabled: bool) -> Response {
    button_with(ui, rect, id, Label::Text(label), accent, enabled)
}

/// Скляна кнопка з векторною іконкою.
pub fn icon_button(ui: &mut Ui, rect: Rect, id: impl std::hash::Hash + std::fmt::Debug, icon: Icon, accent: bool, enabled: bool) -> Response {
    button_with(ui, rect, id, Label::Icon(icon), accent, enabled)
}

/// Скляна кнопка з анімацією наведення та натискання.
pub fn button_with(ui: &mut Ui, rect: Rect, id: impl std::hash::Hash + std::fmt::Debug, label: Label<'_>, accent: bool, enabled: bool) -> Response {
    let resp = ui.interact(rect, ui.scope_id().with(id), if enabled { Sense::click() } else { Sense::hover() });
    let hover = ui.ctx().animate_bool(resp.id.with("hover"), resp.hovered() && enabled);
    let press = resp.is_pointer_button_down_on() && enabled;
    let rect = rect.expand(hover * 1.5).shrink(if press { 2.0 } else { 0.0 });
    let g = if accent {
        // На світлому тлі акцент блідне — даємо густіший тон, щоб білий текст читався.
        let base = match (enabled, is_light()) {
            (true, true) => 215.0,
            (true, false) => 150.0,
            (false, _) => 70.0,
        };
        let tint = if is_light() { Color32::from_rgb(52, 84, 228) } else { ACCENT };
        Glass::tinted(rect.height() / 2.0, tint, (base + hover * 40.0).min(255.0) as u8).with_shadow().frosted(1.0)
    } else {
        let base = if is_light() { 150.0 } else { 24.0 };
        Glass::tinted(rect.height() / 2.0, Color32::WHITE, (base + hover * 22.0) as u8).frosted(1.0)
    };
    glass(ui.painter(), rect, g);
    // На акцентній кнопці текст завжди світлий — синій фон темний в обох темах.
    let color = match (enabled, accent) {
        (true, true) => ON_DARK,
        (true, false) => ink(),
        (false, _) => ink_dim(),
    };
    match label {
        Label::Text(t) => {
            ui.painter().text(rect.center(), egui::Align2::CENTER_CENTER, t, FontId::proportional(15.0), color);
        }
        Label::Icon(icon) => {
            let side = (rect.height() * 0.5).min(22.0);
            icons::draw(ui.painter(), Rect::from_center_size(rect.center(), Vec2::splat(side)), icon, color);
        }
    }
    if enabled && resp.hovered() {
        ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
    }
    resp
}

/// Поле вводу у скляній капсулі.
pub fn text_field(ui: &mut Ui, rect: Rect, text: &mut String, hint: &str, password: bool) -> Response {
    let focused_id = ui.scope_id().with(("field", hint));
    let focus = ui.ctx().animate_bool(focused_id, ui.memory(|m| m.has_focus(ui.scope_id().with(("edit", hint)))));
    glass(ui.painter(), rect, Glass::tinted(rect.height() / 2.0, Color32::WHITE, field_alpha(focus)));
    if focus > 0.0 {
        let cr = CornerRadius::same((rect.height() / 2.0) as u8);
        ui.painter().rect_stroke(rect, cr, Stroke::new(1.5, rgba(ACCENT, (focus * 200.0) as u8)), StrokeKind::Outside);
    }
    let inner = rect.shrink2(vec2(rect.height() * 0.4, 0.0));
    let edit = TextEdit::singleline(text)
        .id(ui.scope_id().with(("edit", hint)))
        .hint_text(hint)
        .password(password)
        .frame(egui::Frame::NONE)
        .font(FontId::proportional(15.5))
        .text_color(ink())
        .align(egui::Align2::LEFT_CENTER)
        .min_size(inner.size())
        .desired_width(inner.width());
    ui.put(inner, edit)
}

/// Густина білого тону поля вводу залежно від фокуса.
fn field_alpha(focus: f32) -> u8 {
    let base = if is_light() { 150.0 } else { 16.0 };
    (base + focus * 14.0) as u8
}

/// Шрифт поля вводу повідомлення.
pub const AREA_FONT: f32 = 15.5;

/// Багаторядкове поле: Enter без Shift повертає `submitted`, Shift+Enter переносить рядок.
pub fn text_area(ui: &mut Ui, rect: Rect, text: &mut String, hint: &str) -> (Response, bool) {
    let edit_id = ui.scope_id().with(("edit", hint));
    let focus = ui.ctx().animate_bool(ui.scope_id().with(("field", hint)), ui.memory(|m| m.has_focus(edit_id)));
    let radius = (rect.height() / 2.0).min(25.0);
    glass(ui.painter(), rect, Glass::tinted(radius, Color32::WHITE, field_alpha(focus)));
    if focus > 0.0 {
        let cr = CornerRadius::same(radius as u8);
        ui.painter().rect_stroke(rect, cr, Stroke::new(1.5, rgba(ACCENT, (focus * 200.0) as u8)), StrokeKind::Outside);
    }
    // Enter забираємо до TextEdit, інакше він вставить перенос рядка.
    let has_focus = ui.memory(|m| m.has_focus(edit_id));
    let submitted = has_focus && ui.input_mut(|i| i.consume_key(egui::Modifiers::NONE, egui::Key::Enter));
    let inner = rect.shrink2(vec2(20.0, 13.0));
    let edit = TextEdit::multiline(text)
        .id(edit_id)
        .hint_text(hint)
        .frame(egui::Frame::NONE)
        .font(FontId::proportional(AREA_FONT))
        .text_color(ink())
        .desired_rows(1)
        .min_size(inner.size())
        .desired_width(inner.width());
    let mut child = ui.new_child(egui::UiBuilder::new().max_rect(inner));
    child.set_clip_rect(inner.expand(2.0));
    let resp = egui::ScrollArea::vertical()
        .id_salt(("area-scroll", hint))
        .max_height(inner.height())
        .stick_to_bottom(true)
        .show(&mut child, |ui| ui.add(edit))
        .inner;
    (resp, submitted)
}

const PALETTE: [(Color32, Color32); 7] = [
    (Color32::from_rgb(255, 120, 120), Color32::from_rgb(220, 60, 110)),
    (Color32::from_rgb(255, 190, 90), Color32::from_rgb(240, 120, 40)),
    (Color32::from_rgb(170, 130, 255), Color32::from_rgb(110, 70, 230)),
    (Color32::from_rgb(110, 225, 140), Color32::from_rgb(40, 170, 110)),
    (Color32::from_rgb(100, 210, 255), Color32::from_rgb(40, 130, 240)),
    (Color32::from_rgb(255, 130, 200), Color32::from_rgb(210, 70, 170)),
    (Color32::from_rgb(90, 230, 220), Color32::from_rgb(30, 160, 180)),
];

pub fn name_color(seed: i64) -> Color32 {
    let (light, deep) = PALETTE[seed.unsigned_abs() as usize % PALETTE.len()];
    // На білих бульбашках світла половина градієнта бліда — беремо темнішу.
    if is_light() { mix(deep, Color32::BLACK, 0.15) } else { light }
}

pub fn initials(name: &str) -> String {
    name.split_whitespace()
        .filter_map(|w| w.chars().find(|c| c.is_alphanumeric()))
        .take(2)
        .flat_map(char::to_uppercase)
        .collect()
}

/// Зображення, обрізане по заокругленому прямокутнику (UV-меш замість кліпу).
pub fn image_rounded(p: &Painter, rect: Rect, r: f32, tex: TextureId, tex_size: Vec2) {
    // Режим "cover": заповнюємо прямокутник, обрізаючи зайве.
    let scale = (rect.width() / tex_size.x).max(rect.height() / tex_size.y);
    let uv_size = vec2(rect.width() / (tex_size.x * scale), rect.height() / (tex_size.y * scale));
    let uv_min = (Vec2::splat(1.0) - uv_size) / 2.0;
    let uv = |q: Pos2| {
        let t = (q - rect.min) / rect.size();
        pos2(uv_min.x + t.x * uv_size.x, uv_min.y + t.y * uv_size.y)
    };
    let ppp = p.pixels_per_point();
    let pts = rounded_points(rect, r, ppp);
    p.add(Shape::mesh(aa_fan(&pts, rect.center(), ppp, tex, |q| (Color32::WHITE, uv(q)))));
}

/// Зображення, вписане в прямокутник зі збереженням пропорцій (для стікерів).
pub fn image_contain(p: &Painter, rect: Rect, tex: &TextureHandle) {
    let s = tex.size_vec2();
    let k = (rect.width() / s.x).min(rect.height() / s.y);
    let r = Rect::from_center_size(rect.center(), s * k);
    p.image(tex.id(), r, Rect::from_min_max(pos2(0.0, 0.0), pos2(1.0, 1.0)), Color32::WHITE);
}

/// Кругла аватарка: фото або градієнт з ініціалами, зі скляним ободом.
pub fn avatar(p: &Painter, center: Pos2, radius: f32, name: &str, seed: i64, icon: Option<Icon>, tex: Option<&TextureHandle>) {
    let rect = Rect::from_center_size(center, Vec2::splat(radius * 2.0));
    match tex {
        Some(t) => image_rounded(p, rect, radius, t.id(), t.size_vec2()),
        None => {
            let (a, b) = PALETTE[seed.unsigned_abs() as usize % PALETTE.len()];
            let ppp = p.pixels_per_point();
            p.add(Shape::mesh(fan(&rounded_points(rect, radius, ppp), center, ppp, |q| {
                mix(a, b, (q.y - rect.top()) / rect.height())
            })));
            match icon {
                Some(icon) => icons::draw(p, Rect::from_center_size(center, Vec2::splat(radius * 1.05)), icon, Color32::WHITE),
                None => {
                    p.text(center, egui::Align2::CENTER_CENTER, initials(name), FontId::proportional(radius * 0.78), Color32::WHITE);
                }
            }
        }
    }
    glass(p, rect, Glass { shine: 0.5, ..Glass::tinted(radius, Color32::WHITE, 0) });
}

/// QR-код на білій скляній підкладці.
pub fn qr(p: &Painter, rect: Rect, data: &str) {
    glass(p, rect, Glass::tinted(24.0, Color32::WHITE, 235).with_shadow());
    let Ok(code) = qrcode::QrCode::new(data.as_bytes()) else { return };
    let n = code.width();
    let colors = code.to_colors();
    let area = rect.shrink(18.0);
    let cell = area.width() / n as f32;
    let ink = Color32::from_rgb(16, 20, 44);
    for (i, c) in colors.iter().enumerate() {
        if *c == qrcode::Color::Dark {
            let (x, y) = ((i % n) as f32, (i / n) as f32);
            let r = Rect::from_min_size(area.min + vec2(x, y) * cell, Vec2::splat(cell)).expand(0.25);
            p.rect_filled(r, 0.0, ink);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rounded_points_scale_with_density() {
        let rect = Rect::from_min_size(pos2(0.0, 0.0), vec2(200.0, 40.0));
        let lo = rounded_points(rect, 20.0, 1.0);
        let hi = rounded_points(rect, 20.0, 2.0);
        assert!(hi.len() > lo.len());
        // Капсула: стики дуг не дублюються.
        for w in hi.windows(2) {
            assert!(w[0].distance(w[1]) > 1e-3);
        }
        assert_eq!(rounded_points(rect, 0.0, 1.0).len(), 4);
    }

    #[test]
    fn aa_fan_has_transparent_rim() {
        let rect = Rect::from_min_size(pos2(0.0, 0.0), vec2(50.0, 50.0));
        let pts = rounded_points(rect, 10.0, 1.0);
        let mesh = fan(&pts, rect.center(), 1.0, |_| Color32::WHITE);
        assert_eq!(mesh.vertices.len(), 1 + 2 * pts.len());
        // Зовнішнє кільце лежить поза контуром і прозоре.
        let outer = &mesh.vertices[2];
        assert_eq!(outer.color, Color32::TRANSPARENT);
        assert!(!rect.shrink(0.1).contains(outer.pos));
    }
}
