//! The call log, as in Telegram Desktop's "Calls" box: `messageActionPhoneCall` service messages
//! from every private chat, found by `messages.search` with `inputMessagesFilterPhoneCalls`.

use super::*;
use crate::i18n::lp;

/// Calls one request asks for.
pub const CALL_PAGE: i32 = 50;

/// A finished call as its service message tells it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct CallInfo {
    pub video: bool,
    pub end: CallEnd,
    /// Seconds; only answered calls have one.
    pub duration: Option<i32>,
}

/// Why the call ended (`PhoneCallDiscardReason`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum CallEnd {
    Hangup,
    /// Nobody answered (or we cancelled before they did).
    Missed,
    /// Declined, or the other side was in another call.
    Busy,
    Disconnect,
}

/// The arrow of a call log row.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CallDir {
    Out,
    In,
    Missed,
}

impl CallInfo {
    /// Telegram Desktop counts an incoming call that wasn't answered as missed; ours never are.
    pub fn dir(&self, out: bool) -> CallDir {
        match (out, self.end) {
            (true, _) => CallDir::Out,
            (false, CallEnd::Missed | CallEnd::Busy) => CallDir::Missed,
            (false, _) => CallDir::In,
        }
    }

    /// "Outgoing call", "Missed video call"…, as in the history bubble.
    pub fn label(&self, out: bool) -> &'static str {
        match (out, self.end, self.video) {
            (true, CallEnd::Missed, false) => l!("Скасований дзвінок", "Cancelled call"),
            (true, CallEnd::Missed, true) => l!("Скасований відеодзвінок", "Cancelled video call"),
            (true, _, false) => l!("Вихідний дзвінок", "Outgoing call"),
            (true, _, true) => l!("Вихідний відеодзвінок", "Outgoing video call"),
            (false, CallEnd::Missed, false) => l!("Пропущений дзвінок", "Missed call"),
            (false, CallEnd::Missed, true) => l!("Пропущений відеодзвінок", "Missed video call"),
            (false, CallEnd::Busy, false) => l!("Відхилений дзвінок", "Declined call"),
            (false, CallEnd::Busy, true) => l!("Відхилений відеодзвінок", "Declined video call"),
            (false, _, false) => l!("Вхідний дзвінок", "Incoming call"),
            (false, _, true) => l!("Вхідний відеодзвінок", "Incoming video call"),
        }
    }

    /// "5 minutes", "42 seconds"; empty for a call nobody answered.
    pub fn duration_text(&self) -> String {
        match self.duration.filter(|d| *d > 0) {
            None => String::new(),
            Some(s) if s < 60 => lp!(s, ["{n} секунда", "{n} секунди", "{n} секунд"], ["{n} second", "{n} seconds"]),
            Some(s) => {
                let n = s / 60;
                lp!(n, ["{n} хвилина", "{n} хвилини", "{n} хвилин"], ["{n} minute", "{n} minutes"])
            }
        }
    }
}

/// The call a service message is about.
pub(super) fn call_info(raw: &tl::enums::Message) -> Option<CallInfo> {
    let tl::enums::Message::Service(x) = raw else { return None };
    let tl::enums::MessageAction::PhoneCall(c) = &x.action else { return None };
    use tl::enums::PhoneCallDiscardReason as R;
    let end = match c.reason {
        Some(R::Missed) => CallEnd::Missed,
        Some(R::Busy) => CallEnd::Busy,
        Some(R::Disconnect) => CallEnd::Disconnect,
        Some(R::Hangup | R::MigrateConferenceCall(_)) | None => CallEnd::Hangup,
    };
    Some(CallInfo { video: c.video, end, duration: c.duration })
}

