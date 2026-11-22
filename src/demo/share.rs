//! Demo location and contact sharing: the card appears at once, with a made-up number.

use super::*;

pub(super) fn handle(app: &mut App, cmd: Cmd) {
    match cmd {
        Cmd::SendLocation { peer, lat, lon, reply_to, send_as } => {
            let geo = GeoInfo { lat, lon, title: String::new(), address: String::new(), live: false };
            app.push_event(Ev::NewMsg(Msg { geo: Some(geo), ..sendas::posted_as(replied(app, outgoing(peer, String::new(), None), &reply_to), send_as) }));
        }
        Cmd::SendContact { peer, user, reply_to, send_as } => {
            let name = demo_contacts().into_iter().find(|c| c.id == user).map_or_else(String::new, |c| c.title);
            let contact = Some((name, "+380 50 000 00 00".to_owned()));
            app.push_event(Ev::NewMsg(Msg { contact, ..sendas::posted_as(replied(app, outgoing(peer, String::new(), None), &reply_to), send_as) }));
        }
        _ => {}
    }
}
