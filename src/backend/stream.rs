//! Streaming large videos, as Telegram Desktop does: instead of downloading the whole file
//! first, a local HTTP server (127.0.0.1 only, an unguessable token per video) serves byte
//! ranges to `ffmpeg` / `ffplay` and fetches the 512 KB parts they ask for from Telegram on
//! demand, so playback starts at once and seeking jumps ahead. Fetched parts land in a sparse
//! file; once every part is there it becomes the ordinary cached file (`Ev::MediaReady`).

use std::io::SeekFrom;
use std::sync::OnceLock;
use std::sync::atomic::{AtomicBool, Ordering};

use grammers_client::client::DownloadIter;
use tokio::io::{AsyncReadExt, AsyncSeekExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};

use super::*;

/// Videos smaller than this download whole, as before: streaming them gains nothing.
const STREAM_MIN_BYTES: u64 = 10 << 20;
/// Telegram's largest part per request.
const CHUNK: u64 = 512 * 1024;
/// A request header bigger than this is not from ffmpeg.
const MAX_HEADER: usize = 16 * 1024;

/// What a video to watch turned out to be.
enum Got {
    File(PathBuf),
    Stream(String),
}

struct Stream {
    client: Client,
    doc: tl::types::Document,
    size: u64,
    mime: String,
    /// The sparse file being filled, and where it goes when complete.
    part: PathBuf,
    path: PathBuf,
    have: Mutex<Vec<bool>>,
    /// One fetch at a time per video: ffmpeg (picture) and ffplay (sound) read it together.
    fetch: tokio::sync::Mutex<()>,
    complete: AtomicBool,
    peer: PeerId,
    id: i32,
    out: Emitter,
}

struct Server {
    port: u16,
    streams: Mutex<HashMap<String, Arc<Stream>>>,
}

static SERVER: OnceLock<Server> = OnceLock::new();

/// Starts the server on first use; parts left by an earlier run are removed then.
async fn server() -> Result<&'static Server, String> {
    if let Some(s) = SERVER.get() {
        return Ok(s);
    }
    if let Ok(mut dir) = tokio::fs::read_dir(data_dir().join("media")).await {
        while let Ok(Some(e)) = dir.next_entry().await {
            if e.path().extension().is_some_and(|x| x == "stream") {
                let _ = tokio::fs::remove_file(e.path()).await;
            }
        }
    }
    let listener = TcpListener::bind("127.0.0.1:0").await.map_err(|e| e.to_string())?;
    let port = listener.local_addr().map_err(|e| e.to_string())?.port();
    if SERVER.set(Server { port, streams: Mutex::new(HashMap::new()) }).is_err() {
        // Another command started it meanwhile; this listener just closes.
        return SERVER.get().ok_or_else(|| "stream server".to_owned());
    }
    tokio::spawn(async move {
        while let Ok((sock, _)) = listener.accept().await {
            tokio::spawn(serve(sock));
        }
    });
    SERVER.get().ok_or_else(|| "stream server".to_owned())
}

fn token() -> Result<String, String> {
    let mut buf = [0u8; 16];
    getrandom::fill(&mut buf).map_err(|e| e.to_string())?;
    Ok(buf.iter().map(|b| format!("{b:02x}")).collect())
}

/// `GET /<token>` (or `HEAD`) and the start and end of `Range: bytes=a-b`.
fn parse_request(head: &str) -> Option<(bool, String, Option<(u64, Option<u64>)>)> {
    let mut lines = head.split("\r\n");
    let mut first = lines.next()?.split(' ');
    let head_only = match first.next()? {
        "GET" => false,
        "HEAD" => true,
        _ => return None,
    };
    let token = first.next()?.strip_prefix('/')?.to_owned();
    let range = lines.filter_map(|l| l.split_once(':')).find(|(k, _)| k.trim().eq_ignore_ascii_case("range")).and_then(|(_, v)| {
        let (a, b) = v.trim().strip_prefix("bytes=")?.split_once('-')?;
        Some((a.trim().parse().ok()?, b.trim().parse().ok()))
    });
    Some((head_only, token, range))
}

