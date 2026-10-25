//! Chat view requests: pinned messages, unread mentions and reactions, "seen by".
//! Each request runs as its own task so opening a chat never waits for them.

use super::*;

impl Worker {
    pub(super) async fn chat_view_cmd(&mut self, cmd: Cmd) -> Result<(), String> {
        let client = self.client()?;
        let out = self.out.clone();
        match cmd {
            Cmd::LoadPinned(peer) => {
                let r = self.peer_ref(peer)?;
                let (loader, names) = (self.loader.clone(), self.names.clone());
                tokio::spawn(async move {
                    let mut iter = client.search_messages(r).filter(tl::enums::MessagesFilter::InputMessagesFilterPinned).limit(PINNED_LIMIT);
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
                            Err(e) => return out.send(Ev::Error(lf!("Закріплені: {e}", "Pinned: {e}"))),
                        }
                    }
                    msgs.sort_by_key(|m| m.id);
                    out.send(Ev::Pinned { peer, msgs });
                });
            }
            Cmd::PinMessage { peer, id, unpin, silent, one_side } => {
                let r = self.peer_ref(peer)?;
                tokio::spawn(async move {
                    let req = tl::functions::messages::UpdatePinnedMessage { silent, unpin, pm_oneside: one_side, peer: r.into(), id };
                    match client.invoke(&req).await {
                        // The server may not echo our own action as an update — tell the UI ourselves.
                        Ok(_) => out.send(Ev::PinnedChanged { peer, ids: vec![id], pinned: !unpin }),
                        Err(e) => out.send(Ev::Error(if unpin { lf!("Не відкріплено: {e}", "Not unpinned: {e}") } else { lf!("Не закріплено: {e}", "Not pinned: {e}") })),
                    }
                });
            }
            Cmd::UnpinAll(peer) => {
                let r = self.peer_ref(peer)?;
                tokio::spawn(async move {
                    let req = tl::functions::messages::UnpinAllMessages { peer: r.into(), top_msg_id: None, saved_peer_id: None };
                    match client.invoke(&req).await {
                        Ok(_) => out.send(Ev::Pinned { peer, msgs: Vec::new() }),
                        Err(e) => out.send(Ev::Error(lf!("Не відкріплено: {e}", "Not unpinned: {e}"))),
                    }
                });
            }
            Cmd::LoadUnreadMarks { peer, reactions } => {
                let r = self.peer_ref(peer)?;
                tokio::spawn(async move {
                    let res = if reactions {
                        let req = tl::functions::messages::GetUnreadReactions {
                            peer: r.into(),
                            top_msg_id: None,
                            saved_peer_id: None,
                            offset_id: 0,
                            add_offset: 0,
                            limit: MARKS_LIMIT,
                            max_id: 0,
                            min_id: 0,
                        };
                        client.invoke(&req).await
                    } else {
                        let req = tl::functions::messages::GetUnreadMentions { peer: r.into(), top_msg_id: None, offset_id: 0, add_offset: 0, limit: MARKS_LIMIT, max_id: 0, min_id: 0 };
                        client.invoke(&req).await
                    };
                    match res {
                        Ok(list) => {
                            let mut ids = message_ids(list);
                            ids.sort_unstable();
                            out.send(Ev::UnreadMarks { peer, reactions, ids });
                        }
                        Err(e) => out.send(Ev::Error(lf!("Непрочитані: {e}", "Unread: {e}"))),
                    }
                });
            }
            Cmd::ReadMarks { peer, reactions } => {
                let r = self.peer_ref(peer)?;
                tokio::spawn(async move {
                    let res = if reactions {
                        client.invoke(&tl::functions::messages::ReadReactions { peer: r.into(), top_msg_id: None, saved_peer_id: None }).await
                    } else {
                        client.invoke(&tl::functions::messages::ReadMentions { peer: r.into(), top_msg_id: None }).await
                    };
                    if let Err(e) = res {
                        out.send(Ev::Error(lf!("Непрочитані: {e}", "Unread: {e}")));
                    }
                });
            }
            Cmd::LoadReadBy { peer, id } => {
                let r = self.peer_ref(peer)?;
                let names = self.names.clone();
                let session = self.session.clone();
                tokio::spawn(async move {
                    let list = match client.invoke(&tl::functions::messages::GetMessageReadParticipants { peer: r.into(), msg_id: id }).await {
                        Ok(l) => l,
                        Err(e) => return out.send(Ev::Error(lf!("Переглянуто: {e}", "Seen: {e}"))),
                    };
                    let readers: Vec<(PeerId, i32)> = list
                        .into_iter()
                        .filter_map(|p| {
                            let tl::enums::ReadParticipantDate::Date(d) = p;
                            Some((PeerId::user(d.user_id)?, d.date))
                        })
                        .collect();
                    // Fetch unknown names via the session: it keeps access hashes of everyone we have seen.
                    let unknown: Vec<PeerId> = {
                        let known = names.lock().unwrap();
                        readers.iter().map(|r| r.0).filter(|p| !known.contains_key(p)).collect()
                    };
                    if let Some(s) = session.filter(|_| !unknown.is_empty()) {
                        let mut ids = Vec::new();
                        for p in unknown {
                            if let Ok(Some(info)) = s.peer(p).await
                                && let Some(auth) = info.auth()
                            {
                                ids.push(tl::enums::InputUser::from(PeerRef { id: p, auth }));
                            }
                        }
                        if !ids.is_empty()
                            && let Ok(users) = client.invoke(&tl::functions::users::GetUsers { id: ids }).await
                        {
                            let mut known = names.lock().unwrap();
                            for u in users {
                                let peer = Peer::User(User::from_raw(&client, u));
                                known.insert(peer.id(), peer_title(&peer));
                            }
                        }
                    }
                    let known = names.lock().unwrap();
                    let readers = readers
                        .into_iter()
                        .map(|(p, date)| {
                            let name = known.get(&p).cloned().unwrap_or_else(|| l!("Учасник", "Member").to_owned());
                            let when = jiff::Timestamp::from_second(date as i64).map(|t| format!("{} {}", day(t), clock(t))).unwrap_or_default();
                            (name, when)
                        })
                        .collect();
                    drop(known);
                    out.send(Ev::ReadBy { peer, id, readers });
                });
            }
            Cmd::JumpToDate { peer, date } => {
                let r = self.peer_ref(peer)?;
                tokio::spawn(async move {
                    // History is newest first and `offset_date` returns older messages,
                    // so `add_offset: -1` steps back to the first message at or after the date.
                    let req = |offset_date, add_offset| tl::functions::messages::GetHistory { peer: r.into(), offset_id: 0, offset_date, add_offset, limit: 1, max_id: 0, min_id: 0, hash: 0 };
                    let found = match client.invoke(&req(date, -1)).await {
                        Ok(list) => message_ids(list).first().copied(),
                        Err(e) => return out.send(Ev::Error(lf!("Перехід до дати: {e}", "Jump to date: {e}"))),
                    };
                    // Nothing that late: the date is after the last message, so show the newest.
                    let id = match found {
                        Some(id) => Some(id),
                        None => client.invoke(&req(0, 0)).await.ok().and_then(|l| message_ids(l).first().copied()),
                    };
                    out.send(Ev::DateFound { peer, id });
                });
            }
            _ => {}
        }
        Ok(())
    }
}

/// How many pinned messages the list shows.
const PINNED_LIMIT: usize = 100;
/// Mentions or reactions per request; the button walks through them one by one.
const MARKS_LIMIT: i32 = 100;

fn message_ids(list: tl::enums::messages::Messages) -> Vec<i32> {
    match list {
        tl::enums::messages::Messages::Messages(m) => m.messages,
        tl::enums::messages::Messages::Slice(m) => m.messages,
        tl::enums::messages::Messages::ChannelMessages(m) => m.messages,
        tl::enums::messages::Messages::NotModified(_) => Vec::new(),
    }
    .iter()
    .map(|m| m.id())
    .collect()
}
