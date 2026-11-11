//! Demo mode (`--demo`): made-up chats without network, to look at the interface.

use std::collections::HashMap;

use grammers_client::session::types::PeerId;

use crate::app::App;
use eframe::egui::{Color32, ColorImage};

use crate::backend::{AdminRights, ColorOption, InviteDraft, InviteLink, JoinRequest, PartList, Participant, SavedTag, PostsFlood, PasswordChange, PasswordState, ReportStep, Gift, Story, StoryPeer, StoryAlbum, StoryList, StoryViewer, STORY_PAGE, Birthday, ChatRights, GlobalPrivacy, WebAppKind, WebAppOpened, NotifyException, NotifyScope, Sponsored, SponsoredReport, MUTE_FOREVER, Profile, Queued, UsernameState, BotCmd, Mentionable, WebPageInfo, ChatDetails, Member, MemberRole, NotifyState, Ringtone, ReactFrom, ReactionNotify, Sound, SharedKind, Topic, Button, ButtonAction, GeoInfo, InlineResult, MediaInfo, MediaKind, PollInfo, PollVoter, TodoInfo, TodoTask, QueuedMedia, ReplyTo, PollOption, Reaction, ChatInfo, ChatKind, Cmd, Ev, Folder, ImgKey, Msg, Presence, SearchHit};
use crate::format;
use crate::i18n::{l, lf};

mod stickers;
mod stars;
mod custom_emoji;
mod effects;
mod music;
mod wallpapers;
mod langpack;
mod sendas;
mod aitext;
mod translate;
mod attach;

pub fn data() -> (String, Vec<ChatInfo>, HashMap<PeerId, Vec<Msg>>) {
    let chats = [
        (1, "Olena Koval", ChatKind::User, 2, true),
        (2, "Rust Team 🦀", ChatKind::Group, 14, true),
        (3, "Saved Messages", ChatKind::Saved, 0, true),
        (4, "Tech News", ChatKind::Channel, 128, false),
        (5, "Andriy", ChatKind::User, 0, false),
        (6, "WeatherBot", ChatKind::Bot, 0, false),
        (7, "Design Studio", ChatKind::Group, 3, false),
        (8, "Maria Shevchenko", ChatKind::User, 0, false),
        (9, "Old Project", ChatKind::Group, 0, false),
    ];
    let scripts: [&[(bool, &str, &str)]; 9] = [
        &[
            (false, "Olena", "Hi! Have you seen the new liquid glass design?"),
            (true, "", "Yes! I'm building a Telegram client in Rust with exactly that look 😄"),
            (false, "Olena", "Wow, how do you do the blur and refraction?"),
            (true, "", "A GPU blur, refraction at the rounded edges and a specular rim. Plus a live background with drifting colours, so the glass feels \"liquid\"."),
            (false, "Olena", "Can you show me a screenshot?"),
            (true, "", "📷"),
        ],
        &[
            (false, "Taras", "Who has updated to the new compiler yet?"),
            (false, "Iryna", "Me, everything built on the first try"),
            (true, "", "egui + grammers, pushing tonight"),
            (false, "Taras", "cargo clippy -- -D warnings 👀"),
            (false, "Iryna", "**Reminder:** the release is on __Friday__, ~~Thursday~~ is off. Details [in the tracker](https://codeberg.org/Lonami/grammers), run it with `cargo run --release`. ||A surprise for the team 🎁||"),
        ],
        &[(true, "", "Ideas: dark theme, reactions, voice messages"), (false, "Tech News", "Rust 1.95: faster builds and new const fn powers")],
        &[
            (false, "Tech News", "📷 Photo\nThe new Wi-Fi standard promises half the latency."),
            (false, "Tech News", "Linux 7.3 is out: a new scheduler and faster file systems."),
            (false, "Tech News", "Europe agrees on one charging port for laptops."),
        ],
        &[(false, "Andriy", "Call tomorrow at 10:00?"), (true, "", "Yes, deal 👍"), (false, "Andriy", "Here's [the quarterly report](https://example.org/q3-report), have a look before the call")],
        &[(true, "", "/weather Kyiv"), (false, "WeatherBot", "☀ Kyiv: +18°, clear, wind 3 m/s")],
        &[(false, "Sofia", "Dropped the mockups in Figma"), (false, "Sofia", "📄"), (false, "Maksym", "🐱"), (false, "Maksym", "Great, I'll take a look")],
        &[(false, "Maria", "Thanks for the help!"), (false, "Maria", "Here's the bot I told you about: [t.me/GlassHelperBot?start=from_maria](https://t.me/GlassHelperBot?start=from_maria)"), (false, "Maria", "It can also live in the 📎 menu: [t.me/GlassHelperBot?startattach](https://t.me/GlassHelperBot?startattach)")],
        &[(false, "Oleh", "Archived chat: nothing has happened here for ages")],
    ];

    let mut infos = Vec::new();
    let mut history = HashMap::new();
    for ((id, title, kind, unread, pinned), script) in chats.into_iter().zip(scripts) {
        let peer = PeerId::user_unchecked(id);
        let msgs: Vec<Msg> = script
            .iter()
            .enumerate()
            .map(|(i, (out, sender, text))| (i, out, sender, format::parse(text)))
            .map(|(i, out, sender, (text, ents))| Msg {
                id: i as i32 + 1,
                out: *out,
                sender: sender.to_string(),
                sender_color: demo_color(sender),
                spans: format::from_entities(&text, &ents, 0),
                comments: (id == 4 && !*out).then_some(3),
                // Pinned: each of these chats has something for the bar above history.
                pinned: matches!((id, i), (1, 3) | (2, 4) | (4, 0)),
                views: (id == 4).then_some(12_400),
                time: format!("{}:{:02}", 9 + i, (id * 7 + i as i64 * 13) % 60),
                day: l!("Сьогодні", "Today").into(),
                // In "Rust Team" Taras answers Iryna: the quote stripe takes her name colour.
                reply_to: match (id, i) {
                    (1, 3) => Some(3),
                    (2, 3) => Some(2),
                    _ => None,
                },
                photo: text == "📷",
                file: (text == "📄").then(|| ("Mockups_v3.fig".to_owned(), "14.2 MB".to_owned())),
                sticker: (text == "🐱").then(|| (900, "🐱".to_owned())),
                lottie: text == "🐱",
                text,
                ..Msg::placeholder(peer)
            })
            .map(|m| if m.file.is_some() || m.sticker.is_some() { Msg { text: String::new(), ..m } } else { m })
            .map(|m| if id == 3 && !m.out { Msg { forwarded: Some(m.sender.clone()), sender: String::new(), out: true, ..m } } else { m })
            .map(|m| if id == 6 && !m.out { Msg { buttons: weather_buttons(), ..m } } else { m })
            .map(|m| if m.photo { Msg { text: "This is how a photo looks in glass ✨".into(), ..m } } else { m })
            .collect();
        // The reply in the first chat quotes the last words of the message it answers.
        let quoted = msgs.iter().find(|m| m.id == 3).map(|m| {
            let words: Vec<&str> = m.text.split_whitespace().collect();
            words[words.len().saturating_sub(3)..].join(" ")
        });
        let msgs: Vec<Msg> = msgs.into_iter().map(|m| if id == 1 && m.reply_to == Some(3) { Msg { reply_quote: quoted.clone(), ..m } } else { m }).collect();
        let last = msgs.last().unwrap();
        infos.push(ChatInfo {
            id: peer,
            title: title.into(),
            kind,
            last_text: if last.out { lf!("Ви: {}", "You: {}", last.text) } else { last.text.replace('\n', " ") },
            last_time: last.time.clone(),
            last_ts: 100 - id,
            unread,
            pinned,
            contact: matches!(kind, ChatKind::User),
            muted: id == 4,
            archived: id == 9,
            unread_mark: false,
            draft: if id == 5 { "Don't forget the **slides**".into() } else { String::new() },
            presence: match id {
                1 => Presence::Online(i64::MAX),
                5 => Presence::Offline(jiff::Timestamp::now().as_second() - 3600),
                8 => Presence::Recently,
                _ => Presence::None,
            },
            forum: false,
            joined: true,
            can_post: kind != ChatKind::Channel,
            // Olena hasn't read our last messages yet: a single check mark is visible.
            read_out: if id == 1 { 4 } else { i32::MAX },
            // "Rust Team" was read up to the second message: it opens at the third, under "Unread messages".
            read_in: if id == 2 { 2 } else { 0 },
            // "Rust Team" has two mentions and a reaction for us — shows the "@" and "❤" buttons.
            unread_mentions: if id == 2 { 2 } else { 0 },
            unread_reactions: if id == 2 { 1 } else { 0 },
            // "Design Studio" has protected content: no forwarding or copying.
            noforwards: id == 7,
            can_pin: !matches!(kind, ChatKind::Channel) && id != 7,
            // The archived group is blocked by Telegram: its reason shows instead of the history.
            restricted: (id == 9).then(|| "This group is blocked for violating Telegram's Terms of Service.".to_owned()),
            // Olena wears an emoji status: a custom emoji after her name.
            emoji_status: (id == 1).then_some(custom_emoji::STATUS),
        });
        let mut msgs = msgs;
        extras(id, peer, &mut msgs);
        history.insert(peer, msgs);
    }
    let (forum, msgs) = forum_chat();
    history.insert(forum.id, msgs);
    infos.insert(3, forum);
    // A German group: the translate bar and whole-chat translation.
    let (german, msgs) = translate::chat();
    history.insert(german.id, msgs);
    infos.insert(4, german);
    let bot = helper_bot();
    history.insert(bot.id, Vec::new());
    infos.push(bot);
    ("Demo User".into(), infos, history)
}

pub fn folders() -> Vec<Folder> {
    vec![
        // Pinned chats inside a folder can be dragged too.
        Folder {
            id: 1,
            title: "Personal".into(),
            emoticon: "👤".into(),
            contacts: true,
            non_contacts: true,
            include: vec![PeerId::user_unchecked(8), PeerId::user_unchecked(5)],
            pinned: vec![PeerId::user_unchecked(8), PeerId::user_unchecked(5)],
            color: Some(3),
            ..Default::default()
        },
        Folder { id: 2, title: "Groups".into(), emoticon: "👥".into(), groups: true, color: Some(5), ..Default::default() },
        Folder { id: 3, title: "Channels & bots".into(), emoticon: "📢".into(), broadcasts: true, bots: true, color: Some(1), ..Default::default() },
        Folder { id: 4, title: "Unread".into(), emoticon: "✅".into(), contacts: true, non_contacts: true, groups: true, broadcasts: true, bots: true, exclude_read: true, ..Default::default() },
    ]
}

/// A sent reply as the server echoes it: the quote, and the title of the other chat it replies in.
fn replied(app: &App, m: Msg, reply: &Option<ReplyTo>) -> Msg {
    let Some(r) = reply else { return m };
    let chat = |p: PeerId| app.demo_history().find(|(x, _, _)| *x == p).map(|(_, (title, _), _)| title.to_owned()).unwrap_or_default();
    Msg { reply_to: Some(r.id), reply_quote: r.quote.as_ref().map(|q| q.text.clone()), reply_elsewhere: r.peer.map(chat), ..m }
}

fn outgoing(peer: PeerId, markup: String, reply_to: Option<i32>) -> Msg {
    let (text, ents) = format::parse(&markup);
    Msg {
        spans: format::from_entities(&text, &ents, 0),
        id: 10_000 + rand_ish(),
        out: true,
        text,
        time: jiff::Zoned::now().strftime("%H:%M").to_string(),
        ts: jiff::Timestamp::now().as_second(),
        day: l!("Сьогодні", "Today").into(),
        reply_to,
        ..Msg::placeholder(peer)
    }
}

