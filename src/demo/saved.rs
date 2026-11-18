//! Demo Saved Messages: saved dialogs, reaction tags and their histories.

use super::*;

pub(super) fn handle(app: &mut App, cmd: Cmd) {
    match cmd {
        Cmd::LoadSavedDialogs => {
            // Saved Messages (id 3) hold our own note, a channel post and a photo from Olena.
            let list = [3, 4, 1].into_iter().filter_map(|id| app.demo_chat(PeerId::user_unchecked(id)).cloned()).collect();
            app.push_event(Ev::SavedDialogs(list));
        }
        Cmd::LoadSavedTags => app.push_event(Ev::SavedTags(SAVED_TAGS.with_borrow(Clone::clone))),
        Cmd::RenameSavedTag { emoji, title } => {
            SAVED_TAGS.with_borrow_mut(|tags| tags.iter_mut().filter(|t| t.emoji == emoji).for_each(|t| t.title = title.clone()));
            app.push_event(Ev::SavedTags(SAVED_TAGS.with_borrow(Clone::clone)));
        }
        Cmd::LoadSavedTagged { saved, emoji } => {
            // 👍 marks the note and the channel post, ❤ the note only.
            let ids: &[i32] = if emoji == "👍" { &[1, 2] } else { &[1] };
            let msgs = app
                .demo_history()
                .find(|(p, _, _)| *p == saved)
                .map(|(_, _, m)| m.iter().filter(|m| ids.contains(&m.id)).cloned().collect::<Vec<_>>())
                .unwrap_or_default()
                .into_iter()
                .map(|mut m| {
                    m.reactions = SAVED_TAGS.with_borrow(|t| t.iter().filter(|t| t.emoji == "👍" || m.id == 1).map(|t| Reaction { emoji: t.emoji.clone(), count: 1, mine: true }).collect());
                    m
                })
                .collect();
            app.push_event(Ev::SavedTagged(msgs));
        }
        Cmd::LoadSavedHistory { saved, source, .. } => {
            let mut msgs: Vec<Msg> = app.demo_history().find(|(p, _, _)| *p == saved).map(|(_, _, m)| m.clone()).unwrap_or_default();
            // Message 1 is our note, 2 the channel post.
            for m in &mut msgs {
                m.saved_from = Some(PeerId::user_unchecked(if m.id == 1 { 3 } else { 4 }));
            }
            msgs.push(Msg {
                id: 900,
                out: true,
                forwarded: Some("Olena Koval".into()),
                saved_from: Some(PeerId::user_unchecked(1)),
                ..outgoing(saved, "Mockup of the new settings screen, have a look tonight".into(), None)
            });
            msgs.retain(|m| m.saved_from == Some(source));
            app.push_event(Ev::SavedHistory { source, msgs, older: false });
        }
        // Reasons first, then a comment for "Other", like the server.
        _ => {}
    }
}
