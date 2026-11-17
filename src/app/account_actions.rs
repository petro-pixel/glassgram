//! "Change number" (My Account → phone number) and "Delete my account" (bottom of My Account and
//! of Privacy and Security), as boxes over the settings in Telegram Desktop's order: a warning,
//! the new number with its country, the code. Deleting the account takes two steps — what is lost
//! plus an optional reason, then typing a word (and the cloud password, if set) to unlock the red button.

use super::*;
use crate::backend::{CountryCode, country_of, phone_digits};

/// The word typed to unlock deleting the account.
fn confirm_word() -> &'static str {
    l!("ВИДАЛИТИ", "DELETE")
}

/// The open box.
#[derive(Clone, PartialEq)]
enum Stage {
    /// What changing the number does, with the "Change number" button.
    Warning,
    /// The new number and its country.
    Number,
    /// The code sent to the new number.
    Code { phone: String, hash: String, length: i32, via_app: bool },
    /// What deleting loses, and an optional reason.
    DeleteWarning,
    /// The typed word and the cloud password.
    DeleteConfirm,
}

#[derive(Default)]
pub(super) struct AccountActions {
    /// `None` — no box is open.
    stage: Option<Stage>,
    /// Calling codes from the server, for recognising the country of a typed number.
    codes: Vec<CountryCode>,
    /// One row per country for the picker, sorted by name: (name, code).
    picker: Vec<(String, String)>,
    /// The language the codes were loaded in.
    codes_lang: String,
    phone: String,
    code: String,
    reason: String,
    typed: String,
    password: String,
    /// The country list is unfolded, and its search.
    picking: bool,
    search: String,
    /// A request is on its way.
    busy: bool,
    error: Option<String>,
}

impl AccountActions {
    pub(super) fn is_open(&self) -> bool {
        self.stage.is_some()
    }

    /// Closes the box and forgets everything typed.
    fn close(&mut self) {
        *self = Self { codes: std::mem::take(&mut self.codes), picker: std::mem::take(&mut self.picker), codes_lang: std::mem::take(&mut self.codes_lang), ..Self::default() };
    }

    fn go(&mut self, stage: Stage) {
        self.stage = Some(stage);
        self.error = None;
        self.busy = false;
    }
}

fn wrapped(ui: &mut Ui, text: &str, color: Color32, size: f32) {
    ui.add(egui::Label::new(egui::RichText::new(text).color(color).size(size)).wrap());
}

fn full_field(ui: &mut Ui, text: &mut String, hint: &str, secret: bool) -> egui::Response {
    let r = ui.allocate_exact_size(vec2(ui.available_width(), 38.0), Sense::hover()).0;
    glass::text_field(ui, r, text, hint, secret)
}

impl App {
    pub(super) fn open_change_phone(&mut self) {
        self.account_actions.close();
        self.account_actions.go(Stage::Warning);
        let lang = self.settings.lang.code().to_owned();
        if self.account_actions.codes.is_empty() || self.account_actions.codes_lang != lang {
            self.account_actions.codes_lang = lang.clone();
            self.send(Cmd::LoadCountries(lang));
        }
    }

    pub(super) fn open_delete_account(&mut self) {
        self.account_actions.close();
        self.account_actions.go(Stage::DeleteWarning);
        // The confirm step asks for the cloud password only when one is set.
        self.send(Cmd::LoadPassword);
    }

    pub(super) fn account_actions_event(&mut self, ev: Ev) {
        let a = &mut self.account_actions;
        match ev {
            Ev::Countries(codes) => {
                let mut picker: Vec<(String, String)> = Vec::new();
                for c in &codes {
                    if !picker.iter().any(|(n, _)| *n == c.name) {
                        picker.push((c.name.clone(), c.code.clone()));
                    }
                }
                picker.sort_by_key(|(n, _)| n.to_lowercase());
                a.codes = codes;
                a.picker = picker;
            }
            Ev::ChangePhoneCode { phone, hash, length, via_app } => {
                if matches!(a.stage, Some(Stage::Number | Stage::Code { .. })) {
                    a.code.clear();
                    a.go(Stage::Code { phone, hash, length, via_app });
                }
            }
            Ev::PhoneChanged(phone) => {
                a.close();
                self.profile.phone = phone;
                self.toast = Some((l!("Ваш номер телефону змінено", "Your phone number has been changed").into(), 0.0, false));
                self.send(Cmd::LoadProfile);
            }
            Ev::AccountDeleted => {
                a.close();
                self.settings_open = false;
            }
            Ev::AccountActionFailed(e) => {
                a.busy = false;
                a.error = Some(e);
            }
            _ => {}
        }
    }

