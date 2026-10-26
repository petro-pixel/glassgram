//! Emoji keywords for suggestions while typing `:word` (`messages.getEmojiKeywords`).

use grammers_client::tl;

use super::{Cmd, Ev, Worker};
use crate::i18n::lf;

impl Worker {
    pub(super) async fn emoji_keywords_cmd(&mut self, cmd: Cmd) -> Result<(), String> {
        let Cmd::LoadEmojiKeywords(lang) = cmd else { return Ok(()) };
        let client = self.client()?;
        let out = self.out.clone();
        // A few hundred kilobytes: don't hold up other commands.
        tokio::spawn(async move {
            let req = tl::functions::messages::GetEmojiKeywords { lang_code: lang };
            match client.invoke(&req).await {
                Ok(tl::enums::EmojiKeywordsDifference::Difference(d)) => {
                    let words = d
                        .keywords
                        .into_iter()
                        .filter_map(|k| match k {
                            tl::enums::EmojiKeyword::Keyword(k) => Some((k.keyword.to_lowercase(), k.emoticons)),
                            tl::enums::EmojiKeyword::Deleted(_) => None,
                        })
                        .collect();
                    out.send(Ev::EmojiKeywords { words });
                }
                // Suggestions are a convenience: the field works without them.
                Err(e) => super::log_error(&lf!("Ключові слова емодзі: {e}", "Emoji keywords: {e}")),
            }
        });
        Ok(())
    }
}
