//! Sending in the demo: scheduled and inline messages, the helper bot's commands, forwards, dice, polls, checklists, the offline queue and media edits.

use super::*;

pub(super) fn handle(app: &mut App, cmd: Cmd) {
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
                iv: false,
                card: CardKind::Link,
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
        Cmd::Forward { from, ids, to, send_as } => {
            let src = app.demo_history().find(|(p, _, _)| *p == from).map(|(_, (title, _), msgs)| (title.to_owned(), msgs.clone()));
            for m in src.iter().flat_map(|(title, msgs)| msgs.iter().filter(|m| ids.contains(&m.id)).map(move |m| (title, m))) {
                let from_name = if m.1.out { "Demo User".to_owned() } else if m.1.sender.is_empty() { m.0.clone() } else { m.1.sender.clone() };
                let mut copy = outgoing(to, String::new(), None);
                copy = Msg { text: m.1.text.clone(), spans: m.1.spans.clone(), photo: m.1.photo, file: m.1.file.clone(), forwarded: Some(from_name), id: copy.id + m.1.id, ..copy };
                app.push_event(Ev::NewMsg(sendas::posted_as(copy, send_as)));
            }
        }
        Cmd::PlayGame { .. } => app.push_event(Ev::BotAnswer { text: l!("Демо: гра відкрилася б у браузері", "Demo: the game would open in the browser").into(), alert: false }),
        Cmd::PressButton { data, .. } => {
            let text = match data.as_slice() {
                b"tomorrow" => "Tomorrow in Kyiv: +21°, partly cloudy",
                b"stop" => "You will no longer get the daily forecast",
                _ => "Updated: +18°, clear",
            };
            app.push_event(Ev::BotAnswer { text: text.into(), alert: false });
        }
        Cmd::SendDice { peer, emoji, reply_to, send_as } => {
            let top = match emoji.as_str() {
                "🎰" => 64,
                "🏀" | "⚽" => 5,
                _ => 6,
            };
            let value = rand_ish() % top + 1;
            app.push_event(Ev::NewMsg(Msg { dice: Some((emoji, value)), ..sendas::posted_as(replied(app, outgoing(peer, String::new(), None), &reply_to), send_as) }));
        }
        cmd => queued(app, cmd),
    }
}

/// The offline queue, polls, checklists and media edits.
fn queued(app: &mut App, cmd: Cmd) {
    match cmd {
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
            app.push_event(Ev::NewMsg(Msg { topic: topic.filter(|t| *t != 1), effect: q.effect, ..sendas::posted_as(replied(app, outgoing(q.peer, q.text.clone(), None), &reply), q.send_as) }));
            app.push_event(Ev::QueueSent { key: q.key, peer: q.peer });
            // A command gets the group bot's answer that only we see.
            if q.peer == PeerId::user_unchecked(ephemeral::GROUP) && q.text.starts_with('/') {
                ephemeral::answer(app, q.peer, &q.text);
            // The team answers with a mention: it counts on the "@" button unless the chat is in view.
            } else if q.peer == PeerId::user_unchecked(2) {
                let m = Msg { out: false, sender: "Taras".into(), mentioned: true, id: 20_000 + rand_ish(), ..outgoing(q.peer, "@demo got it, thanks!".into(), None) };
                app.push_event(Ev::NewMsg(m));
            }
        }
        // The UI already shows the new order; there is no server to tell.
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
        _ => {}
    }
}
