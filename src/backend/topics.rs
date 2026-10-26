//! Forum topics: the list, a topic's messages, creating, editing and reading topics.

use super::*;

impl Worker {
    pub(super) async fn topics_cmd(&mut self, cmd: Cmd) -> Result<(), String> {
        match cmd {
            Cmd::LoadTopics(peer) => {
                let client = self.client()?;
                let r = self.peer_ref(peer)?;
                let req = tl::functions::messages::GetForumTopics { peer: r.into(), q: None, offset_date: 0, offset_id: 0, offset_topic: 0, limit: 100 };
                let tl::enums::messages::ForumTopics::Topics(res) = client.invoke(&req).await.map_err(|e| lf!("Теми: {e}", "Topics: {e}"))?;
                // Last messages of topics come with the list: take previews from them.
                let last: HashMap<i32, &tl::types::Message> = res
                    .messages
                    .iter()
                    .filter_map(|m| match m {
                        tl::enums::Message::Message(x) => Some((x.id, x)),
                        _ => None,
                    })
                    .collect();
                let mut topics: Vec<Topic> = res
                    .topics
                    .iter()
                    .filter_map(|t| match t {
                        tl::enums::ForumTopic::Topic(t) if !t.hidden || t.id == GENERAL_TOPIC => {
                            let m = last.get(&t.top_message);
                            let ts = m.map_or(t.date, |m| m.date) as i64;
                            let when = jiff::Timestamp::from_second(ts).unwrap_or_default();
                            Some(Topic {
                                id: t.id,
                                title: t.title.clone(),
                                color: t.icon_color as u32,
                                unread: t.unread_count,
                                pinned: t.pinned,
                                closed: t.closed,
                                last_text: m.map(|m| m.message.replace('\n', " ")).filter(|s| !s.is_empty()).unwrap_or_else(|| l!("Вкладення", "Attachment").into()),
                                last_time: short_time(when),
                                last_ts: ts,
                            })
                        }
                        _ => None,
                    })
                    .collect();
                topics.sort_by_key(|t| (!t.pinned, -t.last_ts));
                self.out.send(Ev::Topics { peer, topics });
                Ok(())
            }
            Cmd::OpenTopic { peer, topic, before } => {
                let client = self.client()?;
                let r = self.peer_ref(peer)?;
                let req = tl::functions::messages::GetReplies {
                    peer: r.into(),
                    msg_id: topic,
                    offset_id: before.unwrap_or(0),
                    offset_date: 0,
                    add_offset: 0,
                    limit: PAGE as i32,
                    max_id: 0,
                    min_id: 0,
                    hash: 0,
                };
                let raw = client.invoke(&req).await.map_err(|e| lf!("Тема: {e}", "Topic: {e}"))?;
                let list = match raw {
                    tl::enums::messages::Messages::Messages(m) => m.messages,
                    tl::enums::messages::Messages::Slice(m) => m.messages,
                    tl::enums::messages::Messages::ChannelMessages(m) => m.messages,
                    tl::enums::messages::Messages::NotModified(_) => Vec::new(),
                };
                let ids: Vec<i32> = list.iter().map(|m| m.id()).collect();
                // Raw messages can't be converted without the members map: fetch them with a regular request.
                let mut msgs: Vec<Msg> = if ids.is_empty() {
                    Vec::new()
                } else {
                    client
                        .get_messages_by_id(r, &ids)
                        .await
                        .map_err(|e| lf!("Тема: {e}", "Topic: {e}"))?
                        .into_iter()
                        .flatten()
                        .map(|m| self.convert_loaded(peer, &m))
                        .collect()
                };
                msgs.sort_by_key(|m| m.id);
                self.out.send(Ev::TopicHistory { peer, topic, msgs, older: before.is_some() });
                Ok(())
            }
            Cmd::CreateTopic { peer, title, color } => {
                let client = self.client()?;
                let r = self.peer_ref(peer)?;
                let req = tl::functions::messages::CreateForumTopic {
                    title_missing: false,
                    peer: r.into(),
                    title,
                    icon_color: Some(color as i32),
                    icon_emoji_id: None,
                    random_id: jiff::Timestamp::now().as_nanosecond() as i64,
                    send_as: None,
                };
                client.invoke(&req).await.map_err(|e| lf!("Тему не створено: {e}", "Topic not created: {e}"))?;
                let _ = self.back.send(Cmd::LoadTopics(peer));
                Ok(())
            }
            Cmd::EditTopic { peer, id, title, closed } => {
                let client = self.client()?;
                let r = self.peer_ref(peer)?;
                let req = tl::functions::messages::EditForumTopic {
                    peer: r.into(),
                    topic_id: id,
                    title,
                    icon_emoji_id: None,
                    closed: closed.map(Into::into),
                    hidden: None,
                };
                client.invoke(&req).await.map_err(|e| lf!("Тему не змінено: {e}", "Topic not changed: {e}"))?;
                let _ = self.back.send(Cmd::LoadTopics(peer));
                Ok(())
            }
            Cmd::ReadTopic { peer, topic, max_id } => {
                let client = self.client()?;
                let r = self.peer_ref(peer)?;
                client
                    .invoke(&tl::functions::messages::ReadDiscussion { peer: r.into(), msg_id: topic, read_max_id: max_id })
                    .await
                    .map(drop)
                    .map_err(|e| lf!("Тема: {e}", "Topic: {e}"))
            }
            _ => Ok(()),
        }
    }
}
