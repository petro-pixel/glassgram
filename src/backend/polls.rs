//! Creating polls and quizzes (`messages.sendMedia` with `inputMediaPoll`), as in Telegram Desktop's
//! "New poll" box.

use grammers_client::session::types::PeerId;
use grammers_client::tl;

use super::{Ev, ReplyTo, SendOpts, Worker};
use crate::format;
use crate::i18n::lf;

/// A poll from the "New poll" box.
#[derive(Debug, Clone, Default)]
pub struct NewPoll {
    pub question: String,
    pub options: Vec<String>,
    /// Voters are hidden (always so in channels).
    pub anonymous: bool,
    pub multiple: bool,
    /// Quiz: the correct option and the explanation shown after answering.
    pub quiz: Option<usize>,
    pub solution: String,
}

fn text(s: &str) -> tl::enums::TextWithEntities {
    tl::types::TextWithEntities { text: s.to_owned(), entities: Vec::new() }.into()
}

/// The id of the message the server created, from the reply to `sendMedia`.
pub(super) fn new_message_id(u: &tl::enums::Updates) -> Option<i32> {
    let list = match u {
        tl::enums::Updates::Updates(u) => &u.updates,
        tl::enums::Updates::Combined(u) => &u.updates,
        _ => return None,
    };
    list.iter().find_map(|u| match u {
        tl::enums::Update::NewMessage(m) => Some(m.message.id()),
        tl::enums::Update::NewChannelMessage(m) => Some(m.message.id()),
        tl::enums::Update::MessageId(m) => Some(m.id),
        _ => None,
    })
}

impl Worker {
    pub(super) async fn send_poll(&self, peer: PeerId, poll: NewPoll, reply_to: Option<ReplyTo>) -> Result<(), String> {
        let client = self.client()?;
        let r = self.peer_ref(peer)?;
        let quiz = poll.quiz.is_some();
        let (solution, solution_entities) = if quiz && !poll.solution.trim().is_empty() {
            let (t, e) = format::parse(poll.solution.trim());
            (Some(t), Some(e))
        } else {
            (None, None)
        };
        let media = tl::types::InputMediaPoll {
            poll: tl::types::Poll {
                id: 0,
                closed: false,
                public_voters: !poll.anonymous,
                multiple_choice: poll.multiple && !quiz,
                quiz,
                open_answers: false,
                revoting_disabled: false,
                shuffle_answers: false,
                hide_results_until_close: false,
                creator: false,
                subscribers_only: false,
                question: text(poll.question.trim()),
                answers: poll.options.iter().map(|o| tl::types::InputPollAnswer { text: text(o.trim()), media: None }.into()).collect(),
                close_period: None,
                close_date: None,
                countries_iso2: None,
                hash: 0,
            }
            .into(),
            correct_answers: poll.quiz.map(|i| vec![i as i32]),
            attached_media: None,
            solution,
            solution_entities,
            solution_media: None,
        };
        let local = ReplyTo::local(&reply_to);
        let reply_to = reply_to.as_ref().map(|x| self.input_reply(x)).transpose()?;
        let req = tl::functions::messages::SendMedia {
            silent: false,
            background: false,
            clear_draft: false,
            noforwards: false,
            update_stickersets_order: false,
            invert_media: false,
            allow_paid_floodskip: false,
            peer: r.into(),
            reply_to,
            media: media.into(),
            message: String::new(),
            random_id: jiff::Timestamp::now().as_nanosecond() as i64,
            reply_markup: None,
            entities: None,
            schedule_date: None,
            schedule_repeat_period: None,
            send_as: None,
            quick_reply_shortcut: None,
            effect: None,
            allow_paid_stars: None,
            suggested_post: None,
        };
        let updates = client.invoke(&req).await.map_err(|e| lf!("Опитування не надіслано: {e}", "Poll not sent: {e}"))?;
        // The reply carries only the new id: fetch the message to show it like any sent one.
        if let Some(id) = new_message_id(&updates)
            && let Ok(msgs) = client.get_messages_by_id(r, &[id]).await
            && let Some(Some(m)) = msgs.into_iter().next()
        {
            self.sent().emit(peer, &m, local, SendOpts::default());
        } else {
            self.out.send(Ev::Notice(lf!("Опитування надіслано", "Poll sent")));
        }
        Ok(())
    }
}
