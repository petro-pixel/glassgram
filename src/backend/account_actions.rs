//! Changing the account's number (`account.sendChangePhoneCode`, `changePhone`, with the country
//! list from `help.getCountriesList`) and deleting the account (`account.deleteAccount`, with an
//! SRP proof of the cloud password when one is set). Both run only from an explicit user action.

use grammers_client::InvocationError;

use super::*;

/// One calling code a country uses: `+1 204` is Canada, `+1` alone the US.
#[derive(Debug, Clone, PartialEq)]
pub struct CountryCode {
    /// Digits after "+": the country code plus a prefix, if the country has prefixes.
    pub digits: String,
    /// The country code alone, what the picker puts in front of the number.
    pub code: String,
    pub iso2: String,
    pub name: String,
}

/// Digits only: what the server expects for a phone number.
pub fn phone_digits(phone: &str) -> String {
    phone.chars().filter(char::is_ascii_digit).collect()
}

/// The country of a number typed so far: the longest code it starts with.
pub fn country_of<'a>(list: &'a [CountryCode], phone: &str) -> Option<&'a CountryCode> {
    let digits = phone_digits(phone);
    list.iter().filter(|c| !c.digits.is_empty() && digits.starts_with(&c.digits)).max_by_key(|c| c.digits.len())
}

/// Readable explanation of the refusals Telegram Desktop names; the rest as is.
fn action_error(e: &InvocationError, phone: &str) -> String {
    let Some(name) = account::rpc_name(e) else { return e.to_string() };
    match name {
        "PHONE_NUMBER_OCCUPIED" => lf!(
            "Номер {phone} уже прив'язаний до акаунта Telegram. Спершу видаліть той акаунт, а тоді переходьте на цей номер.",
            "The number {phone} is already connected to a Telegram account. Please delete that account before migrating to the new number."
        ),
        "PHONE_NUMBER_INVALID" => l!("Некоректний номер телефону", "Invalid phone number").into(),
        "PHONE_NUMBER_BANNED" => l!("Цей номер заблоковано в Telegram", "This phone number is banned").into(),
        "PHONE_CODE_INVALID" | "PHONE_CODE_EMPTY" => l!("Ви ввели невірний код", "You have entered an invalid code").into(),
        "PHONE_CODE_EXPIRED" => l!("Код застарів — надішліть новий", "The code has expired — request a new one").into(),
        "PASSWORD_HASH_INVALID" => l!("Невірний пароль", "Wrong password").into(),
        "FLOOD_WAIT" | "FLOOD_PREMIUM_WAIT" => match e {
            InvocationError::Rpc(r) if r.value.is_some() => {
                let min = r.value.unwrap_or(0).div_ceil(60);
                lf!("Забагато спроб. Спробуйте ще раз за {min} хв", "Too many tries. Please try again in {min} min")
            }
            _ => l!("Забагато спроб. Спробуйте пізніше", "Too many tries. Please try again later").into(),
        },
        // Without the password a 2FA account is deleted only after a week of waiting.
        n if n.ends_with("CONFIRM_WAIT") => l!(
            "Telegram видалить акаунт за тиждень, якщо ви не зайдете в нього з іншого пристрою",
            "Telegram will delete the account in a week unless you log in to it from another device"
        )
        .into(),
        _ => e.to_string(),
    }
}

