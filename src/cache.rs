//! Local cache of dialogs and messages (SQLite): chats are visible right after launch and offline.
//! All access goes through one task, so writes and reads never overtake each other,
//! and the backend never waits on disk, except reading history before a server request.
//! Messages are stored as JSON: the schema doesn't duplicate `Msg`, and `VERSION` handles format changes.

use std::path::{Path, PathBuf};

use grammers_client::session::types::{PeerAuth, PeerId, PeerRef};
use libsql::{Connection, params};
use tokio::sync::{mpsc, oneshot};

use crate::backend::{ChatInfo, Ev, Msg, Queued, Reaction, make_private};

/// Bump when the shape of `Msg` or `ChatInfo` changes: the old cache is then simply wiped.
const VERSION: i64 = 1;
/// How many recent messages of each chat to keep.
const KEEP: i64 = 1000;
/// Bot API ids of channels and supergroups are below this; the rest are private chats and small groups.
const CHANNEL_IDS: i64 = -1_000_000_000_000;

/// Cache file next to the account session: deleted together with its directory.
pub fn path(session: &Path) -> PathBuf {
    session.with_file_name("chats.sqlite")
}

/// What to show before the server responds.
#[derive(Default)]
pub struct Start {
    pub me: String,
    pub chats: Vec<ChatInfo>,
    pub refs: Vec<PeerRef>,
}

enum Op {
    Start(oneshot::Sender<Start>),
    Me(String),
    /// Full dialog list (with access hashes), replaces the previous one.
    Dialogs(Vec<(ChatInfo, Option<i64>)>),
    Msgs(Vec<Msg>),
    Delete { peer: Option<PeerId>, ids: Vec<i32> },
    Reactions { peer: PeerId, id: i32, reactions: Vec<Reaction> },
    /// A page of history older than `before` (or the newest one).
    History { peer: PeerId, before: Option<i32>, limit: usize, reply: oneshot::Sender<Vec<Msg>> },
    Clear,
    /// Replies once all previous writes are on disk.
    Flush(oneshot::Sender<()>),
    /// Send queue: add or update, remove, read all (in queueing order).
    QueuePut(Queued),
    QueueDrop(i64),
    Outbox(oneshot::Sender<Vec<Queued>>),
}

#[derive(Clone)]
pub struct Cache(mpsc::UnboundedSender<Op>);

impl Cache {
    /// Starts the cache task; if the file didn't open, the cache is silently disabled.
    pub fn spawn(path: PathBuf) -> Self {
        let (tx, rx) = mpsc::unbounded_channel();
        tokio::spawn(async move {
            match open(&path).await {
                Ok(conn) => serve(conn, rx).await,
                Err(e) => eprintln!("cache: {e}"),
            }
        });
        Self(tx)
    }

    pub async fn start(&self) -> Start {
        let (tx, rx) = oneshot::channel();
        let _ = self.0.send(Op::Start(tx));
        rx.await.unwrap_or_default()
    }

    pub async fn history(&self, peer: PeerId, before: Option<i32>, limit: usize) -> Vec<Msg> {
        let (reply, rx) = oneshot::channel();
        let _ = self.0.send(Op::History { peer, before, limit, reply });
        rx.await.unwrap_or_default()
    }

    pub async fn flush(&self) {
        let (tx, rx) = oneshot::channel();
        let _ = self.0.send(Op::Flush(tx));
        let _ = rx.await;
    }

    pub fn me(&self, name: String) {
        let _ = self.0.send(Op::Me(name));
    }

    pub fn dialogs(&self, chats: Vec<(ChatInfo, Option<i64>)>) {
        let _ = self.0.send(Op::Dialogs(chats));
    }

    pub fn clear(&self) {
        let _ = self.0.send(Op::Clear);
    }

    pub fn queue_put(&self, q: &Queued) {
        let _ = self.0.send(Op::QueuePut(q.clone()));
    }

