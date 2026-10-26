//! Chat list actions: mute, archive, pin and reorder, read / unread marks, drafts, joining.

use super::*;

impl Worker {
    pub(super) async fn dialogs_cmd(&mut self, cmd: Cmd) -> Result<(), String> {
        match cmd {
            Cmd::Join(peer) => {
                let client = self.client()?;
                let r = self.peer_ref(peer)?;
                client
                    .invoke(&tl::functions::channels::JoinChannel { channel: r.into() })
                    .await
                    .map_err(|e| lf!("Не вдалося приєднатися: {e}", "Could not join: {e}"))?;
                self.out.send(Ev::Joined(peer));
                self.load_dialogs().await
            }
            Cmd::SetPinned { peer, pinned } => {
                let r = self.peer_ref(peer)?;
                let req = tl::functions::messages::ToggleDialogPin { pinned, peer: dialog_peer(r) };
                self.chat_action(&req, l!("Не вдалося закріпити", "Could not pin")).await
            }
            Cmd::SetMuted { peer, muted } => {
                let r = self.peer_ref(peer)?;
                let req = tl::functions::account::UpdateNotifySettings {
                    peer: tl::types::InputNotifyPeer { peer: r.into() }.into(),
                    settings: tl::types::InputPeerNotifySettings {
                        show_previews: None,
                        silent: None,
                        mute_until: Some(if muted { i32::MAX } else { 0 }),
                        sound: None,
                        stories_muted: None,
                        stories_hide_sender: None,
                        stories_sound: None,
                    }
                    .into(),
                };
                self.chat_action(&req, l!("Не вдалося змінити сповіщення", "Could not change notifications")).await
            }
            Cmd::SetArchived { peer, archived } => {
                let r = self.peer_ref(peer)?;
                let req = tl::functions::folders::EditPeerFolders {
                    folder_peers: vec![tl::types::InputFolderPeer { peer: r.into(), folder_id: archived as i32 }.into()],
                };
                self.chat_action(&req, l!("Не вдалося перемістити в архів", "Could not move to archive")).await
            }
            Cmd::MarkRead(peer) => {
                let client = self.client()?;
                let r = self.peer_ref(peer)?;
                let _ = client.mark_as_read(r).await;
                let req = tl::functions::messages::MarkDialogUnread { unread: false, parent_peer: None, peer: dialog_peer(r) };
                self.chat_action(&req, l!("Не вдалося позначити прочитаним", "Could not mark as read")).await
            }
            Cmd::MarkUnread(peer) => {
                let r = self.peer_ref(peer)?;
                let req = tl::functions::messages::MarkDialogUnread { unread: true, parent_peer: None, peer: dialog_peer(r) };
                self.chat_action(&req, l!("Не вдалося позначити непрочитаним", "Could not mark as unread")).await
            }
            Cmd::SaveDraft { peer, text } => {
                let client = self.client()?;
                let r = self.peer_ref(peer)?;
                let (message, entities) = format::parse(text.trim());
                let req = tl::functions::messages::SaveDraft {
                    no_webpage: false,
                    invert_media: false,
                    reply_to: None,
                    peer: r.into(),
                    message,
                    entities: (!entities.is_empty()).then_some(entities),
                    media: None,
                    effect: None,
                    suggested_post: None,
                    rich_message: None,
                };
                client.invoke(&req).await.map(drop).map_err(|e| lf!("Чернетка: {e}", "Draft: {e}"))
            }
            Cmd::ReorderPinned { archived, order } => {
                let order = order.into_iter().map(|p| self.peer_ref(p).map(dialog_peer)).collect::<Result<Vec<_>, _>>()?;
                // No `force`: chats the UI does not know about stay pinned.
                let req = tl::functions::messages::ReorderPinnedDialogs { force: false, folder_id: archived as i32, order };
                self.chat_action(&req, l!("Не вдалося змінити порядок", "Could not reorder")).await
            }
            _ => Ok(()),
        }
    }
}
