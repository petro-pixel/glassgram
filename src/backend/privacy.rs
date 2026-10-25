//! Privacy: exceptions of the per-setting rules ("always" / "never" for people and groups) and the
//! global settings (`account.getGlobalPrivacySettings`): archiving new chats from non-contacts,
//! hiding read times, who can message me.

use grammers_client::peer::{Peer, User};
use grammers_client::session::types::{PeerId, PeerKind};
use grammers_client::tl;

use super::{Audience, ChatInfo, Cmd, Ev, Worker, person};
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

/// People and groups a privacy rule makes an exception for.
#[derive(Debug, Clone, Default)]
pub struct PrivacyExceptions {
    /// "Always allow".
    pub allow: Vec<ChatInfo>,
    /// "Never allow".
    pub deny: Vec<ChatInfo>,
}

impl Worker {
    /// Exception lists from the rules, with names and access remembered for later edits.
    pub(super) async fn privacy_exceptions(&self, r: &tl::types::account::PrivacyRules) -> PrivacyExceptions {
        let Ok(client) = self.client() else { return PrivacyExceptions::default() };
        let peers: Vec<Peer> =
            r.users.iter().cloned().map(|u| Peer::User(User::from_raw(&client, u))).chain(r.chats.iter().cloned().map(|c| Peer::from_raw(&client, c))).collect();
        let mut out = PrivacyExceptions::default();
        for rule in &r.rules {
            use tl::enums::PrivacyRule as R;
            let (ids, allow): (Vec<i64>, bool) = match rule {
                R::PrivacyValueAllowUsers(u) => (u.users.clone(), true),
                R::PrivacyValueDisallowUsers(u) => (u.users.clone(), false),
                R::PrivacyValueAllowChatParticipants(c) => (c.chats.clone(), true),
                R::PrivacyValueDisallowChatParticipants(c) => (c.chats.clone(), false),
                _ => continue,
            };
            for id in ids {
                let Some(p) = peers.iter().find(|p| p.id().bare_id_unchecked() == id) else { continue };
                if self.remember(p).await {
                    let list = if allow { &mut out.allow } else { &mut out.deny };
                    list.push(person(p));
                }
            }
        }
        out
    }

    /// The base rule plus exceptions, in the order Telegram Desktop sends them.
    pub(super) fn privacy_input(&self, audience: Audience, allow: &[PeerId], deny: &[PeerId]) -> Result<Vec<tl::enums::InputPrivacyRule>, String> {
        use tl::enums::InputPrivacyRule as R;
        let users = |list: &[PeerId]| -> Result<Vec<tl::enums::InputUser>, String> {
            list.iter().filter(|p| p.kind() == PeerKind::User).map(|p| self.peer_ref(*p).map(tl::enums::InputUser::from)).collect()
        };
        let chats = |list: &[PeerId]| -> Vec<i64> { list.iter().filter(|p| p.kind() != PeerKind::User).map(|p| p.bare_id_unchecked()).collect() };
        let mut rules = Vec::new();
        // "Always" only makes sense when not everybody is allowed, "never" when not nobody.
        if audience != Audience::Everybody {
            let (u, c) = (users(allow)?, chats(allow));
            if !u.is_empty() {
                rules.push(tl::types::InputPrivacyValueAllowUsers { users: u }.into());
            }
            if !c.is_empty() {
                rules.push(tl::types::InputPrivacyValueAllowChatParticipants { chats: c }.into());
            }
        }
        if audience != Audience::Nobody {
            let (u, c) = (users(deny)?, chats(deny));
            if !u.is_empty() {
                rules.push(tl::types::InputPrivacyValueDisallowUsers { users: u }.into());
            }
            if !c.is_empty() {
                rules.push(tl::types::InputPrivacyValueDisallowChatParticipants { chats: c }.into());
            }
        }
        match audience {
            Audience::Everybody => rules.push(R::InputPrivacyValueAllowAll),
            Audience::Contacts => rules.extend([R::InputPrivacyValueAllowContacts, R::InputPrivacyValueDisallowAll]),
            Audience::Nobody => rules.push(R::InputPrivacyValueDisallowAll),
        }
        Ok(rules)
    }
}
