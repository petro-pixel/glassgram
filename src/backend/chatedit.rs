//! Editing a group or channel, as in Telegram Desktop's "Edit" box: title, description, photo,
//! default permissions of members, slow mode, allowed reactions, hidden members and anti-spam.

use std::path::PathBuf;

use grammers_client::session::types::{PeerId, PeerKind};
use grammers_client::tl;

use super::{Cmd, Ev, Worker};
use crate::i18n::{l, lf};

/// Which reactions members may leave on messages.
#[derive(Debug, Clone, PartialEq, Default)]
pub enum ReactionMode {
    #[default]
    All,
    /// Only these emoji.
    Some(Vec<String>),
    Off,
}

impl ReactionMode {
    pub(super) fn from_raw(r: Option<&tl::enums::ChatReactions>) -> Self {
        match r {
            Some(tl::enums::ChatReactions::All(_)) => Self::All,
            Some(tl::enums::ChatReactions::Some(s)) => Self::Some(
                s.reactions
                    .iter()
                    .filter_map(|r| match r {
                        tl::enums::Reaction::Emoji(e) => Some(e.emoticon.clone()),
                        _ => None,
                    })
                    .collect(),
            ),
            // The server leaves the field out when reactions are off, as Telegram Desktop reads it.
            _ => Self::Off,
        }
    }

    fn raw(&self) -> tl::enums::ChatReactions {
        match self {
            Self::All => tl::types::ChatReactionsAll { allow_custom: false }.into(),
            Self::Off => tl::enums::ChatReactions::None,
            Self::Some(list) => tl::types::ChatReactionsSome {
                reactions: list.iter().map(|e| tl::types::ReactionEmoji { emoticon: e.clone() }.into()).collect(),
            }
            .into(),
        }
    }
}

/// What members may do by default (`true` — allowed); the inverse of `chatBannedRights`.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ChatRights {
    pub send_messages: bool,
    pub send_media: bool,
    /// Stickers and GIFs (one switch in Telegram Desktop).
    pub send_stickers: bool,
    pub send_polls: bool,
    pub embed_links: bool,
    pub invite_users: bool,
    pub pin_messages: bool,
    pub change_info: bool,
}

impl ChatRights {
    pub fn from_banned(b: Option<&tl::enums::ChatBannedRights>) -> Self {
        let Some(tl::enums::ChatBannedRights::Rights(b)) = b else {
            return Self { send_messages: true, send_media: true, send_stickers: true, send_polls: true, embed_links: true, invite_users: true, pin_messages: true, change_info: true };
        };
        Self {
            send_messages: !b.send_messages,
            send_media: !b.send_media,
            send_stickers: !b.send_stickers,
            send_polls: !b.send_polls,
            embed_links: !b.embed_links,
            invite_users: !b.invite_users,
            pin_messages: !b.pin_messages,
            change_info: !b.change_info,
        }
    }

    fn banned(self) -> tl::enums::ChatBannedRights {
        // Media covers every kind of file, as the media switch in the official clients does.
        let media = !self.send_media;
        tl::types::ChatBannedRights {
            view_messages: false,
            send_messages: !self.send_messages,
            send_media: media,
            send_stickers: !self.send_stickers,
            send_gifs: !self.send_stickers,
            send_games: !self.send_stickers,
            send_inline: !self.send_stickers,
            embed_links: !self.embed_links,
            send_polls: !self.send_polls,
            change_info: !self.change_info,
            invite_users: !self.invite_users,
            pin_messages: !self.pin_messages,
            manage_topics: !self.pin_messages,
            send_photos: media,
            send_videos: media,
            send_roundvideos: media,
            send_audios: media,
            send_voices: media,
            send_docs: media,
            send_plain: !self.send_messages,
            edit_rank: false,
            send_reactions: false,
            manage_linked_peers: false,
            until_date: 0,
        }
        .into()
    }
}

