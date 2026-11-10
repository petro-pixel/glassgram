//! Proxies, as in Telegram Desktop: SOCKS5 and MTProto, several saved, switched on the fly.
//!
//! grammers only speaks SOCKS5, so whenever proxies are saved it connects through a local bridge
//! (127.0.0.1, SOCKS5 without a password) that forwards each connection directly, through the chosen
//! SOCKS5 proxy, or wrapped in MTProxy's obfuscated transport. Switching only changes the bridge's
//! upstream and drops the open connections; grammers reconnects by itself.
//!
//! MTProxy: the stream is AES-256-CTR with keys from a random 64-byte header mixed with the secret,
//! the frames are "intermediate" (`dd` secrets: padded intermediate), the header names the DC.
//! grammers frames its packets with the "full" transport (length, sequence number, CRC32), so the
//! bridge unpacks and repacks every packet in both directions.

use std::net::{IpAddr, SocketAddr};
use std::sync::OnceLock;
use std::time::{Duration, Instant};

use aes::Aes256;
use ctr::cipher::{KeyIvInit, StreamCipher};
use sha2::{Digest, Sha256};
use tokio::io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};
use tokio::sync::watch;

use super::*;

type Aes = ctr::Ctr128BE<Aes256>;

/// A saved proxy, parsed from its link.
#[derive(Clone, Debug, PartialEq)]
pub enum Proxy {
    Socks { host: String, port: u16, user: String, pass: String },
    MtProto { host: String, port: u16, secret: [u8; 16], padded: bool },
}

impl Proxy {
    /// `socks5://[user:pass@]host:port`, or a Telegram link: `tg://proxy?server=…&port=…&secret=…`,
    /// `https://t.me/proxy?…`, `tg://socks?server=…&port=…[&user=…&pass=…]`, `https://t.me/socks?…`.
    pub fn parse(link: &str) -> Result<Proxy, &'static str> {
        let link = link.trim();
        if let Some(rest) = link.strip_prefix("socks5://") {
            let (auth, host_port) = rest.rsplit_once('@').map_or(("", rest), |(a, h)| (a, h));
            let (user, pass) = auth.split_once(':').unwrap_or((auth, ""));
            let (host, port) = host_port.rsplit_once(':').ok_or(l!("Потрібні хост і порт, напр. socks5://127.0.0.1:1080", "Host and port are required, e.g. socks5://127.0.0.1:1080"))?;
            return Self::socks(host, port, user, pass);
        }
        let rest = link.strip_prefix("https://").or_else(|| link.strip_prefix("http://")).unwrap_or(link);
        let query = ["tg://proxy?", "t.me/proxy?", "telegram.me/proxy?", "tg://socks?", "t.me/socks?", "telegram.me/socks?"]
            .iter()
            .find_map(|p| rest.strip_prefix(p).map(|q| (p.contains("socks"), q)));
        let Some((socks, query)) = query else {
            return Err(l!("Вставте посилання на проксі (t.me/proxy?…, tg://proxy?…) або socks5://хост:порт", "Paste a proxy link (t.me/proxy?…, tg://proxy?…) or socks5://host:port"));
        };
        let param = |name: &str| query.split('&').find_map(|kv| kv.split_once('=').filter(|(k, _)| *k == name).map(|(_, v)| percent_decode(v))).unwrap_or_default();
        let (host, port) = (param("server"), param("port"));
        if socks {
            return Self::socks(&host, &port, &param("user"), &param("pass"));
        }
        let port = port.parse::<u16>().ok().filter(|p| *p > 0).ok_or(l!("У посиланні немає порту", "The link has no port"))?;
        if host.is_empty() {
            return Err(l!("У посиланні немає сервера", "The link has no server"));
        }
        let secret = decode_secret(&param("secret")).ok_or(l!("Секрет проксі пошкоджений", "The proxy secret is damaged"))?;
        match secret.as_slice() {
            [s @ ..] if s.len() == 16 => Ok(Proxy::MtProto { host, port, secret: s.try_into().unwrap_or_default(), padded: false }),
            [0xdd, s @ ..] if s.len() == 16 => Ok(Proxy::MtProto { host, port, secret: s.try_into().unwrap_or_default(), padded: true }),
            [0xee, ..] => Err(l!("Проксі з маскуванням під TLS (секрет починається з ee) поки не підтримуються", "Proxies disguised as TLS (a secret starting with ee) aren't supported yet")),
            _ => Err(l!("Секрет проксі пошкоджений", "The proxy secret is damaged")),
        }
    }

    fn socks(host: &str, port: &str, user: &str, pass: &str) -> Result<Proxy, &'static str> {
        let host = host.trim_start_matches('[').trim_end_matches(']');
        match port.parse::<u16>() {
            Ok(port) if port > 0 && !host.is_empty() => Ok(Proxy::Socks { host: host.to_owned(), port, user: user.to_owned(), pass: pass.to_owned() }),
            _ => Err(l!("Потрібні хост і порт, напр. socks5://127.0.0.1:1080", "Host and port are required, e.g. socks5://127.0.0.1:1080")),
        }
    }

    pub fn kind(&self) -> &'static str {
        match self {
            Proxy::Socks { .. } => "SOCKS5",
            Proxy::MtProto { .. } => "MTProto",
        }
    }

    pub fn address(&self) -> String {
        let (Proxy::Socks { host, port, .. } | Proxy::MtProto { host, port, .. }) = self;
        if host.contains(':') { format!("[{host}]:{port}") } else { format!("{host}:{port}") }
    }

    async fn connect(&self) -> std::io::Result<TcpStream> {
        let (Proxy::Socks { host, port, .. } | Proxy::MtProto { host, port, .. }) = self;
        let s = tokio::time::timeout(Duration::from_secs(10), TcpStream::connect((host.as_str(), *port))).await.map_err(|_| io_err("timed out"))??;
        let _ = s.set_nodelay(true);
        Ok(s)
    }
}

