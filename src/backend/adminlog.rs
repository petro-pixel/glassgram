//! Channel and supergroup management from Telegram Desktop's "Manage" box: the "Recent actions" admin
//! log (`channels.getAdminLog`), author signatures and the discussion group of a channel.

use super::*;

/// How many events one request brings; older ones are paged with `max_id`.
const LOG_PAGE: i32 = 100;

/// One line of the admin log: who did what and when.
#[derive(Debug, Clone)]
pub struct AdminLogEntry {
    pub id: i64,
    pub date: String,
    pub who: String,
    pub what: String,
    /// The old or new text, title or message involved; may be empty.
    pub detail: String,
}

impl Worker {
    pub(super) async fn admin_cmd(&mut self, cmd: Cmd) -> Result<(), String> {
        let client = self.client()?;
        let err = |e: grammers_client::InvocationError| lf!("Не вдалося змінити чат: {e}", "Could not edit the chat: {e}");
        match cmd {
            Cmd::LoadAdminLog { peer, before } => {
                let r = self.peer_ref(peer)?;
                let out = self.out.clone();
                tokio::spawn(async move {
                    let req = tl::functions::channels::GetAdminLog { channel: r.into(), q: String::new(), events_filter: None, admins: None, max_id: before.unwrap_or(0), min_id: 0, limit: LOG_PAGE };
                    match client.invoke(&req).await {
                        Ok(tl::enums::channels::AdminLogResults::Results(res)) => {
                            let names = user_names(&res.users);
                            let entries = res.events.into_iter().map(|tl::enums::ChannelAdminLogEvent::Event(e)| entry(e, &names)).collect();
                            out.send(Ev::AdminLog { peer, entries, older: before.is_some() });
                        }
                        Err(e) => out.send(Ev::Error(lf!("Останні дії: {e}", "Recent actions: {e}"))),
                    }
                });
            }
            Cmd::SetSignatures { peer, on } => {
                let r = self.peer_ref(peer)?;
                client.invoke(&tl::functions::channels::ToggleSignatures { signatures_enabled: on, profiles_enabled: false, channel: r.into() }).await.map_err(err)?;
                self.out.send(Ev::Notice(l!("Зміни збережено", "Changes saved").into()));
                let _ = self.back.send(Cmd::LoadChatInfo(peer));
            }
            Cmd::LoadDiscussionGroups(peer) => {
                let res = client.invoke(&tl::functions::channels::GetGroupsForDiscussion {}).await.map_err(|e| lf!("Групи: {e}", "Groups: {e}"))?;
                let chats = match res {
                    tl::enums::messages::Chats::Chats(c) => c.chats,
                    tl::enums::messages::Chats::Slice(c) => c.chats,
                };
                let mut groups = Vec::new();
                for raw in chats {
                    let p = Peer::from_raw(&client, raw);
                    if self.remember(&p).await {
                        groups.push(found_chat(&p, String::new()));
                    }
                }
                self.out.send(Ev::DiscussionGroups { peer, groups });
            }
            Cmd::SetDiscussionGroup { peer, group } => {
                let r = self.peer_ref(peer)?;
                // An empty channel unlinks the group.
                let group: tl::enums::InputChannel = match group {
                    Some(g) => self.peer_ref(g)?.into(),
                    None => tl::enums::InputChannel::Empty,
                };
                client.invoke(&tl::functions::channels::SetDiscussionGroup { broadcast: r.into(), group }).await.map_err(err)?;
                self.out.send(Ev::Notice(l!("Зміни збережено", "Changes saved").into()));
                let _ = self.back.send(Cmd::LoadChatInfo(peer));
            }
            _ => {}
        }
        Ok(())
    }
}

fn user_names(users: &[tl::enums::User]) -> HashMap<i64, String> {
    users
        .iter()
        .filter_map(|u| match u {
            tl::enums::User::User(u) => Some((u.id, [u.first_name.clone(), u.last_name.clone()].into_iter().flatten().collect::<Vec<_>>().join(" "))),
            _ => None,
        })
        .collect()
}

fn text_of(m: &tl::enums::Message) -> String {
    match m {
        tl::enums::Message::Message(m) if !m.message.is_empty() => m.message.replace('\n', " "),
        tl::enums::Message::Message(_) => l!("Медіа", "Media").to_owned(),
        _ => String::new(),
    }
}

fn on_off(on: bool) -> &'static str {
    if on { l!("увімкнено", "on") } else { l!("вимкнено", "off") }
}

