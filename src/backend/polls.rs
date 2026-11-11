//! Polls: creating polls and quizzes (`messages.sendMedia` with `inputMediaPoll`) as in Telegram
//! Desktop's "New poll" box, who voted for what (`messages.getPollVotes`) and adding answers to
//! polls open for them (`messages.addPollAnswer`).

use std::collections::HashMap;

use grammers_client::session::types::PeerId;
use grammers_client::tl;

use super::{Cmd, Ev, ReplyTo, SendOpts, Worker};
use crate::format;
use crate::i18n::{l, lf};

/// Voters fetched per page of "View results".
const VOTES_PAGE: i32 = 50;

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
    /// Closes by itself after this many seconds.
    pub close_period: Option<i32>,
    /// Voters may add their own answers.
    pub open_answers: bool,
    /// A vote can't be taken back.
    pub final_vote: bool,
    pub shuffle: bool,
    /// Results stay hidden until the poll closes (needs `close_period`).
    pub hide_results: bool,
    /// Only channel subscribers may vote.
    pub subscribers_only: bool,
}

/// One vote in "View results".
#[derive(Debug, Clone, PartialEq)]
pub struct PollVoter {
    pub peer: PeerId,
    pub name: String,
    /// The options picked (several in a multiple-answer poll).
    pub options: Vec<Vec<u8>>,
    /// When the vote was cast (unix time).
    pub ts: i64,
}

fn peer_names(users: &[tl::enums::User], chats: &[tl::enums::Chat]) -> HashMap<PeerId, String> {
    let users = users.iter().filter_map(|u| match u {
        tl::enums::User::User(u) => Some((PeerId::user(u.id)?, [u.first_name.clone(), u.last_name.clone()].into_iter().flatten().collect::<Vec<_>>().join(" "))),
        _ => None,
    });
    let chats = chats.iter().filter_map(|c| match c {
        tl::enums::Chat::Chat(c) => Some((PeerId::chat(c.id)?, c.title.clone())),
        tl::enums::Chat::Channel(c) => Some((PeerId::channel(c.id)?, c.title.clone())),
        _ => None,
    });
    users.chain(chats).collect()
}

