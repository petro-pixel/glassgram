//! The chart widget of the statistics page: lines, steps, stacked bars and 100% areas from
//! Telegram's chart JSON, with a grid, a hover tooltip and legend chips that hide series.

use std::collections::HashSet;

use eframe::egui::{self, Align2, Color32, FontId, Mesh, Pos2, Rect, Sense, Shape, Stroke, Ui, pos2, vec2};

use crate::backend::{Chart, ChartKind};
use crate::glass::{self, ink, ink_dim};
use crate::i18n::{l, lf};

/// Height of the plot itself, without the x labels and the legend.
const PLOT_H: f32 = 190.0;
/// Horizontal grid lines, the baseline included.
const GRID: usize = 5;

/// 950 → "950", 12 400 → "12.4K", 0.5 → "0.5", like Telegram's numbers.
pub(super) fn compact(v: f64) -> String {
    shorten(v, 1)
}

/// `compact` with up to `digits` decimals, trailing zeros dropped (axis steps like 1.15K).
fn shorten(v: f64, digits: usize) -> String {
    let short = |v: f64| {
        let s = format!("{v:.digits$}");
        if s.contains('.') { s.trim_end_matches('0').trim_end_matches('.').to_owned() } else { s }
    };
    let a = v.abs();
    if a >= 1e6 {
        lf!("{} млн", "{}M", short(v / 1e6))
    } else if a >= 1e3 {
        lf!("{} тис.", "{}K", short(v / 1e3))
    } else if v.fract().abs() > 0.05 {
        short(v)
    } else {
        format!("{}", v.round() as i64)
    }
}

/// An x value as a label: a date, or an hour of the day for the "by hours" charts.
fn x_label(chart: &Chart, x: i64, long: bool) -> String {
    if !chart.x_is_time() {
        return if chart.x.len() == 24 && (0..24).contains(&x) { format!("{x}:00") } else { x.to_string() };
    }
    let Ok(ts) = jiff::Timestamp::from_millisecond(x) else { return String::new() };
    // Daily points are dated in UTC by the server: local time would shift them a day.
    let z = ts.to_zoned(jiff::tz::TimeZone::UTC);
    let hourly = chart.x.windows(2).next().is_some_and(|w| w[1] - w[0] < 86_400_000);
    match (long, hourly) {
        (true, true) => z.strftime("%d.%m.%Y %H:%M").to_string(),
        (true, false) => z.strftime("%d.%m.%Y").to_string(),
        (false, true) => z.strftime("%H:%M").to_string(),
        (false, false) => z.strftime("%d.%m").to_string(),
    }
}

/// The smallest of 1, 2, 2.5, 5 × 10ⁿ at or above `v`.
fn nice_step(v: f64) -> f64 {
    let mag = 10f64.powf(v.log10().floor());
    [1.0, 2.0, 2.5, 5.0, 10.0].into_iter().map(|m| m * mag).find(|s| *s >= v * 0.999_999).unwrap_or(10.0 * mag)
}

/// Round bottom and top of a y axis around `lo..hi`, so the grid labels read well. Lines start
/// at their lowest value, as in Telegram, so a slow growth isn't drawn flat.
fn nice_range(lo: f64, hi: f64) -> (f64, f64) {
    let span = if hi > lo { hi - lo } else { hi.abs().max(1.0) };
    let mut step = nice_step(span / (GRID - 1) as f64);
    let mut bottom = (lo / step).floor() * step;
    while bottom + step * ((GRID - 1) as f64) < hi {
        step = nice_step(step * 1.01);
        bottom = (lo / step).floor() * step;
    }
    (bottom, bottom + step * (GRID - 1) as f64)
}

