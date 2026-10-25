//! Message translation (`messages.translateText`), as "Translate" in Telegram Desktop's message menu.

use grammers_client::session::types::PeerId;
use grammers_client::tl;

use super::{Ev, Worker};
use crate::format;
use crate::i18n::lf;

impl Worker {
    pub(super) fn translate(&self, peer: PeerId, id: i32, to: String) -> Result<(), String> {
        let client = self.client()?;
        let r = self.peer_ref(peer)?;
        let out = self.out.clone();
        tokio::spawn(async move {
            let req = tl::functions::messages::TranslateText { peer: Some(r.into()), id: Some(vec![id]), text: None, to_lang: to.clone(), tone: None };
            // Errors (quota, unsupported language) are shown in the translation window, not as a toast.
            let result = match client.invoke(&req).await {
                Ok(tl::enums::messages::TranslatedText::TranslateResult(t)) => match t.result.into_iter().next() {
                    Some(tl::enums::TextWithEntities::Entities(t)) => {
                        let spans = format::from_entities(&t.text, &t.entities, 0);
                        Ok((t.text, spans))
                    }
                    None => Err(lf!("Сервер не повернув переклад", "The server returned no translation")),
                },
                Err(e) => Err(lf!("Не вдалося перекласти: {e}", "Could not translate: {e}")),
            };
            out.send(Ev::Translated { peer, id, to, result });
        });
        Ok(())
    }
}