fn voters(list: tl::enums::messages::VotesList) -> (Vec<PollVoter>, Option<String>) {
    let tl::enums::messages::VotesList::List(list) = list;
    let names = peer_names(&list.users, &list.chats);
    let votes = list
        .votes
        .iter()
        .map(|v| match v {
            tl::enums::MessagePeerVote::Vote(v) => (&v.peer, vec![v.option.clone()], v.date),
            tl::enums::MessagePeerVote::Multiple(v) => (&v.peer, v.options.clone(), v.date),
            // Voted with an answer they added themselves: the option isn't named.
            tl::enums::MessagePeerVote::InputOption(v) => (&v.peer, Vec::new(), v.date),
        })
        .map(|(p, options, date)| {
            let peer = PeerId::from(p);
            let name = names.get(&peer).cloned().filter(|n| !n.is_empty()).unwrap_or_else(|| l!("Видалений акаунт", "Deleted account").to_owned());
            PollVoter { peer, name, options, ts: date as i64 }
        })
        .collect();
    (votes, list.next_offset)
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
    pub(super) async fn polls_cmd(&mut self, cmd: Cmd) -> Result<(), String> {
        match cmd {
            Cmd::SendPoll { peer, poll, reply_to, send_as } => self.send_poll(peer, poll, reply_to, send_as).await,
            Cmd::PollVotes { peer, id, offset } => {
                let client = self.client()?;
                let r = self.peer_ref(peer)?;
                let out = self.out.clone();
                // Big polls page slowly: keep other commands moving.
                tokio::spawn(async move {
                    let req = tl::functions::messages::GetPollVotes { peer: r.into(), id, option: None, offset: offset.clone(), limit: VOTES_PAGE };
                    match client.invoke(&req).await {
                        Ok(list) => {
                            let (voters, next) = voters(list);
                            out.send(Ev::PollVotes { peer, id, more: offset.is_some(), voters, next });
                        }
                        Err(e) => out.send(Ev::Error(lf!("Результати опитування: {e}", "Poll results: {e}"))),
                    }
                });
                Ok(())
            }
            Cmd::AddPollAnswer { peer, id, text: answer } => {
                let client = self.client()?;
                let r = self.peer_ref(peer)?;
                let answer = tl::types::InputPollAnswer { text: text(answer.trim()), media: None }.into();
                let res = client.invoke(&tl::functions::messages::AddPollAnswer { peer: r.into(), msg_id: id, answer }).await;
                self.refresh_message(&client, peer, r, id).await;
                res.map(|_| ()).map_err(|e| lf!("Варіант не додано: {e}", "Answer not added: {e}"))
            }
            Cmd::StopPoll { peer, id } => {
                let client = self.client()?;
                let r = self.peer_ref(peer)?;
                let res = stop_poll(&client, r, id).await;
                self.refresh_message(&client, peer, r, id).await;
                res.map_err(|e| lf!("Опитування не зупинено: {e}", "Poll not stopped: {e}"))
            }
            _ => Ok(()),
        }
    }

    pub(super) async fn send_poll(&self, peer: PeerId, poll: NewPoll, reply_to: Option<ReplyTo>, send_as: Option<PeerId>) -> Result<(), String> {
        let client = self.client()?;
        let r = self.peer_ref(peer)?;
        let send_as = self.send_as_ref(send_as)?;
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
                open_answers: poll.open_answers && !quiz,
                revoting_disabled: poll.final_vote,
                shuffle_answers: poll.shuffle,
                hide_results_until_close: poll.hide_results && poll.close_period.is_some(),
                creator: false,
                subscribers_only: poll.subscribers_only,
                question: text(poll.question.trim()),
                answers: poll.options.iter().map(|o| tl::types::InputPollAnswer { text: text(o.trim()), media: None }.into()).collect(),
                close_period: poll.close_period,
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
            send_as,
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

/// The poll is sent back as it is with `closed` set, as Telegram Desktop does. A quiz must name its
/// correct answer again: its index among the answers, taken from the results we see as its author.
async fn stop_poll(client: &grammers_client::Client, r: grammers_client::session::types::PeerRef, id: i32) -> Result<(), String> {
    let msg = client.get_messages_by_id(r, &[id]).await.map_err(|e| e.to_string())?.into_iter().flatten().next();
    let Some(grammers_client::media::Media::Poll(p)) = msg.as_ref().and_then(|m| m.media()) else {
        return Err(l!("Опитування не знайдено", "Poll not found").to_owned());
    };
    let mut poll = p.raw.clone();
    poll.closed = true;
    let correct: Vec<i32> = p
        .iter_voters_summary()
        .map(|v| v.filter(|v| v.correct).filter_map(|v| poll.answers.iter().position(|a| matches!(a, tl::enums::PollAnswer::Answer(a) if a.option == v.option))).map(|i| i as i32).collect())
        .unwrap_or_default();
    let media = tl::types::InputMediaPoll {
        correct_answers: (poll.quiz && !correct.is_empty()).then_some(correct),
        poll: poll.into(),
        attached_media: None,
        solution: p.raw_results.solution.clone(),
        solution_entities: p.raw_results.solution_entities.clone(),
        solution_media: None,
    };
    client
        .invoke(&tl::functions::messages::EditMessage {
            no_webpage: false,
            invert_media: false,
            peer: r.into(),
            id,
            message: None,
            media: Some(media.into()),
            reply_markup: None,
            entities: None,
            schedule_date: None,
            schedule_repeat_period: None,
            quick_reply_shortcut_id: None,
            rich_message: None,
        })
        .await
        .map(|_| ())
        .map_err(|e| e.to_string())
}
