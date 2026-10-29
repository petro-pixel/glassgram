//! Shared folders by link (`chatlists.*`), as in Telegram Desktop: adding a folder from a
//! `t.me/addlist/<slug>` link, adding chats that appeared in it since, and leaving it.

use super::*;

/// What the box offers to do with the chats it lists.
#[derive(Debug, Clone, PartialEq)]
pub enum ChatlistAction {
    /// Add the folder (or the chats we still miss) from an invite link.
    Join(String),
    /// Add chats that the folder's owner added since.
    Update(i32),
    /// Delete the folder, leaving the chats that are ticked.
    Leave(i32),
}

/// A shared folder box: its title and the chats to choose from.
#[derive(Debug, Clone)]
pub struct Chatlist {
    pub action: ChatlistAction,
    pub title: String,
    pub emoticon: String,
    /// (chat, title); avatars come by `ImgKey::Avatar`.
    pub chats: Vec<(PeerId, String)>,
    /// Chats of the folder we already have (an invite to a folder we added before).
    pub already: usize,
}

impl Worker {
    pub(super) async fn chatlists_cmd(&mut self, cmd: Cmd) -> Result<(), String> {
        let client = self.client()?;
        let err = |e: grammers_client::InvocationError| lf!("Спільна папка: {e}", "Shared folder: {e}");
        let input = |id: i32| -> tl::enums::InputChatlist { tl::types::InputChatlistDialogFilter { filter_id: id }.into() };
        match cmd {
            Cmd::CheckChatlist(slug) => {
                let res = client.invoke(&tl::functions::chatlists::CheckChatlistInvite { slug: slug.clone() }).await.map_err(|e| match &e {
                    grammers_client::InvocationError::Rpc(r) if r.name.starts_with("INVITE_SLUG") => l!("Посилання на папку недійсне або застаріло", "The folder link is invalid or expired").to_owned(),
                    _ => err(e),
                })?;
                let list = match res {
                    tl::enums::chatlists::ChatlistInvite::Invite(i) => {
                        let tl::enums::TextWithEntities::Entities(t) = i.title;
                        let chats = self.known_peers(&client, &i.peers, i.chats, i.users).await;
                        Chatlist { action: ChatlistAction::Join(slug), title: t.text, emoticon: i.emoticon.unwrap_or_default(), chats, already: 0 }
                    }
                    tl::enums::chatlists::ChatlistInvite::Already(a) => {
                        let chats = self.known_peers(&client, &a.missing_peers, a.chats, a.users).await;
                        Chatlist { action: ChatlistAction::Join(slug), title: self.folder_title(&client, a.filter_id).await, emoticon: String::new(), chats, already: a.already_peers.len() }
                    }
                };
                self.out.send(Ev::Chatlist(list));
            }
            Cmd::CheckChatlistUpdates(id) => {
                let tl::enums::chatlists::ChatlistUpdates::Updates(u) = client.invoke(&tl::functions::chatlists::GetChatlistUpdates { chatlist: input(id) }).await.map_err(err)?;
                let chats = self.known_peers(&client, &u.missing_peers, u.chats, u.users).await;
                if chats.is_empty() {
                    self.out.send(Ev::Notice(l!("Нових чатів у папці немає", "No new chats in the folder").into()));
                    return Ok(());
                }
                let title = self.folder_title(&client, id).await;
                self.out.send(Ev::Chatlist(Chatlist { action: ChatlistAction::Update(id), title, emoticon: String::new(), chats, already: 0 }));
            }
            Cmd::AskLeaveChatlist(id) => {
                // The server suggests the chats that came with the folder and nothing else holds.
                let peers = client.invoke(&tl::functions::chatlists::GetLeaveChatlistSuggestions { chatlist: input(id) }).await.map_err(err)?;
                let names = self.names.lock().unwrap().clone();
                let chats = peers.iter().map(raw_peer_id).map(|id| (id, names.get(&id).cloned().unwrap_or_default())).collect();
                let title = self.folder_title(&client, id).await;
                self.out.send(Ev::Chatlist(Chatlist { action: ChatlistAction::Leave(id), title, emoticon: String::new(), chats, already: 0 }));
            }
            Cmd::ApplyChatlist { action, peers } => {
                let peers = peers.into_iter().map(|p| self.peer_ref(p).map(tl::enums::InputPeer::from)).collect::<Result<Vec<_>, _>>()?;
                match action {
                    ChatlistAction::Join(slug) => {
                        client.invoke(&tl::functions::chatlists::JoinChatlistInvite { slug, peers }).await.map_err(err)?;
                    }
                    ChatlistAction::Update(id) => {
                        client.invoke(&tl::functions::chatlists::JoinChatlistUpdates { chatlist: input(id), peers }).await.map_err(err)?;
                    }
                    ChatlistAction::Leave(id) => {
                        client.invoke(&tl::functions::chatlists::LeaveChatlist { chatlist: input(id), peers }).await.map_err(err)?;
                    }
                }
                self.load_folders().await?;
                self.load_dialogs().await?;
            }
            _ => {}
        }
        Ok(())
    }

    /// The listed peers as chat rows; their refs are kept, so joining them works.
    async fn known_peers(&self, client: &Client, peers: &[tl::enums::Peer], chats: Vec<tl::enums::Chat>, users: Vec<tl::enums::User>) -> Vec<(PeerId, String)> {
        let mut all: HashMap<PeerId, Peer> = chats.into_iter().map(|c| Peer::from_raw(client, c)).map(|p| (p.id(), p)).collect();
        all.extend(users.into_iter().map(|u| Peer::User(grammers_client::peer::User::from_raw(client, u))).map(|p| (p.id(), p)));
        let mut out = Vec::new();
        for id in peers.iter().map(raw_peer_id) {
            if let Some(p) = all.get(&id)
                && self.remember(p).await
            {
                if let Some(loader) = &self.loader {
                    loader.avatar(p);
                }
                out.push((id, peer_title(p)));
            }
        }
        out
    }

    async fn folder_title(&self, client: &Client, id: i32) -> String {
        let Ok(tl::enums::messages::DialogFilters::Filters(f)) = client.invoke(&tl::functions::messages::GetDialogFilters {}).await else { return String::new() };
        f.filters
            .into_iter()
            .find_map(|f| match f {
                tl::enums::DialogFilter::Chatlist(c) if c.id == id => {
                    let tl::enums::TextWithEntities::Entities(t) = c.title;
                    Some(t.text)
                }
                _ => None,
            })
            .unwrap_or_default()
    }
}

fn raw_peer_id(p: &tl::enums::Peer) -> PeerId {
    match p {
        tl::enums::Peer::User(u) => PeerId::user_unchecked(u.user_id),
        tl::enums::Peer::Chat(c) => PeerId::chat_unchecked(c.chat_id),
        tl::enums::Peer::Channel(c) => PeerId::channel_unchecked(c.channel_id),
    }
}
