//! Demo mode (`--demo`): made-up chats without network, to look at the interface.

use std::collections::HashMap;

use grammers_client::session::types::PeerId;

use crate::app::App;
use eframe::egui::{Color32, ColorImage};

use crate::backend::{Birthday, ChatRights, GlobalPrivacy, WebAppKind, WebAppOpened, NotifyException, NotifyScope, Sponsored, SponsoredReport, MUTE_FOREVER, Profile, Queued, UsernameState, BotCmd, Mentionable, WebPageInfo, ChatDetails, Member, MemberRole, NotifyState, SharedKind, Topic, Button, ButtonAction, GeoInfo, InlineResult, MediaInfo, MediaKind, PollInfo, PollOption, Reaction, ChatInfo, ChatKind, Cmd, Ev, Folder, ImgKey, Msg, Presence, SearchHit, StickerSetInfo};
use crate::format;

pub fn data() -> (String, Vec<ChatInfo>, HashMap<PeerId, Vec<Msg>>) {
    let chats = [
        (1, "Олена Коваль", ChatKind::User, 2, true),
        (2, "Команда Rust 🦀", ChatKind::Group, 14, true),
        (3, "Збережене", ChatKind::Saved, 0, true),
        (4, "Новини технологій", ChatKind::Channel, 128, false),
        (5, "Андрій", ChatKind::User, 0, false),
        (6, "WeatherBot", ChatKind::Bot, 0, false),
        (7, "Дизайн-студія", ChatKind::Group, 3, false),
        (8, "Марія Шевченко", ChatKind::User, 0, false),
        (9, "Старий проєкт", ChatKind::Group, 0, false),
    ];
    let scripts: [&[(bool, &str, &str)]; 9] = [
        &[
            (false, "Олена", "Привіт! Ти бачив новий дизайн у стилі liquid glass?"),
            (true, "", "Так, якраз роблю клієнт Telegram на Rust з таким інтерфейсом 😄"),
            (false, "Олена", "Ого, а як ти робиш розмиття та заломлення?"),
            (true, "", "Градієнтна заливка, спекулярний обід і внутрішній ореол на дальньому краю. Плюс живий фон із кольоровими плямами — скло виглядає «рідким»."),
            (false, "Олена", "Покажеш скріншот?"),
            (true, "", "📷"),
        ],
        &[
            (false, "Тарас", "Хто вже оновився на нову версію компілятора?"),
            (false, "Ірина", "Я, все зібралось з першого разу"),
            (true, "", "egui + grammers — пушу ввечері"),
            (false, "Тарас", "cargo clippy -- -D warnings 👀"),
            (false, "Ірина", "**Нагадую:** реліз у __п'ятницю__, ~~четвер~~ скасовано. Деталі — [у трекері](https://codeberg.org/Lonami/grammers), запуск: `cargo run --release`. ||Сюрприз для команди 🎁||"),
        ],
        &[(true, "", "Ідеї: темна тема, реакції, голосові повідомлення"), (false, "Новини технологій", "Rust 1.95: швидша компіляція та нові можливості const fn")],
        &[(false, "Новини технологій", "📷 Фото\nНовий стандарт Wi-Fi обіцяє вдвічі нижчі затримки.")],
        &[(false, "Андрій", "Завтра о 10:00 зідзвон?"), (true, "", "Так, домовились 👍")],
        &[(true, "", "/weather Київ"), (false, "WeatherBot", "☀ Київ: +18°, ясно, вітер 3 м/с")],
        &[(false, "Софія", "Скинула макети у Figma"), (false, "Софія", "📄"), (false, "Максим", "🐱"), (false, "Максим", "Супер, гляну")],
        &[(false, "Марія", "Дякую за допомогу!"), (false, "Марія", "Ось бот, про якого казала: [t.me/GlassHelperBot?start=from_maria](https://t.me/GlassHelperBot?start=from_maria)")],
        &[(false, "Олег", "Архівний чат: тут давно нічого не відбувається")],
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
                peer,
                out: *out,
                sender: sender.to_string(),
                spans: format::from_entities(&text, &ents, 0),
                forwarded: None,
                buttons: Vec::new(),
                group: None,
                reactions: Vec::new(),
                poll: None,
                geo: None,
                contact: None,
                media: None,
                video_sticker: false,
                media_unread: false,
                topic: None,
                comments: (id == 4 && !*out).then_some(3),
                // Pinned: each of these chats has something for the bar above history.
                pinned: matches!((id, i), (1, 3) | (2, 4) | (4, 0)),
                views: (id == 4).then_some(12_400),
                noforwards: false,
                webpage: None,
                saved_from: None,
                time: format!("{}:{:02}", 9 + i, (id * 7 + i as i64 * 13) % 60),
                ts: 0,
                day: "Сьогодні".into(),
                edited: false,
                reply_to: (id == 1 && i == 3).then_some(3),
                photo: text == "📷",
                file: (text == "📄").then(|| ("Макети_v3.fig".to_owned(), "14.2 МБ".to_owned())),
                sticker: (text == "🐱").then(|| (900, "🐱".to_owned())),
                text,
            })
            .map(|m| if m.file.is_some() || m.sticker.is_some() { Msg { text: String::new(), ..m } } else { m })
            .map(|m| if id == 3 && !m.out { Msg { forwarded: Some(m.sender.clone()), sender: String::new(), out: true, ..m } } else { m })
            .map(|m| if id == 6 && !m.out { Msg { buttons: weather_buttons(), ..m } } else { m })
            .map(|m| if m.photo { Msg { text: "Ось так виглядає фото у склі ✨".into(), ..m } } else { m })
            .collect();
        let last = msgs.last().unwrap();
        infos.push(ChatInfo {
            id: peer,
            title: title.into(),
            kind,
            last_text: if last.out { format!("Ви: {}", last.text) } else { last.text.replace('\n', " ") },
            last_time: last.time.clone(),
            last_ts: 100 - id,
            unread,
            pinned,
            contact: matches!(kind, ChatKind::User),
            muted: id == 4,
            archived: id == 9,
            unread_mark: false,
            draft: if id == 5 { "Не забути **презентацію**".into() } else { String::new() },
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
            // "Команда Rust" has two mentions and a reaction for us — shows the "@" and "❤" buttons.
            unread_mentions: if id == 2 { 2 } else { 0 },
            unread_reactions: if id == 2 { 1 } else { 0 },
            // "Дизайн-студія" has protected content: no forwarding or copying.
            noforwards: id == 7,
            can_pin: !matches!(kind, ChatKind::Channel) && id != 7,
        });
        let mut msgs = msgs;
        extras(id, peer, &mut msgs);
        history.insert(peer, msgs);
    }
    let (forum, msgs) = forum_chat();
    history.insert(forum.id, msgs);
    infos.insert(3, forum);
    let bot = helper_bot();
    history.insert(bot.id, Vec::new());
    infos.push(bot);
    ("Демо-користувач".into(), infos, history)
}