/// Whether a link from a message is a proxy to offer.
pub fn is_proxy_link(link: &str) -> bool {
    let rest = link.strip_prefix("https://").or_else(|| link.strip_prefix("http://")).unwrap_or(link);
    ["tg://proxy?", "t.me/proxy?", "telegram.me/proxy?", "tg://socks?", "t.me/socks?", "telegram.me/socks?"].iter().any(|p| rest.starts_with(p))
}

fn percent_decode(s: &str) -> String {
    let b = s.as_bytes();
    let mut out = Vec::with_capacity(b.len());
    let mut i = 0;
    while i < b.len() {
        let hex = || std::str::from_utf8(b.get(i + 1..i + 3)?).ok().and_then(|h| u8::from_str_radix(h, 16).ok());
        match (b[i], hex()) {
            (b'%', Some(v)) => {
                out.push(v);
                i += 3;
            }
            (c, _) => {
                out.push(c);
                i += 1;
            }
        }
    }
    String::from_utf8_lossy(&out).into_owned()
}

/// Secrets come as hex or as (URL-safe) base64.
fn decode_secret(s: &str) -> Option<Vec<u8>> {
    use base64::Engine;
    let s = s.trim();
    if s.len() % 2 == 0 && !s.is_empty() && s.bytes().all(|c| c.is_ascii_hexdigit()) {
        return (0..s.len()).step_by(2).map(|i| u8::from_str_radix(&s[i..i + 2], 16).ok()).collect();
    }
    let s = s.trim_end_matches('=').replace('-', "+").replace('_', "/");
    base64::engine::general_purpose::STANDARD_NO_PAD.decode(s).ok()
}

fn io_err(e: &str) -> std::io::Error {
    std::io::Error::other(e.to_owned())
}

// ───────────────────────────── The bridge ─────────────────────────────

struct Bridge {
    port: u16,
    /// The proxy connections go through now; `None` — straight to Telegram.
    up: watch::Sender<Option<Proxy>>,
    /// To tell which DC an address belongs to (MTProxy needs its number).
    session: Mutex<Arc<SqliteSession>>,
}

static BRIDGE: OnceLock<Bridge> = OnceLock::new();

