//! Demo folders and shared folder links.

use super::*;

pub(super) fn handle(app: &mut App, cmd: Cmd) {
    match cmd {
        Cmd::SaveFolder(f) => {
            let mut list = app.demo_folders();
            match list.iter_mut().find(|x| x.id == f.id) {
                Some(x) => *x = f,
                None => list.push(f),
            }
            app.push_event(Ev::Folders(list));
        }
        Cmd::SetFolderTags(on) => app.push_event(Ev::FolderTags(on)),
        Cmd::LoadSuggestedFolders => {
            let suggested = vec![
                (Folder { title: "New".into(), emoticon: "🆕".into(), contacts: true, non_contacts: true, groups: true, broadcasts: true, bots: true, exclude_read: true, exclude_muted: true, ..Default::default() }, "Unread chats with notifications".into()),
                (Folder { title: "Bots".into(), emoticon: "🤖".into(), bots: true, ..Default::default() }, "Chats with bots".into()),
            ];
            let have: Vec<String> = app.demo_folders().into_iter().map(|f| f.title).collect();
            app.push_event(Ev::SuggestedFolders(suggested.into_iter().filter(|(f, _)| !have.contains(&f.title)).collect()));
        }
        Cmd::DeleteFolder(id) => {
            let list = app.demo_folders().into_iter().filter(|f| f.id != id).collect();
            app.push_event(Ev::Folders(list));
        }
        Cmd::ReorderFolders(order) => {
            let mut list = app.demo_folders();
            list.sort_by_key(|f| order.iter().position(|id| *id == f.id));
            app.push_event(Ev::Folders(list));
        }
        Cmd::CheckChatlist(_) | Cmd::CheckChatlistUpdates(_) | Cmd::AskLeaveChatlist(_) => {
            use crate::backend::{Chatlist, ChatlistAction};
            let action = match cmd {
                Cmd::CheckChatlist(slug) => ChatlistAction::Join(slug),
                Cmd::CheckChatlistUpdates(id) => ChatlistAction::Update(id),
                Cmd::AskLeaveChatlist(id) => ChatlistAction::Leave(id),
                _ => return,
            };
            let chats = app.demo_chats().iter().filter(|c| matches!(c.kind, ChatKind::Group | ChatKind::Channel)).map(|c| (c.id, c.title.clone())).collect();
            app.push_event(Ev::Chatlist(Chatlist { action, title: "Rust Community".into(), emoticon: "🦀".into(), chats, already: 0 }));
        }
        Cmd::ApplyChatlist { .. } => app.push_event(Ev::Notice(crate::i18n::l!("Зміни збережено", "Changes saved").into())),
        _ => {}
    }
}
