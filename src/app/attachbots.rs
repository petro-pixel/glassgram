//! Bots in the attachment menu: they are listed in the 📎 right-click menu for the chat types
//! they allow and open their mini app for the chat. `t.me/<bot>?startattach` adds one after a
//! confirmation; a right-click on a bot in the menu removes it. As in Telegram Desktop.

use super::*;
use crate::backend::{AttachBot, AttachPeer, WebAppKind};

/// A question about the menu, waiting for the user.
pub(super) enum AttachAsk {
    /// Add the bot of a `startattach` link, then open it in `peer`; `write`: the checkbox.
    Add { peer: PeerId, bot: AttachBot, start_param: String, write: bool },
    Remove(AttachBot),
}

#[derive(Default)]
pub(super) struct AttachMenu {
    /// The list from the server and its hash, so a repeated load costs nothing.
    bots: Vec<AttachBot>,
    hash: i64,
    /// Loaded (or loading) in this session.
    requested: bool,
    ask: Option<AttachAsk>,
    /// Added from a link: opened once the server confirms.
    pending: Option<(PeerId, AttachBot, String)>,
}

/// (bot username, start parameter) from `t.me/<bot>?startattach[=<param>]`.
pub(super) fn tme_start_attach(url: &str) -> Option<(String, String)> {
    let rest = url.trim_start_matches("https://").trim_start_matches("http://");
    let rest = rest.strip_prefix("t.me/").or_else(|| rest.strip_prefix("telegram.me/"))?;
    let (bot, query) = rest.split_once('?')?;
    let bot = bot.trim_end_matches('/');
    let param = query.split('&').find_map(|kv| match kv.split_once('=') {
        Some(("startattach", v)) => Some(v.to_owned()),
        None if kv == "startattach" => Some(String::new()),
        _ => None,
    })?;
    // The same limits as for `start`: up to 64 characters of A–Z, a–z, 0–9, _ and -.
    let ok = bot.len() >= 4 && bot.chars().all(|c| c.is_ascii_alphanumeric() || c == '_')
        && param.len() <= 64 && param.chars().all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-');
    ok.then(|| (bot.to_owned(), param))
}

/// Whether the bot works in this chat.
fn allows(bot: &AttachBot, peer: PeerId, kind: ChatKind) -> bool {
    bot.peer_types.iter().any(|t| match t {
        AttachPeer::SameBot => peer == bot.bot,
        AttachPeer::Bot => matches!(kind, ChatKind::Bot) && peer != bot.bot,
        // Saved Messages is a private chat with ourselves.
        AttachPeer::User => matches!(kind, ChatKind::User | ChatKind::Saved),
        AttachPeer::Group => matches!(kind, ChatKind::Group),
        AttachPeer::Channel => matches!(kind, ChatKind::Channel),
    })
}

impl App {
    /// The list is loaded once a chat is open; later loads only send the hash.
    pub(super) fn attach_bots_load(&mut self) {
        if !self.attach.requested {
            self.attach.requested = true;
            self.send(Cmd::LoadAttachBots { hash: self.attach.hash });
        }
    }

    /// For the demo: the menu as it is now.
    pub fn demo_attach_bots(&self) -> Vec<AttachBot> {
        self.attach.bots.clone()
    }

    pub(super) fn attach_bots_event(&mut self, ev: Ev) {
        match ev {
            Ev::AttachBots { hash, bots } => {
                self.attach.hash = hash;
                self.attach.bots = bots;
            }
            Ev::AttachBotFound { peer, bot, start_param } => {
                if bot.inactive {
                    let write = bot.write_access;
                    self.attach.ask = Some(AttachAsk::Add { peer, bot, start_param, write });
                } else {
                    self.open_attach_bot(peer, &bot, start_param);
                }
            }
            Ev::AttachBotToggled { bot, enabled } => {
                let pending = self.attach.pending.take_if(|(_, b, _)| b.bot == bot);
                match pending {
                    Some((peer, b, start_param)) if enabled => self.open_attach_bot(peer, &b, start_param),
                    _ if !enabled => self.toast = Some((l!("Бота прибрано з меню вкладень", "Bot removed from the attachment menu").into(), 0.0, false)),
                    _ => {}
                }
            }
            _ => {}
        }
    }

