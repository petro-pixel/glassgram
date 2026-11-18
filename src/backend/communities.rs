//! Communities (2026): groups, channels and bots joined into one structure. Read only for now:
//! the communities we are in (`communities.getJoinedCommunities`) and the chats each one links
//! (`communityFull.linked_peers`), which open like any chat.

use grammers_client::tl;

use super::stories::peers_by_id;
use super::{ChatInfo, Cmd, Ev, Worker, found_chat};
use crate::i18n::lf;

/// A community we are in.
#[derive(Debug, Clone)]
pub struct Community {
    pub id: i64,
    pub access_hash: i64,
    pub title: String,
}

fn err(e: grammers_client::InvocationError) -> String {
    lf!("Спільноти: {e}", "Communities: {e}")
}

impl Worker {
    pub(super) async fn communities_cmd(&mut self, cmd: Cmd) -> Result<(), String> {
        let client = self.client()?;
        match cmd {
            Cmd::LoadCommunities => {
                let chats = match client.invoke(&tl::functions::communities::GetJoinedCommunities {}).await.map_err(err)? {
                    tl::enums::messages::Chats::Chats(c) => c.chats,
                    tl::enums::messages::Chats::Slice(c) => c.chats,
                };
                let list = chats
                    .into_iter()
                    .filter_map(|c| match c {
                        // Without an access hash it can't be asked about.
                        tl::enums::Chat::Community(c) if !c.left => Some(Community { id: c.id, access_hash: c.access_hash?, title: c.title }),
                        _ => None,
                    })
                    .collect();
                self.out.send(Ev::Communities(list));
            }
            Cmd::LoadCommunityChats { id, access_hash } => {
                let req = tl::functions::channels::GetFullChannel { channel: tl::types::InputChannel { channel_id: id, access_hash }.into() };
                let tl::enums::messages::ChatFull::Full(full) = client.invoke(&req).await.map_err(err)?;
                let tl::enums::ChatFull::CommunityFull(c) = full.full_chat else { return Ok(()) };
                let peers = peers_by_id(&client, full.users, full.chats);
                let mut chats: Vec<ChatInfo> = Vec::new();
                for tl::enums::CommunityPeer::Peer(p) in &c.linked_peers {
                    let Some(peer) = peers.get(&grammers_client::session::types::PeerId::from(&p.peer)) else { continue };
                    // Remembered so that opening it works like any found chat.
                    if self.remember(peer).await {
                        chats.push(found_chat(peer, String::new()));
                    }
                }
                self.out.send(Ev::CommunityChats { id, about: c.about, chats });
            }
            _ => {}
        }
        Ok(())
    }
}
