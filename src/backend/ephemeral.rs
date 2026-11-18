//! Ephemeral bot messages (2026): a bot's reply in a group that only we see
//! (`updateNewEphemeralMessage` and its edits and deletions). They have ids of their own and are
//! never cached; their callback buttons go through `ephemeral.getCallbackAnswer`.

use grammers_client::session::types::PeerId;
use grammers_client::tl;

use super::convert::markup_buttons;
use super::{Button, Cmd, Ev, Names, Worker};
use crate::format::{self, Span};
use crate::i18n::lf;

/// A message only we can see.
#[derive(Debug, Clone)]
pub struct EphemeralMsg {
    /// The chat it shows in.
    pub peer: PeerId,
    /// Its own id: not a message id of the chat.
    pub id: i32,
    /// The bot that sent it.
    pub from: String,
    pub text: String,
    pub spans: Vec<Span>,
    pub buttons: Vec<Vec<Button>>,
}

fn convert(m: &tl::enums::EphemeralMessage, names: &Names) -> EphemeralMsg {
    let tl::enums::EphemeralMessage::Message(m) = m;
    let from = PeerId::from(&m.from_id);
    let peer = m.peer_id.as_ref().map_or(from, PeerId::from);
    let text = m.message.clone();
    let spans = format::from_entities(&text, m.entities.as_deref().unwrap_or_default(), 0);
    EphemeralMsg {
        peer,
        id: m.id,
        from: names.lock().unwrap().get(&from).cloned().unwrap_or_default(),
        spans,
        text,
        buttons: markup_buttons(m.reply_markup.clone(), from.bare_id_unchecked()),
    }
}

/// The ephemeral updates, as events.
pub(super) fn update_event(u: &tl::enums::Update, names: &Names) -> Option<Ev> {
    Some(match u {
        tl::enums::Update::NewEphemeralMessage(n) => Ev::Ephemeral(convert(&n.message, names)),
        tl::enums::Update::EditEphemeralMessage(e) => Ev::Ephemeral(convert(&e.message, names)),
        tl::enums::Update::DeleteEphemeralMessages(d) => Ev::EphemeralGone { peer: PeerId::from(&d.peer), ids: d.ids.clone() },
        _ => return None,
    })
}

impl Worker {
    pub(super) async fn ephemeral_cmd(&mut self, cmd: Cmd) -> Result<(), String> {
        let Cmd::PressEphemeral { peer, id, data } = cmd else { return Ok(()) };
        let client = self.client()?;
        let req = tl::functions::ephemeral::GetCallbackAnswer { peer: self.peer_ref(peer)?.into(), id, data: Some(data) };
        let out = self.out.clone();
        tokio::spawn(async move {
            match client.invoke(&req).await {
                Ok(a) => super::inline::bot_answer(&out, a),
                Err(e) => out.send(Ev::Error(lf!("Кнопка: {e}", "Button: {e}"))),
            }
        });
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn group_reply_shows_in_the_group_with_the_bot_name() {
        let bot = PeerId::user_unchecked(77);
        let names: Names = std::sync::Arc::new(std::sync::Mutex::new([(bot, "Group Bot".to_owned())].into()));
        let msg = tl::types::EphemeralMessage {
            out: false,
            welcome_template: false,
            invert_media: false,
            noforwards: false,
            id: 5,
            from_id: tl::types::PeerUser { user_id: 77 }.into(),
            peer_id: Some(tl::types::PeerChannel { channel_id: 900 }.into()),
            receiver_id: 1,
            top_msg_id: None,
            date: 0,
            message: "only for you".into(),
            entities: None,
            media: None,
            reply_markup: None,
            reply_to: None,
            rich_message: None,
            chat_instance: None,
            anchor_msg_id: None,
        };
        let u = tl::enums::Update::NewEphemeralMessage(tl::types::UpdateNewEphemeralMessage { message: msg.into() });
        let Some(Ev::Ephemeral(m)) = update_event(&u, &names) else { panic!("no event") };
        assert_eq!(m.peer, PeerId::channel_unchecked(900));
        assert_eq!((m.id, m.from.as_str(), m.text.as_str()), (5, "Group Bot", "only for you"));
        let gone = tl::enums::Update::DeleteEphemeralMessages(tl::types::UpdateDeleteEphemeralMessages { peer: tl::types::PeerChannel { channel_id: 900 }.into(), ids: vec![5] });
        assert!(matches!(update_event(&gone, &names), Some(Ev::EphemeralGone { ids, .. }) if ids == [5]));
    }
}
