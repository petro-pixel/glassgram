//! Privacy and Security → Login Email and Passkeys, as in the official apps: the masked email that
//! sign-in codes go to, changed by a code sent to the new address; the passkeys that sign in
//! without a code, each with its dates and "Delete".

use super::*;
use crate::backend::Passkey;

#[derive(Default)]
pub(super) struct LoginSecurity {
    /// The masked login email; the outer `None` — not loaded yet.
    email: Option<Option<String>>,
    /// `None` — not loaded yet.
    passkeys: Option<Vec<Passkey>>,
    /// "Change email" is unfolded: the new address, and once its code went out, the code.
    changing: bool,
    new_email: String,
    code_sent: Option<String>,
    code: String,
    /// The passkey whose "Delete" was pressed once and asks again.
    confirm_delete: Option<String>,
    busy: bool,
    error: Option<String>,
}

impl LoginSecurity {
    /// Forgets the open form (leaving the settings or the page).
    pub(super) fn reset(&mut self) {
        *self = Self { email: self.email.take(), passkeys: self.passkeys.take(), ..Self::default() };
    }
}

fn when(t: i64) -> String {
    jiff::Timestamp::from_second(t).map(|t| t.to_zoned(jiff::tz::TimeZone::system()).strftime("%d.%m.%Y").to_string()).unwrap_or_default()
}

fn note(ui: &mut Ui, text: &str) {
    ui.label(egui::RichText::new(text).color(ink_dim()).size(12.5));
}

fn input(ui: &mut Ui, text: &mut String, hint: &str) -> bool {
    ui.add_space(4.0);
    let r = ui.allocate_exact_size(vec2(ui.available_width(), 36.0), Sense::hover()).0;
    let resp = glass::text_field(ui, r, text, hint, false);
    resp.lost_focus() && ui.input(|i| i.key_pressed(Key::Enter))
}

/// "Created 01.02.2026 · last used 03.04.2026".
fn passkey_dates(p: &Passkey) -> String {
    let made = lf!("Створено {}", "Created {}", when(p.date));
    match p.last_used {
        Some(t) => lf!("{made} · востаннє використано {}", "{made} · last used {}", when(t)),
        None => made,
    }
}

impl App {
    pub(super) fn login_security_event(&mut self, ev: Ev) {
        let s = &mut self.login_security;
        s.busy = false;
        match ev {
            Ev::LoginEmail(pattern) => {
                s.email = Some(pattern);
                s.changing = false;
                s.code_sent = None;
                s.new_email.clear();
                s.code.clear();
                s.error = None;
            }
            Ev::LoginEmailCode { pattern } => {
                s.code_sent = Some(pattern);
                s.error = None;
            }
            Ev::Passkeys(list) => {
                s.passkeys = Some(list);
                s.confirm_delete = None;
            }
            Ev::LoginSecurityFailed(e) => s.error = Some(e),
            _ => {}
        }
    }

    fn login_security_send(&mut self, cmd: Cmd) {
        self.login_security.busy = true;
        self.login_security.error = None;
        self.send(cmd);
    }

