//! Demo transcriptions and AI summaries: Olena's voice and round video messages transcribe after a
//! short pending phase, and a long "Tech News" post offers its summary.

use grammers_client::session::types::PeerId;

use crate::app::App;
use crate::backend::{Ev, Msg, Transcript};
use crate::format;
use crate::i18n::l;

/// The long channel post with a summary.
const POST: i32 = 40;

/// What Olena says in her voice (80, 81) and round video (85) messages.
fn speech(id: i32) -> Option<&'static str> {
    Some(match id {
        80 => "Hi! I'm on my way, I'll be there in ten minutes. Order me a cappuccino, please.",
        81 => "Oh, and bring the charger, mine is dead.",
        85 => "Look at this view from the roof! The whole city is in lights tonight.",
        _ => return None,
    })
}

/// A long post the server would summarize.
pub(super) fn post(base: Msg) -> Msg {
    let text = "Telegram's new desktop release is out, and it is the biggest update of the year. \
The chat list now loads twice as fast on large accounts thanks to a rewritten local cache, and media \
downloads resume after a lost connection instead of starting over. Voice messages can be turned into text \
right in the chat, long channel posts get short AI summaries, and translations keep the original formatting. \
On the design side there are new glass themes with real blur, smoother animations at 120 Hz and a reworked \
settings window that groups everything the way the mobile apps do. Developers will like the new bot features: \
mini apps can open full screen, ask for the location and save small files on the device. The update is rolling \
out gradually over the next few days on Windows, macOS and Linux.";
    Msg { id: POST, text: text.into(), spans: format::from_entities(text, &[], 0), views: Some(8_300), summary_lang: Some("en".into()), ..base }
}

/// The first call answers "pending" with the start of the text, the re-ask with all of it.
pub(super) fn transcribe(app: &mut App, peer: PeerId, id: i32) {
    let result = match speech(id) {
        Some(text) if app.demo_transcribing(peer, id) => Ok(Transcript { pending: false, text: text.into(), trial: None }),
        Some(text) => Ok(Transcript { pending: true, text: text.split(' ').take(4).collect::<Vec<_>>().join(" "), trial: None }),
        None => Err(l!("Не вдалося розпізнати мовлення", "Could not recognise the speech").into()),
    };
    app.push_event(Ev::Transcribed { peer, id, result });
}

pub(super) fn summarize(app: &mut App, peer: PeerId, id: i32) {
    let text = "• The chat list loads twice as fast, downloads resume after a lost connection\n\
• Voice-to-text, AI summaries of long posts, translations keep formatting\n\
• Glass themes with blur, 120 Hz animations, a reworked settings window\n\
• Mini apps go full screen; rolling out over the next few days";
    let result = if id == POST { Ok((text.to_owned(), Vec::new())) } else { Err(l!("Для цього повідомлення немає підсумку", "There is no summary for this message").into()) };
    app.push_event(Ev::Summarized { peer, id, result });
}