impl Worker {
    pub(super) async fn chat_edit_cmd(&mut self, cmd: Cmd) -> Result<(), String> {
        let client = self.client()?;
        let err = |e: grammers_client::InvocationError| lf!("Не вдалося змінити чат: {e}", "Could not edit the chat: {e}");
        match cmd {
            Cmd::EditChat { peer, title, about } => {
                let r = self.peer_ref(peer)?;
                if let Some(title) = title {
                    if peer.kind() == PeerKind::Channel {
                        client.invoke(&tl::functions::channels::EditTitle { channel: r.into(), title: title.clone() }).await.map_err(err)?;
                    } else {
                        client.invoke(&tl::functions::messages::EditChatTitle { chat_id: peer.bare_id_unchecked(), title: title.clone() }).await.map_err(err)?;
                    }
                    self.names.lock().unwrap().insert(peer, title.clone());
                    self.out.send(Ev::ChatTitle { peer, title });
                }
                if let Some(about) = about {
                    client.invoke(&tl::functions::messages::EditChatAbout { peer: r.into(), about }).await.map_err(err)?;
                }
                self.after_edit(peer);
            }
            Cmd::SetChatPhoto { peer, path } => self.set_chat_photo(peer, path)?,
            Cmd::SetChatRights { peer, rights } => {
                let r = self.peer_ref(peer)?;
                client.invoke(&tl::functions::messages::EditChatDefaultBannedRights { peer: r.into(), banned_rights: rights.banned() }).await.map_err(err)?;
                self.after_edit(peer);
            }
            Cmd::SetSlowMode { peer, seconds } => {
                let r = self.peer_ref(peer)?;
                client.invoke(&tl::functions::channels::ToggleSlowMode { channel: r.into(), seconds }).await.map_err(err)?;
                self.after_edit(peer);
            }
            Cmd::SetChatReactions { peer, mode } => {
                let r = self.peer_ref(peer)?;
                let req = tl::functions::messages::SetChatAvailableReactions { peer: r.into(), available_reactions: mode.raw(), reactions_limit: None, paid_enabled: None };
                client.invoke(&req).await.map_err(err)?;
                self.after_edit(peer);
            }
            Cmd::SetMembersHidden { peer, hidden } => {
                let r = self.peer_ref(peer)?;
                client.invoke(&tl::functions::channels::ToggleParticipantsHidden { channel: r.into(), enabled: hidden }).await.map_err(err)?;
                self.after_edit(peer);
            }
            Cmd::SetAntiSpam { peer, on } => {
                let r = self.peer_ref(peer)?;
                client.invoke(&tl::functions::channels::ToggleAntiSpam { channel: r.into(), enabled: on }).await.map_err(err)?;
                self.after_edit(peer);
            }
            _ => {}
        }
        Ok(())
    }

    /// The info panel reloads, so it shows what the server stored.
    fn after_edit(&self, peer: PeerId) {
        self.out.send(Ev::Notice(l!("Зміни збережено", "Changes saved").into()));
        let _ = self.back.send(Cmd::LoadChatInfo(peer));
    }

    /// Cropped to a square and uploaded in the background, like the profile photo.
    fn set_chat_photo(&self, peer: PeerId, path: PathBuf) -> Result<(), String> {
        let client = self.client()?;
        let r = self.peer_ref(peer)?;
        let (out, back) = (self.out.clone(), self.back.clone());
        tokio::spawn(async move {
            let file = match tokio::task::spawn_blocking(move || super::account::square_photo(&path)).await {
                Ok(Ok(f)) => f,
                Ok(Err(e)) => return out.send(Ev::Error(e)),
                Err(e) => return out.send(Ev::Error(e.to_string())),
            };
            let uploaded = client.upload_file(&file).await;
            let _ = tokio::fs::remove_file(&file).await;
            let up = match uploaded {
                Ok(up) => up,
                Err(e) => return out.send(Ev::Error(lf!("Фото: {e}", "Photo: {e}"))),
            };
            let photo: tl::enums::InputChatPhoto = tl::types::InputChatUploadedPhoto { file: Some(up.raw), video: None, video_start_ts: None, video_emoji_markup: None }.into();
            let res = match peer.kind() {
                PeerKind::Channel => client.invoke(&tl::functions::channels::EditPhoto { channel: r.into(), photo }).await,
                _ => client.invoke(&tl::functions::messages::EditChatPhoto { chat_id: peer.bare_id_unchecked(), photo }).await,
            };
            match res {
                Ok(_) => {
                    out.send(Ev::Notice(l!("Фото чату оновлено", "Chat photo updated").into()));
                    // A fresh dialog list brings the new avatar.
                    let _ = back.send(Cmd::ReloadDialogs);
                    let _ = back.send(Cmd::LoadChatInfo(peer));
                }
                Err(e) => out.send(Ev::Error(lf!("Фото: {e}", "Photo: {e}"))),
            }
        });
        Ok(())
    }
}