    /// The email sign-in codes go to; hidden until loaded.
    pub(super) fn login_email_section(&mut self, ui: &mut Ui) {
        let Some(email) = self.login_security.email.clone() else { return };
        section_title(ui, l!("Пошта для входу", "Login Email"));
        let mut cmd = None;
        let s = &mut self.login_security;
        let (row, _) = ui.allocate_exact_size(vec2(ui.available_width(), 52.0), Sense::hover());
        let p = ui.painter();
        p.rect_filled(row, 14.0, glass::veil(10));
        let c = pos2(row.left() + 26.0, row.center().y);
        p.circle_filled(c, 16.0, glass::rgba(ACCENT, 40));
        icons::draw(p, Rect::from_center_size(c, vec2(16.0, 16.0)), Icon::Mail, ACCENT);
        let title = email.clone().unwrap_or_else(|| l!("Не задано", "Not set").to_owned());
        p.galley(pos2(row.left() + 52.0, row.top() + 8.0), elided(ui, &title, 14.5, ink(), row.width() - 180.0), ink());
        let sub = l!("Сюди надходять коди для входу", "Sign-in codes are sent here");
        p.galley(pos2(row.left() + 52.0, row.top() + 28.0), elided(ui, sub, 12.5, ink_dim(), row.width() - 180.0), ink_dim());
        let b = Rect::from_min_size(pos2(row.right() - 124.0, row.top() + 10.0), vec2(116.0, 32.0));
        let label = if email.is_some() { l!("Змінити", "Change") } else { l!("Додати", "Add") };
        if glass::button(ui, b, "login-email-change", label, s.changing, true).clicked() {
            s.changing = !s.changing;
            s.code_sent = None;
            s.error = None;
        }
        if s.changing {
            match s.code_sent.clone() {
                None => {
                    let enter = input(ui, &mut s.new_email, l!("Нова пошта", "New email"));
                    let ok = s.new_email.contains('@') && !s.busy;
                    ui.add_space(4.0);
                    let r = ui.allocate_exact_size(vec2(ui.available_width(), 34.0), Sense::hover()).0;
                    if (glass::button(ui, r, "login-email-send", l!("Надіслати код", "Send code"), true, ok).clicked() || enter) && ok {
                        cmd = Some(Cmd::ChangeLoginEmail(s.new_email.trim().to_owned()));
                    }
                }
                Some(pattern) => {
                    note(ui, &lf!("Введіть код, надісланий на {pattern}", "Enter the code sent to {pattern}"));
                    let enter = input(ui, &mut s.code, l!("Код", "Code"));
                    let ok = !s.code.trim().is_empty() && !s.busy;
                    ui.add_space(4.0);
                    let r = ui.allocate_exact_size(vec2(ui.available_width(), 34.0), Sense::hover()).0;
                    if (glass::button(ui, r, "login-email-verify", l!("Підтвердити", "Confirm"), true, ok).clicked() || enter) && ok {
                        cmd = Some(Cmd::VerifyLoginEmail(s.code.trim().to_owned()));
                    }
                }
            }
        }
        if s.busy {
            ui.add(egui::Spinner::new().color(ink_dim()));
        }
        if let Some(e) = &s.error {
            ui.label(egui::RichText::new(e).color(glass::danger()).size(12.5));
        }
        if let Some(c) = cmd {
            self.login_security_send(c);
        }
    }

    pub(super) fn passkeys_section(&mut self, ui: &mut Ui) {
        let Some(list) = self.login_security.passkeys.clone() else { return };
        section_title(ui, l!("Ключі доступу", "Passkeys"));
        let mut cmd = None;
        for k in &list {
            let (row, _) = ui.allocate_exact_size(vec2(ui.available_width(), 52.0), Sense::hover());
            let p = ui.painter();
            p.rect_filled(row, 14.0, glass::veil(10));
            let c = pos2(row.left() + 26.0, row.center().y);
            p.circle_filled(c, 16.0, glass::veil(24));
            icons::draw(p, Rect::from_center_size(c, vec2(16.0, 16.0)), Icon::KeyRound, ink());
            let w = row.width() - 180.0;
            let name = if k.name.is_empty() { l!("Ключ доступу", "Passkey") } else { k.name.as_str() };
            p.galley(pos2(row.left() + 52.0, row.top() + 8.0), elided(ui, name, 14.5, ink(), w), ink());
            p.galley(pos2(row.left() + 52.0, row.top() + 28.0), elided(ui, &passkey_dates(k), 12.5, ink_dim(), w), ink_dim());
            let confirm = self.login_security.confirm_delete.as_deref() == Some(k.id.as_str());
            let b = Rect::from_min_size(pos2(row.right() - 124.0, row.top() + 10.0), vec2(116.0, 32.0));
            let label = if confirm { l!("Точно?", "Sure?") } else { l!("Видалити", "Delete") };
            if glass::button(ui, b, ("passkey-del", &k.id), label, confirm, !self.login_security.busy).clicked() {
                if confirm {
                    cmd = Some(Cmd::DeletePasskey(k.id.clone()));
                } else {
                    self.login_security.confirm_delete = Some(k.id.clone());
                }
            }
            ui.add_space(4.0);
        }
        note(ui, if list.is_empty() {
            l!("Ключів доступу немає. Створити ключ можна в офіційному застосунку на телефоні.", "No passkeys. You can create one in the official app on your phone.")
        } else {
            l!("Ключ доступу входить в акаунт без коду — відбитком пальця, обличчям або PIN-кодом пристрою.", "A passkey signs in without a code — with your fingerprint, face or device PIN.")
        });
        if let Some(c) = cmd {
            self.login_security_send(c);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn passkey_dates_line() {
        let k = Passkey { id: "a".into(), name: String::new(), date: 1_700_000_000, last_used: None };
        assert!(!passkey_dates(&k).contains('·'));
        let used = Passkey { last_used: Some(1_710_000_000), ..k };
        assert!(passkey_dates(&used).contains('·'));
    }
}