pub fn handle(app: &mut App, cmd: Cmd) {
    let cmd_kind = match &cmd {
        Cmd::SendScheduledNow { .. } => Some(true),
        _ => None,
    };
    match cmd {
        Cmd::Send { peer, text, opts: crate::backend::SendOpts { schedule: Some(ts), .. }, .. } => {
            let mut list = app.demo_scheduled(peer);
            let t = jiff::Timestamp::from_second(ts).unwrap_or_default().to_zoned(jiff::tz::TimeZone::system());
            list.push(Msg { id: rand_ish(), ts, time: t.strftime("%H:%M").to_string(), day: t.strftime("%d.%m").to_string(), ..outgoing(peer, text, None) });
            app.push_event(Ev::Scheduled { peer, msgs: list });
            app.push_event(Ev::Notice(l!("Заплановано", "Scheduled").into()));
        }
        Cmd::DeleteScheduled { peer, ids } | Cmd::SendScheduledNow { peer, ids } => {
            let (gone, keep): (Vec<Msg>, Vec<Msg>) = app.demo_scheduled(peer).into_iter().partition(|m| ids.contains(&m.id));
            app.push_event(Ev::Scheduled { peer, msgs: keep });
            if matches!(cmd_kind, Some(true)) {
                for m in gone {
                    app.push_event(Ev::NewMsg(Msg { day: l!("Сьогодні", "Today").into(), ..m }));
                }
            }
        }
        Cmd::InlineQuery { peer, bot, query } => {
            let results = (1..=4)
                .map(|i| InlineResult { id: i.to_string(), title: format!("{query} — result {i}"), description: format!("From @{bot}"), kind: "article".into() })
                .collect();
            app.push_event(Ev::InlineResults { peer, bot, query, query_id: 1, results });
        }
        Cmd::SendInline { peer, result, send_as, .. } => app.push_event(Ev::NewMsg(sendas::posted_as(outgoing(peer, format!("Inline result #{result}"), None), send_as))),
        Cmd::Send { peer, text, .. } if peer == PeerId::user_unchecked(HELPER_BOT) && text.starts_with('/') => {
            app.push_event(Ev::NewMsg(outgoing(peer, text.clone(), None)));
            let answer = match text.as_str() {
                "/start" => "Hi! I'm the Glassgram demo bot. Try /help or the menu button next to the message field.",
                "/help" => "Commands: /start, /help, /about, or pick one from the \"/\" menu.",
                _ => "Glassgram is a Telegram client in Rust with \"liquid glass\".",
            };
            app.push_event(Ev::NewMsg(Msg { out: false, sender: "GlassHelperBot".into(), id: 20_000 + rand_ish(), ..outgoing(peer, answer.into(), None) }));
        }
        Cmd::WebPreview(text) => {
            let site = text.split("://").last().unwrap_or_default().split('/').next().unwrap_or_default().to_owned();
            let page = WebPageInfo {
                id: text.len() as i64,
                url: text.clone(),
                title: format!("{site} — demo page"),
                description: "In demo mode the preview is made up without the network.".into(),
                site,
                photo: false,
            };
            app.push_event(Ev::WebPreview { text, page: Some(page) });
        }
        Cmd::LoadMentions { peer, query } => {
            let q = query.to_lowercase();
            let members = MENTIONABLE
                .into_iter()
                .map(|(id, name, username, bot)| Mentionable { id: PeerId::user_unchecked(id), name: name.into(), username: username.into(), bot })
                .filter(|m| m.name.to_lowercase().contains(&q) || m.username.starts_with(&q))
                .collect();
            app.push_event(Ev::Mentions { peer, query, members });
        }
        Cmd::LoadBotCommands(peer) => {
            let list: &[(&str, &str, &str)] = match peer {
                p if p == PeerId::user_unchecked(6) => &[("weatherbot", "weather", "Weather now"), ("weatherbot", "forecast", "Weekly forecast"), ("weatherbot", "settings", "City and units")],
                p if p == PeerId::user_unchecked(HELPER_BOT) => &[("glasshelperbot", "start", "Start"), ("glasshelperbot", "help", "Help"), ("glasshelperbot", "about", "About Glassgram")],
                p if p == PeerId::user_unchecked(2) => &[("rustdocsbot", "docs", "Search the docs"), ("rustdocsbot", "crate", "Crate info")],
                _ => &[],
            };
            let commands = list.iter().map(|(bot, command, description)| BotCmd { bot: (*bot).into(), command: (*command).into(), description: (*description).into() }).collect();
            // The weather bot has a mini app behind its menu button.
            if peer == PeerId::user_unchecked(6) {
                app.push_event(Ev::BotMenuApp { peer, text: "Radar".into(), url: "https://example.org/radar".into() });
            }
            app.push_event(Ev::BotCommands { peer, commands });
        }
        Cmd::Send { peer, text, reply_to, send_as, .. } => {
            // In a forum, a reply goes into the topic of the message being replied to (or the topic itself).
            let topic = reply_to.as_ref().map(|r| r.id).filter(|_| peer == PeerId::channel_unchecked(FORUM)).map(|r| {
                app.demo_history().find(|(p, _, _)| *p == peer).and_then(|(_, _, msgs)| msgs.iter().find(|m| m.id == r).map(Msg::topic_id)).unwrap_or(r)
            });
            app.push_event(Ev::NewMsg(Msg { topic: topic.filter(|t| *t != 1), ..sendas::posted_as(replied(app, outgoing(peer, text, None), &reply_to), send_as) }));
        }
        Cmd::Forward { from, ids, to } => {
            let src = app.demo_history().find(|(p, _, _)| *p == from).map(|(_, (title, _), msgs)| (title.to_owned(), msgs.clone()));
            for m in src.iter().flat_map(|(title, msgs)| msgs.iter().filter(|m| ids.contains(&m.id)).map(move |m| (title, m))) {
                let from_name = if m.1.out { "Demo User".to_owned() } else if m.1.sender.is_empty() { m.0.clone() } else { m.1.sender.clone() };
                let mut copy = outgoing(to, String::new(), None);
                copy = Msg { text: m.1.text.clone(), spans: m.1.spans.clone(), photo: m.1.photo, file: m.1.file.clone(), forwarded: Some(from_name), id: copy.id + m.1.id, ..copy };
                app.push_event(Ev::NewMsg(copy));
            }
        }
        Cmd::PressButton { data, .. } => {
            let text = match data.as_slice() {
                b"tomorrow" => "Tomorrow in Kyiv: +21°, partly cloudy",
                _ => "Updated: +18°, clear",
            };
            app.push_event(Ev::BotAnswer { text: text.into(), alert: false });
        }
        cmd @ (Cmd::LoadStickers
        | Cmd::OpenStickerSet(_)
        | Cmd::InstallStickerSet { .. }
        | Cmd::SearchStickerSets(_)
        | Cmd::ReorderStickerSets(_)
        | Cmd::FaveSticker { .. }
        | Cmd::LoadSavedGifs
        | Cmd::SaveGif { .. }
        | Cmd::LoadStickerThumbs(_)
        | Cmd::SendSticker { .. }) => stickers::handle(app, cmd),
        cmd @ (Cmd::LoadWallPapers | Cmd::LoadChatThemes | Cmd::LoadChatWallPaper(_) | Cmd::DownloadWallPaper(_) | Cmd::SetChatWallPaper { .. } | Cmd::SetChatTheme { .. }) => wallpapers::handle(app, cmd),
        Cmd::SendDice { peer, emoji, reply_to, send_as } => {
            let top = match emoji.as_str() {
                "🎰" => 64,
                "🏀" | "⚽" => 5,
                _ => 6,
            };
            let value = rand_ish() % top + 1;
            app.push_event(Ev::NewMsg(Msg { dice: Some((emoji, value)), ..sendas::posted_as(replied(app, outgoing(peer, String::new(), None), &reply_to), send_as) }));
        }
        Cmd::SaveFolder(f) => {
            let mut list = app.demo_folders();
            match list.iter_mut().find(|x| x.id == f.id) {
                Some(x) => *x = f,
                None => list.push(f),
            }
            app.push_event(Ev::Folders(list));
        }
        Cmd::SetFolderTags(on) => app.push_event(Ev::FolderTags(on)),
        Cmd::LoadSuggestedFolders => {
            let suggested = vec![
                (Folder { title: "New".into(), emoticon: "🆕".into(), contacts: true, non_contacts: true, groups: true, broadcasts: true, bots: true, exclude_read: true, exclude_muted: true, ..Default::default() }, "Unread chats with notifications".into()),
                (Folder { title: "Bots".into(), emoticon: "🤖".into(), bots: true, ..Default::default() }, "Chats with bots".into()),
            ];
            let have: Vec<String> = app.demo_folders().into_iter().map(|f| f.title).collect();
            app.push_event(Ev::SuggestedFolders(suggested.into_iter().filter(|(f, _)| !have.contains(&f.title)).collect()));
        }
        Cmd::DeleteFolder(id) => {
            let list = app.demo_folders().into_iter().filter(|f| f.id != id).collect();
            app.push_event(Ev::Folders(list));
        }
        Cmd::OpenComments { peer, id, .. } => {
            // Discussion for the "Tech News" channel: a separate made-up group.
            let disc = PeerId::channel_unchecked(40);
            let c = |cid: i32, sender: &str, text: &str, reply: Option<i32>| Msg {
                id: cid,
                sender: sender.into(),
                text: text.into(),
                time: format!("10:{:02}", cid % 60),
                day: l!("Сьогодні", "Today").into(),
                reply_to: reply.or(Some(1)),
                ..Msg::placeholder(disc)
            };
            let msgs = vec![
                c(2, "Oleh", "Finally! Waiting for routers that support it.", None),
                c(3, "Marta", "Can phones do it already?", None),
                c(4, "Oleh", "Flagships can, since last year.", Some(3)),
            ];
            app.push_event(Ev::Comments { peer, post: id, discussion: disc, root: 1, msgs, older: false });
        }
        Cmd::LoadTopics(peer) => app.push_event(Ev::Topics { peer, topics: demo_topics() }),
        Cmd::CreateTopic { .. } | Cmd::EditTopic { .. } => app.push_event(Ev::Notice(l!("У демо-режимі теми не змінюються", "Topics don't change in demo mode").into())),
        Cmd::OpenTopic { peer, topic, before } => {
            let msgs = app
                .demo_history()
                .find(|(p, _, _)| *p == peer)
                .map(|(_, _, msgs)| msgs.iter().filter(|m| m.topic_id() == topic).cloned().collect())
                .unwrap_or_default();
            app.push_event(Ev::TopicHistory { peer, topic, msgs: if before.is_some() { Vec::new() } else { msgs }, older: before.is_some() });
        }
        Cmd::ExportChat { peer, .. } => {
            app.push_event(Ev::Exported { peer, dir: std::env::temp_dir(), count: 0 });
            app.push_event(Ev::Notice(l!("У демо-режимі експорт недоступний", "Export isn't available in demo mode").into()));
        }
        Cmd::LoadSessions => {
            let s = |hash, app: &str, device: &str, place: &str, active: &str, current| crate::backend::Session {
                hash,
                app: app.into(),
                device: device.into(),
                place: place.into(),
                active: active.into(),
                current,
            };
            app.push_event(Ev::Sessions(vec![
                s(1, "Glassgram 0.1", "Linux · Ubuntu", "Ukraine · 93.170.0.1", "now", true),
                s(2, "Telegram Android 11.2", "Pixel 7 · Android 15", "Ukraine · 46.211.0.7", "Today 09:41", false),
                s(3, "Telegram Desktop 5.6", "Windows 11", "Poland · 31.0.12.4", "12.10 18:02", false),
            ]));
        }
        Cmd::TerminateSession(_) | Cmd::TerminateOtherSessions => app.push_event(Ev::Notice(l!("У демо-режимі сеанси не завершуються", "Sessions aren't terminated in demo mode").into())),
        Cmd::LoadPrivacy => {
            use crate::backend::{Audience, PrivacyKey, PrivacyState};
            for key in PrivacyKey::ALL {
                let audience = match key {
                    PrivacyKey::Phone => Audience::Nobody,
                    PrivacyKey::LastSeen | PrivacyKey::Invites => Audience::Contacts,
                    _ => Audience::Everybody,
                };
                app.push_event(Ev::Privacy(key, PrivacyState { audience, exceptions: key == PrivacyKey::LastSeen }));
                // "Last seen": hidden from one colleague, shown to a whole group.
                let ex = if key == PrivacyKey::LastSeen {
                    let pick = |id: i64| app.demo_chats().iter().find(|c| c.id == PeerId::user_unchecked(id)).cloned();
                    crate::backend::PrivacyExceptions { allow: pick(7).into_iter().collect(), deny: pick(5).into_iter().collect() }
                } else {
                    Default::default()
                };
                app.push_event(Ev::PrivacyExceptions(key, ex));
            }
        }
        Cmd::SetPrivacy { key, audience, allow, deny } => {
            let exceptions = !allow.is_empty() || !deny.is_empty();
            app.push_event(Ev::Privacy(key, crate::backend::PrivacyState { audience, exceptions }));
            // Names come from the chat list and contacts the app already has.
            let find = |ids: &[PeerId]| ids.iter().filter_map(|p| app.demo_chats().iter().chain(demo_contacts().iter()).find(|c| c.id == *p).cloned()).collect::<Vec<_>>();
            let ex = crate::backend::PrivacyExceptions { allow: find(&allow), deny: find(&deny) };
            app.push_event(Ev::PrivacyExceptions(key, ex));
        }
        Cmd::Download { .. } => app.push_event(Ev::Notice(l!("У демо-режимі файли не завантажуються", "Files aren't downloaded in demo mode").into())),
        Cmd::SetAutoDownload(_) => {}
        Cmd::LoadPhoto { peer, id } => {
            if let Some((key, image)) = images().into_iter().find(|(k, _)| *k == ImgKey::Photo(peer, id)) {
                app.push_event(Ev::Image { key, image });
            }
        }
        // The cat sticker is animated: a small Lottie file of our own.
        Cmd::StreamMedia { peer, id } => handle(app, Cmd::LoadMedia { peer, id }),
        Cmd::LoadMedia { peer, id } if app.demo_history().any(|(p, _, msgs)| p == peer && msgs.iter().any(|m| m.id == id && m.lottie)) => match demo_tgs() {
            Some(path) => app.push_event(Ev::MediaReady { peer, id, path }),
            None => {}
        },
        Cmd::LoadMedia { peer, id } => match tone(id).or_else(|| clip(id)) {
            Some(path) => app.push_event(Ev::MediaReady { peer, id, path }),
            None => app.push_event(Ev::Notice(l!("Для демо-відео потрібен ffmpeg", "The demo video needs ffmpeg").into())),
        },
        Cmd::Delete { peer, ids } => app.push_event(Ev::Deleted { ids, peer: Some(peer) }),
        Cmd::LoadOlder { peer, .. } => app.push_event(Ev::History { peer, msgs: vec![], older: true }),
        Cmd::Logout => app.push_event(Ev::Error(l!("У демо-режимі вихід недоступний", "Logging out isn't available in demo mode").into())),
        Cmd::Search { peer, query, kind, from } => {
            let q = query.to_lowercase();
            let mut hits: Vec<SearchHit> = app
                .demo_history()
                .filter(|(p, _, _)| peer.is_none_or(|x| x == *p))
                .flat_map(|(_, chat, msgs)| msgs.iter().map(move |m| (chat, m)))
                .filter(|(_, m)| kind.is_none() && m.text.to_lowercase().contains(&q))
                .map(|((title, kind), m)| SearchHit { chat: title.to_owned(), kind, msg: m.clone() })
                .collect();
            // A type filter answers with the same messages as the profile's shared media tab.
            if let (Some(k), Some(p)) = (kind, peer) {
                let title = app.demo_chat(p).map(|c| (c.title.clone(), c.kind)).unwrap_or((String::new(), ChatKind::User));
                hits = demo_shared(app, p, k).into_iter().filter(|m| m.text.to_lowercase().contains(&q)).map(|m| SearchHit { chat: title.0.clone(), kind: title.1, msg: m }).collect();
            }
            // The Media search tab: photos and videos of every chat, by caption or chat name.
            if let (Some(k), None) = (kind, peer) {
                hits = search_media(app, k, &q);
            }
            // Demo messages carry only the sender's name: match it against the member picked.
            if let Some((_, name, _, _)) = from.and_then(|f| MENTIONABLE.into_iter().find(|m| PeerId::user_unchecked(m.0) == f)) {
                let first = name.split(' ').next().unwrap_or_default();
                hits.retain(|h| h.msg.sender == first || h.msg.sender == name);
            }
            app.push_event(Ev::SearchResults { peer, query, kind, from, hits });
        }
        Cmd::LoadPinned(peer) => {
            let msgs = app.demo_history().find(|(p, _, _)| *p == peer).map(|(_, _, msgs)| msgs.iter().filter(|m| m.pinned).cloned().collect()).unwrap_or_default();
            app.push_event(Ev::Pinned { peer, msgs });
        }
        Cmd::PinMessage { peer, id, unpin, .. } => app.push_event(Ev::PinnedChanged { peer, ids: vec![id], pinned: !unpin }),
        Cmd::UnpinAll(peer) => app.push_event(Ev::Pinned { peer, msgs: Vec::new() }),
        Cmd::LoadUnreadMarks { peer, reactions } => {
            let left = app.demo_chat(peer).map_or(0, |c| if reactions { c.unread_reactions } else { c.unread_mentions });
            let ids = match (peer == PeerId::user_unchecked(2), reactions) {
                (true, false) => vec![2, 4],
                (true, true) => vec![DEMO_REACTED],
                _ => Vec::new(),
            };
            let ids = if left > 0 { ids } else { Vec::new() };
            app.push_event(Ev::UnreadMarks { peer, reactions, ids });
        }
        Cmd::LoadNotifyExceptions(scope) => {
            let default = app.demo_notify_default(scope);
            app.push_event(Ev::NotifyDefault { scope, state: default });
            // Chats that differ from their category are its exceptions.
            let list = app
                .demo_chats()
                .iter()
                .filter(|c| NotifyScope::of(c.kind) == scope && c.muted != default.muted())
                .map(|c| NotifyException { peer: c.id, title: c.title.clone(), state: NotifyState { mute_until: if c.muted { MUTE_FOREVER } else { 0 }, silent: false, inherit: false, sound: None } })
                .collect();
            app.push_event(Ev::NotifyExceptions { scope, list });
        }
        Cmd::SetNotifyDefault { scope, muted } => {
            app.push_event(Ev::NotifyDefault { scope, state: NotifyState { mute_until: if muted { MUTE_FOREVER } else { 0 }, ..Default::default() } });
            handle(app, Cmd::LoadNotifyExceptions(scope));
        }
        Cmd::ResetNotify(peer) => {
            let Some(kind) = app.demo_chat(peer).map(|c| c.kind) else { return };
            let state = NotifyState { inherit: true, ..app.demo_notify_default(NotifyScope::of(kind)) };
            app.push_event(Ev::Notify { peer, state });
        }
        Cmd::LoadSponsored(peer) => {
            let ads = if peer == PeerId::user_unchecked(DEMO_CHANNEL) && !app.demo_profile().ads_off { vec![demo_ad(), demo_media_ad()] } else { Vec::new() };
            app.push_event(Ev::Sponsored { peer, ads, between: Some(2) });
        }
        Cmd::ViewSponsored(_) | Cmd::ClickSponsored(_) => {}
        Cmd::BotWriteAccess { bot, allow } => app.push_event(Ev::BotWriteAccess { bot, allowed: allow, asked: allow }),
        Cmd::SharePhone { bot } => app.push_event(Ev::PhoneShared { bot, sent: true }),
        Cmd::SetAdsEnabled(enabled) => app.push_event(Ev::Profile(Profile { ads_off: !enabled, ..app.demo_profile() })),
        Cmd::ReportSponsored { peer, random_id, option } => {
            let result = if option.is_empty() {
                let reasons = ["Not interested", "Misleading ad", "Scam", "Other"];
                SponsoredReport::Choose { title: "Why are you reporting this?".into(), options: reasons.iter().map(|r| (r.to_string(), r.as_bytes().to_vec())).collect() }
            } else {
                SponsoredReport::Reported
            };
            app.push_event(Ev::SponsoredReport { peer, random_id, result });
        }
        Cmd::Translate { peer, id, to } => {
            let text = app.demo_history().find(|(p, _, _)| *p == peer).and_then(|(_, _, msgs)| msgs.iter().find(|m| m.id == id)).map(|m| m.text.clone()).unwrap_or_default();
            // A couple of real translations, the rest just marked with the target language.
            let known = [
                ("Hi! Have you seen the new liquid glass design?", "Привіт! Ти бачив новий дизайн у стилі liquid glass?"),
                ("Wow, how do you do the blur and refraction?", "Ого, а як ти робиш розмиття та заломлення?"),
            ];
            let translated = match known.iter().find(|(en, _)| *en == text) {
                Some((en, uk)) => if to == "uk" { uk.to_string() } else { en.to_string() },
                None => format!("[{to}] {text}"),
            };
            app.push_event(Ev::Translated { peer, id, to, result: Ok((translated, Vec::new())) });
        }
        Cmd::Transcribe { peer, id } => aitext::transcribe(app, peer, id),
        Cmd::Summarize { peer, id } => aitext::summarize(app, peer, id),
        Cmd::JumpToDate { peer, date } => {
            let msgs = app.demo_history().find(|(p, _, _)| *p == peer).map(|(_, _, msgs)| msgs.clone()).unwrap_or_default();
            let id = msgs.iter().find(|m| m.ts >= date as i64).or(msgs.last()).map(|m| m.id);
            app.push_event(Ev::DateFound { peer, id });
        }
        Cmd::LoadReadBy { peer, id } => {
            let readers = [("Taras", "Today 11:02"), ("Iryna", "Today 11:15"), ("Ostap", "Today 12:40")];
            app.push_event(Ev::ReadBy { peer, id, readers: readers.iter().map(|(n, t)| (n.to_string(), t.to_string())).collect() });
        }
        Cmd::LoadChatInfo(peer) => {
            let kind = app.demo_history().find(|(p, _, _)| *p == peer).map(|(_, (_, k), _)| k);
            let mut details = demo_details(peer, kind);
            details.counts = SharedKind::ALL.into_iter().map(|k| (k, demo_shared(app, peer, k).len() as i32)).collect();
            app.push_event(Ev::ChatDetails { peer, details });
            // Olena's profile photo is a video.
            if peer == PeerId::user_unchecked(1)
                && let Some(path) = clip(87)
            {
                app.push_event(Ev::AvatarVideo { peer, path });
            }
        }
        Cmd::LoadMembers { peer, query, admins } => {
            let q = query.to_lowercase();
            let list = demo_members().into_iter().filter(|m| (!admins || m.role != MemberRole::Member) && m.chat.title.to_lowercase().contains(&q)).collect();
            app.push_event(Ev::Members { peer, query, admins, list });
        }
        Cmd::ReloadImages(keys) => {
            for (key, image) in images().into_iter().filter(|(k, _)| keys.contains(k)) {
                app.push_event(Ev::Image { key, image });
            }
        }
        Cmd::LoadStories => {
            // Olena and the channel have unseen stories, Andriy's were watched.
            let list = [(1, vec![1, 2, 3], 0), (4, vec![7], 0), (5, vec![4, 5], 5)]
                .into_iter()
                .filter_map(|(id, ids, max_read)| app.demo_chat(PeerId::user_unchecked(id)).cloned().map(|chat| StoryPeer { chat, ids, stories: Vec::new(), max_read }))
                .collect();
            app.push_event(Ev::Stories(list));
        }
        Cmd::LoadPeerStories(peer) => {
            let ids = app.stories_ids(peer);
            let now = jiff::Timestamp::now().as_second();
            let captions = ["The new chat screen in glass ✨", "", "Sunset from the balcony 🌅", "Rust 1.95 is here", "", "", "Tech week in review"];
            let stories = ids
                .iter()
                .enumerate()
                .map(|(k, id)| Story { id: *id, date: now - 3600 * (ids.len() - k) as i64, video: *id == 2, duration: if *id == 2 { 5.0 } else { 0.0 }, caption: captions.get(*id as usize - 1).copied().unwrap_or_default().into(), reaction: None, views: None, reactions: 0 })
                .collect();
            for id in &ids {
                app.push_event(Ev::Image { key: ImgKey::Story(peer, *id), image: story_picture(*id) });
            }
            let max_read = if peer == PeerId::user_unchecked(5) { 5 } else { 0 };
            app.push_event(Ev::PeerStories { peer, stories, max_read });
        }
        Cmd::ReadStories { .. } | Cmd::ReactStory { .. } => {}
        Cmd::LoadProfileStories { peer, list, offset } => {
            let all = profile_story_ids(peer, list);
            let rest: Vec<i32> = match list {
                StoryList::Album(_) => all.iter().copied().skip(offset as usize).collect(),
                _ => all.iter().copied().filter(|id| offset == 0 || *id < offset).collect(),
            };
            let page: Vec<i32> = rest.iter().copied().take(STORY_PAGE as usize).collect();
            let now = jiff::Timestamp::now().as_second();
            let own = peer == PeerId::user_unchecked(ME);
            let captions = ["Morning in the Carpathians 🏔", "", "Glassgram in light theme", "", "Coffee and code ☕"];
            let stories = page
                .iter()
                .map(|&id| {
                    let video = id % 10 == 4;
                    // Our own stories carry the counters, as the server sends them only to the owner.
                    let views = own.then_some((id * 37) % 180 + 12);
                    Story { id, date: now - 86_400 * (1 + (id % 100) as i64), video, duration: if video { 5.0 } else { 0.0 }, caption: captions[id as usize % captions.len()].into(), reaction: None, views, reactions: if own { id % 9 } else { 0 } }
                })
                .collect();
            for id in &page {
                app.push_event(Ev::Image { key: ImgKey::Story(peer, *id), image: story_picture(*id) });
            }
            let next = match list {
                StoryList::Album(_) => offset + page.len() as i32,
                _ => page.iter().copied().min().unwrap_or(0),
            };
            let done = rest.len() <= STORY_PAGE as usize;
            app.push_event(Ev::ProfileStories { peer, list, stories, total: all.len() as i32, next, done, append: offset != 0 });
        }
        Cmd::LoadStoryAlbums(peer) => {
            let albums = match peer.bot_api_dialog_id_unchecked() {
                1 => vec![(1, "Travel ✈"), (2, "Glassgram")],
                4 => vec![(3, "Releases")],
                ME => vec![(4, "Kyiv")],
                _ => Vec::new(),
            };
            let albums = albums.into_iter().map(|(id, title)| StoryAlbum { id, title: title.into() }).collect();
            app.push_event(Ev::StoryAlbums { peer, albums });
        }
        Cmd::LoadStoryViewers { peer, id, offset: _ } => {
            let now = jiff::Timestamp::now().as_second();
            let people = [(1, "Olena Koval", Some("❤")), (8, "Maria Shevchenko", Some("🔥")), (5, "Andriy", None), (21, "Taras Bondar", None), (22, "Iryna Melnyk", Some("👍")), (23, "Dmytro Hnatiuk", None)];
            let viewers = people
                .iter()
                .enumerate()
                .map(|(k, (uid, name, r))| StoryViewer { peer: PeerId::user_unchecked(*uid), name: (*name).into(), date: now - 600 * (k as i64 + 1), reaction: r.map(Into::into) })
                .collect();
            let views = (id * 37) % 180 + 12;
            app.push_event(Ev::StoryViewers { peer, id, viewers, views, reactions: id % 9, next: String::new(), append: false });
        }
        // Story 2 is a video: an upright test picture with a tone.
        Cmd::LoadStoryVideo { peer, id } => match clip(86) {
            Some(path) => app.push_event(Ev::StoryVideo { peer, id, path }),
            None => app.push_event(Ev::Notice(l!("Для демо-відео потрібен ffmpeg", "The demo video needs ffmpeg").into())),
        },
        Cmd::ReplyStory { .. } => app.push_event(Ev::Notice(l!("Відповідь надіслано", "Reply sent").into())),
        Cmd::LoadMediaDays { peer, before } => {
            // A few days with photos in the month before `before`.
            let days = [9, 10, 17, 24, 30].into_iter().map(|back| (before - back * 86_400 - 3 * 3600, (back % 4 + 1) as i32)).collect();
            app.push_event(Ev::MediaDays { peer, days });
        }
        Cmd::LoadSavedDialogs => {
            // Saved Messages (id 3) hold our own note, a channel post and a photo from Olena.
            let list = [3, 4, 1].into_iter().filter_map(|id| app.demo_chat(PeerId::user_unchecked(id)).cloned()).collect();
            app.push_event(Ev::SavedDialogs(list));
        }
        Cmd::LoadSavedTags => app.push_event(Ev::SavedTags(SAVED_TAGS.with_borrow(Clone::clone))),
        Cmd::RenameSavedTag { emoji, title } => {
            SAVED_TAGS.with_borrow_mut(|tags| tags.iter_mut().filter(|t| t.emoji == emoji).for_each(|t| t.title = title.clone()));
            app.push_event(Ev::SavedTags(SAVED_TAGS.with_borrow(Clone::clone)));
        }
        Cmd::LoadSavedTagged { saved, emoji } => {
            // 👍 marks the note and the channel post, ❤ the note only.
            let ids: &[i32] = if emoji == "👍" { &[1, 2] } else { &[1] };
            let msgs = app
                .demo_history()
                .find(|(p, _, _)| *p == saved)
                .map(|(_, _, m)| m.iter().filter(|m| ids.contains(&m.id)).cloned().collect::<Vec<_>>())
                .unwrap_or_default()
                .into_iter()
                .map(|mut m| {
                    m.reactions = SAVED_TAGS.with_borrow(|t| t.iter().filter(|t| t.emoji == "👍" || m.id == 1).map(|t| Reaction { emoji: t.emoji.clone(), count: 1, mine: true }).collect());
                    m
                })
                .collect();
            app.push_event(Ev::SavedTagged(msgs));
        }
        Cmd::LoadSavedHistory { saved, source, .. } => {
            let mut msgs: Vec<Msg> = app.demo_history().find(|(p, _, _)| *p == saved).map(|(_, _, m)| m.clone()).unwrap_or_default();
            // Message 1 is our note, 2 the channel post.
            for m in &mut msgs {
                m.saved_from = Some(PeerId::user_unchecked(if m.id == 1 { 3 } else { 4 }));
            }
            msgs.push(Msg {
                id: 900,
                out: true,
                forwarded: Some("Olena Koval".into()),
                saved_from: Some(PeerId::user_unchecked(1)),
                ..outgoing(saved, "Mockup of the new settings screen, have a look tonight".into(), None)
            });
            msgs.retain(|m| m.saved_from == Some(source));
            app.push_event(Ev::SavedHistory { source, msgs, older: false });
        }
        // Reasons first, then a comment for "Other", like the server.
        Cmd::Report { peer, ids, option, .. } => {
            let step = match option.as_slice() {
                [] => ReportStep::Choose {
                    title: String::new(),
                    options: [("Spam", b"s"), ("Violence", b"v"), ("Scam", b"f"), ("Other", b"o")].into_iter().map(|(t, o)| (t.to_owned(), o.to_vec())).collect(),
                },
                b"o" => ReportStep::Comment { option: b"o2".to_vec(), optional: false },
                _ => ReportStep::Done,
            };
            app.push_event(Ev::ReportStep { peer, ids, step });
        }
        Cmd::LoadCustomEmoji(ids) => custom_emoji::load(app, ids),
        Cmd::LoadEffects => effects::load(app),
        Cmd::LoadPeerColors => app.push_event(Ev::PeerColors { names: peer_colors(), profiles: Vec::new() }),
        Cmd::LoadEmojiStatuses => app.push_event(Ev::EmojiStatuses(custom_emoji::statuses())),
        Cmd::SetNameColor { profile, color, emoji } => {
            let p = app.demo_profile();
            let p = if profile { Profile { profile_color: (color, emoji), ..p } } else { Profile { color: (color, emoji), ..p } };
            app.push_event(Ev::Profile(p));
            app.push_event(Ev::Notice(l!("Колір збережено", "Color saved").into()));
        }
        Cmd::SetEmojiStatus(emoji_status) => app.push_event(Ev::Profile(Profile { emoji_status, ..app.demo_profile() })),
        Cmd::LoadEmojiKeywords(lang) => {
            let w = |k: &str, e: &[&str]| (k.to_owned(), e.iter().map(|x| x.to_string()).collect());
            let words = if lang == "uk" {
                vec![w("серце", &["❤", "💖", "💕"]), w("сміх", &["😂", "🤣", "😆"]), w("сонце", &["☀", "🌞"]), w("кіт", &["🐱", "😺"]), w("ракета", &["🚀"])]
            } else {
                vec![w("heart", &["❤", "💖", "💕"]), w("laugh", &["😂", "🤣"]), w("sun", &["☀", "🌞"]), w("cat", &["🐱", "😺"]), w("rocket", &["🚀"])]
            };
            app.push_event(Ev::EmojiKeywords { words });
        }
        Cmd::LoadGifts(peer) => {
            let gift = |title: &str, stars: i64, from: &str, note: &str| Gift { title: title.into(), stars, sticker: None, from: from.into(), note: note.into(), date: 0 };
            let gifts = vec![
                gift("", 50, "Andriy", "Happy birthday! 🎂"),
                gift("Plush Pepe #1842", 0, "", ""),
                gift("", 25, "Maria Shevchenko", ""),
                gift("", 100, "Rust Team", "Thanks for the release"),
                gift("", 15, "", ""),
            ];
            app.push_event(Ev::Gifts { peer, gifts, total: 5 });
        }
        cmd @ (Cmd::SearchChannels(_) | Cmd::SearchApps(_) | Cmd::CheckPostsFlood(_) | Cmd::SearchPosts(_)) => search_tabs(app, cmd),
        cmd @ (Cmd::LoadSendAs(_) | Cmd::SaveSendAs { .. }) => sendas::handle(app, cmd),
        cmd @ (Cmd::LoadMusic(_) | Cmd::LoadSavedMusic(_) | Cmd::LoadSavedTrack { .. }) => music::handle(app, cmd),
        cmd @ (Cmd::LoadTranslationOffer(_) | Cmd::TranslateMany { .. } | Cmd::TogglePeerTranslations { .. }) => translate::handle(app, cmd),
        cmd @ (Cmd::LoadAttachBots { .. } | Cmd::OpenAttachLink { .. } | Cmd::ToggleAttachBot { .. } | Cmd::LoadUrlAuthDomains | Cmd::RequestUrlAuth { .. } | Cmd::AcceptUrlAuth { .. } | Cmd::DeclineUrlAuth { .. }) => attach::handle(app, cmd),
        Cmd::LoadSimilarChannels(peer) => {
            let chats = [(61, "Rust Weekly", 12480), (62, "Design Daily", 8115), (63, "Open Source UA", 3902)]
                .into_iter()
                .map(|(id, title, n)| ChatInfo {
                    id: PeerId::channel_unchecked(id),
                    kind: ChatKind::Channel,
                    contact: false,
                    joined: false,
                    can_post: false,
                    ..person(0, title, &crate::i18n::lp!(n, ["{n} підписник", "{n} підписники", "{n} підписників"], ["{n} subscriber", "{n} subscribers"]), Presence::None)
                })
                .collect();
            // Without Premium the server sends a few and counts the rest.
            app.push_event(Ev::SimilarChannels { peer, chats, total: 8 });
        }
        Cmd::ExportInvite(peer) => app.push_event(Ev::Invite { peer, link: "https://t.me/+GlassgramDemo".into() }),
        Cmd::LoadShared { peer, kind, before } => {
            let msgs = if before.is_some() { Vec::new() } else { demo_shared(app, peer, kind) };
            app.push_event(Ev::Shared { peer, kind, msgs, older: before.is_some() });
        }
        Cmd::SetNotify { peer, mute_until, silent, sound } => app.push_event(Ev::Notify { peer, state: NotifyState { mute_until, silent, inherit: false, sound } }),
        Cmd::SetProxy(_) => {}
        // Made-up pings; a proxy on port 1 never answers.
        Cmd::CheckProxy(link) => {
            let result = if link.contains("port=1&") || link.ends_with(":1") { Err("timed out".to_owned()) } else { Ok(40 + (link.len() as u32 * 7) % 160) };
            app.push_event(Ev::ProxyChecked { link, result });
        }
        Cmd::LoadRingtones => {
            // No files: listening falls back to the built-in chime.
            let list = app.demo_ringtones().unwrap_or_else(|| vec![Ringtone { id: 7001, title: "Marimba".into(), duration: 2.0 }, Ringtone { id: 7002, title: "Glass bell".into(), duration: 3.0 }]);
            app.push_event(Ev::Ringtones(list));
        }
        Cmd::UploadRingtone(path) => {
            let mut list = app.demo_ringtones().unwrap_or_default();
            let title = path.file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_default();
            list.push(Ringtone { id: 7000 + list.len() as i64 + 10, title, duration: 4.0 });
            app.push_event(Ev::Ringtones(list));
        }
        Cmd::DeleteRingtone(id) => {
            let list = app.demo_ringtones().unwrap_or_default().into_iter().filter(|r| r.id != id).collect();
            app.push_event(Ev::Ringtones(list));
        }
        Cmd::LoadReactionNotify => app.push_event(Ev::ReactionNotify(app.demo_reaction_notify().unwrap_or(ReactionNotify { messages: ReactFrom::All, stories: ReactFrom::Contacts, polls: ReactFrom::All, previews: true, sound: Sound::Default }))),
        Cmd::SetReactionNotify(s) => app.push_event(Ev::ReactionNotify(s)),
        Cmd::LeaveChat(peer) | Cmd::DeleteChat { peer, .. } => app.push_event(Ev::ChatRemoved(peer)),
        Cmd::ClearHistory { peer, .. } => app.push_event(Ev::HistoryCleared(peer)),
        Cmd::LoadProfile => {
            let p = app.demo_profile();
            app.push_event(Ev::Profile(if p.id.is_some() { p } else { demo_profile() }));
        }
        Cmd::SaveProfile { first, last, bio } => {
            app.push_event(Ev::Profile(Profile { first, last, bio, ..app.demo_profile() }));
            app.push_event(Ev::Notice(l!("Профіль збережено (демо)", "Profile saved (demo)").into()));
        }
        // A few names are "taken" so every answer can be seen.
        Cmd::CheckUsername(name) => {
            let taken = ["durov", "telegram", "glassgram", "admin"].iter().any(|t| name.eq_ignore_ascii_case(t));
            app.push_event(Ev::UsernameCheck { name, state: if taken { UsernameState::Taken } else { UsernameState::Free } });
        }
        Cmd::SetUsername(username) => {
            app.push_event(Ev::Profile(Profile { username, ..app.demo_profile() }));
            app.push_event(Ev::Notice(l!("Ім'я користувача збережено (демо)", "Username saved (demo)").into()));
        }
        Cmd::SetProfilePhoto(path) => match image::open(&path) {
            Ok(img) => {
                let side = img.width().min(img.height());
                let img = img.crop_imm((img.width() - side) / 2, (img.height() - side) / 2, side, side).thumbnail(256, 256).to_rgba8();
                let image = ColorImage::from_rgba_unmultiplied([img.width() as usize, img.height() as usize], img.as_raw());
                app.push_event(Ev::Image { key: ImgKey::Avatar(PeerId::user_unchecked(ME)), image });
                app.push_event(Ev::Notice(l!("Фото профілю оновлено (демо)", "Profile photo updated (demo)").into()));
            }
            Err(e) => app.push_event(Ev::Error(e.to_string())),
        },
        // `#offline` in the text pretends there is no network: the clock, retries, then the red mark.
        Cmd::SendQueued(q) if q.text.contains("#offline") => {
            app.push_event(Ev::QueueFailed { key: q.key, error: l!("Не надіслано: немає мережі (демо)", "Not sent: no network (demo)").into(), retry: true });
        }
        Cmd::SendQueued(q) if q.media.is_some() => {
            queued_media(app, &q);
            app.push_event(Ev::UploadDone(q.key as u64));
            app.push_event(Ev::QueueSent { key: q.key, peer: q.peer });
        }
        Cmd::SendQueued(q) => {
            let topic = q.reply_to.filter(|_| q.peer == PeerId::channel_unchecked(FORUM)).map(|r| {
                app.demo_history().find(|(p, _, _)| *p == q.peer).and_then(|(_, _, msgs)| msgs.iter().find(|m| m.id == r).map(Msg::topic_id)).unwrap_or(r)
            });
            let reply = q.reply();
            app.push_event(Ev::NewMsg(Msg { topic: topic.filter(|t| *t != 1), effect: q.effect, ..sendas::posted_as(replied(app, outgoing(q.peer, q.text, None), &reply), q.send_as) }));
            app.push_event(Ev::QueueSent { key: q.key, peer: q.peer });
            // The team answers with a mention: it counts on the "@" button unless the chat is in view.
            if q.peer == PeerId::user_unchecked(2) {
                let m = Msg { out: false, sender: "Taras".into(), mentioned: true, id: 20_000 + rand_ish(), ..outgoing(q.peer, "@demo got it, thanks!".into(), None) };
                app.push_event(Ev::NewMsg(m));
            }
        }
        // The UI already shows the new order; there is no server to tell.
        Cmd::ReorderPinned { .. } => {}
        Cmd::ReorderFolders(order) => {
            let mut list = app.demo_folders();
            list.sort_by_key(|f| order.iter().position(|id| *id == f.id));
            app.push_event(Ev::Folders(list));
        }
        Cmd::OpenWebApp { peer, bot, kind, .. } => {
            let url = match &kind {
                WebAppKind::Inline(u) | WebAppKind::Simple(u) | WebAppKind::Menu(u) => u.clone(),
                WebAppKind::Main { .. } | WebAppKind::Named { .. } => "https://example.org/app".into(),
                WebAppKind::Attach { .. } => "https://example.org/attach".into(),
            };
            app.push_event(Ev::WebAppOpened(WebAppOpened { peer, bot, kind, url, query_id: None }));
        }
        Cmd::OpenBotStart { param, .. } => {
            app.push_event(Ev::ChatFound(helper_bot()));
            app.push_event(Ev::BotStart { peer: PeerId::user_unchecked(HELPER_BOT), param });
        }
        Cmd::StartBot { peer, param } => {
            app.push_event(Ev::NewMsg(outgoing(peer, "/start".into(), None)));
            let answer = format!("Hi! You came from a link with the parameter \"{param}\".");
            app.push_event(Ev::NewMsg(Msg { out: false, sender: "GlassHelperBot".into(), id: 20_000 + rand_ish(), ..outgoing(peer, answer, None) }));
        }
        Cmd::OpenWebAppLink { username, .. } => app.push_event(Ev::Notice(format!("t.me/{username}"))),
        Cmd::ProlongWebApp { .. } | Cmd::WebAppData { .. } | Cmd::WebAppMethod { .. } => {}
        Cmd::SendPoll { peer, poll, reply_to, send_as } => {
            let answers = poll
                .options
                .iter()
                .enumerate()
                .map(|(i, o)| PollOption { text: o.clone(), option: vec![i as u8], voters: 0, chosen: false, correct: poll.quiz == Some(i) })
                .collect();
            let info = PollInfo {
                question: poll.question,
                answers,
                quiz: poll.quiz.is_some(),
                multiple: poll.multiple,
                public: !poll.anonymous,
                open_answers: poll.open_answers,
                final_vote: poll.final_vote,
                hide_results: poll.hide_results,
                close_date: poll.close_period.map(|p| jiff::Timestamp::now().as_second() + p as i64),
                solution: Some(poll.solution).filter(|s| !s.is_empty()),
                ..PollInfo::default()
            };
            app.push_event(Ev::NewMsg(Msg { id: rand_ish(), poll: Some(info), ..sendas::posted_as(replied(app, outgoing(peer, String::new(), None), &reply_to), send_as) }));
        }
        Cmd::PollVotes { peer, id, .. } => {
            // A few made-up voters per answer, newest first.
            let names = ["Taras", "Iryna", "Olena", "Andriy", "Marko", "Sofia"];
            let poll = app.demo_history().find(|(p, _, _)| *p == peer).and_then(|(_, _, msgs)| msgs.iter().find(|m| m.id == id)?.poll.clone());
            let now = jiff::Timestamp::now().as_second();
            let voters = poll
                .iter()
                .flat_map(|p| p.answers.iter())
                .enumerate()
                .flat_map(|(k, a)| (0..a.voters.min(3) as usize).map(move |i| (k, i, a.option.clone())))
                .map(|(k, i, option)| {
                    let n = (k * 2 + i) % names.len();
                    PollVoter { peer: PeerId::user_unchecked(900 + n as i64), name: names[n].into(), options: vec![option], ts: now - (k * 3 + i) as i64 * 1700 }
                })
                .collect();
            app.push_event(Ev::PollVotes { peer, id, more: false, voters, next: None });
        }
        Cmd::AddPollAnswer { peer, id, text } => {
            let msg = app.demo_history().find(|(p, _, _)| *p == peer).and_then(|(_, _, msgs)| msgs.iter().find(|m| m.id == id).cloned());
            if let Some(mut m) = msg
                && let Some(poll) = &mut m.poll
            {
                let option = vec![poll.answers.len() as u8];
                poll.answers.push(PollOption { text, option, ..PollOption::default() });
                app.push_event(Ev::Edited(m));
            }
        }
        Cmd::SendTodo { peer, todo, reply_to, send_as } => {
            let items = todo.items.into_iter().enumerate().map(|(i, title)| TodoTask { id: i as i32 + 1, title, done: None }).collect();
            let info = TodoInfo { title: todo.title, items, others_can_append: todo.others_can_append, others_can_complete: todo.others_can_complete };
            app.push_event(Ev::NewMsg(Msg { id: rand_ish(), todo: Some(info), ..sendas::posted_as(replied(app, outgoing(peer, String::new(), None), &reply_to), send_as) }));
        }
        // The app ticks and adds items on its own; there is no server to confirm.
        Cmd::ToggleTodo { .. } | Cmd::AppendTodo { .. } => {}
        Cmd::LoadGlobalPrivacy => app.push_event(Ev::GlobalPrivacy(GlobalPrivacy { archive_new: true, keep_unmuted: false, keep_folders: true, hide_read: false, premium_only: false })),
        Cmd::SetGlobalPrivacy(p) => app.push_event(Ev::GlobalPrivacy(p)),
        Cmd::LoadCacheUsage | Cmd::ClearCache(_) | Cmd::TrimCache(_) => {
            use crate::backend::CacheKind as K;
            if let Cmd::ClearCache(kinds) = &cmd {
                DEMO_CLEARED.with_borrow_mut(|c| c.extend(kinds.iter().copied()));
            }
            let made_up = [(K::Photos, 184 << 20, 640), (K::Videos, 1210 << 20, 38), (K::Audio, 23 << 20, 51), (K::Stickers, 46 << 20, 412), (K::Profiles, 12 << 20, 230), (K::Other, 3 << 20, 4)];
            let usage = made_up.into_iter().map(|(k, b, n)| if DEMO_CLEARED.with_borrow(|c| c.contains(&k)) { (k, 0, 0) } else { (k, b, n) }).collect();
            app.push_event(Ev::CacheUsage(usage));
        }
        Cmd::LoadAccountTtl => app.push_event(Ev::AccountTtl(DEMO_ACCOUNT_TTL.get())),
        Cmd::SetAccountTtl(days) => {
            DEMO_ACCOUNT_TTL.set(days);
            app.push_event(Ev::AccountTtl(days));
        }
        Cmd::LoadDefaultTtl => app.push_event(Ev::DefaultTtl(DEMO_DEFAULT_TTL.get())),
        Cmd::SetDefaultTtl(period) => {
            DEMO_DEFAULT_TTL.set(period);
            app.push_event(Ev::DefaultTtl(period));
        }
        Cmd::SetHistoryTtl { peer, period } => {
            DEMO_TTL.with_borrow_mut(|t| t.insert(peer, period));
            app.push_event(Ev::HistoryTtl { peer, period });
        }
        Cmd::LoadAdminLog { peer, before } => {
            let e = |id: i64, date: &str, who: &str, what: &str, detail: &str| crate::backend::AdminLogEntry { id, date: date.into(), who: who.into(), what: what.into(), detail: detail.into() };
            let entries = if before.is_some() {
                Vec::new()
            } else {
                vec![
                    e(5, "29.10 12:40", "Olena Koval", "pinned a message", "Release on Friday, testing until Thursday"),
                    e(4, "29.10 11:02", "Andriy", "deleted a message", "Buy cheap subscribers!!!"),
                    e(3, "28.10 19:15", "Olena Koval", "changed the description", "We talk Rust, egui and grammers. No spam 🦀"),
                    e(2, "28.10 18:30", "Maria Shevchenko", "joined", ""),
                    e(1, "28.10 09:00", "Olena Koval", "slow mode", "30 s"),
                ]
            };
            app.push_event(Ev::AdminLog { peer, entries, older: before.is_some() });
        }
        Cmd::SetSignatures { .. } | Cmd::SetDiscussionGroup { .. } => app.push_event(Ev::Notice(crate::i18n::l!("Зміни збережено", "Changes saved").into())),
        Cmd::CheckChatlist(_) | Cmd::CheckChatlistUpdates(_) | Cmd::AskLeaveChatlist(_) => {
            use crate::backend::{Chatlist, ChatlistAction};
            let action = match cmd {
                Cmd::CheckChatlist(slug) => ChatlistAction::Join(slug),
                Cmd::CheckChatlistUpdates(id) => ChatlistAction::Update(id),
                Cmd::AskLeaveChatlist(id) => ChatlistAction::Leave(id),
                _ => return,
            };
            let chats = app.demo_chats().iter().filter(|c| matches!(c.kind, ChatKind::Group | ChatKind::Channel)).map(|c| (c.id, c.title.clone())).collect();
            app.push_event(Ev::Chatlist(Chatlist { action, title: "Rust Community".into(), emoticon: "🦀".into(), chats, already: 0 }));
        }
        Cmd::ApplyChatlist { .. } => app.push_event(Ev::Notice(crate::i18n::l!("Зміни збережено", "Changes saved").into())),
        Cmd::LoadDiscussionGroups(peer) => {
            let groups = app.demo_chats().iter().filter(|c| c.kind == ChatKind::Group).cloned().collect();
            app.push_event(Ev::DiscussionGroups { peer, groups });
        }
        cmd @ Cmd::LoadStars { .. } => stars::handle(app, cmd),
        cmd @ (Cmd::LoadLanguages | Cmd::LoadLangPack { .. }) => langpack::handle(app, cmd),
        Cmd::LoadWebsites | Cmd::LogoutWebsite(_) | Cmd::LogoutAllWebsites => {
            let gone = match &cmd {
                Cmd::LogoutWebsite(h) => vec![*h],
                Cmd::LogoutAllWebsites => vec![1, 2],
                _ => Vec::new(),
            };
            DEMO_WEB_GONE.with_borrow_mut(|g| g.extend(gone));
            let site = |hash, domain: &str, bot: &str, device: &str, place: &str, active: &str| crate::backend::WebSession { hash, domain: domain.into(), bot: bot.into(), device: device.into(), place: place.into(), active: active.into() };
            let list = [site(1, "example.org", "Example Login", "Firefox 140 · Linux", "Kyiv, Ukraine · 192.0.2.10", "yesterday 21:14"), site(2, "forum.example.net", "Forum Bot", "Chrome 141 · Windows", "Lviv, Ukraine · 198.51.100.7", "12.10.2026 09:30")];
            let list = list.into_iter().filter(|s| !DEMO_WEB_GONE.with_borrow(|g| g.contains(&s.hash))).collect();
            app.push_event(Ev::Websites(list));
        }
        Cmd::LoadContentSettings => app.push_event(Ev::ContentSettings { enabled: false, can_change: true }),
        Cmd::SetSensitive(enabled) => app.push_event(Ev::ContentSettings { enabled, can_change: true }),
        cmd @ (Cmd::LoadParticipants { .. } | Cmd::EditAdmin { .. } | Cmd::RestrictMember { .. } | Cmd::BanMember { .. } | Cmd::UnbanMember { .. }) => manage(app, cmd),
        cmd @ (Cmd::LoadInviteLinks(_) | Cmd::SaveInviteLink { .. } | Cmd::RevokeInviteLink { .. } | Cmd::DeleteInviteLink { .. } | Cmd::DeleteRevokedInvites(_) | Cmd::LoadJoinRequests(_) | Cmd::AnswerJoinRequest { .. }) => invites(app, cmd),
        cmd @ (Cmd::LoadPassword | Cmd::CheckPassword(_) | Cmd::UpdatePassword { .. } | Cmd::ConfirmPasswordEmail(_) | Cmd::ResendPasswordEmail | Cmd::CancelPasswordEmail) => password(app, cmd),
        Cmd::EditMedia { peer, id, text, path, as_file, spoiler, above } => {
            let Some(old) = app.demo_history().find(|(p, _, _)| *p == peer).and_then(|(_, _, msgs)| msgs.iter().find(|m| m.id == id)).cloned() else { return };
            let (plain, ents) = format::parse(&text);
            let m = Msg { text: plain.clone(), spans: format::from_entities(&plain, &ents, 0), edited: true, photo: false, file: None, media: None, spoiler, invert_media: above, ..old };
            // As the server answers: a picture becomes a photo, anything else a file.
            let picture = (!as_file).then(|| image::open(&path).ok()).flatten().map(|img| img.thumbnail(800, 800).to_rgba8());
            match picture {
                Some(rgba) => {
                    let size = [rgba.width() as usize, rgba.height() as usize];
                    app.push_event(Ev::MediaReplaced(Msg { photo: true, ..m }));
                    app.push_event(Ev::Image { key: ImgKey::Photo(peer, id), image: ColorImage::from_rgba_unmultiplied(size, rgba.as_raw()) });
                }
                None => {
                    let name = path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
                    let size = std::fs::metadata(&path).map(|x| crate::backend::human_size(x.len() as usize)).unwrap_or_default();
                    app.push_event(Ev::MediaReplaced(Msg { file: Some((name, size)), ..m }));
                }
            }
        }
        Cmd::EditChat { peer, title, .. } => {
            if let Some(title) = title {
                app.push_event(Ev::ChatTitle { peer, title });
            }
            app.push_event(Ev::Notice(crate::i18n::l!("Зміни збережено", "Changes saved").into()));
        }
        Cmd::SetChatPhoto { .. } | Cmd::SetChatRights { .. } | Cmd::SetSlowMode { .. } | Cmd::SetChatReactions { .. } | Cmd::SetMembersHidden { .. } | Cmd::SetAntiSpam { .. } => app.push_event(Ev::Notice(crate::i18n::l!("Зміни збережено", "Changes saved").into())),
        Cmd::SetBirthday(birthday) => app.push_event(Ev::Profile(Profile { birthday, ..app.demo_profile() })),
        Cmd::ToggleUsername { name, active } => {
            let mut p = app.demo_profile();
            if let Some(u) = p.usernames.iter_mut().find(|u| u.0 == name) {
                u.1 = active;
            }
            app.push_event(Ev::Profile(p));
        }
        Cmd::ReorderUsernames(order) => {
            let mut p = app.demo_profile();
            p.usernames.sort_by_key(|u| order.iter().position(|o| *o == u.0));
            app.push_event(Ev::Profile(p));
        }
        Cmd::LoadBirthdays => {
            // Olena has her birthday today, so the reminder is always there in the demo.
            let olena = app.demo_chats().iter().find(|c| c.id == PeerId::user_unchecked(1)).cloned();
            app.push_event(Ev::Birthdays(olena.into_iter().map(|c| (c, today_birthday(Some(1996)))).collect()));
        }
        Cmd::LoadContacts => app.push_event(Ev::Contacts(demo_contacts())),
        Cmd::AddContact { user, phone, first, last } => {
            let name = if last.is_empty() { first } else { format!("{first} {last}") };
            let c = match user {
                Some(peer) => ChatInfo { title: name, contact: true, ..app.demo_chat(peer).cloned().unwrap_or_else(|| person(0, "", "", Presence::None)) },
                // A made-up account behind any number.
                None => person(40 + phone.len() as i64, &name, "last seen recently", Presence::None),
            };
            app.push_event(Ev::ContactSaved(c));
        }
        Cmd::DeleteContact(peer) => app.push_event(Ev::ContactRemoved(peer)),
        Cmd::Block { peer, blocked } => {
            let chat = app.demo_chat(peer).cloned();
            BLOCKED.with_borrow_mut(|list| {
                list.retain(|c| c.id != peer);
                if blocked && let Some(c) = chat {
                    list.insert(0, c);
                }
            });
            app.push_event(Ev::Blocked { peer, blocked });
        }
        Cmd::LoadBlocked => app.push_event(Ev::BlockedList(BLOCKED.with_borrow(Clone::clone))),
        Cmd::SearchContacts(query) => {
            let q = query.to_lowercase();
            let chats = GLOBAL_PEOPLE
                .iter()
                .filter(|(_, name, user)| name.to_lowercase().contains(&q) || user.contains(&q))
                .map(|(id, name, user)| person(*id, name, &format!("@{user}"), Presence::Recently))
                .collect();
            app.push_event(Ev::ContactResults { query, chats });
        }
        Cmd::CreateGroup { title, users } => {
            let n = users.len() + 1;
            created(app, &title, ChatKind::Group, format!("Group \"{title}\" created · {n} members"));
        }
        Cmd::CreateChannel { title, about } => {
            let text = if about.is_empty() { format!("Channel \"{title}\" created") } else { format!("Channel \"{title}\" created\n{about}") };
            created(app, &title, ChatKind::Channel, text);
        }
        // Short famous names are taken, the rest are free.
        Cmd::CheckChannelUsername { peer, name } => {
            let state = if ["telegram", "durov", "news"].iter().any(|t| name.to_lowercase().contains(t)) { UsernameState::Taken } else { UsernameState::Free };
            app.push_event(Ev::ChannelUsernameCheck { peer, name, state });
        }
        Cmd::SetChannelUsername { name, .. } => app.push_event(Ev::Notice(format!("Link: t.me/{name}"))),
        Cmd::InviteToChannel { users, .. } => app.push_event(Ev::Notice(format!("Subscribers added: {}", users.len()))),
        // Contacts without a demo history open as an empty chat.
        Cmd::OpenChat(peer) if !app.demo_history().any(|(p, ..)| p == peer) => {
            app.push_event(Ev::History { peer, msgs: Vec::new(), older: false });
        }
        // Like the server: opening a chat answers with its history page.
        Cmd::OpenChat(peer) => {
            let msgs = app.demo_history().find(|(p, ..)| *p == peer).map(|(_, _, m)| m.clone()).unwrap_or_default();
            app.push_event(Ev::History { peer, msgs, older: false });
        }
        Cmd::Typing(peer) if peer == PeerId::user_unchecked(1) => {
            // Olena "replies" when text is typed.
            app.push_event(Ev::Typing { peer, who: "Olena".into(), active: true });
        }
        _ => {}
    }
}