    pub fn queue_drop(&self, key: i64) {
        let _ = self.0.send(Op::QueueDrop(key));
    }

    pub async fn outbox(&self) -> Vec<Queued> {
        let (tx, rx) = oneshot::channel();
        let _ = self.0.send(Op::Outbox(tx));
        rx.await.unwrap_or_default()
    }

    /// Remembers what the backend just handed to the UI.
    pub fn observe(&self, ev: &Ev) {
        let op = match ev {
            Ev::History { msgs, .. } | Ev::TopicHistory { msgs, .. } if !msgs.is_empty() => Op::Msgs(msgs.clone()),
            Ev::NewMsg(m) | Ev::Edited(m) => Op::Msgs(vec![m.clone()]),
            Ev::Deleted { ids, peer } => Op::Delete { peer: *peer, ids: ids.clone() },
            Ev::Reactions { peer, id, reactions } => Op::Reactions { peer: *peer, id: *id, reactions: reactions.clone() },
            _ => return,
        };
        let _ = self.0.send(op);
    }
}

async fn open(path: &Path) -> Result<Connection, libsql::Error> {
    let conn = libsql::Builder::new_local(path).build().await?.connect()?;
    make_private(path);
    let mut rows = conn.query("PRAGMA user_version", ()).await?;
    let version = match rows.next().await? {
        Some(r) => r.get::<i64>(0)?,
        None => 0,
    };
    if version != VERSION {
        conn.execute_batch("DROP TABLE IF EXISTS dialogs; DROP TABLE IF EXISTS messages; DROP TABLE IF EXISTS meta;").await?;
    }
    conn.execute_batch(&format!(
        "CREATE TABLE IF NOT EXISTS dialogs (peer INTEGER PRIMARY KEY, auth INTEGER, pos INTEGER NOT NULL, data TEXT NOT NULL);
         CREATE TABLE IF NOT EXISTS messages (peer INTEGER NOT NULL, id INTEGER NOT NULL, data TEXT NOT NULL, PRIMARY KEY (peer, id));
         CREATE TABLE IF NOT EXISTS meta (key TEXT PRIMARY KEY, value TEXT NOT NULL);
         CREATE TABLE IF NOT EXISTS outbox (key INTEGER PRIMARY KEY, data TEXT NOT NULL);
         PRAGMA user_version = {VERSION};"
    ))
    .await?;
    Ok(conn)
}

async fn serve(conn: Connection, mut rx: mpsc::UnboundedReceiver<Op>) {
    while let Some(op) = rx.recv().await {
        if let Err(e) = apply(&conn, op).await {
            eprintln!("cache: {e}");
        }
    }
}

fn key(peer: PeerId) -> i64 {
    peer.bot_api_dialog_id_unchecked()
}

