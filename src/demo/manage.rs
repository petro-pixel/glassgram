//! Demo chat administration: the admin log, discussion groups, editing a chat, members and invite links.

use super::*;

pub(super) fn handle(app: &mut App, cmd: Cmd) {
    match cmd {
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
        Cmd::LoadDiscussionGroups(peer) => {
            let groups = app.demo_chats().iter().filter(|c| c.kind == ChatKind::Group).cloned().collect();
            app.push_event(Ev::DiscussionGroups { peer, groups });
        }
        Cmd::EditChat { peer, title, .. } => {
            if let Some(title) = title {
                app.push_event(Ev::ChatTitle { peer, title });
            }
            app.push_event(Ev::Notice(crate::i18n::l!("Зміни збережено", "Changes saved").into()));
        }
        Cmd::SetChatPhoto { .. } | Cmd::SetChatRights { .. } | Cmd::SetSlowMode { .. } | Cmd::SetChatReactions { .. } | Cmd::SetMembersHidden { .. } | Cmd::SetAntiSpam { .. } => app.push_event(Ev::Notice(crate::i18n::l!("Зміни збережено", "Changes saved").into())),
        _ => {}
    }
}

/// Admins (with rights and titles), restrictions, removed people, join requests and our links.
pub(super) struct DemoManage {
    pub(super) admins: Vec<(i64, AdminRights, String)>,
    pub(super) restricted: Vec<(i64, ChatRights, i32)>,
    pub(super) removed: Vec<i64>,
    pub(super) requests: Vec<(i64, &'static str)>,
    pub(super) links: Vec<InviteLink>,
}

pub(super) fn demo_manage(m: &mut Option<DemoManage>) -> &mut DemoManage {
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
pub(super) fn demo_person(id: i64) -> ChatInfo {
    let name = match id {
        104 => "Bohdan Tkach",
        105 => "Nataliia Humeniuk",
        106 => "Viktor Savchuk",
        _ => return demo_members().into_iter().find(|m| m.chat.id == PeerId::user_unchecked(id)).map(|m| m.chat).unwrap_or_else(|| person(id, "?", "", Presence::None)),
    };
    person(id, name, "", Presence::Recently)
}

pub(super) fn participants(app: &mut App, cmd: Cmd) {
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
                    // Olena owns the group; the news channel is ours, or Andriy's after the transfer.
                    PartList::Admins if boosts::owned(peer) => m.admins.iter().map(|a| a.0).map(admin).collect::<Vec<_>>(),
                    PartList::Admins if boosts::transferred(peer) => vec![Participant { owner: true, admin: Some(AdminRights::all()), can_edit: false, ..part(5) }],
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

pub(super) fn invites(app: &mut App, cmd: Cmd) {
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

pub(super) fn demo_link(d: InviteDraft) -> InviteLink {
    InviteLink { title: d.title, usage_limit: d.usage_limit, expire: d.expire, request_needed: d.request_needed, ..Default::default() }
}