    pub(super) fn account_actions_view(&mut self, ui: &mut Ui) {
        let Some(stage) = self.account_actions.stage.clone() else { return };
        let screen = ui.max_rect();
        ui.painter().rect_filled(screen, 0.0, Color32::from_black_alpha(90));
        ui.interact(screen, ui.scope_id().with("account-actions-bg"), Sense::CLICK);
        let h: f32 = match &stage {
            Stage::Warning => 300.0,
            Stage::Number if self.account_actions.picking => 470.0,
            Stage::Number => 300.0,
            Stage::Code { .. } => 260.0,
            Stage::DeleteWarning => 360.0,
            Stage::DeleteConfirm => 330.0,
        };
        let rect = Rect::from_center_size(screen.center(), vec2(screen.width().min(420.0) - 20.0, h.min(screen.height() - 20.0)));
        glass::glass(ui.painter(), rect, Glass::panel(24.0).with_shadow());
        let inner = rect.shrink(18.0);
        let title = match &stage {
            Stage::Warning => l!("Змінити номер", "Change Number"),
            Stage::Number => l!("Новий номер", "Enter New Number"),
            Stage::Code { .. } => l!("Введіть код", "Enter Code"),
            Stage::DeleteWarning | Stage::DeleteConfirm => l!("Видалити акаунт", "Delete Account"),
        };
        ui.painter().text(inner.left_top(), Align2::LEFT_TOP, title, FontId::proportional(17.0), ink());
        let buttons_y = inner.bottom() - 36.0;
        let body = Rect::from_min_max(pos2(inner.left(), inner.top() + 34.0), pos2(inner.right(), buttons_y - 8.0));
        let mut child = ui.new_child(UiBuilder::new().max_rect(body));
        child.set_clip_rect(body);
        child.spacing_mut().item_spacing.y = 8.0;
        let enter = match &stage {
            Stage::Warning => self.change_phone_warning(&mut child),
            Stage::Number => self.change_phone_number(&mut child),
            Stage::Code { phone, length, via_app, .. } => self.change_phone_code(&mut child, phone, *length, *via_app),
            Stage::DeleteWarning => self.delete_account_warning(&mut child),
            Stage::DeleteConfirm => self.delete_account_confirm(&mut child),
        };
        if let Some(e) = &self.account_actions.error {
            child.add_space(2.0);
            wrapped(&mut child, e, glass::danger(), 12.5);
        }

        let a = &self.account_actions;
        let (label, ready, danger) = match &stage {
            Stage::Warning => (l!("Змінити номер", "Change Number"), true, false),
            Stage::Number => (l!("Далі", "Next"), phone_digits(&a.phone).len() >= 5, false),
            Stage::Code { .. } => (l!("Підтвердити", "Submit"), !a.code.trim().is_empty(), false),
            Stage::DeleteWarning => (l!("Продовжити", "Continue"), true, true),
            Stage::DeleteConfirm => {
                let pw_ok = self.two_step.has_password() != Some(true) || !a.password.is_empty();
                (l!("Видалити мій акаунт", "Delete My Account"), a.typed.trim() == confirm_word() && pw_ok && self.two_step.has_password().is_some(), true)
            }
        };
        let ready = ready && !a.busy;
        let half = (inner.width() - 8.0) / 2.0;
        let esc = ui.input_mut(|i| i.consume_key(egui::Modifiers::NONE, Key::Escape));
        let cancel = glass::button(ui, Rect::from_min_size(pos2(inner.left(), buttons_y), vec2(half, 36.0)), "account-action-cancel", l!("Скасувати", "Cancel"), false, true).clicked();
        let go_r = Rect::from_min_size(pos2(inner.right() - half, buttons_y), vec2(half, 36.0));
        let label = if a.busy { l!("Зачекайте…", "Please wait…") } else { label };
        let go = if danger && ready {
            glass::tone_button(ui, go_r, "account-action-go", label, glass::danger()).clicked()
        } else {
            glass::button(ui, go_r, "account-action-go", label, true, ready).clicked()
        };
        // Enter never deletes the account: only the red button does.
        let go = go || (enter && !danger);
        if cancel || esc {
            self.account_actions.close();
            return;
        }
        if !(go && ready) {
            return;
        }
        let a = &mut self.account_actions;
        match stage {
            Stage::Warning => a.go(Stage::Number),
            Stage::Number => {
                a.busy = true;
                a.error = None;
                let phone = a.phone.clone();
                self.send(Cmd::SendChangePhoneCode(phone));
            }
            Stage::Code { phone, hash, .. } => {
                a.busy = true;
                a.error = None;
                let code = a.code.trim().to_owned();
                self.send(Cmd::ChangePhone { phone, hash, code });
            }
            Stage::DeleteWarning => a.go(Stage::DeleteConfirm),
            Stage::DeleteConfirm => {
                a.busy = true;
                a.error = None;
                let cmd = Cmd::DeleteAccount { reason: a.reason.trim().to_owned(), password: std::mem::take(&mut a.password) };
                self.send(cmd);
            }
        }
    }

