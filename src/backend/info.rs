//! Chat info: profile, members, shared media, notifications, leaving and deleting.

use grammers_client::Client;
use grammers_client::peer::Peer;
use grammers_client::session::types::{PeerKind, PeerRef};
use grammers_client::tl;

use super::{Birthday, ChatInfo, ChatRights, Cmd, Ev, Worker, clock, convert, found_chat, local, peer_title, remember_name};
use crate::i18n::{l, lf};

/// How many members we show at once; beyond that, search.
const MEMBERS_LIMIT: usize = 200;
/// Shared media page size.
pub const SHARED_PAGE: usize = 60;
/// Mute with no end date, as Telegram does it.
pub const MUTE_FOREVER: i32 = i32::MAX;

/// Shared media tabs, as in the Telegram Desktop profile.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum SharedKind {
    Media,
    Files,
    Links,
    Voice,
    Gifs,
}

impl SharedKind {
    pub const ALL: [Self; 5] = [Self::Media, Self::Files, Self::Links, Self::Voice, Self::Gifs];

    pub fn title(self) -> &'static str {
        match self {
            Self::Media => l!("Медіа", "Media"),
            Self::Files => l!("Файли", "Files"),
            Self::Links => l!("Посилання", "Links"),
            Self::Voice => l!("Голосові", "Voice"),
            Self::Gifs => "GIF",
        }
    }

    pub(super) fn filter(self) -> tl::enums::MessagesFilter {
        use tl::enums::MessagesFilter as F;
        match self {
            Self::Media => F::InputMessagesFilterPhotoVideo,
            Self::Files => F::InputMessagesFilterDocument,
            Self::Links => F::InputMessagesFilterUrl,
            // Telegram Desktop lists round videos together with voice messages.
            Self::Voice => F::InputMessagesFilterRoundVoice,
            Self::Gifs => F::InputMessagesFilterGif,
        }
    }
}

/// Notification settings of one chat.
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct NotifyState {
    /// Unix time muted until (0 = on, `MUTE_FOREVER` = forever).
    pub mute_until: i32,
    pub silent: bool,
    /// The chat has no mute setting of its own and follows its category.
    pub inherit: bool,
}

impl NotifyState {
    pub fn muted(&self) -> bool {
        self.mute_until as i64 > jiff::Timestamp::now().as_second()
    }

    /// Caption: "On", "Muted until 14:30", "Off".
    pub fn label(&self) -> String {
        if !self.muted() {
            return if self.silent { l!("Увімкнено, без звуку", "On, silent").to_owned() } else { l!("Увімкнено", "On").to_owned() };
        }
        // Telegram stores "forever" as a date years ahead, so we just say "Off".
        let Ok(t) = jiff::Timestamp::from_second(self.mute_until as i64) else { return l!("Вимкнено", "Off").to_owned() };
        if self.mute_until as i64 - jiff::Timestamp::now().as_second() > 365 * 86400 {
            return l!("Вимкнено", "Off").to_owned();
        }
        let (z, now) = (local(t), jiff::Zoned::now());
        if z.date() == now.date() {
            lf!("Вимкнено до {}", "Muted until {}", clock(t))
        } else {
            lf!("Вимкнено до {} {}", "Muted until {} {}", z.strftime("%d.%m"), clock(t))
        }
    }

    pub(super) fn from_raw(raw: &tl::enums::PeerNotifySettings) -> Self {
        let tl::enums::PeerNotifySettings::Settings(n) = raw;
        Self { mute_until: n.mute_until.unwrap_or(0), silent: n.silent.unwrap_or(false), inherit: n.mute_until.is_none() }
    }
}