thread_local! {
    /// Demo auto-delete timers: chats changed in this run, and the default for new chats.
    static DEMO_TTL: std::cell::RefCell<HashMap<PeerId, i32>> = std::cell::RefCell::new(HashMap::new());
    static DEMO_DEFAULT_TTL: std::cell::Cell<i32> = const { std::cell::Cell::new(0) };
    static DEMO_ACCOUNT_TTL: std::cell::Cell<i32> = const { std::cell::Cell::new(180) };
    /// Demo cache types the user cleared (the real cache is never touched in the demo).
    static DEMO_CLEARED: std::cell::RefCell<Vec<crate::backend::CacheKind>> = const { std::cell::RefCell::new(Vec::new()) };
    /// Demo websites the user disconnected.
    static DEMO_WEB_GONE: std::cell::RefCell<Vec<i64>> = const { std::cell::RefCell::new(Vec::new()) };
    /// Demo reaction tags of Saved Messages; renaming changes them.
    static SAVED_TAGS: std::cell::RefCell<Vec<SavedTag>> = std::cell::RefCell::new(vec![
        SavedTag { emoji: "👍".into(), title: "Ideas".into(), count: 2 },
        SavedTag { emoji: "❤".into(), title: String::new(), count: 1 },
    ]);
    /// Demo cloud password: "demo", with a hint and no recovery email at first.
    static PASSWORD: std::cell::RefCell<(String, PasswordState)> = std::cell::RefCell::new((
        "demo".into(),
        PasswordState { has_password: true, hint: "demo".into(), has_recovery: false, email_pending: None },
    ));
    /// Demo administration, the same for every chat we run.
    static MANAGE: std::cell::RefCell<Option<DemoManage>> = const { std::cell::RefCell::new(None) };
    /// Demo blocked list: starts with one spammer, follows Block / Unblock.
    static BLOCKED: std::cell::RefCell<Vec<ChatInfo>> = std::cell::RefCell::new(vec![person(50, "Spam Blast", "", Presence::None)]);
}

