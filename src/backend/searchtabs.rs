//! Global search tabs as in Telegram Desktop 7.3: channels, apps (popular mini app bots) and
//! public posts. Chats and media go through `Cmd::Search` and `Cmd::SearchPublic`.

use super::*;

/// How many results one tab asks for.
const TAB_LIMIT: i32 = 30;

/// Free public post searches (`searchPostsFlood`): a daily quota, then paid with Stars.
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct PostsFlood {
    /// This query costs nothing (repeated or otherwise free).
    pub free: bool,
    pub total: i32,
    pub remains: i32,
    /// Unix time the next free search becomes available.
    pub wait_till: Option<i32>,
    /// Price of one paid search; we never pay it.
    pub stars: i64,
}

impl PostsFlood {
    fn from_raw(raw: tl::enums::SearchPostsFlood) -> Self {
        let tl::enums::SearchPostsFlood::Flood(f) = raw;
        Self { free: f.query_is_free, total: f.total_daily, remains: f.remains, wait_till: f.wait_till, stars: f.stars_amount }
    }

    /// The free quota is spent: searching now would cost Stars.
    pub fn blocked(&self) -> bool {
        !self.free && self.remains <= 0
    }
}

impl Worker {
    pub(super) async fn search_tabs_cmd(&mut self, cmd: Cmd) -> Result<(), String> {
        let client = self.client()?;
        let err = |e| lf!("Пошук: {e}", "Search: {e}");
        match cmd {
            Cmd::SearchChannels(query) => {
                let req = tl::functions::contacts::Search { broadcasts: true, bots: false, q: query.clone(), limit: TAB_LIMIT };
                let tl::enums::contacts::Found::Found(res) = client.invoke(&req).await.map_err(err)?;
                let wanted: Vec<PeerId> = res.my_results.iter().chain(&res.results).map(PeerId::from).collect();
                let mut chats = Vec::new();
                for raw in res.chats {
                    let peer = Peer::from_raw(&client, raw);
                    // `Peer::Channel` is a broadcast channel; groups belong to the Chats tab.
                    let Peer::Channel(c) = &peer else { continue };
                    let members = c.raw.participants_count;
                    if !wanted.contains(&peer.id()) || !self.remember(&peer).await {
                        continue;
                    }
                    let user = peer.username().map(|u| format!("@{u}")).unwrap_or_default();
                    let label = match members {
                        Some(n) => lf!("{user} · підписників: {n}", "{user} · {n} subscribers"),
                        None => user,
                    };
                    chats.push(found_chat(&peer, label.trim_start_matches(" · ").to_owned()));
                }
                self.out.send(Ev::ChannelResults { query, chats });
            }
            Cmd::SearchApps(query) => {
                let q = query.trim().to_lowercase();
                let tl::enums::bots::PopularAppBots::Bots(popular) = client.invoke(&tl::functions::bots::GetPopularAppBots { offset: String::new(), limit: 100 }).await.map_err(err)?;
                let mut users = popular.users;
                // Popular apps are few: by name, also bots in general.
                if !q.is_empty() {
                    let req = tl::functions::contacts::Search { broadcasts: false, bots: true, q: query.clone(), limit: TAB_LIMIT };
                    if let Ok(tl::enums::contacts::Found::Found(res)) = client.invoke(&req).await {
                        users.extend(res.users);
                    }
                }
                let mut bots: Vec<ChatInfo> = Vec::new();
                for raw in users {
                    let monthly = match &raw {
                        tl::enums::User::User(u) => u.bot_active_users,
                        _ => None,
                    };
                    let peer = Peer::User(User::from_raw(&client, raw));
                    let Peer::User(u) = &peer else { continue };
                    let name = format!("{} {}", peer_title(&peer), u.username().unwrap_or_default()).to_lowercase();
                    if !u.is_bot() || !name.contains(&q) || bots.iter().any(|b| b.id == peer.id()) || !self.remember(&peer).await {
                        continue;
                    }
                    let label = match (monthly, u.username()) {
                        (Some(n), _) => lf!("{n} користувачів на місяць", "{n} monthly users"),
                        (None, Some(name)) => format!("@{name}"),
                        (None, None) => String::new(),
                    };
                    bots.push(found_chat(&peer, label));
                }
                self.out.send(Ev::AppResults { query, bots });
            }
            Cmd::CheckPostsFlood(query) => {
                let req = tl::functions::channels::CheckSearchPostsFlood { query: Some(query.clone()) };
                let flood = PostsFlood::from_raw(client.invoke(&req).await.map_err(err)?);
                self.out.send(Ev::PostsFlood { query, flood });
            }
            Cmd::SearchPosts(query) => {
                let check = tl::functions::channels::CheckSearchPostsFlood { query: Some(query.clone()) };
                let flood = client.invoke(&check).await.ok().map(PostsFlood::from_raw);
                // Past the free quota the server asks for Stars: show the limit instead of paying.
                if flood.is_some_and(|f| f.blocked()) {
                    self.out.send(Ev::PostResults { query, hits: Vec::new(), flood, error: None });
                    return Ok(());
                }
                let (hashtag, text) = match query.strip_prefix('#') {
                    Some(tag) => (Some(tag.to_owned()), None),
                    None => (None, Some(query.clone())),
                };
                let req = tl::functions::channels::SearchPosts {
                    hashtag,
                    query: text,
                    offset_rate: 0,
                    offset_peer: tl::enums::InputPeer::Empty,
                    offset_id: 0,
                    limit: TAB_LIMIT,
                    allow_paid_stars: None,
                };
                let (raw, chats, after) = match client.invoke(&req).await {
                    Ok(tl::enums::messages::Messages::Messages(m)) => (m.messages, m.chats, None),
                    Ok(tl::enums::messages::Messages::Slice(m)) => (m.messages, m.chats, m.search_flood.map(PostsFlood::from_raw)),
                    Ok(tl::enums::messages::Messages::ChannelMessages(m)) => (m.messages, m.chats, None),
                    Ok(tl::enums::messages::Messages::NotModified(_)) => (Vec::new(), Vec::new(), None),
                    Err(e) => {
                        self.out.send(Ev::PostResults { query, hits: Vec::new(), flood, error: Some(e.to_string()) });
                        return Ok(());
                    }
                };
                let mut titles = HashMap::new();
                for chat in chats {
                    let p = Peer::from_raw(&client, chat);
                    if self.remember(&p).await {
                        titles.insert(p.id(), (peer_title(&p), peer_kind(&p)));
                    }
                }
                // Raw posts lack grammers' peer map: fetch each channel's posts by id, keeping the order.
                let order: Vec<(PeerId, i32)> = raw
                    .iter()
                    .filter_map(|m| match m {
                        tl::enums::Message::Message(x) => Some((PeerId::from(&x.peer_id), x.id)),
                        _ => None,
                    })
                    .collect();
                let mut by_channel: Vec<(PeerId, PeerRef, Vec<i32>)> = Vec::new();
                for (peer, id) in &order {
                    match by_channel.iter_mut().find(|c| c.0 == *peer) {
                        Some(c) => c.2.push(*id),
                        None => {
                            if let Ok(r) = self.peer_ref(*peer) {
                                by_channel.push((*peer, r, vec![*id]));
                            }
                        }
                    }
                }
                let (out, names) = (self.out.clone(), self.names.clone());
                let flood = after.or(flood);
                tokio::spawn(async move {
                    let mut found = Vec::new();
                    for (peer, r, ids) in by_channel {
                        let Ok(msgs) = client.get_messages_by_id(r, &ids).await else { continue };
                        found.extend(msgs.into_iter().flatten().map(|m| (peer, convert(&m, &names))));
                    }
                    let hits = order
                        .iter()
                        .filter_map(|(peer, id)| {
                            let (_, msg) = found.iter().find(|(p, m)| p == peer && m.id == *id)?;
                            let (chat, kind) = titles.get(peer).cloned().unwrap_or((String::new(), ChatKind::Channel));
                            Some(SearchHit { chat, kind, msg: Msg { peer: *peer, ..msg.clone() } })
                        })
                        .collect();
                    out.send(Ev::PostResults { query, hits, flood, error: None });
                });
            }
            _ => {}
        }
        Ok(())
    }
}