/// The address grammers should connect through: the bridge when any proxy is saved (so switching
/// later needs no reconnect), nothing when there are none.
pub(super) async fn connection_url(session: &Arc<SqliteSession>) -> Option<String> {
    let s = crate::settings::Settings::load();
    if s.proxies.is_empty() {
        return None;
    }
    let up = Proxy::parse(&s.proxy).ok();
    if let Some(b) = BRIDGE.get() {
        *b.session.lock().unwrap() = Arc::clone(session);
        b.up.send_replace(up);
        return Some(format!("socks5://127.0.0.1:{}", b.port));
    }
    let listener = match TcpListener::bind("127.0.0.1:0").await {
        Ok(l) => l,
        Err(e) => {
            log_error(&format!("proxy bridge: {e}"));
            return None;
        }
    };
    let port = listener.local_addr().ok()?.port();
    let bridge = Bridge { port, up: watch::Sender::new(up), session: Mutex::new(Arc::clone(session)) };
    if BRIDGE.set(bridge).is_ok() {
        tokio::spawn(async move {
            while let Ok((sock, _)) = listener.accept().await {
                let _ = sock.set_nodelay(true);
                tokio::spawn(async move {
                    if let Err(e) = serve(sock).await {
                        log_error(&format!("proxy: {e}"));
                    }
                });
            }
        });
    }
    BRIDGE.get().map(|b| format!("socks5://127.0.0.1:{}", b.port))
}

/// Points the bridge at another proxy (or none). `false`: there is no bridge in this run, the
/// connection was made without one, so the change waits for a restart.
pub(super) fn switch(up: Option<Proxy>) -> bool {
    match BRIDGE.get() {
        Some(b) => {
            b.up.send_replace(up);
            true
        }
        None => false,
    }
}

async fn serve(mut sock: TcpStream) -> std::io::Result<()> {
    let target = socks_accept(&mut sock).await?;
    let b = BRIDGE.get().ok_or_else(|| io_err("no bridge"))?;
    let mut rx = b.up.subscribe();
    let up = rx.borrow_and_update().clone();
    let upstream = async {
        match &up {
            None => TcpStream::connect(target).await,
            Some(p @ Proxy::Socks { user, pass, .. }) => {
                let mut s = p.connect().await?;
                socks_connect(&mut s, target, user, pass).await?;
                Ok(s)
            }
            Some(p @ Proxy::MtProto { .. }) => p.connect().await,
        }
    };
    let mut out = match upstream.await {
        Ok(s) => s,
        Err(e) => {
            let _ = sock.write_all(&[5, 1, 0, 1, 0, 0, 0, 0, 0, 0]).await;
            return Err(e);
        }
    };
    sock.write_all(&[5, 0, 0, 1, 0, 0, 0, 0, 0, 0]).await?;
    let pump = async {
        match &up {
            Some(Proxy::MtProto { secret, padded, .. }) => {
                let dc = dc_of(&b.session.lock().unwrap(), target.ip());
                mtproxy_pump(sock, out, secret, *padded, dc).await
            }
            _ => tokio::io::copy_bidirectional(&mut sock, &mut out).await.map(|_| ()),
        }
    };
    tokio::select! {
        r = pump => r,
        // Another proxy was chosen: drop this connection, grammers opens a new one through it.
        _ = rx.changed() => Ok(()),
    }
}

/// The DC an address belongs to, from the session's own list (what grammers connects to).
fn dc_of(session: &SqliteSession, ip: IpAddr) -> i16 {
    for id in 1..=5 {
        if let Ok(Some(o)) = session.dc_option(id)
            && (IpAddr::V4(*o.ipv4.ip()) == ip || IpAddr::V6(*o.ipv6.ip()) == ip)
        {
            return id as i16;
        }
    }
    session.home_dc_id().unwrap_or(2) as i16
}

/// The SOCKS5 server side, for grammers: no authentication, CONNECT only.
async fn socks_accept(s: &mut TcpStream) -> std::io::Result<SocketAddr> {
    let mut head = [0u8; 2];
    s.read_exact(&mut head).await?;
    let mut methods = vec![0u8; head[1] as usize];
    s.read_exact(&mut methods).await?;
    if head[0] != 5 || !methods.contains(&0) {
        return Err(io_err("not a SOCKS5 client"));
    }
    s.write_all(&[5, 0]).await?;
    let mut req = [0u8; 4];
    s.read_exact(&mut req).await?;
    if req[1] != 1 {
        let _ = s.write_all(&[5, 7, 0, 1, 0, 0, 0, 0, 0, 0]).await;
        return Err(io_err("only CONNECT"));
    }
    let ip: IpAddr = match req[3] {
        1 => {
            let mut a = [0u8; 4];
            s.read_exact(&mut a).await?;
            a.into()
        }
        4 => {
            let mut a = [0u8; 16];
            s.read_exact(&mut a).await?;
            a.into()
        }
        3 => {
            let mut n = [0u8; 1];
            s.read_exact(&mut n).await?;
            let mut name = vec![0u8; n[0] as usize];
            s.read_exact(&mut name).await?;
            let mut port = [0u8; 2];
            s.read_exact(&mut port).await?;
            let host = format!("{}:{}", String::from_utf8_lossy(&name), u16::from_be_bytes(port));
            return tokio::net::lookup_host(host).await?.next().ok_or_else(|| io_err("unknown host"));
        }
        _ => return Err(io_err("bad address")),
    };
    let mut port = [0u8; 2];
    s.read_exact(&mut port).await?;
    Ok(SocketAddr::new(ip, u16::from_be_bytes(port)))
}

