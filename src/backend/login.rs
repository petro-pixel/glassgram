//! Signing in: phone or email code, binding a login email, 2FA password, QR login and logging out.
//!
//! The code is requested with our own `auth.sendCode` rather than grammers' `request_login_code`:
//! that one drops the delivery type, and an email code (`auth.sentCodeTypeEmailCode`) has to go
//! back as `email_verification`, not as `phone_code`.

use super::*;
use grammers_client::InvocationError;

/// How the sign-in code was delivered (`auth.SentCodeType`), for the code screen.
#[derive(Debug, Clone, PartialEq)]
pub enum CodeVia {
    /// A message from Telegram in the app on another device.
    App,
    Sms,
    Call,
    /// Flash and missed calls, Fragment, Firebase: told apart only by the server.
    Other,
    /// To the login email (`auth.sentCodeTypeEmailCode`).
    Email {
        pattern: String,
        length: i32,
        /// The official apps can sign in with Apple or Google instead.
        apple: bool,
        google: bool,
        /// Seconds to wait before the email can be reset (`auth.resetLoginEmail`).
        reset_after: Option<i32>,
        /// A reset was already asked for and happens at this unix time.
        reset_at: Option<i32>,
    },
    /// The account has to bind a login email before any code (`auth.sentCodeTypeSetUpEmailRequired`).
    SetUpEmail { apple: bool, google: bool },
    /// The code sent to the email being bound (`account.sendVerifyEmailCode`, purpose LoginSetup).
    SetUpEmailCode { pattern: String, length: i32 },
}

impl CodeVia {
    /// The typed code goes as `email_verification` rather than `phone_code`.
    pub fn is_email(&self) -> bool {
        matches!(self, Self::Email { .. } | Self::SetUpEmailCode { .. })
    }
}

/// What the server told about the code: the phone and its hash, needed with every later step.
pub(super) struct CodeToken {
    phone: String,
    hash: String,
    via: CodeVia,
}

pub(super) fn code_via(t: &tl::enums::auth::SentCodeType) -> CodeVia {
    use tl::enums::auth::SentCodeType as T;
    match t {
        T::App(_) => CodeVia::App,
        T::Sms(_) | T::SmsWord(_) | T::SmsPhrase(_) | T::FirebaseSms(_) => CodeVia::Sms,
        T::Call(_) => CodeVia::Call,
        T::FlashCall(_) | T::MissedCall(_) | T::FragmentSms(_) => CodeVia::Other,
        T::EmailCode(e) => CodeVia::Email {
            pattern: e.email_pattern.clone(),
            length: e.length,
            apple: e.apple_signin_allowed,
            google: e.google_signin_allowed,
            reset_after: e.reset_available_period,
            reset_at: e.reset_pending_date,
        },
        T::SetUpEmailRequired(s) => CodeVia::SetUpEmail { apple: s.apple_signin_allowed, google: s.google_signin_allowed },
    }
}

fn wrong_code(e: &InvocationError) -> bool {
    e.is("PHONE_CODE_*") || e.is("EMAIL_CODE_INVALID") || e.is("CODE_INVALID")
}

impl Worker {
    /// `auth.sendCode`, moving to the account's DC when the server asks (as grammers does).
    async fn send_code(&self, phone: &str) -> Result<tl::enums::auth::SentCode, String> {
        let client = self.client()?;
        let req = tl::functions::auth::SendCode {
            phone_number: phone.to_owned(),
            api_id: self.api_id,
            api_hash: self.api_hash.clone(),
            settings: tl::types::CodeSettings {
                allow_flashcall: false,
                current_number: false,
                allow_app_hash: false,
                allow_missed_call: false,
                allow_firebase: false,
                logout_tokens: None,
                token: None,
                app_sandbox: None,
                unknown_number: false,
            }
            .into(),
        };
        let failed = |e: InvocationError| lf!("Не вдалося надіслати код: {e}", "Could not send the code: {e}");
        match client.invoke(&req).await {
            Err(InvocationError::Rpc(e)) if e.code == 303 => {
                let dc = e.value.ok_or(l!("Сервер не назвав DC", "The server named no DC"))? as i32;
                let session = self.session.clone().ok_or(l!("Немає сесії", "No session"))?;
                session.set_home_dc_id(dc).await.map_err(|e| lf!("Міграція DC: {e}", "DC migration: {e}"))?;
                client.invoke(&req).await.map_err(failed)
            }
            r => r.map_err(failed),
        }
    }

