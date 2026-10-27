//! Creating groups and channels for the "New chat" window, and the steps after a new channel:
//! its public link and the first subscribers.

use grammers_client::peer::Peer;
use grammers_client::session::types::PeerId;
use grammers_client::{Client, InvocationError, tl};

use super::{Cmd, Ev, UsernameState, Worker, found_chat};
use crate::i18n::{l, lf};

impl Worker {
    pub(super) async fn new_chat_cmd(&mut self, cmd: Cmd) -> Result<(), String> {
        match cmd {
            Cmd::CreateGroup { title, users } => self.create_group(title, users).await,
            Cmd::CreateChannel { title, about } => self.create_channel(title, about).await,
            Cmd::CheckChannelUsername { peer, name } => {
                let client = self.client()?;
                let r = self.peer_ref(peer)?;
                let out = self.out.clone();
                // In the background: the user is still typing.
                tokio::spawn(async move {
                    let state = match client.invoke(&tl::functions::channels::CheckUsername { channel: r.into(), username: name.clone() }).await {
                        Ok(true) => UsernameState::Free,
                        Ok(false) => UsernameState::Taken,
                        Err(InvocationError::Rpc(e)) if e.name == "USERNAME_INVALID" => UsernameState::Invalid,
                        Err(InvocationError::Rpc(e)) if e.name.starts_with("USERNAME_") => UsernameState::Taken,
                        Err(e) => return out.send(Ev::Error(lf!("Посилання: {e}", "Link: {e}"))),
                    };
                    out.send(Ev::ChannelUsernameCheck { peer, name, state });
                });
                Ok(())
            }
            Cmd::SetChannelUsername { peer, name } => {
                let client = self.client()?;
                let r = self.peer_ref(peer)?;
                match client.invoke(&tl::functions::channels::UpdateUsername { channel: r.into(), username: name }).await {
                    // Already private and staying so.
                    Ok(_) => Ok(()),
                    Err(InvocationError::Rpc(e)) if e.name == "USERNAME_NOT_MODIFIED" => Ok(()),
                    Err(InvocationError::Rpc(e)) if e.name == "CHANNELS_ADMIN_PUBLIC_TOO_MUCH" => {
                        Err(l!("Забагато публічних каналів — звільніть посилання в одному з них", "Too many public channels — free a link in one of them").into())
                    }
                    Err(e) => Err(lf!("Посилання: {e}", "Link: {e}")),
                }
            }
            Cmd::InviteToChannel { peer, users } => {
                let client = self.client()?;
                let r = self.peer_ref(peer)?;
                let users = users.into_iter().map(|p| self.peer_ref(p).map(tl::enums::InputUser::from)).collect::<Result<Vec<_>, _>>()?;
                let tl::enums::messages::InvitedUsers::Users(res) = client
                    .invoke(&tl::functions::channels::InviteToChannel { channel: r.into(), users })
                    .await
                    .map_err(|e| lf!("Не вдалося додати: {e}", "Could not add: {e}"))?;
                if !res.missing_invitees.is_empty() {
                    let n = res.missing_invitees.len();
                    self.out.send(Ev::Notice(lf!("Не вдалося додати учасників: {n} — їхня приватність це забороняє", "Could not add {n} members — their privacy settings forbid it")));
                }
                Ok(())
            }
            _ => Ok(()),
        }
    }

    async fn create_group(&mut self, title: String, users: Vec<PeerId>) -> Result<(), String> {
        let client = self.client()?;
        let users = users.into_iter().map(|p| self.peer_ref(p).map(tl::enums::InputUser::from)).collect::<Result<Vec<_>, _>>()?;
        let req = tl::functions::messages::CreateChat { users, title, ttl_period: None };
        let tl::enums::messages::InvitedUsers::Users(res) =
            client.invoke(&req).await.map_err(|e| lf!("Групу не створено: {e}", "Group not created: {e}"))?;
        if !res.missing_invitees.is_empty() {
            let n = res.missing_invitees.len();
            self.out.send(Ev::Notice(lf!("Не вдалося додати учасників: {n} — їхня приватність це забороняє", "Could not add {n} members — their privacy settings forbid it")));
        }
        self.created(&client, res.updates).await
    }

    async fn create_channel(&mut self, title: String, about: String) -> Result<(), String> {
        let client = self.client()?;
        let req = tl::functions::channels::CreateChannel {
            broadcast: true,
            megagroup: false,
            for_import: false,
            forum: false,
            title,
            about,
            geo_point: None,
            address: None,
            ttl_period: None,
        };
        let updates = client.invoke(&req).await.map_err(|e| lf!("Канал не створено: {e}", "Channel not created: {e}"))?;
        self.created(&client, updates).await
    }

    /// The new chat comes in the reply updates: take it from there and open it.
    async fn created(&mut self, client: &Client, updates: tl::enums::Updates) -> Result<(), String> {
        let chats = match updates {
            tl::enums::Updates::Updates(u) => u.chats,
            tl::enums::Updates::Combined(u) => u.chats,
            _ => Vec::new(),
        };
        let peer = chats.into_iter().next().map(|raw| Peer::from_raw(client, raw));
        let Some(peer) = peer else { return self.load_dialogs().await };
        if !self.remember(&peer).await {
            return self.load_dialogs().await;
        }
        let mut info = found_chat(&peer, String::new());
        info.joined = true;
        info.can_post = true;
        info.last_ts = jiff::Timestamp::now().as_second();
        let _ = self.load_dialogs().await;
        self.out.send(Ev::ChatCreated(info));
        Ok(())
    }
}