/// The SOCKS5 client side, to the user's proxy.
async fn socks_connect(s: &mut TcpStream, target: SocketAddr, user: &str, pass: &str) -> std::io::Result<()> {
    let auth = !user.is_empty();
    s.write_all(if auth { &[5, 2, 0, 2] } else { &[5, 1, 0] }).await?;
    let mut reply = [0u8; 2];
    s.read_exact(&mut reply).await?;
    match reply {
        [5, 0] => {}
        [5, 2] if auth => {
            let mut req = vec![1, user.len().min(255) as u8];
            req.extend_from_slice(&user.as_bytes()[..user.len().min(255)]);
            req.push(pass.len().min(255) as u8);
            req.extend_from_slice(&pass.as_bytes()[..pass.len().min(255)]);
            s.write_all(&req).await?;
            let mut ok = [0u8; 2];
            s.read_exact(&mut ok).await?;
            if ok[1] != 0 {
                return Err(io_err("the proxy refused the login or password"));
            }
        }
        _ => return Err(io_err("the proxy wants a login it wasn't given")),
    }
    let mut req = vec![5, 1, 0];
    match target.ip() {
        IpAddr::V4(ip) => {
            req.push(1);
            req.extend_from_slice(&ip.octets());
        }
        IpAddr::V6(ip) => {
            req.push(4);
            req.extend_from_slice(&ip.octets());
        }
    }
    req.extend_from_slice(&target.port().to_be_bytes());
    s.write_all(&req).await?;
    let mut head = [0u8; 4];
    s.read_exact(&mut head).await?;
    if head[1] != 0 {
        return Err(io_err("the proxy couldn't reach Telegram"));
    }
    let skip = match head[3] {
        1 => 4 + 2,
        4 => 16 + 2,
        3 => {
            let mut n = [0u8; 1];
            s.read_exact(&mut n).await?;
            n[0] as usize + 2
        }
        _ => return Err(io_err("bad proxy reply")),
    };
    let mut rest = vec![0u8; skip];
    s.read_exact(&mut rest).await?;
    Ok(())
}

// ───────────────────────────── MTProxy ─────────────────────────────

/// The obfuscated header and the two ciphers that continue after it.
fn obfuscated_init(secret: &[u8; 16], padded: bool, dc: i16) -> std::io::Result<([u8; 64], Aes, Aes)> {
    let mut init = [0u8; 64];
    // Not something the server would take for another protocol.
    loop {
        getrandom::fill(&mut init).map_err(|e| io_err(&e.to_string()))?;
        let first = u32::from_le_bytes([init[0], init[1], init[2], init[3]]);
        let bad = init[0] == 0xef
            || [*b"HEAD", *b"POST", *b"GET ", *b"OPTI", [0xdd; 4], [0xee; 4], [0x16, 0x03, 0x01, 0x02]].iter().any(|p| init[..4] == *p)
            || init[4..8] == [0; 4]
            || first == 0;
        if !bad {
            break;
        }
    }
    init[56..60].copy_from_slice(&if padded { [0xdd; 4] } else { [0xee; 4] });
    init[60..62].copy_from_slice(&dc.to_le_bytes());
    let key = |k: &[u8]| -> [u8; 32] { Sha256::new().chain_update(k).chain_update(secret).finalize().into() };
    let rev: Vec<u8> = init[8..56].iter().rev().copied().collect();
    let mut enc = Aes::new_from_slices(&key(&init[8..40]), &init[40..56]).map_err(|e| io_err(&e.to_string()))?;
    let dec = Aes::new_from_slices(&key(&rev[..32]), &rev[32..48]).map_err(|e| io_err(&e.to_string()))?;
    let mut sealed = init;
    enc.apply_keystream(&mut sealed);
    init[56..].copy_from_slice(&sealed[56..]);
    Ok((init, enc, dec))
}

