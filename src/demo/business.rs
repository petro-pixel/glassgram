//! Demo Telegram Business: our own settings with a few quick replies, links and a bot, all
//! editable for the run; and "Glass Coffee", a business contact with an address and opening hours.

use std::cell::RefCell;

use grammers_client::session::types::PeerId;

use crate::app::App;
use crate::backend::{
    Away, AwaySchedule, BusinessCmd, BusinessEv, BusinessInfo, ChatInfo, ChatKind, ChatLink, ConnectedBot, Ev, Greeting, Msg, MyBusiness, Presence, QuickMsg, QuickReply, Recipients, Timezone, WorkHours,
};
use crate::format;
use crate::i18n::l;

/// The coffee shop's user id.
pub(super) const SHOP: i64 = 60;
const DAY: i32 = 24 * 60;

/// (id, shortcut, messages)
type Shortcut = (i32, String, Vec<QuickMsg>);

struct State {
    my: MyBusiness,
    quick: Vec<Shortcut>,
    links: Vec<ChatLink>,
    bots: Vec<ConnectedBot>,
    next_id: i32,
}

thread_local! {
    static STATE: RefCell<Option<State>> = const { RefCell::new(None) };
}

fn msg(id: i32, text: &str) -> QuickMsg {
    QuickMsg { id, text: text.into(), media: false }
}

/// Weekdays from `open` to `close` (minutes), and Saturday if given.
fn week(open: i32, close: i32, saturday: Option<(i32, i32)>) -> Vec<(i32, i32)> {
    let mut list: Vec<(i32, i32)> = (0..5).map(|d| (d * DAY + open, d * DAY + close)).collect();
    if let Some((s, e)) = saturday {
        list.push((5 * DAY + s, 5 * DAY + e));
    }
    list
}

fn initial() -> State {
    let zone = |id: &str, name: &str, offset: i32| Timezone { id: id.into(), name: name.into(), offset };
    let timezones = vec![
        zone("Europe/London", "London (UTC+00:00)", 0),
        zone("Europe/Berlin", "Berlin (UTC+01:00)", 3600),
        zone("Europe/Warsaw", "Warsaw (UTC+01:00)", 3600),
        zone("Europe/Kyiv", "Kyiv (UTC+02:00)", 7200),
        zone("America/New_York", "New York (UTC−05:00)", -18_000),
        zone("Asia/Tokyo", "Tokyo (UTC+09:00)", 32_400),
    ];
    let all = Recipients { exclude_selected: true, ..Default::default() };
    let my = MyBusiness {
        address: "вул. Скляна, 1, Київ".into(),
        hours: Some(WorkHours { tz: "Europe/Kyiv".into(), open_now: true, weekly: week(9 * 60, 18 * 60, None) }),
        greeting: Some(Greeting { shortcut: 1, recipients: Recipients { contacts: true, ..all.clone() }, days: 14 }),
        away: Some(Away { shortcut: 2, offline_only: false, schedule: AwaySchedule::OutsideHours, recipients: all }),
        timezones,
    };
    let quick = vec![
        (1, "hello".to_owned(), vec![msg(101, "Hi! Thanks for writing to **Glass Studio** 👋"), msg(102, "We usually answer within an hour.")]),
        (2, "away".to_owned(), vec![msg(201, "We're closed right now — we'll get back to you in the morning ☕")]),
        (3, "prices".to_owned(), vec![msg(301, "**Prices:** a logo from 200 €, a website from 900 €."), msg(302, "The full list: https://example.org/prices")]),
        (4, "thanks".to_owned(), vec![msg(401, "Thank you for your order! 🙏")]),
    ];
    let links = vec![
        ChatLink { link: "https://t.me/m/GlassDemo01".into(), title: "Instagram".into(), message: "Hi! I'd like to order a design".into(), views: 42 },
        ChatLink { link: "https://t.me/m/GlassDemo02".into(), title: String::new(), message: "Hello, a question about prices".into(), views: 7 },
    ];
    let bots = vec![ConnectedBot { id: PeerId::user_unchecked(61), name: "Support Assistant".into(), username: "glass_support_bot".into(), can_reply: true }];
    State { my, quick, links, bots, next_id: 1000 }
}

