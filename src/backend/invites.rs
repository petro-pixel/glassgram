//! Invite links and join requests of a group or channel, as in Telegram Desktop's "Invite links":
//! our links with their usage, new links with a name, expiry, limit or admin approval
//! (`messages.exportChatInvite`, `getExportedChatInvites`, `editExportedChatInvite`), and the
//! requests waiting for approval (`getChatInviteImporters`, `hideChatJoinRequest`).

use super::*;

/// Links and requests per request; more than enough for a desktop list.
const LIMIT: i32 = 100;

/// One of our invite links.
#[derive(Debug, Clone, Default)]
pub struct InviteLink {
    pub link: String,
    pub title: String,
    /// People who joined through it.
    pub usage: i32,
    /// 0 — no limit.
    pub usage_limit: i32,
    /// Unix time it stops working (0 — never).
    pub expire: i32,
    /// Join requests waiting on this link.
    pub requested: i32,
    pub request_needed: bool,
    pub revoked: bool,
    /// The chat's primary link: it can be replaced but not edited or deleted.
    pub permanent: bool,
}

impl InviteLink {
    pub fn expired(&self) -> bool {
        (self.expire > 0 && self.expire as i64 <= jiff::Timestamp::now().as_second()) || (self.usage_limit > 0 && self.usage >= self.usage_limit)
    }

    fn from_raw(raw: tl::enums::ExportedChatInvite) -> Option<Self> {
        let tl::enums::ExportedChatInvite::ChatInviteExported(x) = raw else { return None };
        Some(Self {
            link: x.link,
            title: x.title.unwrap_or_default(),
            usage: x.usage.unwrap_or(0),
            usage_limit: x.usage_limit.unwrap_or(0),
            expire: x.expire_date.unwrap_or(0),
            requested: x.requested.unwrap_or(0),
            request_needed: x.request_needed,
            revoked: x.revoked,
            permanent: x.permanent,
        })
    }
}

/// What the "New link" / "Edit link" box sets.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct InviteDraft {
    pub title: String,
    /// Unix time (0 — never expires).
    pub expire: i32,
    /// 0 — no limit; not allowed together with approval.
    pub usage_limit: i32,
    pub request_needed: bool,
}

/// A person asking to join.
#[derive(Debug, Clone)]
pub struct JoinRequest {
    pub chat: ChatInfo,
    pub date: String,
    /// What they wrote with the request.
    pub about: String,
}

