//! "Notifications for chats" in Settings → Notifications, as in Telegram Desktop:
//! private chats, groups and channels on or off, with the chats that differ (exceptions).

use std::collections::HashMap;

use super::*;
use crate::backend::{NotifyException, NotifyScope, NotifyState};

const ROW_H: f32 = 52.0;
const EXCEPTION_H: f32 = 46.0;

/// Category defaults and exceptions; kept in one field of `App`.
#[derive(Default)]
pub(super) struct NotifyPrefs {
    defaults: HashMap<NotifyScope, NotifyState>,
    exceptions: HashMap<NotifyScope, Vec<NotifyException>>,
    /// Category whose exceptions are unfolded.
    open: Option<NotifyScope>,
}

fn scope_title(scope: NotifyScope) -> &'static str {
    match scope {
        NotifyScope::Private => l!("Особисті чати", "Private chats"),
        NotifyScope::Groups => l!("Групи", "Groups"),
        NotifyScope::Channels => l!("Канали", "Channels"),
    }
}

fn scope_icon(scope: NotifyScope) -> Icon {
    match scope {
        NotifyScope::Private => Icon::User,
        NotifyScope::Groups => Icon::Users,
        NotifyScope::Channels => Icon::Channel,
    }
}

/// "On / Off" switch, drawn flat inside a row (the page itself is glass).
fn switch(p: &egui::Painter, center: egui::Pos2, on: bool) {
    let r = Rect::from_center_size(center, vec2(40.0, 22.0));
    p.rect_filled(r, 11.0, if on { ACCENT } else { glass::veil(50) });
    let x = if on { r.right() - 11.0 } else { r.left() + 11.0 };
    p.circle_filled(pos2(x, r.center().y), 8.5, glass::ON_DARK);
}

impl App {
    pub(super) fn notify_prefs_event(&mut self, ev: Ev) {
        match ev {
            Ev::NotifyDefault { scope, state } => {
                // A change (ours or from another device) re-sorts which chats are muted.
                let changed = self.notify.defaults.get(&scope).is_some_and(|old| old.muted() != state.muted());
                self.notify.defaults.insert(scope, state);
                if changed && !self.demo {
                    self.send(Cmd::ReloadDialogs);
                }
            }
            Ev::NotifyExceptions { scope, mut list } => {
                list.sort_by_key(|e| e.title.to_lowercase());
                self.notify.exceptions.insert(scope, list);
            }
            _ => {}
        }
    }

    /// A chat's settings changed: keep the exception lists in step.
    pub(super) fn notify_changed(&mut self, peer: PeerId, state: NotifyState) {
        for list in self.notify.exceptions.values_mut() {
            if state.inherit {
                list.retain(|e| e.peer != peer);
            } else if let Some(e) = list.iter_mut().find(|e| e.peer == peer) {
                e.state = state;
            }
        }
    }

    /// Category default for the demo backend (all on until switched).
    pub fn demo_notify_default(&self, scope: NotifyScope) -> NotifyState {
        self.notify.defaults.get(&scope).copied().unwrap_or_default()
    }

    pub fn demo_chats(&self) -> &[ChatInfo] {
        &self.chats
    }

    pub(super) fn load_notify_exceptions(&mut self) {
        for scope in NotifyScope::ALL {
            self.send(Cmd::LoadNotifyExceptions(scope));
        }
    }

    pub(super) fn notify_scopes(&mut self, ui: &mut Ui) {
        section_title(ui, l!("Сповіщення для чатів", "Notifications for chats"));
        let mut toggle = None;
        let mut reset = None;
        for scope in NotifyScope::ALL {
            let state = self.notify.defaults.get(&scope).copied();
            let exceptions = self.notify.exceptions.get(&scope);
            let open = self.notify.open == Some(scope);
            let w = ui.available_width();
            let (row, resp) = ui.allocate_exact_size(vec2(w, ROW_H), Sense::click());
            let switch_at = pos2(row.right() - 32.0, row.center().y);
            let on_switch = resp.hover_pos().is_some_and(|p| (p - switch_at).length() < 24.0);
            let p = ui.painter().clone();
            if resp.hovered() {
                p.rect_filled(row, 14.0, glass::veil(16));
                ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
            }
            icons::draw(&p, Rect::from_center_size(pos2(row.left() + 22.0, row.center().y), vec2(22.0, 22.0)), scope_icon(scope), ink_dim());
            p.text(pos2(row.left() + 48.0, row.top() + 17.0), Align2::LEFT_CENTER, scope_title(scope), FontId::proportional(14.5), ink());
            let status = match state {
                None => l!("Завантаження…", "Loading…").to_owned(),
                Some(s) if s.muted() => l!("Вимкнено", "Off").to_owned(),
                Some(_) => l!("Увімкнено", "On").to_owned(),
            };
            let sub = match exceptions.map_or(0, Vec::len) {
                0 => status,
                n => lf!("{status}, винятків: {n}", "{status}, {n} exceptions"),
            };
            p.text(pos2(row.left() + 48.0, row.top() + 36.0), Align2::LEFT_CENTER, sub, FontId::proportional(12.5), ink_dim());
            if exceptions.is_some_and(|l| !l.is_empty()) {
                let icon = if open { Icon::ChevronDown } else { Icon::ChevronRight };
                icons::draw(&p, Rect::from_center_size(pos2(switch_at.x - 42.0, row.center().y), vec2(16.0, 16.0)), icon, ink_dim());
            }
            if let Some(s) = state {
                switch(&p, switch_at, !s.muted());
            }
            if resp.clicked() {
                if on_switch {
                    toggle = state.map(|s| (scope, !s.muted()));
                } else {
                    self.notify.open = if open { None } else { Some(scope) };
                }
            }
            if open && let Some(list) = exceptions {
                for e in list {
                    let (r, _) = ui.allocate_exact_size(vec2(w, EXCEPTION_H), Sense::hover());
                    let tex = self.textures.get(&ImgKey::Avatar(e.peer));
                    glass::avatar(&p, pos2(r.left() + 66.0, r.center().y), 16.0, &e.title, e.peer.bot_api_dialog_id_unchecked(), None, tex);
                    p.galley(pos2(r.left() + 92.0, r.top() + 5.0), elided(ui, &e.title, 14.0, ink(), w - 150.0), ink());
                    p.text(pos2(r.left() + 92.0, r.top() + 33.0), Align2::LEFT_CENTER, e.state.label(), FontId::proportional(12.0), ink_dim());
                    let b = Rect::from_center_size(pos2(r.right() - 32.0, r.center().y), vec2(36.0, 36.0));
                    if glass::icon_button(ui, b, ("notify-reset", e.peer), Icon::Close, false, true).on_hover_text(l!("Прибрати виняток", "Remove exception")).clicked() {
                        reset = Some(e.peer);
                    }
                }
            }
        }
        ui.label(egui::RichText::new(l!("Винятки — чати з власними налаштуваннями; їх змінюють у меню чату або в інформації про чат.", "Exceptions are chats with their own settings; change them in the chat menu or chat info.")).color(ink_dim()).size(12.5));
        if let Some((scope, muted)) = toggle {
            self.send(Cmd::SetNotifyDefault { scope, muted });
        }
        if let Some(peer) = reset {
            self.send(Cmd::ResetNotify(peer));
        }
    }
}