async fn apply(conn: &Connection, op: Op) -> Result<(), libsql::Error> {
    match op {
        Op::Start(reply) => {
            let _ = reply.send(start(conn).await?);
        }
        Op::Me(name) => {
            conn.execute("INSERT OR REPLACE INTO meta (key, value) VALUES ('me', ?1)", params![name]).await?;
        }
        Op::Dialogs(chats) => {
            let tx = conn.transaction().await?;
            tx.execute("DELETE FROM dialogs", ()).await?;
            for (pos, (chat, auth)) in chats.iter().enumerate() {
                let Ok(data) = serde_json::to_string(chat) else { continue };
                tx.execute("INSERT OR REPLACE INTO dialogs (peer, auth, pos, data) VALUES (?1, ?2, ?3, ?4)", params![key(chat.id), *auth, pos as i64, data])
                    .await?;
            }
            tx.commit().await?;
        }
        Op::Msgs(msgs) => {
            let tx = conn.transaction().await?;
            for m in &msgs {
                let Ok(data) = serde_json::to_string(m) else { continue };
                tx.execute("INSERT OR REPLACE INTO messages (peer, id, data) VALUES (?1, ?2, ?3)", params![key(m.peer), m.id, data]).await?;
            }
            let mut peers: Vec<i64> = msgs.iter().map(|m| key(m.peer)).collect();
            peers.dedup();
            for p in peers {
                tx.execute(
                    "DELETE FROM messages WHERE peer = ?1 AND id < (SELECT id FROM messages WHERE peer = ?1 ORDER BY id DESC LIMIT 1 OFFSET ?2)",
                    params![p, KEEP - 1],
                )
                .await?;
            }
            tx.commit().await?;
        }
        Op::Delete { peer, ids } => {
            let tx = conn.transaction().await?;
            for id in ids {
                match peer {
                    Some(p) => tx.execute("DELETE FROM messages WHERE peer = ?1 AND id = ?2", params![key(p), id]).await?,
                    // Ids without a chat are unique within the account's private chats and small groups.
                    None => tx.execute("DELETE FROM messages WHERE peer > ?1 AND id = ?2", params![CHANNEL_IDS, id]).await?,
                };
            }
            tx.commit().await?;
        }
        Op::Reactions { peer, id, reactions } => {
            let mut rows = conn.query("SELECT data FROM messages WHERE peer = ?1 AND id = ?2", params![key(peer), id]).await?;
            if let Some(row) = rows.next().await?
                && let Ok(mut m) = serde_json::from_str::<Msg>(&row.get::<String>(0)?)
            {
                m.reactions = reactions;
                if let Ok(data) = serde_json::to_string(&m) {
                    conn.execute("UPDATE messages SET data = ?3 WHERE peer = ?1 AND id = ?2", params![key(peer), id, data]).await?;
                }
            }
        }
        Op::History { peer, before, limit, reply } => {
            let mut rows = conn
                .query(
                    "SELECT data FROM messages WHERE peer = ?1 AND id < ?2 ORDER BY id DESC LIMIT ?3",
                    params![key(peer), before.unwrap_or(i32::MAX), limit as i64],
                )
                .await?;
            let mut msgs = Vec::new();
            while let Some(row) = rows.next().await? {
                if let Ok(m) = serde_json::from_str::<Msg>(&row.get::<String>(0)?) {
                    msgs.push(m);
                }
            }
            msgs.reverse();
            let _ = reply.send(msgs);
        }
        Op::Clear => {
            conn.execute_batch("DELETE FROM dialogs; DELETE FROM messages; DELETE FROM meta; DELETE FROM outbox;").await?;
        }
        Op::Flush(reply) => {
            let _ = reply.send(());
        }
        Op::QueuePut(q) => {
            if let Ok(data) = serde_json::to_string(&q) {
                conn.execute("INSERT OR REPLACE INTO outbox (key, data) VALUES (?1, ?2)", params![q.key, data]).await?;
            }
        }
        Op::QueueDrop(key) => {
            conn.execute("DELETE FROM outbox WHERE key = ?1", params![key]).await?;
        }
        Op::Outbox(reply) => {
            let mut rows = conn.query("SELECT data FROM outbox ORDER BY key", ()).await?;
            let mut list = Vec::new();
            while let Some(row) = rows.next().await? {
                if let Ok(q) = serde_json::from_str::<Queued>(&row.get::<String>(0)?) {
                    list.push(q);
                }
            }
            let _ = reply.send(list);
        }
    }
    Ok(())
}

async fn start(conn: &Connection) -> Result<Start, libsql::Error> {
    let mut out = Start::default();
    let mut rows = conn.query("SELECT value FROM meta WHERE key = 'me'", ()).await?;
    if let Some(row) = rows.next().await? {
        out.me = row.get(0)?;
    }
    let mut rows = conn.query("SELECT auth, data FROM dialogs ORDER BY pos", ()).await?;
    while let Some(row) = rows.next().await? {
        let Ok(chat) = serde_json::from_str::<ChatInfo>(&row.get::<String>(1)?) else { continue };
        if let Some(hash) = row.get::<Option<i64>>(0)? {
            out.refs.push(PeerRef { id: chat.id, auth: PeerAuth::from_hash(hash) });
        }
        out.chats.push(chat);
    }
    Ok(out)
}

