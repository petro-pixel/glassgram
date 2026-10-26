//! Bot deep links `t.me/<bot>?start=<param>`: the bot chat opens with a Start button that hands the
//! parameter to the bot (`messages.startBot`), as in Telegram Desktop.

use grammers_client::peer::Peer;
use grammers_client::tl;

use super::{Cmd, Ev, Worker, found_chat};
use crate::i18n::lf;

impl Worker {
    pub(super) async fn bot_start_cmd(&mut self, cmd: Cmd) -> Result<(), String> {
        match cmd {
            Cmd::OpenBotStart { username, param } => {
                let peer = self.resolve_peer(&username).await?;
                self.out.send(Ev::ChatFound(found_chat(&peer, format!("@{username}"))));
                // A start link to a channel or a person just opens the chat.
                if matches!(&peer, Peer::User(u) if u.is_bot()) {
                    self.out.send(Ev::BotStart { peer: peer.id(), param });
                }
            }
            Cmd::StartBot { peer, param } => {
                let client = self.client()?;
                let r = self.peer_ref(peer)?;
                let req = tl::functions::messages::StartBot {
                    bot: r.into(),
                    peer: r.into(),
                    random_id: jiff::Timestamp::now().as_nanosecond() as i64,
                    start_param: param,
                };
                // The /start message itself comes back through updates.
                client.invoke(&req).await.map_err(|e| lf!("Бота не запущено: {e}", "Bot not started: {e}"))?;
            }
            _ => {}
        }
        Ok(())
    }
}
