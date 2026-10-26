//! Long-running loops: live updates from Telegram and QR login.

use super::*;

pub(super) fn is_login_token_update(u: &UpdatesLike) -> bool {
    let is = |x: &tl::enums::Update| matches!(x, tl::enums::Update::LoginToken);
    match u {
        UpdatesLike::Updates(tl::enums::Updates::UpdateShort(s)) => is(&s.update),
        UpdatesLike::Updates(tl::enums::Updates::Updates(s)) => s.updates.iter().any(is),
        UpdatesLike::Updates(tl::enums::Updates::Combined(s)) => s.updates.iter().any(is),
        _ => false,
    }
}

pub(super) async fn update_loop(
    client: Client,
    mut updates: tmpsc::Receiver<UpdatesLike>,
    authorized: Arc<Notify>,
    login_update: Arc<Notify>,
    out: Emitter,
    refs: Refs,
    names: Names,
    notifs: notify::Notifs,
    loader: Loader,
) {
    // Before authorization we only listen for updateLoginToken (the signal that the QR was scanned).
    loop {
        tokio::select! {
            _ = authorized.notified() => break,
            u = updates.recv() => match u {
                Some(u) if is_login_token_update(&u) => login_update.notify_one(),
                Some(_) => {}
                None => return,
            },
        }
    }
    let mut stream = match client.stream_updates(updates, UpdatesConfiguration { catch_up: false }).await {
        Ok(s) => s,
        Err(e) => return out.send(Ev::Error(lf!("Оновлення: {e}", "Updates: {e}"))),
    };
    loop {
        match stream.next().await {
            Ok(Update::NewMessage(m)) => {
                if let Ok(Some(r)) = m.peer_ref().await {
                    refs.lock().unwrap().insert(r.id, r);
                }
                loader.message_photo(m.peer_id(), &m);
                remember_name(&names, &m);
                if let Some(s) = m.sender() {
                    out.send(Ev::Typing { peer: m.peer_id(), who: short_name(s), active: false });
                }
                out.send(Ev::NewMsg(convert(&m, &names)));
            }
            Ok(Update::MessageEdited(m)) => out.send(Ev::Edited(convert(&m, &names))),
            Ok(Update::MessageDeleted(d)) => {
                let peer = d.channel_id().and_then(PeerId::channel);
                out.send(Ev::Deleted { ids: d.into_messages(), peer })
            }
            Ok(Update::Raw(raw)) => {
                if let Some(ev) = raw_event(&raw.raw, &names, &notifs) {
                    out.send(ev);
                }
            }
            Ok(_) => {}
            Err(e) => {
                out.send(Ev::Error(lf!("З'єднання втрачено: {e}", "Connection lost: {e}")));
                break;
            }
        }
    }
}

