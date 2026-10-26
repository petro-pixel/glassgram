//! Send queue: a message stays in the cache until the server accepts it.
//! The UI decides when and how often to retry; here it is a single attempt and a verdict.

use std::time::Duration;

use grammers_client::InvocationError;

use super::{Cmd, Ev, Queued, SendOpts, Worker, rich};
use crate::format;
use crate::i18n::lf;

/// Offline a request can hang for long, and backend commands run one by one — don't hold them up.
const SEND_TIMEOUT: Duration = Duration::from_secs(25);

/// Worth retrying: connection failures and temporary server errors yes, a refusal on the merits no.
fn retryable(e: &InvocationError) -> bool {
    match e {
        InvocationError::Rpc(r) => r.code >= 500 || r.code == 420 || r.name == "FLOOD_WAIT",
        _ => true,
    }
}

impl Worker {
    pub(super) async fn send_queued(&mut self, q: Queued) -> Result<(), String> {
        if let Some(c) = &self.out.cache {
            c.queue_put(&q);
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
        let opts = SendOpts { silent: q.silent, schedule: None, no_webpage: q.no_webpage };
        let sent = tokio::time::timeout(SEND_TIMEOUT, client.send_message(r, opts.apply(rich(&q.text).reply_to(q.reply_to)))).await;
        match sent {
            Ok(Ok(m)) => {
                let (plain, ents) = format::parse(&q.text);
                self.sent().emit_with(peer, &m, q.reply_to, opts, Some((&plain, &ents)));
                if let Some(c) = &self.out.cache {
                    c.queue_drop(key);
                }
                self.out.send(Ev::QueueSent { key, peer });
            }
            Ok(Err(e)) => self.out.send(fail(lf!("Не надіслано: {e}", "Not sent: {e}"), retryable(&e))),
            Err(_) => self.out.send(fail(lf!("Не надіслано: немає відповіді за {} с", "Not sent: no response in {} s", SEND_TIMEOUT.as_secs()), true)),
        }
        Ok(())
    }
}

impl Worker {
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
                Ok(())
            }
            _ => Ok(()),
        }
    }
}
