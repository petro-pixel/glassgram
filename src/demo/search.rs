//! Demo search tabs: channels, apps and public posts with a daily limit.

use super::*;

/// Media search tab: shared photos and videos of all demo chats; with no match, all of them.
pub(super) fn search_media(app: &mut App, kind: SharedKind, q: &str) -> Vec<SearchHit> {
    let all: Vec<SearchHit> = app
        .demo_history()
        .map(|(p, (title, k), _)| (p, title.to_owned(), k))
        .collect::<Vec<_>>()
        .into_iter()
        .flat_map(|(p, title, k)| demo_shared(app, p, kind).into_iter().map(move |msg| SearchHit { chat: title.clone(), kind: k, msg }))
        .collect();
    let matching: Vec<SearchHit> = all.iter().filter(|h| h.msg.text.to_lowercase().contains(q) || h.chat.to_lowercase().contains(q)).cloned().collect();
    let hits = if matching.is_empty() { all } else { matching };
    let images = images();
    for h in &hits {
        if let Some((key, image)) = images.iter().find(|(k, _)| *k == ImgKey::Photo(h.msg.peer, h.msg.id)) {
            app.push_event(Ev::Image { key: key.clone(), image: image.clone() });
        }
    }
    hits
}

/// Channels, apps and posts tabs of the global search, without the network.
pub(super) fn handle(app: &mut App, cmd: Cmd) {
    let channel = |id: i64, title: &str, label: String| ChatInfo { id: PeerId::channel_unchecked(id), kind: ChatKind::Channel, contact: false, joined: false, can_post: false, ..person(0, title, &label, Presence::None) };
    let matches = |title: &str, q: &str| title.to_lowercase().contains(&q.to_lowercase());
    match cmd {
        Cmd::SearchChannels(query) => {
            let chats = [(61, "Rust Weekly", "@rustweekly", "12 480"), (62, "Design Daily", "@design_daily", "8 115"), (63, "Open Source UA", "@opensource_ua", "3 902"), (64, "Tech Without Noise", "@tech_calm", "21 300")]
                .into_iter()
                .filter(|(_, title, user, _)| matches(title, &query) || matches(user, &query) || query.chars().count() <= 2)
                .map(|(id, title, user, n)| channel(id, title, crate::i18n::lf!("{user} · підписників: {n}", "{user} · {n} subscribers")))
                .collect();
            app.push_event(Ev::ChannelResults { query, chats });
        }
        Cmd::SearchApps(query) => {
            let bots = [(81, "Glass Weather", "1 204 000"), (82, "Pixel Farm", "860 400"), (83, "Quiz Night", "312 900"), (84, "Daily Puzzle", "95 120"), (HELPER_BOT, "GlassHelperBot", "4 210")]
                .into_iter()
                .filter(|(_, title, _)| matches(title, &query))
                .map(|(id, title, n)| ChatInfo { kind: ChatKind::Bot, contact: false, ..person(id, title, &crate::i18n::lf!("{n} користувачів на місяць", "{n} monthly users"), Presence::None) })
                .collect();
            app.push_event(Ev::AppResults { query, bots });
        }
        // Three free searches a day; the quota counts down as in Telegram.
        Cmd::CheckPostsFlood(query) => {
            let remains = POSTS_LEFT.get();
            app.push_event(Ev::PostsFlood { query, flood: PostsFlood { free: false, total: 3, remains, wait_till: None, stars: 10 } });
        }
        Cmd::SearchPosts(query) => {
            let remains = (POSTS_LEFT.get() - 1).max(0);
            POSTS_LEFT.set(remains);
            let post = |id: i64, title: &str, mid: i32, text: String| SearchHit {
                chat: title.to_owned(),
                kind: ChatKind::Channel,
                msg: Msg { id: mid, out: false, text, time: "09:30".into(), day: "02.11".into(), ..Msg::placeholder(PeerId::channel_unchecked(id)) },
            };
            let hits = vec![
                post(61, "Rust Weekly", 812, format!("This week: {query} in the new release, async closures and faster builds")),
                post(62, "Design Daily", 450, format!("Roundup: how \"liquid glass\" changes {query} in interfaces")),
                post(64, "Tech Without Noise", 1203, format!("{query} in brief: what to know before updating")),
            ];
            let flood = PostsFlood { free: false, total: 3, remains, wait_till: Some((jiff::Timestamp::now().as_second() + 5 * 3600) as i32), stars: 10 };
            app.push_event(Ev::PostResults { query, hits, flood: Some(flood), error: None });
        }
        _ => {}
    }
}

thread_local! {
    /// Free post searches left today in the demo.
    static POSTS_LEFT: std::cell::Cell<i32> = const { std::cell::Cell::new(3) };
}