pub fn folders() -> Vec<Folder> {
    vec![
        // Pinned chats inside a folder can be dragged too.
        Folder {
            id: 1,
            title: "Особисті".into(),
            emoticon: "👤".into(),
            contacts: true,
            non_contacts: true,
            include: vec![PeerId::user_unchecked(8), PeerId::user_unchecked(5)],
            pinned: vec![PeerId::user_unchecked(8), PeerId::user_unchecked(5)],
            ..Default::default()
        },
        Folder { id: 2, title: "Групи".into(), emoticon: "👥".into(), groups: true, ..Default::default() },
        Folder { id: 3, title: "Канали й боти".into(), emoticon: "📢".into(), broadcasts: true, bots: true, ..Default::default() },
        Folder { id: 4, title: "Непрочитані".into(), emoticon: "✅".into(), contacts: true, non_contacts: true, groups: true, broadcasts: true, bots: true, exclude_read: true, ..Default::default() },
    ]
}

fn outgoing(peer: PeerId, markup: String, reply_to: Option<i32>) -> Msg {
    let (text, ents) = format::parse(&markup);
    Msg {
        spans: format::from_entities(&text, &ents, 0),
        forwarded: None,
        buttons: Vec::new(),
        group: None,
        reactions: Vec::new(),
        poll: None,
        geo: None,
        contact: None,
        media: None,
        video_sticker: false,
        media_unread: false,
        topic: None,
        comments: None,
        pinned: false,
        views: None,
        noforwards: false,
        webpage: None,
        saved_from: None,
        id: 10_000 + rand_ish(),
        peer,
        out: true,
        sender: String::new(),
        text,
        time: jiff::Zoned::now().strftime("%H:%M").to_string(),
        ts: jiff::Timestamp::now().as_second(),
        day: "Сьогодні".into(),
        edited: false,
        reply_to,
        photo: false,
        file: None,
        sticker: None,
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
            app.push_event(Ev::Notice("Заплановано".into()));
        }
        Cmd::DeleteScheduled { peer, ids } | Cmd::SendScheduledNow { peer, ids } => {
            let (gone, keep): (Vec<Msg>, Vec<Msg>) = app.demo_scheduled(peer).into_iter().partition(|m| ids.contains(&m.id));
            app.push_event(Ev::Scheduled { peer, msgs: keep });
            if matches!(cmd_kind, Some(true)) {
                for m in gone {
                    app.push_event(Ev::NewMsg(Msg { day: "Сьогодні".into(), ..m }));
                }
            }
        }
        Cmd::InlineQuery { peer, bot, query } => {
            let results = (1..=4)
                .map(|i| InlineResult { id: i.to_string(), title: format!("{query} — результат {i}"), description: format!("Від @{bot}"), kind: "article".into() })
                .collect();
            app.push_event(Ev::InlineResults { peer, bot, query, query_id: 1, results });
        }
        Cmd::SendInline { peer, result, .. } => app.push_event(Ev::NewMsg(outgoing(peer, format!("Inline-результат №{result}"), None))),
        Cmd::Send { peer, text, .. } if peer == PeerId::user_unchecked(HELPER_BOT) && text.starts_with('/') => {
            app.push_event(Ev::NewMsg(outgoing(peer, text.clone(), None)));
            let answer = match text.as_str() {
                "/start" => "Привіт! Я демо-бот Glassgram. Спробуйте /help або кнопку меню біля поля вводу.",
                "/help" => "Команди: /start, /help, /about — або оберіть у меню «/».",
                _ => "Glassgram — клієнт Telegram на Rust з «рідким склом».",
            };
            app.push_event(Ev::NewMsg(Msg { out: false, sender: "GlassHelperBot".into(), id: 20_000 + rand_ish(), ..outgoing(peer, answer.into(), None) }));
        }
        Cmd::WebPreview(text) => {
            let site = text.split("://").last().unwrap_or_default().split('/').next().unwrap_or_default().to_owned();
            let page = WebPageInfo {
                id: text.len() as i64,
                url: text.clone(),
                title: format!("{site} — демо-сторінка"),
                description: "У демо-режимі прев'ю складається без мережі.".into(),
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
                p if p == PeerId::user_unchecked(6) => &[("weatherbot", "weather", "Погода зараз"), ("weatherbot", "forecast", "Прогноз на тиждень"), ("weatherbot", "settings", "Місто й одиниці")],
                p if p == PeerId::user_unchecked(HELPER_BOT) => &[("glasshelperbot", "start", "Почати"), ("glasshelperbot", "help", "Довідка"), ("glasshelperbot", "about", "Про Glassgram")],
                p if p == PeerId::user_unchecked(2) => &[("rustdocsbot", "docs", "Знайти в документації"), ("rustdocsbot", "crate", "Інформація про крейт")],
                _ => &[],
            };
            let commands = list.iter().map(|(bot, command, description)| BotCmd { bot: (*bot).into(), command: (*command).into(), description: (*description).into() }).collect();
            // The weather bot has a mini app behind its menu button.
            if peer == PeerId::user_unchecked(6) {
                app.push_event(Ev::BotMenuApp { peer, text: "Радар".into(), url: "https://example.org/radar".into() });
            }
            app.push_event(Ev::BotCommands { peer, commands });
        }
        Cmd::Send { peer, text, reply_to, .. } => {
            // In a forum, a reply goes into the topic of the message being replied to (or the topic itself).
            let topic = reply_to.filter(|_| peer == PeerId::channel_unchecked(FORUM)).map(|r| {
                app.demo_history().find(|(p, _, _)| *p == peer).and_then(|(_, _, msgs)| msgs.iter().find(|m| m.id == r).map(Msg::topic_id)).unwrap_or(r)
            });
            app.push_event(Ev::NewMsg(Msg { topic: topic.filter(|t| *t != 1), ..outgoing(peer, text, reply_to) }));
        }
        Cmd::Forward { from, ids, to } => {
            let src = app.demo_history().find(|(p, _, _)| *p == from).map(|(_, (title, _), msgs)| (title.to_owned(), msgs.clone()));
            for m in src.iter().flat_map(|(title, msgs)| msgs.iter().filter(|m| ids.contains(&m.id)).map(move |m| (title, m))) {
                let from_name = if m.1.out { "Демо-користувач".to_owned() } else if m.1.sender.is_empty() { m.0.clone() } else { m.1.sender.clone() };
                let mut copy = outgoing(to, String::new(), None);
                copy = Msg { text: m.1.text.clone(), spans: m.1.spans.clone(), photo: m.1.photo, file: m.1.file.clone(), forwarded: Some(from_name), id: copy.id + m.1.id, ..copy };
                app.push_event(Ev::NewMsg(copy));
            }
        }
        Cmd::PressButton { data, .. } => {
            let text = match data.as_slice() {
                b"tomorrow" => "Завтра в Києві: +21°, мінлива хмарність",
                _ => "Оновлено: +18°, ясно",
            };
            app.push_event(Ev::BotAnswer { text: text.into(), alert: false });
        }
        Cmd::SendFiles { id, peer, paths, caption, reply_to, .. } => {
            for (i, path) in paths.iter().enumerate() {
                let name = path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
                let size = std::fs::metadata(path).map(|m| crate::backend::human_size(m.len() as usize)).unwrap_or_default();
                let text = if i == 0 { caption.clone() } else { String::new() };
                let mut m = outgoing(peer, text, reply_to);
                m.id += i as i32;
                app.push_event(Ev::NewMsg(Msg { file: Some((name, size)), ..m }));
            }
            app.push_event(Ev::UploadDone(id));
        }
        Cmd::LoadStickers => {
            let set = |id: i64, title: &str, emojis: &str| StickerSetInfo {
                id,
                title: title.into(),
                stickers: emojis.chars().enumerate().map(|(i, c)| (id * 100 + i as i64, c.to_string())).collect(),
            };
            app.push_event(Ev::Stickers(vec![
                set(1, "Нещодавні", "🐱👍😂🔥❤🎉"),
                set(2, "Котики", "🐱😺😸😹😻😼😽🙀😿😾🐈"),
                set(3, "Емоції", "😀😃😄😁😆😅🤣😂🙂🙃😉😊😇🥰😍🤩😘"),
            ]));
        }
        Cmd::SendSticker { peer, doc, reply_to } => {
            let emoji = ["🐱", "😺", "😀", "👍", "🔥"][doc as usize % 5].to_owned();
            app.push_event(Ev::NewMsg(Msg { sticker: Some((doc, emoji)), ..outgoing(peer, String::new(), reply_to) }));
        }
        Cmd::SendVoice { id, peer, path, duration, waveform, reply_to } => {
            let info = MediaInfo { kind: MediaKind::Voice, duration, size: (0, 0), title: String::new(), performer: String::new(), waveform };
            let m = Msg { media: Some(info), ..outgoing(peer, String::new(), reply_to) };
            let mid = m.id;
            app.push_event(Ev::NewMsg(m));
            // The file stays on disk: that's what we will play.
            app.push_event(Ev::MediaReady { peer, id: mid, path });
            app.push_event(Ev::UploadDone(id));
        }
        Cmd::SaveFolder(f) => {
            let mut list = app.demo_folders();
            match list.iter_mut().find(|x| x.id == f.id) {
                Some(x) => *x = f,
                None => list.push(f),
            }
            app.push_event(Ev::Folders(list));
        }
        Cmd::DeleteFolder(id) => {
            let list = app.demo_folders().into_iter().filter(|f| f.id != id).collect();
            app.push_event(Ev::Folders(list));
        }
        Cmd::OpenComments { peer, id, .. } => {
            // Discussion for the "Новини технологій" channel: a separate made-up group.
            let disc = PeerId::channel_unchecked(40);
            let c = |cid: i32, sender: &str, text: &str, reply: Option<i32>| Msg {
                id: cid,
                sender: sender.into(),
                text: text.into(),
                time: format!("10:{:02}", cid % 60),
                day: "Сьогодні".into(),
                reply_to: reply.or(Some(1)),
                ..Msg::placeholder(disc)
            };
            let msgs = vec![
                c(2, "Олег", "Нарешті! Чекаю на роутери з підтримкою.", None),
                c(3, "Марта", "А телефони вже вміють?", None),
                c(4, "Олег", "Флагмани — так, з минулого року.", Some(3)),
            ];
            app.push_event(Ev::Comments { peer, post: id, discussion: disc, root: 1, msgs, older: false });
        }
        Cmd::LoadTopics(peer) => app.push_event(Ev::Topics { peer, topics: demo_topics() }),
        Cmd::CreateTopic { .. } | Cmd::EditTopic { .. } => app.push_event(Ev::Notice("У демо-режимі теми не змінюються".into())),
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
            app.push_event(Ev::Notice("У демо-режимі експорт недоступний".into()));
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
                s(1, "Glassgram 0.1", "Linux · Ubuntu", "Україна · 93.170.0.1", "зараз", true),
                s(2, "Telegram Android 11.2", "Pixel 7 · Android 15", "Україна · 46.211.0.7", "Сьогодні 09:41", false),
                s(3, "Telegram Desktop 5.6", "Windows 11", "Польща · 31.0.12.4", "12.10 18:02", false),
            ]));
        }
        Cmd::TerminateSession(_) | Cmd::TerminateOtherSessions => app.push_event(Ev::Notice("У демо-режимі сеанси не завершуються".into())),
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
        Cmd::SendRound { id, peer, path, duration, reply_to } => {
            let info = MediaInfo { kind: MediaKind::Round, duration, size: (384, 384), title: String::new(), performer: String::new(), waveform: Vec::new() };
            let m = Msg { media: Some(info), ..outgoing(peer, String::new(), reply_to) };
            let mid = m.id;
            app.push_event(Ev::NewMsg(m));
            app.push_event(Ev::MediaReady { peer, id: mid, path });
            app.push_event(Ev::UploadDone(id));
        }
        Cmd::Download { .. } => app.push_event(Ev::Notice("У демо-режимі файли не завантажуються".into())),
        Cmd::LoadMedia { peer, id } => match tone(id).or_else(|| clip(id)) {
            Some(path) => app.push_event(Ev::MediaReady { peer, id, path }),
            None => app.push_event(Ev::Notice("Для демо-відео потрібен ffmpeg".into())),
        },
        Cmd::Delete { peer, ids } => app.push_event(Ev::Deleted { ids, peer: Some(peer) }),
        Cmd::LoadOlder { peer, .. } => app.push_event(Ev::History { peer, msgs: vec![], older: true }),
        Cmd::Logout => app.push_event(Ev::Error("У демо-режимі вихід недоступний".into())),
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
                .map(|c| NotifyException { peer: c.id, title: c.title.clone(), state: NotifyState { mute_until: if c.muted { MUTE_FOREVER } else { 0 }, silent: false, inherit: false } })
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
            let ads = if peer == PeerId::user_unchecked(DEMO_CHANNEL) { vec![demo_ad()] } else { Vec::new() };
            app.push_event(Ev::Sponsored { peer, ads });
        }
        Cmd::ViewSponsored(_) | Cmd::ClickSponsored(_) => {}
        Cmd::ReportSponsored { peer, random_id, option } => {
            let result = if option.is_empty() {
                let reasons = ["Не цікавить", "Оманлива реклама", "Шахрайство", "Інше"];
                SponsoredReport::Choose { title: "Чому ви скаржитесь?".into(), options: reasons.iter().map(|r| (r.to_string(), r.as_bytes().to_vec())).collect() }
            } else {
                SponsoredReport::Reported
            };
            app.push_event(Ev::SponsoredReport { peer, random_id, result });
        }
        Cmd::Translate { peer, id, to } => {
            let text = app.demo_history().find(|(p, _, _)| *p == peer).and_then(|(_, _, msgs)| msgs.iter().find(|m| m.id == id)).map(|m| m.text.clone()).unwrap_or_default();
            // A couple of real translations, the rest just marked with the target language.
            let known = [
                ("Привіт! Ти бачив новий дизайн у стилі liquid glass?", "Hi! Have you seen the new liquid glass design?"),
                ("Ого, а як ти робиш розмиття та заломлення?", "Wow, how do you do the blur and refraction?"),
            ];
            let translated = match known.iter().find(|(uk, _)| *uk == text) {
                Some((uk, en)) => if to == "uk" { uk.to_string() } else { en.to_string() },
                None => format!("[{to}] {text}"),
            };
            app.push_event(Ev::Translated { peer, id, to, result: Ok((translated, Vec::new())) });
        }
        Cmd::JumpToDate { peer, date } => {
            let msgs = app.demo_history().find(|(p, _, _)| *p == peer).map(|(_, _, msgs)| msgs.clone()).unwrap_or_default();
            let id = msgs.iter().find(|m| m.ts >= date as i64).or(msgs.last()).map(|m| m.id);
            app.push_event(Ev::DateFound { peer, id });
        }
        Cmd::LoadReadBy { peer, id } => {
            let readers = [("Тарас", "Сьогодні 11:02"), ("Ірина", "Сьогодні 11:15"), ("Остап", "Сьогодні 12:40")];
            app.push_event(Ev::ReadBy { peer, id, readers: readers.iter().map(|(n, t)| (n.to_string(), t.to_string())).collect() });
        }
        Cmd::LoadChatInfo(peer) => {
            let kind = app.demo_history().find(|(p, _, _)| *p == peer).map(|(_, (_, k), _)| k);
            let mut details = demo_details(peer, kind);
            details.counts = SharedKind::ALL.into_iter().map(|k| (k, demo_shared(app, peer, k).len() as i32)).collect();
            app.push_event(Ev::ChatDetails { peer, details });
        }
        Cmd::LoadMembers { peer, query, admins } => {
            let q = query.to_lowercase();
            let list = demo_members().into_iter().filter(|m| (!admins || m.role != MemberRole::Member) && m.chat.title.to_lowercase().contains(&q)).collect();
            app.push_event(Ev::Members { peer, query, admins, list });
        }
        Cmd::LoadMediaDays { peer, before } => {
            // A few days with photos in the month before `before`.
            let days = [9, 10, 17, 24, 30].into_iter().map(|back| (before - back * 86_400 - 3 * 3600, (back % 4 + 1) as i32)).collect();
            app.push_event(Ev::MediaDays { peer, days });
        }
        Cmd::LoadSavedDialogs => {
            // Saved Messages (id 3) hold our own note, a channel post and a photo from Олена.
            let list = [3, 4, 1].into_iter().filter_map(|id| app.demo_chat(PeerId::user_unchecked(id)).cloned()).collect();
            app.push_event(Ev::SavedDialogs(list));
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
                forwarded: Some("Олена Коваль".into()),
                saved_from: Some(PeerId::user_unchecked(1)),
                ..outgoing(saved, "Макет нового екрана налаштувань — глянь увечері".into(), None)
            });
            msgs.retain(|m| m.saved_from == Some(source));
            app.push_event(Ev::SavedHistory { source, msgs, older: false });
        }
        Cmd::LoadSimilarChannels(peer) => {
            let chats = [(61, "Rust Weekly", "12 480"), (62, "Дизайн щодня", "8 115"), (63, "Open Source UA", "3 902")]
                .into_iter()
                .map(|(id, title, n)| ChatInfo {
                    id: PeerId::channel_unchecked(id),
                    kind: ChatKind::Channel,
                    contact: false,
                    joined: false,
                    can_post: false,
                    ..person(0, title, &crate::i18n::lf!("підписників: {n}", "{n} subscribers"), Presence::None)
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
        Cmd::SetNotify { peer, mute_until, silent } => app.push_event(Ev::Notify { peer, state: NotifyState { mute_until, silent, inherit: false } }),
        Cmd::LeaveChat(peer) | Cmd::DeleteChat { peer, .. } => app.push_event(Ev::ChatRemoved(peer)),
        Cmd::ClearHistory { peer, .. } => app.push_event(Ev::HistoryCleared(peer)),
        Cmd::LoadProfile => {
            let p = app.demo_profile();
            app.push_event(Ev::Profile(if p.id.is_some() { p } else { demo_profile() }));
        }
        Cmd::SaveProfile { first, last, bio } => {
            app.push_event(Ev::Profile(Profile { first, last, bio, ..app.demo_profile() }));
            app.push_event(Ev::Notice("Профіль збережено (демо)".into()));
        }
        // A few names are "taken" so every answer can be seen.
        Cmd::CheckUsername(name) => {
            let taken = ["durov", "telegram", "glassgram", "admin"].iter().any(|t| name.eq_ignore_ascii_case(t));
            app.push_event(Ev::UsernameCheck { name, state: if taken { UsernameState::Taken } else { UsernameState::Free } });
        }
        Cmd::SetUsername(username) => {
            app.push_event(Ev::Profile(Profile { username, ..app.demo_profile() }));
            app.push_event(Ev::Notice("Ім'я користувача збережено (демо)".into()));
        }
        Cmd::SetProfilePhoto(path) => match image::open(&path) {
            Ok(img) => {
                let side = img.width().min(img.height());
                let img = img.crop_imm((img.width() - side) / 2, (img.height() - side) / 2, side, side).thumbnail(256, 256).to_rgba8();
                let image = ColorImage::from_rgba_unmultiplied([img.width() as usize, img.height() as usize], img.as_raw());
                app.push_event(Ev::Image { key: ImgKey::Avatar(PeerId::user_unchecked(ME)), image });
                app.push_event(Ev::Notice("Фото профілю оновлено (демо)".into()));
            }
            Err(e) => app.push_event(Ev::Error(e.to_string())),
        },
        // `#offline` in the text pretends there is no network: the clock, retries, then the red mark.
        Cmd::SendQueued(q) if q.text.contains("#offline") => {
            app.push_event(Ev::QueueFailed { key: q.key, error: "Не надіслано: немає мережі (демо)".into(), retry: true });
        }
        Cmd::SendQueued(q) => {
            let topic = q.reply_to.filter(|_| q.peer == PeerId::channel_unchecked(FORUM)).map(|r| {
                app.demo_history().find(|(p, _, _)| *p == q.peer).and_then(|(_, _, msgs)| msgs.iter().find(|m| m.id == r).map(Msg::topic_id)).unwrap_or(r)
            });
            app.push_event(Ev::NewMsg(Msg { topic: topic.filter(|t| *t != 1), ..outgoing(q.peer, q.text, q.reply_to) }));
            app.push_event(Ev::QueueSent { key: q.key, peer: q.peer });
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
            };
            app.push_event(Ev::WebAppOpened(WebAppOpened { peer, bot, kind, url, query_id: None }));
        }
        Cmd::OpenBotStart { param, .. } => {
            app.push_event(Ev::ChatFound(helper_bot()));
            app.push_event(Ev::BotStart { peer: PeerId::user_unchecked(HELPER_BOT), param });
        }
        Cmd::StartBot { peer, param } => {
            app.push_event(Ev::NewMsg(outgoing(peer, "/start".into(), None)));
            let answer = format!("Привіт! Ви прийшли за посиланням з параметром «{param}».");
            app.push_event(Ev::NewMsg(Msg { out: false, sender: "GlassHelperBot".into(), id: 20_000 + rand_ish(), ..outgoing(peer, answer, None) }));
        }
        Cmd::OpenWebAppLink { username, .. } => app.push_event(Ev::Notice(format!("t.me/{username}"))),
        Cmd::ProlongWebApp { .. } | Cmd::WebAppData { .. } | Cmd::WebAppMethod { .. } => {}
        Cmd::SendPoll { peer, poll, reply_to } => {
            let answers = poll
                .options
                .iter()
                .enumerate()
                .map(|(i, o)| PollOption { text: o.clone(), option: vec![i as u8], voters: 0, chosen: false, correct: poll.quiz == Some(i) })
                .collect();
            let info = PollInfo { question: poll.question, answers, total: 0, closed: false, quiz: poll.quiz.is_some(), multiple: poll.multiple };
            app.push_event(Ev::NewMsg(Msg { id: rand_ish(), poll: Some(info), ..outgoing(peer, String::new(), reply_to) }));
        }
        Cmd::LoadGlobalPrivacy => app.push_event(Ev::GlobalPrivacy(GlobalPrivacy { archive_new: true, keep_unmuted: false, keep_folders: true, hide_read: false, premium_only: false })),
        Cmd::SetGlobalPrivacy(p) => app.push_event(Ev::GlobalPrivacy(p)),
        Cmd::EditChat { peer, title, .. } => {
            if let Some(title) = title {
                app.push_event(Ev::ChatTitle { peer, title });
            }
            app.push_event(Ev::Notice(crate::i18n::l!("Зміни збережено", "Changes saved").into()));
        }
        Cmd::SetChatPhoto { .. } | Cmd::SetChatRights { .. } | Cmd::SetSlowMode { .. } => app.push_event(Ev::Notice(crate::i18n::l!("Зміни збережено", "Changes saved").into())),
        Cmd::SetBirthday(birthday) => app.push_event(Ev::Profile(Profile { birthday, ..app.demo_profile() })),
        Cmd::LoadBirthdays => {
            // Олена has her birthday today, so the reminder is always there in the demo.
            let olena = app.demo_chats().iter().find(|c| c.id == PeerId::user_unchecked(1)).cloned();
            app.push_event(Ev::Birthdays(olena.into_iter().map(|c| (c, today_birthday(Some(1996)))).collect()));
        }
        Cmd::LoadContacts => app.push_event(Ev::Contacts(demo_contacts())),
        Cmd::AddContact { user, phone, first, last } => {
            let name = if last.is_empty() { first } else { format!("{first} {last}") };
            let c = match user {
                Some(peer) => ChatInfo { title: name, contact: true, ..app.demo_chat(peer).cloned().unwrap_or_else(|| person(0, "", "", Presence::None)) },
                // A made-up account behind any number.
                None => person(40 + phone.len() as i64, &name, "був(ла) нещодавно", Presence::None),
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
            created(app, &title, ChatKind::Group, format!("Групу «{title}» створено · учасників: {n}"));
        }
        Cmd::CreateChannel { title, about } => {
            let text = if about.is_empty() { format!("Канал «{title}» створено") } else { format!("Канал «{title}» створено\n{about}") };
            created(app, &title, ChatKind::Channel, text);
        }
        // Contacts without a demo history open as an empty chat.
        Cmd::OpenChat(peer) if !app.demo_history().any(|(p, ..)| p == peer) => {
            app.push_event(Ev::History { peer, msgs: Vec::new(), older: false });
        }
        Cmd::Typing(peer) if peer == PeerId::user_unchecked(1) => {
            // Olena "replies" when text is typed.
            app.push_event(Ev::Typing { peer, who: "Олена".into(), active: true });
        }
        _ => {}
    }
}

thread_local! {
    /// Demo blocked list: starts with one spammer, follows Block / Unblock.
    static BLOCKED: std::cell::RefCell<Vec<ChatInfo>> = std::cell::RefCell::new(vec![person(50, "Спам-розсилка", "", Presence::None)]);
}

/// People found only by global search in the demo: (id, name, username).
const GLOBAL_PEOPLE: [(i64, &str, &str); 3] = [(30, "Олександр Мельник", "melnyk"), (31, "Олена Петренко", "olena_p"), (32, "Rust Weekly", "rustweekly")];

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
        unread_mentions: 0,
        unread_reactions: 0,
        noforwards: false,
        can_pin: false,
    }
}

