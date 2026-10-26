//! Account menu under the name in the header: switching and adding accounts.

use super::*;

impl App {
    /// Switch to another account: stop the backend and start from a fresh state.
    pub(super) fn switch_account(&mut self, id: u32) {
        self.close_web_apps();
        self.save_chats();
        self.send(Cmd::Shutdown);
        self.accounts.active = id;
        self.accounts.save();
        let tray = self.tray.take();
        let tray_rx = std::mem::replace(&mut self.tray_rx, mpsc::channel().1);
        let mut next = Self::fresh(self.ctx.clone(), self.settings.clone(), tray, tray_rx);
        // The passcode was already entered in this run, so don't lock the other account.
        next.locked = false;
        next.start_backend();
        *self = next;
    }

    /// Accounts menu under the name in the header: switching and adding.
    pub(super) fn accounts_menu(&mut self, ui: &mut Ui) {
        let screen = ui.max_rect();
        // A click outside the menu closes it.
        if ui.interact(screen, ui.scope_id().with("accounts-bg"), Sense::click()).clicked() {
            self.accounts_open = false;
            return;
        }
        let rows = self.accounts.list.len() as f32 + 1.0;
        let rect = Rect::from_min_size(self.accounts_anchor.left_bottom() + vec2(-6.0, 8.0), vec2(260.0, rows * 44.0 + 16.0));
        glass::glass(ui.painter(), rect, Glass::panel(20.0).with_shadow());
        ui.interact(rect, ui.scope_id().with("accounts-panel"), Sense::click());
        let mut pick = None;
        let mut add = false;
        let mut y = rect.top() + 8.0;
        for a in &self.accounts.list {
            let r = Rect::from_min_size(pos2(rect.left() + 8.0, y), vec2(rect.width() - 16.0, 40.0));
            let resp = ui.interact(r, ui.scope_id().with(("account", a.id)), Sense::click());
            let p = ui.painter();
            if resp.hovered() {
                p.rect_filled(r, 12.0, glass::veil(16));
            }
            let name = if a.name.is_empty() { l!("Новий акаунт", "New account").to_owned() } else { a.name.clone() };
            glass::avatar(p, pos2(r.left() + 20.0, r.center().y), 15.0, &name, a.id as i64 + 7, None, None);
            p.galley(pos2(r.left() + 44.0, r.center().y - 9.0), elided(ui, &name, 14.5, ink(), r.width() - 80.0), ink());
            if a.id == self.accounts.active {
                icons::draw(p, Rect::from_center_size(pos2(r.right() - 16.0, r.center().y), vec2(18.0, 18.0)), Icon::Check, ACCENT);
            }
            if resp.clicked() && a.id != self.accounts.active {
                pick = Some(a.id);
            }
            y += 44.0;
        }
        let r = Rect::from_min_size(pos2(rect.left() + 8.0, y), vec2(rect.width() - 16.0, 40.0));
        if glass::button(ui, r, "account-add", l!("Додати акаунт", "Add account"), false, true).clicked() {
            add = true;
        }
        if self.demo && (pick.is_some() || add) {
            self.accounts_open = false;
            self.toast = Some((l!("У демо-режимі акаунти не перемикаються", "Accounts can't be switched in demo mode").into(), 0.0, true));
        } else if let Some(id) = pick {
            self.accounts_open = false;
            self.switch_account(id);
        } else if add {
            self.accounts_open = false;
            self.add_account();
        } else if ui.input(|i| i.key_pressed(Key::Escape)) {
            self.accounts_open = false;
        }
    }

    /// Add an account: a new empty entry and sign-in to it.
    pub(super) fn add_account(&mut self) {
        let id = self.accounts.add();
        self.switch_account(id);
    }
}