/// People found only by global search in the demo: (id, name, username).
const GLOBAL_PEOPLE: [(i64, &str, &str); 3] = [(30, "Oleksandr Melnyk", "melnyk"), (31, "Olena Petrenko", "olena_p"), (32, "Rust Weekly", "rustweekly")];

fn person(id: i64, name: &str, status: &str, presence: Presence) -> ChatInfo {
    ChatInfo {
        id: PeerId::user_unchecked(id),
        title: name.into(),
        kind: ChatKind::User,
        last_text: status.into(),
        last_time: String::new(),
        last_ts: 0,
        unread: 0,
        pinned: false,
        contact: true,
        muted: false,
        archived: false,
        unread_mark: false,
        draft: String::new(),
        presence,
        forum: false,
        joined: true,
        can_post: true,
        read_out: 0,
        read_in: 0,
        unread_mentions: 0,
        unread_reactions: 0,
        noforwards: false,
        can_pin: false,
        restricted: None,
        emoji_status: None,
    }
}

/// Demo contacts: people from the chat list plus a few without any chat yet.
fn demo_contacts() -> Vec<ChatInfo> {
    let now = jiff::Timestamp::now().as_second();
    let mut list = vec![
        person(1, "Olena Koval", "@olena", Presence::Online(i64::MAX)),
        person(5, "Andriy", "@andriy", Presence::Offline(now - 3600)),
        person(8, "Maria Shevchenko", "", Presence::Recently),
        person(20, "Ihor Bondar", "@bondar", Presence::LastWeek),
        person(21, "Nataliia Lysenko", "", Presence::Offline(now - 86_400 * 3)),
        person(22, "Tetiana", "@tanya", Presence::Online(i64::MAX)),
    ];
    list.sort_by_key(|c| c.title.to_lowercase());
    list
}

