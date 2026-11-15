//! Transferring the ownership of a group or channel to one of its admins (`messages.editChatCreator`),
//! as Telegram Desktop does it: a first call without a password tells whether the account may transfer
//! at all (two-step verification older than 7 days, this session older than 24 hours), then the
//! cloud password goes as an SRP proof (`password.rs`).

use grammers_client::{Client, InvocationError, tl};
use grammers_client::session::types::PeerRef;

use super::{Cmd, Ev, Worker, password};
use crate::i18n::{l, lf};

/// Where the transfer stands after one call.
#[derive(Debug, Clone, PartialEq)]
pub enum TransferStep {
    /// The account passes the checks: ask for the cloud password.
    AskPassword,
    /// Too soon (or no password): Telegram Desktop's "Security check"; `no_password`: offer to set one.
    SecurityCheck { no_password: bool },
    /// The box stays open with this error (e.g. a wrong password).
    Failed(String),
    Done,
}

fn step_of(res: Result<tl::enums::Updates, InvocationError>, with_password: bool) -> TransferStep {
    match res {
        Ok(_) => TransferStep::Done,
        // Without a password this is the server saying "everything else is fine".
        Err(e) if e.is("PASSWORD_HASH_INVALID") && !with_password => TransferStep::AskPassword,
        Err(e) if e.is("PASSWORD_HASH_INVALID") => TransferStep::Failed(l!("Невірний пароль", "Wrong password").to_owned()),
        Err(e) if e.is("PASSWORD_MISSING") => TransferStep::SecurityCheck { no_password: true },
        Err(e) if e.is("PASSWORD_TOO_FRESH_*") || e.is("SESSION_TOO_FRESH_*") => TransferStep::SecurityCheck { no_password: false },
        Err(e) if e.is("USER_NOT_PARTICIPANT") || e.is("PARTICIPANT_ID_INVALID") => {
            TransferStep::Failed(l!("Цей користувач не є учасником чату.", "This user is not a member of the chat.").to_owned())
        }
        Err(e) if e.is("CHANNELS_ADMIN_PUBLIC_TOO_MUCH") => {
            TransferStep::Failed(l!("У цього користувача забагато публічних каналів чи груп.", "This user owns too many public channels or groups.").to_owned())
        }
        Err(e) if e.is("CHANNELS_ADMIN_LOCATED_TOO_MUCH") => {
            TransferStep::Failed(l!("У цього користувача забагато геогруп.", "This user owns too many location-based groups.").to_owned())
        }
        Err(e) => TransferStep::Failed(lf!("Не вдалося передати права: {e}", "Could not transfer ownership: {e}")),
    }
}

async fn run(client: &Client, chat: PeerRef, user: PeerRef, password: Option<String>) -> TransferStep {
    let with_password = password.is_some();
    let check = match password {
        None => tl::enums::InputCheckPasswordSrp::InputCheckPasswordEmpty,
        Some(pw) => match password::get(client).await.and_then(|p| password::proof(&p, &pw)) {
            Ok(c) => c,
            Err(e) => return TransferStep::Failed(e),
        },
    };
    let req = tl::functions::messages::EditChatCreator { peer: chat.into(), user_id: user.into(), password: check };
    step_of(client.invoke(&req).await, with_password)
}

impl Worker {
    pub(super) async fn transfer_cmd(&mut self, cmd: Cmd) -> Result<(), String> {
        let Cmd::TransferOwnership { peer, user, password } = cmd else { return Ok(()) };
        let client = self.client()?;
        let (chat, member): (PeerRef, PeerRef) = (self.peer_ref(peer)?, self.peer_ref(user)?);
        let out = self.out.clone();
        // The SRP proof runs PBKDF2: off the command loop.
        tokio::spawn(async move {
            let step = run(&client, chat, member, password).await;
            let done = step == TransferStep::Done;
            out.send(Ev::Transfer { peer, user, step });
            if done {
                out.send(Ev::ParticipantsChanged(peer));
            }
        });
        Ok(())
    }
}
