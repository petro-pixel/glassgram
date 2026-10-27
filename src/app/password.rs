//! Two-step verification section of Privacy and Security, as in Telegram Desktop: the current
//! password unlocks changing it, its hint and recovery email, or turning it off; a new recovery
//! email waits for the code sent to it.

use super::*;
use crate::backend::{PasswordChange, PasswordState};

/// Which form is unfolded once the current password is known.
#[derive(Clone, Copy, PartialEq)]
enum Form {
    Password,
    Email,
    Off,
}

/// The section's state. Typed passwords live only while the settings are open.
#[derive(Default)]
pub(super) struct TwoStep {
    /// From the server (`None` — not loaded yet).
    state: Option<PasswordState>,
    /// The current password as typed, and the recovery email it unlocked.
    current: String,
    unlocked: Option<String>,
    form: Option<Form>,
    new: String,
    again: String,
    hint: String,
    email: String,
    code: String,
    /// A request is on its way; `relock` — forget the current password when it succeeds.
    busy: bool,
    relock: bool,
    error: Option<String>,
}

impl TwoStep {
    /// Forgets everything typed (leaving the settings or the page).
    pub(super) fn reset(&mut self) {
        *self = Self { state: self.state.take(), ..Self::default() };
    }

    fn clear_form(&mut self) {
        self.form = None;
        for f in [&mut self.new, &mut self.again, &mut self.hint, &mut self.email] {
            f.clear();
        }
    }
}

/// What the new password fields lack, if anything.
fn password_problem(new: &str, again: &str, hint: &str) -> Option<&'static str> {
    if new.is_empty() {
        Some(l!("Введіть новий пароль", "Enter a new password"))
    } else if new != again {
        Some(l!("Паролі не збігаються", "Passwords don't match"))
    } else if !hint.is_empty() && hint == new {
        Some(l!("Підказка не може бути паролем", "The hint can't be the password"))
    } else {
        None
    }
}

fn full_button(ui: &mut Ui, id: &str, label: &str, accent: bool, enabled: bool) -> bool {
    let r = ui.allocate_exact_size(vec2(ui.available_width(), 34.0), Sense::hover()).0;
    glass::button(ui, r, id, label, accent, enabled).clicked() && enabled
}

fn input(ui: &mut Ui, text: &mut String, hint: &str, secret: bool) -> egui::Response {
    let r = ui.allocate_exact_size(vec2(ui.available_width(), 36.0), Sense::hover()).0;
    glass::text_field(ui, r, text, hint, secret)
}

fn note(ui: &mut Ui, text: &str) {
    ui.label(egui::RichText::new(text).color(ink_dim()).size(12.5));
}

impl App {
    pub(super) fn on_password(&mut self, state: PasswordState) {
        let t = &mut self.two_step;
        t.busy = false;
        if t.relock || !state.has_password {
            t.relock = false;
            t.current.clear();
            t.unlocked = None;
            t.clear_form();
        }
        t.state = Some(state);
    }

    pub(super) fn on_password_checked(&mut self, email: String) {
        self.two_step.unlocked = Some(email);
        self.two_step.error = None;
    }

    pub(super) fn on_password_failed(&mut self, error: String) {
        self.two_step.relock = false;
        self.two_step.error = Some(error);
    }

    fn password_send(&mut self, cmd: Cmd, relock: bool) {
        self.two_step.busy = true;
        self.two_step.relock = relock;
        self.two_step.error = None;
        self.send(cmd);
    }

