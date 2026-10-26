//! Opening chats and paging through their history, typing status.

use super::*;

impl Worker {
    /// Opening chats and paging through their history, typing status.
    pub(super) async fn history_cmd(&mut self, cmd: Cmd) -> Result<(), String> {
        match cmd {
            Cmd::OpenChat(peer) => {
                // Cache first: the chat opens instantly and offline; the server replaces the page later.
                if let Some(c) = &self.out.cache {
                    let msgs = c.history(peer, None, PAGE).await;
                    if !msgs.is_empty() {
                        self.out.replay(Ev::History { peer, msgs, older: false });
                    }
                }
                let client = self.client()?;
                let r = self.peer_ref(peer)?;
                let msgs = self.fetch_history(&client, r, None).await?;
                self.out.send(Ev::History { peer, msgs, older: false });
                let _ = client.mark_as_read(r).await;
                Ok(())
            }
            Cmd::LoadOlder { peer, before } => {
                let fetched = match (self.client(), self.peer_ref(peer)) {
                    (Ok(client), Ok(r)) => self.fetch_history(&client, r, Some(before)).await,
                    (Err(e), _) | (_, Err(e)) => Err(e),
                };
                match fetched {
                    Ok(msgs) => self.out.send(Ev::History { peer, msgs, older: true }),
                    // Offline, we scroll through what is in the cache.
                    Err(e) => {
                        let cached = match &self.out.cache {
                            Some(c) => c.history(peer, Some(before), PAGE).await,
                            None => Vec::new(),
                        };
                        if cached.is_empty() {
                            self.out.send(Ev::History { peer, msgs: Vec::new(), older: true });
                            return Err(e);
                        }
                        self.out.replay(Ev::History { peer, msgs: cached, older: true });
                    }
                }
                Ok(())
            }
            Cmd::OpenAt { peer, id } => {
                let client = self.client()?;
                let r = self.peer_ref(peer)?;
                let mut iter = client.iter_messages(r).limit(JUMP_LIMIT);
                let mut msgs = Vec::new();
                while let Some(m) = iter.next().await.map_err(|e| lf!("Історія: {e}", "History: {e}"))? {
                    let reached = m.id() <= id;
                    msgs.push(self.convert_loaded(r.id, &m));
                    if reached && msgs.len() >= PAGE {
                        break;
                    }
                }
                msgs.reverse();
                self.out.send(Ev::History { peer, msgs, older: false });
                Ok(())
            }
            Cmd::Typing(peer) => {
                let client = self.client()?;
                let r = self.peer_ref(peer)?;
                tokio::spawn(async move {
                    let _ = client.action(r).oneshot(tl::types::SendMessageTypingAction {}).await;
                });
                Ok(())
            }
            _ => Ok(()),
        }
    }
}
