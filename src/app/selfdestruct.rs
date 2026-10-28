//! Privacy and Security → "Delete my account if away for…", with the periods Telegram Desktop offers.

use super::*;

/// (days, label): 1, 3, 6, 12, 18 and 24 months as the server counts them.
fn periods() -> [(i32, &'static str); 6] {
    [
        (30, l!("1 місяць", "1 month")),
        (90, l!("3 місяці", "3 months")),
        (180, l!("6 місяців", "6 months")),
        (365, l!("1 рік", "1 year")),
        (548, l!("18 місяців", "18 months")),
        (730, l!("2 роки", "2 years")),
    ]
}

#[derive(Default)]
pub(super) struct SelfDestruct {
    /// Days of inactivity before the account is deleted (`None` — not loaded yet).
    days: Option<i32>,
}

impl App {
    pub(super) fn on_account_ttl(&mut self, days: i32) {
        self.self_destruct.days = Some(days);
    }

    pub(super) fn self_destruct_section(&mut self, ui: &mut Ui) {
        let Some(current) = self.self_destruct.days else { return };
        section_title(ui, l!("Видалити мій акаунт, якщо мене немає", "Delete my account if away for"));
        let mut set = None;
        ui.horizontal_wrapped(|ui| {
            ui.spacing_mut().item_spacing = vec2(6.0, 6.0);
            let w = ((ui.available_width() - 12.0) / 3.0).clamp(90.0, 140.0);
            for (days, label) in periods() {
                let (r, _) = ui.allocate_exact_size(vec2(w, 30.0), Sense::hover());
                if glass::button(ui, r, ("account-ttl", days), label, current == days, true).clicked() && current != days {
                    set = Some(days);
                }
            }
        });
        ui.label(egui::RichText::new(l!("Якщо ви жодного разу не зайдете за цей час, акаунт видалиться разом з усіма повідомленнями й контактами.", "If you don't come online at least once in this period, your account will be deleted along with all messages and contacts.")).color(ink_dim()).size(12.5));
        if let Some(days) = set {
            self.self_destruct.days = Some(days);
            self.send(Cmd::SetAccountTtl(days));
        }
    }
}
