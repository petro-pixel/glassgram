//! Stories kept in a profile: the pinned ones (`stories.getPinnedStories`), albums
//! (`getAlbums`, `getAlbumStories`), our own archive (`getStoriesArchive`) and who viewed our
//! stories (`getStoryViewsList`). Read only: nothing here marks stories seen.

use grammers_client::peer::Peer;
use grammers_client::session::types::PeerId;
use grammers_client::{Client, tl};

use super::stories::{peers_by_id, story_of};
use super::{Cmd, Ev, Worker, peer_title};
use crate::i18n::{l, lf};

/// Stories per request, as many as a few screens of the grid.
pub const STORY_PAGE: i32 = 30;

/// Which of a profile's story lists.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum StoryList {
    /// Kept in the profile after the day is over.
    Pinned,
    /// All our expired stories (only our own profile).
    Archive,
    Album(i32),
}

#[derive(Debug, Clone)]
pub struct StoryAlbum {
    pub id: i32,
    pub title: String,
}

/// Someone who saw our story.
#[derive(Debug, Clone)]
pub struct StoryViewer {
    pub peer: PeerId,
    pub name: String,
    /// Unix time they saw it.
    pub date: i64,
    pub reaction: Option<String>,
}

fn reaction_text(r: &tl::enums::Reaction) -> Option<String> {
    match r {
        tl::enums::Reaction::Emoji(e) => Some(e.emoticon.clone()),
        tl::enums::Reaction::Paid => Some("⭐".into()),
        _ => None,
    }
}

impl Worker {
    /// Our own profile may be missing from the known chats: Telegram takes `inputPeerSelf` for it.
    async fn story_peer(&self, client: &Client, peer: PeerId) -> Result<tl::enums::InputPeer, String> {
        if let Ok(r) = self.peer_ref(peer) {
            return Ok(r.into());
        }
        let me = client.get_me().await.map_err(|e| e.to_string())?;
        if me.id() == peer { Ok(tl::enums::InputPeer::PeerSelf) } else { Err(l!("Невідомий чат", "Unknown chat").to_owned()) }
    }

    pub(super) async fn profile_stories_cmd(&mut self, cmd: Cmd) -> Result<(), String> {
        let client = self.client()?;
        let err = |e| lf!("Історії: {e}", "Stories: {e}");
        match cmd {
            Cmd::LoadProfileStories { peer, list, offset } => {
                let input = self.story_peer(&client, peer).await?;
                let res = match list {
                    StoryList::Pinned => client.invoke(&tl::functions::stories::GetPinnedStories { peer: input, offset_id: offset, limit: STORY_PAGE }).await,
                    StoryList::Archive => client.invoke(&tl::functions::stories::GetStoriesArchive { peer: input, offset_id: offset, limit: STORY_PAGE }).await,
                    StoryList::Album(album_id) => client.invoke(&tl::functions::stories::GetAlbumStories { peer: input, album_id, offset, limit: STORY_PAGE }).await,
                };
                let tl::enums::stories::Stories::Stories(res) = res.map_err(err)?;
                let mut stories: Vec<_> = res
                    .stories
                    .iter()
                    .filter_map(|s| match s {
                        tl::enums::StoryItem::Item(x) => Some(story_of(self.loader.as_ref(), peer, x)),
                        _ => None,
                    })
                    .collect();
                // Stories pinned to the top come first, as in the official apps.
                if offset == 0
                    && let Some(top) = &res.pinned_to_top
                {
                    stories.sort_by_key(|s| top.iter().position(|id| *id == s.id).unwrap_or(usize::MAX));
                }
                let next = match list {
                    StoryList::Album(_) => offset + res.stories.len() as i32,
                    _ => stories.iter().map(|s| s.id).min().unwrap_or(0),
                };
                let done = res.stories.len() < STORY_PAGE as usize;
                self.out.send(Ev::ProfileStories { peer, list, stories, total: res.count, next, done, append: offset != 0 });
            }
            Cmd::LoadStoryAlbums(peer) => {
                let input = self.story_peer(&client, peer).await?;
                let albums = match client.invoke(&tl::functions::stories::GetAlbums { peer: input, hash: 0 }).await.map_err(err)? {
                    tl::enums::stories::Albums::Albums(a) => a
                        .albums
                        .into_iter()
                        .map(|tl::enums::StoryAlbum::Album(a)| StoryAlbum { id: a.album_id, title: a.title })
                        .collect(),
                    tl::enums::stories::Albums::NotModified => Vec::new(),
                };
                self.out.send(Ev::StoryAlbums { peer, albums });
            }
            Cmd::LoadStoryViewers { peer, id, offset } => {
                let input = self.story_peer(&client, peer).await?;
                let req = tl::functions::stories::GetStoryViewsList {
                    just_contacts: false,
                    reactions_first: true,
                    forwards_first: false,
                    peer: input,
                    q: None,
                    id,
                    offset: offset.clone(),
                    limit: STORY_PAGE,
                };
                let tl::enums::stories::StoryViewsList::List(res) = client.invoke(&req).await.map_err(err)?;
                let peers = peers_by_id(&client, res.users, res.chats);
                let viewers = res
                    .views
                    .iter()
                    .filter_map(|v| match v {
                        tl::enums::StoryView::View(v) => {
                            let p = peers.get(&PeerId::user_unchecked(v.user_id))?;
                            Some(viewer(&self.loader, p, v.date.into(), v.reaction.as_ref().and_then(reaction_text)))
                        }
                        tl::enums::StoryView::PublicRepost(v) => {
                            let p = peers.get(&PeerId::from(&v.peer_id))?;
                            Some(viewer(&self.loader, p, 0, Some("🔁".into())))
                        }
                        // A forward to a public chat: shown in the counters only.
                        tl::enums::StoryView::PublicForward(_) => None,
                    })
                    .collect();
                self.out.send(Ev::StoryViewers { peer, id, viewers, views: res.views_count, reactions: res.reactions_count, next: res.next_offset.unwrap_or_default(), append: !offset.is_empty() });
            }
            _ => {}
        }
        Ok(())
    }
}

fn viewer(loader: &Option<super::Loader>, p: &Peer, date: i64, reaction: Option<String>) -> StoryViewer {
    if let Some(l) = loader {
        l.avatar(p);
    }
    StoryViewer { peer: p.id(), name: peer_title(p), date, reaction }
}
