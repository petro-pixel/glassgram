//! Chat folders: saving, deleting and reordering (messages.updateDialogFilter).

use super::*;

impl Worker {
    pub(super) async fn folders_cmd(&mut self, cmd: Cmd) -> Result<(), String> {
        match cmd {
            Cmd::SaveFolder(f) => {
                let client = self.client()?;
                let peers = |ids: &mut dyn Iterator<Item = &PeerId>| ids.filter_map(|p| self.peer_ref(*p).ok()).map(tl::enums::InputPeer::from).collect::<Vec<_>>();
                let filter = tl::types::DialogFilter {
                    contacts: f.contacts,
                    non_contacts: f.non_contacts,
                    groups: f.groups,
                    broadcasts: f.broadcasts,
                    bots: f.bots,
                    exclude_muted: f.exclude_muted,
                    exclude_read: f.exclude_read,
                    exclude_archived: f.exclude_archived,
                    title_noanimate: false,
                    id: f.id,
                    title: tl::types::TextWithEntities { text: f.title.clone(), entities: Vec::new() }.into(),
                    emoticon: (!f.emoticon.is_empty()).then(|| f.emoticon.clone()),
                    color: None,
                    pinned_peers: peers(&mut f.pinned.iter().filter(|p| f.include.contains(p))),
                    include_peers: peers(&mut f.include.iter().filter(|p| !f.pinned.contains(p))),
                    exclude_peers: peers(&mut f.exclude.iter()),
                };
                client
                    .invoke(&tl::functions::messages::UpdateDialogFilter { id: f.id, filter: Some(filter.into()) })
                    .await
                    .map_err(|e| lf!("Папку не збережено: {e}", "Folder not saved: {e}"))?;
                self.load_folders().await
            }
            Cmd::DeleteFolder(id) => {
                let client = self.client()?;
                client
                    .invoke(&tl::functions::messages::UpdateDialogFilter { id, filter: None })
                    .await
                    .map_err(|e| lf!("Папку не видалено: {e}", "Folder not deleted: {e}"))?;
                self.load_folders().await
            }
            Cmd::ReorderFolders(order) => {
                let client = self.client()?;
                if let Err(e) = client.invoke(&tl::functions::messages::UpdateDialogFiltersOrder { order }).await {
                    let _ = self.load_folders().await;
                    return Err(lf!("Не вдалося змінити порядок папок: {e}", "Could not reorder folders: {e}"));
                }
                Ok(())
            }
            _ => Ok(()),
        }
    }
}
