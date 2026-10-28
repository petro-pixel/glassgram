//! Send queue: a message stays in the cache until the server accepts it.
//! The UI decides when and how often to retry; here it is a single attempt and a verdict.

use std::path::PathBuf;
use std::time::Duration;

use grammers_client::session::types::PeerId;
use grammers_client::{InvocationError, tl};

use grammers_client::Client;
use grammers_client::media::Uploaded;
use grammers_client::message::Message;
use grammers_client::session::types::PeerRef;

use super::reply::{self, SendError};
use super::{Cmd, Ev, Queued, QueuedMedia, ReplyTo, SendOpts, Sent, Worker, compress_photo, is_image, outbox_dir, pack_waveform, upload};
use crate::format;
use crate::i18n::lf;

/// Offline a request can hang for long, and backend commands run one by one — don't hold them up.
const SEND_TIMEOUT: Duration = Duration::from_secs(25);

/// Worth retrying: connection failures and temporary server errors yes, a refusal on the merits no.
pub(super) fn retryable(e: &InvocationError) -> bool {
    match e {
        InvocationError::Rpc(r) => r.code >= 500 || r.code == 420 || r.name == "FLOOD_WAIT",
        _ => true,
    }
}

/// Id of the message a send request created, found by its random_id.
pub(super) fn sent_id(updates: &tl::enums::Updates, random_id: i64) -> Option<i32> {
    let list = match updates {
        tl::enums::Updates::UpdateShortSentMessage(u) => return Some(u.id),
        tl::enums::Updates::Updates(u) => &u.updates,
        tl::enums::Updates::Combined(u) => &u.updates,
        _ => return None,
    };
    list.iter().find_map(|u| match u {
        tl::enums::Update::MessageId(m) if m.random_id == random_id => Some(m.id),
        _ => None,
    })
}

impl Worker {
    /// `[name](tg://user?id=N)` from the @mention popup becomes a real mention of that user:
    /// the server needs the user's access hash for it, which only we have.
    fn with_mentions(&self, ents: Vec<tl::enums::MessageEntity>) -> Vec<tl::enums::MessageEntity> {
        ents.into_iter()
            .map(|e| match &e {
                tl::enums::MessageEntity::TextUrl(u) => {
                    let user = u.url.strip_prefix("tg://user?id=").and_then(|id| id.parse::<i64>().ok());
                    match user.and_then(|id| self.peer_ref(PeerId::user_unchecked(id)).ok()) {
                        Some(r) => tl::types::InputMessageEntityMentionName { offset: u.offset, length: u.length, user_id: r.into() }.into(),
                        None => e,
                    }
                }
                _ => e,
            })
            .collect()
    }

