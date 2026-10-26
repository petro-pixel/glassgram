//! Stories: the strip of people and channels with active stories (`stories.getAllStories`),
//! a peer's stories for the viewer (`stories.getPeerStories`) and marking them seen.
//! Photos come in full; a video story shows its cover until the player learns stories.

use std::collections::HashMap;

use grammers_client::media::Photo;
use grammers_client::peer::{Peer, User};
use grammers_client::session::types::PeerId;
use grammers_client::{Client, tl};

use super::{ChatInfo, Cmd, DocLoc, Ev, ImgKey, Loader, Worker, best_thumb, found_chat};
use crate::i18n::lf;

/// Longest side of a story picture, in points: enough for the viewer on a large window.
const STORY_PT: f32 = 900.0;

/// One story in the viewer.
#[derive(Debug, Clone)]
pub struct Story {
    pub id: i32,
    /// Unix time it was posted.
    pub date: i64,
    pub video: bool,
    pub caption: String,
}

/// A person or channel in the strip: the stories, if loaded, and the last one seen.
#[derive(Debug, Clone)]
pub struct StoryPeer {
    pub chat: ChatInfo,
    /// Ids in posting order; `stories` stays empty until the viewer asks for them.
    pub ids: Vec<i32>,
    pub stories: Vec<Story>,
    pub max_read: i32,
}

impl StoryPeer {
    pub fn unread(&self) -> bool {
        self.ids.last().is_some_and(|id| *id > self.max_read)
    }
}

fn peers_by_id(client: &Client, users: Vec<tl::enums::User>, chats: Vec<tl::enums::Chat>) -> HashMap<PeerId, Peer> {
    let users = users.into_iter().map(|u| Peer::User(User::from_raw(client, u)));
    let chats = chats.into_iter().map(|c| Peer::from_raw(client, c));
    users.chain(chats).map(|p| (p.id(), p)).collect()
}

fn item_ids(items: &[tl::enums::StoryItem]) -> Vec<i32> {
    let mut ids: Vec<i32> = items
        .iter()
        .filter_map(|s| match s {
            tl::enums::StoryItem::Item(x) => Some(x.id),
            tl::enums::StoryItem::Skipped(x) => Some(x.id),
            tl::enums::StoryItem::Deleted(_) => None,
        })
        .collect();
    ids.sort_unstable();
    ids
}

impl Loader {
    /// The picture of a story (or a video's cover) as `ImgKey::Story`.
    fn story_media(&self, peer: PeerId, item: &tl::types::StoryItem) {
        let key = ImgKey::Story(peer, item.id);
        let file = format!("s{}_{}.jpg", peer.bot_api_dialog_id_unchecked(), item.id);
        match &item.media {
            tl::enums::MessageMedia::Photo(m) => {
                let Some(photo) = m.photo.clone().map(Photo::from_raw) else { return };
                match best_thumb(&photo) {
                    Some(size) => self.fetch(key, file, STORY_PT, size),
                    None => self.fetch(key, file, STORY_PT, photo),
                }
            }
            tl::enums::MessageMedia::Document(m) => {
                let Some(tl::enums::Document::Document(doc)) = m.document.clone() else { return };
                let thumb = doc.thumbs.iter().flatten().filter_map(|t| match t {
                    tl::enums::PhotoSize::Size(s) => Some((s.w, s.r#type.clone())),
                    tl::enums::PhotoSize::Progressive(s) => Some((s.w, s.r#type.clone())),
                    _ => None,
                }).max_by_key(|t| t.0);
                if let Some((_, thumb)) = thumb {
                    self.fetch(key, file, STORY_PT, DocLoc { doc, thumb });
                }
            }
            _ => {}
        }
    }
}

impl Worker {
    pub(super) async fn stories_cmd(&mut self, cmd: Cmd) -> Result<(), String> {
        let client = self.client()?;
        let err = |e| lf!("Історії: {e}", "Stories: {e}");
        match cmd {
            Cmd::LoadStories => {
                let req = tl::functions::stories::GetAllStories { next: false, hidden: false, state: None };
                let tl::enums::stories::AllStories::Stories(all) = client.invoke(&req).await.map_err(err)? else { return Ok(()) };
                let peers = peers_by_id(&client, all.users, all.chats);
                let mut list = Vec::new();
                for tl::enums::PeerStories::Stories(ps) in all.peer_stories {
                    let Some(peer) = peers.get(&PeerId::from(&ps.peer)) else { continue };
                    if !self.remember(peer).await {
                        continue;
                    }
                    list.push(StoryPeer { chat: found_chat(peer, String::new()), ids: item_ids(&ps.stories), stories: Vec::new(), max_read: ps.max_read_id.unwrap_or(0) });
                }
                self.out.send(Ev::Stories(list));
            }
            Cmd::LoadPeerStories(peer) => {
                let r = self.peer_ref(peer)?;
                let tl::enums::stories::PeerStories::Stories(res) = client.invoke(&tl::functions::stories::GetPeerStories { peer: r.into() }).await.map_err(err)?;
                let tl::enums::PeerStories::Stories(ps) = res.stories;
                let mut stories = Vec::new();
                for item in &ps.stories {
                    // The full list has no skipped items; anything else is gone already.
                    let tl::enums::StoryItem::Item(x) = item else { continue };
                    if let Some(l) = &self.loader {
                        l.story_media(peer, x);
                    }
                    let caption = x.caption.clone().unwrap_or_default();
                    stories.push(Story { id: x.id, date: x.date.into(), video: matches!(x.media, tl::enums::MessageMedia::Document(_)), caption });
                }
                stories.sort_by_key(|s| s.id);
                self.out.send(Ev::PeerStories { peer, stories, max_read: ps.max_read_id.unwrap_or(0) });
            }
            Cmd::ReadStories { peer, max_id } => {
                let r = self.peer_ref(peer)?;
                client.invoke(&tl::functions::stories::ReadStories { peer: r.into(), max_id }).await.map_err(err)?;
            }
            _ => {}
        }
        Ok(())
    }
}
