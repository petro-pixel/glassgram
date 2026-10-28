//! Auto-deleting messages, as in Telegram Desktop: the default timer for new chats in Privacy
//! and Security, and a chat's own timer in its info panel.

use super::*;
use crate::backend::ChatDetails;

const DAY: i32 = 86400;

/// The timers the official apps offer.
fn choices() -> [(i32, &'static str); 4] {
    [(0, l!("Вимкнено", "Off")), (DAY, l!("1 день", "1 day")), (7 * DAY, l!("1 тиждень", "1 week")), (31 * DAY, l!("1 місяць", "1 month"))]
}

/// A timer in words; ones set elsewhere (another app's custom value) in days or hours.
pub(super) fn ttl_label(secs: i32) -> String {
    if let Some((_, label)) = choices().into_iter().find(|(s, _)| *s == secs) {
        return label.to_owned();
    }
    match secs {
        s if s >= DAY => {
            let n = s / DAY;
            lf!("{n} дн.", "{n} days")
        }
        s => {
            let n = (s / 3600).max(1);
            lf!("{n} год.", "{n} hours")
        }
    }
}

#[derive(Default)]
pub(super) struct AutoDelete {
    /// The timer for new chats (`None` — not loaded yet).
    default: Option<i32>,
}

/// The "Auto-delete messages" row of a chat's info and, unfolded, its timers; the chosen one is returned.
pub(super) fn ttl_rows(ui: &mut Ui, d: &ChatDetails, open: &mut bool) -> Option<i32> {
    if !d.can_ttl && d.ttl == 0 {
        return None;
    }
    if info::info_row(ui, Icon::Clock, &ttl_label(d.ttl), l!("Автовидалення повідомлень", "Auto-delete messages")).clicked() && d.can_ttl {
        *open = !*open;
    }
    if !*open {
        return None;
    }
    let mut chosen = None;
    for (secs, label) in choices() {
        if info::menu_row(ui, label, d.ttl == secs).clicked() {
            chosen = Some(secs);
        }
    }
    if chosen.is_some() {
        *open = false;
    }
    chosen
}

impl App {
    pub(super) fn on_default_ttl(&mut self, period: i32) {
        self.autodelete.default = Some(period);
    }

    pub(super) fn on_history_ttl(&mut self, peer: PeerId, period: i32) {
        if let Some(d) = self.info_details_mut(peer) {
            d.ttl = period;
        }
    }

    /// Privacy and Security → "Auto-delete messages": the timer every new chat starts with.
    pub(super) fn default_ttl_section(&mut self, ui: &mut Ui) {
        let Some(current) = self.autodelete.default else { return };
        section_title(ui, l!("Автовидалення повідомлень", "Auto-delete messages"));
        let mut set = None;
        ui.horizontal_wrapped(|ui| {
            ui.spacing_mut().item_spacing = vec2(6.0, 6.0);
            // Four in one line, as wide as the page allows.
            let w = ((ui.available_width() - 18.0) / 4.0).clamp(80.0, 120.0);
            for (secs, label) in choices() {
                let (r, _) = ui.allocate_exact_size(vec2(w, 30.0), Sense::hover());
                if glass::button(ui, r, ("default-ttl", secs), label, current == secs, true).clicked() && current != secs {
                    set = Some(secs);
                }
            }
        });
        ui.label(egui::RichText::new(l!("У нових чатах, які ви почнете, повідомлення видалятимуться самі через цей час. Наявних чатів це не змінює.", "In new chats you start, messages will delete themselves after this time. Existing chats don't change.")).color(ink_dim()).size(12.5));
        if let Some(secs) = set {
            self.autodelete.default = Some(secs);
            self.send(Cmd::SetDefaultTtl(secs));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn labels_for_custom_timers() {
        assert!(ttl_label(2 * DAY).starts_with('2'));
        assert!(ttl_label(1800).starts_with('1'));
        assert_eq!(ttl_label(0), l!("Вимкнено", "Off"));
    }
}
