//! Voice transcription (`messages.transcribeAudio`) and AI summaries of long posts
//! (`messages.summarizeText`), as in Telegram Desktop.

use grammers_client::session::types::PeerId;
use grammers_client::tl;

use super::account::rpc_name;
use super::{Cmd, Ev, Worker};
use crate::format;
use crate::i18n::{l, lf};

/// A transcription, from the answer or from `updateTranscribedAudio`.
#[derive(Debug, Clone)]
pub struct Transcript {
    /// The server is still working: the text may be partial, an update finishes it.
    pub pending: bool,
    pub text: String,
    /// Without Premium: free transcriptions left and when they come back (unix time).
    pub trial: Option<(i32, i64)>,
}

/// The server's refusals in words; the rest as they come.
fn transcribe_error(e: &grammers_client::InvocationError) -> String {
    match rpc_name(e) {
        Some("PREMIUM_ACCOUNT_REQUIRED") => l!("Розшифровка голосових доступна з Telegram Premium", "Voice transcription needs Telegram Premium").to_owned(),
        Some("TRANSCRIPTION_FAILED") => l!("Не вдалося розпізнати мовлення", "Could not recognise the speech").to_owned(),
        Some("MSG_VOICE_TOO_LONG") => l!("Повідомлення задовге для розшифровки", "The message is too long to transcribe").to_owned(),
        _ => lf!("Не вдалося розшифрувати: {e}", "Could not transcribe: {e}"),
    }
}

impl Worker {
    pub(super) async fn aitext_cmd(&mut self, cmd: Cmd) -> Result<(), String> {
        let client = self.client()?;
        let out = self.out.clone();
        match cmd {
            Cmd::Transcribe { peer, id } => {
                let req = tl::functions::messages::TranscribeAudio { peer: self.peer_ref(peer)?.into(), msg_id: id };
                tokio::spawn(async move {
                    let result = match client.invoke(&req).await {
                        Ok(tl::enums::messages::TranscribedAudio::Audio(t)) => Ok(Transcript {
                            pending: t.pending,
                            text: t.text,
                            trial: t.trial_remains_num.map(|n| (n, t.trial_remains_until_date.unwrap_or(0) as i64)),
                        }),
                        Err(e) => Err(transcribe_error(&e)),
                    };
                    out.send(Ev::Transcribed { peer, id, result });
                });
            }
            Cmd::Summarize { peer, id } => {
                // No target language: the summary stays in the post's own language, as in Telegram Desktop.
                let req = tl::functions::messages::SummarizeText { peer: self.peer_ref(peer)?.into(), id, to_lang: None, tone: None };
                tokio::spawn(async move {
                    let result = match client.invoke(&req).await {
                        Ok(tl::enums::TextWithEntities::Entities(t)) => {
                            let spans = format::from_entities(&t.text, &t.entities, 0);
                            Ok((t.text, spans))
                        }
                        Err(e) => Err(lf!("Не вдалося підсумувати: {e}", "Could not summarize: {e}")),
                    };
                    out.send(Ev::Summarized { peer, id, result });
                });
            }
            _ => {}
        }
        Ok(())
    }
}

/// `updateTranscribedAudio`: finishes a pending transcription.
pub(super) fn update_event(u: &tl::types::UpdateTranscribedAudio) -> Ev {
    let t = Transcript { pending: u.pending, text: u.text.clone(), trial: None };
    Ev::Transcribed { peer: PeerId::from(&u.peer), id: u.msg_id, result: Ok(t) }
}

/// The message offers an AI summary (the server sets `summary_from_language` on long posts).
pub(super) fn summary_lang(m: &tl::enums::Message) -> Option<String> {
    match m {
        tl::enums::Message::Message(x) => x.summary_from_language.clone(),
        _ => None,
    }
}