    pub(super) async fn send_queued(&mut self, q: Queued) -> Result<(), String> {
        if let Some(c) = &self.out.cache {
            c.queue_put(&q);
        }
        if q.media.is_some() {
            return self.send_queued_media(q);
        }
        let (key, peer) = (q.key, q.peer);
        let fail = |error: String, retry: bool| Ev::QueueFailed { key, error, retry };
        // There may be no connection or no chat yet (cache before login) — also a reason to retry later.
        let (client, r) = match (self.client(), self.peer_ref(peer)) {
            (Ok(c), Ok(r)) => (c, r),
            (Err(e), _) | (_, Err(e)) => {
                self.out.send(fail(e, true));
                return Ok(());
            }
        };
        let opts = SendOpts { silent: q.silent, no_webpage: q.no_webpage, ..Default::default() };
        let reply_to = match q.reply().map(|x| self.input_reply(&x)).transpose() {
            Ok(x) => x,
            // The other chat is not known in this session yet.
            Err(e) => {
                self.out.send(fail(e, true));
                return Ok(());
            }
        };
        let (plain, ents) = format::parse(&q.text);
        let ents = self.with_mentions(ents);
        // The queue key is the request's random_id on every attempt: if an earlier attempt reached
        // the server but its answer was lost, the server refuses the copy instead of posting it twice.
        let req = tl::functions::messages::SendMessage {
            no_webpage: q.no_webpage,
            silent: q.silent,
            background: false,
            clear_draft: true,
            noforwards: false,
            update_stickersets_order: false,
            invert_media: false,
            allow_paid_floodskip: false,
            peer: r.into(),
            reply_to,
            message: plain.clone(),
            random_id: key,
            reply_markup: None,
            entities: (!ents.is_empty()).then(|| ents.clone()),
            schedule_date: None,
            schedule_repeat_period: None,
            send_as: None,
            quick_reply_shortcut: None,
            effect: None,
            allow_paid_stars: None,
            suggested_post: None,
            rich_message: None,
        };
        let sent = tokio::time::timeout(SEND_TIMEOUT, client.invoke(&req)).await;
        let done = |w: &Worker| {
            if let Some(c) = &w.out.cache {
                c.queue_drop(key);
            }
            w.out.send(Ev::QueueSent { key, peer });
        };
        match sent {
            Ok(Ok(updates)) => {
                // The answer has only the new id: fetch the message itself to show it like any other.
                if let Some(id) = sent_id(&updates, key)
                    && let Ok(Some(m)) = client.get_messages_by_id(r, &[id]).await.map(|l| l.into_iter().flatten().next())
                {
                    self.sent().emit_with(peer, &m, ReplyTo::local(&q.reply()), opts, Some((&plain, &ents)));
                }
                done(self);
            }
            // An earlier attempt got through after all.
            Ok(Err(InvocationError::Rpc(e))) if e.name == "RANDOM_ID_DUPLICATE" => done(self),
            // The quote no longer matches the message (it was edited): send a plain reply instead of failing.
            Ok(Err(InvocationError::Rpc(e))) if e.name.starts_with("QUOTE_") && q.quote.is_some() => {
                return Box::pin(self.send_queued(Queued { quote: None, ..q })).await;
            }
            Ok(Err(e)) => self.out.send(fail(lf!("Не надіслано: {e}", "Not sent: {e}"), retryable(&e))),
            Err(_) => self.out.send(fail(lf!("Не надіслано: немає відповіді за {} с", "Not sent: no response in {} s", SEND_TIMEOUT.as_secs()), true)),
        }
        Ok(())
    }
}

impl Worker {
    /// A queued message with files: uploaded and sent in the background (uploads are slow), as our own
    /// requests whose `random_id`s come from the queue key, so a retry can't post anything twice.
    fn send_queued_media(&mut self, q: Queued) -> Result<(), String> {
        let (key, peer) = (q.key, q.peer);
        let fail = move |error: String, retry: bool| Ev::QueueFailed { key, error, retry };
        let reply = q.reply().map(|x| self.input_reply(&x)).transpose();
        let (client, r, reply) = match (self.client(), self.peer_ref(peer), reply) {
            (Ok(c), Ok(r), Ok(x)) => (c, r, x),
            (Err(e), _, _) | (_, Err(e), _) | (_, _, Err(e)) => {
                self.out.send(fail(e, true));
                return Ok(());
            }
        };
        let sent = self.sent();
        let cache = self.out.cache.clone();
        let dir = outbox_dir(&self.session_path, key);
        tokio::spawn(async move {
            let res = queued_media(&client, r, &q, reply, &sent).await;
            sent.out.send(Ev::UploadDone(key as u64));
            match res {
                Ok(()) => {
                    if let Some(c) = &cache {
                        c.queue_drop(key);
                    }
                    let _ = std::fs::remove_dir_all(&dir);
                    sent.out.send(Ev::QueueSent { key, peer });
                }
                Err(e) => sent.out.send(fail(lf!("Не надіслано: {e}", "Not sent: {e}"), e.retry())),
            }
        });
        Ok(())
    }

    /// Keeping the send queue in the cache.
    pub(super) async fn queue_cmd(&mut self, cmd: Cmd) -> Result<(), String> {
        match cmd {
            Cmd::StoreQueued(q) => {
                if let Some(c) = &self.out.cache {
                    c.queue_put(&q);
                }
                Ok(())
            }
            Cmd::DropQueued(key) => {
                if let Some(c) = &self.out.cache {
                    c.queue_drop(key);
                }
                let _ = std::fs::remove_dir_all(outbox_dir(&self.session_path, key));
                Ok(())
            }
            _ => Ok(()),
        }
    }
}

