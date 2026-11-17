//! Demo boosts and ownership transfer: "Tech News" is a level 2 channel we own, all three of our
//! Premium slots boost other chats (one still cooling down), so "Boost" goes through the reassign
//! list; its admin Andriy can be made the owner after the password box (any password works).

use std::cell::RefCell;

use grammers_client::session::types::PeerId;

use crate::app::App;
use crate::backend::{BoostSlot, BoostStatus, Booster, Cmd, Ev, Presence, TransferStep};
use crate::i18n::lf;

/// The demo channel with boosts.
const NEWS: i64 = 4;

struct State {
    slots: Vec<BoostSlot>,
    /// Chats whose ownership went to someone else.
    transferred: Vec<PeerId>,
}

thread_local! {
    static STATE: RefCell<Option<State>> = const { RefCell::new(None) };
}

fn with<R>(f: impl FnOnce(&mut State) -> R) -> R {
    STATE.with_borrow_mut(|s| {
        let s = s.get_or_insert_with(|| {
            let now = jiff::Timestamp::now().as_second();
            let slot = |slot, id: i64, name: &str, cooldown| BoostSlot { slot, peer: Some((PeerId::user_unchecked(id), name.into())), expires: now + 24 * 86400, cooldown_until: cooldown };
            State {
                slots: vec![slot(1, 71, "Olena on Design", 0), slot(2, 2, "Rust Team 🦀", 0), slot(3, 7, "Design Studio", now + 5 * 3600 + 12 * 60)],
                transferred: Vec::new(),
            }
        });
        f(s)
    })
}

/// We gave this chat away in the demo: no longer its owner.
pub fn transferred(peer: PeerId) -> bool {
    with(|s| s.transferred.contains(&peer))
}

/// We own the news channel until it is transferred.
pub fn owned(peer: PeerId) -> bool {
    peer.bare_id_unchecked() == NEWS && !transferred(peer)
}

fn status(peer: PeerId) -> BoostStatus {
    let mine = with(|s| s.slots.iter().filter(|x| x.peer.as_ref().is_some_and(|p| p.0 == peer)).count() as i32);
    let (level, level_boosts, base, next) = if peer.bare_id_unchecked() == NEWS { (2, 10, 14, Some(20)) } else { (0, 0, 0, Some(1)) };
    BoostStatus { level, level_boosts, boosts: base + mine, next_level_boosts: next, boosted: mine > 0, url: format!("https://t.me/boost/{}", if peer.bare_id_unchecked() == NEWS { "tech_news_demo" } else { "demo" }), gift_boosts: 3 }
}

fn boosters() -> Vec<Booster> {
    let now = jiff::Timestamp::now().as_second();
    let user = |id, name: &str| Some(super::person(id, name, "", Presence::Recently));
    let b = |user, multiplier, days: i64, gift, giveaway| Booster { user, multiplier, expires: now + days * 86400, gift, giveaway };
    vec![
        b(user(1, "Olena Koval"), 4, 27, false, false),
        b(user(8, "Maria Shevchenko"), 2, 19, false, false),
        b(user(5, "Andriy"), 1, 12, false, false),
        b(user(101, "Taras Bondar"), 1, 8, true, false),
        b(user(102, "Iryna Moroz"), 1, 30, false, true),
        b(user(103, "Sofia Lysenko"), 2, 4, false, false),
        b(None, 3, 60, false, true),
    ]
}

pub fn handle(app: &mut App, cmd: Cmd) {
    match cmd {
        Cmd::LoadBoosts(peer) => {
            app.push_event(Ev::BoostStatus { peer, status: status(peer) });
            app.push_event(Ev::MyBoosts(with(|s| s.slots.clone())));
        }
        Cmd::ApplyBoost { peer, slots } => {
            let title = app.demo_chats().iter().find(|c| c.id == peer).map(|c| c.title.clone()).unwrap_or_default();
            let all = with(|s| {
                for x in s.slots.iter_mut().filter(|x| slots.contains(&x.slot)) {
                    x.peer = Some((peer, title.clone()));
                    x.cooldown_until = jiff::Timestamp::now().as_second() + 86400;
                }
                s.slots.clone()
            });
            app.push_event(Ev::MyBoosts(all));
            app.push_event(Ev::Boosted(peer));
            app.push_event(Ev::BoostStatus { peer, status: status(peer) });
        }
        Cmd::LoadBoosters { peer, .. } => {
            let list = if peer.bare_id_unchecked() == NEWS { boosters() } else { Vec::new() };
            let count = list.iter().map(|b| b.multiplier).sum();
            app.push_event(Ev::Boosters { peer, count, list, next: String::new(), append: false });
        }
        Cmd::OpenBoostLink(name) => match app.demo_chats().iter().find(|c| name == "tech_news_demo" && c.id == PeerId::user_unchecked(NEWS)).cloned() {
            Some(chat) => app.push_event(Ev::BoostLink(chat)),
            None => app.push_event(Ev::Error(lf!("@{name} не знайдено", "@{name} not found"))),
        },
        Cmd::TransferOwnership { peer, user, password } => {
            let step = match password {
                None => TransferStep::AskPassword,
                Some(p) if p.trim().is_empty() => TransferStep::Failed(crate::i18n::l!("Невірний пароль", "Wrong password").to_owned()),
                Some(_) => {
                    with(|s| s.transferred.push(peer));
                    TransferStep::Done
                }
            };
            let done = step == TransferStep::Done;
            app.push_event(Ev::Transfer { peer, user, step });
            if done {
                app.push_event(Ev::ParticipantsChanged(peer));
            }
        }
        _ => {}
    }
}
