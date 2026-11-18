//! The AI editor of the message field (`messages.composeMessageWithAI`, `aicompose.getTones`):
//! proofread, add emoji, translate or rewrite the draft in a tone, as in Telegram Desktop.

use grammers_client::tl;

use super::account::rpc_name;
use super::{Cmd, Ev, Worker};
use crate::format;
use crate::i18n::{l, lf};

/// What to do with the draft.
#[derive(Debug, Clone, PartialEq)]
pub enum AiMode {
    Proofread,
    Emojify,
    /// Into a language code.
    Translate(String),
    Tone(AiToneKey),
}

/// How a tone is named to the server.
#[derive(Debug, Clone, PartialEq)]
pub enum AiToneKey {
    /// One of the server's built-in tones, by its name.
    Default(String),
    /// A tone someone made (`aiComposeTone`).
    Id { id: i64, access_hash: i64 },
}

/// A tone for the tone chips.
#[derive(Debug, Clone)]
pub struct AiTone {
    pub key: AiToneKey,
    pub title: String,
}

/// The server's refusals in words; the rest as they come.
fn compose_error(e: &grammers_client::InvocationError) -> String {
    match rpc_name(e) {
        Some("PREMIUM_ACCOUNT_REQUIRED") => l!("Редактор ШІ доступний з Telegram Premium", "The AI editor needs Telegram Premium").to_owned(),
        Some(n) if n.starts_with("FLOOD_WAIT") || n.contains("LIMIT") => l!("Ліміт запитів до ШІ вичерпано, спробуйте пізніше", "AI request limit reached, try again later").to_owned(),
        Some("MESSAGE_TOO_LONG") => l!("Текст задовгий для редактора ШІ", "The text is too long for the AI editor").to_owned(),
        _ => lf!("Не вдалося змінити текст: {e}", "Could not rewrite the text: {e}"),
    }
}

fn input_tone(key: &AiToneKey) -> tl::enums::InputAiComposeTone {
    match key {
        AiToneKey::Default(tone) => tl::types::InputAiComposeToneDefault { tone: tone.clone() }.into(),
        AiToneKey::Id { id, access_hash } => tl::types::InputAiComposeToneId { id: *id, access_hash: *access_hash }.into(),
    }
}

impl Worker {
    pub(super) async fn aicompose_cmd(&mut self, cmd: Cmd) -> Result<(), String> {
        let client = self.client()?;
        let out = self.out.clone();
        match cmd {
            Cmd::AiCompose { draft, mode } => {
                let (text, entities) = format::parse(&draft);
                let req = tl::functions::messages::ComposeMessageWithAi {
                    proofread: mode == AiMode::Proofread,
                    emojify: mode == AiMode::Emojify,
                    text: tl::types::TextWithEntities { text, entities }.into(),
                    translate_to_lang: match &mode {
                        AiMode::Translate(to) => Some(to.clone()),
                        _ => None,
                    },
                    tone: match &mode {
                        AiMode::Tone(key) => Some(input_tone(key)),
                        _ => None,
                    },
                };
                tokio::spawn(async move {
                    let result = match client.invoke(&req).await {
                        Ok(tl::enums::messages::ComposedMessageWithAi::Ai(r)) => {
                            let tl::enums::TextWithEntities::Entities(t) = r.result_text;
                            Ok(format::to_markup(&t.text, &format::from_entities(&t.text, &t.entities, 0)))
                        }
                        Err(e) => Err(compose_error(&e)),
                    };
                    out.send(Ev::AiComposed { draft, mode, result });
                });
            }
            Cmd::LoadAiTones => {
                tokio::spawn(async move {
                    match client.invoke(&tl::functions::aicompose::GetTones { hash: 0 }).await {
                        Ok(tl::enums::aicompose::Tones::Tones(t)) => out.send(Ev::AiTones(t.tones.into_iter().map(tone).collect())),
                        Ok(tl::enums::aicompose::Tones::NotModified) => {}
                        Err(e) => out.send(Ev::Error(lf!("Не вдалося завантажити тони: {e}", "Could not load the tones: {e}"))),
                    }
                });
            }
            _ => {}
        }
        Ok(())
    }
}

fn tone(t: tl::enums::AiComposeTone) -> AiTone {
    match t {
        tl::enums::AiComposeTone::Default(d) => AiTone { key: AiToneKey::Default(d.tone), title: d.title },
        tl::enums::AiComposeTone::Tone(t) => AiTone { key: AiToneKey::Id { id: t.id, access_hash: t.access_hash }, title: t.title },
    }
}
