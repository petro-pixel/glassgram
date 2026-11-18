//! A demo group bot that answers commands in "Rust Team" so that only we see the reply.

use super::*;
use crate::backend::{ButtonStyle, EphemeralMsg};

/// "Rust Team": commands sent there get an ephemeral answer.
pub(super) const GROUP: i64 = 2;

pub(super) fn answer(app: &mut App, peer: PeerId, text: &str) {
    let reply = format!("Hi! Only you can see this reply to {text}. The group rules: be kind, stay on topic, no spam. **Release notes** are pinned.");
    let (plain, ents) = format::parse(&reply);
    let b = |t: &str, data: &[u8]| Button { text: t.into(), action: ButtonAction::Callback(data.to_vec()), style: ButtonStyle::Plain };
    app.push_event(Ev::Ephemeral(EphemeralMsg {
        peer,
        id: 1 + rand_ish().rem_euclid(1000),
        from: "Rust Team Bot".into(),
        spans: format::from_entities(&plain, &ents, 0),
        text: plain,
        buttons: vec![vec![b("I agree", b"agree"), b("Show the release notes", b"notes")]],
    }));
}

pub(super) fn press(app: &mut App, peer: PeerId, id: i32, data: Vec<u8>) {
    let text = if data == b"agree" { "Thanks! Welcome to the team 🦀" } else { "Release 0.8 ships on Friday" };
    app.push_event(Ev::BotAnswer { text: text.into(), alert: false });
    if data == b"agree" {
        app.push_event(Ev::EphemeralGone { peer, ids: vec![id] });
    }
}
