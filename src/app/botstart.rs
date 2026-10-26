//! Bot deep links `t.me/<bot>?start=<param>`: the chat opens with a Start bar that sends the
//! parameter to the bot instead of a plain /start.

use super::*;

/// (bot username, start parameter) from `t.me/<bot>?start=<param>`.
pub(super) fn tme_bot_start(url: &str) -> Option<(String, String)> {
    let rest = url.trim_start_matches("https://").trim_start_matches("http://");
    let rest = rest.strip_prefix("t.me/").or_else(|| rest.strip_prefix("telegram.me/"))?;
    let (bot, query) = rest.split_once('?')?;
    let bot = bot.trim_end_matches('/');
    let param = query.split('&').find_map(|kv| kv.strip_prefix("start="))?;
    // The server accepts up to 64 characters of A–Z, a–z, 0–9, _ and -.
    let ok = bot.len() >= 4 && bot.chars().all(|c| c.is_ascii_alphanumeric() || c == '_')
        && !param.is_empty() && param.len() <= 64 && param.chars().all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-');
    ok.then(|| (bot.to_owned(), param.to_owned()))
}

impl App {
    pub(super) fn open_bot_start(&mut self, username: String, param: String) {
        self.send(Cmd::OpenBotStart { username, param });
    }

    /// The parameter waiting for the Start bar in this chat.
    pub(super) fn pending_bot_start(&self, peer: PeerId) -> Option<&String> {
        self.bot_start.get(&peer)
    }

    /// Start pressed: with a parameter it goes through `messages.startBot`, otherwise as /start.
    pub(super) fn start_bot(&mut self, peer: PeerId) {
        match self.bot_start.remove(&peer) {
            Some(param) => self.send(Cmd::StartBot { peer, param }),
            None => self.send(Cmd::Send { peer, text: "/start".into(), reply_to: None, opts: Default::default() }),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::tme_bot_start;

    #[test]
    fn start_links() {
        assert_eq!(tme_bot_start("https://t.me/SomeBot?start=ref_42"), Some(("SomeBot".into(), "ref_42".into())));
        assert_eq!(tme_bot_start("t.me/some_bot/?x=1&start=a-b"), Some(("some_bot".into(), "a-b".into())));
        assert_eq!(tme_bot_start("t.me/SomeBot?startapp=x"), None);
        assert_eq!(tme_bot_start("t.me/SomeBot?start="), None);
        assert_eq!(tme_bot_start("t.me/SomeBot?start=bad%20param"), None);
        assert_eq!(tme_bot_start("t.me/SomeBot"), None);
    }
}