/// A freshly "created" group or channel with a service-like first message.
fn created(app: &mut App, title: &str, kind: ChatKind, text: String) {
    let peer = PeerId::user_unchecked(2000 + rand_ish() as i64);
    let msg = outgoing(peer, text, None);
    let mut info = person(0, title, &msg.text, Presence::None);
    info.id = peer;
    info.kind = kind;
    info.contact = false;
    info.last_time = msg.time.clone();
    info.last_ts = msg.ts;
    app.push_event(Ev::History { peer, msgs: vec![msg], older: false });
    app.push_event(Ev::ChatCreated(info));
}

const FORUM: i64 = 10;
/// Our message in "Rust Team" with an unread reaction.
const DEMO_REACTED: i32 = 3;
/// The channel that shows a sponsored message.
const DEMO_CHANNEL: i64 = 4;
const DEMO_AD_PHOTO: i64 = 77;
const DEMO_AD_MEDIA: i64 = 78;

fn demo_ad() -> Sponsored {
    let text = "A weekly digest of new crates, releases and articles about **Rust**. No fluff.";
    let (text, entities) = format::parse(text);
    Sponsored {
        random_id: vec![7; 16],
        title: "Rust Weekly".into(),
        spans: format::from_entities(&text, &entities, 0),
        text,
        url: "https://t.me/rustweekly".into(),
        button: "View channel".into(),
        recommended: false,
        can_report: true,
        sponsor_info: "Demo Media LLC, reg. no. 00000000".into(),
        additional_info: "This ad was placed via Telegram Ads.".into(),
        photo: Some(DEMO_AD_PHOTO),
        media: None,
        video: false,
    }
}