    /// The answer to any step that sends a code: show its screen, or finish when already signed in.
    async fn on_sent_code(&mut self, phone: String, sent: tl::enums::auth::SentCode) -> Result<(), String> {
        use tl::enums::auth::SentCode as SC;
        match sent {
            SC::Code(c) => {
                let via = code_via(&c.r#type);
                self.login_token = Some(CodeToken { phone, hash: c.phone_code_hash, via: via.clone() });
                self.out.send(Ev::NeedCode(via));
                Ok(())
            }
            SC::Success(s) => self.on_authorization(s.authorization).await,
            SC::PaymentRequired(_) => Err(l!(
                "Telegram просить оплату за надсилання коду — увійдіть в офіційному застосунку",
                "Telegram asks for a payment to send the code — sign in with the official app"
            )
            .into()),
        }
    }

    async fn on_authorization(&mut self, auth: tl::enums::auth::Authorization) -> Result<(), String> {
        match auth {
            tl::enums::auth::Authorization::Authorization(a) => {
                self.login_token = None;
                self.complete_login(a).await?;
                self.after_login().await
            }
            tl::enums::auth::Authorization::SignUpRequired(_) => {
                Err(l!("Акаунт не знайдено — зареєструйтесь в офіційному застосунку", "Account not found — sign up in the official app").into())
            }
        }
    }

    /// `auth.signIn` with a phone or email code; a 2FA account answers with the password step.
    async fn sign_in(&mut self, code: String) -> Result<(), String> {
        let client = self.client()?;
        let token = self.login_token.as_ref().ok_or(l!("Спершу запросіть код", "Request a code first"))?;
        let email = token.via.is_email();
        let req = tl::functions::auth::SignIn {
            phone_number: token.phone.clone(),
            phone_code_hash: token.hash.clone(),
            phone_code: (!email).then(|| code.clone()),
            email_verification: email.then(|| tl::types::EmailVerificationCode { code }.into()),
        };
        match client.invoke(&req).await {
            Ok(auth) => self.on_authorization(auth).await,
            Err(e) if e.is("SESSION_PASSWORD_NEEDED") => {
                let tl::enums::account::Password::Password(pw) =
                    client.invoke(&tl::functions::account::GetPassword {}).await.map_err(|e| format!("2FA: {e}"))?;
                let pt = PasswordToken::new(pw);
                self.out.send(Ev::NeedPassword(pt.hint().map(str::to_owned)));
                self.password_token = Some(pt);
                Ok(())
            }
            Err(e) if wrong_code(&e) => Err(l!("Невірний код", "Wrong code").into()),
            Err(e) => Err(e.to_string()),
        }
    }

    pub(super) async fn login_cmd(&mut self, cmd: Cmd) -> Result<(), String> {
        match cmd {
            Cmd::RequestCode(phone) => {
                self.stop_qr();
                let phone = phone.trim().to_owned();
                let sent = self.send_code(&phone).await?;
                self.on_sent_code(phone, sent).await
            }
            Cmd::SubmitCode(code) => {
                let code = code.trim().to_owned();
                let Some(token) = &self.login_token else { return Err(l!("Спершу запросіть код", "Request a code first").into()) };
                if !matches!(token.via, CodeVia::SetUpEmailCode { .. }) {
                    return self.sign_in(code).await;
                }
                // The new login email is confirmed; the server then sends the usual phone code.
                let client = self.client()?;
                let (phone, hash) = (token.phone.clone(), token.hash.clone());
                let req = tl::functions::account::VerifyEmail {
                    purpose: tl::types::EmailVerifyPurposeLoginSetup { phone_number: phone.clone(), phone_code_hash: hash }.into(),
                    verification: tl::types::EmailVerificationCode { code }.into(),
                };
                match client.invoke(&req).await {
                    Ok(tl::enums::account::EmailVerified::Login(v)) => self.on_sent_code(phone, v.sent_code).await,
                    Ok(tl::enums::account::EmailVerified::Verified(_)) => Err(l!("Пошту підтверджено — запросіть код ще раз", "Email confirmed — request the code again").into()),
                    Err(e) if wrong_code(&e) => Err(l!("Невірний код", "Wrong code").into()),
                    Err(e) => Err(e.to_string()),
                }
            }
            Cmd::SetUpLoginEmail(email) => {
                let client = self.client()?;
                let token = self.login_token.as_mut().ok_or(l!("Спершу запросіть код", "Request a code first"))?;
                let req = tl::functions::account::SendVerifyEmailCode {
                    purpose: tl::types::EmailVerifyPurposeLoginSetup { phone_number: token.phone.clone(), phone_code_hash: token.hash.clone() }.into(),
                    email: email.trim().to_owned(),
                };
                let tl::enums::account::SentEmailCode::Code(sent) = client.invoke(&req).await.map_err(|e| email_error(&e))?;
                token.via = CodeVia::SetUpEmailCode { pattern: sent.email_pattern, length: sent.length };
                self.out.send(Ev::NeedCode(token.via.clone()));
                Ok(())
            }
            Cmd::ResetLoginEmail => {
                let client = self.client()?;
                let token = self.login_token.as_ref().ok_or(l!("Спершу запросіть код", "Request a code first"))?;
                let phone = token.phone.clone();
                let req = tl::functions::auth::ResetLoginEmail { phone_number: phone.clone(), phone_code_hash: token.hash.clone() };
                let sent = client.invoke(&req).await.map_err(|e| lf!("Не вдалося скинути пошту: {e}", "Could not reset the email: {e}"))?;
                self.on_sent_code(phone, sent).await
            }
            Cmd::SubmitPassword(pw) => {
                let client = self.client()?;
                let token = self.password_token.take().ok_or(l!("Пароль не очікувався", "Password was not expected"))?;
                match client.check_password(token, pw.as_bytes()).await {
                    Ok(_) => self.after_login().await,
                    Err(SignInError::InvalidPassword(pt)) => {
                        self.password_token = Some(pt);
                        Err(l!("Невірний пароль", "Wrong passcode").into())
                    }
                    Err(e) => Err(e.to_string()),
                }
            }
            Cmd::QrLogin => {
                self.stop_qr();
                let client = self.client()?;
                let session = self.session.clone().ok_or(l!("Немає сесії", "No session"))?;
                self.qr_task = Some(tokio::spawn(qr_loop(
                    client,
                    session,
                    self.api_id,
                    self.api_hash.clone(),
                    self.login_update.clone(),
                    self.out.clone(),
                    self.back.clone(),
                )));
                Ok(())
            }
            Cmd::QrPassword(pt) => {
                self.out.send(Ev::NeedPassword(pt.hint().map(str::to_owned)));
                self.password_token = Some(pt);
                Ok(())
            }
            Cmd::QrSuccess(auth) => {
                self.complete_login(*auth).await?;
                self.after_login().await
            }
            Cmd::Logout => {
                self.stop_qr();
                let client = self.client()?;
                let _ = client.sign_out().await;
                self.client = None;
                self.session = None;
                self.loader = None;
                let _ = std::fs::remove_file(&self.session_path);
                if let Some(c) = &self.out.cache {
                    c.clear();
                }
                self.out.send(Ev::LoggedOut);
                Ok(())
            }
            _ => Ok(()),
        }
    }
}

/// A refused email address or code, in words; shared with the login email in Settings.
pub(super) fn email_error(e: &InvocationError) -> String {
    if e.is("EMAIL_INVALID") {
        l!("Невірна адреса пошти", "Invalid email address").to_owned()
    } else if e.is("EMAIL_NOT_ALLOWED") {
        l!("Цю адресу не можна використати для входу", "This address can't be used to sign in").to_owned()
    } else if wrong_code(e) {
        l!("Невірний код", "Wrong code").to_owned()
    } else if e.is("EMAIL_HASH_EXPIRED") || e.is("CODE_EXPIRED") {
        l!("Код застарів — надішліть новий", "The code has expired — send a new one").to_owned()
    } else {
        e.to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tl::enums::auth::SentCodeType as T;

    #[test]
    fn sent_code_types() {
        assert_eq!(code_via(&T::App(tl::types::auth::SentCodeTypeApp { length: 5 })), CodeVia::App);
        assert_eq!(code_via(&T::Sms(tl::types::auth::SentCodeTypeSms { length: 5 })), CodeVia::Sms);
        assert_eq!(code_via(&T::Call(tl::types::auth::SentCodeTypeCall { length: 5 })), CodeVia::Call);
        let email = code_via(&T::EmailCode(tl::types::auth::SentCodeTypeEmailCode {
            apple_signin_allowed: true,
            google_signin_allowed: false,
            email_pattern: "a***@e***.com".into(),
            length: 6,
            reset_available_period: Some(604800),
            reset_pending_date: None,
        }));
        assert_eq!(
            email,
            CodeVia::Email { pattern: "a***@e***.com".into(), length: 6, apple: true, google: false, reset_after: Some(604800), reset_at: None }
        );
        assert!(email.is_email());
        let setup = code_via(&T::SetUpEmailRequired(tl::types::auth::SentCodeTypeSetUpEmailRequired { apple_signin_allowed: false, google_signin_allowed: true }));
        assert_eq!(setup, CodeVia::SetUpEmail { apple: false, google: true });
        // Binding the email is not a code yet; its own code is.
        assert!(!setup.is_email());
        assert!(CodeVia::SetUpEmailCode { pattern: String::new(), length: 6 }.is_email());
        assert!(!CodeVia::Sms.is_email());
    }
}