async fn serve(mut sock: TcpStream) {
    let mut buf = Vec::new();
    let mut tmp = [0u8; 2048];
    while !buf.windows(4).any(|w| w == b"\r\n\r\n") {
        match sock.read(&mut tmp).await {
            Ok(0) | Err(_) => return,
            Ok(n) => buf.extend_from_slice(&tmp[..n]),
        }
        if buf.len() > MAX_HEADER {
            return;
        }
    }
    let head = String::from_utf8_lossy(&buf).into_owned();
    let stream = parse_request(&head).and_then(|(h, token, range)| {
        let s = SERVER.get()?.streams.lock().unwrap().get(&token).cloned()?;
        Some((h, s, range))
    });
    let Some((head_only, s, range)) = stream else {
        let _ = sock.write_all(b"HTTP/1.1 404 Not Found\r\nContent-Length: 0\r\nConnection: close\r\n\r\n").await;
        return;
    };
    let (start, end) = match range {
        Some((a, b)) => (a, b.unwrap_or(s.size - 1).min(s.size - 1)),
        None => (0, s.size - 1),
    };
    if start >= s.size || start > end {
        let reply = format!("HTTP/1.1 416 Range Not Satisfiable\r\nContent-Range: bytes */{}\r\nContent-Length: 0\r\nConnection: close\r\n\r\n", s.size);
        let _ = sock.write_all(reply.as_bytes()).await;
        return;
    }
    let status = if range.is_some() { format!("206 Partial Content\r\nContent-Range: bytes {start}-{end}/{}", s.size) } else { "200 OK".to_owned() };
    let reply = format!("HTTP/1.1 {status}\r\nContent-Type: {}\r\nContent-Length: {}\r\nAccept-Ranges: bytes\r\nConnection: close\r\n\r\n", s.mime, end - start + 1);
    if sock.write_all(reply.as_bytes()).await.is_err() || head_only {
        return;
    }
    let mut iter = None;
    let mut pos = start;
    while pos <= end {
        let idx = pos / CHUNK;
        if let Err(e) = s.ensure(idx, &mut iter).await {
            log_error(&format!("stream: {e}"));
            return;
        }
        let upto = ((idx + 1) * CHUNK).min(end + 1);
        let Ok(data) = s.read(pos, upto - pos).await else { return };
        if sock.write_all(&data).await.is_err() {
            // ffmpeg seeks by dropping the connection: nothing more to send.
            return;
        }
        pos = upto;
    }
}

impl Stream {
    /// Part `idx` is in the file, fetched now if not. `iter` continues where the last fetch on
    /// this connection stopped, so reading on doesn't start a new download each time.
    async fn ensure(&self, idx: u64, iter: &mut Option<(u64, DownloadIter)>) -> Result<(), String> {
        if self.have.lock().unwrap().get(idx as usize).copied().unwrap_or(true) {
            return Ok(());
        }
        let _one = self.fetch.lock().await;
        if self.have.lock().unwrap()[idx as usize] {
            return Ok(());
        }
        if iter.as_ref().is_none_or(|(next, _)| *next != idx) {
            let it = self.client.iter_download(&DocLoc { doc: self.doc.clone(), thumb: String::new() }).chunk_size(CHUNK as i32).skip_chunks(idx as i32);
            *iter = Some((idx, it));
        }
        let Some((next, it)) = iter.as_mut() else { return Ok(()) };
        let bytes = it.next().await.map_err(|e| e.to_string())?.unwrap_or_default();
        *next = idx + 1;
        let mut f = tokio::fs::OpenOptions::new().write(true).open(&self.part).await.map_err(|e| e.to_string())?;
        f.seek(SeekFrom::Start(idx * CHUNK)).await.map_err(|e| e.to_string())?;
        f.write_all(&bytes).await.map_err(|e| e.to_string())?;
        f.flush().await.map_err(|e| e.to_string())?;
        drop(f);
        let all = {
            let mut have = self.have.lock().unwrap();
            have[idx as usize] = true;
            have.iter().all(|h| *h)
        };
        if all && tokio::fs::rename(&self.part, &self.path).await.is_ok() {
            self.complete.store(true, Ordering::Relaxed);
            self.out.send(Ev::MediaReady { peer: self.peer, id: self.id, path: self.path.clone() });
        }
        Ok(())
    }

