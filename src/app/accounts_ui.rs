//! Account menu under the name in the header: switching and adding accounts. The other
//! accounts stay connected in the background, for their unread counters and notifications.

use std::collections::HashSet;

use super::*;

/// How often the background accounts refresh their dialogs, so the counters follow reads made elsewhere.
const REFRESH: f64 = 300.0;

/// An account that is not on screen: its own backend, which we only listen to.
pub(super) struct Background {
    id: u32,
    /// `None` only in the demo, which has no network.
    handle: Option<Handle>,
    /// Unread messages in unmuted, unarchived chats.
    unread: i32,
    /// Chats that don't notify, and every chat's title, from its dialogs.
    muted: HashSet<PeerId>,
    titles: HashMap<PeerId, String>,
    refreshed_at: f64,
}

impl Background {
    pub(super) fn demo(id: u32, unread: i32) -> Self {
        Self { id, handle: None, unread, muted: HashSet::new(), titles: HashMap::new(), refreshed_at: 0.0 }
    }
}

/// Unread messages that count for an account's badge.
fn unread_of(chats: &[ChatInfo]) -> i32 {
    chats.iter().filter(|c| !c.muted && !c.archived).map(|c| c.unread.max(c.unread_mark as i32)).sum()
}

impl App {
    /// Connect the accounts that are not active (those signed in before).
    pub(super) fn start_background(&mut self) {
        let Some((api_id, api_hash)) = backend::saved_credentials() else { return };
        let now = self.ctx.input(|i| i.time);
        for a in &self.accounts.list {
            let session = accounts::session_path(a.id);
            if a.id == self.accounts.active || !session.exists() {
                continue;
            }
            let handle = backend::spawn(self.ctx.clone(), session);
            let _ = handle.tx.send(Cmd::Connect { api_id, api_hash: api_hash.clone() });
            self.background.push(Background { id: a.id, handle: Some(handle), unread: 0, muted: HashSet::new(), titles: HashMap::new(), refreshed_at: now });
        }
    }

    pub(super) fn stop_background(&mut self) {
        for h in self.background.iter().filter_map(|b| b.handle.as_ref()) {
            let _ = h.tx.send(Cmd::Shutdown);
        }
        for h in self.background.drain(..).filter_map(|b| b.handle) {
            let _ = h.done.recv_timeout(std::time::Duration::from_secs(2));
        }
    }

    /// What the background accounts heard: counters and notifications; the rest is not ours to show.
    pub(super) fn poll_background(&mut self) {
        let now = self.ctx.input(|i| i.time);
        let mut notes = Vec::new();
        for b in &mut self.background {
            let Some(handle) = &b.handle else { continue };
            for ev in handle.rx.try_iter() {
                match ev {
                    Ev::Dialogs(chats) => {
                        b.unread = unread_of(&chats);
                        b.muted = chats.iter().filter(|c| c.muted).map(|c| c.id).collect();
                        b.titles = chats.iter().map(|c| (c.id, c.title.clone())).collect();
                    }
                    Ev::NewMsg(m) if !m.out && !b.muted.contains(&m.peer) => {
                        b.unread += 1;
                        let chat = b.titles.get(&m.peer).cloned().unwrap_or_default();
                        notes.push((b.id, chat, m));
                    }
                    _ => {}
                }
            }
            if now - b.refreshed_at > REFRESH {
                b.refreshed_at = now;
                let _ = handle.tx.send(Cmd::ReloadDialogs);
            }
        }
        if !self.settings.notifications {
            return;
        }
        for (id, chat, m) in notes {
            // Under a local passcode, don't show who sent what.
            if self.lock.on {
                notify("Glassgram", l!("Нове повідомлення", "New message"));
                continue;
            }
            let account = self.accounts.list.iter().find(|a| a.id == id).map(|a| a.name.clone()).unwrap_or_default();
            let title = if chat.is_empty() { account } else { format!("{account} · {chat}") };
            let body = if m.sender.is_empty() { quote(&m) } else { format!("{}: {}", m.sender, quote(&m)) };
            // Even with the window focused: this account isn't the one on screen.
            notify(&title, &body);
        }
    }

    /// Unread messages of the other accounts, for the dot by the name.
    pub(super) fn background_unread(&self) -> i32 {
        self.background.iter().map(|b| b.unread).sum()
    }

    /// Switch to another account: stop the backend and start from a fresh state.
    pub(super) fn switch_account(&mut self, id: u32) {
        self.close_web_apps();
        self.save_chats();
        self.send(Cmd::Shutdown);
        self.stop_background();
        self.accounts.active = id;
        self.accounts.save();
        let tray = self.tray.take();
        let tray_rx = std::mem::replace(&mut self.tray_rx, mpsc::channel().1);
        let mut next = Self::fresh(self.ctx.clone(), self.settings.clone(), tray, tray_rx);
        // The passcode was already entered in this run, so don't lock the other account.
        next.lock.on = false;
        next.start_backend();
        next.start_background();
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
            } else if let Some(n) = self.background.iter().find(|b| b.id == a.id).map(|b| b.unread).filter(|n| *n > 0) {
                let text = if n > 99 { "99+".to_owned() } else { n.to_string() };
                let g = elided(ui, &text, 11.5, glass::ON_DARK, 40.0);
                let badge = Rect::from_center_size(pos2(r.right() - 18.0, r.center().y), vec2((g.size().x + 12.0).max(20.0), 20.0));
                glass::glass(p, badge, Glass::tinted(10.0, ACCENT, 230));
                p.galley(badge.center() - g.size() / 2.0, g, glass::ON_DARK);
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
