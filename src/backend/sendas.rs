//! Sending as a channel, as in Telegram Desktop: who a supergroup or channel lets us post as
//! (`channels.getSendAs`), the default from the full chat (`default_send_as`), and choosing it
//! (`messages.saveDefaultSendAs`). Send requests take the choice as their `send_as`.

use grammers_client::peer::{Peer, User};
use grammers_client::session::types::{PeerId, PeerKind, PeerRef};
use grammers_client::tl;

use super::{Cmd, Ev, Worker, log_error, peer_title};
use crate::i18n::{l, lf, lp};

/// Someone a message can be sent as: ourselves or a channel we own or run.
#[derive(Debug, Clone, PartialEq)]
pub struct SendAsPeer {
    pub id: PeerId,
    pub title: String,
    /// "personal account", or how many subscribers or members.
    pub about: String,
    /// Can be chosen only with Telegram Premium.
    pub premium: bool,
}

impl SendAsPeer {
    /// Our own account rather than a channel.
    pub fn is_me(&self) -> bool {
        self.id.kind() == PeerKind::User
    }
}

fn describe(peer: &Peer) -> (String, String) {
    let count = |n: Option<i32>, broadcast: bool| match (n, broadcast) {
        (Some(n), true) => lp!(n, ["{n} підписник", "{n} підписники", "{n} підписників"], ["{n} subscriber", "{n} subscribers"]),
        (Some(n), false) => lp!(n, ["{n} учасник", "{n} учасники", "{n} учасників"], ["{n} member", "{n} members"]),
        (None, true) => l!("канал", "channel").to_owned(),
        (None, false) => l!("група", "group").to_owned(),
    };
    match peer {
        // Not "Saved Messages": here it is our name.
        Peer::User(u) => (u.full_name(), l!("особистий акаунт", "personal account").to_owned()),
        Peer::Channel(c) => (peer_title(peer), count(c.raw.participants_count, true)),
        Peer::Group(g) => {
            let n = match &g.raw {
                tl::enums::Chat::Channel(c) => c.participants_count,
                _ => None,
            };
            (peer_title(peer), count(n, false))
        }
        other => (peer_title(other), String::new()),
    }
}

impl Worker {
    pub(super) async fn send_as_cmd(&mut self, cmd: Cmd) -> Result<(), String> {
        match cmd {
            Cmd::LoadSendAs(peer) => self.load_send_as(peer).await,
            Cmd::SaveSendAs { peer, send_as } => {
                let client = self.client()?;
                let send_as = self.send_as_ref(Some(send_as))?.unwrap_or(tl::enums::InputPeer::PeerSelf);
                let req = tl::functions::messages::SaveDefaultSendAs { peer: self.peer_ref(peer)?.into(), send_as };
                client.invoke(&req).await.map(drop).map_err(|e| lf!("Не вдалося змінити, від чийого імені надсилати: {e}", "Could not change who to send as: {e}"))
            }
            _ => Ok(()),
        }
    }

    async fn load_send_as(&self, peer: PeerId) -> Result<(), String> {
        // Basic groups and private chats can't post as anyone else.
        if peer.kind() != PeerKind::Channel {
            return Ok(());
        }
        let client = self.client()?;
        let r = self.peer_ref(peer)?;
        let req = tl::functions::channels::GetSendAs { for_paid_reactions: false, for_live_stories: false, peer: r.into() };
        // Most chats simply have nothing to offer: not worth an error on screen.
        let res = match client.invoke(&req).await {
            Ok(tl::enums::channels::SendAsPeers::Peers(res)) => res,
            Err(e) => {
                log_error(&format!("getSendAs: {e}"));
                return Ok(());
            }
        };
        let mut known = Vec::new();
        for raw in res.users {
            known.push(Peer::User(User::from_raw(&client, raw)));
        }
        for raw in res.chats {
            known.push(Peer::from_raw(&client, raw));
        }
        let mut peers = Vec::new();
        for tl::enums::SendAsPeer::Peer(p) in res.peers {
            let id = PeerId::from(&p.peer);
            let Some(found) = known.iter().find(|k| k.id() == id) else { continue };
            if !self.remember(found).await {
                continue;
            }
            let (title, about) = describe(found);
            peers.push(SendAsPeer { id, title, about, premium: p.premium_required });
        }
        let current = self.default_send_as(r).await.filter(|id| peers.iter().any(|p| p.id == *id)).or(peers.first().map(|p| p.id));
        self.out.send(Ev::SendAs { peer, peers, current });
        Ok(())
    }

    /// The chat's saved choice; none yet means the first one offered.
    async fn default_send_as(&self, r: PeerRef) -> Option<PeerId> {
        let client = self.client().ok()?;
        let res = client.invoke(&tl::functions::channels::GetFullChannel { channel: r.into() }).await.ok()?;
        let tl::enums::messages::ChatFull::Full(full) = res;
        let tl::enums::ChatFull::ChannelFull(f) = full.full_chat else { return None };
        f.default_send_as.as_ref().map(PeerId::from)
    }

    /// `send_as` for a request. The only account offered is our own; a channel must be one this
    /// session knows, like the chat itself.
    pub(super) fn send_as_ref(&self, send_as: Option<PeerId>) -> Result<Option<tl::enums::InputPeer>, String> {
        match send_as {
            None => Ok(None),
            Some(p) if p.kind() == PeerKind::User => Ok(Some(tl::enums::InputPeer::PeerSelf)),
            Some(p) => self.peer_ref(p).map(|r| Some(r.into())),
        }
    }
}