    /// The link's chat if the bot works there, otherwise the bot's own chat.
    fn open_attach_bot(&mut self, peer: PeerId, bot: &AttachBot, start_param: String) {
        let kind_of = |p: PeerId| self.chats.iter().find(|c| c.id == p).map(|c| c.kind);
        let target = [peer, bot.bot].into_iter().find(|p| allows(bot, *p, kind_of(*p).unwrap_or(ChatKind::Bot)));
        match target {
            Some(p) => self.open_web_app(p, bot.bot, WebAppKind::Attach { start_param }, String::new()),
            None => {
                let bot = &bot.name;
                self.toast = Some((lf!("{bot} не працює в цьому чаті", "{bot} doesn't work in this chat"), 0.0, true));
            }
        }
    }

    /// Bots of the 📎 right-click menu for this chat: a click opens, a right-click asks to remove.
    pub(super) fn attach_menu_items(&mut self, ui: &mut Ui, peer: PeerId) {
        let Some(kind) = self.chats.iter().find(|c| c.id == peer).map(|c| c.kind) else { return };
        let bots: Vec<AttachBot> = self.attach.bots.iter().filter(|b| b.in_menu && !b.inactive && allows(b, peer, kind)).cloned().collect();
        if bots.is_empty() {
            return;
        }
        ui.separator();
        for b in bots {
            // The bot's avatar stands in for its menu icon.
            let tex = self.textures.get(&ImgKey::Avatar(b.bot)).cloned();
            let (name, seed) = (b.name.clone(), b.bot.bare_id_unchecked());
            let icon = egui::Atom::paint(egui::Vec2::splat(18.0), move |ui, args| glass::avatar(ui.painter(), args.rect.center(), 9.0, &name, seed, None, tex.as_ref()));
            let r = ui.add(egui::Button::new((icon, b.name.as_str()))).on_hover_text(l!("Правий клік — прибрати з меню", "Right-click to remove from the menu"));
            if r.secondary_clicked() {
                self.attach.ask = Some(AttachAsk::Remove(b));
                ui.close();
            } else if r.clicked() {
                self.open_web_app(peer, b.bot, WebAppKind::Attach { start_param: String::new() }, String::new());
            }
        }
    }

    /// The add / remove confirmation.
    pub(super) fn attach_ask_view(&mut self, ui: &mut Ui) {
        let Some(ask) = &mut self.attach.ask else { return };
        let answer = match ask {
            AttachAsk::Add { bot: b, write, .. } => {
                let bot = &b.name;
                let title = lf!("Додати {bot} до меню вкладень?", "Add {bot} to the attachment menu?");
                let body = l!("Бот з'явиться в меню скріпки і зможе відкривати свій міні-застосунок у ваших чатах.", "The bot will appear in the 📎 menu and can open its mini app in your chats.");
                let label = lf!("Дозволити {bot} надсилати мені повідомлення", "Allow {bot} to send me messages");
                let check = b.write_access.then_some((label.as_str(), write));
                confirm::ask_box(ui, "attach-add", &title, body, check, &[l!("Скасувати", "Cancel"), l!("Додати", "Add")])
            }
            AttachAsk::Remove(b) => {
                let bot = &b.name;
                let title = lf!("Прибрати {bot} з меню вкладень?", "Remove {bot} from the attachment menu?");
                let body = l!("Його можна буде додати знову за посиланням від бота.", "It can be added again from the bot's link.");
                confirm::ask_box(ui, "attach-remove", &title, body, None, &[l!("Скасувати", "Cancel"), l!("Прибрати", "Remove")])
            }
        };
        let Some(i) = answer else { return };
        match self.attach.ask.take() {
            Some(AttachAsk::Add { peer, bot, start_param, write }) if i == 1 => {
                self.send(Cmd::ToggleAttachBot { bot: bot.bot, enabled: true, write_allowed: bot.write_access && write });
                self.attach.pending = Some((peer, bot, start_param));
            }
            Some(AttachAsk::Remove(bot)) if i == 1 => self.send(Cmd::ToggleAttachBot { bot: bot.bot, enabled: false, write_allowed: false }),
            _ => {}
        }
    }
}

#[cfg(test)]
mod tests {
    use super::tme_start_attach;

    #[test]
    fn start_attach_links() {
        assert_eq!(tme_start_attach("https://t.me/SomeBot?startattach"), Some(("SomeBot".into(), String::new())));
        assert_eq!(tme_start_attach("t.me/some_bot?startattach=ref-1&choose=users"), Some(("some_bot".into(), "ref-1".into())));
        assert_eq!(tme_start_attach("t.me/SomeBot?start=x"), None);
        assert_eq!(tme_start_attach("t.me/SomeBot?startapp"), None);
        assert_eq!(tme_start_attach("t.me/SomeBot?startattach=bad%20param"), None);
        assert_eq!(tme_start_attach("t.me/SomeBot"), None);
    }
}
