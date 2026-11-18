//! Where a click on a link, mention, hashtag or command in a message goes: chats, searches, bot
//! commands, t.me links the client handles itself (boosts, bot starts, sticker sets, folders,
//! attachment bots, mini apps, usernames, proxies), and everything else to the browser.

use super::*;

impl App {
    pub(super) fn open_target(&mut self, peer: PeerId, target: String) {
        if target.starts_with("tg://user?id=") {
            // A mention by id opens the chat with that person if we have it.
            let id = target["tg://user?id=".len()..].parse::<i64>().ok().map(PeerId::user_unchecked);
            match id.filter(|p| self.chats.iter().any(|c| c.id == *p)) {
                Some(p) => self.open_chat(p),
                None => self.toast = Some((l!("Цього користувача немає у ваших чатах", "This user is not in your chats").into(), 0.0, false)),
            }
            return;
        }
        match target.chars().next() {
            // A hashtag is searched in this chat, a bot command is sent.
            Some('#' | '$') => {
                let s = self.chat_search.get_or_insert_with(Search::default);
                s.query = target;
                s.edited_at = 0.0;
            }
            Some('/') => self.send(Cmd::Send { peer, text: target, reply_to: self.reply_target(), opts: SendOpts::default(), send_as: self.send_as_for(peer) }),
            // A mention opens the chat in the client (a public one without joining).
            Some('@') if !self.demo => self.send(Cmd::ResolveUsername(target[1..].to_owned())),
            Some('@') => self.open_link(&format!("https://t.me/{}", &target[1..])),
            _ if crate::backend::proxy::is_proxy_link(&target) => match crate::backend::proxy::Proxy::parse(&target) {
                Ok(_) => self.opening = Some(confirm::Opening::Proxy(target)),
                Err(e) => self.toast = Some((e.to_owned(), 0.0, true)),
            },
            _ if boosts::tme_boost(&target).is_some() => {
                if let Some(name) = boosts::tme_boost(&target) {
                    self.open_boost_link(name);
                }
            }
            _ if botstart::tme_bot_start(&target).is_some() => {
                if let Some((username, param)) = botstart::tme_bot_start(&target) {
                    self.open_bot_start(username, param);
                }
            }
            _ if crate::backend::addstickers_name(&target).is_some() => {
                if let Some(name) = crate::backend::addstickers_name(&target) {
                    self.open_sticker_set(crate::backend::StickerSetRef::Name(name));
                }
            }
            _ if chatlists::tme_addlist(&target).is_some() => {
                if let Some(slug) = chatlists::tme_addlist(&target) {
                    self.send(Cmd::CheckChatlist(slug));
                }
            }
            _ if attachbots::tme_start_attach(&target).is_some() => {
                if let Some((username, start_param)) = attachbots::tme_start_attach(&target) {
                    self.send(Cmd::OpenAttachLink { peer, username, start_param });
                }
            }
            _ if tme_web_app(&target).is_some() => {
                if let Some((username, app, start_param)) = tme_web_app(&target) {
                    self.open_web_app_link(username, app, start_param);
                }
            }
            _ if !self.demo && tme_username(&target).is_some() => {
                let name = tme_username(&target).unwrap_or_default();
                self.send(Cmd::ResolveUsername(name));
            }
            _ => {
                let url = if target.contains("://") || target.starts_with("mailto:") { target } else { format!("https://{target}") };
                if !self.url_auth_link(&url) {
                    self.open_link(&url);
                }
            }
        }
    }
}