/// `PeerId` in JSON as a Bot API id, so we don't need to enable serde in grammers.
pub mod peer_id {
    use grammers_client::session::types::PeerId;
    use serde::{Deserialize, Deserializer, Serializer, de::Error};

    pub fn serialize<S: Serializer>(peer: &PeerId, s: S) -> Result<S::Ok, S::Error> {
        s.serialize_i64(peer.bot_api_dialog_id_unchecked())
    }

    pub fn deserialize<'de, D: Deserializer<'de>>(d: D) -> Result<PeerId, D::Error> {
        let id = i64::deserialize(d)?;
        PeerId::from_bot_api_dialog_id(id).ok_or_else(|| D::Error::custom("peer id"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn msg(peer: PeerId, id: i32) -> Msg {
        Msg { id, text: format!("#{id}"), ..Msg::placeholder(peer) }
    }

    #[tokio::test]
    async fn keeps_history_and_dialogs() {
        let dir = std::env::temp_dir().join(format!("glassgram-cache-test-{}", std::process::id()));
        let _ = std::fs::create_dir_all(&dir);
        let file = dir.join("cache.sqlite");
        let _ = std::fs::remove_file(&file);
        let peer = PeerId::channel_unchecked(77);
        let user = PeerId::user_unchecked(5);
        {
            let cache = Cache::spawn(file.clone());
            cache.me("Я".into());
            let chat = crate::demo::data().1.remove(0);
            cache.dialogs(vec![(ChatInfo { id: user, ..chat }, Some(42))]);
            cache.observe(&Ev::History { peer, msgs: (1..=60).map(|i| msg(peer, i)).collect(), older: false });
            cache.observe(&Ev::NewMsg(msg(user, 9)));
            cache.observe(&Ev::Deleted { ids: vec![9], peer: None });
            cache.observe(&Ev::Deleted { ids: vec![60], peer: Some(peer) });
            cache.flush().await;
        }
        let cache = Cache::spawn(file.clone());
        let start = cache.start().await;
        assert_eq!(start.me, "Я");
        assert_eq!(start.chats.len(), 1);
        assert_eq!(start.refs, vec![PeerRef { id: user, auth: PeerAuth::from_hash(42) }]);
        let last: Vec<i32> = cache.history(peer, None, 5).await.iter().map(|m| m.id).collect();
        assert_eq!(last, vec![55, 56, 57, 58, 59]);
        let older: Vec<i32> = cache.history(peer, Some(3), 50).await.iter().map(|m| m.id).collect();
        assert_eq!(older, vec![1, 2]);
        assert!(cache.history(user, None, 50).await.is_empty());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[tokio::test]
    async fn outbox_survives_restart() {
        let dir = std::env::temp_dir().join(format!("glassgram-outbox-test-{}", std::process::id()));
        let _ = std::fs::create_dir_all(&dir);
        let file = dir.join("cache.sqlite");
        let _ = std::fs::remove_file(&file);
        let peer = PeerId::user_unchecked(5);
        let q = |key, text: &str| Queued { key, peer, text: text.into(), reply_to: None, silent: false, ts: 0, failed: false, no_webpage: false, quote: None };
        {
            let cache = Cache::spawn(file.clone());
            cache.queue_put(&q(2, "друге"));
            cache.queue_put(&q(1, "перше"));
            cache.queue_put(&q(3, "третє"));
            cache.queue_drop(3);
            cache.queue_put(&Queued { failed: true, ..q(2, "друге") });
            cache.flush().await;
        }
        let cache = Cache::spawn(file.clone());
        let list = cache.outbox().await;
        assert_eq!(list.iter().map(|q| q.key).collect::<Vec<_>>(), vec![1, 2]);
        assert!(list[1].failed && !list[0].failed);
        let _ = std::fs::remove_dir_all(&dir);
    }
}