impl Worker {
    pub(super) async fn account_actions_cmd(&mut self, cmd: Cmd) -> Result<(), String> {
        let client = self.client()?;
        let fail = |out: &Emitter, e: String| out.send(Ev::AccountActionFailed(e));
        match cmd {
            Cmd::LoadCountries(lang_code) => {
                let req = tl::functions::help::GetCountriesList { lang_code, hash: 0 };
                let tl::enums::help::CountriesList::List(list) = client.invoke(&req).await.map_err(|e| lf!("Країни: {e}", "Countries: {e}"))? else {
                    return Ok(());
                };
                let mut codes = Vec::new();
                for tl::enums::help::Country::Country(c) in list.countries {
                    let name = c.name.clone().filter(|n| !n.is_empty()).unwrap_or(c.default_name.clone());
                    for tl::enums::help::CountryCode::Code(code) in c.country_codes {
                        let prefixes = code.prefixes.unwrap_or_default();
                        let prefixes = if prefixes.is_empty() { vec![String::new()] } else { prefixes };
                        for p in prefixes {
                            codes.push(CountryCode { digits: format!("{}{p}", code.country_code), code: code.country_code.clone(), iso2: c.iso2.clone(), name: name.clone() });
                        }
                    }
                }
                self.out.send(Ev::Countries(codes));
            }
            Cmd::SendChangePhoneCode(phone) => {
                let digits = phone_digits(&phone);
                let settings = tl::types::CodeSettings {
                    allow_flashcall: false,
                    current_number: false,
                    allow_app_hash: false,
                    allow_missed_call: false,
                    allow_firebase: false,
                    unknown_number: false,
                    logout_tokens: None,
                    token: None,
                    app_sandbox: None,
                };
                let req = tl::functions::account::SendChangePhoneCode { phone_number: digits.clone(), settings: settings.into() };
                match client.invoke(&req).await {
                    Ok(tl::enums::auth::SentCode::Code(c)) => {
                        use tl::enums::auth::SentCodeType as T;
                        let (length, via_app) = match c.r#type {
                            T::App(t) => (t.length, true),
                            T::Sms(t) => (t.length, false),
                            T::Call(t) => (t.length, false),
                            T::FragmentSms(t) => (t.length, false),
                            _ => (0, false),
                        };
                        self.out.send(Ev::ChangePhoneCode { phone: format!("+{digits}"), hash: c.phone_code_hash, length, via_app });
                    }
                    Ok(_) => fail(&self.out, l!("Telegram просить підтвердити номер у мобільному застосунку", "Telegram asks to confirm the number in the mobile app").into()),
                    Err(e) => fail(&self.out, action_error(&e, &format!("+{digits}"))),
                }
            }
            Cmd::ChangePhone { phone, hash, code } => {
                let digits = phone_digits(&phone);
                let req = tl::functions::account::ChangePhone { phone_number: digits.clone(), phone_code_hash: hash, phone_code: code.trim().to_owned() };
                match client.invoke(&req).await {
                    Ok(_) => {
                        self.out.send(Ev::PhoneChanged(format!("+{digits}")));
                        self.load_profile().await?;
                    }
                    Err(e) => fail(&self.out, action_error(&e, &phone)),
                }
            }
            Cmd::DeleteAccount { reason, password } => {
                let p = password::get(&client).await?;
                let proof = if p.has_password {
                    if password.is_empty() {
                        fail(&self.out, l!("Введіть пароль двоетапної перевірки", "Enter your two-step verification password").into());
                        return Ok(());
                    }
                    // PBKDF2 takes a moment: off the async thread.
                    match tokio::task::spawn_blocking(move || password::proof(&p, &password)).await {
                        Ok(Ok(proof)) => Some(proof),
                        Ok(Err(e)) => return Err(e),
                        Err(e) => return Err(e.to_string()),
                    }
                } else {
                    None
                };
                match client.invoke(&tl::functions::account::DeleteAccount { reason, password: proof }).await {
                    Ok(_) => {
                        self.out.send(Ev::AccountDeleted);
                        // The authorization is gone with the account: forget it locally, like logging out.
                        self.forget_session();
                    }
                    Err(e) => fail(&self.out, action_error(&e, "")),
                }
            }
            _ => {}
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn code(digits: &str, iso2: &str) -> CountryCode {
        CountryCode { digits: digits.into(), code: String::new(), iso2: iso2.into(), name: iso2.into() }
    }

    #[test]
    fn longest_code_wins() {
        let list = [code("1", "US"), code("1204", "CA"), code("380", "UA"), code("38", "XX")];
        assert_eq!(country_of(&list, "+1 204 555").map(|c| c.iso2.as_str()), Some("CA"));
        assert_eq!(country_of(&list, "+1 212 555").map(|c| c.iso2.as_str()), Some("US"));
        assert_eq!(country_of(&list, "+380 67").map(|c| c.iso2.as_str()), Some("UA"));
        assert_eq!(country_of(&list, "+7"), None);
        assert_eq!(phone_digits("+380 (67) 123-45-67"), "380671234567");
    }
}
