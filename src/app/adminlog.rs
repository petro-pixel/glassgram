//! "Recent actions" of a channel or supergroup, as in Telegram Desktop: who changed what, newest first.

use super::*;
use crate::backend::AdminLogEntry;

/// Events per page, as the backend asks for them.
const PAGE: usize = 100;

#[derive(Default)]
pub(super) struct AdminLog {
    /// The chat whose log is open.
    peer: Option<PeerId>,
    entries: Vec<AdminLogEntry>,
    loading: bool,
    /// The last page was short: nothing older.
    exhausted: bool,
}

impl App {
    pub(super) fn open_admin_log(&mut self, peer: PeerId) {
        self.admin_log = AdminLog { peer: Some(peer), loading: true, ..Default::default() };
        self.send(Cmd::LoadAdminLog { peer, before: None });
    }

    pub(super) fn admin_log_open(&self) -> bool {
        self.admin_log.peer.is_some()
    }

    pub(super) fn on_admin_log(&mut self, peer: PeerId, entries: Vec<AdminLogEntry>, older: bool) {
        let log = &mut self.admin_log;
        if log.peer != Some(peer) {
            return;
        }
        log.loading = false;
        log.exhausted = entries.len() < PAGE;
        if !older {
            log.entries.clear();
        }
        log.entries.extend(entries);
    }

    pub(super) fn admin_log_view(&mut self, ui: &mut Ui) {
        let Some(peer) = self.admin_log.peer else { return };
        let screen = ui.max_rect();
        ui.painter().rect_filled(screen, 0.0, Color32::from_black_alpha(90));
        ui.interact(screen, ui.scope_id().with("admin-log-bg"), Sense::click());
        let rect = Rect::from_center_size(screen.center(), vec2(screen.width().min(480.0) - 20.0, (screen.height() - 60.0).min(640.0)));
        glass::glass(ui.painter(), rect, Glass::panel(26.0).with_shadow());
        let inner = rect.shrink(18.0);
        ui.painter().text(inner.left_top() + vec2(0.0, 12.0), Align2::LEFT_CENTER, l!("Останні дії", "Recent actions"), FontId::proportional(17.0), ink());
        let close = Rect::from_min_size(pos2(inner.right() - 32.0, inner.top() - 4.0), vec2(32.0, 32.0));
        let esc = ui.input_mut(|i| i.consume_key(egui::Modifiers::NONE, Key::Escape));
        if glass::icon_button(ui, close, "admin-log-close", Icon::Close, false, true).clicked() || esc {
            self.admin_log = AdminLog::default();
            return;
        }
        let body = Rect::from_min_max(pos2(inner.left(), inner.top() + 40.0), inner.max);
        let mut child = ui.new_child(UiBuilder::new().max_rect(body));
        child.set_clip_rect(body);
        let log = &self.admin_log;
        let mut more = false;
        egui::ScrollArea::vertical().id_salt("admin-log").auto_shrink(false).show(&mut child, |ui| {
            ui.spacing_mut().item_spacing.y = 4.0;
            if log.entries.is_empty() {
                if log.loading {
                    ui.add(egui::Spinner::new().color(ink_dim()));
                } else {
                    ui.label(egui::RichText::new(l!("За останні 48 годин дій не було.", "No actions in the last 48 hours.")).color(ink_dim()).size(13.5));
                }
            }
            for e in &log.entries {
                let w = ui.available_width() - 8.0;
                let h = if e.detail.is_empty() { 40.0 } else { 58.0 };
                let (row, _) = ui.allocate_exact_size(vec2(w, h), Sense::hover());
                let p = ui.painter();
                p.rect_filled(row, 12.0, glass::veil(10));
                let date = p.layout_no_wrap(e.date.clone(), FontId::proportional(11.0), ink_dim());
                let text_w = row.width() - date.size().x - 30.0;
                p.galley(pos2(row.right() - 10.0 - date.size().x, row.top() + 9.0), date, ink_dim());
                let line = format!("{} {}", e.who, e.what);
                p.galley(pos2(row.left() + 12.0, row.top() + 8.0), elided(ui, &line, 14.0, ink(), text_w), ink());
                if !e.detail.is_empty() {
                    p.galley(pos2(row.left() + 12.0, row.top() + 30.0), elided(ui, &e.detail, 12.5, ink_dim(), row.width() - 24.0), ink_dim());
                }
            }
            if !log.entries.is_empty() && !log.exhausted {
                let r = ui.allocate_exact_size(vec2(ui.available_width() - 8.0, 34.0), Sense::hover()).0;
                more = glass::button(ui, r, "admin-log-more", l!("Раніші дії", "Older actions"), false, !log.loading).clicked();
            }
        });
        if more {
            let before = self.admin_log.entries.last().map(|e| e.id);
            self.admin_log.loading = true;
            self.send(Cmd::LoadAdminLog { peer, before });
        }
    }
}