/// Full data for the info panel.
#[derive(Debug, Clone, Default)]
pub struct ChatDetails {
    /// Group/channel description or user bio.
    pub about: String,
    /// Without the "@".
    pub username: String,
    pub phone: String,
    /// Invite link (admins have one).
    pub invite: String,
    pub members: Option<i32>,
    pub online: Option<i32>,
    pub admins: Option<i32>,
    /// May invite: can create a link when there is none yet.
    pub admin: bool,
    /// We own the group or channel: "Delete" removes it for everyone instead of leaving.
    pub creator: bool,
    /// Member list is visible (in channels, only to admins).
    pub can_view_members: bool,
    /// Groups in common with the user.
    pub common: i32,
    /// We blocked this user (or stopped this bot).
    pub blocked: bool,
    /// A user's birthday, if they share it.
    pub birthday: Option<Birthday>,
    /// We may change the title, photo, description and permissions.
    pub can_edit: bool,
    /// Default member permissions (groups only).
    pub rights: Option<ChatRights>,
    /// Slow mode in seconds (supergroups only).
    pub slowmode: Option<i32>,
    pub notify: NotifyState,
    /// Item counts per shared media tab.
    pub counts: Vec<(SharedKind, i32)>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MemberRole {
    Member,
    Admin,
    Owner,
}

/// Group member, carried as a chat row so a click can open the private chat.
#[derive(Debug, Clone)]
pub struct Member {
    pub chat: ChatInfo,
    pub role: MemberRole,
    /// Admin title (custom rank).
    pub rank: String,
}

impl Worker {
    /// Info panel commands. Slow requests run in the background so the queue keeps moving.
    pub(super) async fn chat_info_cmd(&mut self, cmd: Cmd) -> Result<(), String> {
        let client = self.client()?;
        let out = self.out.clone();
        match cmd {
            Cmd::LoadChatInfo(peer) => {
                let r = self.peer_ref(peer)?;
                let notifs = self.notifs.clone();
                tokio::spawn(async move {
                    out.send(match details(&client, r).await {
                        Ok(mut details) => {
                            details.notify = notifs.lock().unwrap().resolve(peer, details.notify);
                            Ev::ChatDetails { peer, details }
                        }
                        Err(e) => Ev::Error(lf!("Інформація про чат: {e}", "Chat info: {e}")),
                    });
                });
            }
            Cmd::LoadMembers { peer, query, admins } => {
                let r = self.peer_ref(peer)?;
                let (refs, names, loader) = (self.refs.clone(), self.names.clone(), self.loader.clone());
                tokio::spawn(async move {
                    let mut iter = client.iter_participants(r);
                    // Supergroups are searched by the server; small groups are filtered here.
                    if peer.kind() == PeerKind::Channel {
                        iter = iter.filter(if admins {
                            tl::types::ChannelParticipantsAdmins {}.into()
                        } else if query.is_empty() {
                            tl::types::ChannelParticipantsRecent {}.into()
                        } else {
                            tl::types::ChannelParticipantsSearch { q: query.clone() }.into()
                        });
                    }
                    let q = query.to_lowercase();
                    let mut list = Vec::new();
                    while list.len() < MEMBERS_LIMIT {
                        let p = match iter.next().await {
                            Ok(Some(p)) => p,
                            Ok(None) => break,
                            Err(e) => {
                                out.send(Ev::Error(lf!("Учасники: {e}", "Members: {e}")));
                                return;
                            }
                        };
                        use grammers_client::peer::Role;
                        let (role, rank) = match &p.role {
                            Role::Creator(c) => (MemberRole::Owner, c.rank().unwrap_or_default().to_owned()),
                            Role::Admin(a) => (MemberRole::Admin, a.rank().unwrap_or_default().to_owned()),
                            Role::Banned(_) | Role::Left(_) => continue,
                            _ => (MemberRole::Member, String::new()),
                        };
                        if admins && role == MemberRole::Member {
                            continue;
                        }
                        let user = Peer::User(p.user);
                        let name = peer_title(&user);
                        let username = user.username().unwrap_or_default().to_lowercase();
                        if peer.kind() != PeerKind::Channel && !q.is_empty() && !name.to_lowercase().contains(&q) && !username.contains(&q) {
                            continue;
                        }
                        if let Ok(Some(r)) = user.to_ref().await {
                            refs.lock().unwrap().entry(user.id()).or_insert(r);
                        }
                        names.lock().unwrap().insert(user.id(), name);
                        if let Some(l) = &loader {
                            l.avatar(&user);
                        }
                        list.push(Member { chat: found_chat(&user, String::new()), role, rank });
                    }
                    // Like Telegram: owner, admins, then everyone else.
                    list.sort_by_key(|m| match m.role {
                        MemberRole::Owner => 0,
                        MemberRole::Admin => 1,
                        MemberRole::Member => 2,
                    });
                    out.send(Ev::Members { peer, query, admins, list });
                });
            }
            Cmd::ExportInvite(peer) => {
                let r = self.peer_ref(peer)?;
                let req = tl::functions::messages::ExportChatInvite {
                    legacy_revoke_permanent: false,
                    request_needed: false,
                    peer: r.into(),
                    expire_date: None,
                    usage_limit: None,
                    title: None,
                    subscription_pricing: None,
                };
                let res = client.invoke(&req).await.map_err(|e| lf!("Посилання не створено: {e}", "Link not created: {e}"))?;
                if let tl::enums::ExportedChatInvite::ChatInviteExported(x) = res {
                    out.send(Ev::Invite { peer, link: x.link });
                }
            }
            Cmd::LoadShared { peer, kind, before } => {
                let r = self.peer_ref(peer)?;
                let (names, loader) = (self.names.clone(), self.loader.clone());
                tokio::spawn(async move {
                    let mut iter = client.search_messages(r).filter(kind.filter()).limit(SHARED_PAGE);
                    if let Some(id) = before {
                        iter = iter.offset_id(id);
                    }
                    let mut msgs = Vec::new();
                    loop {
                        match iter.next().await {
                            Ok(Some(m)) => {
                                if let Some(l) = &loader {
                                    l.message_photo(peer, &m);
                                }
                                remember_name(&names, &m);
                                let mut msg = convert(&m, &names);
                                msg.peer = peer;
                                msgs.push(msg);
                            }
                            Ok(None) => break,
                            Err(e) => {
                                out.send(Ev::Error(lf!("Спільні медіа: {e}", "Shared media: {e}")));
                                return;
                            }
                        }
                    }
                    out.send(Ev::Shared { peer, kind, msgs, older: before.is_some() });
                });
            }
            Cmd::SetNotify { peer, mute_until, silent } => {
                let r = self.peer_ref(peer)?;
                let req = tl::functions::account::UpdateNotifySettings {
                    peer: tl::types::InputNotifyPeer { peer: r.into() }.into(),
                    settings: tl::types::InputPeerNotifySettings {
                        show_previews: None,
                        silent: Some(silent),
                        mute_until: Some(mute_until),
                        sound: None,
                        stories_muted: None,
                        stories_hide_sender: None,
                        stories_sound: None,
                    }
                    .into(),
                };
                client.invoke(&req).await.map_err(|e| lf!("Не вдалося змінити сповіщення: {e}", "Could not change notifications: {e}"))?;
                // The server may round the time, so read back what it stored.
                let get = tl::functions::account::GetNotifySettings { peer: tl::types::InputNotifyPeer { peer: r.into() }.into() };
                let state = match client.invoke(&get).await {
                    Ok(raw) => self.notifs.lock().unwrap().resolve(peer, NotifyState::from_raw(&raw)),
                    Err(_) => NotifyState { mute_until, silent, inherit: false },
                };
                out.send(Ev::Notify { peer, state });
            }
            Cmd::LeaveChat(peer) => {
                let r = self.peer_ref(peer)?;
                leave(&client, r).await?;
                out.send(Ev::ChatRemoved(peer));
            }
            Cmd::ClearHistory { peer, revoke } => {
                let r = self.peer_ref(peer)?;
                if peer.kind() == PeerKind::Channel {
                    let req = tl::functions::channels::DeleteHistory { for_everyone: false, channel: r.into(), max_id: 0 };
                    client.invoke(&req).await.map_err(|e| lf!("Історію не очищено: {e}", "History not cleared: {e}"))?;
                } else {
                    delete_history(&client, r, true, revoke).await?;
                }
                out.send(Ev::HistoryCleared(peer));
            }
            Cmd::DeleteChat { peer, revoke } => {
                let r = self.peer_ref(peer)?;
                // `revoke` in a group or channel: the owner deletes it for everyone (otherwise it would stay orphaned).
                match peer.kind() {
                    PeerKind::Channel if revoke => {
                        let req = tl::functions::channels::DeleteChannel { channel: r.into() };
                        client.invoke(&req).await.map_err(|e| lf!("Не вдалося видалити: {e}", "Could not delete: {e}"))?;
                    }
                    PeerKind::Chat if revoke => {
                        let req = tl::functions::messages::DeleteChat { chat_id: r.id.bare_id_unchecked() };
                        client.invoke(&req).await.map_err(|e| lf!("Не вдалося видалити: {e}", "Could not delete: {e}"))?;
                    }
                    PeerKind::Channel => leave(&client, r).await?,
                    PeerKind::Chat => {
                        // Leave first, then drop the dialog from the list.
                        leave(&client, r).await?;
                        delete_history(&client, r, false, false).await?;
                    }
                    PeerKind::User => delete_history(&client, r, false, revoke).await?,
                }
                out.send(Ev::ChatRemoved(peer));
            }
            _ => {}
        }
        Ok(())
    }
}

/// Leaving: `leaveChannel` for channels and supergroups, `deleteChatUser` for small groups.
async fn leave(client: &Client, r: PeerRef) -> Result<(), String> {
    let err = |e| lf!("Не вдалося вийти: {e}", "Could not leave: {e}");
    match r.id.kind() {
        PeerKind::Channel => client.invoke(&tl::functions::channels::LeaveChannel { channel: r.into() }).await.map(drop).map_err(err),
        PeerKind::Chat => {
            let req = tl::functions::messages::DeleteChatUser { revoke_history: false, chat_id: r.into(), user_id: tl::enums::InputUser::UserSelf };
            client.invoke(&req).await.map(drop).map_err(err)
        }
        PeerKind::User => Ok(()),
    }
}

/// `deleteHistory` works in chunks: repeat until the server reports nothing left.
async fn delete_history(client: &Client, r: PeerRef, just_clear: bool, revoke: bool) -> Result<(), String> {
    loop {
        let req = tl::functions::messages::DeleteHistory { just_clear, revoke, peer: r.into(), max_id: 0, min_date: None, max_date: None };
        let tl::enums::messages::AffectedHistory::History(res) =
            client.invoke(&req).await.map_err(|e| lf!("Історію не видалено: {e}", "History not deleted: {e}"))?;
        if res.offset <= 0 {
            return Ok(());
        }
    }
}

/// User profile or full group/channel info.
async fn details(client: &Client, r: PeerRef) -> Result<ChatDetails, String> {
    let mut d = ChatDetails::default();
    match r.id.kind() {
        PeerKind::User => {
            let tl::enums::users::UserFull::Full(f) = client.invoke(&tl::functions::users::GetFullUser { id: r.into() }).await.map_err(|e| e.to_string())?;
            let tl::enums::UserFull::Full(u) = f.full_user;
            d.about = u.about.unwrap_or_default();
            d.common = u.common_chats_count;
            d.blocked = u.blocked;
            d.birthday = u.birthday.as_ref().map(Birthday::from_raw);
            d.notify = NotifyState::from_raw(&u.notify_settings);
            if let Some(tl::enums::User::User(x)) = f.users.into_iter().find(|x| x.id() == u.id) {
                d.username = first_username(x.username, x.usernames);
                d.phone = x.phone.map(|p| format!("+{p}")).unwrap_or_default();
            }
        }
        PeerKind::Chat => {
            let tl::enums::messages::ChatFull::Full(f) =
                client.invoke(&tl::functions::messages::GetFullChat { chat_id: r.into() }).await.map_err(|e| e.to_string())?;
            if let tl::enums::ChatFull::Full(c) = f.full_chat {
                d.about = c.about;
                d.notify = NotifyState::from_raw(&c.notify_settings);
                d.invite = invite_link(c.exported_invite);
                if let tl::enums::ChatParticipants::Participants(p) = &c.participants {
                    d.members = Some(p.participants.len() as i32);
                    d.admins = Some(p.participants.iter().filter(|x| !matches!(x, tl::enums::ChatParticipant::Participant(_))).count() as i32);
                }
            }
            if let Some(tl::enums::Chat::Chat(c)) = f.chats.into_iter().find(|c| c.id() == r.id.bare_id_unchecked()) {
                d.creator = c.creator;
                d.admin = c.creator || matches!(&c.admin_rights, Some(tl::enums::ChatAdminRights::Rights(a)) if a.invite_users);
                d.can_edit = c.creator || matches!(&c.admin_rights, Some(tl::enums::ChatAdminRights::Rights(a)) if a.change_info);
                d.rights = Some(ChatRights::from_banned(c.default_banned_rights.as_ref()));
            }
            d.can_view_members = true;
        }
        PeerKind::Channel => {
            let tl::enums::messages::ChatFull::Full(f) =
                client.invoke(&tl::functions::channels::GetFullChannel { channel: r.into() }).await.map_err(|e| e.to_string())?;
            if let tl::enums::ChatFull::ChannelFull(c) = f.full_chat {
                d.about = c.about;
                d.notify = NotifyState::from_raw(&c.notify_settings);
                d.invite = invite_link(c.exported_invite);
                d.members = c.participants_count;
                d.online = c.online_count;
                d.admins = c.admins_count;
                d.can_view_members = c.can_view_participants;
                d.slowmode = Some(c.slowmode_seconds.unwrap_or(0));
            }
            if let Some(tl::enums::Chat::Channel(c)) = f.chats.into_iter().find(|c| c.id() == r.id.bare_id_unchecked()) {
                d.creator = c.creator;
                d.admin = c.creator || matches!(&c.admin_rights, Some(tl::enums::ChatAdminRights::Rights(a)) if a.invite_users);
                d.username = first_username(c.username, c.usernames);
                d.can_edit = c.creator || matches!(&c.admin_rights, Some(tl::enums::ChatAdminRights::Rights(a)) if a.change_info);
                if c.megagroup {
                    d.rights = Some(ChatRights::from_banned(c.default_banned_rights.as_ref()));
                } else {
                    d.slowmode = None;
                }
            }
        }
    }
    let filters = SharedKind::ALL.map(SharedKind::filter).to_vec();
    let req = tl::functions::messages::GetSearchCounters { peer: r.into(), saved_peer_id: None, top_msg_id: None, filters };
    if let Ok(counters) = client.invoke(&req).await {
        d.counts = SharedKind::ALL.into_iter().zip(counters).map(|(k, tl::enums::messages::SearchCounter::Counter(c))| (k, c.count)).collect();
    }
    Ok(d)
}

/// Main username, or else the first active collectible one.
fn first_username(main: Option<String>, all: Option<Vec<tl::enums::Username>>) -> String {
    main.or_else(|| {
        all?.into_iter().find_map(|tl::enums::Username::Username(u)| u.active.then_some(u.username))
    })
    .unwrap_or_default()
}

fn invite_link(raw: Option<tl::enums::ExportedChatInvite>) -> String {
    match raw {
        Some(tl::enums::ExportedChatInvite::ChatInviteExported(x)) if !x.revoked => x.link,
        _ => String::new(),
    }
}
