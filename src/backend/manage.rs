//! Administration of a group or channel, as in Telegram Desktop's "Manage" box: the administrators
//! with their rights and titles, restricted and removed members (`channels.getParticipants`,
//! `channels.editAdmin`, `channels.editBanned`; basic groups: `messages.editChatAdmin`).

use grammers_client::session::types::PeerKind;

use super::*;

/// How many people one list shows; beyond that, search.
const LIST_LIMIT: i32 = 200;

/// Admin rights, as the switches of Telegram Desktop's admin box (`chatAdminRights`).
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct AdminRights {
    pub change_info: bool,
    /// Channels: post, edit others' posts.
    pub post_messages: bool,
    pub edit_messages: bool,
    pub delete_messages: bool,
    /// Groups: ban and restrict members.
    pub ban_users: bool,
    pub invite_users: bool,
    /// Groups: pin messages, manage topics.
    pub pin_messages: bool,
    pub manage_topics: bool,
    pub stories: bool,
    pub manage_call: bool,
    /// Groups: post as the group itself.
    pub anonymous: bool,
    pub add_admins: bool,
}

impl AdminRights {
    /// What the owner may do.
    pub fn all() -> Self {
        Self {
            change_info: true,
            post_messages: true,
            edit_messages: true,
            delete_messages: true,
            ban_users: true,
            invite_users: true,
            pin_messages: true,
            manage_topics: true,
            stories: true,
            manage_call: true,
            anonymous: false,
            add_admins: true,
        }
    }

    /// What Telegram Desktop ticks for a new admin: everything but adding admins and anonymity.
    pub fn new_admin() -> Self {
        Self { add_admins: false, ..Self::all() }
    }

    pub(super) fn from_raw(r: &tl::enums::ChatAdminRights) -> Self {
        let tl::enums::ChatAdminRights::Rights(r) = r;
        Self {
            change_info: r.change_info,
            post_messages: r.post_messages,
            edit_messages: r.edit_messages,
            delete_messages: r.delete_messages,
            ban_users: r.ban_users,
            invite_users: r.invite_users,
            pin_messages: r.pin_messages,
            manage_topics: r.manage_topics,
            stories: r.post_stories || r.edit_stories || r.delete_stories,
            manage_call: r.manage_call,
            anonymous: r.anonymous,
            add_admins: r.add_admins,
        }
    }

    /// Our rights from a chat object: the owner may do everything.
    pub(super) fn of_chat(creator: bool, raw: Option<&tl::enums::ChatAdminRights>) -> Option<Self> {
        if creator {
            return Some(Self::all());
        }
        raw.map(Self::from_raw)
    }

    fn raw(self) -> tl::enums::ChatAdminRights {
        tl::types::ChatAdminRights {
            change_info: self.change_info,
            post_messages: self.post_messages,
            edit_messages: self.edit_messages,
            delete_messages: self.delete_messages,
            ban_users: self.ban_users,
            invite_users: self.invite_users,
            pin_messages: self.pin_messages,
            add_admins: self.add_admins,
            anonymous: self.anonymous,
            manage_call: self.manage_call,
            // "Manage the chat": every admin has it in the official clients; none at all dismisses.
            other: self != Self::default(),
            manage_topics: self.manage_topics,
            post_stories: self.stories,
            edit_stories: self.stories,
            delete_stories: self.stories,
            manage_direct_messages: false,
            manage_ranks: false,
            manage_linked_peers: false,
            manage_welcome_messages: false,
        }
        .into()
    }
}

/// Banned rights that forbid nothing: lifting a restriction or a ban.
fn no_limits() -> tl::enums::ChatBannedRights {
    ChatRights::from_banned(None).banned_until(0)
}

/// Which people a list shows.
#[derive(Debug, Clone, PartialEq)]
pub enum PartList {
    Admins,
    /// Members with personal restrictions.
    Restricted,
    /// Removed and banned members.
    Removed,
    /// Members found by name, to promote or restrict.
    Search(String),
}

/// One person in a management list.
#[derive(Debug, Clone)]
pub struct Participant {
    pub chat: ChatInfo,
    pub owner: bool,
    /// Admin title.
    pub rank: String,
    /// Admins only.
    pub admin: Option<AdminRights>,
    /// We may change or dismiss this admin.
    pub can_edit: bool,
    /// What a restricted member may still do, and until when (0 — forever).
    pub restricted: Option<(ChatRights, i32)>,
    /// Who promoted, restricted or removed them.
    pub by: String,
}

