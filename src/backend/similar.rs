//! Similar channels in a channel's profile (`channels.getChannelRecommendations`), as the
//! "Similar channels" tab in Telegram Desktop.

use grammers_client::peer::Peer;
use grammers_client::session::types::PeerId;
use grammers_client::tl;

use super::{Ev, Worker, found_chat};
use crate::i18n::lf;

impl Worker {
    pub(super) async fn load_similar(&self, peer: PeerId) -> Result<(), String> {
        let client = self.client()?;
        let r = self.peer_ref(peer)?;
        let req = tl::functions::channels::GetChannelRecommendations { channel: Some(r.into()) };
        let res = client.invoke(&req).await.map_err(|e| lf!("Схожі канали: {e}", "Similar channels: {e}"))?;
        // Without Premium the server sends a few and counts the rest.
        let (raw, total) = match res {
            tl::enums::messages::Chats::Chats(c) => {
                let n = c.chats.len() as i32;
                (c.chats, n)
            }
            tl::enums::messages::Chats::Slice(c) => (c.chats, c.count),
        };
        let mut chats = Vec::new();
        for raw in raw {
            let p = Peer::from_raw(&client, raw);
            if !self.remember(&p).await {
                continue;
            }
            let label = match &p {
                Peer::Channel(c) => c.raw.participants_count.map(|n| lf!("підписників: {n}", "{n} subscribers")),
                _ => None,
            };
            chats.push(found_chat(&p, label.unwrap_or_default()));
        }
        let total = total.max(chats.len() as i32);
        self.out.send(Ev::SimilarChannels { peer, chats, total });
        Ok(())
    }
}
