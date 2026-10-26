//! Inline bots and message buttons: queries, sending a result, callback buttons.

use super::*;

impl Worker {
    pub(super) async fn inline_cmd(&mut self, cmd: Cmd) -> Result<(), String> {
        match cmd {
            Cmd::PressButton { peer, id, data } => {
                let client = self.client()?;
                let r = self.peer_ref(peer)?;
                let out = self.out.clone();
                // A bot may take several seconds to answer: don't block other commands.
                tokio::spawn(async move {
                    let req = tl::functions::messages::GetBotCallbackAnswer { game: false, peer: r.into(), msg_id: id, data: Some(data), password: None };
                    match client.invoke(&req).await {
                        Ok(tl::enums::messages::BotCallbackAnswer::Answer(a)) => {
                            // A bot decides the link: only web links, never files or protocol handlers.
                            if let Some(url) = a.url
                                && !crate::platform::open_url(&url)
                            {
                                out.send(Ev::Error(lf!("Посилання не відкрито: {url}", "Link not opened: {url}")));
                            }
                            if let Some(text) = a.message.filter(|t| !t.is_empty()) {
                                out.send(Ev::BotAnswer { text, alert: a.alert });
                            }
                        }
                        Err(e) => out.send(Ev::Error(lf!("Кнопка: {e}", "Button: {e}"))),
                    }
                });
                Ok(())
            }
            Cmd::InlineQuery { peer, bot, query } => {
                let client = self.client()?;
                let r = self.peer_ref(peer)?;
                let bot_ref = self.resolve_bot(&client, &bot).await?;
                let out = self.out.clone();
                tokio::spawn(async move {
                    let req = tl::functions::messages::GetInlineBotResults {
                        bot: bot_ref.into(),
                        peer: r.into(),
                        geo_point: None,
                        query: query.clone(),
                        offset: String::new(),
                    };
                    match client.invoke(&req).await {
                        Ok(tl::enums::messages::BotResults::Results(res)) => {
                            let results = res.results.into_iter().map(inline_result).collect();
                            out.send(Ev::InlineResults { peer, bot, query, query_id: res.query_id, results });
                        }
                        // The bot may not have answered in time: not an error for the user, just empty.
                        Err(e) => {
                            log_error(&format!("inline @{bot}: {e}"));
                            out.send(Ev::InlineResults { peer, bot, query, query_id: 0, results: Vec::new() });
                        }
                    }
                });
                Ok(())
            }
            Cmd::SendInline { peer, query_id, result, reply_to } => {
                let client = self.client()?;
                let r = self.peer_ref(peer)?;
                let req = tl::functions::messages::SendInlineBotResult {
                    silent: false,
                    background: false,
                    clear_draft: true,
                    hide_via: false,
                    peer: r.into(),
                    reply_to: reply_to.map(|id| {
                        tl::types::InputReplyToMessage {
                            reply_to_msg_id: id,
                            top_msg_id: None,
                            reply_to_peer_id: None,
                            quote_text: None,
                            quote_entities: None,
                            quote_offset: None,
                            monoforum_peer_id: None,
                            todo_item_id: None,
                            poll_option: None,
                        }
                        .into()
                    }),
                    random_id: jiff::Timestamp::now().as_nanosecond() as i64,
                    query_id,
                    id: result,
                    schedule_date: None,
                    send_as: None,
                    quick_reply_shortcut: None,
                    allow_paid_stars: None,
                };
                client.invoke(&req).await.map_err(|e| lf!("Не надіслано: {e}", "Not sent: {e}"))?;
                self.emit_latest(peer, 1).await;
                Ok(())
            }
            _ => Ok(()),
        }
    }
}
