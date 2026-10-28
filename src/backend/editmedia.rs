//! Replacing the photo or file of a sent message (`messages.editMessage` with `media`).
//! Caption-only edits of media messages go through `Cmd::Edit`, which keeps the media.

use super::*;

impl Worker {
    pub(super) async fn edit_media_cmd(&mut self, cmd: Cmd) -> Result<(), String> {
        let Cmd::EditMedia { peer, id, text, path, as_file, spoiler, above } = cmd else { return Ok(()) };
        let client = self.client()?;
        let r = self.peer_ref(peer)?;
        let (out, loader, names) = (self.out.clone(), self.loader.clone(), self.names.clone());
        // Uploading takes a while: other commands go on meanwhile.
        tokio::spawn(async move {
            let res = replace(&client, r, id, &text, &path, as_file, spoiler, above, &out).await;
            let m = match res {
                Ok(m) => m,
                Err(e) => return out.send(Ev::Error(lf!("Не вдалося замінити: {e}", "Could not replace: {e}"))),
            };
            // The cached pictures and files belong to the old media.
            forget(peer, id, loader.as_ref());
            if let Some(l) = &loader {
                l.message_photo(peer, &m);
            }
            let mut msg = convert(&m, &names);
            msg.peer = peer;
            out.send(Ev::MediaReplaced(msg));
        });
        Ok(())
    }
}

#[allow(clippy::too_many_arguments)]
async fn replace(client: &Client, r: PeerRef, id: i32, text: &str, path: &std::path::Path, as_file: bool, spoiler: bool, above: bool, out: &Emitter) -> Result<Message, String> {
    let photo = !as_file && is_image(path);
    let file = if photo {
        let p = path.to_path_buf();
        tokio::task::spawn_blocking(move || compress_photo(&p).unwrap_or(p)).await.map_err(|e| e.to_string())?
    } else {
        path.to_path_buf()
    };
    let total = std::fs::metadata(&file).map(|m| m.len()).map_err(|e| e.to_string())?;
    // Progress for an id no upload pill waits on is simply dropped by the UI.
    let (up, _) = upload(client, &file, 0, 0, total, out).await?;
    let media = if photo {
        reply::photo(up, spoiler)
    } else {
        let name = path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
        let doc = reply::document(up, &name, None, Vec::new());
        if spoiler { reply::video_spoiler(doc) } else { doc }
    };
    let (message, ents) = format::parse(text);
    client
        .invoke(&tl::functions::messages::EditMessage {
            no_webpage: false,
            invert_media: above,
            peer: r.into(),
            id,
            message: Some(message),
            media: Some(media),
            reply_markup: None,
            entities: Some(ents),
            schedule_date: None,
            schedule_repeat_period: None,
            quick_reply_shortcut_id: None,
            rich_message: None,
        })
        .await
        .map_err(|e| e.to_string())?;
    client.get_messages_by_id(r, &[id]).await.map_err(|e| e.to_string())?.into_iter().flatten().next().ok_or_else(|| l!("Повідомлення не знайдено", "Message not found").to_owned())
}

/// Drops the cache files named after the message (thumbnail, full photo, played media).
fn forget(peer: PeerId, id: i32, loader: Option<&Loader>) {
    let dlg = peer.bot_api_dialog_id_unchecked();
    let cache = data_dir().join("cache");
    for file in [format!("p{dlg}_{id}.jpg"), format!("v{dlg}_{id}.jpg"), format!("f{dlg}_{id}.jpg")] {
        let _ = std::fs::remove_file(cache.join(&file));
        if let Some(l) = loader {
            l.requested.lock().unwrap().remove(&file);
        }
    }
    if let Some(l) = loader {
        l.files.lock().unwrap().retain(|k, _| !matches!(k, ImgKey::Photo(p, i) | ImgKey::Full(p, i) if *p == peer && *i == id));
    }
    let prefix = format!("{dlg}_{id}.");
    if let Ok(dir) = std::fs::read_dir(data_dir().join("media")) {
        for e in dir.flatten() {
            if e.file_name().to_string_lossy().starts_with(&prefix) {
                let _ = std::fs::remove_file(e.path());
            }
        }
    }
}