fn quick_list(s: &State) -> Vec<QuickReply> {
    s.quick.iter().map(|(id, name, msgs)| QuickReply { id: *id, name: name.clone(), top: msgs.first().map(|m| m.text.clone()).unwrap_or_default(), count: msgs.len() as i32 }).collect()
}

/// Events to send after a change, worked out while the state is borrowed.
fn apply(s: &mut State, cmd: BusinessCmd) -> Vec<Ev> {
    let b = |e| Ev::Business(e);
    let quick = |s: &State| b(BusinessEv::QuickReplies(quick_list(s)));
    let msgs = |s: &State, id: i32| b(BusinessEv::QuickMessages { shortcut: id, msgs: s.quick.iter().find(|q| q.0 == id).map(|q| q.2.clone()).unwrap_or_default() });
    match cmd {
        BusinessCmd::Load => vec![b(BusinessEv::Loaded(s.my.clone()))],
        BusinessCmd::SetLocation(a) => {
            s.my.address = a.trim().to_owned();
            vec![b(BusinessEv::Loaded(s.my.clone())), Ev::Notice(l!("Збережено", "Saved").into())]
        }
        BusinessCmd::SetHours(h) => {
            s.my.hours = h;
            vec![b(BusinessEv::Loaded(s.my.clone())), Ev::Notice(l!("Збережено", "Saved").into())]
        }
        BusinessCmd::SetGreeting(g) => {
            s.my.greeting = g;
            vec![b(BusinessEv::Loaded(s.my.clone())), Ev::Notice(l!("Збережено", "Saved").into())]
        }
        BusinessCmd::SetAway(a) => {
            s.my.away = a;
            vec![b(BusinessEv::Loaded(s.my.clone())), Ev::Notice(l!("Збережено", "Saved").into())]
        }
        BusinessCmd::LoadQuickReplies => vec![quick(s)],
        BusinessCmd::LoadQuickMessages(id) => vec![msgs(s, id)],
        BusinessCmd::AddQuickMessage { shortcut, text } => {
            s.next_id += 1;
            let mid = s.next_id;
            let id = match s.quick.iter_mut().find(|q| q.1 == shortcut) {
                Some(q) => {
                    q.2.push(msg(mid, &text));
                    q.0
                }
                None => {
                    s.next_id += 1;
                    let id = s.next_id;
                    s.quick.push((id, shortcut, vec![msg(mid, &text)]));
                    id
                }
            };
            vec![quick(s), msgs(s, id)]
        }
        BusinessCmd::EditQuickMessage { shortcut, id, text } => {
            if let Some(m) = s.quick.iter_mut().filter(|q| q.0 == shortcut).flat_map(|q| q.2.iter_mut()).find(|m| m.id == id) {
                m.text = text;
            }
            vec![quick(s), msgs(s, shortcut)]
        }
        BusinessCmd::DeleteQuickMessage { shortcut, id } => {
            if let Some(q) = s.quick.iter_mut().find(|q| q.0 == shortcut) {
                q.2.retain(|m| m.id != id);
            }
            // Like the server: a shortcut without messages is gone.
            s.quick.retain(|q| !q.2.is_empty());
            vec![quick(s), msgs(s, shortcut)]
        }
        BusinessCmd::RenameQuickReply { shortcut, name } => {
            if let Some(q) = s.quick.iter_mut().find(|q| q.0 == shortcut) {
                q.1 = name;
            }
            vec![quick(s)]
        }
        BusinessCmd::DeleteQuickReply(id) => {
            s.quick.retain(|q| q.0 != id);
            vec![quick(s)]
        }
        BusinessCmd::SendQuickReply { peer, shortcut } => {
            let texts: Vec<String> = s.quick.iter().find(|q| q.0 == shortcut).map(|q| q.2.iter().map(|m| m.text.clone()).collect()).unwrap_or_default();
            texts.into_iter().enumerate().map(|(k, t)| Ev::NewMsg(Msg { id: 30_000 + super::rand_ish() + k as i32, ..super::outgoing(peer, t, None) })).collect()
        }
        BusinessCmd::LoadLinks => vec![b(BusinessEv::Links(s.links.clone()))],
        BusinessCmd::SaveLink { slug, title, message } => {
            match slug.and_then(|slug| s.links.iter_mut().find(|l| l.slug() == slug)) {
                Some(l) => {
                    l.title = title;
                    l.message = message;
                }
                None => {
                    s.next_id += 1;
                    s.links.push(ChatLink { link: format!("https://t.me/m/GlassDemo{}", s.next_id), title, message, views: 0 });
                }
            }
            vec![b(BusinessEv::Links(s.links.clone()))]
        }
        BusinessCmd::DeleteLink(slug) => {
            s.links.retain(|l| l.slug() != slug);
            vec![b(BusinessEv::Links(s.links.clone()))]
        }
        BusinessCmd::LoadBots => vec![b(BusinessEv::Bots(s.bots.clone()))],
        BusinessCmd::ConnectBot(name) => {
            s.next_id += 1;
            let title = name.trim_end_matches("_bot").trim_end_matches("bot").replace('_', " ");
            s.bots.push(ConnectedBot { id: PeerId::user_unchecked(s.next_id as i64), name: title.trim().to_owned(), username: name, can_reply: true });
            vec![b(BusinessEv::Bots(s.bots.clone()))]
        }
        BusinessCmd::DisconnectBot(id) => {
            s.bots.retain(|x| x.id != id);
            vec![b(BusinessEv::Bots(s.bots.clone()))]
        }
    }
}