/// The second ad, a few posts higher: a media ad with a wide picture.
fn demo_media_ad() -> Sponsored {
    Sponsored {
        random_id: vec![8; 16],
        title: "Glass Wallpapers".into(),
        text: "A pack of \"liquid glass\" desktop wallpapers.".into(),
        spans: Vec::new(),
        url: "https://example.org/wallpapers".into(),
        button: "Open".into(),
        photo: None,
        media: Some(DEMO_AD_MEDIA),
        ..demo_ad()
    }
}
/// Group members for @mentions and the search sender filter: (id, name, username, bot).
const MENTIONABLE: [(i64, &str, &str, bool); 5] =
    [(31, "Taras Boyko", "taras_b", false), (32, "Iryna Tkachenko", "iryna", false), (33, "Sofia", "", false), (34, "Maksym Lysenko", "max_l", false), (35, "RustDocsBot", "rustdocsbot", true)];

/// Bot chat with no messages yet: shows the "Start" bar.
const HELPER_BOT: i64 = 11;

fn helper_bot() -> ChatInfo {
    ChatInfo {
        id: PeerId::user_unchecked(HELPER_BOT),
        title: "GlassHelperBot".into(),
        kind: ChatKind::Bot,
        last_text: String::new(),
        last_time: String::new(),
        last_ts: 0,
        unread: 0,
        pinned: false,
        contact: false,
        muted: false,
        archived: false,
        unread_mark: false,
        draft: String::new(),
        presence: Presence::None,
        forum: false,
        joined: true,
        can_post: true,
        read_out: i32::MAX,
        read_in: 0,
        unread_mentions: 0,
        unread_reactions: 0,
        noforwards: false,
        can_pin: false,
        restricted: None,
        emoji_status: None,
    }
}
/// Name colours of demo people: built-in ones and the server's two-tone 9.
fn demo_color(sender: &str) -> Option<i32> {
    match sender {
        "" => None,
        "Taras" => Some(0),
        "Iryna" => Some(3),
        "Sofia" => Some(9),
        "Maksym" => Some(1),
        "Olena" => Some(5),
        "Oleh" => Some(4),
        _ => Some(2),
    }
}

/// Extra name colours as `help.getPeerColors` sends them: two tones for each theme.
fn peer_colors() -> Vec<ColorOption> {
    [
        (7, [0xe15052, 0xf9ae63], [0xff9380, 0x992f37]),
        (8, [0xe0802b, 0xfac534], [0xecb04e, 0xc35714]),
        (9, [0xa05ff3, 0xf48fff], [0xc697ff, 0x5e31c8]),
        (10, [0x27a910, 0xa7dc57], [0xa7eb6e, 0x167e2d]),
        (11, [0x27acce, 0x82e8d6], [0x40d8d0, 0x045c7f]),
        (12, [0x3391d4, 0x7dd3f0], [0x52bfff, 0x0b5494]),
        (13, [0xdd4371, 0xffbe9f], [0xff86a6, 0x8e366e]),
    ]
    .into_iter()
    .map(|(id, light, dark)| ColorOption { id, light: light.to_vec(), dark: dark.to_vec() })
    .collect()
}

/// The demo user's own id (for the avatar in settings).
const ME: i64 = 1000;

fn demo_profile() -> Profile {
    Profile {
        id: Some(PeerId::user_unchecked(ME)),
        first: "Demo".into(),
        last: "User".into(),
        bio: "Writing a Telegram client in Rust 🦀".into(),
        username: "glassgram_demo".into(),
        phone: "+380 00 000 00 00".into(),
        birthday: Some(Birthday { day: 12, month: 5, year: None }),
        // Collectible usernames bought on Fragment: one shown in the profile, one hidden.
        usernames: vec![("glass".into(), true), ("liquidgram".into(), false)],
        // Premium: "About this ad" offers to hide ads.
        premium: true,
        ads_off: false,
        color: (Some(5), None),
        profile_color: (Some(4), None),
        emoji_status: Some(9003),
    }
}

/// Send queue "from the previous run": one message that failed, to show the red mark and its menu.
pub fn outbox() -> Vec<Queued> {
    let ts = jiff::Timestamp::now().as_second() - 120;
    vec![Queued { key: 1, peer: PeerId::user_unchecked(5), text: "Sent the report, have a look when you're online".into(), reply_to: None, silent: false, ts, failed: true, no_webpage: false, quote: None, reply_peer: None, media: None, effect: None, send_as: None }]
}

/// Demo forum topics: (id, title, color, unread, pinned, closed).
const TOPICS: [(i32, &str, u32, i32, bool, bool); 4] = [
    (1, "General", 0, 0, false, false),
    (100, "Help for newcomers", 0x6FB9F0, 3, true, false),
    (200, "Release 1.0", 0xFFD67E, 1, false, false),
    (300, "Discussion archive", 0xCB86DB, 0, false, true),
];

/// Forum group "Rust Ukraine" with several topics.
fn forum_chat() -> (ChatInfo, Vec<Msg>) {
    let peer = PeerId::channel_unchecked(FORUM);
    let script: [(i32, i32, &str, &str); 7] = [
        (1, 1, "Moderator", "Welcome to the forum! Questions go to \"Help for newcomers\"."),
        (100, 101, "Ostap", "How do I get rid of `borrow of moved value`?"),
        (100, 102, "Lesia", "Clone it or pass a reference: `&value`."),
        (100, 103, "", "Also worth reading the chapter on **ownership** in the book."),
        (200, 201, "Taras", "Building release 1.0 on Friday 🦀"),
        (200, 202, "Iryna", "The changelog is ready, release notes are left."),
        (300, 301, "Moderator", "Topic closed."),
    ];
    let msgs = script
        .iter()
        .map(|&(topic, id, sender, text)| {
            let (text, ents) = format::parse(text);
            Msg {
                id,
                sender: sender.into(),
                out: sender.is_empty(),
                spans: format::from_entities(&text, &ents, 0),
                text,
                time: format!("1{}:{:02}", id % 10, id % 60),
                day: l!("Сьогодні", "Today").into(),
                topic: (topic != 1).then_some(topic),
                ..Msg::placeholder(peer)
            }
        })
        .collect();
    let chat = ChatInfo {
        id: peer,
        title: "Rust Ukraine".into(),
        kind: ChatKind::Group,
        last_text: "Iryna: The changelog is ready, release notes are left.".into(),
        last_time: "12:02".into(),
        last_ts: 97,
        unread: 4,
        pinned: false,
        contact: false,
        muted: false,
        archived: false,
        unread_mark: false,
        draft: String::new(),
        presence: Presence::None,
        forum: true,
        joined: true,
        can_post: true,
        read_out: i32::MAX,
        read_in: 0,
        unread_mentions: 0,
        unread_reactions: 0,
        noforwards: false,
        can_pin: true,
        restricted: None,
        emoji_status: None,
    };
    (chat, msgs)
}

fn demo_topics() -> Vec<Topic> {
    TOPICS
        .iter()
        .map(|&(id, title, color, unread, pinned, closed)| {
            let last = match id {
                1 => "Moderator: Welcome to the forum!",
                100 => "You: Also worth reading the chapter on ownership in the book.",
                200 => "Iryna: The changelog is ready, release notes are left.",
                _ => "Moderator: Topic closed.",
            };
            Topic { id, title: title.into(), color, unread, pinned, closed, last_text: last.into(), last_time: "12:02".into(), last_ts: 0 }
        })
        .collect()
}

/// Extra demo messages: poll, place, contact, album, reactions.
fn extras(id: i64, peer: PeerId, msgs: &mut Vec<Msg>) {
    let base = |n: i32, sender: &str| Msg { sender: sender.into(), sender_color: demo_color(sender), out: sender.is_empty(), time: format!("12:{n:02}"), ..Msg::placeholder(peer) };
    match id {
        2 => {
            // A link with a preview card (its picture comes from `images()`).
            if let Some(m) = msgs.iter_mut().find(|m| m.id == 5) {
                m.webpage = Some(WebPageInfo {
                    id: 1,
                    url: "https://codeberg.org/Lonami/grammers".into(),
                    site: "Codeberg.org".into(),
                    title: "Lonami/grammers".into(),
                    description: "(tele)gramme.rs — a set of Rust crates for the Telegram MTProto API".into(),
                    photo: true,
                });
            }
            if let Some(m) = msgs.first_mut() {
                m.reactions = vec![Reaction { emoji: "👍".into(), count: 3, mine: false }, Reaction { emoji: "🔥".into(), count: 1, mine: true }];
            }
            // Unread reaction to our message.
            if let Some(m) = msgs.iter_mut().find(|m| m.id == DEMO_REACTED) {
                m.reactions = vec![Reaction { emoji: "❤".into(), count: 1, mine: false }];
            }
            let opt = |t: &str, v: i32, o: u8| PollOption { text: t.into(), option: vec![o], voters: v, chosen: false, correct: false };
            msgs.push(Msg {
                id: 50,
                poll: Some(PollInfo {
                    question: "When do we release?".into(),
                    answers: vec![opt("On Friday", 4, 0), opt("Next week", 2, 1), opt("When it's ready 🦀", 5, 2)],
                    total: 11,
                    public: true,
                    open_answers: true,
                    close_date: Some(jiff::Timestamp::now().as_second() + 5 * 3600),
                    ..PollInfo::default()
                }),
                pinned: true,
                ..base(10, "Taras")
            });
            // A shared checklist, partly done.
            let task = |id: i32, title: &str, done: Option<(&str, i64)>| TodoTask { id, title: title.into(), done: done.map(|(w, t)| (w.to_owned(), t)) };
            let ts = jiff::Timestamp::now().as_second();
            msgs.push(Msg {
                id: 52,
                todo: Some(TodoInfo {
                    title: "Before the release".into(),
                    items: vec![
                        task(1, "Update the README", Some(("Iryna", ts - 7200))),
                        task(2, "Run the tests on Windows", Some(("Taras", ts - 3600))),
                        task(3, "Build the Flatpak", None),
                        task(4, "Write the release notes", None),
                    ],
                    others_can_append: true,
                    others_can_complete: true,
                }),
                ..base(11, "Iryna")
            });
            msgs.push(Msg {
                id: 51,
                geo: Some(GeoInfo { lat: 50.4501, lon: 30.5234, title: "Office".into(), address: "1 Khreshchatyk St, Kyiv".into(), live: false }),
                text: "Meeting here at 10:00".into(),
                ..base(12, "Iryna")
            });
        }
        1 => {
            msgs.push(Msg { id: 60, contact: Some(("Andriy Melnyk".into(), "+380 67 123 45 67".into())), ..base(20, "Olena") });
            for (k, t) in ["Sunset from the balcony 🌅", "", ""].iter().enumerate() {
                msgs.push(Msg { id: 70 + k as i32, photo: true, group: Some(7), text: t.to_string(), ..base(21, "") });
            }
            let media = |kind, duration, title: &str, performer: &str| MediaInfo {
                kind,
                duration,
                size: (0, 0),
                title: title.into(),
                performer: performer.into(),
                waveform: (0..63u32).map(|i| ((i * 7 % 13) as f32 * 1.6 + (i as f32 * 0.4).sin().abs() * 12.0) as u8).collect(),
                bytes: 0,
            };
            msgs.push(Msg { id: 80, media: Some(media(MediaKind::Voice, 4.0, "", "")), ..base(22, "Olena") });
            msgs.push(Msg { id: 81, media: Some(media(MediaKind::Voice, 3.0, "", "")), media_unread: true, ..base(22, "Olena") });
            msgs.push(Msg { id: 82, media: Some(media(MediaKind::Audio, 5.0, "Glass Morning", "Glassgram")), ..base(23, "") });
            msgs.extend(music::messages(base(23, "Olena")));
            msgs.push(Msg {
                id: 83,
                media: Some(MediaInfo { size: (640, 360), ..media(MediaKind::Video, 6.0, "", "") }),
                text: "Video from the concert".into(),
                ..base(24, "Olena")
            });
            msgs.push(Msg { id: 84, media: Some(MediaInfo { size: (480, 270), ..media(MediaKind::Gif, 3.0, "", "") }), ..base(25, "") });
            msgs.push(Msg { id: 85, media: Some(MediaInfo { size: (240, 240), ..media(MediaKind::Round, 5.0, "", "") }), media_unread: true, ..base(26, "Olena") });
            msgs.push(Msg { id: 73, dice: Some(("🎰".into(), 64)), ..base(27, "Olena") });
            msgs.push(Msg { id: 74, photo: true, spoiler: true, text: "Don't open it if you haven't seen the finale 🙈".into(), ..base(27, "Olena") });
            msgs.push(Msg { id: 75, photo: true, invert_media: true, text: "Caption above the photo, as in Telegram".into(), ..base(27, "") });
            msgs.push(Msg { id: 76, unsupported: true, ..base(27, "Olena") });
            msgs.push(custom_emoji::message(base(28, "Olena")));
            msgs.extend(effects::messages(base(29, "Olena")));
        }
        4 => msgs.push(aitext::post(base(30, "Tech News"))),
        9 => {
            // A long history: shows virtualization (only bubbles near the screen are drawn).
            const LONG: i32 = 3000;
            let lines = ["Who's on duty today?", "Me, after lunch", "The build broke on CI, looking", "Fixed, it was an outdated dependency", "👍", "Reminder: retro on Friday"];
            let now = jiff::Zoned::now();
            let old = (1..=LONG).map(|n| {
                let date = now.checked_sub(jiff::Span::new().days(i64::from((LONG - n) / 40 + 1))).ok();
                let day = date.as_ref().map_or_else(String::new, |d| d.strftime("%d.%m.%Y").to_string());
                // Real times so the calendar can jump through this history.
                let ts = date.map_or(0, |d| d.timestamp().as_second());
                let sender = ["Oleh", "Natalia", ""][(n % 3) as usize];
                let text = lines[(n as usize * 7) % lines.len()];
                Msg { id: n, text: text.into(), spans: format::from_entities(text, &[], 0), day, ts, reply_to: (n % 17 == 0).then_some(n - 2), ..base(n % 60, sender) }
            });
            let tail = std::mem::take(msgs);
            msgs.extend(old);
            msgs.extend(tail.into_iter().map(|m| Msg { id: m.id + LONG, ..m }));
        }
        _ => {}
    }
}

