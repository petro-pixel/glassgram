//! Media on demand: full-size photos, files to play or save, sticker thumbnails, listened marks.

use super::*;

impl Worker {
    /// Media on demand: full-size photos, files to play or save, sticker thumbnails, listened marks.
    pub(super) async fn media_cmd(&mut self, cmd: Cmd) -> Result<(), String> {
        match cmd {
            Cmd::MarkListened { peer, id } => {
                let client = self.client()?;
                let r = self.peer_ref(peer)?;
                let res = if peer.kind() == grammers_client::session::types::PeerKind::Channel {
                    client.invoke(&tl::functions::channels::ReadMessageContents { channel: r.into(), id: vec![id] }).await.map(drop)
                } else {
                    client.invoke(&tl::functions::messages::ReadMessageContents { id: vec![id] }).await.map(drop)
                };
                res.map_err(|e| lf!("Не вдалося позначити прослуханим: {e}", "Could not mark as listened: {e}"))
            }
            Cmd::LoadStickerThumbs(ids) => {
                if let Some(loader) = &self.loader {
                    for id in ids {
                        if let Some(d) = self.docs.get(&id) {
                            loader.sticker(d);
                        }
                    }
                }
                Ok(())
            }
            Cmd::LoadFullPhoto { peer, id } => {
                let client = self.client()?;
                let r = self.peer_ref(peer)?;
                let loader = self.loader.clone().ok_or(l!("Немає з'єднання", "No connection"))?;
                tokio::spawn(async move {
                    if let Ok(msgs) = client.get_messages_by_id(r, &[id]).await
                        && let Some(Some(m)) = msgs.into_iter().next()
                        && let Some(Media::Photo(photo)) = m.media()
                    {
                        let file = format!("f{}_{id}.jpg", peer.bot_api_dialog_id_unchecked());
                        loader.fetch(ImgKey::Full(peer, id), file, FULL_PT, photo);
                    }
                });
                Ok(())
            }
            Cmd::LoadMedia { peer, id } => {
                let client = self.client()?;
                let r = self.peer_ref(peer)?;
                let out = self.out.clone();
                tokio::spawn(async move {
                    match cache_media(&client, r, id).await {
                        Ok(path) => out.send(Ev::MediaReady { peer, id, path }),
                        Err(e) => out.send(Ev::Error(e)),
                    }
                });
                Ok(())
            }
            Cmd::Download { peer, id } => {
                let client = self.client()?;
                let r = self.peer_ref(peer)?;
                let out = self.out.clone();
                tokio::spawn(async move {
                    let res = download(&client, r, id).await;
                    out.send(match res {
                        Ok(path) => Ev::Notice(lf!("Збережено: {}", "Saved: {}", path.display())),
                        Err(e) => Ev::Error(e),
                    });
                });
                Ok(())
            }
            _ => Ok(()),
        }
    }
}
