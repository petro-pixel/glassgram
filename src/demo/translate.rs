//! Demo whole-chat translation: a German group, "Rust Meetup Berlin", whose full info offers the
//! translate bar; its messages come back with canned Ukrainian and English translations.

use grammers_client::session::types::PeerId;

use crate::app::App;
use crate::backend::{ChatInfo, ChatKind, Cmd, Ev, Msg, Presence};
use crate::format;
use crate::i18n::l;

/// The German group.
const GROUP: i64 = 50;

/// (sender, German text, Ukrainian, English); an empty sender is us.
const SCRIPT: [(&str, &str, &str, &str); 6] = [
    ("Lukas", "Hallo zusammen! Wer kommt heute Abend zum Treffen?", "Привіт усім! Хто прийде сьогодні ввечері на зустріч?", "Hi everyone! Who's coming to the meetup tonight?"),
    ("Anna", "Ich bin dabei, ich bringe die Folien über async Rust mit.", "Я буду, принесу слайди про async Rust.", "I'm in, I'll bring the slides about async Rust."),
    ("", "I'll join online 🙂", "", ""),
    ("Jonas", "Das ist super! Der Raum ist im dritten Stock, die Tür ist offen.", "Чудово! Зала на третьому поверсі, двері відчинені.", "Great! The room is on the third floor, the door is open."),
    ("Anna", "Und wir haben noch Pizza für alle 🍕", "А ще в нас є піца для всіх 🍕", "And we've got pizza for everyone 🍕"),
    ("Lukas", "Bis später, und vergesst nicht die Laptops!", "До зустрічі, і не забудьте ноутбуки!", "See you later, and don't forget your laptops!"),
];

pub(super) fn chat() -> (ChatInfo, Vec<Msg>) {
    let peer = PeerId::channel_unchecked(GROUP);
    let msgs: Vec<Msg> = SCRIPT
        .iter()
        .enumerate()
        .map(|(i, &(sender, text, ..))| {
            let (text, ents) = format::parse(text);
            Msg {
                id: i as i32 + 1,
                out: sender.is_empty(),
                sender: sender.into(),
                sender_color: super::demo_color(sender),
                spans: format::from_entities(&text, &ents, 0),
                text,
                time: format!("18:{:02}", 5 + i * 4),
                day: l!("Сьогодні", "Today").into(),
                ..Msg::placeholder(peer)
            }
        })
        .collect();
    let chat = ChatInfo {
        id: peer,
        title: "Rust Meetup Berlin".into(),
        kind: ChatKind::Group,
        last_text: "Lukas: Bis später, und vergesst nicht die Laptops!".into(),
        last_time: "18:25".into(),
        last_ts: 96,
        unread: 2,
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
        read_in: 4,
        unread_mentions: 0,
        unread_reactions: 0,
        noforwards: false,
        can_pin: false,
        restricted: None,
        emoji_status: None,
    };
    (chat, msgs)
}

pub(super) fn handle(app: &mut App, cmd: Cmd) {
    let group = PeerId::channel_unchecked(GROUP);
    match cmd {
        // Only the German group offers the bar: the other demo chats are in English, like the interface.
        Cmd::LoadTranslationOffer(peer) => app.push_event(Ev::TranslationOffer { peer, offered: peer == group }),
        Cmd::TranslateMany { peer, ids, to } => {
            let list = ids
                .iter()
                .filter_map(|&id| {
                    let &(_, de, uk, en) = SCRIPT.get(usize::try_from(id - 1).ok()?).filter(|_| peer == group)?;
                    let text = match to.as_str() {
                        "uk" => uk.to_owned(),
                        "en" => en.to_owned(),
                        "de" => de.to_owned(),
                        other => format!("[{other}] {en}"),
                    };
                    Some((id, text, Vec::new()))
                })
                .collect();
            app.push_event(Ev::TranslatedMany { peer, ids, to, result: Ok(list) });
        }
        // Remembered by the app; there is no server to tell.
        _ => {}
    }
}
