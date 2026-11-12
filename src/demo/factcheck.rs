//! Demo fact checks and member tags: a "Tech News" post marked `need_check` gets its fact check
//! once shown, and the people in "Rust Team" carry member tags.

use grammers_client::session::types::PeerId;

use crate::app::App;
use crate::backend::{Ev, FactCheck, Msg};
use crate::format;

/// The channel post with a fact check.
const POST: i32 = 41;

/// Adds the fact-checked post to "Tech News" (4) and member tags to "Rust Team" (2).
pub(super) fn extras(chat: i64, msgs: &mut Vec<Msg>, base: impl Fn(i32, &str) -> Msg) {
    match chat {
        2 => {
            for m in msgs.iter_mut().filter(|m| !m.out) {
                m.rank = match m.sender.as_str() {
                    "Taras" => Some("maintainer".into()),
                    "Iryna" => Some("release lead".into()),
                    _ => None,
                };
            }
        }
        4 => {
            let text = "Scientists have proved that phone batteries last twice as long if you charge them only at night.";
            // The text comes later, from `getFactCheck`, as for a post whose check changed.
            let check = FactCheck { need_check: true, country: "ua".into(), ..FactCheck::default() };
            msgs.push(Msg { id: POST, text: text.into(), spans: format::from_entities(text, &[], 0), views: Some(5_100), factcheck: Some(check), ..base(31, "Tech News") });
        }
        _ => {}
    }
}

pub(super) fn answer(app: &mut App, peer: PeerId, ids: Vec<i32>) {
    let text = "There is no such study. Battery wear depends on heat and on how often it is charged to 100%, not on the time of day. \
Most phones already slow down charging overnight to protect the battery, so charging at night is fine, \
but so is topping up during the day. Keeping the phone out of the sun and away from hot car dashboards matters far more. \
Source: battery manufacturers' guidance and independent lab tests.";
    let checks = ids
        .into_iter()
        .filter(|id| *id == POST)
        .map(|id| (id, FactCheck { need_check: false, country: "ua".into(), text: text.into(), spans: format::from_entities(text, &[], 0) }))
        .collect();
    app.push_event(Ev::FactChecks { peer, checks });
}
