//! Music for the player: a chat's audio files for the queue (`messages.search` with the music
//! filter) and the music a user saved to their profile (`users.getSavedMusic`, 2026).

use super::*;

/// Tracks per request: enough for a playlist, one request for most chats.
const MUSIC_PAGE: usize = 100;

impl Worker {
    pub(super) async fn music_cmd(&mut self, cmd: Cmd) -> Result<(), String> {
        let client = self.client()?;
        match cmd {
            Cmd::LoadMusic(peer) => {
                let r = self.peer_ref(peer)?;
                let (names, out) = (self.names.clone(), self.out.clone());
                tokio::spawn(async move {
                    let mut iter = client.search_messages(r).filter(tl::enums::MessagesFilter::InputMessagesFilterMusic).limit(MUSIC_PAGE);
                    let mut msgs = Vec::new();
                    loop {
                        match iter.next().await {
                            Ok(Some(m)) => {
                                remember_name(&names, &m);
                                let mut msg = convert(&m, &names);
                                msg.peer = peer;
                                msgs.push(msg);
                            }
                            Ok(None) => break,
                            // The queue still has the loaded history; a failed search only shortens it.
                            Err(e) => {
                                log_error(&format!("music search: {e}"));
                                break;
                            }
                        }
                    }
                    msgs.reverse();
                    out.send(Ev::Music { peer, msgs });
                });
            }
            Cmd::LoadSavedMusic(peer) => {
                let id: tl::enums::InputUser = self.peer_ref(peer)?.into();
                let out = self.out.clone();
                tokio::spawn(async move {
                    let req = tl::functions::users::GetSavedMusic { id, offset: 0, limit: MUSIC_PAGE as i32, hash: 0 };
                    match client.invoke(&req).await {
                        Ok(tl::enums::users::SavedMusic::Music(m)) => {
                            let tracks: Vec<MediaInfo> = m.documents.iter().filter_map(track_info).collect();
                            out.send(Ev::SavedMusic { peer, total: m.count.max(tracks.len() as i32), tracks });
                        }
                        Ok(tl::enums::users::SavedMusic::NotModified(_)) => {}
                        // Most profiles have no music: not worth a toast.
                        Err(e) => log_error(&format!("saved music: {e}")),
                    }
                });
            }
            Cmd::LoadSavedTrack { peer, index } => {
                let id: tl::enums::InputUser = self.peer_ref(peer)?.into();
                let out = self.out.clone();
                tokio::spawn(async move {
                    match saved_track(&client, peer, id, index).await {
                        Ok(path) => out.send(Ev::MediaReady { peer, id: -1 - index, path }),
                        Err(e) => out.send(Ev::Error(e)),
                    }
                });
            }
            _ => {}
        }
        Ok(())
    }
}

/// Fetches the track again rather than keeping documents: file references expire anyway.
async fn saved_track(client: &Client, peer: PeerId, user: tl::enums::InputUser, index: i32) -> Result<PathBuf, String> {
    let err = |e: &dyn std::fmt::Display| lf!("Музика: {e}", "Music: {e}");
    let req = tl::functions::users::GetSavedMusic { id: user, offset: index, limit: 1, hash: 0 };
    let doc = match client.invoke(&req).await.map_err(|e| err(&e))? {
        tl::enums::users::SavedMusic::Music(m) => m.documents.into_iter().next(),
        tl::enums::users::SavedMusic::NotModified(_) => None,
    };
    let Some(doc @ tl::enums::Document::Document(_)) = doc else { return Err(err(&l!("трек не знайдено", "track not found"))) };
    let raw = tl::types::MessageMediaDocument {
        nopremium: false,
        spoiler: false,
        video: false,
        round: false,
        voice: false,
        document: Some(doc),
        alt_documents: None,
        video_cover: None,
        video_timestamp: None,
        ttl_seconds: None,
    };
    let media = Media::Document(grammers_client::media::Document::from_raw_media(raw));
    let path = upload::media_path(peer, -1 - index, &media);
    if tokio::fs::metadata(&path).await.is_err() {
        let _ = tokio::fs::create_dir_all(data_dir().join("media")).await;
        let part = path.with_extension("part");
        client.download_media(&media, &part).await.map_err(|e| err(&e))?;
        tokio::fs::rename(&part, &path).await.map_err(|e| e.to_string())?;
    }
    Ok(path)
}

/// Title, performer and duration of a profile track.
fn track_info(doc: &tl::enums::Document) -> Option<MediaInfo> {
    let tl::enums::Document::Document(d) = doc else { return None };
    let mut info = MediaInfo { kind: MediaKind::Audio, duration: 0.0, size: (0, 0), title: String::new(), performer: String::new(), waveform: Vec::new(), bytes: d.size.max(0) as u64 };
    for a in &d.attributes {
        match a {
            tl::enums::DocumentAttribute::Audio(a) => {
                info.duration = a.duration as f64;
                info.title = a.title.clone().filter(|t| !t.is_empty()).unwrap_or(info.title);
                info.performer = a.performer.clone().unwrap_or_default();
            }
            tl::enums::DocumentAttribute::Filename(f) if info.title.is_empty() => info.title = f.file_name.clone(),
            _ => {}
        }
    }
    Some(info)
}
