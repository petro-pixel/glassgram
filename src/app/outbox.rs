//! Send queue: a message shows up as a bubble right away and is retried with growing delays
//! while the network is down. The backend keeps it in the cache, so it survives a restart.

use std::path::{Path, PathBuf};

use grammers_client::session::types::PeerId;

use super::App;
use crate::backend::{Cmd, Msg, Queued, QueuedMedia, ReplyTo, SendOpts, outbox_dir};
use crate::i18n::{l, lf};

/// Automatic attempts before the bubble turns into "failed" and waits for "Retry".
const MAX_ATTEMPTS: u32 = 5;
/// Delay before the next attempt: 2, 4, 8, 16… seconds, at most a minute.
fn backoff(attempts: u32) -> f64 {
    (2.0f64.powi(attempts as i32)).min(60.0)
}

pub(super) struct Pending {
    pub(super) q: Queued,
    /// Local bubble id (negative, so it never collides with server ids).
    pub(super) id: i32,
    attempts: u32,
    next_at: f64,
    in_flight: bool,
}

impl App {
    fn new_local_id(&mut self) -> i32 {
        self.next_local -= 1;
        self.next_local
    }

    /// Bubble state for `id`: `Some(false)` — sending, `Some(true)` — failed, `None` — a regular message.
    pub(super) fn pending_state(&self, id: i32) -> Option<bool> {
        if id >= 0 {
            return None;
        }
        self.outbox.iter().find(|p| p.id == id).map(|p| p.q.failed)
    }

    /// Nanoseconds keep the queue order; a collision within one run is avoided explicitly.
    fn next_key(&self) -> i64 {
        let key = jiff::Timestamp::now().as_nanosecond() as i64;
        self.outbox.iter().map(|p| p.q.key + 1).max().map_or(key, |last| key.max(last))
    }

    /// Queue a text message: it appears in the chat immediately and is sent in the background.
    pub(super) fn enqueue(&mut self, peer: PeerId, text: String, reply: Option<ReplyTo>, opts: &SendOpts) {
        let key = self.next_key();
        self.enqueue_key(key, peer, text, reply, opts, None);
    }

    /// Queue files, a voice or a round video message. They are copied into the queue's folder
    /// first (`moved`: a recording's temp file is moved), so they survive a restart.
    pub(super) fn enqueue_media(&mut self, peer: PeerId, caption: String, reply: Option<ReplyTo>, media: QueuedMedia, moved: bool) {
        let key = self.next_key();
        let dir = outbox_dir(&crate::accounts::session_path(self.accounts.active), key);
        let keep = |k: usize, path: &Path| -> std::io::Result<PathBuf> {
            // One folder per file keeps two files with one name apart, and the name as it was.
            let sub = dir.join(k.to_string());
            std::fs::create_dir_all(&sub)?;
            let name = path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
            let to = sub.join(crate::platform::safe_file_name(&name));
            if !(moved && std::fs::rename(path, &to).is_ok()) {
                std::fs::copy(path, &to)?;
                if moved {
                    let _ = std::fs::remove_file(path);
                }
            }
            Ok(to)
        };
        let kept = match media {
            QueuedMedia::Files { paths, as_file } => paths.iter().enumerate().map(|(k, p)| keep(k, p)).collect::<Result<Vec<_>, _>>().map(|paths| QueuedMedia::Files { paths, as_file }),
            QueuedMedia::Voice { path, duration, waveform } => keep(0, &path).map(|path| QueuedMedia::Voice { path, duration, waveform }),
            QueuedMedia::Round { path, duration } => keep(0, &path).map(|path| QueuedMedia::Round { path, duration }),
        };
        match kept {
            Ok(media) => self.enqueue_key(key, peer, caption, reply, &SendOpts::default(), Some(media)),
            Err(e) => {
                let _ = std::fs::remove_dir_all(&dir);
                self.toast = Some((lf!("Не вдалося підготувати файл: {e}", "Could not prepare the file: {e}"), 0.0, true));
            }
        }
    }

    fn enqueue_key(&mut self, key: i64, peer: PeerId, text: String, reply: Option<ReplyTo>, opts: &SendOpts, media: Option<QueuedMedia>) {
        let ts = jiff::Timestamp::now();
        let (reply_to, reply_peer, quote) = match reply {
            Some(r) => (Some(r.id), r.peer, r.quote),
            None => (None, None, None),
        };
        let q = Queued { key, peer, text, reply_to, silent: opts.silent, ts: ts.as_second(), failed: false, no_webpage: opts.no_webpage, quote, reply_peer, media };
        let id = self.new_local_id();
        let msg = self.queued_msg(&q, id);
        self.outbox.push(Pending { q, id, attempts: 0, next_at: 0.0, in_flight: false });
        // Same path as an incoming message: the chat preview and order update too.
        self.on_new_message(msg);
        self.pump_outbox(self.ctx.input(|i| i.time));
    }

