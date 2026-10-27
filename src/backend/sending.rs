//! Sending and changing messages: text, stickers, forwarding, editing, deleting, reactions, votes.
//! Files, voice and round videos go through the send queue (`outbox.rs`).

use super::*;

impl Worker {
    pub(super) async fn send_cmd(&mut self, cmd: Cmd) -> Result<(), String> {
        match cmd {
            Cmd::Send { peer, text, reply_to, opts } => {
                let client = self.client()?;
                let r = self.peer_ref(peer)?;
                let not_sent = |e: &dyn std::fmt::Display| lf!("Не надіслано: {e}", "Not sent: {e}");
                let m = match self.extended_reply(&reply_to)? {
                    Some(reply) => reply::send_one(&client, r, &text, None, reply, opts).await.map_err(|e| not_sent(&e))?,
                    None => Some(client.send_message(r, opts.apply(rich(&text).reply_to(reply_to.as_ref().map(|x| x.id)))).await.map_err(|e| not_sent(&e))?),
                };
                // In private chats the server replies with a short UpdateShortSentMessage that has only some
                // of the entities (no `code`, etc.). We know our own entities exactly, so we use them.
                let (plain, ents) = format::parse(&text);
                match m {
                    Some(m) => self.sent().emit_with(peer, &m, ReplyTo::local(&reply_to), opts, Some((&plain, &ents))),
                    None => self.sent().scheduled(opts),
                }
                Ok(())
            }
            Cmd::Forward { from, ids, to } => {
                let client = self.client()?;
                let (src, dst) = (self.peer_ref(from)?, self.peer_ref(to)?);
                let sent = client.forward_messages(dst, &ids, src).await.map_err(|e| lf!("Не переслано: {e}", "Not forwarded: {e}"))?;
                let emitter = self.sent();
                for m in sent.iter().flatten() {
                    emitter.emit(to, m, None, SendOpts::default());
                }
                let title = self.names.lock().unwrap().get(&to).cloned().unwrap_or_default();
                self.out.send(Ev::Notice(lf!("Переслано: {} → {title}", "Forwarded: {} → {title}", ids.len())));
                Ok(())
            }
            Cmd::Edit { peer, id, text } => {
                let client = self.client()?;
                let r = self.peer_ref(peer)?;
                client
                    // Otherwise grammers drops the link preview on every edit.
                    .edit_message(r, id, rich(&text).link_preview(true))
                    .await
                    .map_err(|e| lf!("Не вдалося змінити: {e}", "Could not edit: {e}"))
            }
            Cmd::Delete { peer, ids } => {
                let client = self.client()?;
                let r = self.peer_ref(peer)?;
                client
                    .delete_messages(r, &ids)
                    .await
                    .map_err(|e| lf!("Не вдалося видалити: {e}", "Could not delete: {e}"))?;
                self.out.send(Ev::Deleted { ids, peer: Some(peer) });
                Ok(())
            }
            Cmd::SendSticker { peer, doc, reply_to } => {
                let client = self.client()?;
                let r = self.peer_ref(peer)?;
                let d = self.docs.get(&doc).ok_or(l!("Стікер не знайдено", "Sticker not found"))?;
                let media = tl::types::InputMediaDocument {
                    spoiler: false,
                    id: tl::types::InputDocument { id: d.id, access_hash: d.access_hash, file_reference: d.file_reference.clone() }.into(),
                    video_cover: None,
                    video_timestamp: None,
                    ttl_seconds: None,
                    query: None,
                };
                let not_sent = |e: &dyn std::fmt::Display| lf!("Стікер не надіслано: {e}", "Sticker not sent: {e}");
                let m = match self.extended_reply(&reply_to)? {
                    Some(x) => reply::send_one(&client, r, "", Some(media.into()), x, SendOpts::default()).await.map_err(|e| not_sent(&e))?,
                    None => Some(client.send_message(r, InputMessage::new().media(media).reply_to(reply_to.as_ref().map(|x| x.id))).await.map_err(|e| not_sent(&e))?),
                };
                if let Some(m) = m {
                    self.sent().emit(peer, &m, ReplyTo::local(&reply_to), SendOpts::default());
                }
                Ok(())
            }
            Cmd::React { peer, id, emoji } => {
                let client = self.client()?;
                let r = self.peer_ref(peer)?;
                let reaction = match emoji {
                    Some(e) => InputReactions::emoticon(e),
                    None => InputReactions::remove(),
                };
                client.send_reactions(r, id, reaction).await.map_err(|e| lf!("Реакція: {e}", "Reaction: {e}"))
            }
            Cmd::Vote { peer, id, options } => {
                let client = self.client()?;
                let r = self.peer_ref(peer)?;
                client
                    .invoke(&tl::functions::messages::SendVote { peer: r.into(), msg_id: id, options })
                    .await
                    .map_err(|e| lf!("Голосування: {e}", "Vote: {e}"))?;
                // Updated results are taken from the message itself.
                if let Ok(msgs) = client.get_messages_by_id(r, &[id]).await
                    && let Some(Some(m)) = msgs.into_iter().next()
                {
                    let mut msg = convert(&m, &self.names);
                    msg.peer = peer;
                    self.out.send(Ev::Edited(msg));
                }
                Ok(())
            }
            _ => Ok(()),
        }
    }
}
