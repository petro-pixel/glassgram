//! Saved Messages split by where the messages came from (`messages.getSavedDialogs`,
//! `messages.getSavedHistory`), as the source tabs of Saved Messages in Telegram Desktop, and by
//! reaction tags (`messages.getSavedReactionTags`, `messages.search` with `saved_reaction`).

use std::collections::HashMap;

use grammers_client::peer::{Peer, User};
use grammers_client::session::types::PeerId;
use grammers_client::tl;

use super::{Cmd, Ev, Msg, PAGE, Worker, found_chat};
use crate::i18n::{l, lf};

/// A reaction used as a tag in Saved Messages, with its optional name (Premium) and use count.
#[derive(Debug, Clone, PartialEq)]
pub struct SavedTag {
    pub emoji: String,
    pub title: String,
    pub count: i32,
}

/// How many tagged messages one filter loads: tags are for finding, not for paging through.
const TAGGED_LIMIT: i32 = 100;

/// The chat a message in Saved Messages was saved from.
pub(super) fn saved_from(raw: &tl::enums::Message) -> Option<PeerId> {
    match raw {
        tl::enums::Message::Message(x) => x.saved_peer_id.as_ref().map(PeerId::from),
        _ => None,
    }
}

impl Worker {
    pub(super) async fn saved_cmd(&mut self, cmd: Cmd) -> Result<(), String> {
        let client = self.client()?;
        match cmd {
            Cmd::LoadSavedDialogs => {
                let req = tl::functions::messages::GetSavedDialogs {
                    exclude_pinned: false,
                    parent_peer: None,
                    offset_date: 0,
                    offset_id: 0,
                    offset_peer: tl::enums::InputPeer::Empty,
                    limit: 100,
                    hash: 0,
                };
                let res = client.invoke(&req).await.map_err(|e| lf!("Збережене: {e}", "Saved Messages: {e}"))?;
                let (dialogs, chats, users) = match res {
                    tl::enums::messages::SavedDialogs::Dialogs(d) => (d.dialogs, d.chats, d.users),
                    tl::enums::messages::SavedDialogs::Slice(d) => (d.dialogs, d.chats, d.users),
                    tl::enums::messages::SavedDialogs::NotModified(_) => return Ok(()),
                };
                let mut peers: HashMap<PeerId, Peer> = HashMap::new();
                for p in users.into_iter().map(|u| Peer::User(User::from_raw(&client, u))) {
                    peers.insert(p.id(), p);
                }
                for p in chats.into_iter().map(|c| Peer::from_raw(&client, c)) {
                    peers.insert(p.id(), p);
                }
                // In the server's order: pinned first, then by the newest message.
                let mut list = Vec::new();
                for d in dialogs {
                    let Some(peer) = peers.get(&PeerId::from(&d.peer())) else { continue };
                    if self.remember(peer).await {
                        list.push(found_chat(peer, String::new()));
                    }
                }
                self.out.send(Ev::SavedDialogs(list));
            }
            Cmd::LoadSavedHistory { saved, source, before } => {
                let r = self.peer_ref(source)?;
                let req = tl::functions::messages::GetSavedHistory {
                    parent_peer: None,
                    peer: r.into(),
                    offset_id: before.unwrap_or(0),
                    offset_date: 0,
                    add_offset: 0,
                    limit: PAGE as i32,
                    max_id: 0,
                    min_id: 0,
                    hash: 0,
                };
                let raw = client.invoke(&req).await.map_err(|e| lf!("Збережене: {e}", "Saved Messages: {e}"))?;
                let list = match raw {
                    tl::enums::messages::Messages::Messages(m) => m.messages,
                    tl::enums::messages::Messages::Slice(m) => m.messages,
                    tl::enums::messages::Messages::ChannelMessages(m) => m.messages,
                    tl::enums::messages::Messages::NotModified(_) => Vec::new(),
                };
                let ids: Vec<i32> = list.iter().map(|m| m.id()).collect();
                // As with topics: the regular request gives messages we can convert.
                let mut msgs: Vec<Msg> = if ids.is_empty() {
                    Vec::new()
                } else {
                    let own = self.peer_ref(saved)?;
                    client
                        .get_messages_by_id(own, &ids)
                        .await
                        .map_err(|e| lf!("Збережене: {e}", "Saved Messages: {e}"))?
                        .into_iter()
                        .flatten()
                        .map(|m| self.convert_loaded(saved, &m))
                        .collect()
                };
                msgs.sort_by_key(|m| m.id);
                self.out.send(Ev::SavedHistory { source, msgs, older: before.is_some() });
            }
            Cmd::LoadSavedTags => {
                let req = tl::functions::messages::GetSavedReactionTags { peer: None, hash: 0 };
                let res = client.invoke(&req).await.map_err(|e| lf!("Теги: {e}", "Tags: {e}"))?;
                let tl::enums::messages::SavedReactionTags::Tags(t) = res else { return Ok(()) };
                let tags = t
                    .tags
                    .into_iter()
                    .filter_map(|tl::enums::SavedReactionTag::Tag(t)| match t.reaction {
                        // Custom emoji tags have no picture yet: left out rather than shown blank.
                        tl::enums::Reaction::Emoji(e) => Some(SavedTag { emoji: e.emoticon, title: t.title.unwrap_or_default(), count: t.count }),
                        _ => None,
                    })
                    .collect();
                self.out.send(Ev::SavedTags(tags));
            }
            Cmd::LoadSavedTagged { saved, emoji } => {
                let own = self.peer_ref(saved)?;
                let req = tl::functions::messages::Search {
                    peer: own.into(),
                    q: String::new(),
                    from_id: None,
                    saved_peer_id: None,
                    saved_reaction: Some(vec![tl::types::ReactionEmoji { emoticon: emoji.clone() }.into()]),
                    top_msg_id: None,
                    filter: tl::enums::MessagesFilter::InputMessagesFilterEmpty,
                    min_date: 0,
                    max_date: 0,
                    offset_id: 0,
                    add_offset: 0,
                    limit: TAGGED_LIMIT,
                    max_id: 0,
                    min_id: 0,
                    hash: 0,
                };
                let err = |e| lf!("Теги: {e}", "Tags: {e}");
                let ids: Vec<i32> = match client.invoke(&req).await.map_err(err)? {
                    tl::enums::messages::Messages::Messages(m) => m.messages,
                    tl::enums::messages::Messages::Slice(m) => m.messages,
                    tl::enums::messages::Messages::ChannelMessages(m) => m.messages,
                    tl::enums::messages::Messages::NotModified(_) => Vec::new(),
                }
                .iter()
                .map(|m| m.id())
                .collect();
                let mut msgs: Vec<Msg> = if ids.is_empty() {
                    Vec::new()
                } else {
                    client.get_messages_by_id(own, &ids).await.map_err(err)?.into_iter().flatten().map(|m| self.convert_loaded(saved, &m)).collect()
                };
                msgs.sort_by_key(|m| m.id);
                self.out.send(Ev::SavedTagged(msgs));
            }
            Cmd::RenameSavedTag { emoji, title } => {
                let req = tl::functions::messages::UpdateSavedReactionTag {
                    reaction: tl::types::ReactionEmoji { emoticon: emoji }.into(),
                    title: Some(title).filter(|t| !t.is_empty()),
                };
                if let Err(e) = client.invoke(&req).await {
                    let e = if e.is("PREMIUM_ACCOUNT_REQUIRED") { l!("Назви тегів — функція Telegram Premium", "Tag names need Telegram Premium").to_owned() } else { e.to_string() };
                    self.out.send(Ev::Error(e));
                }
                // The server's names either way, so a refused rename snaps back.
                return Box::pin(self.saved_cmd(Cmd::LoadSavedTags)).await;
            }
            _ => {}
        }
        Ok(())
    }
}
