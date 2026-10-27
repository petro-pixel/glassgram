//! Sending files (photos are compressed first), downloading, and caching media for playback.

use super::*;

/// Show a just-sent message in the UI (from background tasks too).
#[derive(Clone)]
pub(super) struct Sent {
    pub(super) out: Emitter,
    pub(super) loader: Option<Loader>,
    pub(super) names: Names,
}

/// A reply resolved before a background send: the id for grammers, the id bubbles look up in
/// this chat, and our own request for quotes and other chats.
#[derive(Clone, Default)]
pub(super) struct Reply {
    pub(super) id: Option<i32>,
    pub(super) local: Option<i32>,
    pub(super) extended: Option<tl::enums::InputReplyTo>,
}

impl Sent {
    /// Scheduled messages live in a separate history: we only notify about them.
    pub(super) fn scheduled(&self, opts: SendOpts) {
        if let Some(ts) = opts.schedule {
            let when = jiff::Timestamp::from_second(ts).map(|t| format!("{} {}", day(t), clock(t))).unwrap_or_default();
            self.out.send(Ev::Notice(lf!("Заплановано на {when}", "Scheduled for {when}")));
        }
    }

    pub(super) fn emit(&self, peer: PeerId, m: &Message, reply_to: Option<i32>, opts: SendOpts) {
        self.emit_with(peer, m, reply_to, opts, None);
    }

    /// `own`: the text and entities we sent with; they replace the incomplete ones from the server response.
    pub(super) fn emit_with(&self, peer: PeerId, m: &Message, reply_to: Option<i32>, opts: SendOpts, own: Option<(&str, &[tl::enums::MessageEntity])>) {
        if opts.schedule.is_some() {
            return self.scheduled(opts);
        }
        if let Some(loader) = &self.loader {
            loader.message_photo(peer, m);
        }
        let mut msg = convert(m, &self.names);
        msg.peer = peer;
        msg.out = true;
        msg.reply_to = msg.reply_to.or(reply_to);
        if let Some((plain, ents)) = own
            && msg.text == plain
        {
            msg.spans = format::from_entities(plain, ents, 0);
        }
        self.out.send(Ev::NewMsg(msg));
    }
}

/// A file reader that reports progress to the UI (no more often than every 2%).
pub(super) struct Progress<R> {
    pub(super) inner: R,
    pub(super) id: u64,
    pub(super) done: u64,
    pub(super) total: u64,
    pub(super) reported: u64,
    pub(super) out: Emitter,
}

impl<R: tokio::io::AsyncRead + Unpin> tokio::io::AsyncRead for Progress<R> {
    fn poll_read(
        mut self: std::pin::Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
        buf: &mut tokio::io::ReadBuf<'_>,
    ) -> std::task::Poll<std::io::Result<()>> {
        let before = buf.filled().len();
        let res = std::pin::Pin::new(&mut self.inner).poll_read(cx, buf);
        self.done += (buf.filled().len() - before) as u64;
        if self.done - self.reported >= (self.total / 50).max(1) || self.done == self.total {
            self.reported = self.done;
            self.out.send(Ev::UploadProgress { id: self.id, done: self.done, total: self.total });
        }
        res
    }
}

pub(super) fn is_image(path: &std::path::Path) -> bool {
    let ext = path.extension().map(|e| e.to_string_lossy().to_lowercase()).unwrap_or_default();
    matches!(ext.as_str(), "jpg" | "jpeg" | "png" | "webp")
}

