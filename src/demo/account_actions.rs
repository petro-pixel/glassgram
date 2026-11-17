//! Demo "Change number" and "Delete my account": answered locally, and nothing is ever deleted.
//! A number ending in 0000 is "occupied", code 12345 is right, 99999 hits the flood limit.

use crate::app::App;
use crate::backend::{Cmd, CountryCode, Ev, phone_digits};
use crate::i18n::{l, lf};

fn countries() -> Vec<CountryCode> {
    let c = |code: &str, prefix: &str, iso2: &str, name: &str| CountryCode { digits: format!("{code}{prefix}"), code: code.into(), iso2: iso2.into(), name: name.into() };
    vec![
        c("380", "", "UA", "Ukraine"),
        c("48", "", "PL", "Poland"),
        c("49", "", "DE", "Germany"),
        c("44", "", "GB", "United Kingdom"),
        c("1", "", "US", "USA"),
        c("1", "204", "CA", "Canada"),
        c("1", "416", "CA", "Canada"),
        c("33", "", "FR", "France"),
        c("39", "", "IT", "Italy"),
        c("34", "", "ES", "Spain"),
    ]
}

/// `password_ok` — whether the typed cloud password is right (`None` — none is set).
pub(super) fn answer(app: &mut App, cmd: Cmd, password_ok: impl FnOnce(&str) -> Option<bool>) {
    let fail = |app: &mut App, e: String| app.push_event(Ev::AccountActionFailed(e));
    match cmd {
        Cmd::LoadCountries(_) => app.push_event(Ev::Countries(countries())),
        Cmd::SendChangePhoneCode(phone) => {
            let digits = phone_digits(&phone);
            if digits.len() < 7 {
                fail(app, l!("Некоректний номер телефону", "Invalid phone number").into());
            } else if digits.ends_with("0000") {
                let phone = format!("+{digits}");
                fail(app, lf!(
                    "Номер {phone} уже прив'язаний до акаунта Telegram. Спершу видаліть той акаунт, а тоді переходьте на цей номер.",
                    "The number {phone} is already connected to a Telegram account. Please delete that account before migrating to the new number."
                ));
            } else {
                app.push_event(Ev::ChangePhoneCode { phone: format!("+{digits}"), hash: "demo".into(), length: 5, via_app: false });
            }
        }
        Cmd::ChangePhone { phone, code, .. } => match code.as_str() {
            "12345" => app.push_event(Ev::PhoneChanged(format!("+{}", phone_digits(&phone)))),
            "99999" => {
                let min = 5;
                fail(app, lf!("Забагато спроб. Спробуйте ще раз за {min} хв", "Too many tries. Please try again in {min} min"));
            }
            _ => fail(app, l!("Ви ввели невірний код", "You have entered an invalid code").into()),
        },
        Cmd::DeleteAccount { password, .. } => match password_ok(&password) {
            Some(false) => fail(app, l!("Невірний пароль", "Wrong password").into()),
            _ => {
                app.push_event(Ev::AccountDeleted);
                app.push_event(Ev::Notice(l!("Демо: акаунт не видалено — це лише перевірка діалогу", "Demo: the account was not deleted — this only shows the box").into()));
            }
        },
        _ => {}
    }
}
