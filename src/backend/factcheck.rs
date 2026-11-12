//! Fact checks under messages (`factCheck`, `messages.getFactCheck`) and member tags
//! (`message.from_rank`), as in Telegram Desktop.

use grammers_client::tl;
use serde::{Deserialize, Serialize};

use super::{Cmd, Ev, Worker};
use crate::format::{self, Span};
use crate::i18n::lf;

/// The most ids one `messages.getFactCheck` takes, as Telegram Desktop sends them.
pub const FACT_CHECK_BATCH: usize = 100;

/// A clarification independent checkers added to a message.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct FactCheck {
    /// The text is missing or stale: `messages.getFactCheck` brings it when the message is shown.
    pub need_check: bool,
    /// ISO code of the checkers' country.
    pub country: String,
    pub text: String,
    pub spans: Vec<Span>,
}

impl FactCheck {
    fn from_tl(f: tl::enums::FactCheck) -> Self {
        let tl::enums::FactCheck::Check(f) = f;
        let (text, spans) = match f.text {
            Some(tl::enums::TextWithEntities::Entities(t)) => {
                let spans = format::from_entities(&t.text, &t.entities, 0);
                (t.text, spans)
            }
            None => (String::new(), Vec::new()),
        };
        Self { need_check: f.need_check, country: f.country.unwrap_or_default(), text, spans }
    }
}

impl Worker {
    pub(super) async fn factcheck_cmd(&mut self, cmd: Cmd) -> Result<(), String> {
        let Cmd::LoadFactChecks { peer, mut ids } = cmd else { return Ok(()) };
        ids.truncate(FACT_CHECK_BATCH);
        let client = self.client()?;
        let out = self.out.clone();
        let req = tl::functions::messages::GetFactCheck { peer: self.peer_ref(peer)?.into(), msg_id: ids.clone() };
        tokio::spawn(async move {
            match client.invoke(&req).await {
                // The answer goes in the order of the ids asked.
                Ok(list) => out.send(Ev::FactChecks { peer, checks: ids.into_iter().zip(list.into_iter().map(FactCheck::from_tl)).collect() }),
                Err(e) => out.send(Ev::Error(lf!("Не вдалося завантажити перевірку фактів: {e}", "Could not load the fact check: {e}"))),
            }
        });
        Ok(())
    }
}

/// The fact check a message arrived with.
pub(super) fn of_message(m: &tl::enums::Message) -> Option<FactCheck> {
    match m {
        tl::enums::Message::Message(x) => x.factcheck.clone().map(FactCheck::from_tl),
        _ => None,
    }
}

/// The member tag shown by the author's name (`from_rank`).
pub(super) fn rank(m: &tl::enums::Message) -> Option<String> {
    match m {
        tl::enums::Message::Message(x) => x.from_rank.clone().filter(|r| !r.trim().is_empty()),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fact_check_keeps_text_country_and_staleness() {
        let text = tl::types::TextWithEntities { text: "No such study".into(), entities: vec![tl::types::MessageEntityBold { offset: 0, length: 2 }.into()] };
        let raw = tl::types::FactCheck { need_check: true, country: Some("UA".into()), text: Some(text.into()), hash: 7 };
        let f = FactCheck::from_tl(raw.into());
        assert!(f.need_check);
        assert_eq!((f.country.as_str(), f.text.as_str()), ("UA", "No such study"));
        assert_eq!(f.spans.len(), 1);
        // A stale check may come without a text: nothing to show until it is fetched.
        let bare = FactCheck::from_tl(tl::types::FactCheck { need_check: true, country: None, text: None, hash: 0 }.into());
        assert!(bare.text.is_empty() && bare.country.is_empty());
    }
}