    pub(super) fn two_step_section(&mut self, ui: &mut Ui) {
        section_title(ui, l!("Двоетапна перевірка", "Two-Step Verification"));
        let Some(state) = self.two_step.state.clone() else {
            ui.add(egui::Spinner::new().color(ink_dim()));
            return;
        };
        let mut cmd = None;
        let t = &mut self.two_step;
        if let Some(pattern) = &state.email_pending {
            note(ui, &lf!("Введіть код, надісланий на {pattern}", "Enter the code sent to {pattern}"));
            let resp = input(ui, &mut t.code, l!("Код", "Code"), false);
            let enter = resp.lost_focus() && ui.input(|i| i.key_pressed(Key::Enter));
            ui.horizontal(|ui| {
                ui.spacing_mut().item_spacing.x = 6.0;
                let (r, _) = ui.allocate_exact_size(vec2(140.0, 30.0), Sense::hover());
                if (glass::button(ui, r, "pw-code", l!("Підтвердити", "Confirm"), true, !t.code.trim().is_empty()).clicked() || enter) && !t.code.trim().is_empty() {
                    cmd = Some((Cmd::ConfirmPasswordEmail(t.code.trim().to_owned()), false));
                    t.code.clear();
                }
                let (r, _) = ui.allocate_exact_size(vec2(140.0, 30.0), Sense::hover());
                if glass::button(ui, r, "pw-resend", l!("Надіслати ще раз", "Resend code"), false, true).clicked() {
                    cmd = Some((Cmd::ResendPasswordEmail, false));
                }
                let (r, _) = ui.allocate_exact_size(vec2(110.0, 30.0), Sense::hover());
                if glass::button(ui, r, "pw-cancel-mail", l!("Скасувати", "Cancel"), false, true).clicked() {
                    cmd = Some((Cmd::CancelPasswordEmail, false));
                }
            });
            ui.add_space(6.0);
        }

        if !state.has_password {
            note(ui, l!("Додатковий пароль, який потрібен під час входу на новому пристрої разом із кодом.", "An extra password needed, along with the code, when you sign in on a new device."));
            if t.form != Some(Form::Password) {
                if full_button(ui, "pw-on", l!("Увімкнути пароль", "Set password"), true, true) {
                    t.form = Some(Form::Password);
                }
            } else if let Some(c) = password_form(ui, t, true) {
                cmd = Some((Cmd::UpdatePassword { current: String::new(), change: c }, true));
            }
        } else if let Some(email) = t.unlocked.clone() {
            let mail = if email.is_empty() { l!("не задано", "not set").to_owned() } else { email };
            note(ui, &lf!("Пароль увімкнено. Пошта для відновлення: {mail}", "Password is on. Recovery email: {mail}"));
            ui.horizontal_wrapped(|ui| {
                ui.spacing_mut().item_spacing = vec2(6.0, 6.0);
                for (form, label) in [(Form::Password, l!("Змінити пароль", "Change password")), (Form::Email, l!("Пошта для відновлення", "Recovery email")), (Form::Off, l!("Вимкнути пароль", "Turn off password"))] {
                    let w = elided(ui, label, 14.0, ink(), 300.0).size().x + 26.0;
                    let (r, _) = ui.allocate_exact_size(vec2(w, 30.0), Sense::hover());
                    if glass::button(ui, r, ("pw-form", label), label, t.form == Some(form), true).clicked() {
                        t.form = if t.form == Some(form) { None } else { Some(form) };
                    }
                }
            });
            let current = t.current.clone();
            match t.form {
                Some(Form::Password) => {
                    if let Some(c) = password_form(ui, t, false) {
                        cmd = Some((Cmd::UpdatePassword { current, change: c }, true));
                    }
                }
                Some(Form::Email) => {
                    input(ui, &mut t.email, l!("Нова пошта для відновлення", "New recovery email"), false);
                    let ok = t.email.contains('@');
                    if full_button(ui, "pw-mail", l!("Зберегти пошту", "Save email"), true, ok && !t.busy) {
                        cmd = Some((Cmd::UpdatePassword { current, change: PasswordChange::Email(t.email.trim().to_owned()) }, true));
                    }
                }
                Some(Form::Off) => {
                    note(ui, l!("Без пароля для входу на новому пристрої вистачить коду.", "Without a password, the code alone will be enough to sign in on a new device."));
                    if full_button(ui, "pw-off", l!("Точно вимкнути", "Turn off"), false, !t.busy) {
                        cmd = Some((Cmd::UpdatePassword { current, change: PasswordChange::Off }, true));
                    }
                }
                None => {}
            }
        } else {
            let hint = if state.hint.is_empty() { String::new() } else { lf!(" Підказка: {}", " Hint: {}", state.hint) };
            note(ui, &format!("{}{hint}", l!("Пароль увімкнено. Введіть його, щоб змінити налаштування.", "Password is on. Enter it to change the settings.")));
            let resp = input(ui, &mut t.current, l!("Поточний пароль", "Current password"), true);
            let enter = resp.lost_focus() && ui.input(|i| i.key_pressed(Key::Enter));
            if (full_button(ui, "pw-check", l!("Продовжити", "Continue"), true, !t.current.is_empty() && !t.busy) || enter) && !t.current.is_empty() {
                cmd = Some((Cmd::CheckPassword(t.current.clone()), false));
            }
        }
        if t.busy {
            ui.add(egui::Spinner::new().color(ink_dim()));
        }
        if let Some(e) = &t.error {
            ui.label(egui::RichText::new(e).color(glass::danger()).size(12.5));
        }
        if let Some((cmd, relock)) = cmd {
            self.password_send(cmd, relock);
        }
    }
}

/// New password, repeat, hint and (for the first password) a recovery email; `Some` on save.
fn password_form(ui: &mut Ui, t: &mut TwoStep, first: bool) -> Option<PasswordChange> {
    input(ui, &mut t.new, l!("Новий пароль", "New password"), true);
    input(ui, &mut t.again, l!("Повторіть пароль", "Repeat password"), true);
    input(ui, &mut t.hint, l!("Підказка (необов'язково)", "Hint (optional)"), false);
    if first {
        input(ui, &mut t.email, l!("Пошта для відновлення (необов'язково)", "Recovery email (optional)"), false);
        note(ui, l!("Без пошти забутий пароль не відновити.", "Without an email a forgotten password can't be recovered."));
    }
    let problem = password_problem(&t.new, &t.again, &t.hint);
    if let Some(p) = problem.filter(|_| !t.again.is_empty()) {
        note(ui, p);
    }
    let save = full_button(ui, "pw-save", l!("Зберегти пароль", "Save password"), true, problem.is_none() && !t.busy);
    save.then(|| PasswordChange::Set { new: t.new.clone(), hint: t.hint.trim().to_owned(), email: t.email.trim().to_owned() })
}

#[cfg(test)]
mod tests {
    use super::password_problem;

    #[test]
    fn new_password_rules() {
        assert!(password_problem("", "", "").is_some());
        assert!(password_problem("abc", "abd", "").is_some());
        assert!(password_problem("abc", "abc", "abc").is_some());
        assert!(password_problem("abc", "abc", "letters").is_none());
    }
}
