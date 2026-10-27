//! Send queue: a message stays in the cache until the server accepts it.
//! The UI decides when and how often to retry; here it is a single attempt and a verdict.

use std::time::Duration;

use grammers_client::session::types::PeerId;
use grammers_client::{InvocationError, tl};

use super::{Cmd, Ev, Queued, ReplyTo, SendOpts, Worker};
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
