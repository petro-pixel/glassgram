//! "Report" box for messages: the server's reasons, then an optional comment, as in Telegram Desktop.

use super::*;
use crate::backend::ReportStep;

pub(super) struct ReportBox {
    peer: PeerId,
    ids: Vec<i32>,
    /// The current step; `None` while waiting for the server.
    step: Option<ReportStep>,
    /// The comment being typed for the `Comment` step.
    text: String,
}

impl App {
    pub(super) fn start_report(&mut self, peer: PeerId, ids: Vec<i32>) {
        self.report = Some(ReportBox { peer, ids: ids.clone(), step: None, text: String::new() });
        self.send(Cmd::Report { peer, ids, option: Vec::new(), message: String::new() });
    }

    pub(super) fn on_report_step(&mut self, peer: PeerId, ids: Vec<i32>, step: ReportStep) {
        let Some(b) = self.report.as_mut().filter(|b| b.peer == peer && b.ids == ids) else { return };
        if matches!(step, ReportStep::Done) {
            self.report = None;
            self.toast = Some((l!("Дякуємо! Модератори Telegram розглянуть скаргу.", "Thank you! Telegram moderators will review your report.").into(), 0.0, false));
        } else {
            b.step = Some(step);
        }
    }

    pub(super) fn report_view(&mut self, ui: &mut Ui, peer: PeerId, rect: Rect) {
        let Some(b) = &mut self.report else { return };
        if b.peer != peer {
            self.report = None;
            return;
        }
        let rows = match &b.step {
            Some(ReportStep::Choose { options, .. }) => options.len(),
            _ => 3,
        };
        let h = (60.0 + rows as f32 * 40.0 + 16.0).min(rect.height() - 40.0);
        let panel = Rect::from_center_size(rect.center(), vec2(rect.width().min(380.0), h));
        glass::glass(ui.painter(), panel, Glass::panel(24.0).with_shadow());
        ui.interact(panel, ui.scope_id().with("report-bg"), Sense::click());
        let title = match &b.step {
            Some(ReportStep::Choose { title, .. }) if !title.is_empty() => title.clone(),
            _ => l!("Поскаржитися", "Report").to_owned(),
        };
        let p = ui.painter();
        p.galley(pos2(panel.left() + 18.0, panel.top() + 18.0), elided(ui, &title, 16.0, ink(), panel.width() - 80.0), ink());
        let close = Rect::from_center_size(pos2(panel.right() - 30.0, panel.top() + 28.0), vec2(34.0, 34.0));
        let esc = ui.input_mut(|i| i.consume_key(egui::Modifiers::NONE, Key::Escape));
        if glass::icon_button(ui, close, "report-close", Icon::Close, false, true).clicked() || esc {
            self.report = None;
            return;
        }
        let body = Rect::from_min_max(pos2(panel.left() + 12.0, panel.top() + 56.0), pos2(panel.right() - 12.0, panel.bottom() - 12.0));
        let mut next: Option<(Vec<u8>, String)> = None;
        match &mut b.step {
            None => {
                ui.put(Rect::from_center_size(body.center(), vec2(22.0, 22.0)), egui::Spinner::new().size(22.0).color(ink()));
            }
            Some(ReportStep::Choose { options, .. }) => {
                let mut child = ui.new_child(UiBuilder::new().max_rect(body));
                egui::ScrollArea::vertical().id_salt("report-options").auto_shrink(false).show(&mut child, |ui| {
                    for (text, option) in options.iter() {
                        let (r, resp) = ui.allocate_exact_size(vec2(ui.available_width(), 38.0), Sense::click());
                        if resp.hovered() {
                            ui.painter().rect_filled(r, 12.0, glass::veil(16));
                            ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
                        }
                        ui.painter().galley(pos2(r.left() + 8.0, r.center().y - 9.0), elided(ui, text, 14.0, ink(), r.width() - 16.0), ink());
                        if resp.clicked() {
                            next = Some((option.clone(), String::new()));
                        }
                    }
                });
            }
            Some(ReportStep::Comment { option, optional }) => {
                let field = Rect::from_min_size(body.min, vec2(body.width(), 36.0));
                let hint = if *optional { l!("Коментар (необов'язково)", "Comment (optional)") } else { l!("Опишіть проблему", "Describe the problem") };
                glass::text_field(ui, field, &mut b.text, hint, false);
                let ok = !b.text.trim().is_empty() || *optional;
                let send = Rect::from_min_size(pos2(body.left(), field.bottom() + 12.0), vec2(body.width(), 34.0));
                if glass::button(ui, send, "report-send", l!("Надіслати скаргу", "Send report"), true, ok).clicked() {
                    next = Some((option.clone(), b.text.trim().to_owned()));
                }
            }
            Some(ReportStep::Done) => {}
        }
        if let Some((option, message)) = next {
            let (peer, ids) = (b.peer, b.ids.clone());
            b.step = None;
            self.send(Cmd::Report { peer, ids, option, message });
        }
    }
}
