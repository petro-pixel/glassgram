//! Message effects (`messages.getAvailableEffects`): the list for the send menu and for the
//! badges of received messages, and each effect's `.tgs` animation, kept in the cache.

use grammers_client::tl;

use super::loader::DocLoc;
use super::{Ev, Worker};
use crate::i18n::lf;

/// An effect a message can be sent with.
#[derive(Debug, Clone, PartialEq)]
pub struct Effect {
    pub id: i64,
    /// Shown in the badge by the message time and in the menu.
    pub emoji: String,
    pub premium: bool,
}

impl Worker {
    pub(super) async fn effects_cmd(&mut self) -> Result<(), String> {
        let client = self.client()?;
        let out = self.out.clone();
        tokio::spawn(async move {
            let list = match client.invoke(&tl::functions::messages::GetAvailableEffects { hash: 0 }).await {
                Ok(tl::enums::messages::AvailableEffects::Effects(l)) => l,
                Ok(tl::enums::messages::AvailableEffects::NotModified) => return,
                // Badges stay empty and the menu has no effects: nothing else is lost.
                Err(e) => return super::log_error(&lf!("Ефекти повідомлень: {e}", "Message effects: {e}")),
            };
            let docs: Vec<tl::types::Document> = list.documents.into_iter().filter_map(|d| match d {
                tl::enums::Document::Document(d) => Some(d),
                tl::enums::Document::Empty(_) => None,
            }).collect();
            let effects: Vec<(super::Effect, Option<DocLoc>)> = list.effects.into_iter().map(|tl::enums::AvailableEffect::Effect(e)| {
                let loc = animation(&docs, e.effect_animation_id, e.effect_sticker_id);
                (super::Effect { id: e.id, emoji: e.emoticon, premium: e.premium_required }, loc)
            }).collect();
            out.send(Ev::Effects(effects.iter().map(|(e, _)| e.clone()).collect()));
            // A dozen small files, fetched once: an effect then plays without a wait.
            for (e, loc) in effects {
                let Some(loc) = loc else { continue };
                // "st…" counts as stickers in Data and storage.
                let path = crate::platform::data_dir().join("cache").join(format!("steff{}.tgs", e.id));
                if !path.exists() {
                    let mut data = Vec::new();
                    let mut it = client.iter_download(&loc);
                    while let Ok(Some(chunk)) = it.next().await {
                        data.extend(chunk);
                    }
                    if data.is_empty() {
                        continue;
                    }
                    if let Some(dir) = path.parent() {
                        let _ = tokio::fs::create_dir_all(dir).await;
                    }
                    if tokio::fs::write(&path, &data).await.is_err() {
                        continue;
                    }
                }
                out.send(Ev::EffectFile { id: e.id, path });
            }
        });
        Ok(())
    }
}

/// Where an effect's animation is: its own `.tgs` (emoji effects), or else the premium effect
/// (video size "f") of its sticker, as Telegram Desktop picks them.
fn animation(docs: &[tl::types::Document], animation_id: Option<i64>, sticker_id: i64) -> Option<DocLoc> {
    let find = |id: i64| docs.iter().find(|d| d.id == id).cloned();
    if let Some(doc) = animation_id.and_then(find) {
        return Some(DocLoc { doc, thumb: String::new() });
    }
    let doc = find(sticker_id)?;
    let has_effect = doc.video_thumbs.iter().flatten().any(|v| matches!(v, tl::enums::VideoSize::Size(s) if s.r#type == "f"));
    has_effect.then(|| DocLoc { doc, thumb: "f".into() })
}