fn weather_buttons() -> Vec<Vec<Button>> {
    let b = |text: &str, action| Button { text: text.into(), action };
    vec![
        vec![b("Refresh",ButtonAction::Callback(b"now".to_vec())), b("Tomorrow",ButtonAction::Callback(b"tomorrow".to_vec()))],
        vec![b("Weather map",ButtonAction::Url("https://www.windy.com".into())), b("Radar", ButtonAction::WebApp { url: "https://example.org/radar".into(), simple: false, bot: 6 })],
        vec![b("Log in to the site", ButtonAction::UrlAuth { url: "https://example.org/weather/login".into(), button_id: 3 })],
    ]
}

/// Demo sound for voice messages: a pulsing chord as WAV; music has its own tunes (`music::tone`).
fn tone(id: i32) -> Option<std::path::PathBuf> {
    let secs = match id {
        80 => 4.0,
        81 => 3.0,
        _ => return music::tone(id),
    };
    const RATE: u32 = 22_050;
    let n = (RATE as f32 * secs) as u32;
    let base = 330.0;
    let samples = (0..n).map(|i| {
        let t = i as f32 / RATE as f32;
        let env = (t * 3.0 * std::f32::consts::PI).sin().abs() * (1.0 - t / secs).min(t * 20.0).min(1.0);
        let v: f32 = [1.0, 1.25, 1.5].iter().map(|k| (t * base * k * std::f32::consts::TAU).sin()).sum::<f32>() / 3.0;
        (v * env * 0.4 * i16::MAX as f32) as i16
    });
    write_wav(&format!("demo-{id}.wav"), RATE, samples)
}

/// Mono 16-bit WAV in our own temp folder.
fn write_wav(name: &str, rate: u32, samples: impl ExactSizeIterator<Item = i16>) -> Option<std::path::PathBuf> {
    let n = samples.len() as u32;
    let mut wav = Vec::with_capacity(44 + n as usize * 2);
    wav.extend_from_slice(b"RIFF");
    wav.extend_from_slice(&(36 + n * 2).to_le_bytes());
    wav.extend_from_slice(b"WAVEfmt ");
    for v in [16u32.to_le_bytes(), [1, 0, 1, 0], rate.to_le_bytes(), (rate * 2).to_le_bytes(), [2, 0, 16, 0]] {
        wav.extend_from_slice(&v);
    }
    wav.extend_from_slice(b"data");
    wav.extend_from_slice(&(n * 2).to_le_bytes());
    samples.for_each(|s| wav.extend_from_slice(&s.to_le_bytes()));
    let path = crate::platform::temp_file(name);
    std::fs::write(&path, wav).ok()?;
    Some(path)
}

/// Demo videos from ffmpeg test sources: video with sound, silent GIF, round video.
fn clip(id: i32) -> Option<std::path::PathBuf> {
    let (video, audio, secs) = match id {
        83 => ("testsrc2=size=640x360:rate=30", true, 6),
        84 => ("mandelbrot=size=480x270:rate=30", false, 3),
        85 => ("life=size=240x240:mold=10:rate=30:ratio=0.5:death_color=#5a3cc8:life_color=#ffd6f0", true, 5),
        86 => ("testsrc2=size=360x640:rate=30", true, 5),
        87 => ("mandelbrot=size=240x240:rate=30", false, 4),
        _ => return None,
    };
    let path = std::env::temp_dir().join(format!("glassgram-demo-{id}.mp4"));
    if path.exists() {
        return Some(path);
    }
    let mut cmd = crate::platform::tool("ffmpeg");
    cmd.args(["-loglevel", "quiet", "-y", "-f", "lavfi", "-i", video]);
    if audio {
        cmd.args(["-f", "lavfi", "-i", "sine=frequency=440:beep_factor=4"]);
    }
    cmd.args(["-t", &secs.to_string(), "-pix_fmt", "yuv420p", "-shortest"]).arg(&path);
    cmd.status().ok()?.success().then_some(path)
}

/// Files, voice and round video messages from the send queue, as the server would echo them.
fn queued_media(app: &mut App, q: &Queued) {
    let (peer, reply) = (q.peer, q.reply());
    match q.media.clone() {
        Some(QueuedMedia::Files { paths, as_file, spoiler, above, once }) => {
            for (i, path) in paths.iter().enumerate() {
                let name = path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
                let size = std::fs::metadata(path).map(|m| crate::backend::human_size(m.len() as usize)).unwrap_or_default();
                let text = if i == 0 { q.text.clone() } else { String::new() };
                let mut m = sendas::posted_as(replied(app, outgoing(peer, text, None), &reply), q.send_as);
                m.id += i as i32;
                // Pictures come back as photos, like from the server, with the options chosen.
                let picture = (!as_file).then(|| image::open(path).ok()).flatten().map(|img| img.thumbnail(800, 800).to_rgba8());
                match picture {
                    Some(rgba) => {
                        let (mid, size) = (m.id, [rgba.width() as usize, rgba.height() as usize]);
                        app.push_event(Ev::NewMsg(Msg { photo: true, spoiler, invert_media: above, ttl: once.then_some(crate::backend::VIEW_ONCE), ..m }));
                        app.push_event(Ev::Image { key: ImgKey::Photo(peer, mid), image: ColorImage::from_rgba_unmultiplied(size, rgba.as_raw()) });
                    }
                    None => app.push_event(Ev::NewMsg(Msg { file: Some((name, size)), ..m })),
                }
            }
            let _ = std::fs::remove_dir_all(crate::backend::outbox_dir(&crate::accounts::session_path(0), q.key));
        }
        Some(QueuedMedia::Voice { path, duration, waveform }) => {
            let info = MediaInfo { kind: MediaKind::Voice, duration, size: (0, 0), title: String::new(), performer: String::new(), waveform, bytes: 0 };
            let m = Msg { media: Some(info), ..sendas::posted_as(replied(app, outgoing(peer, String::new(), None), &reply), q.send_as) };
            let mid = m.id;
            app.push_event(Ev::NewMsg(m));
            // The file stays on disk: that's what we will play.
            app.push_event(Ev::MediaReady { peer, id: mid, path });
        }
        Some(QueuedMedia::Round { path, duration }) => {
            let info = MediaInfo { kind: MediaKind::Round, duration, size: (384, 384), title: String::new(), performer: String::new(), waveform: Vec::new(), bytes: 0 };
            let m = Msg { media: Some(info), ..sendas::posted_as(replied(app, outgoing(peer, String::new(), None), &reply), q.send_as) };
            let mid = m.id;
            app.push_event(Ev::NewMsg(m));
            app.push_event(Ev::MediaReady { peer, id: mid, path });
        }
        None => {}
    }
}

/// A beating heart as a `.tgs` (gzipped Lottie), for the demo's animated sticker.
fn demo_tgs() -> Option<std::path::PathBuf> {
    use std::io::Write;
    let path = std::env::temp_dir().join("glassgram-demo-heart.tgs");
    if path.exists() {
        return Some(path);
    }
    let scale = r#"{"a":1,"k":[{"t":0,"s":[80,80,100],"i":{"x":[0.4],"y":[1]},"o":{"x":[0.6],"y":[0]}},{"t":15,"s":[105,105,100],"i":{"x":[0.4],"y":[1]},"o":{"x":[0.6],"y":[0]}},{"t":30,"s":[80,80,100]}]}"#;
    let heart = r#"{"ty":"sh","ks":{"a":0,"k":{"c":true,"v":[[0,40],[-60,-10],[-30,-55],[0,-30],[30,-55],[60,-10]],"i":[[0,0],[0,25],[-25,0],[0,0],[0,0],[0,-25]],"o":[[0,0],[0,-25],[0,0],[0,0],[25,0],[0,25]]}}}"#;
    let json = format!(
        r#"{{"v":"5.5.2","fr":30,"ip":0,"op":30,"w":200,"h":200,"layers":[{{"ty":4,"ind":1,"ip":0,"op":30,"st":0,"ks":{{"o":{{"a":0,"k":100}},"r":{{"a":0,"k":0}},"p":{{"a":0,"k":[100,105,0]}},"a":{{"a":0,"k":[0,0,0]}},"s":{scale}}},"shapes":[{heart},{{"ty":"fl","c":{{"a":0,"k":[0.95,0.25,0.4,1]}},"o":{{"a":0,"k":100}}}}]}}]}}"#
    );
    let mut gz = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::default());
    gz.write_all(json.as_bytes()).ok()?;
    std::fs::write(&path, gz.finish().ok()?).ok()?;
    Some(path)
}

fn rand_ish() -> i32 {
    jiff::Timestamp::now().subsec_nanosecond() % 1_000_000
}

/// Generated "photos": a gradient sunset and an avatar.
pub fn images() -> Vec<(ImgKey, ColorImage)> {
    let sunset = |w: usize, h: usize| {
        let mut img = ColorImage::filled([w, h], Color32::BLACK);
        for y in 0..h {
            for x in 0..w {
                let (fx, fy) = (x as f32 / w as f32, y as f32 / h as f32);
                let d = ((fx - 0.5).powi(2) + (fy - 0.62).powi(2)).sqrt();
                let sun = (1.0 - d * 6.0).clamp(0.0, 1.0);
                let sea = if fy > 0.65 { 0.45 } else { 1.0 };
                let r = ((255.0 * (0.55 + 0.45 * fy) + 255.0 * sun) * sea).min(255.0);
                let g = ((90.0 + 120.0 * fy * fy + 200.0 * sun) * sea).min(255.0);
                let b = ((170.0 - 110.0 * fy + 120.0 * sun) * sea).min(255.0);
                img.pixels[y * w + x] = Color32::from_rgb(r as u8, g as u8, b as u8);
            }
        }
        img
    };
    let id = PeerId::user_unchecked(1);
    vec![
        (ImgKey::Photo(id, 6), sunset(480, 320)),
        (ImgKey::Photo(id, 70), sunset(400, 300)),
        (ImgKey::Photo(id, 71), sunset(300, 300)),
        (ImgKey::Photo(id, 72), sunset(320, 420)),
        (ImgKey::Photo(id, 74), sunset(420, 300)),
        (ImgKey::Photo(id, 75), sunset(400, 260)),
        (ImgKey::Photo(id, effects::VIEW_ONCE_PHOTO), sunset(380, 300)),
        (ImgKey::Avatar(PeerId::user_unchecked(5)), sunset(96, 96)),
        (ImgKey::Photo(PeerId::user_unchecked(2), 5), sunset(480, 250)),
        (ImgKey::Sponsored(DEMO_AD_PHOTO), sunset(112, 112)),
        (ImgKey::Sponsored(DEMO_AD_MEDIA), sunset(640, 360)),
    ]
}

/// A tall gradient for a demo story, a different hue per story.
/// Story ids of a demo profile list, newest first: Olena, the channel and our own profile have some.
fn profile_story_ids(peer: PeerId, list: StoryList) -> Vec<i32> {
    let ids: Vec<i32> = match (peer.bot_api_dialog_id_unchecked(), list) {
        (1, StoryList::Pinned) => (101..=107).collect(),
        (1, StoryList::Album(1)) => vec![101, 103, 105],
        (1, StoryList::Album(2)) => vec![102, 106],
        (4, StoryList::Pinned) => (201..=205).collect(),
        (4, StoryList::Album(3)) => vec![201, 203],
        (ME, StoryList::Pinned) => (301..=304).collect(),
        (ME, StoryList::Album(4)) => vec![301, 302],
        // Over one page, so "Show more" appears.
        (ME, StoryList::Archive) => (401..=440).collect(),
        _ => Vec::new(),
    };
    ids.into_iter().rev().collect()
}

fn story_picture(id: i32) -> ColorImage {
    let (w, h) = (360, 640);
    let mut img = ColorImage::filled([w, h], Color32::BLACK);
    let hue = id as f32 * 0.9;
    for y in 0..h {
        for x in 0..w {
            let (fx, fy) = (x as f32 / w as f32, y as f32 / h as f32);
            let wave = ((fx * 6.0 + fy * 3.0 + hue).sin() * 0.5 + 0.5) * 0.35;
            let r = (120.0 + 120.0 * (hue + fy * 2.0).sin().abs() + 60.0 * wave).min(255.0);
            let g = (60.0 + 90.0 * (hue * 0.7 + fx).cos().abs() + 80.0 * wave).min(255.0);
            let b = (150.0 + 100.0 * (1.0 - fy) + 40.0 * wave).min(255.0);
            img.pixels[y * w + x] = Color32::from_rgb(r as u8, g as u8, b as u8);
        }
    }
    img
}

/// A birthday falling on today, so the reminder always shows in the demo.
fn today_birthday(year: Option<u16>) -> Birthday {
    let d = jiff::Zoned::now().date();
    Birthday { day: d.day() as u8, month: d.month() as u8, year }
}

