//! Notification defaults for private chats, groups and channels, and the chats that differ
//! from them (Settings → Notifications → "Notifications for chats" in Telegram Desktop).

use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use grammers_client::peer::{Peer, User};
use grammers_client::session::types::PeerId;
use grammers_client::{Client, tl};

use super::{ChatKind, Cmd, Ev, MUTE_FOREVER, NotifyState, Worker, peer_kind, peer_title};
use crate::i18n::lf;

/// Chat category with its own default notification settings.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum NotifyScope {
    Private,
    Groups,
    Channels,
}

impl NotifyScope {
    pub const ALL: [NotifyScope; 3] = [NotifyScope::Private, NotifyScope::Groups, NotifyScope::Channels];

    pub fn of(kind: ChatKind) -> Self {
        match kind {
            ChatKind::User | ChatKind::Bot | ChatKind::Saved => NotifyScope::Private,
            ChatKind::Group => NotifyScope::Groups,
            ChatKind::Channel => NotifyScope::Channels,
        }
    }

    fn input(self) -> tl::enums::InputNotifyPeer {
        match self {
            NotifyScope::Private => tl::types::InputNotifyUsers {}.into(),
            NotifyScope::Groups => tl::types::InputNotifyChats {}.into(),
            NotifyScope::Channels => tl::types::InputNotifyBroadcasts {}.into(),
        }
    }

    fn index(self) -> usize {
        self as usize
    }
}

/// A chat whose notification settings differ from its category.
#[derive(Debug, Clone)]
pub struct NotifyException {
    pub peer: PeerId,
    pub title: String,
    pub state: NotifyState,
}

/// Category defaults and the category of every chat seen, so a chat without settings of its
/// own can be resolved (the server only sends what was set for the chat itself).
#[derive(Default)]
pub(super) struct Defaults {
    mute: [i32; 3],
    loaded: bool,
    scope: HashMap<PeerId, NotifyScope>,
}

pub(super) type Notifs = Arc<Mutex<Defaults>>;

impl Defaults {
    pub(super) fn remember(&mut self, peer: &Peer) {
        self.scope.insert(peer.id(), NotifyScope::of(peer_kind(peer)));
    }

    /// The chat's own setting, or its category's if it has none.
    pub(super) fn resolve(&self, peer: PeerId, mut s: NotifyState) -> NotifyState {
        if s.inherit
            && let Some(scope) = self.scope.get(&peer)
        {
            s.mute_until = self.mute[scope.index()];
        }
        s
    }

    pub(super) fn set(&mut self, scope: NotifyScope, mute_until: i32) {
        self.mute[scope.index()] = mute_until;
    }
}

/// `updateNotifySettings`: one chat, or a whole category.
pub(super) fn update_event(u: &tl::types::UpdateNotifySettings, notifs: &Notifs) -> Option<Ev> {
    let state = NotifyState::from_raw(&u.notify_settings);
    let scope = match &u.peer {
        tl::enums::NotifyPeer::Peer(p) => {
            let peer = PeerId::from(&p.peer);
            return Some(Ev::Notify { peer, state: notifs.lock().unwrap().resolve(peer, state) });
        }
        tl::enums::NotifyPeer::NotifyUsers => NotifyScope::Private,
        tl::enums::NotifyPeer::NotifyChats => NotifyScope::Groups,
        tl::enums::NotifyPeer::NotifyBroadcasts => NotifyScope::Channels,
        _ => return None,
    };
    notifs.lock().unwrap().set(scope, state.mute_until);
    Some(Ev::NotifyDefault { scope, state })
}

async fn get(client: &Client, peer: tl::enums::InputNotifyPeer) -> Result<NotifyState, String> {
    let raw = client.invoke(&tl::functions::account::GetNotifySettings { peer }).await.map_err(|e| e.to_string())?;
    Ok(NotifyState::from_raw(&raw))
}

impl Worker {
    /// Category defaults, once per connection: dialogs need them to know which chats are muted.
    pub(super) async fn load_notify_defaults(&self) {
        let Ok(client) = self.client() else { return };
        if self.notifs.lock().unwrap().loaded {
            return;
        }
        let mut states = Vec::new();
        for scope in NotifyScope::ALL {
            match get(&client, scope.input()).await {
                Ok(s) => states.push((scope, s)),
                Err(e) => return super::log_error(&format!("notify defaults: {e}")),
            }
        }
        let mut d = self.notifs.lock().unwrap();
        for (scope, s) in &states {
            d.set(*scope, s.mute_until);
        }
        d.loaded = true;
        drop(d);
        for (scope, state) in states {
            self.out.send(Ev::NotifyDefault { scope, state });
        }
    }