/// Raw updates that grammers has no wrappers for: typing, statuses, reads, drafts.
pub(super) fn raw_event(u: &tl::enums::Update, names: &Names, notifs: &notify::Notifs) -> Option<Ev> {
    use tl::enums::{SendMessageAction as A, Update as U};
    let typing = |peer: PeerId, from: PeerId, action: &A| {
        let who = names.lock().unwrap().get(&from).cloned().unwrap_or_default();
        Ev::Typing { peer, who, active: !matches!(action, A::SendMessageCancelAction) }
    };
    Some(match u {
        U::UserTyping(t) => {
            let peer = PeerId::user(t.user_id)?;
            typing(peer, peer, &t.action)
        }
        U::ChatUserTyping(t) => typing(PeerId::chat(t.chat_id)?, PeerId::from(&t.from_id), &t.action),
        U::ChannelUserTyping(t) => typing(PeerId::channel(t.channel_id)?, PeerId::from(&t.from_id), &t.action),
        U::UserStatus(s) => Ev::Presence { peer: PeerId::user(s.user_id)?, presence: Presence::from_status(&s.status) },
        U::ReadHistoryInbox(r) => Ev::ReadInbox { peer: PeerId::from(&r.peer), unread: r.still_unread_count },
        U::ReadChannelInbox(r) => Ev::ReadInbox { peer: PeerId::channel(r.channel_id)?, unread: r.still_unread_count },
        U::ReadHistoryOutbox(r) => Ev::ReadOutbox { peer: PeerId::from(&r.peer), max_id: r.max_id },
        U::ReadChannelOutbox(r) => Ev::ReadOutbox { peer: PeerId::channel(r.channel_id)?, max_id: r.max_id },
        U::ReadMessagesContents(r) => Ev::Listened { peer: None, ids: r.messages.clone() },
        U::ChannelReadMessagesContents(r) => Ev::Listened { peer: Some(PeerId::channel(r.channel_id)?), ids: r.messages.clone() },
        U::MessageReactions(r) => Ev::Reactions { peer: PeerId::from(&r.peer), id: r.msg_id, reactions: reactions(&r.reactions) },
        U::PinnedMessages(p) => Ev::PinnedChanged { peer: PeerId::from(&p.peer), ids: p.messages.clone(), pinned: p.pinned },
        U::PinnedChannelMessages(p) => Ev::PinnedChanged { peer: PeerId::channel(p.channel_id)?, ids: p.messages.clone(), pinned: p.pinned },
        U::ChannelMessageViews(v) => Ev::Views { peer: PeerId::channel(v.channel_id)?, id: v.id, views: v.views },
        U::NotifySettings(n) => return notify::update_event(n, notifs),
        U::DraftMessage(d) if d.top_msg_id.is_none() => Ev::Draft { peer: PeerId::from(&d.peer), text: draft_text(&Some(d.draft.clone())) },
        _ => return None,
    })
}

/// QR login: show the token, wait for the scan, handle DC migration and 2FA.
pub(super) async fn qr_loop(
    client: Client,
    session: Arc<SqliteSession>,
    api_id: i32,
    api_hash: String,
    login_update: Arc<Notify>,
    out: Emitter,
    back: tmpsc::UnboundedSender<Cmd>,
) {
    use tl::enums::auth::LoginToken as T;
    let export = tl::functions::auth::ExportLoginToken { api_id, api_hash, except_ids: Vec::new() };
    let mut result = client.invoke(&export).await;
    loop {
        match result {
            Ok(T::Token(t)) => {
                let token = base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(&t.token);
                out.send(Ev::Qr(format!("tg://login?token={token}")));
                let left = (t.expires as i64 - jiff::Timestamp::now().as_second()).clamp(3, 30);
                tokio::select! {
                    _ = login_update.notified() => {}
                    _ = tokio::time::sleep(Duration::from_secs(left as u64)) => {}
                }
                result = client.invoke(&export).await;
            }
            Ok(T::MigrateTo(m)) => {
                if let Err(e) = session.set_home_dc_id(m.dc_id).await {
                    return out.send(Ev::Error(lf!("Міграція DC: {e}", "DC migration: {e}")));
                }
                result = client.invoke(&tl::functions::auth::ImportLoginToken { token: m.token }).await;
            }
            Ok(T::Success(s)) => {
                if let tl::enums::auth::Authorization::Authorization(a) = s.authorization {
                    let _ = back.send(Cmd::QrSuccess(Box::new(a)));
                }
                return;
            }
            Err(e) if e.is("SESSION_PASSWORD_NEEDED") => {
                match client.invoke(&tl::functions::account::GetPassword {}).await {
                    Ok(pw) => {
                        let tl::enums::account::Password::Password(pw) = pw;
                        let _ = back.send(Cmd::QrPassword(PasswordToken::new(pw)));
                    }
                    Err(e) => out.send(Ev::Error(format!("2FA: {e}"))),
                }
                return;
            }
            Err(e) => return out.send(Ev::Error(lf!("QR-вхід: {e}", "QR login: {e}"))),
        }
    }
}