/// Demo contacts: people from the chat list plus a few without any chat yet.
fn demo_contacts() -> Vec<ChatInfo> {
    let now = jiff::Timestamp::now().as_second();
    let mut list = vec![
        person(1, "Олена Коваль", "@olena", Presence::Online(i64::MAX)),
        person(5, "Андрій", "@andriy", Presence::Offline(now - 3600)),
        person(8, "Марія Шевченко", "", Presence::Recently),
        person(20, "Ігор Бондар", "@bondar", Presence::LastWeek),
        person(21, "Наталія Лисенко", "", Presence::Offline(now - 86_400 * 3)),
        person(22, "Тетяна", "@tanya", Presence::Online(i64::MAX)),
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
/// Our message in "Команда Rust" with an unread reaction.
const DEMO_REACTED: i32 = 3;
/// The channel that shows a sponsored message.
const DEMO_CHANNEL: i64 = 4;
const DEMO_AD_PHOTO: i64 = 77;

fn demo_ad() -> Sponsored {
    let text = "Щотижневий огляд нових крейтів, релізів і статей про **Rust**. Без води.";
    let (text, entities) = format::parse(text);
    Sponsored {
        random_id: vec![7; 16],
        title: "Rust Weekly".into(),
        spans: format::from_entities(&text, &entities, 0),
        text,
        url: "https://t.me/rustweekly".into(),
        button: "Переглянути канал".into(),
        recommended: false,
        can_report: true,
        sponsor_info: "ТОВ «Демо Медіа», ЄДРПОУ 00000000".into(),
        additional_info: "Реклама розміщена через Telegram Ads.".into(),
        photo: Some(DEMO_AD_PHOTO),
    }
}
/// Group members for @mentions and the search sender filter: (id, name, username, bot).
const MENTIONABLE: [(i64, &str, &str, bool); 5] =
    [(31, "Тарас Бойко", "taras_b", false), (32, "Ірина Ткаченко", "iryna", false), (33, "Софія", "", false), (34, "Максим Лисенко", "max_l", false), (35, "RustDocsBot", "rustdocsbot", true)];

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
        unread_mentions: 0,
        unread_reactions: 0,
        noforwards: false,
        can_pin: false,
    }
}
/// The demo user's own id (for the avatar in settings).
const ME: i64 = 1000;

