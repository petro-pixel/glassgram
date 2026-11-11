//! Demo sending as a channel: "Rust Team" lets us post as ourselves or as two channels we run,
//! one of them only with Premium; messages sent as a channel come back named after it.

use grammers_client::session::types::PeerId;

use crate::app::App;
use crate::backend::{Cmd, Ev, Msg, SendAsPeer};
use crate::i18n::{l, lp};

/// The demo group that offers a choice.
const GROUP: i64 = 2;

/// Channels we run: (id, title, subscribers, Premium only).
const CHANNELS: [(i64, &str, i32, bool); 2] = [(40, "Rust Team News", 1_240, false), (41, "Glassgram Dev", 312, true)];

pub(super) fn handle(app: &mut App, cmd: Cmd) {
    match cmd {
        Cmd::LoadSendAs(peer) if peer == PeerId::user_unchecked(GROUP) => {
            let me = PeerId::user_unchecked(super::ME);
            let mut peers = vec![SendAsPeer { id: me, title: "Demo User".into(), about: l!("особистий акаунт", "personal account").into(), premium: false }];
            peers.extend(CHANNELS.iter().map(|&(id, title, n, premium)| SendAsPeer {
                id: PeerId::channel_unchecked(id),
                title: title.into(),
                about: lp!(n, ["{n} підписник", "{n} підписники", "{n} підписників"], ["{n} subscriber", "{n} subscribers"]),
                premium,
            }));
            app.push_event(Ev::SendAs { peer, peers, current: Some(me) });
        }
        // Remembered by the app; there is no server to tell.
        _ => {}
    }
}

/// A sent message as the server echoes it: posted as one of our channels, it carries its name.
pub(super) fn posted_as(m: Msg, send_as: Option<PeerId>) -> Msg {
    match CHANNELS.iter().find(|c| Some(PeerId::channel_unchecked(c.0)) == send_as) {
        Some(&(_, title, ..)) => Msg { sender: title.into(), as_channel: true, ..m },
        None => m,
    }
}