/// Draws the chart and its legend. `hidden` holds the series switched off in the legend;
/// `sent_dark` says which theme the server picked colours for.
pub(super) fn chart(ui: &mut Ui, chart: &Chart, hidden: &mut HashSet<usize>, sent_dark: bool) {
    let w = ui.available_width();
    let (rect, resp) = ui.allocate_exact_size(vec2(w, PLOT_H + 22.0), Sense::hover());
    let plot = Rect::from_min_max(pos2(rect.left() + 4.0, rect.top() + 18.0), pos2(rect.right() - 4.0, rect.top() + PLOT_H));
    let colors: Vec<Color32> = chart.series.iter().map(|s| glass::chart_color(s.color, sent_dark)).collect();
    let shown: Vec<usize> = (0..chart.series.len()).filter(|i| !hidden.contains(i)).collect();
    let n = chart.x.len();
    let p = ui.painter_at(rect.expand(2.0));

    // The axis: per series for y_scaled lines, else shared.
    let lo_of = |i: usize| chart.series[i].values.iter().cloned().fold(f64::INFINITY, f64::min);
    let hi_of = |i: usize| chart.series[i].values.iter().cloned().fold(f64::NEG_INFINITY, f64::max);
    let axis = match chart.kind {
        ChartKind::Percent => (0.0, 100.0),
        ChartKind::Bars => nice_range(0.0, (0..n).map(|k| shown.iter().map(|&i| chart.series[i].values[k]).sum::<f64>()).fold(0.0, f64::max)),
        _ => nice_range(shown.iter().map(|&i| lo_of(i)).fold(f64::INFINITY, f64::min).min(f64::MAX), shown.iter().map(|&i| hi_of(i)).fold(0.0, f64::max)),
    };
    let axes: Vec<(f64, f64)> = (0..chart.series.len()).map(|i| if chart.y_scaled { nice_range(lo_of(i), hi_of(i)) } else { axis }).collect();

    // Grid lines under the series, their labels over them (100% areas cover the whole plot).
    let label_font = FontId::proportional(11.0);
    let grid_y = |g: usize| plot.bottom() - plot.height() * g as f32 / (GRID - 1) as f32;
    for g in 0..GRID {
        p.line_segment([pos2(plot.left(), grid_y(g)), pos2(plot.right(), grid_y(g))], Stroke::new(1.0, glass::veil(if g == 0 { 40 } else { 16 })));
    }
    let y_labels = || {
        let plain = if chart.kind == ChartKind::Percent { glass::ON_DARK } else { ink_dim() };
        for g in 0..GRID {
            if g == 0 && axes.iter().all(|a| a.0 == 0.0) {
                continue;
            }
            let f = g as f64 / (GRID - 1) as f64;
            let y = grid_y(g);
            let at = |i: usize| {
                let (b, t) = axes[i];
                let v = b + (t - b) * f;
                if chart.kind == ChartKind::Percent { format!("{}%", v.round()) } else { shorten(v, 2) }
            };
            if chart.y_scaled && shown.len() == 2 {
                p.text(pos2(plot.left(), y - 3.0), Align2::LEFT_BOTTOM, at(shown[0]), label_font.clone(), colors[shown[0]]);
                p.text(pos2(plot.right(), y - 3.0), Align2::RIGHT_BOTTOM, at(shown[1]), label_font.clone(), colors[shown[1]]);
            } else if let Some(&i) = shown.first() {
                p.text(pos2(plot.left() + 2.0, y - 3.0), Align2::LEFT_BOTTOM, at(i), label_font.clone(), plain);
            }
        }
    };
    if n == 0 {
        y_labels();
        return;
    }
    // Bars take a slot each; lines run from edge to edge.
    let slot = plot.width() / n as f32;
    let x_at = |k: usize| match chart.kind {
        ChartKind::Bars => plot.left() + slot * (k as f32 + 0.5),
        _ if n == 1 => plot.center().x,
        _ => plot.left() + plot.width() * k as f32 / (n - 1) as f32,
    };
    let y_at = |v: f64, (b, t): (f64, f64)| plot.bottom() - plot.height() * ((v - b) / (t - b)).clamp(0.0, 1.05) as f32;

    // X labels: about one per 90 points, never overlapping.
    let labels = ((plot.width() / 90.0) as usize).clamp(2, 7).min(n);
    for j in 0..labels {
        let k = if labels == 1 { 0 } else { j * (n - 1) / (labels - 1) };
        let align = if j == 0 { Align2::LEFT_TOP } else if j + 1 == labels { Align2::RIGHT_TOP } else { Align2::CENTER_TOP };
        p.text(pos2(x_at(k), plot.bottom() + 5.0), align, x_label(chart, chart.x[k], false), label_font.clone(), ink_dim());
    }

    let hover = resp.hover_pos().filter(|h| plot.expand2(vec2(0.0, 6.0)).contains(*h)).map(|h| {
        (0..n).min_by(|&a, &b| (x_at(a) - h.x).abs().total_cmp(&(x_at(b) - h.x).abs())).unwrap_or(0)
    });

    match chart.kind {
        ChartKind::Line | ChartKind::Step => {
            for &i in &shown {
                let v = &chart.series[i].values;
                let mut pts: Vec<Pos2> = Vec::with_capacity(n * 2);
                for k in 0..n {
                    let pt = pos2(x_at(k), y_at(v[k], axes[i]));
                    if chart.kind == ChartKind::Step && k > 0 {
                        pts.push(pos2(pt.x, y_at(v[k - 1], axes[i])));
                    }
                    pts.push(pt);
                }
                p.add(Shape::line(pts, Stroke::new(2.0, colors[i])));
            }
            if let Some(k) = hover {
                let x = x_at(k);
                p.line_segment([pos2(x, plot.top()), pos2(x, plot.bottom())], Stroke::new(1.0, glass::veil(60)));
                for &i in &shown {
                    let c = pos2(x, y_at(chart.series[i].values[k], axes[i]));
                    p.circle(c, 4.0, glass::chrome(), Stroke::new(2.0, colors[i]));
                }
            }
        }
        ChartKind::Bars => {
            let bw = (slot * 0.78).max(1.0);
            for k in 0..n {
                let dim = hover.is_some_and(|h| h != k);
                let mut base = 0.0;
                for &i in &shown {
                    let v = chart.series[i].values[k];
                    let r = Rect::from_x_y_ranges(x_at(k) - bw / 2.0..=x_at(k) + bw / 2.0, y_at(base + v, axis)..=y_at(base, axis));
                    let c = if dim { glass::rgba(colors[i], 150) } else { colors[i] };
                    p.rect_filled(r, if bw > 6.0 { 2.0 } else { 0.0 }, c);
                    base += v;
                }
            }
        }
        ChartKind::Percent => {
            // Each band is a strip of quads between its lower and upper edge.
            let sums: Vec<f64> = (0..n).map(|k| shown.iter().map(|&i| chart.series[i].values[k]).sum::<f64>().max(f64::EPSILON)).collect();
            let mut lo = vec![0.0; n];
            for &i in &shown {
                let hi: Vec<f64> = (0..n).map(|k| lo[k] + chart.series[i].values[k] / sums[k] * 100.0).collect();
                let mut mesh = Mesh::default();
                for k in 0..n {
                    mesh.colored_vertex(pos2(x_at(k), y_at(lo[k], axis)), glass::rgba(colors[i], 200));
                    mesh.colored_vertex(pos2(x_at(k), y_at(hi[k], axis)), glass::rgba(colors[i], 200));
                    if k > 0 {
                        let b = (k as u32 - 1) * 2;
                        mesh.add_triangle(b, b + 1, b + 3);
                        mesh.add_triangle(b, b + 3, b + 2);
                    }
                }
                p.add(Shape::mesh(mesh));
                lo = hi;
            }
            if let Some(k) = hover {
                let x = x_at(k);
                p.line_segment([pos2(x, plot.top()), pos2(x, plot.bottom())], Stroke::new(1.5, glass::chrome()));
            }
        }
    }

    y_labels();
    if let Some(k) = hover {
        resp.on_hover_ui_at_pointer(|ui| tooltip(ui, chart, k, &shown, &colors));
    }
    legend(ui, chart, hidden, &colors);
}

