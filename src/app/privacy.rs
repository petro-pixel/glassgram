//! Global privacy switches on the Privacy and Security page, as in Telegram Desktop:
//! new chats from non-contacts, read times, who can message me.

use super::*;

impl App {
    pub(super) fn global_privacy_section(&mut self, ui: &mut Ui) {
        let Some(before) = self.global_privacy else {
            section_title(ui, l!("Повідомлення", "Messages"));
            ui.add(egui::Spinner::new().color(ink_dim()));
            return;
        };
        let mut p = before;
        let note = |ui: &mut Ui, text: &str| {
            ui.label(egui::RichText::new(text).color(ink_dim()).size(12.5));
        };

        section_title(ui, l!("Хто може мені писати", "Who can message me"));
        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = 6.0;
            for (premium_only, label) in [(false, l!("Усі", "Everybody")), (true, l!("Контакти й Premium", "Contacts and Premium"))] {
                let (r, _) = ui.allocate_exact_size(vec2(180.0, 30.0), Sense::hover());
                if glass::button(ui, r, ("who-can-message", premium_only), label, p.premium_only == premium_only, true).clicked() {
                    p.premium_only = premium_only;
                }
            }
        });
        note(ui, l!("Обмежити нові чати контактами й користувачами Premium може лише Telegram Premium.", "Limiting new chats to contacts and Premium users needs Telegram Premium."));

        section_title(ui, l!("Нові чати не з контактів", "New chats from non-contacts"));
        ui.checkbox(&mut p.archive_new, l!("Архівувати й вимикати звук", "Archive and mute"));
        ui.checkbox(&mut p.keep_unmuted, l!("Залишати в архіві чати зі звуком", "Keep unmuted chats archived"));
        ui.checkbox(&mut p.keep_folders, l!("Залишати в архіві чати з папок", "Keep chats from folders archived"));
        note(ui, l!("Архівовані чати повертаються в список із новим повідомленням, якщо ці пункти вимкнено.", "With these off, archived chats come back to the list on a new message."));

        section_title(ui, l!("Час прочитання", "Read time"));
        ui.checkbox(&mut p.hide_read, l!("Приховувати час прочитання", "Hide read time"));
        note(ui, l!("Інші не бачитимуть, коли ви прочитали їхні повідомлення, а ви — їхній час прочитання.", "Others won't see when you've read their messages, and you won't see theirs."));

        if p != before {
            self.global_privacy = Some(p);
            self.send(Cmd::SetGlobalPrivacy(p));
        }
    }
}
