//! Emoji games: sending 🎲 and friends as `inputMediaDice`; the server rolls the value.

use super::*;

impl Worker {
    pub(super) async fn dice_cmd(&mut self, cmd: Cmd) -> Result<(), String> {
        let Cmd::SendDice { peer, emoji, reply_to } = cmd else { return Ok(()) };
        let client = self.client()?;
        let r = self.peer_ref(peer)?;
        let media = tl::types::InputMediaDice { emoticon: emoji };
        let not_sent = |e: &dyn std::fmt::Display| lf!("Не надіслано: {e}", "Not sent: {e}");
        let m = match self.extended_reply(&reply_to)? {
            Some(x) => reply::send_one(&client, r, "", Some(media.into()), x, SendOpts::default()).await.map_err(|e| not_sent(&e))?,
            None => Some(client.send_message(r, InputMessage::new().media(media).reply_to(reply_to.as_ref().map(|x| x.id))).await.map_err(|e| not_sent(&e))?),
        };
        if let Some(m) = m {
            self.sent().emit(peer, &m, ReplyTo::local(&reply_to), SendOpts::default());
        }
        Ok(())
    }
}
