//! Векторні іконки: шляхи на сітці 24×24, які масштабуються до будь-якого розміру
//! й згладжуються тесселятором egui (на відміну від емодзі з растрового атласу).

use std::f32::consts::{FRAC_PI_2, PI};

use eframe::egui::{Color32, Painter, Pos2, Rect, Shape, Stroke, Vec2, vec2};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Icon {
    Send,
    Check,
    Close,
    Search,
    Reload,
    Power,
    Attach,
    Smile,
    Reply,
    Pencil,
    Pin,
    Muted,
    Channel,
    Bot,
    File,
    Camera,
    Bookmark,
    Forward,
    Trash,
    Copy,
    Download,
    Clock,
    Location,
    ChevronLeft,
    ChevronRight,
    Bell,
    Play,
    Pause,
    Mic,
    Stop,
    Gear,
    Lock,
}

/// Відображення сітки 24×24 у квадрат усередині `rect` (з необов'язковим поворотом).
struct Grid {
    origin: Pos2,
    scale: f32,
    rot: f32,
    /// Дзеркально по горизонталі.
    flip: bool,
}

impl Grid {
    fn p(&self, x: f32, y: f32) -> Pos2 {
        let (s, c) = self.rot.sin_cos();
        let (dx, dy) = (if self.flip { 12.0 - x } else { x - 12.0 }, y - 12.0);
        self.origin + vec2(12.0 + dx * c - dy * s, 12.0 + dx * s + dy * c) * self.scale
    }

    fn pts(&self, xy: &[(f32, f32)]) -> Vec<Pos2> {
        xy.iter().map(|&(x, y)| self.p(x, y)).collect()
    }

    /// Точки дуги з центром (cx, cy), кути в радіанах (y донизу: 0 — праворуч, π/2 — донизу).
    fn arc(&self, cx: f32, cy: f32, r: f32, from: f32, to: f32) -> Vec<Pos2> {
        let n = (((to - from).abs() * r * self.scale / 2.0).ceil() as usize).clamp(6, 96);
        (0..=n)
            .map(|i| {
                let a = from + (to - from) * i as f32 / n as f32;
                self.p(cx + r * a.cos(), cy + r * a.sin())
            })
            .collect()
    }
}

