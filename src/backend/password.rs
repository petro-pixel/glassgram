//! Two-step verification management, as in Telegram Desktop's Privacy and Security → Two-Step
//! Verification: turning the cloud password on, changing it, its hint and recovery email, turning
//! it off (`account.getPassword`, `getPasswordSettings`, `updatePasswordSettings`) and the email
//! confirmation code (`confirmPasswordEmail`, `resendPasswordEmail`, `cancelPasswordEmail`).
//!
//! The password never leaves this module in plain form: the server only gets an SRP proof of the
//! current one (https://core.telegram.org/api/srp) and `g^x mod p` of the new one.

use grammers_client::{Client, tl};
use hmac::Hmac;
use num_bigint::BigUint;
use sha2_11::{Digest, Sha256, Sha512};

use super::{Cmd, Ev, Worker};
use crate::i18n::{l, lf};

/// What the page needs to know about the account's cloud password.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct PasswordState {
    pub has_password: bool,
    pub hint: String,
    /// A confirmed recovery email is set.
    pub has_recovery: bool,
    /// A recovery email waits for its code: its masked form (`a***@g***.com`).
    pub email_pending: Option<String>,
}

/// What to do with the password once the current one is known.
#[derive(Debug, Clone)]
pub enum PasswordChange {
    /// A new password (also the first one); an empty email keeps the current one.
    Set { new: String, hint: String, email: String },
    /// Only the recovery email.
    Email(String),
    Off,
}

impl PasswordState {
    fn from_raw(p: &tl::types::account::Password) -> Self {
        Self {
            has_password: p.has_password,
            hint: p.hint.clone().unwrap_or_default(),
            has_recovery: p.has_recovery,
            email_pending: p.email_unconfirmed_pattern.clone().filter(|s| !s.is_empty()),
        }
    }
}

async fn get(client: &Client) -> Result<tl::types::account::Password, String> {
    let tl::enums::account::Password::Password(p) =
        client.invoke(&tl::functions::account::GetPassword {}).await.map_err(|e| lf!("Пароль: {e}", "Password: {e}"))?;
    Ok(p)
}

fn random(n: usize) -> Result<Vec<u8>, String> {
    let mut buf = vec![0; n];
    getrandom::fill(&mut buf).map_err(|e| e.to_string())?;
    Ok(buf)
}

fn modpow_algo(algo: &tl::enums::PasswordKdfAlgo) -> Result<&tl::types::PasswordKdfAlgoSha256Sha256Pbkdf2Hmacsha512iter100000Sha256ModPow, String> {
    match algo {
        tl::enums::PasswordKdfAlgo::Sha256Sha256Pbkdf2Hmacsha512iter100000Sha256ModPow(a)
            // `check_p_and_g` panics on other generators; a bad prime would leak the password.
            if (2..=7).contains(&a.g) && grammers_crypto::two_factor_auth::check_p_and_g(&a.p, &a.g) =>
        {
            Ok(a)
        }
        _ => Err(l!("Сервер прислав невідомий алгоритм пароля — оновіть Glassgram", "The server sent an unknown password algorithm — update Glassgram").to_owned()),
    }
}

/// SRP proof of the current password, or "no password" when there is none.
fn proof(p: &tl::types::account::Password, password: &str) -> Result<tl::enums::InputCheckPasswordSrp, String> {
    if !p.has_password {
        return Ok(tl::enums::InputCheckPasswordSrp::InputCheckPasswordEmpty);
    }
    let algo = modpow_algo(p.current_algo.as_ref().ok_or("current_algo")?)?;
    let (Some(srp_b), Some(srp_id)) = (p.srp_b.clone(), p.srp_id) else { return Err("srp_B".into()) };
    let (m1, g_a) = grammers_crypto::two_factor_auth::calculate_2fa(&algo.salt1, &algo.salt2, &algo.p, &algo.g, srp_b, random(256)?, password);
    Ok(tl::types::InputCheckPasswordSrp { srp_id, a: g_a.to_vec(), m1: m1.to_vec() }.into())
}