/// Uploads the files of a queued message and sends them. Message `i` of it gets the `random_id`
/// `key + i`: one the server already has (an earlier attempt got through) counts as sent.
async fn queued_media(client: &Client, r: PeerRef, q: &Queued, reply: Option<tl::enums::InputReplyTo>, sent: &Sent) -> Result<(), SendError> {
    let Some(media) = &q.media else { return Ok(()) };
    if let Some(gone) = media.paths().into_iter().find(|p| !p.exists()) {
        return Err(SendError::Other(lf!("файл зник: {}", "the file is gone: {}", gone.display())));
    }
    let id = q.key as u64;
    let invert = matches!(media, QueuedMedia::Files { above: true, .. });
    let opts = SendOpts { silent: q.silent, no_webpage: true, invert, ..Default::default() };
    let local = ReplyTo::local(&q.reply());
    let total: u64 = media.paths().iter().filter_map(|p| std::fs::metadata(p).ok()).map(|m| m.len()).sum();
    let mut base = 0;
    let mut rid = q.key;
    let mut caption = q.text.clone();
    let mut reply = reply;
    // Only the first message carries the caption and the reply.
    let mut step = |n: usize| -> (i64, String, Option<tl::enums::InputReplyTo>) {
        let out = (rid, std::mem::take(&mut caption), reply.take());
        rid += n as i64;
        out
    };
    let mut up = async |path: &std::path::Path| -> Result<Uploaded, SendError> {
        let (u, len) = upload(client, path, id, base, total, &sent.out).await.map_err(|e| SendError::Rpc(InvocationError::Io(std::io::Error::other(e))))?;
        base += len;
        Ok(u)
    };
    let emit = |m: Option<Message>| {
        if let Some(m) = m {
            sent.emit(r.id, &m, local, opts);
        }
    };
    match media {
        QueuedMedia::Voice { path, duration, waveform } => {
            let attr = tl::types::DocumentAttributeAudio { voice: true, duration: duration.max(1.0).round() as i32, title: None, performer: None, waveform: Some(pack_waveform(waveform)) };
            let doc = reply::document(up(path).await?, "voice.ogg", Some("audio/ogg"), vec![attr.into()]);
            let (rid, cap, reply) = step(1);
            emit(once(reply::send_raw(client, r, &cap, Some(doc), reply, opts, rid).await)?);
        }
        QueuedMedia::Round { path, duration } => {
            let side = crate::camera::SIDE as i32;
            let attr = tl::types::DocumentAttributeVideo { round_message: true, supports_streaming: true, nosound: false, duration: duration.max(1.0), w: side, h: side, preload_prefix_size: None, video_start_ts: None, video_codec: None };
            let doc = reply::document(up(path).await?, "round.mp4", Some("video/mp4"), vec![attr.into()]);
            let (rid, cap, reply) = step(1);
            emit(once(reply::send_raw(client, r, &cap, Some(doc), reply, opts, rid).await)?);
        }
        QueuedMedia::Files { paths, as_file, spoiler, above: _ } => {
            let (photos, docs): (Vec<&PathBuf>, Vec<&PathBuf>) = paths.iter().partition(|p| !as_file && is_image(p));
            let photos: Vec<PathBuf> = photos.into_iter().cloned().collect();
            let photos: Vec<PathBuf> = tokio::task::spawn_blocking(move || photos.into_iter().map(|p| compress_photo(&p).unwrap_or(p)).collect())
                .await
                .map_err(|e| SendError::Other(e.to_string()))?;
            for chunk in photos.chunks(10) {
                let mut ups = Vec::new();
                for p in chunk {
                    ups.push(up(p).await?);
                }
                let (rid, cap, reply) = step(ups.len());
                match ups.len() {
                    1 => emit(once(reply::send_raw(client, r, &cap, ups.pop().map(|u| reply::photo(u, *spoiler)), reply, opts, rid).await)?),
                    _ => match reply::album_raw(client, r, ups, &cap, reply, opts, rid, *spoiler).await {
                        Ok(msgs) => msgs.into_iter().for_each(|m| emit(Some(m))),
                        Err(e) if e.duplicate() => {}
                        Err(e) => return Err(e),
                    },
                }
            }
            for path in docs {
                let name = path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
                let doc = reply::document(up(path).await?, &name, None, Vec::new());
                let doc = if *spoiler { reply::video_spoiler(doc) } else { doc };
                let (rid, cap, reply) = step(1);
                emit(once(reply::send_raw(client, r, &cap, Some(doc), reply, opts, rid).await)?);
            }
        }
    }
    Ok(())
}

/// A message an earlier attempt already delivered is no error, just nothing new to show.
fn once(res: Result<Option<Message>, SendError>) -> Result<Option<Message>, SendError> {
    match res {
        Err(e) if e.duplicate() => Ok(None),
        other => other,
    }
}
