//! Sign-in screens (phone, code, password, QR) and the local passcode lock.

use super::*;
use crate::backend::CodeVia;

/// Local passcode lock: whether locked, the typed passcode, the last error, the last input.
pub(super) struct Lock {
    /// The interface is locked by a local passcode.
    pub(super) on: bool,
    /// The passcode being typed.
    pub(super) input: String,
    /// The last attempt was wrong.
    pub(super) error: bool,
    /// Time of the last input, for locking after inactivity.
    pub(super) last_activity: f64,
}

/// A build with keys inside (`--features no-key-screen`): the key step is only a "Retry" after a failed connection.
const NO_KEY_SCREEN: bool = cfg!(feature = "no-key-screen");

/// Sign-in form fields.
pub(super) struct LoginForm {
    pub(super) api_id: String,
    pub(super) api_hash: String,
    pub(super) phone: String,
    /// The login email an account without one is asked to bind.
    pub(super) email: String,
    /// Unix time the code screen came up: an email reset waits from here.
    pub(super) code_at: i64,
    pub(super) code: String,
    pub(super) password: String,
}

impl App {
    /// Lock screen: just a passcode field over the background.
    pub(super) fn lock_view(&mut self, ui: &mut Ui) {
        let screen = ui.max_rect();
        let rect = Rect::from_center_size(screen.center(), vec2(320.0, 230.0));
        glass::glass(ui.painter(), rect, Glass::panel(28.0).with_shadow());
        let logo = pos2(rect.center().x, rect.top() + 46.0);
        glass::avatar(ui.painter(), logo, 26.0, "", 4, Some(Icon::Lock), None);
        ui.painter().text(pos2(rect.center().x, rect.top() + 88.0), Align2::CENTER_CENTER, l!("Glassgram заблоковано", "Glassgram is locked"), FontId::proportional(17.0), ink());
        let field = Rect::from_min_size(pos2(rect.left() + 24.0, rect.top() + 112.0), vec2(rect.width() - 48.0, 40.0));
        let resp = glass::text_field(ui, field, &mut self.lock.input, l!("Локальний пароль", "Local passcode"), true);
        if !resp.has_focus() {
            resp.request_focus();
        }
        if resp.changed() {
            self.lock.error = false;
        }
        let enter = resp.lost_focus() && ui.input(|i| i.key_pressed(Key::Enter));
        let b = Rect::from_min_size(pos2(rect.left() + 24.0, rect.bottom() - 58.0), vec2(rect.width() - 48.0, 36.0));
        if glass::button(ui, b, "unlock", l!("Розблокувати", "Unlock"), true, !self.lock.input.is_empty()).clicked() || enter {
            if crate::lock::check(&self.settings.passcode, &self.lock.input) {
                self.lock.on = false;
                self.lock.error = false;
            } else {
                self.lock.error = true;
            }
            self.lock.input.clear();
        }
        if self.lock.error {
            ui.painter().text(pos2(rect.center().x, field.bottom() + 9.0), Align2::CENTER_CENTER, l!("Невірний пароль", "Wrong passcode"), FontId::proportional(12.5), Color32::from_rgb(255, 110, 120));
        }
    }

