//! Demo attachment menu bots and website login: WeatherBot is in the 📎 menu, GlassHelperBot is
//! added from Maria's `startattach` link, and WeatherBot's "Log in" button asks to log in.

use grammers_client::session::types::PeerId;

use super::HELPER_BOT;
use crate::app::App;
use crate::backend::{AttachBot, AttachPeer, Cmd, Ev, UrlAuthAsk};

const WEATHER_BOT: i64 = 6;

fn weather() -> AttachBot {
    AttachBot {
        bot: PeerId::user_unchecked(WEATHER_BOT),
        name: "WeatherBot".into(),
        peer_types: vec![AttachPeer::SameBot, AttachPeer::User, AttachPeer::Group],
        inactive: false,
        write_access: false,
        in_menu: true,
    }
}

fn helper(inactive: bool) -> AttachBot {
    AttachBot {
        bot: PeerId::user_unchecked(HELPER_BOT),
        name: "GlassHelperBot".into(),
        peer_types: vec![AttachPeer::SameBot, AttachPeer::Bot, AttachPeer::User],
        inactive,
        write_access: true,
        in_menu: true,
    }
}

pub(super) fn handle(app: &mut App, cmd: Cmd) {
    match cmd {
        Cmd::LoadAttachBots { .. } => app.push_event(Ev::AttachBots { hash: 1, bots: vec![weather()] }),
        Cmd::OpenAttachLink { peer, username, start_param } if username.eq_ignore_ascii_case("GlassHelperBot") => {
            let added = app.demo_attach_bots().iter().any(|b| b.bot == PeerId::user_unchecked(HELPER_BOT));
            app.push_event(Ev::AttachBotFound { peer, bot: helper(!added), start_param });
        }
        Cmd::OpenAttachLink { username, .. } => app.push_event(Ev::Notice(format!("t.me/{username}?startattach"))),
        Cmd::ToggleAttachBot { bot, enabled, .. } => {
            let mut bots: Vec<AttachBot> = app.demo_attach_bots().into_iter().filter(|b| b.bot != bot).collect();
            if enabled && bot == PeerId::user_unchecked(HELPER_BOT) {
                bots.push(helper(false));
            }
            app.push_event(Ev::AttachBotToggled { bot, enabled });
            app.push_event(Ev::AttachBots { hash: 2, bots });
        }
        // No login links in the demo: messages keep their ordinary links.
        Cmd::LoadUrlAuthDomains => {}
        Cmd::RequestUrlAuth { from, url } => app.push_event(Ev::UrlAuthAsk(UrlAuthAsk {
            from,
            url,
            domain: "example.org".into(),
            bot_name: "WeatherBot".into(),
            write_access: true,
        })),
        Cmd::AcceptUrlAuth { url, .. } => app.push_event(Ev::UrlAuthOpen(format!("{url}#tgAuthResult=demo"))),
        _ => {}
    }
}
