//! Privacy and Security → Login Email and Passkeys: the masked login email from `account.getPassword`
//! and changing it (`account.sendVerifyEmailCode` + `account.verifyEmail`, purpose LoginChange);
//! the account's passkeys (`account.getPasskeys`) and deleting one (`account.deletePasskey`).
//!
//! Creating a passkey (`account.initPasskeyRegistration` + `registerPasskey`) needs a WebAuthn
//! authenticator, which has no pure Rust path to the system one on all three platforms yet.

use super::*;

/// A passkey bound to the account.
#[derive(Debug, Clone, PartialEq)]
pub struct Passkey {
    pub id: String,
    pub name: String,
    /// Unix times it was created and last used to sign in.
    pub date: i64,
    pub last_used: Option<i64>,
}

impl Passkey {
    fn from_raw(p: tl::enums::Passkey) -> Self {
        let tl::enums::Passkey::Passkey(p) = p;
        Self { id: p.id, name: p.name, date: p.date as i64, last_used: p.last_usage_date.map(i64::from) }
    }
}

async fn email_pattern(client: &Client) -> Result<Option<String>, String> {
    let p = password::get(client).await?;
    Ok(p.login_email_pattern.filter(|s| !s.is_empty()))
}

async fn passkeys(client: &Client) -> Result<Vec<Passkey>, String> {
    let tl::enums::account::Passkeys::Passkeys(res) =
        client.invoke(&tl::functions::account::GetPasskeys {}).await.map_err(|e| lf!("Ключі доступу: {e}", "Passkeys: {e}"))?;
    Ok(res.passkeys.into_iter().map(Passkey::from_raw).collect())
}

/// One command; its results go out as events, a failure as `Ev::LoginSecurityFailed`.
async fn run(client: &Client, out: &Emitter, cmd: Cmd) -> Result<(), String> {
    match cmd {
        Cmd::LoadLoginSecurity => {
            out.send(Ev::LoginEmail(email_pattern(client).await?));
            // Not every account (or server) answers with passkeys: an empty list hides nothing important.
            out.send(Ev::Passkeys(passkeys(client).await.unwrap_or_default()));
        }
        Cmd::ChangeLoginEmail(email) => {
            let req = tl::functions::account::SendVerifyEmailCode { purpose: tl::enums::EmailVerifyPurpose::LoginChange, email: email.trim().to_owned() };
            let tl::enums::account::SentEmailCode::Code(sent) = client.invoke(&req).await.map_err(|e| login::email_error(&e))?;
            out.send(Ev::LoginEmailCode { pattern: sent.email_pattern });
        }
        Cmd::VerifyLoginEmail(code) => {
            let req = tl::functions::account::VerifyEmail {
                purpose: tl::enums::EmailVerifyPurpose::LoginChange,
                verification: tl::types::EmailVerificationCode { code: code.trim().to_owned() }.into(),
            };
            client.invoke(&req).await.map_err(|e| login::email_error(&e))?;
            out.send(Ev::LoginEmail(email_pattern(client).await?));
        }
        Cmd::DeletePasskey(id) => {
            client
                .invoke(&tl::functions::account::DeletePasskey { id })
                .await
                .map_err(|e| lf!("Не вдалося видалити ключ: {e}", "Could not delete the passkey: {e}"))?;
            out.send(Ev::Passkeys(passkeys(client).await?));
        }
        _ => {}
    }
    Ok(())
}

impl Worker {
    pub(super) async fn login_security_cmd(&mut self, cmd: Cmd) -> Result<(), String> {
        let client = self.client()?;
        let out = self.out.clone();
        tokio::spawn(async move {
            if let Err(e) = run(&client, &out, cmd).await {
                out.send(Ev::LoginSecurityFailed(e));
            }
        });
        Ok(())
    }
}