/// Compresses a large photo to `SEND_PHOTO_PX` as JPEG (quality 87). `None`: send as is.
pub(super) fn compress_photo(path: &std::path::Path) -> Option<PathBuf> {
    let len = std::fs::metadata(path).ok()?.len();
    let (w, h) = image::image_dimensions(path).ok()?;
    if w.max(h) <= SEND_PHOTO_PX && len <= 1_500_000 {
        return None;
    }
    let img = image::open(path).ok()?;
    let img = if w.max(h) > SEND_PHOTO_PX {
        img.resize(SEND_PHOTO_PX, SEND_PHOTO_PX, image::imageops::FilterType::Lanczos3)
    } else {
        img
    };
    let dir = data_dir().join("cache");
    std::fs::create_dir_all(&dir).ok()?;
    let stem = path.file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_default();
    let out = dir.join(format!("send-{}-{stem}.jpg", jiff::Timestamp::now().as_millisecond()));
    let file = std::io::BufWriter::new(std::fs::File::create(&out).ok()?);
    image::codecs::jpeg::JpegEncoder::new_with_quality(file, 87).encode_image(&img.to_rgb8()).ok()?;
    Some(out)
}

/// Uploads one file with progress offset by `base` within the `total` of the whole group.
pub(super) async fn upload(client: &Client, path: &std::path::Path, id: u64, base: u64, total: u64, out: &Emitter) -> Result<(Uploaded, u64), String> {
    let name = path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
    let err = |e: &dyn std::fmt::Display| format!("{name}: {e}");
    let file = tokio::fs::File::open(path).await.map_err(|e| err(&e))?;
    let len = file.metadata().await.map_err(|e| err(&e))?.len();
    let mut stream = Progress { inner: file, id, done: base, total: total.max(base + len), reported: base, out: out.clone() };
    let up = client.upload_stream(&mut stream, len as usize, name.clone()).await.map_err(|e| err(&e))?;
    Ok((up, len))
}

/// Sends a group of files: photos one by one or as albums of up to 10 (compressed),
/// the rest as documents. The caption and reply attach to the first message.
#[allow(clippy::too_many_arguments)]
pub(super) async fn send_files(
    client: &Client,
    r: PeerRef,
    id: u64,
    paths: Vec<PathBuf>,
    caption: String,
    reply: Reply,
    opts: SendOpts,
    as_file: bool,
    sent: &Sent,
) -> Result<(), String> {
    let (photos, docs): (Vec<PathBuf>, Vec<PathBuf>) = paths.into_iter().partition(|p| !as_file && is_image(p));
    let photos: Vec<PathBuf> = tokio::task::spawn_blocking(move || {
        photos.into_iter().map(|p| compress_photo(&p).unwrap_or(p)).collect()
    })
    .await
    .map_err(|e| e.to_string())?;
    let total: u64 = photos.iter().chain(&docs).filter_map(|p| std::fs::metadata(p).ok()).map(|m| m.len()).sum();
    let mut base = 0;
    let mut first = Some((caption, reply));
    let emit = |m: &Message, reply: &Reply| sent.emit(r.id, m, reply.local, opts);

    for chunk in photos.chunks(10) {
        let (cap, reply) = first.take().unwrap_or_default();
        let mut ups = Vec::new();
        for path in chunk {
            let (up, len) = upload(client, path, id, base, total, &sent.out).await?;
            base += len;
            ups.push(up);
        }
        if let Some(x) = reply.extended.clone() {
            let msgs = match ups.len() {
                1 => reply::send_one(client, r, &cap, ups.pop().map(reply::photo), x, opts).await?.into_iter().collect(),
                _ => reply::send_album(client, r, ups, &cap, x, opts).await?,
            };
            msgs.iter().for_each(|m| emit(m, &reply));
            if msgs.is_empty() {
                sent.scheduled(opts);
            }
        } else if ups.len() == 1 {
            let Some(up) = ups.pop() else { continue };
            let m = client.send_message(r, opts.apply(rich(&cap).reply_to(reply.id)).photo(up)).await.map_err(|e| e.to_string())?;
            emit(&m, &reply);
        } else {
            let medias = ups.into_iter().enumerate().map(|(i, up)| {
                let media = InputMedia::new().photo(up);
                if i > 0 {
                    return media;
                }
                let (text, entities) = format::parse(&cap);
                media.caption(text).fmt_entities(entities).reply_to(reply.id)
            });
            let msgs = client.send_album(r, medias.collect()).await.map_err(|e| lf!("Альбом: {e}", "Album: {e}"))?;
            msgs.iter().flatten().for_each(|m| emit(m, &reply));
        }
    }
    for path in &docs {
        let (cap, reply) = first.take().unwrap_or_default();
        let (up, len) = upload(client, path, id, base, total, &sent.out).await?;
        base += len;
        let m = match reply.extended.clone() {
            Some(x) => {
                let name = path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
                reply::send_one(client, r, &cap, Some(reply::document(up, &name, None, Vec::new())), x, opts).await?
            }
            None => Some(client.send_message(r, opts.apply(rich(&cap).reply_to(reply.id)).document(up)).await.map_err(|e| e.to_string())?),
        };
        match m {
            Some(m) => emit(&m, &reply),
            None => sent.scheduled(opts),
        }
    }
    Ok(())
}

