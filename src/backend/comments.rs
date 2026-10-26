//! Channel post comments from the linked discussion group.

use super::*;

impl Worker {
    pub(super) async fn comments_cmd(&mut self, cmd: Cmd) -> Result<(), String> {
        match cmd {
            Cmd::OpenComments { peer, id, before } => {
                let client = self.client()?;
                let r = self.peer_ref(peer)?;
                let tl::enums::messages::DiscussionMessage::Message(d) = client
                    .invoke(&tl::functions::messages::GetDiscussionMessage { peer: r.into(), msg_id: id })
                    .await
                    .map_err(|e| lf!("Коментарі: {e}", "Comments: {e}"))?;
                let (discussion, root) = d
                    .messages
                    .iter()
                    .find_map(|m| match m {
                        tl::enums::Message::Message(x) => Some((PeerId::from(&x.peer_id), x.id)),
                        _ => None,
                    })
                    .ok_or(l!("Коментарі: немає групи обговорення", "Comments: no discussion group"))?;
                // The discussion group reference is needed to read and write comments.
                for chat in d.chats {
                    let p = Peer::from_raw(&client, chat);
                    if p.id() == discussion
                        && let Ok(Some(rf)) = p.to_ref().await
                    {
                        self.refs.lock().unwrap().insert(discussion, rf);
                        self.names.lock().unwrap().insert(discussion, peer_title(&p));
                    }
                }
                let disc = self.peer_ref(discussion)?;
                let req = tl::functions::messages::GetReplies { peer: r.into(), msg_id: id, offset_id: before.unwrap_or(0), offset_date: 0, add_offset: 0, limit: PAGE as i32, max_id: 0, min_id: 0, hash: 0 };
                let ids: Vec<i32> = match client.invoke(&req).await.map_err(|e| lf!("Коментарі: {e}", "Comments: {e}"))? {
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
                    client
                        .get_messages_by_id(disc, &ids)
                        .await
                        .map_err(|e| lf!("Коментарі: {e}", "Comments: {e}"))?
                        .into_iter()
                        .flatten()
                        .map(|m| self.convert_loaded(discussion, &m))
                        .collect()
                };
                msgs.sort_by_key(|m| m.id);
                self.out.send(Ev::Comments { peer, post: id, discussion, root, msgs, older: before.is_some() });
                Ok(())
            }
            _ => Ok(()),
        }
    }
}