pub fn draw(p: &Painter, rect: Rect, icon: Icon, color: Color32) {
    let side = rect.width().min(rect.height());
    let scale = side / 24.0;
    let origin = rect.center() - Vec2::splat(side / 2.0);
    let g = Grid { origin, scale, rot: 0.0, flip: false };
    let w = (1.9 * scale).max(1.1);
    let stroke = Stroke::new(w, color);
    let line = |pts: Vec<Pos2>| {
        p.add(Shape::line(pts, stroke));
    };
    let closed = |pts: Vec<Pos2>| {
        p.add(Shape::closed_line(pts, stroke));
    };
    let fill = |pts: Vec<Pos2>| {
        p.add(Shape::convex_polygon(pts, color, Stroke::NONE));
    };
    let dot = |x: f32, y: f32, r: f32| p.circle_filled(g.p(x, y), r * scale, color);
    let circle = |x: f32, y: f32, r: f32| p.circle_stroke(g.p(x, y), r * scale, stroke);

    match icon {
        Icon::Send => {
            // Паперовий літачок з двох опуклих половин.
            fill(g.pts(&[(3.0, 3.5), (21.0, 12.0), (14.5, 12.0), (3.0, 10.2)]));
            fill(g.pts(&[(3.0, 13.8), (14.5, 12.0), (21.0, 12.0), (3.0, 20.5)]));
        }
        Icon::Check => line(g.pts(&[(5.0, 12.5), (10.0, 17.5), (19.5, 7.0)])),
        Icon::Close => {
            line(g.pts(&[(6.0, 6.0), (18.0, 18.0)]));
            line(g.pts(&[(18.0, 6.0), (6.0, 18.0)]));
        }
        Icon::Search => {
            circle(10.5, 10.5, 6.3);
            line(g.pts(&[(15.3, 15.3), (20.0, 20.0)]));
        }
        Icon::Reload => {
            let (from, to) = (-FRAC_PI_2 + 0.5, -FRAC_PI_2 + 2.0 * PI - 0.3);
            line(g.arc(12.0, 12.0, 7.5, from, to));
            let tip = (12.0 + 7.5 * to.cos(), 12.0 + 7.5 * to.sin());
            fill(g.pts(&[(tip.0 - 3.6, tip.1 - 2.2), (tip.0 + 2.6, tip.1 - 0.4), (tip.0 - 1.0, tip.1 + 4.0)]));
        }
        Icon::Power => {
            line(g.arc(12.0, 13.0, 7.5, -FRAC_PI_2 + 0.75, -FRAC_PI_2 + 2.0 * PI - 0.75));
            line(g.pts(&[(12.0, 3.5), (12.0, 11.5)]));
        }
        Icon::Attach => {
            // Скріпка: дві дуги та прямі, повернуті на 45°.
            let g = Grid { rot: PI / 4.0, ..g };
            let mut pts = g.pts(&[(15.5, 6.5), (15.5, 16.0)]);
            pts.extend(g.arc(12.0, 16.0, 3.5, 0.0, PI));
            pts.extend(g.pts(&[(8.5, 5.5)]));
            pts.extend(g.arc(10.75, 5.5, 2.25, PI, 2.0 * PI));
            pts.extend(g.pts(&[(13.0, 15.0)]));
            line(pts);
        }
        Icon::Smile => {
            circle(12.0, 12.0, 8.8);
            dot(9.0, 10.0, 1.2);
            dot(15.0, 10.0, 1.2);
            line(g.arc(12.0, 12.0, 4.6, 0.45, PI - 0.45));
        }
        Icon::Reply | Icon::Forward => {
            let g = Grid { flip: icon == Icon::Forward, ..g };
            line(g.pts(&[(10.0, 6.0), (4.0, 12.0), (10.0, 18.0)]));
            let mut pts = g.pts(&[(4.0, 12.0), (13.0, 12.0)]);
            pts.extend(g.arc(13.0, 19.0, 7.0, -FRAC_PI_2, 0.0));
            line(pts);
        }
        Icon::Pencil => {
            closed(g.pts(&[(4.0, 20.0), (5.0, 15.5), (15.5, 5.0), (19.0, 8.5), (8.5, 19.0)]));
            line(g.pts(&[(13.5, 7.0), (17.0, 10.5)]));
        }
        Icon::Pin => {
            let g = Grid { rot: PI / 6.0, ..g };
            line(g.pts(&[(7.5, 3.0), (16.5, 3.0)]));
            closed(g.pts(&[(9.5, 3.0), (9.5, 9.0), (6.5, 13.5), (17.5, 13.5), (14.5, 9.0), (14.5, 3.0)]));
            line(g.pts(&[(12.0, 13.5), (12.0, 21.0)]));
        }
        Icon::Muted => {
            closed(g.pts(&[(3.5, 9.0), (7.5, 9.0), (12.5, 5.0), (12.5, 19.0), (7.5, 15.0), (3.5, 15.0)]));
            line(g.pts(&[(16.0, 9.5), (21.0, 14.5)]));
            line(g.pts(&[(21.0, 9.5), (16.0, 14.5)]));
        }
        Icon::Play => fill(g.pts(&[(7.0, 4.5), (19.5, 12.0), (7.0, 19.5)])),
        Icon::Pause => {
            p.rect_filled(Rect::from_two_pos(g.p(6.5, 5.0), g.p(10.0, 19.0)), 1.0 * scale, color);
            p.rect_filled(Rect::from_two_pos(g.p(14.0, 5.0), g.p(17.5, 19.0)), 1.0 * scale, color);
        }
        Icon::Lock => {
            // Дужка й корпус замка з замковою щілиною.
            let mut pts = g.pts(&[(8.0, 11.0)]);
            pts.extend(g.arc(12.0, 8.0, 4.0, PI, 2.0 * PI));
            pts.extend(g.pts(&[(16.0, 11.0)]));
            line(pts);
            closed(g.pts(&[(5.5, 11.0), (18.5, 11.0), (18.5, 20.5), (5.5, 20.5)]));
            dot(12.0, 15.2, 1.4);
        }
        Icon::Gear => {
            // Шестерня: вісім зубців по колу й отвір.
            let mut pts = Vec::new();
            for k in 0..8 {
                let a = k as f32 * PI / 4.0;
                for (da, r) in [(-0.24, 7.0), (-0.15, 9.3), (0.15, 9.3), (0.24, 7.0)] {
                    pts.push(g.p(12.0 + r * (a + da).cos(), 12.0 + r * (a + da).sin()));
                }
            }
            closed(pts);
            circle(12.0, 12.0, 3.0);
        }
        Icon::Stop => {
            p.rect_filled(Rect::from_two_pos(g.p(6.0, 6.0), g.p(18.0, 18.0)), 2.5 * scale, color);
        }
        Icon::Mic => {
            // Капсула мікрофона, тримач-дуга і ніжка.
            let mut pts = g.arc(12.0, 7.0, 3.2, PI, 2.0 * PI);
            pts.extend(g.arc(12.0, 12.0, 3.2, 0.0, PI));
            closed(pts);
            line(g.arc(12.0, 11.5, 6.3, 0.15, PI - 0.15));
            line(g.pts(&[(12.0, 17.8), (12.0, 21.0)]));
            line(g.pts(&[(8.5, 21.0), (15.5, 21.0)]));
        }
        Icon::Bell => {
            let mut pts = g.pts(&[(4.5, 17.0), (6.5, 14.5), (6.5, 10.5)]);
            pts.extend(g.arc(12.0, 10.5, 5.5, PI, 2.0 * PI));
            pts.extend(g.pts(&[(17.5, 14.5), (19.5, 17.0)]));
            closed(pts);
            line(g.arc(12.0, 18.5, 2.3, 0.2, PI - 0.2));
        }
        Icon::Channel => {
            closed(g.pts(&[(3.5, 9.5), (8.0, 9.5), (18.0, 4.5), (18.0, 19.5), (8.0, 14.5), (3.5, 14.5)]));
            line(g.pts(&[(8.0, 14.5), (9.5, 19.5)]));
        }
        Icon::Bot => {
            p.rect_stroke(Rect::from_min_max(g.p(4.5, 8.0), g.p(19.5, 19.5)), 3.5 * scale, stroke, eframe::egui::StrokeKind::Middle);
            dot(9.3, 13.5, 1.4);
            dot(14.7, 13.5, 1.4);
            line(g.pts(&[(12.0, 8.0), (12.0, 5.0)]));
            dot(12.0, 4.0, 1.4);
        }
        Icon::File => {
            closed(g.pts(&[(6.0, 3.0), (14.0, 3.0), (19.0, 8.0), (19.0, 21.0), (6.0, 21.0)]));
            line(g.pts(&[(14.0, 3.0), (14.0, 8.0), (19.0, 8.0)]));
        }
        Icon::Camera => {
            let mut pts = g.pts(&[(3.0, 7.5), (8.0, 7.5), (9.5, 5.0), (14.5, 5.0), (16.0, 7.5), (21.0, 7.5), (21.0, 19.0), (3.0, 19.0)]);
            pts.dedup();
            closed(pts);
            circle(12.0, 13.0, 3.6);
        }
        Icon::Bookmark => closed(g.pts(&[(6.0, 3.0), (18.0, 3.0), (18.0, 21.0), (12.0, 16.0), (6.0, 21.0)])),
        Icon::Trash => {
            line(g.pts(&[(4.0, 6.5), (20.0, 6.5)]));
            line(g.pts(&[(9.5, 6.5), (9.5, 3.5), (14.5, 3.5), (14.5, 6.5)]));
            closed(g.pts(&[(6.0, 6.5), (7.0, 20.5), (17.0, 20.5), (18.0, 6.5)]));
        }
        Icon::Download => {
            line(g.pts(&[(12.0, 3.5), (12.0, 15.0)]));
            line(g.pts(&[(7.0, 10.5), (12.0, 15.5), (17.0, 10.5)]));
            line(g.pts(&[(4.5, 20.0), (19.5, 20.0)]));
        }
        Icon::Clock => {
            circle(12.0, 12.0, 8.8);
            line(g.pts(&[(12.0, 7.0), (12.0, 12.0), (15.5, 14.0)]));
        }
        Icon::Location => {
            // Мітка на мапі: дуга зверху й загострений низ.
            let mut pts = g.arc(12.0, 9.5, 6.5, PI - 0.5, 2.0 * PI + 0.5);
            pts.push(g.p(12.0, 21.0));
            closed(pts);
            circle(12.0, 9.5, 2.3);
        }
        Icon::ChevronLeft | Icon::ChevronRight => {
            let g = Grid { flip: icon == Icon::ChevronRight, ..g };
            line(g.pts(&[(15.0, 5.0), (8.0, 12.0), (15.0, 19.0)]));
        }
        Icon::Copy => {
            closed(g.pts(&[(8.5, 8.5), (20.0, 8.5), (20.0, 20.0), (8.5, 20.0)]));
            line(g.pts(&[(15.5, 8.5), (15.5, 4.0), (4.0, 4.0), (4.0, 15.5), (8.5, 15.5)]));
        }
    }
}