/// The values at point `k`: the date, then one row per shown series.
fn tooltip(ui: &mut Ui, chart: &Chart, k: usize, shown: &[usize], colors: &[Color32]) {
    // Right-aligned values would otherwise stretch the tooltip across the window.
    ui.set_min_width(160.0);
    ui.set_max_width(240.0);
    ui.label(egui::RichText::new(x_label(chart, chart.x[k], true)).size(12.5).color(ink()).strong());
    let total: f64 = shown.iter().map(|&i| chart.series[i].values[k]).sum();
    for &i in shown {
        let s = &chart.series[i];
        let v = s.values[k];
        let value = match chart.kind {
            ChartKind::Percent if total > 0.0 => format!("{:.0}%", v / total * 100.0),
            _ => thousands(v),
        };
        ui.horizontal(|ui| {
            let (dot, _) = ui.allocate_exact_size(vec2(10.0, 14.0), Sense::hover());
            ui.painter().circle_filled(dot.center(), 4.0, colors[i]);
            ui.label(egui::RichText::new(&s.name).size(12.5).color(ink_dim()));
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                ui.label(egui::RichText::new(value).size(12.5).color(colors[i]).strong());
            });
        });
    }
    if chart.kind == ChartKind::Bars && shown.len() > 1 {
        ui.horizontal(|ui| {
            ui.add_space(14.0);
            ui.label(egui::RichText::new(l!("Усього", "All")).size(12.5).color(ink_dim()));
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                ui.label(egui::RichText::new(thousands(total)).size(12.5).color(ink()).strong());
            });
        });
    }
}

