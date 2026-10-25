//! Global privacy settings (`account.getGlobalPrivacySettings`): archiving new chats from
//! non-contacts, hiding read times, who can message me.

use grammers_client::tl;

use super::{Cmd, Ev, Worker};
use crate::i18n::lf;

/// The switches the Privacy and Security page shows; other server fields are kept as they are.
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct GlobalPrivacy {
    /// Archive and mute new chats from non-contacts.
    pub archive_new: bool,
    /// Archived chats stay archived when a message arrives, even if unmuted.
    pub keep_unmuted: bool,
    /// Archived chats that are in folders stay archived too.
    pub keep_folders: bool,
    /// Don't show others when I've read their messages (and don't see theirs).
    pub hide_read: bool,
    /// Only contacts and Premium users can start a chat with me (setting it needs Premium).
    pub premium_only: bool,
}

impl GlobalPrivacy {
    fn from_raw(s: &tl::types::GlobalPrivacySettings) -> Self {
        Self {
            archive_new: s.archive_and_mute_new_noncontact_peers,
            keep_unmuted: s.keep_archived_unmuted,
            keep_folders: s.keep_archived_folders,
            hide_read: s.hide_read_marks,
            premium_only: s.new_noncontact_peers_require_premium,
        }
    }
}

async fn get(client: &grammers_client::Client) -> Result<tl::types::GlobalPrivacySettings, String> {
    let tl::enums::GlobalPrivacySettings::Settings(s) =
        client.invoke(&tl::functions::account::GetGlobalPrivacySettings {}).await.map_err(|e| lf!("Приватність: {e}", "Privacy: {e}"))?;
    Ok(s)
}

impl Worker {
    pub(super) async fn global_privacy_cmd(&mut self, cmd: Cmd) -> Result<(), String> {
        let client = self.client()?;
        match cmd {
            Cmd::LoadGlobalPrivacy => {
                let s = get(&client).await?;
                self.out.send(Ev::GlobalPrivacy(GlobalPrivacy::from_raw(&s)));
            }
            Cmd::SetGlobalPrivacy(p) => {
                // Start from the server's settings so fields we don't show (gifts, paid messages) survive.
                let mut s = get(&client).await?;
                s.archive_and_mute_new_noncontact_peers = p.archive_new;
                s.keep_archived_unmuted = p.keep_unmuted;
                s.keep_archived_folders = p.keep_folders;
                s.hide_read_marks = p.hide_read;
                s.new_noncontact_peers_require_premium = p.premium_only;
                let res = client.invoke(&tl::functions::account::SetGlobalPrivacySettings { settings: s.into() }).await;
                // On refusal (e.g. Premium needed) the UI gets the real state back along with the error.
                let state = match res {
                    Ok(tl::enums::GlobalPrivacySettings::Settings(s)) => GlobalPrivacy::from_raw(&s),
                    Err(e) => {
                        self.out.send(Ev::Error(lf!("Не вдалося змінити приватність: {e}", "Could not change privacy: {e}")));
                        GlobalPrivacy::from_raw(&get(&client).await?)
                    }
                };
                self.out.send(Ev::GlobalPrivacy(state));
            }
            _ => {}
        }
        Ok(())
    }
}