    fn change_phone_warning(&mut self, ui: &mut Ui) -> bool {
        if !self.profile.phone.is_empty() {
            wrapped(ui, &lf!("Поточний номер: {}", "Current number: {}", self.profile.phone), ink(), 14.5);
        }
        wrapped(ui, l!(
            "Тут можна змінити номер Telegram. Ваш акаунт і всі хмарні дані — повідомлення, медіа, контакти тощо — перейдуть на новий номер.",
            "You can change your Telegram number here. Your account and all your cloud data — messages, media, contacts, etc. will be moved to the new number."
        ), ink_dim(), 13.5);
        wrapped(ui, l!(
            "Важливо: усі ваші контакти в Telegram отримають новий номер у своїй адресній книзі, якщо в них був старий і ви їх не заблокували.",
            "Important: all your Telegram contacts will get your new number added to their address book, provided they had your old number and you haven't blocked them in Telegram."
        ), ink_dim(), 13.5);
        false
    }

    fn change_phone_number(&mut self, ui: &mut Ui) -> bool {
        let a = &mut self.account_actions;
        wrapped(ui, l!("Ми надішлемо код підтвердження на новий номер.", "We will send a confirmation code to your new number."), ink_dim(), 13.5);
        // The country row: recognised from the typed code, or picked from the list.
        let country = country_of(&a.codes, &a.phone).map(|c| c.name.clone());
        let label = match &country {
            Some(name) => name.clone(),
            None if phone_digits(&a.phone).is_empty() => l!("Оберіть країну", "Choose a country").to_owned(),
            None => l!("Невідомий код країни", "Invalid country code").to_owned(),
        };
        let r = ui.allocate_exact_size(vec2(ui.available_width(), 34.0), Sense::hover()).0;
        if glass::button(ui, r, "change-phone-country", &label, false, !a.picker.is_empty()).clicked() {
            a.picking = !a.picking;
            a.search.clear();
        }
        let mut picked = None;
        if a.picking {
            let r = ui.allocate_exact_size(vec2(ui.available_width(), 34.0), Sense::hover()).0;
            glass::search_field(ui, r, &mut a.search, l!("Пошук", "Search"));
            let q = a.search.trim().to_lowercase();
            egui::ScrollArea::vertical().id_salt("change-phone-countries").max_height(150.0).auto_shrink([false, true]).show(ui, |ui| {
                for (name, code) in a.picker.iter().filter(|(n, c)| q.is_empty() || n.to_lowercase().contains(&q) || c.starts_with(q.trim_start_matches('+'))) {
                    let (r, resp) = ui.allocate_exact_size(vec2(ui.available_width(), 30.0), Sense::click());
                    if resp.hovered() {
                        ui.painter().rect_filled(r, 10.0, glass::veil(16));
                    }
                    ui.painter().galley(pos2(r.left() + 10.0, r.center().y - 8.0), elided(ui, name, 14.0, ink(), r.width() - 80.0), ink());
                    ui.painter().text(pos2(r.right() - 10.0, r.center().y), Align2::RIGHT_CENTER, format!("+{code}"), FontId::proportional(13.5), ink_dim());
                    if resp.clicked() {
                        picked = Some(code.clone());
                    }
                }
            });
        }
        if let Some(code) = picked {
            a.phone = format!("+{code} ");
            a.picking = false;
        }
        let resp = full_field(ui, &mut a.phone, "+380 00 000 00 00", false);
        if resp.changed() {
            a.error = None;
        }
        if !a.picking && !resp.has_focus() && a.phone.is_empty() {
            resp.request_focus();
        }
        resp.lost_focus() && ui.input(|i| i.key_pressed(Key::Enter))
    }

