//! Send queue: a message shows up as a bubble right away and is retried with growing delays
//! while the network is down. The backend keeps it in the cache, so it survives a restart.

use grammers_client::session::types::PeerId;

use super::App;
use crate::backend::{Cmd, Queued, Quote, SendOpts};
use crate::i18n::l;

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

    /// Queue a text message: it appears in the chat immediately and is sent in the background.
    pub(super) fn enqueue(&mut self, peer: PeerId, text: String, reply_to: Option<i32>, quote: Option<Quote>, opts: &SendOpts) {
        let ts = jiff::Timestamp::now();
        // Nanoseconds keep the queue order; a collision within one run is avoided explicitly.
        let mut key = ts.as_nanosecond() as i64;
        if let Some(last) = self.outbox.iter().map(|p| p.q.key).max() {
            key = key.max(last + 1);
        }
        let q = Queued { key, peer, text, reply_to, silent: opts.silent, ts: ts.as_second(), failed: false, no_webpage: opts.no_webpage, quote };
        let id = self.new_local_id();
        let msg = q.to_msg(id);
        self.outbox.push(Pending { q, id, attempts: 0, next_at: 0.0, in_flight: false });
        // Same path as an incoming message: the chat preview and order update too.
        self.on_new_message(msg);
        self.pump_outbox(self.ctx.input(|i| i.time));
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
            if let Some(h) = self.history.get_mut(&q.peer) {
                h.push(q.to_msg(id));
            }
            self.outbox.push(Pending { q, id, attempts: 0, next_at: 0.0, in_flight: false });
        }
    }

    /// A fresh history page from the server replaces the list — put the queued bubbles back at the end.
    pub(super) fn keep_outbox(&mut self, peer: PeerId) {
        let Some(list) = self.history.get_mut(&peer) else { return };
        list.retain(|m| m.id >= 0);
        list.extend(self.outbox.iter().filter(|p| p.q.peer == peer).map(|p| p.q.to_msg(p.id)));
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