/// Where an MTProto packet really ends, so the random tail padded intermediate adds is cut off.
fn packet_len(p: &[u8]) -> usize {
    if p.len() < 24 {
        return p.len();
    }
    if p[..8] == [0; 8] {
        // Plain message: auth_key_id, msg_id, length, data.
        let n = i32::from_le_bytes([p[16], p[17], p[18], p[19]]).max(0) as usize;
        (20 + n).min(p.len())
    } else {
        // Encrypted: auth_key_id and msg_key, then whole AES blocks.
        24 + (p.len() - 24) / 16 * 16
    }
}

/// Frames grammers' "full" transport packets into MTProxy's and back.
async fn mtproxy_pump(client: TcpStream, server: TcpStream, secret: &[u8; 16], padded: bool, dc: i16) -> std::io::Result<()> {
    let (init, mut enc, mut dec) = obfuscated_init(secret, padded, dc)?;
    let (mut cr, mut cw) = client.into_split();
    let (mut sr, mut sw) = server.into_split();
    sw.write_all(&init).await?;
    let up = async move {
        loop {
            let payload = read_full(&mut cr).await?;
            let pad = if padded { random_len(16) } else { 0 };
            let mut frame = Vec::with_capacity(4 + payload.len() + pad);
            frame.extend_from_slice(&((payload.len() + pad) as u32).to_le_bytes());
            frame.extend_from_slice(&payload);
            frame.resize(4 + payload.len() + pad, 0);
            if pad > 0 {
                getrandom::fill(&mut frame[4 + payload.len()..]).map_err(|e| io_err(&e.to_string()))?;
            }
            enc.apply_keystream(&mut frame);
            sw.write_all(&frame).await?;
        }
        #[allow(unreachable_code)]
        Ok::<_, std::io::Error>(())
    };
    let down = async move {
        let mut seq = 0u32;
        loop {
            let mut head = [0u8; 4];
            sr.read_exact(&mut head).await?;
            dec.apply_keystream(&mut head);
            // The top bit asks for a quick ack; the length is the rest.
            let n = (u32::from_le_bytes(head) & 0x7fff_ffff) as usize;
            if n > 16 << 20 {
                return Err(io_err("packet too large"));
            }
            let mut body = vec![0u8; n];
            sr.read_exact(&mut body).await?;
            dec.apply_keystream(&mut body);
            let body = if padded { &body[..packet_len(&body)] } else { &body[..] };
            write_full(&mut cw, seq, body).await?;
            seq = seq.wrapping_add(1);
        }
        #[allow(unreachable_code)]
        Ok::<_, std::io::Error>(())
    };
    tokio::select! {
        r = up => r,
        r = down => r,
    }
}

fn random_len(below: usize) -> usize {
    let mut b = [0u8; 1];
    let _ = getrandom::fill(&mut b);
    b[0] as usize % below
}

/// One packet of the "full" transport: length, sequence number, payload, CRC32 of all before it.
async fn read_full(r: &mut (impl AsyncRead + Unpin)) -> std::io::Result<Vec<u8>> {
    let mut len = [0u8; 4];
    r.read_exact(&mut len).await?;
    let n = u32::from_le_bytes(len) as usize;
    if !(12..=16 << 20).contains(&n) {
        return Err(io_err("bad packet from grammers"));
    }
    let mut rest = vec![0u8; n - 4];
    r.read_exact(&mut rest).await?;
    Ok(rest[4..rest.len() - 4].to_vec())
}

async fn write_full(w: &mut (impl AsyncWrite + Unpin), seq: u32, payload: &[u8]) -> std::io::Result<()> {
    let mut frame = Vec::with_capacity(payload.len() + 12);
    frame.extend_from_slice(&((payload.len() + 12) as u32).to_le_bytes());
    frame.extend_from_slice(&seq.to_le_bytes());
    frame.extend_from_slice(payload);
    let crc = crc32fast::hash(&frame);
    frame.extend_from_slice(&crc.to_le_bytes());
    w.write_all(&frame).await
}

// ───────────────────────────── Checking ─────────────────────────────

/// DC 2, to ping a proxy with.
const PING_DC: (std::net::Ipv4Addr, u16, i16) = (std::net::Ipv4Addr::new(149, 154, 167, 51), 443, 2);

