//! Finding chats: global search by name and resolving usernames.

use super::*;

impl Worker {
    /// Finding chats: global search by name and resolving usernames.
    pub(super) async fn search_cmd(&mut self, cmd: Cmd) -> Result<(), String> {
        match cmd {
            Cmd::ResolveUsername(name) => {
                let name = name.trim_start_matches('@').to_owned();
                let peer = self.resolve_peer(&name).await?;
                self.out.send(Ev::ChatFound(found_chat(&peer, format!("@{name}"))));
                Ok(())
            }
            Cmd::SearchPublic(query) => {
                let client = self.client()?;
                let req = tl::functions::contacts::Search { broadcasts: false, bots: false, q: query.clone(), limit: 20 };
                let tl::enums::contacts::Found::Found(res) = client.invoke(&req).await.map_err(|e| lf!("Пошук: {e}", "Search: {e}"))?;
                // Global results only; our own chats are already in the list.
                let wanted: Vec<PeerId> = res.results.iter().map(PeerId::from).collect();
                let mut chats = Vec::new();
                for raw in res.chats {
                    let peer = Peer::from_raw(&client, raw);
                    if !wanted.contains(&peer.id()) {
                        continue;
                    }
                    let Ok(Some(r)) = peer.to_ref().await else { continue };
                    self.refs.lock().unwrap().insert(peer.id(), r);
                    self.names.lock().unwrap().insert(peer.id(), peer_title(&peer));
                    if let Some(loader) = &self.loader {
                        loader.avatar(&peer);
                    }
                    let members = match &peer {
                        Peer::Channel(c) => c.raw.participants_count,
                        Peer::Group(g) => match &g.raw {
                            tl::enums::Chat::Channel(c) => c.participants_count,
                            tl::enums::Chat::Chat(c) => Some(c.participants_count),
                            _ => None,
                        },
                        _ => None,
                    };
                    let user = peer.username().map(|u| format!("@{u}")).unwrap_or_default();
                    let label = match members {
                        Some(n) => lf!("{user} · учасників: {n}", "{user} · members: {n}"),
                        None => user,
                    };
                    chats.push(found_chat(&peer, label.trim_start_matches(" · ").to_owned()));
                }
                self.out.send(Ev::PublicResults { query, chats });
                Ok(())
            }
            _ => Ok(()),
        }
    }
}