impl Worker {
    pub(super) async fn manage_cmd(&mut self, cmd: Cmd) -> Result<(), String> {
        let client = self.client()?;
        let err = |e: grammers_client::InvocationError| lf!("Не вдалося змінити учасника: {e}", "Could not change the member: {e}");
        match cmd {
            Cmd::LoadParticipants { peer, list } => {
                let items = if peer.kind() == PeerKind::Channel { self.channel_participants(peer, &list).await? } else { self.chat_participants(peer, &list).await? };
                self.out.send(Ev::Participants { peer, list, items });
            }
            Cmd::EditAdmin { peer, user, rights, rank } => {
                let u: tl::enums::InputUser = self.peer_ref(user)?.into();
                if peer.kind() == PeerKind::Channel {
                    let r = self.peer_ref(peer)?;
                    // No rights at all dismisses the admin.
                    let admin_rights = rights.unwrap_or_default().raw();
                    let rank = rights.map(|_| rank);
                    client.invoke(&tl::functions::channels::EditAdmin { channel: r.into(), user_id: u, admin_rights, rank }).await.map_err(err)?;
                } else {
                    client.invoke(&tl::functions::messages::EditChatAdmin { chat_id: peer.bare_id_unchecked(), user_id: u, is_admin: rights.is_some() }).await.map_err(err)?;
                }
                self.after_member_change(peer);
            }
            Cmd::RestrictMember { peer, user, rights, until } => {
                let r = self.peer_ref(peer)?;
                let participant = self.peer_ref(user)?.into();
                client.invoke(&tl::functions::channels::EditBanned { channel: r.into(), participant, banned_rights: rights.banned_until(until) }).await.map_err(err)?;
                self.after_member_change(peer);
            }
            Cmd::BanMember { peer, user } => {
                if peer.kind() == PeerKind::Channel {
                    let r = self.peer_ref(peer)?;
                    let participant = self.peer_ref(user)?.into();
                    let tl::enums::ChatBannedRights::Rights(mut b) = no_limits();
                    b.view_messages = true;
                    let banned_rights = b.into();
                    client.invoke(&tl::functions::channels::EditBanned { channel: r.into(), participant, banned_rights }).await.map_err(err)?;
                } else {
                    // Basic groups have no ban list: the member is just removed.
                    let req = tl::functions::messages::DeleteChatUser { revoke_history: false, chat_id: peer.bare_id_unchecked(), user_id: self.peer_ref(user)?.into() };
                    client.invoke(&req).await.map_err(err)?;
                }
                self.after_member_change(peer);
            }
            Cmd::UnbanMember { peer, user } => {
                let r = self.peer_ref(peer)?;
                let participant = self.peer_ref(user)?.into();
                let banned_rights = no_limits();
                client.invoke(&tl::functions::channels::EditBanned { channel: r.into(), participant, banned_rights }).await.map_err(err)?;
                self.after_member_change(peer);
            }
            _ => {}
        }
        Ok(())
    }

    fn after_member_change(&self, peer: PeerId) {
        self.out.send(Ev::Notice(l!("Зміни збережено", "Changes saved").into()));
        self.out.send(Ev::ParticipantsChanged(peer));
    }

