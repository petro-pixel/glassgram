//! Demo chat info: details, members, shared media and birthdays.

use super::*;

/// A birthday falling on today, so the reminder always shows in the demo.
pub(super) fn today_birthday(year: Option<u16>) -> Birthday {
    let d = jiff::Zoned::now().date();
    Birthday { day: d.day() as u8, month: d.month() as u8, year }
}

/// Demo profile: bio, username and member counts by chat kind.
pub(super) fn demo_details(peer: PeerId, kind: Option<ChatKind>) -> ChatDetails {
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
            if id == business::SHOP {
                d.business = Some(business::shop_info());
                d.about = "Specialty coffee and pastries. Order ahead in this chat ☕".into();
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
                d.stats = Some(Some(2));
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
            d.stats = Some(Some(2));
            d.linked = Some((PeerId::user_unchecked(2), "Rust Team 🦀".into()));
            d.my_rights = Some(AdminRights::all());
            // We own it until it is transferred to Andriy (`demo/boosts.rs`).
            d.creator = boosts::owned(peer);
            if boosts::transferred(peer) {
                d.my_rights = Some(AdminRights { add_admins: false, ..AdminRights::all() });
            }
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
pub(super) fn demo_members() -> Vec<Member> {
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
                last_spans: Vec::new(),
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
                paid_stars: None,
            },
            role,
            rank: if id == 5 { "moderator".into() } else { String::new() },
        })
        .collect()
}

/// Shared media from the demo history, newest first, filtered like `InputMessagesFilter*`.
pub(super) fn demo_shared(app: &App, peer: PeerId, kind: SharedKind) -> Vec<Msg> {
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