fn demo_profile() -> Profile {
    Profile {
        id: Some(PeerId::user_unchecked(ME)),
        first: "Демо".into(),
        last: "Користувач".into(),
        bio: "Пишу клієнт Telegram на Rust 🦀".into(),
        username: "glassgram_demo".into(),
        phone: "+380 00 000 00 00".into(),
        birthday: Some(Birthday { day: 12, month: 5, year: None }),
    }
}

/// Send queue "from the previous run": one message that failed, to show the red mark and its menu.
pub fn outbox() -> Vec<Queued> {
    let ts = jiff::Timestamp::now().as_second() - 120;
    vec![Queued { key: 1, peer: PeerId::user_unchecked(5), text: "Скинув звіт — глянь, коли будеш онлайн".into(), reply_to: None, silent: false, ts, failed: true, no_webpage: false }]
}

/// Demo forum topics: (id, title, color, unread, pinned, closed).
const TOPICS: [(i32, &str, u32, i32, bool, bool); 4] = [
    (1, "Загальне", 0, 0, false, false),
    (100, "Допомога новачкам", 0x6FB9F0, 3, true, false),
    (200, "Реліз 1.0", 0xFFD67E, 1, false, false),
    (300, "Архів обговорень", 0xCB86DB, 0, false, true),
];

