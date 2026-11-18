//! Demo cloud password: "demo", with a hint; changes follow the real flow.

use super::*;

/// Two-step verification without a server: the password is "demo", any 4-digit code confirms an email.
pub(super) fn handle(app: &mut App, cmd: Cmd) {
    let wrong = |app: &mut App| app.push_event(Ev::PasswordFailed("Невірний пароль · Wrong password".into()));
    PASSWORD.with_borrow_mut(|(pw, state)| match cmd {
        Cmd::CheckPassword(p) if p == *pw => app.push_event(Ev::PasswordChecked { email: if state.has_recovery { "o***@example.com".into() } else { String::new() } }),
        Cmd::CheckPassword(_) => wrong(app),
        Cmd::UpdatePassword { current, .. } if state.has_password && current != *pw => wrong(app),
        Cmd::UpdatePassword { change, .. } => match change {
            PasswordChange::Set { new, hint, email } => {
                *pw = new;
                state.has_password = true;
                state.hint = hint;
                if !email.is_empty() {
                    state.email_pending = Some("o***@example.com".into());
                }
            }
            PasswordChange::Email(_) => state.email_pending = Some("o***@example.com".into()),
            PasswordChange::Off => *state = PasswordState::default(),
        },
        Cmd::ConfirmPasswordEmail(code) if code.len() == 4 => {
            state.email_pending = None;
            state.has_recovery = true;
        }
        Cmd::ConfirmPasswordEmail(_) => app.push_event(Ev::PasswordFailed("Невірний код · Wrong code".into())),
        Cmd::CancelPasswordEmail => state.email_pending = None,
        _ => {}
    });
    app.push_event(Ev::Password(PASSWORD.with_borrow(|(_, s)| s.clone())));
}
