//! Bot buttons under a message, as Telegram Desktop handles them: callbacks, links, "Log in",
//! text buttons, copying, mini apps and games.

use super::*;

impl App {
    pub(super) fn press_bot_button(&mut self, peer: PeerId, id: i32, row: usize, col: usize) {
        let Some(b) = self.find(peer, id).and_then(|m| m.buttons.get(row)?.get(col)).cloned() else { return };
        match b.action {
            ButtonAction::Callback(data) => self.send(Cmd::PressButton { peer, id, data }),
            ButtonAction::Url(url) => self.open_link(&url),
            ButtonAction::UrlAuth { url, button_id } => self.url_auth_button(peer, id, button_id, url),
            ButtonAction::Text => {
                let reply_to = self.forum.open.filter(|t| *t != GENERAL_TOPIC).map(ReplyTo::to);
                self.send(Cmd::Send { peer, text: b.text, reply_to, opts: SendOpts::default(), send_as: self.send_as_for(peer) });
            }
            ButtonAction::Copy(text) => {
                self.ctx.copy_text(text);
                self.toast = Some((l!("Скопійовано", "Copied").into(), 0.0, false));
            }
            ButtonAction::WebApp { url, simple, bot } => {
                // Inline buttons in groups belong to the bot that sent the message.
                let bot = PeerId::user(bot).filter(|_| bot != 0).unwrap_or(peer);
                let kind = if simple { backend::WebAppKind::Simple(url) } else { backend::WebAppKind::Inline(url) };
                self.open_web_app(peer, bot, kind, b.text);
            }
            ButtonAction::Game => self.send(Cmd::PlayGame { peer, id }),
            ButtonAction::Other => self.toast = Some((l!("Цей тип кнопки ще не підтримується", "This button type is not supported yet").into(), 0.0, true)),
        }
    }
}
