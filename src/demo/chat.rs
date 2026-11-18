//! The open chat in the demo: comments, topics, media, search, pins, sponsored posts, translation, chat info and the history itself.

use super::*;

pub(super) fn handle(app: &mut App, cmd: Cmd) {
    match cmd {
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
                hits = search::search_media(app, k, &q);
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
        cmd => around(app, cmd),
    }
}

/// Around the chat: sponsored posts, translation, dates, chat info, reports and similar channels.
fn around(app: &mut App, cmd: Cmd) {
    match cmd {
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
        Cmd::LoadMediaDays { peer, before } => {
            // A few days with photos in the month before `before`.
            let days = [9, 10, 17, 24, 30].into_iter().map(|back| (before - back * 86_400 - 3 * 3600, (back % 4 + 1) as i32)).collect();
            app.push_event(Ev::MediaDays { peer, days });
        }
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
        Cmd::LeaveChat(peer) | Cmd::DeleteChat { peer, .. } => app.push_event(Ev::ChatRemoved(peer)),
        Cmd::ClearHistory { peer, .. } => app.push_event(Ev::HistoryCleared(peer)),
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