    pub(super) fn login_view(&mut self, ui: &mut Ui, step: Step) {
        if let Step::Qr(url) = &step {
            return self.qr_view(ui, url.as_deref());
        }
        let screen = ui.max_rect();
        let card_h = match step {
            Step::Creds if NO_KEY_SCREEN => 300.0,
            Step::Creds => 400.0,
            Step::Connecting => 300.0,
            Step::Phone => 420.0,
            Step::Code(CodeVia::Email { .. }) => 500.0,
            Step::Code(CodeVia::SetUpEmail { .. }) => 420.0,
            _ => 360.0,
        };
        let card = Rect::from_center_size(screen.center(), vec2(380.0_f32.min(screen.width() - 32.0), card_h));
        glass::glass(ui.painter(), card, Glass::panel(34.0));
        // Signing in to an added account can be abandoned to return to the already open one.
        let active = self.accounts.active;
        if let Some(back) = self.accounts.list.iter().find(|a| a.id != active && !a.name.is_empty()).cloned() {
            let b = Rect::from_center_size(pos2(card.center().x, card.bottom() + 30.0), vec2(card.width(), 38.0));
            if glass::button(ui, b, "account-back", &lf!("← Повернутися до «{}»", "← Back to “{}”", back.name), false, true).clicked() {
                if self.accounts.list.iter().any(|a| a.id == active && a.name.is_empty()) {
                    self.accounts.remove(active);
                    if active != 0 {
                        let _ = std::fs::remove_dir_all(accounts::session_path(active).parent().unwrap_or(std::path::Path::new("")));
                    }
                }
                self.switch_account(back.id);
                return;
            }
        }

        let p = ui.painter().clone();
        let mut y = card.top() + 34.0;
        let logo = pos2(card.center().x, y + 34.0);
        glass::avatar(&p, logo, 34.0, "", 4, Some(Icon::Send), None);
        y += 84.0;
        p.text(pos2(card.center().x, y), Align2::CENTER_TOP, "Glassgram", FontId::proportional(26.0), ink());
        y += 36.0;
        let subtitle = match &step {
            Step::Creds if NO_KEY_SCREEN => l!("Немає з'єднання з Telegram", "No connection to Telegram").to_owned(),
            Step::Creds => l!("Введіть API ID та API hash з my.telegram.org", "Enter API ID and API hash from my.telegram.org").to_owned(),
            Step::Connecting => l!("Підключення до Telegram…", "Connecting to Telegram…").to_owned(),
            Step::Phone => l!("Ваш номер телефону у міжнародному форматі", "Your phone number in international format").to_owned(),
            Step::Code(via) => code_subtitle(via),
            Step::Password(h) => match h {
                Some(h) => lf!("Пароль двоетапної перевірки (підказка: {h})", "Two-step verification password (hint: {h})"),
                None => l!("Пароль двоетапної перевірки", "Two-step verification password").to_owned(),
            },
            Step::Qr(_) => unreachable!(),
        };
        let galley = p.layout(subtitle, FontId::proportional(14.0), ink_dim(), card.width() - 64.0);
        p.galley(pos2(card.center().x - galley.size().x / 2.0, y), galley.clone(), ink_dim());
        y += galley.size().y + 22.0;

        if step == Step::Connecting {
            let spin = Rect::from_center_size(pos2(card.center().x, y + 20.0), vec2(28.0, 28.0));
            ui.put(spin, egui::Spinner::new().size(28.0).color(ink()));
            return;
        }

        let field = |y: f32| Rect::from_min_size(pos2(card.left() + 32.0, y), vec2(card.width() - 64.0, 46.0));
        let mut submit = false;
        let mut fields: Vec<(&mut String, &str, bool)> = match &step {
            Step::Creds if NO_KEY_SCREEN => Vec::new(),
            Step::Creds => vec![(&mut self.login.api_id, "API ID", false), (&mut self.login.api_hash, "API hash", false)],
            Step::Phone => vec![(&mut self.login.phone, "+380 00 000 00 00", false)],
            Step::Code(CodeVia::SetUpEmail { .. }) => vec![(&mut self.login.email, l!("Електронна пошта", "Email"), false)],
            Step::Code(_) => vec![(&mut self.login.code, l!("Код", "Code"), false)],
            Step::Password(_) => vec![(&mut self.login.password, l!("Пароль", "Password"), true)],
            Step::Connecting | Step::Qr(_) => unreachable!(),
        };
        for (text, hint, pw) in fields.iter_mut() {
            let r = glass::text_field(ui, field(y), text, hint, *pw);
            submit |= r.lost_focus() && ui.input(|i| i.key_pressed(Key::Enter));
            y += 58.0;
        }
        drop(fields);

        let label = match (&step, self.busy) {
            (_, true) => l!("Зачекайте…", "Please wait…"),
            (Step::Creds, _) if NO_KEY_SCREEN => l!("Повторити", "Retry"),
            (Step::Creds, _) => l!("Підключитися", "Connect"),
            (Step::Phone, _) => l!("Отримати код", "Get code"),
            (Step::Code(CodeVia::SetUpEmail { .. } | CodeVia::SetUpEmailCode { .. }), _) => l!("Далі", "Next"),
            _ => l!("Увійти", "Log in"),
        };
        y += 6.0;
        submit |= glass::button(ui, field(y), "login-btn", label, true, !self.busy).clicked();
        if submit && !self.busy {
            self.submit_login(&step);
        }
        if step == Step::Phone && glass::button(ui, field(y + 58.0), "qr-btn", l!("Увійти через QR-код", "Log in with QR code"), false, !self.busy).clicked() {
            self.screen = Screen::Login(Step::Qr(None));
            self.send(Cmd::QrLogin);
        }
        if let Step::Code(via) = &step {
            self.email_code_extras(ui, card, y + 58.0, via);
        }
    }

