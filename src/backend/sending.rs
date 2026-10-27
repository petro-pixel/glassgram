//! Sending and changing messages: text, stickers, files, voice and round videos, forwarding, editing, deleting, reactions, votes.

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
            Cmd::SendFiles { id, peer, paths, caption, reply_to, opts, as_file } => {
                let (client, r, extended) = match (self.client(), self.peer_ref(peer), self.extended_reply(&reply_to)) {
                    (Ok(c), Ok(r), Ok(x)) => (c, r, x),
                    (Err(e), _, _) | (_, Err(e), _) | (_, _, Err(e)) => {
                        self.out.send(Ev::UploadDone(id));
                        return Err(e);
                    }
                };
                let reply_to = Reply { id: reply_to.as_ref().map(|x| x.id), local: ReplyTo::local(&reply_to), extended };
                let sent = self.sent();
                // Upload runs in the background so long files don't block other commands.
                tokio::spawn(async move {
                    let res = send_files(&client, r, id, paths, caption, reply_to, opts, as_file, &sent).await;
                    sent.out.send(Ev::UploadDone(id));
                    if let Err(e) = res {
                        sent.out.send(Ev::Error(e));
                    }
                });
                Ok(())
            }
            Cmd::SendVoice { id, peer, path, duration, waveform, reply_to } => self.send_recording(id, peer, path, reply_to, Recording::Voice { duration, waveform }),
            Cmd::SendRound { id, peer, path, duration, reply_to } => self.send_recording(id, peer, path, reply_to, Recording::Round { duration }),
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

    /// A recorded voice or round video message: uploaded in the background, the file deleted afterwards.
    fn send_recording(&self, id: u64, peer: PeerId, path: PathBuf, reply_to: Option<ReplyTo>, rec: Recording) -> Result<(), String> {
        let (client, r, extended) = match (self.client(), self.peer_ref(peer), self.extended_reply(&reply_to)) {
            (Ok(c), Ok(r), Ok(x)) => (c, r, x),
            (Err(e), _, _) | (_, Err(e), _) | (_, _, Err(e)) => {
                self.out.send(Ev::UploadDone(id));
                return Err(e);
            }
        };
        let reply = Reply { id: reply_to.as_ref().map(|x| x.id), local: ReplyTo::local(&reply_to), extended };
        let sent = self.sent();
        tokio::spawn(async move {
            let res = async {
                let total = std::fs::metadata(&path).map(|m| m.len()).unwrap_or(0);
                let (up, _) = upload(&client, &path, id, 0, total, &sent.out).await?;
                let side = crate::camera::SIDE as i32;
                let voice = matches!(rec, Recording::Voice { .. });
                let (mime, name) = if voice { ("audio/ogg", "voice.ogg") } else { ("video/mp4", "round.mp4") };
                let not_sent = |e: &dyn std::fmt::Display| {
                    if voice { lf!("Голосове не надіслано: {e}", "Voice message not sent: {e}") } else { lf!("Відеоповідомлення не надіслано: {e}", "Video message not sent: {e}") }
                };
                let m = match (reply.extended, rec) {
                    (Some(x), rec) => {
                        let attr: tl::enums::DocumentAttribute = match rec {
                            Recording::Voice { duration, waveform } => {
                                tl::types::DocumentAttributeAudio { voice: true, duration: duration.max(1.0).round() as i32, title: None, performer: None, waveform: Some(pack_waveform(&waveform)) }.into()
                            }
                            Recording::Round { duration } => tl::types::DocumentAttributeVideo {
                                round_message: true,
                                supports_streaming: true,
                                nosound: false,
                                duration: duration.max(1.0),
                                w: side,
                                h: side,
                                preload_prefix_size: None,
                                video_start_ts: None,
                                video_codec: None,
                            }
                            .into(),
                        };
                        reply::send_one(&client, r, "", Some(reply::document(up, name, Some(mime), vec![attr])), x, SendOpts::default()).await.map_err(|e| not_sent(&e))?
                    }
                    (None, rec) => {
                        let attr = match rec {
                            Recording::Voice { duration, waveform } => Attribute::Voice { duration: Duration::from_secs_f64(duration.max(1.0)), waveform: Some(pack_waveform(&waveform)) },
                            Recording::Round { duration } => Attribute::Video { round_message: true, supports_streaming: true, duration: Duration::from_secs_f64(duration.max(1.0)), w: side, h: side },
                        };
                        let msg = InputMessage::new().reply_to(reply.id).mime_type(mime).document(up).attribute(attr);
                        Some(client.send_message(r, msg).await.map_err(|e| not_sent(&e))?)
                    }
                };
                if let Some(m) = m {
                    sent.emit(r.id, &m, reply.local, SendOpts::default());
                }
                Ok::<_, String>(())
            }
            .await;
            sent.out.send(Ev::UploadDone(id));
            let _ = std::fs::remove_file(&path);
            if let Err(e) = res {
                sent.out.send(Ev::Error(e));
            }
        });
        Ok(())
    }
}

/// What `send_recording` sends.
enum Recording {
    Voice { duration: f64, waveform: Vec<u8> },
    Round { duration: f64 },
}