    async fn channel_participants(&self, peer: PeerId, list: &PartList) -> Result<Vec<Participant>, String> {
        let client = self.client()?;
        let r = self.peer_ref(peer)?;
        let filter: tl::enums::ChannelParticipantsFilter = match list {
            PartList::Admins => tl::types::ChannelParticipantsAdmins {}.into(),
            PartList::Restricted => tl::types::ChannelParticipantsBanned { q: String::new() }.into(),
            PartList::Removed => tl::types::ChannelParticipantsKicked { q: String::new() }.into(),
            PartList::Search(q) if q.is_empty() => tl::types::ChannelParticipantsRecent {}.into(),
            PartList::Search(q) => tl::types::ChannelParticipantsSearch { q: q.clone() }.into(),
        };
        let req = tl::functions::channels::GetParticipants { channel: r.into(), filter, offset: 0, limit: LIST_LIMIT, hash: 0 };
        let res = client.invoke(&req).await.map_err(|e| lf!("Учасники: {e}", "Members: {e}"))?;
        let tl::enums::channels::ChannelParticipants::Participants(res) = res else { return Ok(Vec::new()) };
        let users = self.users_of(res.users).await;
        let name = |id: i64| users.get(&id).map(|c| c.title.clone()).unwrap_or_default();
        let mut items = Vec::new();
        for p in res.participants {
            use tl::enums::ChannelParticipant as P;
            let (id, owner, rank, admin, can_edit, restricted, by) = match p {
                P::Creator(c) => (c.user_id, true, c.rank, Some(AdminRights::from_raw(&c.admin_rights)), false, None, String::new()),
                P::Admin(a) => (a.user_id, false, a.rank, Some(AdminRights::from_raw(&a.admin_rights)), a.can_edit, None, name(a.promoted_by)),
                P::Banned(b) => {
                    let tl::enums::Peer::User(u) = b.peer else { continue };
                    let until = match &b.banned_rights {
                        tl::enums::ChatBannedRights::Rights(x) => x.until_date,
                    };
                    (u.user_id, false, None, None, false, Some((ChatRights::from_banned(Some(&b.banned_rights)), until)), name(b.kicked_by))
                }
                P::Participant(x) => (x.user_id, false, x.rank, None, false, None, String::new()),
                P::ParticipantSelf(x) => (x.user_id, false, x.rank, None, false, None, String::new()),
                P::Left(_) => continue,
            };
            let Some(chat) = users.get(&id).cloned() else { continue };
            items.push(Participant { chat, owner, rank: rank.unwrap_or_default(), admin, can_edit, restricted, by });
        }
        Ok(items)
    }

    /// Basic groups: everyone comes with the full chat; there are no restricted or removed lists.
    async fn chat_participants(&self, peer: PeerId, list: &PartList) -> Result<Vec<Participant>, String> {
        if matches!(list, PartList::Restricted | PartList::Removed) {
            return Ok(Vec::new());
        }
        let client = self.client()?;
        let req = tl::functions::messages::GetFullChat { chat_id: peer.bare_id_unchecked() };
        let tl::enums::messages::ChatFull::Full(f) = client.invoke(&req).await.map_err(|e| lf!("Учасники: {e}", "Members: {e}"))?;
        let tl::enums::ChatFull::Full(full) = f.full_chat else { return Ok(Vec::new()) };
        let tl::enums::ChatParticipants::Participants(parts) = full.participants else { return Ok(Vec::new()) };
        let creator = f.chats.iter().any(|c| matches!(c, tl::enums::Chat::Chat(c) if c.creator && c.id == peer.bare_id_unchecked()));
        let users = self.users_of(f.users).await;
        let q = match list {
            PartList::Search(q) => q.to_lowercase(),
            _ => String::new(),
        };
        let mut items = Vec::new();
        for p in parts.participants {
            use tl::enums::ChatParticipant as P;
            let (id, owner, admin) = match &p {
                P::Creator(c) => (c.user_id, true, true),
                P::Admin(a) => (a.user_id, false, true),
                P::Participant(x) => (x.user_id, false, false),
            };
            if matches!(list, PartList::Admins) && !admin {
                continue;
            }
            let Some(chat) = users.get(&id).cloned() else { continue };
            if !q.is_empty() && !chat.title.to_lowercase().contains(&q) {
                continue;
            }
            // Basic group admins have one fixed set of rights; only the owner changes them.
            let rights = admin.then(|| if owner { AdminRights::all() } else { AdminRights::new_admin() });
            items.push(Participant { chat, owner, rank: String::new(), admin: rights, can_edit: creator && !owner, restricted: None, by: String::new() });
        }
        Ok(items)
    }

    /// Users of an answer as chat rows, remembered so they can be opened and changed.
    async fn users_of(&self, raw: Vec<tl::enums::User>) -> HashMap<i64, ChatInfo> {
        let client = match self.client() {
            Ok(c) => c,
            Err(_) => return HashMap::new(),
        };
        let mut users = HashMap::new();
        for u in raw {
            let p = Peer::User(User::from_raw(&client, u));
            if self.remember(&p).await {
                users.insert(p.id().bare_id_unchecked(), found_chat(&p, String::new()));
            }
        }
        users
    }
}