    pub(super) async fn notify_cmd(&mut self, cmd: Cmd) -> Result<(), String> {
        let client = self.client()?;
        let out = self.out.clone();
        match cmd {
            Cmd::SetNotifyDefault { scope, muted } => {
                let req = tl::functions::account::UpdateNotifySettings {
                    peer: scope.input(),
                    settings: tl::types::InputPeerNotifySettings {
                        show_previews: None,
                        silent: None,
                        mute_until: Some(if muted { MUTE_FOREVER } else { 0 }),
                        sound: None,
                        stories_muted: None,
                        stories_hide_sender: None,
                        stories_sound: None,
                    }
                    .into(),
                };
                client.invoke(&req).await.map_err(|e| lf!("Не вдалося змінити сповіщення: {e}", "Could not change notifications: {e}"))?;
                let state = get(&client, scope.input()).await.unwrap_or(NotifyState { mute_until: if muted { MUTE_FOREVER } else { 0 }, ..Default::default() });
                self.notifs.lock().unwrap().set(scope, state.mute_until);
                out.send(Ev::NotifyDefault { scope, state });
            }
            Cmd::LoadNotifyExceptions(scope) => {
                let (refs, notifs) = (self.refs.clone(), self.notifs.clone());
                tokio::spawn(async move {
                    let req = tl::functions::account::GetNotifyExceptions { compare_sound: false, compare_stories: false, peer: Some(scope.input()) };
                    let (updates, users, chats) = match client.invoke(&req).await {
                        Ok(tl::enums::Updates::Updates(u)) => (u.updates, u.users, u.chats),
                        Ok(tl::enums::Updates::Combined(u)) => (u.updates, u.users, u.chats),
                        Ok(_) => (Vec::new(), Vec::new(), Vec::new()),
                        Err(e) => return out.send(Ev::Error(lf!("Винятки: {e}", "Exceptions: {e}"))),
                    };
                    let mut peers = HashMap::new();
                    for p in users.into_iter().map(|u| Peer::User(User::from_raw(&client, u))).chain(chats.into_iter().map(|c| Peer::from_raw(&client, c))) {
                        // Exceptions may include chats not in the loaded list: keep a reference to reset them.
                        if let Ok(Some(r)) = p.to_ref().await {
                            refs.lock().unwrap().entry(p.id()).or_insert(r);
                        }
                        notifs.lock().unwrap().remember(&p);
                        peers.insert(p.id(), peer_title(&p));
                    }
                    let list = updates
                        .iter()
                        .filter_map(|u| match u {
                            tl::enums::Update::NotifySettings(n) => match &n.peer {
                                tl::enums::NotifyPeer::Peer(p) => {
                                    let peer = PeerId::from(&p.peer);
                                    Some(NotifyException { peer, title: peers.get(&peer)?.clone(), state: NotifyState::from_raw(&n.notify_settings) })
                                }
                                _ => None,
                            },
                            _ => None,
                        })
                        .collect();
                    out.send(Ev::NotifyExceptions { scope, list });
                });
            }
            Cmd::ResetNotify(peer) => {
                let r = self.peer_ref(peer)?;
                // Empty settings drop the chat's own values: it follows its category again.
                let req = tl::functions::account::UpdateNotifySettings {
                    peer: tl::types::InputNotifyPeer { peer: r.into() }.into(),
                    settings: tl::types::InputPeerNotifySettings {
                        show_previews: None,
                        silent: None,
                        mute_until: None,
                        sound: None,
                        stories_muted: None,
                        stories_hide_sender: None,
                        stories_sound: None,
                    }
                    .into(),
                };
                client.invoke(&req).await.map_err(|e| lf!("Не вдалося змінити сповіщення: {e}", "Could not change notifications: {e}"))?;
                let state = get(&client, tl::types::InputNotifyPeer { peer: r.into() }.into()).await.unwrap_or_default();
                let state = self.notifs.lock().unwrap().resolve(peer, state);
                out.send(Ev::Notify { peer, state });
            }
            _ => {}
        }
        Ok(())
    }
}