    fn change_phone_code(&mut self, ui: &mut Ui, phone: &str, length: i32, via_app: bool) -> bool {
        let a = &mut self.account_actions;
        let text = if via_app {
            lf!("Ми надіслали код у застосунок Telegram на номері {phone}.", "We've sent the code to the Telegram app on {phone}.")
        } else {
            lf!("Ми надіслали SMS з кодом на номер {phone}.", "We've sent an SMS with an activation code to your phone {phone}.")
        };
        wrapped(ui, &text, ink_dim(), 13.5);
        let hint = if length > 0 { lp!(length, ["Код із {n} цифри", "Код із {n} цифр", "Код із {n} цифр"], ["{n}-digit code", "{n}-digit code"]) } else { l!("Код", "Code").to_owned() };
        let resp = full_field(ui, &mut a.code, &hint, false);
        if resp.changed() {
            a.error = None;
            a.code.retain(|c| c.is_ascii_digit());
        }
        if !resp.has_focus() && a.code.is_empty() {
            resp.request_focus();
        }
        let r = ui.allocate_exact_size(vec2(ui.available_width(), 30.0), Sense::hover()).0;
        if glass::button(ui, r, "change-phone-back", l!("Змінити номер", "Wrong number?"), false, !a.busy).clicked() {
            a.go(Stage::Number);
        }
        resp.lost_focus() && ui.input(|i| i.key_pressed(Key::Enter))
    }

    fn delete_account_warning(&mut self, ui: &mut Ui) -> bool {
        wrapped(ui, l!(
            "Акаунт буде видалено назавжди разом з усіма повідомленнями, медіа й контактами. Групи й канали, які ви створили, залишаться без власника. Скасувати це неможливо.",
            "Your account will be deleted permanently, along with all your messages, media and contacts. Groups and channels you created will be left without an owner. This can't be undone."
        ), ink(), 13.5);
        wrapped(ui, l!(
            "Якщо ви хочете лише змінити номер, скористайтеся «Змінити номер» у розділі «Мій акаунт».",
            "If you only want to change your number, use “Change Number” in My Account instead."
        ), ink_dim(), 13.0);
        section_title(ui, l!("Чому ви йдете? (необов'язково)", "Why are you leaving? (optional)"));
        let a = &mut self.account_actions;
        full_field(ui, &mut a.reason, l!("Причина", "Reason"), false);
        if a.reason.chars().count() > 200 {
            a.reason = a.reason.chars().take(200).collect();
        }
        false
    }

    fn delete_account_confirm(&mut self, ui: &mut Ui) -> bool {
        let word = confirm_word();
        wrapped(ui, &lf!("Щоб підтвердити, введіть {word} великими літерами.", "To confirm, type {word} in capital letters."), ink(), 13.5);
        let has_password = self.two_step.has_password();
        let a = &mut self.account_actions;
        full_field(ui, &mut a.typed, word, false);
        match has_password {
            None => {
                ui.add(egui::Spinner::new().color(ink_dim()));
            }
            Some(true) => {
                wrapped(ui, l!("Пароль двоетапної перевірки:", "Two-step verification password:"), ink_dim(), 13.0);
                if full_field(ui, &mut a.password, l!("Пароль", "Password"), true).changed() {
                    a.error = None;
                }
            }
            Some(false) => {}
        }
        wrapped(ui, l!("Після видалення Glassgram вийде з цього акаунта.", "After deletion Glassgram will log out of this account."), ink_dim(), 12.5);
        false
    }
}