    /// Under an email code: resetting the email, and the Apple / Google sign-in the official apps offer.
    fn email_code_extras(&mut self, ui: &mut Ui, card: Rect, mut y: f32, via: &CodeVia) {
        let (apple, google) = match via {
            CodeVia::Email { apple, google, .. } | CodeVia::SetUpEmail { apple, google } => (*apple, *google),
            _ => return,
        };
        let p = ui.painter().clone();
        let note = |text: String, y: &mut f32| {
            let g = p.layout(text, FontId::proportional(12.5), ink_dim(), card.width() - 64.0);
            p.galley(pos2(card.center().x - g.size().x / 2.0, *y), g.clone(), ink_dim());
            *y += g.size().y + 8.0;
        };
        if let CodeVia::Email { reset_after, reset_at, .. } = via {
            let now = jiff::Timestamp::now().as_second();
            if let Some(at) = reset_at {
                note(lf!("Пошту буде скинуто {}", "The email will be reset on {}", when(*at as i64)), &mut y);
            } else if let Some(wait) = reset_after {
                let left = self.login.code_at + *wait as i64 - now;
                let r = Rect::from_min_size(pos2(card.left() + 32.0, y), vec2(card.width() - 64.0, 38.0));
                if glass::button(ui, r, "email-reset", l!("Скинути пошту", "Reset email"), false, left <= 0 && !self.busy).clicked() && left <= 0 && !self.busy {
                    self.busy = true;
                    self.send(Cmd::ResetLoginEmail);
                }
                y += 46.0;
                if left > 0 {
                    note(lf!("Скинути можна буде {}", "You can reset it on {}", when(now + left)), &mut y);
                }
            }
        }
        let other = match (apple, google) {
            (true, true) => l!("Також можна увійти через Apple або Google в офіційному застосунку.", "You can also sign in with Apple or Google in the official app."),
            (true, false) => l!("Також можна увійти через Apple в офіційному застосунку.", "You can also sign in with Apple in the official app."),
            (false, true) => l!("Також можна увійти через Google в офіційному застосунку.", "You can also sign in with Google in the official app."),
            (false, false) => return,
        };
        note(other.to_owned(), &mut y);
    }

    pub(super) fn qr_view(&mut self, ui: &mut Ui, url: Option<&str>) {
        let screen = ui.max_rect();
        let card = Rect::from_center_size(screen.center(), vec2(400.0_f32.min(screen.width() - 32.0), 510.0));
        glass::glass(ui.painter(), card, Glass::panel(34.0));
        let p = ui.painter().clone();
        let mut y = card.top() + 30.0;
        p.text(pos2(card.center().x, y), Align2::CENTER_TOP, l!("Вхід за QR-кодом", "QR code login"), FontId::proportional(24.0), ink());
        y += 46.0;

        let qr_rect = Rect::from_min_size(pos2(card.center().x - 120.0, y), vec2(240.0, 240.0));
        match url {
            Some(url) => glass::qr(&p, qr_rect, url),
            None => {
                glass::glass(&p, qr_rect, Glass::tinted(24.0, Color32::WHITE, 40));
                ui.put(Rect::from_center_size(qr_rect.center(), vec2(32.0, 32.0)), egui::Spinner::new().size(32.0).color(ink()));
            }
        }
        y = qr_rect.bottom() + 22.0;

        let steps = l!("1. Відкрийте Telegram на телефоні\n2. Налаштування → Пристрої\n3. «Підключити пристрій» і наведіть камеру на код", "1. Open Telegram on your phone\n2. Settings → Devices\n3. “Link Desktop Device” and point the camera at the code");
        let g = p.layout(steps.to_owned(), FontId::proportional(14.5), ink_dim(), card.width() - 72.0);
        p.galley(pos2(card.left() + 36.0, y), g.clone(), ink_dim());
        y += g.size().y + 24.0;

        let btn = Rect::from_min_size(pos2(card.left() + 32.0, y), vec2(card.width() - 64.0, 46.0));
        if glass::button(ui, btn, "qr-back", l!("Увійти за номером", "Log in with phone number"), false, true).clicked() {
            self.send(Cmd::CancelQr);
            self.screen = Screen::Login(Step::Phone);
        }
    }