/// Text for one event, worded like Telegram Desktop's "Recent actions".
fn entry(e: tl::types::ChannelAdminLogEvent, names: &HashMap<i64, String>) -> AdminLogEntry {
    use tl::enums::ChannelAdminLogEventAction as A;
    let (what, detail): (&str, String) = match e.action {
        A::ChangeTitle(a) => (l!("змінює назву", "changed the title"), format!("{} → {}", a.prev_value, a.new_value)),
        A::ChangeAbout(a) => (l!("змінює опис", "changed the description"), a.new_value),
        A::ChangeUsername(a) => (l!("змінює посилання", "changed the link"), a.new_value),
        A::ChangeUsernames(_) => (l!("змінює посилання", "changed the links"), String::new()),
        A::ChangePhoto(_) => (l!("змінює фото", "changed the photo"), String::new()),
        A::ToggleInvites(a) => (l!("запрошення учасниками", "invites by members"), on_off(a.new_value).into()),
        A::ToggleSignatures(a) => (l!("підписи авторів", "author signatures"), on_off(a.new_value).into()),
        A::UpdatePinned(a) => (l!("закріплює повідомлення", "pinned a message"), text_of(&a.message)),
        A::EditMessage(a) => (l!("редагує повідомлення", "edited a message"), text_of(&a.new_message)),
        A::DeleteMessage(a) => (l!("видаляє повідомлення", "deleted a message"), text_of(&a.message)),
        A::SendMessage(a) => (l!("надсилає повідомлення", "sent a message"), text_of(&a.message)),
        A::ParticipantJoin | A::ParticipantJoinByInvite(_) | A::ParticipantJoinByRequest(_) => (l!("приєднується", "joined"), String::new()),
        A::ParticipantLeave => (l!("виходить", "left"), String::new()),
        A::ParticipantInvite(_) => (l!("запрошує учасника", "invited a member"), String::new()),
        A::ParticipantToggleBan(_) => (l!("змінює обмеження учасника", "changed a member's restrictions"), String::new()),
        A::ParticipantToggleAdmin(_) => (l!("змінює права адміністратора", "changed admin rights"), String::new()),
        A::DefaultBannedRights(_) => (l!("змінює дозволи учасників", "changed member permissions"), String::new()),
        A::StopPoll(a) => (l!("зупиняє опитування", "stopped a poll"), text_of(&a.message)),
        A::ChangeLinkedChat(_) => (l!("змінює групу обговорення", "changed the discussion group"), String::new()),
        A::ToggleSlowMode(a) => (l!("повільний режим", "slow mode"), if a.new_value == 0 { on_off(false).into() } else { lf!("{} с", "{}s", a.new_value) }),
        A::ChangeHistoryTtl(a) => (l!("автовидалення", "auto-delete"), if a.new_value == 0 { on_off(false).into() } else { lf!("{} с", "{}s", a.new_value) }),
        A::ToggleNoForwards(a) => (l!("захист вмісту", "content protection"), on_off(a.new_value).into()),
        A::ChangeAvailableReactions(_) => (l!("змінює дозволені реакції", "changed allowed reactions"), String::new()),
        A::ToggleAntiSpam(a) => (l!("агресивний антиспам", "aggressive anti-spam"), on_off(a.new_value).into()),
        A::TogglePreHistoryHidden(a) => (l!("історія для нових учасників", "history for new members"), on_off(!a.new_value).into()),
        A::ToggleForum(a) => (l!("теми", "topics"), on_off(a.new_value).into()),
        A::CreateTopic(_) => (l!("створює тему", "created a topic"), String::new()),
        A::EditTopic(_) => (l!("змінює тему", "edited a topic"), String::new()),
        A::DeleteTopic(_) => (l!("видаляє тему", "deleted a topic"), String::new()),
        A::ExportedInviteDelete(_) | A::ExportedInviteRevoke(_) | A::ExportedInviteEdit(_) => (l!("змінює посилання-запрошення", "changed an invite link"), String::new()),
        A::StartGroupCall(_) => (l!("починає відеочат", "started a video chat"), String::new()),
        A::DiscardGroupCall(_) => (l!("завершує відеочат", "ended a video chat"), String::new()),
        _ => (l!("інша зміна", "made another change"), String::new()),
    };
    let ts = jiff::Timestamp::from_second(e.date as i64).unwrap_or_default();
    AdminLogEntry {
        id: e.id,
        date: local(ts).strftime("%d.%m %H:%M").to_string(),
        who: names.get(&e.user_id).cloned().unwrap_or_else(|| l!("Хтось", "Someone").to_owned()),
        what: what.to_owned(),
        detail: detail.chars().take(160).collect(),
    }
}
