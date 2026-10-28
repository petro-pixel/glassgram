//! "Logged in with Telegram" in Privacy and Security, as in the official apps: websites we logged
//! in to through a bot, each with "Disconnect", and "Disconnect all websites".

use super::*;
use crate::backend::WebSession;

#[derive(Default)]
pub(super) struct Websites {
    /// `None` — not loaded yet.
    list: Option<Vec<WebSession>>,
    /// "Disconnect all" was pressed once and asks again.
    confirm_all: bool,
}

impl App {
    pub(super) fn on_websites(&mut self, list: Vec<WebSession>) {
        self.websites.list = Some(list);
        self.websites.confirm_all = false;
    }

    /// Hidden while there are none, like in Telegram.
    pub(super) fn websites_section(&mut self, ui: &mut Ui) {
        let Some(list) = self.websites.list.as_ref().filter(|l| !l.is_empty()) else { return };
        section_title(ui, l!("Вхід через Telegram", "Logged in with Telegram"));
        let mut cmd = None;
        for s in list {
            let (row, _) = ui.allocate_exact_size(vec2(ui.available_width(), 64.0), Sense::hover());
            let p = ui.painter();
            p.rect_filled(row, 14.0, glass::veil(10));
            let x = row.left() + 12.0;
            let w = row.width() - 140.0;
            let title = if s.bot.is_empty() { s.domain.clone() } else { format!("{} · {}", s.domain, s.bot) };
            p.galley(pos2(x, row.top() + 6.0), elided(ui, &title, 14.5, ink(), w), ink());
            p.galley(pos2(x, row.top() + 26.0), elided(ui, &s.device, 12.5, ink_dim(), w), ink());
            let sub = if s.place.is_empty() { s.active.clone() } else { format!("{} · {}", s.place, s.active) };
            p.galley(pos2(x, row.top() + 43.0), elided(ui, &sub, 12.0, ink_dim(), row.width() - 24.0), ink());
            let b = Rect::from_min_size(pos2(row.right() - 124.0, row.top() + 8.0), vec2(116.0, 32.0));
            if glass::button(ui, b, ("web-logout", s.hash), l!("Відключити", "Disconnect"), false, true).clicked() {
                cmd = Some(Cmd::LogoutWebsite(s.hash));
            }
            ui.add_space(4.0);
        }
        let r = ui.allocate_exact_size(vec2(ui.available_width(), 36.0), Sense::hover()).0;
        let confirm = self.websites.confirm_all;
        let label = if confirm { l!("Точно відключити всі сайти?", "Really disconnect all websites?") } else { l!("Відключити всі сайти", "Disconnect all websites") };
        if glass::button(ui, r, "web-logout-all", label, confirm, true).clicked() {
            if confirm {
                cmd = Some(Cmd::LogoutAllWebsites);
            }
            self.websites.confirm_all = !confirm;
        }
        ui.label(egui::RichText::new(l!("Сайти, на які ви увійшли через Telegram. Відключений сайт попросить увійти знову.", "Websites you logged in to with Telegram. A disconnected site will ask you to log in again.")).color(ink_dim()).size(12.5));
        if let Some(c) = cmd {
            self.send(c);
        }
    }
}