    pub(super) fn submit_login(&mut self, step: &Step) {
        let cmd = match step {
            Step::Creds if NO_KEY_SCREEN => match backend::saved_credentials() {
                Some((api_id, api_hash)) => {
                    self.screen = Screen::Login(Step::Connecting);
                    Cmd::Connect { api_id, api_hash }
                }
                None => return,
            },
            Step::Creds => match self.login.api_id.trim().parse() {
                Ok(api_id) if !self.login.api_hash.trim().is_empty() => {
                    self.screen = Screen::Login(Step::Connecting);
                    Cmd::Connect { api_id, api_hash: self.login.api_hash.trim().to_owned() }
                }
                _ => {
                    self.toast = Some((l!("API ID має бути числом, API hash — непорожнім", "API ID must be a number and API hash must not be empty").into(), 0.0, true));
                    return;
                }
            },
            Step::Phone => Cmd::RequestCode(self.login.phone.clone()),
            Step::Code(CodeVia::SetUpEmail { .. }) if !self.login.email.contains('@') => {
                self.toast = Some((l!("Невірна адреса пошти", "Invalid email address").into(), 0.0, true));
                return;
            }
            Step::Code(CodeVia::SetUpEmail { .. }) => Cmd::SetUpLoginEmail(self.login.email.trim().to_owned()),
            Step::Code(_) => Cmd::SubmitCode(self.login.code.clone()),
            Step::Password(_) => Cmd::SubmitPassword(self.login.password.clone()),
            Step::Connecting | Step::Qr(_) => return,
        };
        self.busy = true;
        self.send(cmd);
    }
}

impl App {
    /// Sign-in steps and the account coming or going.
    pub(super) fn login_event(&mut self, ev: Ev) {
        match ev {
            Ev::NeedPhone => self.screen = Screen::Login(Step::Phone),
            Ev::NeedCode(via) => {
                self.login.code.clear();
                self.login.code_at = jiff::Timestamp::now().as_second();
                self.screen = Screen::Login(Step::Code(via));
            }
            Ev::NeedPassword(hint) => self.screen = Screen::Login(Step::Password(hint)),
            Ev::Qr(url) => {
                if matches!(self.screen, Screen::Login(Step::Qr(_))) {
                    self.screen = Screen::Login(Step::Qr(Some(url)));
                }
            }
            Ev::LoggedIn(name) => {
                self.accounts.set_name(self.accounts.active, &name);
                self.accounts.save();
                self.me = name;
                self.screen = Screen::Main;
                self.login.code.clear();
                self.login.password.clear();
                self.refresh_lang_pack();
                self.send(Cmd::LoadPeerColors);
            }
            Ev::LoggedOut => {
                // Other accounts remain: remove this one and move to the next.
                if self.accounts.list.len() > 1 {
                    let gone = self.accounts.active;
                    self.accounts.remove(gone);
                    let next = self.accounts.active;
                    self.switch_account(next);
                    return;
                }
                self.accounts.set_name(self.accounts.active, "");
                self.accounts.save();
                self.chats.clear();
                self.history.clear();
                self.textures.clear();
                self.custom_emoji.clear();
                self.selected = None;
                self.screen = Screen::Login(Step::Creds);
            }
            _ => {}
        }
    }
}

/// What the code screen says about where the code went, as Telegram Desktop words it.
fn code_subtitle(via: &CodeVia) -> String {
    match via {
        CodeVia::App => l!("Код надіслано в застосунок Telegram на іншому пристрої", "We've sent the code to the Telegram app on your other device").to_owned(),
        CodeVia::Sms => l!("Ми надіслали SMS із кодом", "We've sent you an SMS with the code").to_owned(),
        CodeVia::Call => l!("Ми телефонуємо, щоб продиктувати код", "We're calling your phone to tell you the code").to_owned(),
        CodeVia::Other => l!("Код надіслано в Telegram або SMS", "The code was sent via Telegram or SMS").to_owned(),
        CodeVia::Email { pattern, .. } => lf!(
            "Перевірте пошту {pattern} (і теку «Спам») та введіть код, який ми надіслали",
            "Check your email {pattern} (don't forget the spam folder) and enter the code we sent"
        ),
        CodeVia::SetUpEmail { .. } => l!("Додайте пошту для входу: на неї приходитимуть коди", "Add a login email: sign-in codes will be sent to it").to_owned(),
        CodeVia::SetUpEmailCode { pattern, .. } => lf!("Введіть код, надісланий на {pattern}", "Enter the code sent to {pattern}"),
    }
}

fn when(t: i64) -> String {
    jiff::Timestamp::from_second(t).map(|t| t.to_zoned(jiff::tz::TimeZone::system()).strftime("%d.%m.%Y %H:%M").to_string()).unwrap_or_default()
}