impl Worker {
    pub(super) async fn invites_cmd(&mut self, cmd: Cmd) -> Result<(), String> {
        let client = self.client()?;
        let err = |e: grammers_client::InvocationError| lf!("Посилання: {e}", "Invite link: {e}");
        match cmd {
            Cmd::LoadInviteLinks(peer) => {
                let r = self.peer_ref(peer)?;
                let mut links = Vec::new();
                // Active links first, then the revoked ones, as two lists in Telegram Desktop.
                for revoked in [false, true] {
                    let req = tl::functions::messages::GetExportedChatInvites { revoked, peer: r.into(), admin_id: tl::enums::InputUser::UserSelf, offset_date: None, offset_link: None, limit: LIMIT };
                    let tl::enums::messages::ExportedChatInvites::Invites(res) = client.invoke(&req).await.map_err(err)?;
                    links.extend(res.invites.into_iter().filter_map(InviteLink::from_raw));
                }
                self.out.send(Ev::InviteLinks { peer, links });
            }
            Cmd::SaveInviteLink { peer, link, draft } => {
                let r = self.peer_ref(peer)?;
                let title = Some(draft.title.trim().to_owned()).filter(|t| !t.is_empty());
                match link {
                    None => {
                        let req = tl::functions::messages::ExportChatInvite {
                            legacy_revoke_permanent: false,
                            request_needed: draft.request_needed,
                            peer: r.into(),
                            expire_date: (draft.expire > 0).then_some(draft.expire),
                            usage_limit: (draft.usage_limit > 0 && !draft.request_needed).then_some(draft.usage_limit),
                            title,
                            subscription_pricing: None,
                        };
                        client.invoke(&req).await.map_err(err)?;
                    }
                    Some(link) => {
                        // Zero clears the expiry and the limit.
                        let req = tl::functions::messages::EditExportedChatInvite {
                            revoked: false,
                            peer: r.into(),
                            link,
                            expire_date: Some(draft.expire),
                            usage_limit: Some(if draft.request_needed { 0 } else { draft.usage_limit }),
                            request_needed: Some(draft.request_needed),
                            title: Some(title.unwrap_or_default()),
                        };
                        client.invoke(&req).await.map_err(err)?;
                    }
                }
                self.links_changed(peer);
            }
            Cmd::RevokeInviteLink { peer, link } => {
                let r = self.peer_ref(peer)?;
                let req = tl::functions::messages::EditExportedChatInvite { revoked: true, peer: r.into(), link, expire_date: None, usage_limit: None, request_needed: None, title: None };
                client.invoke(&req).await.map_err(err)?;
                self.links_changed(peer);
                // Revoking the primary link makes a new one: the info panel shows it.
                let _ = self.back.send(Cmd::LoadChatInfo(peer));
            }
            Cmd::DeleteInviteLink { peer, link } => {
                let r = self.peer_ref(peer)?;
                client.invoke(&tl::functions::messages::DeleteExportedChatInvite { peer: r.into(), link }).await.map_err(err)?;
                self.links_changed(peer);
            }
            Cmd::DeleteRevokedInvites(peer) => {
                let r = self.peer_ref(peer)?;
                client.invoke(&tl::functions::messages::DeleteRevokedExportedChatInvites { peer: r.into(), admin_id: tl::enums::InputUser::UserSelf }).await.map_err(err)?;
                self.links_changed(peer);
            }
            Cmd::LoadJoinRequests(peer) => {
                let r = self.peer_ref(peer)?;
                let req = tl::functions::messages::GetChatInviteImporters {
                    requested: true,
                    subscription_expired: false,
                    peer: r.into(),
                    link: None,
                    q: None,
                    offset_date: 0,
                    offset_user: tl::enums::InputUser::Empty,
                    limit: LIMIT,
                };
                let tl::enums::messages::ChatInviteImporters::Importers(res) = client.invoke(&req).await.map_err(|e| lf!("Заявки: {e}", "Join requests: {e}"))?;
                let mut people = HashMap::new();
                for u in res.users {
                    let p = Peer::User(User::from_raw(&client, u));
                    if self.remember(&p).await {
                        people.insert(p.id().bare_id_unchecked(), found_chat(&p, String::new()));
                    }
                }
                let list = res
                    .importers
                    .into_iter()
                    .filter_map(|tl::enums::ChatInviteImporter::Importer(i)| {
                        let chat = people.get(&i.user_id)?.clone();
                        let ts = jiff::Timestamp::from_second(i.date as i64).unwrap_or_default();
                        Some(JoinRequest { chat, date: local(ts).strftime("%d.%m %H:%M").to_string(), about: i.about.unwrap_or_default() })
                    })
                    .collect();
                self.out.send(Ev::JoinRequests { peer, list });
            }
            Cmd::AnswerJoinRequest { peer, user, approve } => {
                let r = self.peer_ref(peer)?;
                let err = |e: grammers_client::InvocationError| lf!("Заявки: {e}", "Join requests: {e}");
                match user {
                    Some(user) => {
                        let user_id = self.peer_ref(user)?.into();
                        client.invoke(&tl::functions::messages::HideChatJoinRequest { approved: approve, peer: r.into(), user_id }).await.map_err(err)?;
                    }
                    None => {
                        client.invoke(&tl::functions::messages::HideAllChatJoinRequests { approved: approve, peer: r.into(), link: None }).await.map_err(err)?;
                    }
                }
                let _ = self.back.send(Cmd::LoadJoinRequests(peer));
                let _ = self.back.send(Cmd::LoadChatInfo(peer));
            }
            _ => {}
        }
        Ok(())
    }

    fn links_changed(&self, peer: PeerId) {
        self.out.send(Ev::Notice(l!("Зміни збережено", "Changes saved").into()));
        let _ = self.back.send(Cmd::LoadInviteLinks(peer));
    }
}