    /// The bubble of a queued message; a reply to another chat names that chat.
    pub(super) fn queued_msg(&self, q: &Queued, id: i32) -> Msg {
        let mut msg = q.to_msg(id);
        if let Some(from) = q.reply_peer {
            msg.reply_elsewhere = Some(self.chats.iter().find(|c| c.id == from).map(|c| c.title.clone()).unwrap_or_default());
        }
        msg
    }

    /// Sends what is due. Per chat only the oldest message goes out, so the order is kept.
    pub(super) fn pump_outbox(&mut self, now: f64) {
        let mut busy: Vec<PeerId> = Vec::new();
        let mut due = Vec::new();
        for p in self.outbox.iter_mut().filter(|p| !p.q.failed) {
            if busy.contains(&p.q.peer) {
                continue;
            }
            busy.push(p.q.peer);
            if !p.in_flight && now >= p.next_at {
                p.in_flight = true;
                p.attempts += 1;
                due.push(p.q.clone());
            }
        }
        // The demo answers inside `send`, so nothing from `outbox` is borrowed here.
        for q in due {
            // Files show their upload progress at every attempt.
            if let Some(m) = q.media.as_ref().filter(|_| !self.uploads.iter().any(|u| u.id == q.key as u64)) {
                let total = m.paths().iter().filter_map(|p| std::fs::metadata(p).ok()).map(|x| x.len()).sum();
                let name = q.to_msg(0).file.map(|f| f.0).unwrap_or_default();
                self.uploads.push(super::Upload { id: q.key as u64, name, done: 0, total });
            }
            self.send(Cmd::SendQueued(q));
        }
    }

    /// Queue from the previous run: bubbles come back, unfinished ones are sent again.
    pub(super) fn restore_outbox(&mut self, list: Vec<Queued>) {
        for q in list {
            if self.outbox.iter().any(|p| p.q.key == q.key) {
                continue;
            }
            let id = self.new_local_id();
            let msg = self.queued_msg(&q, id);
            if let Some(h) = self.history.get_mut(&q.peer) {
                h.push(msg);
            }
            self.outbox.push(Pending { q, id, attempts: 0, next_at: 0.0, in_flight: false });
        }
    }

    /// A fresh history page from the server replaces the list — put the queued bubbles back at the end.
    pub(super) fn keep_outbox(&mut self, peer: PeerId) {
        let queued: Vec<Msg> = self.outbox.iter().filter(|p| p.q.peer == peer).map(|p| self.queued_msg(&p.q, p.id)).collect();
        let Some(list) = self.history.get_mut(&peer) else { return };
        list.retain(|m| m.id >= 0);
        list.extend(queued);
    }

    fn drop_bubble(&mut self, peer: PeerId, id: i32) {
        if let Some(list) = self.history.get_mut(&peer) {
            list.retain(|m| m.id != id);
        }
    }

    pub(super) fn queue_sent(&mut self, key: i64, peer: PeerId) {
        if let Some(i) = self.outbox.iter().position(|p| p.q.key == key) {
            let p = self.outbox.remove(i);
            self.drop_bubble(peer, p.id);
        }
    }

    pub(super) fn queue_failed(&mut self, key: i64, error: String, retry: bool) {
        let now = self.ctx.input(|i| i.time);
        let Some(p) = self.outbox.iter_mut().find(|p| p.q.key == key) else { return };
        p.in_flight = false;
        if retry && p.attempts < MAX_ATTEMPTS {
            p.next_at = now + backoff(p.attempts);
            return;
        }
        p.q.failed = true;
        let q = p.q.clone();
        self.send(Cmd::StoreQueued(q));
        self.toast = Some((error, 0.0, true));
    }

    /// "Retry" from the bubble menu: the counter starts over.
    pub(super) fn retry_queued(&mut self, id: i32) {
        let Some(p) = self.outbox.iter_mut().find(|p| p.id == id) else { return };
        p.q.failed = false;
        p.attempts = 0;
        p.next_at = 0.0;
        self.pump_outbox(self.ctx.input(|i| i.time));
    }

    /// "Delete" from the bubble menu: the message is not sent at all.
    pub(super) fn discard_queued(&mut self, id: i32) {
        let Some(i) = self.outbox.iter().position(|p| p.id == id) else { return };
        let p = self.outbox.remove(i);
        self.drop_bubble(p.q.peer, p.id);
        self.send(Cmd::DropQueued(p.q.key));
        if p.in_flight {
            // The request is already on its way and may still arrive.
            self.toast = Some((l!("Видалено з черги; якщо вже надсилалось, може з'явитися", "Removed from the queue; if it was already sending, it may still appear").into(), 0.0, false));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{MAX_ATTEMPTS, backoff};

    #[test]
    fn backoff_grows_and_is_capped() {
        assert_eq!(backoff(1), 2.0);
        assert_eq!(backoff(2), 4.0);
        assert_eq!(backoff(10), 60.0);
        // All automatic attempts fit within about half a minute before the red mark.
        assert!((1..MAX_ATTEMPTS).map(backoff).sum::<f64>() <= 60.0);
    }
}