fn sh(data: &[u8], salt: &[u8]) -> [u8; 32] {
    Sha256::new().chain_update(salt).chain_update(data).chain_update(salt).finalize().into()
}

/// `v = g^PH2(password) mod p`, padded to 256 bytes — what the server stores instead of the password.
fn verifier(algo: &tl::types::PasswordKdfAlgoSha256Sha256Pbkdf2Hmacsha512iter100000Sha256ModPow, password: &str) -> Vec<u8> {
    let ph1 = sh(&sh(password.as_bytes(), &algo.salt1), &algo.salt2);
    let mut dk = [0u8; 64];
    pbkdf2::pbkdf2::<Hmac<Sha512>>(&ph1, &algo.salt1, 100_000, &mut dk).expect("64-byte output is valid for HMAC");
    let x = BigUint::from_bytes_be(&sh(&dk, &algo.salt2));
    let v = BigUint::from(algo.g as u32).modpow(&x, &BigUint::from_bytes_be(&algo.p)).to_bytes_be();
    let mut out = vec![0; 256 - v.len().min(256)];
    out.extend_from_slice(&v);
    out
}

/// The settings that set `new` as the password: the server's new algorithm with 32 bytes of our own salt.
fn new_password(p: &tl::types::account::Password, new: &str, hint: String) -> Result<tl::types::account::PasswordInputSettings, String> {
    let mut algo = modpow_algo(&p.new_algo)?.clone();
    algo.salt1.extend(random(32)?);
    let hash = verifier(&algo, new);
    Ok(tl::types::account::PasswordInputSettings {
        new_algo: Some(algo.into()),
        new_password_hash: Some(hash),
        hint: Some(hint),
        email: None,
        new_secure_settings: None,
    })
}

/// Telegram asks for the code when a new recovery email is set: not an error for the user.
fn email_sent(e: &grammers_client::InvocationError) -> bool {
    e.is("EMAIL_UNCONFIRMED*")
}

fn wrong(e: &grammers_client::InvocationError) -> String {
    if e.is("PASSWORD_HASH_INVALID") {
        l!("Невірний пароль", "Wrong password").to_owned()
    } else if e.is("EMAIL_INVALID") {
        l!("Невірна адреса пошти", "Invalid email address").to_owned()
    } else if e.is("CODE_INVALID") {
        l!("Невірний код", "Wrong code").to_owned()
    } else {
        e.to_string()
    }
}

impl Worker {
    pub(super) async fn password_cmd(&mut self, cmd: Cmd) -> Result<(), String> {
        let client = self.client()?;
        let out = self.out.clone();
        // PBKDF2 and the prime check take a moment: off the command loop.
        tokio::spawn(async move {
            let res = run(&client, cmd).await;
            if let Err(e) = &res {
                out.send(Ev::PasswordFailed(e.clone()));
            }
            match get(&client).await {
                Ok(p) => out.send(Ev::Password(PasswordState::from_raw(&p))),
                Err(e) => out.send(Ev::Error(e)),
            }
            if let Ok(Some(ev)) = res {
                out.send(ev);
            }
        });
        Ok(())
    }
}