/// Whether the proxy reaches Telegram, and how fast: an unencrypted `req_pq_multi` to DC 2 and its
/// answer, the first step of every connection.
async fn ping(proxy: &Proxy) -> std::io::Result<u32> {
    let start = Instant::now();
    let mut s = proxy.connect().await?;
    let mut nonce = [0u8; 16];
    getrandom::fill(&mut nonce).map_err(|e| io_err(&e.to_string()))?;
    let now = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap_or_default();
    let msg_id = (now.as_secs() << 32) | (u64::from(now.subsec_nanos()) & !3);
    let mut msg = Vec::with_capacity(40);
    msg.extend_from_slice(&[0; 8]);
    msg.extend_from_slice(&msg_id.to_le_bytes());
    msg.extend_from_slice(&20u32.to_le_bytes());
    msg.extend_from_slice(&0xbe7e_8ef1_u32.to_le_bytes());
    msg.extend_from_slice(&nonce);
    let mut frame = (msg.len() as u32).to_le_bytes().to_vec();
    frame.extend_from_slice(&msg);
    let mut head = [0u8; 4];
    match proxy {
        Proxy::Socks { user, pass, .. } => {
            socks_connect(&mut s, SocketAddr::new(PING_DC.0.into(), PING_DC.1), user, pass).await?;
            // Plain intermediate transport straight to the DC.
            s.write_all(&[0xee; 4]).await?;
            s.write_all(&frame).await?;
            s.read_exact(&mut head).await?;
        }
        Proxy::MtProto { secret, padded, .. } => {
            let (init, mut enc, mut dec) = obfuscated_init(secret, *padded, PING_DC.2)?;
            enc.apply_keystream(&mut frame);
            s.write_all(&init).await?;
            s.write_all(&frame).await?;
            s.read_exact(&mut head).await?;
            dec.apply_keystream(&mut head);
        }
    }
    // A real answer (resPQ) is longer than an error code.
    if u32::from_le_bytes(head) & 0x7fff_ffff < 20 {
        return Err(io_err("Telegram refused the connection through this proxy"));
    }
    Ok(start.elapsed().as_millis().min(u32::MAX as u128) as u32)
}

