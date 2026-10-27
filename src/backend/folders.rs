//! Chat folders: saving, deleting and reordering (messages.updateDialogFilter), folder tags on
//! chat rows (`toggleDialogFilterTags`) and the folders Telegram suggests (`getSuggestedDialogFilters`).

use super::*;

/// Our folder from the server's; the "All chats" entry has nothing to show.
pub(super) fn folder_from(f: &tl::enums::DialogFilter) -> Option<Folder> {
    let ids = |v: &[tl::enums::InputPeer]| v.iter().map(|p| PeerRef::from(p.clone()).id).collect::<Vec<_>>();
    let title = |t: &tl::enums::TextWithEntities| {
        let tl::enums::TextWithEntities::Entities(t) = t;
        t.text.clone()
    };
    // -1 is how the server says "no colour".
    let color = |c: Option<i32>| c.filter(|c| (0..7).contains(c));
    match f {
        tl::enums::DialogFilter::Filter(f) => Some(Folder {
            id: f.id,
            title: title(&f.title),
            emoticon: f.emoticon.clone().unwrap_or_default(),
            include: [ids(&f.pinned_peers), ids(&f.include_peers)].concat(),
            pinned: ids(&f.pinned_peers),
            exclude: ids(&f.exclude_peers),
            contacts: f.contacts,
            non_contacts: f.non_contacts,
            groups: f.groups,
            broadcasts: f.broadcasts,
            bots: f.bots,
            exclude_muted: f.exclude_muted,
            exclude_read: f.exclude_read,
            exclude_archived: f.exclude_archived,
            shared: false,
            color: color(f.color),
        }),
        tl::enums::DialogFilter::Chatlist(f) => Some(Folder {
            id: f.id,
            title: title(&f.title),
            emoticon: f.emoticon.clone().unwrap_or_default(),
            include: [ids(&f.pinned_peers), ids(&f.include_peers)].concat(),
            pinned: ids(&f.pinned_peers),
            shared: true,
            color: color(f.color),
            ..Default::default()
        }),
        tl::enums::DialogFilter::Default => None,
    }
}

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
                    color: f.color,
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
            Cmd::SetFolderTags(on) => {
                let client = self.client()?;
                let res = client.invoke(&tl::functions::messages::ToggleDialogFilterTags { enabled: on }).await;
                // The server's state either way, so a refused switch snaps back.
                self.load_folders().await?;
                res.map(|_| ()).map_err(|e| {
                    if e.is("PREMIUM_ACCOUNT_REQUIRED") { l!("Теги папок — функція Telegram Premium", "Folder tags need Telegram Premium").to_owned() } else { e.to_string() }
                })
            }
            Cmd::LoadSuggestedFolders => {
                let client = self.client()?;
                let list = client.invoke(&tl::functions::messages::GetSuggestedDialogFilters {}).await.map_err(|e| lf!("Папки: {e}", "Folders: {e}"))?;
                let suggested = list
                    .into_iter()
                    .filter_map(|tl::enums::DialogFilterSuggested::Suggested(s)| Some((folder_from(&s.filter)?, s.description)))
                    .collect();
                self.out.send(Ev::SuggestedFolders(suggested));
                Ok(())
            }
            _ => Ok(()),
        }
    }
}
