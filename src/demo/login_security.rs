//! Demo login email and passkeys: a masked email that changes after any 5-digit code, and two
//! passkeys that can be deleted.

use std::cell::RefCell;

use crate::app::App;
use crate::backend::{Cmd, Ev, Passkey};

struct Demo {
    email: String,
    /// The masked address a code was "sent" to.
    pending: Option<String>,
    passkeys: Vec<Passkey>,
}

thread_local! {
    static STATE: RefCell<Demo> = RefCell::new(Demo {
        email: "o***@example.com".into(),
        pending: None,
        passkeys: vec![
            Passkey { id: "pk-phone".into(), name: "Pixel 9".into(), date: 1_760_000_000, last_used: Some(1_763_000_000) },
            Passkey { id: "pk-laptop".into(), name: "iCloud Keychain".into(), date: 1_745_000_000, last_used: None },
        ],
    });
}

/// "petro@example.com" → "p***@example.com", as the server masks it.
fn mask(email: &str) -> String {
    match email.split_once('@') {
        Some((user, domain)) => format!("{}***@{domain}", user.chars().next().unwrap_or('*')),
        None => email.to_owned(),
    }
}

pub(super) fn handle(app: &mut App, cmd: Cmd) {
    let evs = STATE.with_borrow_mut(|s| match cmd {
        Cmd::LoadLoginSecurity => vec![Ev::LoginEmail(Some(s.email.clone())), Ev::Passkeys(s.passkeys.clone())],
        Cmd::ChangeLoginEmail(email) => {
            let pattern = mask(email.trim());
            s.pending = Some(pattern.clone());
            vec![Ev::LoginEmailCode { pattern }]
        }
        Cmd::VerifyLoginEmail(code) if code.len() == 5 => match s.pending.take() {
            Some(p) => {
                s.email = p;
                vec![Ev::LoginEmail(Some(s.email.clone()))]
            }
            None => vec![Ev::LoginSecurityFailed("Спершу надішліть код · Send a code first".into())],
        },
        Cmd::VerifyLoginEmail(_) => vec![Ev::LoginSecurityFailed("Невірний код · Wrong code".into())],
        Cmd::DeletePasskey(id) => {
            s.passkeys.retain(|p| p.id != id);
            vec![Ev::Passkeys(s.passkeys.clone())]
        }
        _ => Vec::new(),
    });
    for ev in evs {
        app.push_event(ev);
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn masks_like_the_server() {
        assert_eq!(super::mask("petro@example.com"), "p***@example.com");
    }
}
