//! Sign-in screens (phone, code, password, QR) and the local passcode lock.

use super::*;

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
        let resp = glass::text_field(ui, field, &mut self.lock_input, l!("Локальний пароль", "Local passcode"), true);
        if !resp.has_focus() {
            resp.request_focus();
        }
        if resp.changed() {
            self.lock_error = false;
        }
        let enter = resp.lost_focus() && ui.input(|i| i.key_pressed(Key::Enter));
        let b = Rect::from_min_size(pos2(rect.left() + 24.0, rect.bottom() - 58.0), vec2(rect.width() - 48.0, 36.0));
        if glass::button(ui, b, "unlock", l!("Розблокувати", "Unlock"), true, !self.lock_input.is_empty()).clicked() || enter {
            if crate::lock::check(&self.settings.passcode, &self.lock_input) {
                self.locked = false;
                self.lock_error = false;
            } else {
                self.lock_error = true;
            }
            self.lock_input.clear();
        }
        if self.lock_error {
            ui.painter().text(pos2(rect.center().x, field.bottom() + 9.0), Align2::CENTER_CENTER, l!("Невірний пароль", "Wrong passcode"), FontId::proportional(12.5), Color32::from_rgb(255, 110, 120));
        }
    }

    pub(super) fn login_view(&mut self, ui: &mut Ui, step: Step) {
        if let Step::Qr(url) = &step {
            return self.qr_view(ui, url.as_deref());
        }
        let screen = ui.max_rect();
        let card_h = match step {
            Step::Creds => 400.0,
            Step::Connecting => 300.0,
            Step::Phone => 420.0,
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
            Step::Creds => l!("Введіть API ID та API hash з my.telegram.org", "Enter API ID and API hash from my.telegram.org").to_owned(),
            Step::Connecting => l!("Підключення до Telegram…", "Connecting to Telegram…").to_owned(),
            Step::Phone => l!("Ваш номер телефону у міжнародному форматі", "Your phone number in international format").to_owned(),
            Step::Code => l!("Код надіслано в Telegram або SMS", "The code was sent via Telegram or SMS").to_owned(),
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
            Step::Creds => vec![(&mut self.api_id, "API ID", false), (&mut self.api_hash, "API hash", false)],
            Step::Phone => vec![(&mut self.phone, "+380 00 000 00 00", false)],
            Step::Code => vec![(&mut self.code, l!("Код", "Code"), false)],
            Step::Password(_) => vec![(&mut self.password, l!("Пароль", "Password"), true)],
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
            (Step::Creds, _) => l!("Підключитися", "Connect"),
            (Step::Phone, _) => l!("Отримати код", "Get code"),
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
            Step::Creds => match self.api_id.trim().parse() {
                Ok(api_id) if !self.api_hash.trim().is_empty() => {
                    self.screen = Screen::Login(Step::Connecting);
                    Cmd::Connect { api_id, api_hash: self.api_hash.trim().to_owned() }
                }
                _ => {
                    self.toast = Some((l!("API ID має бути числом, API hash — непорожнім", "API ID must be a number and API hash must not be empty").into(), 0.0, true));
                    return;
                }
            },
            Step::Phone => Cmd::RequestCode(self.phone.clone()),
            Step::Code => Cmd::SubmitCode(self.code.clone()),
            Step::Password(_) => Cmd::SubmitPassword(self.password.clone()),
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
            Ev::NeedCode => self.screen = Screen::Login(Step::Code),
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
                self.code.clear();
                self.password.clear();
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
                self.selected = None;
                self.screen = Screen::Login(Step::Creds);
            }
            _ => {}
        }
    }
}
