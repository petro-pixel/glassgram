//! Signing in: phone code, 2FA password, QR login and logging out.

use super::*;

impl Worker {
    pub(super) async fn login_cmd(&mut self, cmd: Cmd) -> Result<(), String> {
        match cmd {
            Cmd::RequestCode(phone) => {
                self.stop_qr();
                let client = self.client()?;
                let token = client
                    .request_login_code(phone.trim(), &self.api_hash)
                    .await
                    .map_err(|e| lf!("Не вдалося надіслати код: {e}", "Could not send the code: {e}"))?;
                self.login_token = Some(token);
                self.out.send(Ev::NeedCode);
                Ok(())
            }
            Cmd::SubmitCode(code) => {
                let client = self.client()?;
                let token = self.login_token.as_ref().ok_or(l!("Спершу запросіть код", "Request a code first"))?;
                match client.sign_in(token, code.trim()).await {
                    Ok(_) => self.after_login().await,
                    Err(SignInError::PasswordRequired(pt)) => {
                        self.out.send(Ev::NeedPassword(pt.hint().map(str::to_owned)));
                        self.password_token = Some(pt);
                        Ok(())
                    }
                    Err(SignInError::InvalidCode) => Err(l!("Невірний код", "Wrong code").into()),
                    Err(SignInError::SignUpRequired) => {
                        Err(l!("Акаунт не знайдено — зареєструйтесь в офіційному застосунку", "Account not found — sign up in the official app").into())
                    }
                    Err(e) => Err(e.to_string()),
                }
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