    async fn read(&self, at: u64, len: u64) -> std::io::Result<Vec<u8>> {
        let file = if self.complete.load(Ordering::Relaxed) { &self.path } else { &self.part };
        let mut f = tokio::fs::File::open(file).await?;
        f.seek(SeekFrom::Start(at)).await?;
        let mut data = vec![0; len as usize];
        f.read_exact(&mut data).await?;
        Ok(data)
    }
}

impl Worker {
    /// A video to watch: the cached file if there is one, the whole file if it's small,
    /// otherwise a stream URL (`Ev::MediaStream`).
    pub(super) async fn stream_cmd(&mut self, cmd: Cmd) -> Result<(), String> {
        let Cmd::StreamMedia { peer, id } = cmd else { return Ok(()) };
        let client = self.client()?;
        let r = self.peer_ref(peer)?;
        let out = self.out.clone();
        tokio::spawn(async move {
            let res = async {
                let msg = client
                    .get_messages_by_id(r, &[id])
                    .await
                    .map_err(|e| lf!("Медіа: {e}", "Media: {e}"))?
                    .into_iter()
                    .flatten()
                    .next()
                    .ok_or(l!("Повідомлення не знайдено", "Message not found"))?;
                let media = msg.media().ok_or(l!("Немає медіа", "No media"))?;
                let path = media_path(peer, id, &media);
                let doc = raw_document(&msg).filter(|d| d.size as u64 >= STREAM_MIN_BYTES);
                let Some(doc) = doc.filter(|_| std::fs::metadata(&path).is_err()) else {
                    return cache_media(&client, r, id).await.map(Got::File);
                };
                let srv = server().await?;
                let key = format!("{}_{id}", peer.bot_api_dialog_id_unchecked());
                let mut streams = srv.streams.lock().unwrap();
                if let Some((token, _)) = streams.iter().find(|(_, s)| s.peer == peer && s.id == id) {
                    return Ok(Got::Stream(format!("http://127.0.0.1:{}/{token}", srv.port)));
                }
                let size = doc.size as u64;
                let part = data_dir().join("media").join(format!("{key}.stream"));
                std::fs::create_dir_all(data_dir().join("media")).map_err(|e| e.to_string())?;
                std::fs::File::create(&part).and_then(|f| f.set_len(size)).map_err(|e| e.to_string())?;
                let token = token()?;
                let parts = size.div_ceil(CHUNK) as usize;
                let mime = doc.mime_type.clone();
                streams.insert(token.clone(), Arc::new(Stream { client: client.clone(), doc, size, mime, part, path, have: Mutex::new(vec![false; parts]), fetch: tokio::sync::Mutex::new(()), complete: AtomicBool::new(false), peer, id, out: out.clone() }));
                Ok::<_, String>(Got::Stream(format!("http://127.0.0.1:{}/{token}", srv.port)))
            }
            .await;
            out.send(match res {
                Ok(Got::File(path)) => Ev::MediaReady { peer, id, path },
                Ok(Got::Stream(url)) => Ev::MediaStream { peer, id, url },
                Err(e) => Ev::Error(e),
            });
        });
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::parse_request;

    #[test]
    fn requests() {
        assert_eq!(parse_request("GET /abc HTTP/1.1\r\nRange: bytes=100-\r\n\r\n"), Some((false, "abc".into(), Some((100, None)))));
        assert_eq!(parse_request("HEAD /abc HTTP/1.1\r\nrange: bytes=0-511\r\n\r\n"), Some((true, "abc".into(), Some((0, Some(511))))));
        assert_eq!(parse_request("GET /abc HTTP/1.1\r\nHost: x\r\n\r\n"), Some((false, "abc".into(), None)));
        assert_eq!(parse_request("POST /abc HTTP/1.1\r\n\r\n"), None);
    }
}