/// Forum group "Rust Україна" with several topics.
fn forum_chat() -> (ChatInfo, Vec<Msg>) {
    let peer = PeerId::channel_unchecked(FORUM);
    let script: [(i32, i32, &str, &str); 7] = [
        (1, 1, "Модератор", "Ласкаво просимо до форуму! Питання — у «Допомогу новачкам»."),
        (100, 101, "Остап", "Як позбутися `borrow of moved value`?"),
        (100, 102, "Леся", "Клонуй або передавай посилання: `&value`."),
        (100, 103, "", "Ще варто прочитати розділ про **власність** у книзі."),
        (200, 201, "Тарас", "Збираємо реліз 1.0 у п'ятницю 🦀"),
        (200, 202, "Ірина", "Changelog готовий, лишився реліз-нотс."),
        (300, 301, "Модератор", "Тему закрито."),
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
                day: "Сьогодні".into(),
                topic: (topic != 1).then_some(topic),
                ..Msg::placeholder(peer)
            }
        })
        .collect();
    let chat = ChatInfo {
        id: peer,
        title: "Rust Україна".into(),
        kind: ChatKind::Group,
        last_text: "Ірина: Changelog готовий, лишився реліз-нотс.".into(),
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
        unread_mentions: 0,
        unread_reactions: 0,
        noforwards: false,
        can_pin: true,
    };
    (chat, msgs)
}

