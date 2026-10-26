//! Gifts shown in a profile (`payments.getSavedStarGifts`), as the "Gifts" tab in Telegram Desktop.
//! Only showing them: sending, converting and transferring gifts spend Stars and are not here.

use std::collections::HashMap;

use grammers_client::peer::{Peer, User};
use grammers_client::session::types::PeerId;
use grammers_client::tl;

use super::{Cmd, Ev, Worker, peer_title};
use crate::i18n::lf;

/// How many gifts the tab loads.
const GIFTS_LIMIT: i32 = 60;

/// One gift in a profile.
#[derive(Debug, Clone)]
pub struct Gift {
    /// A collectible's name and number ("Plush Pepe #123"); empty for a regular gift.
    pub title: String,
    /// What a regular gift cost; 0 for collectibles.
    pub stars: i64,
    /// The sticker that shows the gift (`ImgKey::Sticker`).
    pub sticker: Option<i64>,
    /// Who sent it; empty when the sender is hidden.
    pub from: String,
    /// The sender's note.
    pub note: String,
    pub date: i64,
}

impl Worker {
    pub(super) async fn gifts_cmd(&mut self, cmd: Cmd) -> Result<(), String> {
        let Cmd::LoadGifts(peer) = cmd else { return Ok(()) };
        let client = self.client()?;
        let r = self.peer_ref(peer)?;
        let (out, loader) = (self.out.clone(), self.loader.clone());
        tokio::spawn(async move {
            let req = tl::functions::payments::GetSavedStarGifts {
                exclude_unsaved: false,
                exclude_saved: false,
                exclude_unlimited: false,
                exclude_unique: false,
                sort_by_value: false,
                exclude_upgradable: false,
                exclude_unupgradable: false,
                peer_color_available: false,
                exclude_hosted: false,
                peer: r.into(),
                collection_id: None,
                offset: String::new(),
                limit: GIFTS_LIMIT,
            };
            let tl::enums::payments::SavedStarGifts::Gifts(res) = match client.invoke(&req).await {
                Ok(res) => res,
                Err(e) => return out.send(Ev::Error(lf!("Подарунки: {e}", "Gifts: {e}"))),
            };
            let users = res.users.into_iter().map(|u| Peer::User(User::from_raw(&client, u)));
            let chats = res.chats.into_iter().map(|c| Peer::from_raw(&client, c));
            let names: HashMap<PeerId, String> = users.chain(chats).map(|p| (p.id(), peer_title(&p))).collect();
            let mut gifts = Vec::new();
            for tl::enums::SavedStarGift::Gift(g) in res.gifts {
                let (title, stars, doc) = match g.gift {
                    tl::enums::StarGift::Gift(x) => (String::new(), x.stars, Some(x.sticker)),
                    tl::enums::StarGift::Unique(x) => {
                        let model = x.attributes.into_iter().find_map(|a| match a {
                            tl::enums::StarGiftAttribute::Model(m) => Some(m.document),
                            _ => None,
                        });
                        (format!("{} #{}", x.title, x.num), 0, model)
                    }
                };
                let sticker = match doc {
                    Some(tl::enums::Document::Document(d)) => {
                        if let Some(l) = &loader {
                            l.sticker(&d);
                        }
                        Some(d.id)
                    }
                    _ => None,
                };
                let from = g.from_id.as_ref().and_then(|p| names.get(&PeerId::from(p))).cloned().unwrap_or_default();
                let note = g.message.map(|tl::enums::TextWithEntities::Entities(t)| t.text).unwrap_or_default();
                gifts.push(Gift { title, stars, sticker, from, note, date: g.date.into() });
            }
            out.send(Ev::Gifts { peer, gifts, total: res.count });
        });
        Ok(())
    }
}
