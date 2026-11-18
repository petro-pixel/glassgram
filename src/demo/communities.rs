//! A demo community that links the team's group, the news channel and the helper bot.

use super::*;
use crate::backend::Community;

const ID: i64 = 70;

pub(super) fn handle(app: &mut App, cmd: Cmd) {
    match cmd {
        Cmd::LoadCommunities => app.push_event(Ev::Communities(vec![Community { id: ID, access_hash: 1, title: "Rust Ukraine Community".into() }])),
        Cmd::LoadCommunityChats { id, .. } => {
            let chat = |id: i64, title: &str, kind| ChatInfo { kind, joined: true, ..person(id, title, "", Presence::None) };
            let chats = vec![chat(2, "Rust Team 🦀", ChatKind::Group), chat(4, "Tech News", ChatKind::Channel), helper_bot()];
            app.push_event(Ev::CommunityChats { id, about: "Everything about Rust in Ukraine: the team chat, news and a helper bot.".into(), chats });
        }
        _ => {}
    }
}
