//! Demo calls: call service messages in a few private chats, and the call log built from them.

use super::*;
use crate::backend::{CallEnd, CallInfo};

/// Calls of a demo chat: (id, outgoing, video, how it ended, seconds, time).
fn script(chat: i64) -> &'static [(i32, bool, bool, CallEnd, Option<i32>, (i8, i8))] {
    match chat {
        // Olena: a video call, then two missed ones in a row (one row "(2)" in the log).
        1 => &[(200, true, true, CallEnd::Hangup, Some(252), (15, 10)), (201, false, false, CallEnd::Missed, None, (15, 40)), (202, false, false, CallEnd::Missed, None, (15, 41))],
        // Andriy: we gave up before he answered, then he called back.
        5 => &[(200, true, false, CallEnd::Missed, None, (12, 58)), (201, false, false, CallEnd::Hangup, Some(1335), (13, 5))],
        // Maria: a short outgoing call and a declined incoming one.
        8 => &[(200, true, false, CallEnd::Hangup, Some(45), (11, 30)), (201, false, true, CallEnd::Busy, None, (11, 52))],
        _ => &[],
    }
}

/// Adds the chat's calls after its messages.
pub(super) fn add(chat: i64, peer: PeerId, msgs: &mut Vec<Msg>) {
    let today = jiff::Zoned::now().date();
    for &(id, out, video, end, duration, (h, m)) in script(chat) {
        let ts = today.at(h, m, 0, 0).to_zoned(jiff::tz::TimeZone::system()).map_or(0, |z| z.timestamp().as_second());
        msgs.push(Msg {
            id,
            out,
            time: format!("{h}:{m:02}"),
            ts,
            day: l!("Сьогодні", "Today").into(),
            call: Some(CallInfo { video, end, duration }),
            ..Msg::placeholder(peer)
        });
    }
}

pub(super) fn handle(app: &mut App, cmd: Cmd) {
    match cmd {
        Cmd::LoadCalls { .. } => {
            let mut calls: Vec<SearchHit> = app
                .demo_history()
                .flat_map(|(_, (title, kind), msgs)| msgs.iter().filter(|m| m.call.is_some()).map(move |m| SearchHit { chat: title.to_owned(), kind, msg: m.clone() }))
                .collect();
            calls.sort_by_key(|h| std::cmp::Reverse(h.msg.ts));
            app.push_event(Ev::CallLog { calls, append: false, more: false });
        }
        Cmd::DeleteCalls { ids, .. } => app.push_event(Ev::CallsDeleted(ids)),
        _ => {}
    }
}
