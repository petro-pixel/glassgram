//! Message translation (`messages.translateText`): one message from Telegram Desktop's message menu,
//! or a whole chat from the translate bar, which the chat's full info offers (`translations_disabled`)
//! and `messages.togglePeerTranslations` hides.

use grammers_client::session::types::{PeerId, PeerKind};
use grammers_client::tl;

use super::{Cmd, Ev, Worker, log_error};
use crate::format;
use crate::i18n::lf;

/// Most message ids one `translateText` takes, as Telegram Desktop sends them.
pub const TRANSLATE_BATCH: usize = 20;
/// And at most this much text per request, so long posts don't make one huge request.
pub const TRANSLATE_CHARS: usize = 24 * 1024;

fn chat_disabled(full: tl::enums::messages::ChatFull) -> bool {
    let tl::enums::messages::ChatFull::Full(f) = full;
    match f.full_chat {
        tl::enums::ChatFull::Full(c) => c.translations_disabled,
        tl::enums::ChatFull::ChannelFull(c) => c.translations_disabled,
        // Communities have no such flag.
        _ => false,
    }
}

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

    pub(super) async fn translate_cmd(&mut self, cmd: Cmd) -> Result<(), String> {
        match cmd {
            Cmd::LoadTranslationOffer(peer) => self.translation_offer(peer),
            Cmd::TranslateMany { peer, ids, to } => self.translate_many(peer, ids, to),
            Cmd::TogglePeerTranslations { peer, disabled } => {
                let client = self.client()?;
                let req = tl::functions::messages::TogglePeerTranslations { disabled, peer: self.peer_ref(peer)?.into() };
                client.invoke(&req).await.map(drop).map_err(|e| lf!("Не вдалося змінити панель перекладу: {e}", "Could not change the translation bar: {e}"))
            }
            _ => Ok(()),
        }
    }

    /// Only the full info knows `translations_disabled`; a failure just means no bar.
    fn translation_offer(&self, peer: PeerId) -> Result<(), String> {
        let client = self.client()?;
        let r = self.peer_ref(peer)?;
        let out = self.out.clone();
        tokio::spawn(async move {
            let disabled = match r.id.kind() {
                PeerKind::User => client.invoke(&tl::functions::users::GetFullUser { id: r.into() }).await.map(|f| {
                    let tl::enums::users::UserFull::Full(f) = f;
                    let tl::enums::UserFull::Full(u) = f.full_user;
                    u.translations_disabled
                }),
                PeerKind::Chat => client.invoke(&tl::functions::messages::GetFullChat { chat_id: r.id.bare_id_unchecked() }).await.map(chat_disabled),
                PeerKind::Channel => client.invoke(&tl::functions::channels::GetFullChannel { channel: r.into() }).await.map(chat_disabled),
            };
            match disabled {
                Ok(disabled) => out.send(Ev::TranslationOffer { peer, offered: !disabled }),
                Err(e) => log_error(&format!("translation offer: {e}")),
            }
        });
        Ok(())
    }

    fn translate_many(&self, peer: PeerId, ids: Vec<i32>, to: String) -> Result<(), String> {
        let client = self.client()?;
        let r = self.peer_ref(peer)?;
        let out = self.out.clone();
        tokio::spawn(async move {
            let req = tl::functions::messages::TranslateText { peer: Some(r.into()), id: Some(ids.clone()), text: None, to_lang: to.clone(), tone: None };
            // The server answers in the order of `id`.
            let result = match client.invoke(&req).await {
                Ok(tl::enums::messages::TranslatedText::TranslateResult(t)) => Ok(ids
                    .iter()
                    .zip(t.result)
                    .map(|(id, tl::enums::TextWithEntities::Entities(t))| {
                        let spans = format::from_entities(&t.text, &t.entities, 0);
                        (*id, t.text, spans)
                    })
                    .collect()),
                Err(e) => Err(lf!("Не вдалося перекласти: {e}", "Could not translate: {e}")),
            };
            out.send(Ev::TranslatedMany { peer, ids, to, result });
        });
        Ok(())
    }
}