/// Downloads a message's media into the cache (once) and returns the path.
pub(super) async fn cache_media(client: &Client, r: PeerRef, id: i32) -> Result<PathBuf, String> {
    let dir = data_dir().join("media");
    let _ = tokio::fs::create_dir_all(&dir).await;
    let msg = client
        .get_messages_by_id(r, &[id])
        .await
        .map_err(|e| lf!("Медіа: {e}", "Media: {e}"))?
        .into_iter()
        .flatten()
        .next()
        .ok_or(l!("Повідомлення не знайдено", "Message not found"))?;
    let media = msg.media().ok_or(l!("Немає медіа", "No media"))?;
    let ext = match &media {
        Media::Document(d) => match d.mime_type() {
            Some("video/webm") => "webm",
            Some("audio/ogg") => "ogg",
            Some("audio/mpeg") => "mp3",
            Some("image/gif") => "gif",
            Some(m) if m.starts_with("audio/") => "m4a",
            _ => "mp4",
        },
        Media::Sticker(_) => "webm",
        _ => "bin",
    };
    let path = dir.join(format!("{}_{id}.{ext}", r.id.bot_api_dialog_id_unchecked()));
    if tokio::fs::metadata(&path).await.is_err() {
        let part = path.with_extension("part");
        client.download_media(&media, &part).await.map_err(|e| lf!("Медіа: {e}", "Media: {e}"))?;
        tokio::fs::rename(&part, &path).await.map_err(|e| e.to_string())?;
    }
    Ok(path)
}

/// Downloads a file or photo from a message into Downloads and opens it.
pub(super) async fn download(client: &Client, r: PeerRef, id: i32) -> Result<PathBuf, String> {
    let msg = client
        .get_messages_by_id(r, &[id])
        .await
        .map_err(|e| lf!("Завантаження: {e}", "Download: {e}"))?
        .into_iter()
        .flatten()
        .next()
        .ok_or(l!("Повідомлення не знайдено", "Message not found"))?;
    let media = msg.media().ok_or(l!("У повідомленні немає файлу", "The message has no file"))?;
    let name = match &media {
        // The sender chooses the name: keep it inside Downloads.
        Media::Document(d) => d.name().filter(|n| !n.is_empty()).map(crate::platform::safe_file_name),
        _ => None,
    }
    .unwrap_or_else(|| format!("telegram_{}_{id}.jpg", r.id.bot_api_dialog_id_unchecked()));
    let dir = crate::platform::download_dir();
    let _ = std::fs::create_dir_all(&dir);
    let mut path = dir.join(&name);
    let mut n = 1;
    while path.exists() {
        let stem = std::path::Path::new(&name).file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_default();
        let ext = std::path::Path::new(&name).extension().map(|e| format!(".{}", e.to_string_lossy())).unwrap_or_default();
        path = dir.join(format!("{stem} ({n}){ext}"));
        n += 1;
    }
    client.download_media(&media, &path).await.map_err(|e| lf!("Завантаження: {e}", "Download: {e}"))?;
    Ok(path)
}