/// One password command; the fresh state is sent after it either way.
async fn run(client: &Client, cmd: Cmd) -> Result<Option<Ev>, String> {
    match cmd {
        Cmd::LoadPassword => Ok(None),
        Cmd::CheckPassword(current) => {
            let p = get(client).await?;
            let req = tl::functions::account::GetPasswordSettings { password: proof(&p, &current)? };
            let tl::enums::account::PasswordSettings::Settings(s) = client.invoke(&req).await.map_err(|e| wrong(&e))?;
            Ok(Some(Ev::PasswordChecked { email: s.email.unwrap_or_default() }))
        }
        Cmd::UpdatePassword { current, change } => {
            let p = get(client).await?;
            let password = proof(&p, &current)?;
            let new_settings = match change {
                PasswordChange::Set { new, hint, email } => {
                    let mut s = new_password(&p, &new, hint)?;
                    s.email = Some(email).filter(|e| !e.trim().is_empty());
                    s
                }
                PasswordChange::Email(email) => tl::types::account::PasswordInputSettings {
                    new_algo: None,
                    new_password_hash: None,
                    hint: None,
                    email: Some(email),
                    new_secure_settings: None,
                },
                PasswordChange::Off => tl::types::account::PasswordInputSettings {
                    new_algo: Some(tl::enums::PasswordKdfAlgo::Unknown),
                    new_password_hash: Some(Vec::new()),
                    hint: Some(String::new()),
                    email: Some(String::new()),
                    new_secure_settings: None,
                },
            };
            let req = tl::functions::account::UpdatePasswordSettings { password, new_settings: new_settings.into() };
            match client.invoke(&req).await {
                Ok(_) => Ok(Some(Ev::Notice(l!("Пароль збережено", "Password saved").to_owned()))),
                Err(e) if email_sent(&e) => Ok(Some(Ev::Notice(l!("Код надіслано на пошту", "A code was sent to the email").to_owned()))),
                Err(e) => Err(wrong(&e)),
            }
        }
        Cmd::ConfirmPasswordEmail(code) => {
            client.invoke(&tl::functions::account::ConfirmPasswordEmail { code }).await.map_err(|e| wrong(&e))?;
            Ok(Some(Ev::Notice(l!("Пошту підтверджено", "Email confirmed").to_owned())))
        }
        Cmd::ResendPasswordEmail => {
            client.invoke(&tl::functions::account::ResendPasswordEmail {}).await.map_err(|e| wrong(&e))?;
            Ok(Some(Ev::Notice(l!("Код надіслано ще раз", "The code was sent again").to_owned())))
        }
        Cmd::CancelPasswordEmail => {
            client.invoke(&tl::functions::account::CancelPasswordEmail {}).await.map_err(|e| wrong(&e))?;
            Ok(None)
        }
        _ => Ok(None),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn h(parts: &[&[u8]]) -> Vec<u8> {
        parts.iter().fold(Sha256::new(), |d, p| d.chain_update(p)).finalize().to_vec()
    }

    fn pad(v: &BigUint) -> Vec<u8> {
        let b = v.to_bytes_be();
        let mut out = vec![0; 256 - b.len()];
        out.extend(b);
        out
    }

    /// Plays the server's side of SRP with our verifier: the proof grammers computes from the same
    /// password must be the one the server expects, or a password we set could never be entered.
    #[test]
    fn verifier_matches_srp_proof() {
        let algo = tl::types::PasswordKdfAlgoSha256Sha256Pbkdf2Hmacsha512iter100000Sha256ModPow { salt1: vec![1, 2, 3], salt2: vec![4, 5], g: 3, p: vec![0xfb] };
        let (p, g) = (BigUint::from(0xfbu32), BigUint::from(3u32));
        let v = BigUint::from_bytes_be(&verifier(&algo, "secret"));
        let g_pad = pad(&g);
        let k = BigUint::from_bytes_be(&h(&[&algo.p, &g_pad]));
        let b = BigUint::from(77u32);
        let g_b = (&k * &v + g.modpow(&b, &p)) % &p;
        let a = vec![55];
        let (m1, g_a) = grammers_crypto::two_factor_auth::calculate_2fa(&algo.salt1, &algo.salt2, &algo.p, &algo.g, g_b.to_bytes_be(), a, "secret");
        let u = BigUint::from_bytes_be(&h(&[&g_a, &pad(&g_b)]));
        let s = (BigUint::from_bytes_be(&g_a) * v.modpow(&u, &p)).modpow(&b, &p);
        let xor: Vec<u8> = h(&[&algo.p]).iter().zip(h(&[&g_pad])).map(|(x, y)| x ^ y).collect();
        let expected = h(&[&xor, &h(&[&algo.salt1]), &h(&[&algo.salt2]), &g_a, &pad(&g_b), &h(&[&pad(&s)])]);
        assert_eq!(m1.to_vec(), expected);
    }
}