impl Worker {
    pub(super) async fn proxy_cmd(&mut self, cmd: Cmd) -> Result<(), String> {
        match cmd {
            Cmd::SetProxy(link) => {
                let up = Proxy::parse(&link).ok().filter(|_| !link.is_empty());
                if !switch(up) && !link.is_empty() {
                    self.out.send(Ev::Notice(l!("Проксі запрацює після перезапуску програми", "The proxy takes effect after a restart").into()));
                }
            }
            Cmd::CheckProxy(link) => {
                let out = self.out.clone();
                tokio::spawn(async move {
                    let result = match Proxy::parse(&link) {
                        Ok(p) => tokio::time::timeout(Duration::from_secs(12), ping(&p)).await.unwrap_or_else(|_| Err(io_err("timed out"))).map_err(|e| e.to_string()),
                        Err(e) => Err(e.to_owned()),
                    };
                    out.send(Ev::ProxyChecked { link, result });
                });
            }
            _ => {}
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn links() {
        let p = Proxy::parse("https://t.me/proxy?server=1.2.3.4&port=443&secret=dd00112233445566778899aabbccddeeff").unwrap();
        assert!(matches!(p, Proxy::MtProto { port: 443, padded: true, .. }));
        assert_eq!(p.address(), "1.2.3.4:443");
        let p = Proxy::parse("tg://proxy?server=proxy.example&port=8443&secret=ABEiM0RVZneImaq7zN3u_w").unwrap();
        assert!(matches!(&p, Proxy::MtProto { host, padded: false, secret, .. } if host == "proxy.example" && secret[0] == 0 && secret[15] == 0xff));
        assert!(Proxy::parse("tg://proxy?server=x&port=1&secret=ee00112233445566778899aabbccddeeff676f6f676c652e636f6d").is_err());
        assert_eq!(Proxy::parse("socks5://u:p@[::1]:1080").unwrap(), Proxy::Socks { host: "::1".into(), port: 1080, user: "u".into(), pass: "p".into() });
        assert_eq!(Proxy::parse("tg://socks?server=h&port=9&user=a%20b").unwrap(), Proxy::Socks { host: "h".into(), port: 9, user: "a b".into(), pass: String::new() });
        assert!(Proxy::parse("http://127.0.0.1:1080").is_err());
        assert!(is_proxy_link("t.me/proxy?server=a") && !is_proxy_link("t.me/someone"));
    }

    #[test]
    fn padding_is_cut() {
        let mut plain = vec![0u8; 8];
        plain.extend_from_slice(&[1; 8]);
        plain.extend_from_slice(&4u32.to_le_bytes());
        plain.extend_from_slice(&[7; 4 + 9]);
        assert_eq!(packet_len(&plain), 24);
        let enc = vec![1u8; 24 + 32 + 5];
        assert_eq!(packet_len(&enc), 56);
    }

    #[test]
    fn header_keys_match() {
        // The server derives its decryption key from the same header the client sent.
        let secret = [9u8; 16];
        let (init, _, _) = obfuscated_init(&secret, true, -2).unwrap();
        let key: [u8; 32] = Sha256::new().chain_update(&init[8..40]).chain_update(secret).finalize().into();
        let mut server = Aes::new_from_slices(&key, &init[40..56]).unwrap();
        let mut copy = init;
        server.apply_keystream(&mut copy);
        assert_eq!(&copy[56..60], &[0xdd; 4]);
        assert_eq!(i16::from_le_bytes([copy[60], copy[61]]), -2);
    }

    /// The bridge against a fake MTProxy on loopback that decrypts each frame and sends it back
    /// with a random tail: grammers' side must get the same packets, framed and numbered right.
    #[tokio::test]
    async fn pump_round_trip() {
        let secret = [5u8; 16];
        let server = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let server_addr = server.local_addr().unwrap();
        tokio::spawn(async move {
            let (mut s, _) = server.accept().await.unwrap();
            let mut init = [0u8; 64];
            s.read_exact(&mut init).await.unwrap();
            let key = |k: &[u8]| -> [u8; 32] { Sha256::new().chain_update(k).chain_update(secret).finalize().into() };
            let rev: Vec<u8> = init[8..56].iter().rev().copied().collect();
            let mut dec = Aes::new_from_slices(&key(&init[8..40]), &init[40..56]).unwrap();
            let mut enc = Aes::new_from_slices(&key(&rev[..32]), &rev[32..48]).unwrap();
            let mut head = init;
            dec.apply_keystream(&mut head);
            assert_eq!(&head[56..60], &[0xdd; 4]);
            loop {
                let mut len = [0u8; 4];
                if s.read_exact(&mut len).await.is_err() {
                    return;
                }
                dec.apply_keystream(&mut len);
                let mut body = vec![0u8; u32::from_le_bytes(len) as usize];
                s.read_exact(&mut body).await.unwrap();
                dec.apply_keystream(&mut body);
                let mut back = body[..packet_len(&body)].to_vec();
                back.extend_from_slice(&[0xab; 7]);
                let mut frame = (back.len() as u32).to_le_bytes().to_vec();
                frame.extend_from_slice(&back);
                enc.apply_keystream(&mut frame);
                s.write_all(&frame).await.unwrap();
            }
        });
        let near = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let mut grammers = TcpStream::connect(near.local_addr().unwrap()).await.unwrap();
        let (client, _) = near.accept().await.unwrap();
        let upstream = TcpStream::connect(server_addr).await.unwrap();
        tokio::spawn(async move {
            let _ = mtproxy_pump(client, upstream, &secret, true, 2).await;
        });
        for (seq, n) in [(0u32, 2usize), (1, 5), (2, 40)] {
            // An encrypted-looking packet: auth_key_id, msg_key, whole blocks.
            let payload: Vec<u8> = (0..24 + 16 * n).map(|i| (i % 251) as u8 + 1).collect();
            write_full(&mut grammers, seq, &payload).await.unwrap();
            let mut len = [0u8; 4];
            grammers.read_exact(&mut len).await.unwrap();
            let total = u32::from_le_bytes(len) as usize;
            let mut rest = vec![0u8; total - 4];
            grammers.read_exact(&mut rest).await.unwrap();
            let mut whole = len.to_vec();
            whole.extend_from_slice(&rest[..rest.len() - 4]);
            assert_eq!(u32::from_le_bytes(rest[rest.len() - 4..].try_into().unwrap()), crc32fast::hash(&whole));
            assert_eq!(u32::from_le_bytes(rest[..4].try_into().unwrap()), seq);
            assert_eq!(&rest[4..rest.len() - 4], &payload[..]);
        }
    }
}