fn demo_topics() -> Vec<Topic> {
    TOPICS
        .iter()
        .map(|&(id, title, color, unread, pinned, closed)| {
            let last = match id {
                1 => "Модератор: Ласкаво просимо до форуму!",
                100 => "Ви: Ще варто прочитати розділ про власність у книзі.",
                200 => "Ірина: Changelog готовий, лишився реліз-нотс.",
                _ => "Модератор: Тему закрито.",
            };
            Topic { id, title: title.into(), color, unread, pinned, closed, last_text: last.into(), last_time: "12:02".into(), last_ts: 0 }
        })
        .collect()
}

/// Extra demo messages: poll, place, contact, album, reactions.
fn extras(id: i64, peer: PeerId, msgs: &mut Vec<Msg>) {
    let base = |n: i32, sender: &str| Msg { sender: sender.into(), out: sender.is_empty(), time: format!("12:{n:02}"), ..Msg::placeholder(peer) };
    match id {
        2 => {
            // A link with a preview card (its picture comes from `images()`).
            if let Some(m) = msgs.iter_mut().find(|m| m.id == 5) {
                m.webpage = Some(WebPageInfo {
                    id: 1,
                    url: "https://codeberg.org/Lonami/grammers".into(),
                    site: "Codeberg.org".into(),
                    title: "Lonami/grammers".into(),
                    description: "(tele)gramme.rs — набір крейтів Rust для роботи з Telegram MTProto API".into(),
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
                    question: "Коли робимо реліз?".into(),
                    answers: vec![opt("У п'ятницю", 4, 0), opt("Наступного тижня", 2, 1), opt("Коли буде готово 🦀", 5, 2)],
                    total: 11,
                    closed: false,
                    quiz: false,
                    multiple: false,
                }),
                pinned: true,
                ..base(10, "Тарас")
            });
            msgs.push(Msg {
                id: 51,
                geo: Some(GeoInfo { lat: 50.4501, lon: 30.5234, title: "Офіс".into(), address: "Київ, Хрещатик, 1".into(), live: false }),
                text: "Збираємось тут о 10:00".into(),
                ..base(12, "Ірина")
            });
        }
        1 => {
            msgs.push(Msg { id: 60, contact: Some(("Андрій Мельник".into(), "+380 67 123 45 67".into())), ..base(20, "Олена") });
            for (k, t) in ["Захід сонця з балкона 🌅", "", ""].iter().enumerate() {
                msgs.push(Msg { id: 70 + k as i32, photo: true, group: Some(7), text: t.to_string(), ..base(21, "") });
            }
            let media = |kind, duration, title: &str, performer: &str| MediaInfo {
                kind,
                duration,
                size: (0, 0),
                title: title.into(),
                performer: performer.into(),
                waveform: (0..63u32).map(|i| ((i * 7 % 13) as f32 * 1.6 + (i as f32 * 0.4).sin().abs() * 12.0) as u8).collect(),
            };
            msgs.push(Msg { id: 80, media: Some(media(MediaKind::Voice, 4.0, "", "")), ..base(22, "Олена") });
            msgs.push(Msg { id: 81, media: Some(media(MediaKind::Voice, 3.0, "", "")), media_unread: true, ..base(22, "Олена") });
            msgs.push(Msg { id: 82, media: Some(media(MediaKind::Audio, 5.0, "Скляний ранок", "Glassgram")), ..base(23, "") });
            msgs.push(Msg {
                id: 83,
                media: Some(MediaInfo { size: (640, 360), ..media(MediaKind::Video, 6.0, "", "") }),
                text: "Відео з концерту".into(),
                ..base(24, "Олена")
            });
            msgs.push(Msg { id: 84, media: Some(MediaInfo { size: (480, 270), ..media(MediaKind::Gif, 3.0, "", "") }), ..base(25, "") });
            msgs.push(Msg { id: 85, media: Some(MediaInfo { size: (240, 240), ..media(MediaKind::Round, 5.0, "", "") }), media_unread: true, ..base(26, "Олена") });
        }
        9 => {
            // A long history: shows virtualization (only bubbles near the screen are drawn).
            const LONG: i32 = 3000;
            let lines = ["Хто сьогодні чергує?", "Я, після обіду", "Збірка впала на CI, дивлюсь", "Полагодив, була застаріла залежність", "👍", "Нагадую про ретро в п'ятницю"];
            let now = jiff::Zoned::now();
            let old = (1..=LONG).map(|n| {
                let date = now.checked_sub(jiff::Span::new().days(i64::from((LONG - n) / 40 + 1))).ok();
                let day = date.as_ref().map_or_else(String::new, |d| d.strftime("%d.%m.%Y").to_string());
                // Real times so the calendar can jump through this history.
                let ts = date.map_or(0, |d| d.timestamp().as_second());
                let sender = ["Олег", "Наталя", ""][(n % 3) as usize];
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
        vec![b("Оновити", ButtonAction::Callback(b"now".to_vec())), b("Завтра", ButtonAction::Callback(b"tomorrow".to_vec()))],
        vec![b("Мапа погоди", ButtonAction::Url("https://www.windy.com".into())), b("Радар", ButtonAction::WebApp { url: "https://example.org/radar".into(), simple: false, bot: 6 })],
    ]
}

/// Demo sound for voice and audio: a pulsing chord as WAV (16-bit, mono).
fn tone(id: i32) -> Option<std::path::PathBuf> {
    let secs = match id {
        80 => 4.0,
        81 => 3.0,
        82 => 5.0,
        _ => return None,
    };
    const RATE: u32 = 22_050;
    let n = (RATE as f32 * secs) as u32;
    let base = if id == 82 { 220.0 } else { 330.0 };
    let samples = (0..n).map(|i| {
        let t = i as f32 / RATE as f32;
        let env = (t * 3.0 * std::f32::consts::PI).sin().abs() * (1.0 - t / secs).min(t * 20.0).min(1.0);
        let v: f32 = [1.0, 1.25, 1.5].iter().map(|k| (t * base * k * std::f32::consts::TAU).sin()).sum::<f32>() / 3.0;
        (v * env * 0.4 * i16::MAX as f32) as i16
    });
    let mut wav = Vec::with_capacity(44 + n as usize * 2);
    wav.extend_from_slice(b"RIFF");
    wav.extend_from_slice(&(36 + n * 2).to_le_bytes());
    wav.extend_from_slice(b"WAVEfmt ");
    for v in [16u32.to_le_bytes(), [1, 0, 1, 0], RATE.to_le_bytes(), (RATE * 2).to_le_bytes(), [2, 0, 16, 0]] {
        wav.extend_from_slice(&v);
    }
    wav.extend_from_slice(b"data");
    wav.extend_from_slice(&(n * 2).to_le_bytes());
    samples.for_each(|s| wav.extend_from_slice(&s.to_le_bytes()));
    let path = std::env::temp_dir().join(format!("glassgram-demo-{id}.wav"));
    std::fs::write(&path, wav).ok()?;
    Some(path)
}

/// Demo videos from ffmpeg test sources: video with sound, silent GIF, round video.
fn clip(id: i32) -> Option<std::path::PathBuf> {
    let (video, audio, secs) = match id {
        83 => ("testsrc2=size=640x360:rate=30", true, 6),
        84 => ("mandelbrot=size=480x270:rate=30", false, 3),
        85 => ("life=size=240x240:mold=10:rate=30:ratio=0.5:death_color=#5a3cc8:life_color=#ffd6f0", true, 5),
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
        (ImgKey::Avatar(PeerId::user_unchecked(5)), sunset(96, 96)),
        (ImgKey::Photo(PeerId::user_unchecked(2), 5), sunset(480, 250)),
        (ImgKey::Sponsored(DEMO_AD_PHOTO), sunset(112, 112)),
    ]
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
                1 => "Дизайнерка інтерфейсів. Люблю скло, світло й котів 🐱".into(),
                6 => "Погода для будь-якого міста: /weather Київ".into(),
                _ => String::new(),
            };
            d.username = match id {
                1 => "olena_koval".into(),
                6 => "weather_demo_bot".into(),
                _ => String::new(),
            };
            if id == 1 {
                d.phone = "+380 50 000 00 01".into();
                d.birthday = Some(today_birthday(Some(1996)));
            }
            if id == 5 {
                d.birthday = Some(Birthday { day: 3, month: 9, year: None });
            }
            d.common = 2;
        }
        Some(ChatKind::Group) => {
            d.about = "Обговорюємо Rust, egui і grammers. Без спаму 🦀".into();
            d.members = Some(demo_members().len() as i32);
            d.online = Some(2);
            d.admins = Some(2);
            d.admin = id == 2;
            // We run "Команда Rust": its edit box shows permissions and slow mode.
            d.can_edit = id == 2;
            let mut rights = ChatRights::from_banned(None);
            rights.change_info = false;
            d.rights = Some(rights);
            d.slowmode = Some(if id == 2 { 30 } else { 0 });
        }
        Some(ChatKind::Channel) => {
            d.about = "Головні новини технологій щодня.".into();
            d.username = "tech_news_demo".into();
            d.members = Some(12840);
            d.can_view_members = false;
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
        (1, "Олена Коваль", MemberRole::Owner, Presence::Online(i64::MAX)),
        (5, "Андрій", MemberRole::Admin, Presence::Offline(jiff::Timestamp::now().as_second() - 3600)),
        (8, "Марія Шевченко", MemberRole::Member, Presence::Recently),
        (101, "Тарас Бондар", MemberRole::Member, Presence::LastWeek),
        (102, "Ірина Мороз", MemberRole::Member, Presence::Online(i64::MAX)),
        (103, "Софія Лисенко", MemberRole::Member, Presence::Recently),
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
                unread_mentions: 0,
                unread_reactions: 0,
                noforwards: false,
                can_pin: false,
            },
            role,
            rank: if id == 5 { "модератор".into() } else { String::new() },
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