impl Worker {
    pub(super) async fn calls_cmd(&mut self, cmd: Cmd) -> Result<(), String> {
        let client = self.client()?;
        match cmd {
            Cmd::LoadCalls { offset_id } => {
                let req = tl::functions::messages::Search {
                    peer: tl::enums::InputPeer::Empty,
                    q: String::new(),
                    from_id: None,
                    saved_peer_id: None,
                    saved_reaction: None,
                    top_msg_id: None,
                    filter: tl::types::InputMessagesFilterPhoneCalls { missed: false }.into(),
                    min_date: 0,
                    max_date: 0,
                    offset_id,
                    add_offset: 0,
                    limit: CALL_PAGE,
                    max_id: 0,
                    min_id: 0,
                    hash: 0,
                };
                use tl::enums::messages::Messages as M;
                let (messages, users) = match client.invoke(&req).await.map_err(|e| lf!("Дзвінки: {e}", "Calls: {e}"))? {
                    M::Messages(m) => (m.messages, m.users),
                    M::Slice(m) => (m.messages, m.users),
                    M::ChannelMessages(m) => (m.messages, m.users),
                    M::NotModified(_) => (Vec::new(), Vec::new()),
                };
                let more = messages.len() as i32 >= CALL_PAGE;
                let mut people = HashMap::new();
                for raw in users {
                    let peer = Peer::User(User::from_raw(&client, raw));
                    if self.remember(&peer).await {
                        people.insert(peer.id(), (peer_title(&peer), peer_kind(&peer)));
                    }
                }
                let calls = messages
                    .iter()
                    .filter_map(|raw| {
                        let call = call_info(raw)?;
                        let tl::enums::Message::Service(x) = raw else { return None };
                        let peer = PeerId::from(&x.peer_id);
                        let (chat, kind) = people.get(&peer).cloned()?;
                        let ts = jiff::Timestamp::from_second(x.date as i64).unwrap_or_default();
                        let msg = Msg { id: x.id, out: x.out, time: clock(ts), ts: x.date as i64, day: day(ts), call: Some(call), ..Msg::placeholder(peer) };
                        Some(SearchHit { chat, kind, msg })
                    })
                    .collect();
                self.out.send(Ev::CallLog { calls, append: offset_id != 0, more });
            }
            Cmd::DeleteCalls { ids, revoke } => {
                // Private chats share one message id space, so a single request covers every peer.
                let id = ids.iter().map(|(_, id)| *id).collect();
                client
                    .invoke(&tl::functions::messages::DeleteMessages { revoke, id })
                    .await
                    .map_err(|e| lf!("Не вдалося видалити: {e}", "Could not delete: {e}"))?;
                self.out.send(Ev::CallsDeleted(ids));
            }
            _ => {}
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn call(out: bool, reason: Option<tl::enums::PhoneCallDiscardReason>, video: bool, duration: Option<i32>) -> tl::enums::Message {
        service(out, tl::types::MessageActionPhoneCall { video, call_id: 1, reason, duration }.into())
    }

    fn service(out: bool, action: tl::enums::MessageAction) -> tl::enums::Message {
        tl::types::MessageService {
            out,
            mentioned: false,
            media_unread: false,
            reactions_are_possible: false,
            silent: false,
            post: false,
            legacy: false,
            id: 7,
            from_id: None,
            peer_id: tl::types::PeerUser { user_id: 5 }.into(),
            saved_peer_id: None,
            reply_to: None,
            date: 0,
            action,
            reactions: None,
            ttl_period: None,
        }
        .into()
    }

    #[test]
    fn call_direction_and_label() {
        use tl::enums::PhoneCallDiscardReason as R;
        let missed = call_info(&call(false, Some(R::Missed), false, None)).unwrap();
        assert_eq!(missed.dir(false), CallDir::Missed);
        assert_eq!(missed.duration_text(), "");
        let declined = call_info(&call(false, Some(R::Busy), true, None)).unwrap();
        assert_eq!(declined.dir(false), CallDir::Missed);
        assert!(declined.video);
        let out = call_info(&call(true, Some(R::Hangup), false, Some(125))).unwrap();
        assert_eq!(out.dir(true), CallDir::Out);
        assert_eq!(out.end, CallEnd::Hangup);
        assert_eq!(out.duration, Some(125));
        // A cancelled outgoing call is still "outgoing" in the log.
        let cancelled = call_info(&call(true, Some(R::Missed), false, None)).unwrap();
        assert_eq!(cancelled.dir(true), CallDir::Out);
        assert_ne!(cancelled.label(true), out.label(true));
        assert!(call_info(&service(false, tl::enums::MessageAction::PinMessage)).is_none());
    }
}
