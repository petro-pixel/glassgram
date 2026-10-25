//! Contacts: adding (by phone or from a profile), deleting, blocking and the blocked list.

use grammers_client::peer::{Peer, User};
use grammers_client::session::types::PeerId;
use grammers_client::tl;

use super::{ChatInfo, Cmd, Ev, Worker, person};
use crate::i18n::{l, lf};

/// How many blocked users the list shows.
const BLOCKED_LIMIT: i32 = 200;

impl Worker {
    pub(super) async fn contacts_cmd(&mut self, cmd: Cmd) -> Result<(), String> {
        let client = self.client()?;
        match cmd {
            Cmd::AddContact { user: Some(peer), first, last, .. } => {
                let r = self.peer_ref(peer)?;
                let req = tl::functions::contacts::AddContact {
                    add_phone_privacy_exception: false,
                    id: r.into(),
                    first_name: first,
                    last_name: last,
                    phone: String::new(),
                    note: None,
                };
                let updates = client.invoke(&req).await.map_err(|e| lf!("Контакт не додано: {e}", "Contact not added: {e}"))?;
                let saved = self.contact_from(&client, updates_users(updates), Some(peer)).await;
                self.out.send(Ev::ContactSaved(saved.ok_or(l!("Контакт не додано", "Contact not added"))?));
            }
            Cmd::AddContact { user: None, phone, first, last } => {
                let contact = tl::types::InputPhoneContact { client_id: 0, phone, first_name: first, last_name: last, note: None };
                let req = tl::functions::contacts::ImportContacts { contacts: vec![contact.into()] };
                let tl::enums::contacts::ImportedContacts::Contacts(res) =
                    client.invoke(&req).await.map_err(|e| lf!("Контакт не додано: {e}", "Contact not added: {e}"))?;
                let saved = self.contact_from(&client, res.users, None).await;
                // The number is not on Telegram (or hides itself from search).
                let saved = saved.ok_or(l!("Цей номер не зареєстрований у Telegram", "This number is not on Telegram"))?;
                self.out.send(Ev::ContactSaved(saved));
            }
            Cmd::DeleteContact(peer) => {
                let r = self.peer_ref(peer)?;
                let req = tl::functions::contacts::DeleteContacts { id: vec![r.into()] };
                client.invoke(&req).await.map_err(|e| lf!("Контакт не видалено: {e}", "Contact not deleted: {e}"))?;
                self.out.send(Ev::ContactRemoved(peer));
            }
            Cmd::Block { peer, blocked } => {
                let r = self.peer_ref(peer)?;
                let res = if blocked {
                    client.invoke(&tl::functions::contacts::Block { my_stories_from: false, id: r.into() }).await
                } else {
                    client.invoke(&tl::functions::contacts::Unblock { my_stories_from: false, id: r.into() }).await
                };
                res.map_err(|e| lf!("Не вдалося змінити блокування: {e}", "Could not change blocking: {e}"))?;
                self.out.send(Ev::Blocked { peer, blocked });
            }
            Cmd::LoadBlocked => {
                let req = tl::functions::contacts::GetBlocked { my_stories_from: false, offset: 0, limit: BLOCKED_LIMIT };
                let (blocked, users, chats) = match client.invoke(&req).await.map_err(|e| lf!("Заблоковані: {e}", "Blocked users: {e}"))? {
                    tl::enums::contacts::Blocked::Blocked(b) => (b.blocked, b.users, b.chats),
                    tl::enums::contacts::Blocked::Slice(b) => (b.blocked, b.users, b.chats),
                };
                let order: Vec<PeerId> = blocked.iter().map(|tl::enums::PeerBlocked::Blocked(b)| PeerId::from(&b.peer_id)).collect();
                let mut list = Vec::new();
                let peers = users.into_iter().map(|u| Peer::User(User::from_raw(&client, u))).chain(chats.into_iter().map(|c| Peer::from_raw(&client, c)));
                for p in peers {
                    if order.contains(&p.id()) && self.remember(&p).await {
                        list.push(person(&p));
                    }
                }
                // Newest blocks first, as the server lists them.
                list.sort_by_key(|c: &ChatInfo| order.iter().position(|p| *p == c.id));
                self.out.send(Ev::BlockedList(list));
            }
            _ => {}
        }
        Ok(())
    }

    /// The saved contact from a reply: the given user, or the first one for an import.
    async fn contact_from(&self, client: &grammers_client::Client, users: Vec<tl::enums::User>, want: Option<PeerId>) -> Option<ChatInfo> {
        for raw in users {
            let peer = Peer::User(User::from_raw(client, raw));
            if want.is_none_or(|w| w == peer.id()) && self.remember(&peer).await {
                return Some(person(&peer));
            }
        }
        None
    }
}

fn updates_users(u: tl::enums::Updates) -> Vec<tl::enums::User> {
    match u {
        tl::enums::Updates::Updates(u) => u.users,
        tl::enums::Updates::Combined(u) => u.users,
        _ => Vec::new(),
    }
}
