//! Демо-режим (`--demo`): вигадані чати без мережі, щоб подивитися на інтерфейс.

use std::collections::HashMap;

use grammers_client::session::types::PeerId;

use crate::app::App;
use eframe::egui::{Color32, ColorImage};

use crate::backend::{Topic, Button, ButtonAction, GeoInfo, InlineResult, MediaInfo, MediaKind, PollInfo, PollOption, Reaction, ChatInfo, ChatKind, Cmd, Ev, Folder, ImgKey, Msg, Presence, SearchHit, StickerSetInfo};
use crate::format;

pub fn data() -> (String, Vec<ChatInfo>, HashMap<PeerId, Vec<Msg>>) {
    let chats = [
        (1, "Олена Коваль", ChatKind::User, 2, true),
        (2, "Команда Rust 🦀", ChatKind::Group, 14, false),
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
        &[(false, "Марія", "Дякую за допомогу!")],
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
        });
        let mut msgs = msgs;
        extras(id, peer, &mut msgs);
        history.insert(peer, msgs);
    }
    let (forum, msgs) = forum_chat();
    history.insert(forum.id, msgs);
    infos.insert(3, forum);
    ("Демо-користувач".into(), infos, history)
}

pub fn folders() -> Vec<Folder> {
    vec![
        Folder { id: 1, title: "Особисті".into(), emoticon: "👤".into(), contacts: true, non_contacts: true, ..Default::default() },
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
        Cmd::Send { peer, text, reply_to, .. } => {
            // У форумі відповідь кладе повідомлення в тему того, на що відповідаємо (або в саму тему).
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
            // Файл лишається на диску — його й програватимемо.
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
            // Обговорення каналу «Новини технологій» — окрема вигадана група.
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
            }
        }
        Cmd::SetPrivacy(key, audience) => app.push_event(Ev::Privacy(key, crate::backend::PrivacyState { audience, exceptions: false })),
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
        Cmd::Search { peer, query } => {
            let q = query.to_lowercase();
            let hits = app
                .demo_history()
                .filter(|(p, _, _)| peer.is_none_or(|x| x == *p))
                .flat_map(|(_, chat, msgs)| msgs.iter().map(move |m| (chat, m)))
                .filter(|(_, m)| m.text.to_lowercase().contains(&q))
                .map(|((title, kind), m)| SearchHit { chat: title.to_owned(), kind, msg: m.clone() })
                .collect();
            app.push_event(Ev::SearchResults { peer, query, hits });
        }
        Cmd::Typing(peer) if peer == PeerId::user_unchecked(1) => {
            // Олена «відповідає» на набір тексту.
            app.push_event(Ev::Typing { peer, who: "Олена".into(), active: true });
        }
        _ => {}
    }
}

const FORUM: i64 = 10;

/// Теми демо-форуму: (id, назва, колір, непрочитані, закріплена, закрита).
const TOPICS: [(i32, &str, u32, i32, bool, bool); 4] = [
    (1, "Загальне", 0, 0, false, false),
    (100, "Допомога новачкам", 0x6FB9F0, 3, true, false),
    (200, "Реліз 1.0", 0xFFD67E, 1, false, false),
    (300, "Архів обговорень", 0xCB86DB, 0, false, true),
];

/// Група-форум «Rust Україна» з кількома темами.
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

/// Додаткові демо-повідомлення: опитування, місце, контакт, альбом, реакції.
fn extras(id: i64, peer: PeerId, msgs: &mut Vec<Msg>) {
    let base = |n: i32, sender: &str| Msg { sender: sender.into(), out: sender.is_empty(), time: format!("12:{n:02}"), ..Msg::placeholder(peer) };
    match id {
        2 => {
            if let Some(m) = msgs.first_mut() {
                m.reactions = vec![Reaction { emoji: "👍".into(), count: 3, mine: false }, Reaction { emoji: "🔥".into(), count: 1, mine: true }];
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
        _ => {}
    }
}

fn weather_buttons() -> Vec<Vec<Button>> {
    let b = |text: &str, action| Button { text: text.into(), action };
    vec![
        vec![b("Оновити", ButtonAction::Callback(b"now".to_vec())), b("Завтра", ButtonAction::Callback(b"tomorrow".to_vec()))],
        vec![b("Мапа погоди", ButtonAction::Url("https://www.windy.com".into()))],
    ]
}

/// Демо-звук для голосових і аудіо: акорд, що пульсує, у WAV (16 біт, моно).
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

/// Демо-відео з тестових джерел ffmpeg: відео зі звуком, GIF без звуку, кружечок.
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
    let mut cmd = std::process::Command::new("ffmpeg");
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

/// Згенеровані «фото»: градієнтний захід сонця та аватарка.
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
    ]
}