/// Demo profile: bio, username and member counts by chat kind.
fn demo_details(peer: PeerId, kind: Option<ChatKind>) -> ChatDetails {
    let id = peer.bare_id_unchecked();
    let mut d = ChatDetails { can_view_members: true, ..Default::default() };
    match kind {
        Some(ChatKind::User) | Some(ChatKind::Bot) => {
            d.about = match id {
                1 => "UI designer. I love glass, light and cats 🐱".into(),
                6 => "Weather for any city: /weather Kyiv".into(),
                _ => String::new(),
            };
            d.username = match id {
                1 => "olena_koval".into(),
                6 => "weather_demo_bot".into(),
                _ => String::new(),
            };
            d.can_ttl = kind == Some(ChatKind::User);
            d.ttl = DEMO_TTL.with_borrow(|t| t.get(&peer).copied().unwrap_or(if id == 1 { 7 * 86400 } else { 0 }));
            if id == 1 {
                d.phone = "+380 50 000 00 01".into();
                d.birthday = Some(today_birthday(Some(1996)));
                d.usernames = vec!["olena_design".into()];
                d.gifts = 5;
                // Her channel opens like a found chat, without joining.
                d.channel = Some(ChatInfo {
                    id: PeerId::channel_unchecked(71),
                    kind: ChatKind::Channel,
                    contact: false,
                    joined: false,
                    can_post: false,
                    ..person(71, "Olena on Design", &crate::i18n::lp!(2310, ["{n} підписник", "{n} підписники", "{n} підписників"], ["{n} subscriber", "{n} subscribers"]), Presence::None)
                });
            }
            if id == 5 {
                d.birthday = Some(Birthday { day: 3, month: 9, year: None });
            }
            d.common = 2;
        }
        Some(ChatKind::Group) => {
            d.about = "We talk Rust, egui and grammers. No spam 🦀".into();
            d.members = Some(demo_members().len() as i32);
            d.online = Some(2);
            d.admins = Some(2);
            d.admin = id == 2;
            // We run "Rust Team": its edit box shows permissions and slow mode.
            d.can_edit = id == 2;
            let mut rights = ChatRights::from_banned(None);
            rights.change_info = false;
            d.rights = Some(rights);
            d.slowmode = Some(if id == 2 { 30 } else { 0 });
            if id == 2 {
                d.admin_log = true;
                d.my_rights = Some(AdminRights { anonymous: false, ..AdminRights::all() });
                d.requests = MANAGE.with_borrow_mut(|m| demo_manage(m).requests.len() as i32);
                d.reactions = Some(crate::backend::ReactionMode::Some(vec!["👍".into(), "❤".into(), "🔥".into(), "🎉".into()]));
                d.members_hidden = Some(false);
                d.antispam = Some(true);
            }
        }
        Some(ChatKind::Channel) => {
            d.about = "The top tech news, daily.".into();
            d.username = "tech_news_demo".into();
            d.members = Some(12840);
            d.can_view_members = false;
            // We run the news channel too: signatures, discussion group and its "Recent actions".
            d.admin = true;
            d.can_edit = true;
            d.signatures = Some(false);
            d.can_link = true;
            d.admin_log = true;
            d.linked = Some((PeerId::user_unchecked(2), "Rust Team 🦀".into()));
            d.my_rights = Some(AdminRights::all());
        }
        _ => {}
    }
    // Chats made with "New group" / "New channel" are ours: the owner gets "Delete" instead of "Leave".
    if id >= 2000 && matches!(kind, Some(ChatKind::Group | ChatKind::Channel)) {
        d.creator = true;
        d.admin = true;
        // Only us so far, and nothing filled in yet.
        d.about.clear();
        d.username.clear();
        d.members = Some(1);
        d.online = None;
        d.admins = Some(1);
        d.can_view_members = true;
        d.can_edit = true;
        d.my_rights = Some(AdminRights::all());
    }
    // Channel 4 starts muted, as in the chat list.
    if id == 4 {
        d.notify.mute_until = crate::backend::MUTE_FOREVER;
    }
    d
}

/// Demo members: several real demo chats plus a few new people.
fn demo_members() -> Vec<Member> {
    let people = [
        (1, "Olena Koval", MemberRole::Owner, Presence::Online(i64::MAX)),
        (5, "Andriy", MemberRole::Admin, Presence::Offline(jiff::Timestamp::now().as_second() - 3600)),
        (8, "Maria Shevchenko", MemberRole::Member, Presence::Recently),
        (101, "Taras Bondar", MemberRole::Member, Presence::LastWeek),
        (102, "Iryna Moroz", MemberRole::Member, Presence::Online(i64::MAX)),
        (103, "Sofia Lysenko", MemberRole::Member, Presence::Recently),
    ];
    people
        .into_iter()
        .map(|(id, name, role, presence)| Member {
            chat: ChatInfo {
                id: PeerId::user_unchecked(id),
                title: name.into(),
                kind: ChatKind::User,
                last_text: String::new(),
                last_time: String::new(),
                last_ts: 0,
                unread: 0,
                pinned: false,
                contact: true,
                muted: false,
                archived: false,
                unread_mark: false,
                draft: String::new(),
                presence,
                forum: false,
                joined: true,
                can_post: true,
                read_out: 0,
                read_in: 0,
                unread_mentions: 0,
                unread_reactions: 0,
                noforwards: false,
                can_pin: false,
                restricted: None,
                emoji_status: None,
            },
            role,
            rank: if id == 5 { "moderator".into() } else { String::new() },
        })
        .collect()
}

/// Shared media from the demo history, newest first, filtered like `InputMessagesFilter*`.
fn demo_shared(app: &App, peer: PeerId, kind: SharedKind) -> Vec<Msg> {
    let Some((_, _, msgs)) = app.demo_history().find(|(p, _, _)| *p == peer) else { return Vec::new() };
    let media = |m: &Msg| m.media.as_ref().map(|i| i.kind);
    msgs.iter()
        .rev()
        .filter(|m| match kind {
            SharedKind::Media => m.photo || media(m) == Some(MediaKind::Video),
            SharedKind::Files => m.file.is_some() || media(m) == Some(MediaKind::Audio),
            SharedKind::Links => m.spans.iter().any(|s| s.target.as_deref().is_some_and(|t| t.starts_with("http"))),
            SharedKind::Voice => matches!(media(m), Some(MediaKind::Voice | MediaKind::Round)),
            SharedKind::Gifs => media(m) == Some(MediaKind::Gif),
        })
        .cloned()
        .collect()
}

/// Two-step verification without a server: the password is "demo", any 4-digit code confirms an email.
fn password(app: &mut App, cmd: Cmd) {
    let wrong = |app: &mut App| app.push_event(Ev::PasswordFailed("Невірний пароль · Wrong password".into()));
    PASSWORD.with_borrow_mut(|(pw, state)| match cmd {
        Cmd::CheckPassword(p) if p == *pw => app.push_event(Ev::PasswordChecked { email: if state.has_recovery { "o***@example.com".into() } else { String::new() } }),
        Cmd::CheckPassword(_) => wrong(app),
        Cmd::UpdatePassword { current, .. } if state.has_password && current != *pw => wrong(app),
        Cmd::UpdatePassword { change, .. } => match change {
            PasswordChange::Set { new, hint, email } => {
                *pw = new;
                state.has_password = true;
                state.hint = hint;
                if !email.is_empty() {
                    state.email_pending = Some("o***@example.com".into());
                }
            }
            PasswordChange::Email(_) => state.email_pending = Some("o***@example.com".into()),
            PasswordChange::Off => *state = PasswordState::default(),
        },
        Cmd::ConfirmPasswordEmail(code) if code.len() == 4 => {
            state.email_pending = None;
            state.has_recovery = true;
        }
        Cmd::ConfirmPasswordEmail(_) => app.push_event(Ev::PasswordFailed("Невірний код · Wrong code".into())),
        Cmd::CancelPasswordEmail => state.email_pending = None,
        _ => {}
    });
    app.push_event(Ev::Password(PASSWORD.with_borrow(|(_, s)| s.clone())));
}

/// Media search tab: shared photos and videos of all demo chats; with no match, all of them.
fn search_media(app: &mut App, kind: SharedKind, q: &str) -> Vec<SearchHit> {
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
fn search_tabs(app: &mut App, cmd: Cmd) {
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

/// Admins (with rights and titles), restrictions, removed people, join requests and our links.
struct DemoManage {
    admins: Vec<(i64, AdminRights, String)>,
    restricted: Vec<(i64, ChatRights, i32)>,
    removed: Vec<i64>,
    requests: Vec<(i64, &'static str)>,
    links: Vec<InviteLink>,
}

fn demo_manage(m: &mut Option<DemoManage>) -> &mut DemoManage {
    m.get_or_insert_with(|| {
        let now = jiff::Timestamp::now().as_second() as i32;
        let mut no_media = ChatRights::from_banned(None);
        (no_media.send_media, no_media.send_stickers) = (false, false);
        let link = |link: &str, title: &str, usage, usage_limit, expire, requested, request_needed, revoked, permanent| InviteLink { link: link.into(), title: title.into(), usage, usage_limit, expire, requested, request_needed, revoked, permanent };
        DemoManage {
            admins: vec![(5, AdminRights { add_admins: false, ..AdminRights::new_admin() }, "moderator".into())],
            restricted: vec![(103, no_media, now + 86400)],
            removed: vec![104],
            requests: vec![(105, "Writing Rust for three years, I'd like to help with egui"), (106, "")],
            links: vec![
                link("https://t.me/+GlassgramDemo", "", 14, 0, 0, 0, false, false, true),
                link("https://t.me/+RustMeetup2026", "Meetup in Lviv", 23, 50, now + 5 * 86400, 0, false, false, false),
                link("https://t.me/+ApplyToTeam", "Team applications", 3, 0, 0, 2, true, false, false),
                link("https://t.me/+OldSpring", "Spring newsletter", 41, 0, 0, 0, false, true, false),
            ],
        }
    })
}

/// Demo people outside the chat list: removed members and those asking to join.
fn demo_person(id: i64) -> ChatInfo {
    let name = match id {
        104 => "Bohdan Tkach",
        105 => "Nataliia Humeniuk",
        106 => "Viktor Savchuk",
        _ => return demo_members().into_iter().find(|m| m.chat.id == PeerId::user_unchecked(id)).map(|m| m.chat).unwrap_or_else(|| person(id, "?", "", Presence::None)),
    };
    person(id, name, "", Presence::Recently)
}

fn manage(app: &mut App, cmd: Cmd) {
    let saved = |app: &mut App, peer| {
        app.push_event(Ev::Notice(crate::i18n::l!("Зміни збережено", "Changes saved").into()));
        app.push_event(Ev::ParticipantsChanged(peer));
    };
    let uid = |p: PeerId| p.bare_id_unchecked();
    match cmd {
        Cmd::LoadParticipants { peer, list } => {
            let items = MANAGE.with_borrow_mut(|m| {
                let m = demo_manage(m);
                let part = |id: i64| Participant { chat: demo_person(id), owner: id == 1, rank: String::new(), admin: None, can_edit: false, restricted: None, by: String::new() };
                let admin = |id: i64| match m.admins.iter().find(|a| a.0 == id) {
                    Some(a) => Participant { admin: Some(a.1), rank: a.2.clone(), can_edit: true, by: "Olena Koval".into(), ..part(id) },
                    None if id == 1 => Participant { admin: Some(AdminRights::all()), ..part(id) },
                    None => part(id),
                };
                match &list {
                    PartList::Admins => std::iter::once(1).chain(m.admins.iter().map(|a| a.0)).map(admin).collect::<Vec<_>>(),
                    PartList::Restricted => m.restricted.iter().map(|r| Participant { restricted: Some((r.1, r.2)), by: "Andriy".into(), ..part(r.0) }).collect(),
                    PartList::Removed => m.removed.iter().map(|id| Participant { by: "Andriy".into(), ..part(*id) }).collect(),
                    PartList::Search(q) => {
                        let q = q.to_lowercase();
                        let ids: Vec<i64> = demo_members().into_iter().filter(|x| x.chat.title.to_lowercase().contains(&q)).map(|x| uid(x.chat.id)).collect();
                        ids.into_iter().filter(|id| !m.removed.contains(id)).map(admin).collect()
                    }
                }
            });
            app.push_event(Ev::Participants { peer, list, items });
        }
        Cmd::EditAdmin { peer, user, rights, rank } => {
            MANAGE.with_borrow_mut(|m| {
                let m = demo_manage(m);
                m.admins.retain(|a| a.0 != uid(user));
                if let Some(r) = rights {
                    m.admins.push((uid(user), r, rank));
                }
            });
            saved(app, peer);
        }
        Cmd::RestrictMember { peer, user, rights, until } => {
            MANAGE.with_borrow_mut(|m| {
                let m = demo_manage(m);
                m.restricted.retain(|r| r.0 != uid(user));
                m.restricted.push((uid(user), rights, until));
            });
            saved(app, peer);
        }
        Cmd::BanMember { peer, user } | Cmd::UnbanMember { peer, user } => {
            let ban = matches!(cmd, Cmd::BanMember { .. });
            MANAGE.with_borrow_mut(|m| {
                let m = demo_manage(m);
                m.restricted.retain(|r| r.0 != uid(user));
                m.removed.retain(|r| *r != uid(user));
                if ban {
                    m.removed.push(uid(user));
                }
            });
            saved(app, peer);
        }
        _ => {}
    }
}

thread_local! {
    /// Free post searches left today in the demo.
    static POSTS_LEFT: std::cell::Cell<i32> = const { std::cell::Cell::new(3) };
}

fn invites(app: &mut App, cmd: Cmd) {
    let peer = match &cmd {
        Cmd::LoadInviteLinks(p) | Cmd::DeleteRevokedInvites(p) | Cmd::LoadJoinRequests(p) => *p,
        Cmd::SaveInviteLink { peer, .. } | Cmd::RevokeInviteLink { peer, .. } | Cmd::DeleteInviteLink { peer, .. } | Cmd::AnswerJoinRequest { peer, .. } => *peer,
        _ => return,
    };
    let requests = matches!(cmd, Cmd::LoadJoinRequests(_) | Cmd::AnswerJoinRequest { .. });
    MANAGE.with_borrow_mut(|m| {
        let m = demo_manage(m);
        match cmd {
            Cmd::SaveInviteLink { link: None, draft, .. } => {
                let n = m.links.len();
                m.links.insert(1, InviteLink { link: format!("https://t.me/+DemoLink{n}"), ..demo_link(draft) });
            }
            Cmd::SaveInviteLink { link: Some(link), draft, .. } => {
                if let Some(l) = m.links.iter_mut().find(|l| l.link == link) {
                    *l = InviteLink { link, usage: l.usage, requested: l.requested, ..demo_link(draft) };
                }
            }
            Cmd::RevokeInviteLink { link, .. } => m.links.iter_mut().filter(|l| l.link == link).for_each(|l| l.revoked = true),
            Cmd::DeleteInviteLink { link, .. } => m.links.retain(|l| l.link != link),
            Cmd::DeleteRevokedInvites(_) => m.links.retain(|l| !l.revoked),
            Cmd::AnswerJoinRequest { user, .. } => m.requests.retain(|r| user.is_some_and(|u| u.bare_id_unchecked() != r.0)),
            _ => {}
        }
    });
    if requests {
        let list = MANAGE.with_borrow_mut(|m| demo_manage(m).requests.iter().map(|(id, about)| JoinRequest { chat: demo_person(*id), date: "28.10 18:20".into(), about: (*about).into() }).collect());
        app.push_event(Ev::JoinRequests { peer, list });
    } else {
        let links = MANAGE.with_borrow_mut(|m| demo_manage(m).links.clone());
        app.push_event(Ev::InviteLinks { peer, links });
    }
}

fn demo_link(d: InviteDraft) -> InviteLink {
    InviteLink { title: d.title, usage_limit: d.usage_limit, expire: d.expire, request_needed: d.request_needed, ..Default::default() }
}