pub fn handle(app: &mut App, cmd: BusinessCmd) {
    let events = STATE.with_borrow_mut(|s| apply(s.get_or_insert_with(initial), cmd));
    for ev in events {
        app.push_event(ev);
    }
}

/// The coffee shop's address and hours: weekdays 8–20, Saturday 10–16, in Kyiv time.
pub(super) fn shop_info() -> BusinessInfo {
    let hours = WorkHours { tz: "Europe/Kyiv".into(), open_now: true, weekly: week(8 * 60, 20 * 60, Some((10 * 60, 16 * 60))) };
    let offset = jiff::tz::TimeZone::get("Europe/Kyiv").map(|z| z.to_offset(jiff::Timestamp::now()).seconds()).unwrap_or(7200);
    BusinessInfo { address: "вул. Хрещатик, 22, Київ".into(), hours: Some(hours), offset: Some(offset) }
}

/// The coffee shop's chat: its greeting came as a business auto-reply.
pub(super) fn chat() -> (ChatInfo, Vec<Msg>) {
    let peer = PeerId::user_unchecked(SHOP);
    let script = [
        (true, "Hi! Do you have oat milk?"),
        (false, "Hi! Thanks for writing to **Glass Coffee** ☕ We'll answer in a few minutes."),
        (false, "Yes, oat and almond. Come by — we're open till 20:00 today."),
    ];
    let msgs: Vec<Msg> = script
        .iter()
        .enumerate()
        .map(|(i, &(out, text))| {
            let (text, ents) = format::parse(text);
            Msg {
                id: i as i32 + 1,
                out,
                sender: if out { String::new() } else { "Glass Coffee".into() },
                spans: format::from_entities(&text, &ents, 0),
                text,
                time: format!("10:{:02}", 12 + i * 3),
                day: l!("Сьогодні", "Today").into(),
                ..Msg::placeholder(peer)
            }
        })
        .collect();
    let chat = ChatInfo {
        id: peer,
        title: "Glass Coffee".into(),
        kind: ChatKind::User,
        last_text: "Yes, oat and almond. Come by — we're open till 20:00 today.".into(),
        last_time: "10:18".into(),
        last_ts: 95,
        unread: 1,
        presence: Presence::Recently,
        ..super::person(SHOP, "Glass Coffee", "", Presence::Recently)
    };
    (chat, msgs)
}
