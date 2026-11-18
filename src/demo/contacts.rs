//! Demo contacts, blocking, birthdays and new groups and channels.

use super::*;

pub(super) fn handle(app: &mut App, cmd: Cmd) {
    match cmd {
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
        _ => {}
    }
}