/// 12345 → "12 345": exact numbers in the tooltip.
pub(super) fn thousands(v: f64) -> String {
    if v.fract().abs() > 0.05 {
        return format!("{v:.1}");
    }
    let digits = (v.round() as i64).unsigned_abs().to_string();
    let mut out = String::new();
    for (j, ch) in digits.chars().enumerate() {
        if j > 0 && (digits.len() - j) % 3 == 0 {
            out.push('\u{202F}');
        }
        out.push(ch);
    }
    if v < 0.0 { format!("-{out}") } else { out }
}

/// Chips under the chart that switch series on and off; the last shown one stays.
fn legend(ui: &mut Ui, chart: &Chart, hidden: &mut HashSet<usize>, colors: &[Color32]) {
    if chart.series.len() < 2 {
        return;
    }
    ui.add_space(4.0);
    ui.horizontal_wrapped(|ui| {
        ui.spacing_mut().item_spacing = vec2(6.0, 6.0);
        for (i, s) in chart.series.iter().enumerate() {
            let on = !hidden.contains(&i);
            let font = FontId::proportional(12.5);
            // Dark text on light series colours (yellow), white on the rest.
            let [r, g, b, _] = colors[i].to_array();
            let on_color = if 0.299 * r as f32 + 0.587 * g as f32 + 0.114 * b as f32 > 170.0 { Color32::from_rgb(28, 30, 40) } else { Color32::WHITE };
            let text = ui.painter().layout_no_wrap(s.name.clone(), font, if on { on_color } else { colors[i] });
            let size = vec2(text.size().x + 34.0, 28.0);
            let (r, resp) = ui.allocate_exact_size(size, Sense::click());
            let p = ui.painter();
            if on {
                p.rect_filled(r, 14.0, colors[i]);
            } else {
                p.rect_stroke(r.shrink(0.75), 14.0, Stroke::new(1.5, colors[i]), egui::StrokeKind::Inside);
            }
            let mark = pos2(r.left() + 15.0, r.center().y);
            if on {
                crate::icons::draw(p, Rect::from_center_size(mark, vec2(12.0, 12.0)), crate::icons::Icon::Check, on_color);
            } else {
                p.circle_stroke(mark, 4.0, Stroke::new(1.5, colors[i]));
            }
            p.galley(pos2(r.left() + 25.0, r.center().y - text.size().y / 2.0), text, Color32::WHITE);
            if resp.hovered() {
                ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
            }
            if resp.clicked() && !(on && hidden.len() + 1 == chart.series.len()) {
                if on {
                    hidden.insert(i);
                } else {
                    hidden.remove(&i);
                }
            }
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn nice_axes() {
        let near = |a: (f64, f64), b: (f64, f64)| (a.0 - b.0).abs() < 1e-6 && (a.1 - b.1).abs() < 1e-6;
        assert!(near(nice_range(0.0, 0.0), (0.0, 1.0)));
        assert!(near(nice_range(0.0, 95.0), (0.0, 100.0)));
        assert!(near(nice_range(0.0, 3.0), (0.0, 4.0)));
        assert!(near(nice_range(12_100.0, 12_840.0), (12_000.0, 13_000.0)), "{:?}", nice_range(12_100.0, 12_840.0));
        let (b, t) = nice_range(37.0, 61.0);
        assert!(b <= 37.0 && t >= 61.0);
    }

    #[test]
    fn exact_numbers() {
        assert_eq!(thousands(12345.0), "12\u{202F}345");
        assert_eq!(thousands(950.0), "950");
        assert_eq!(thousands(2.5), "2.5");
    }
}
