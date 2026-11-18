//! Demo AI editor: canned rewrites that show each mode at work, and the server's default tones.

use crate::app::App;
use crate::backend::{AiMode, AiTone, AiToneKey, Ev};

pub(super) fn tones(app: &mut App) {
    let tone = |key: &str, title: &str| AiTone { key: AiToneKey::Default(key.into()), title: title.into() };
    app.push_event(Ev::AiTones(vec![tone("formal", "Formal"), tone("friendly", "Friendly"), tone("short", "Shorter"), tone("pirate", "Pirate 🏴‍☠️")]));
}

pub(super) fn compose(app: &mut App, draft: String, mode: AiMode) {
    let text = draft.trim();
    let result = match &mode {
        AiMode::Proofread => {
            let mut s: String = text.chars().take(1).flat_map(char::to_uppercase).chain(text.chars().skip(1)).collect();
            for (wrong, right) in [(" i ", " I "), ("shoud", "should"), ("tomorow", "tomorrow"), ("  ", " ")] {
                s = s.replace(wrong, right);
            }
            if !s.ends_with(['.', '!', '?']) {
                s.push('.');
            }
            s
        }
        AiMode::Emojify => format!("{text} ✨🙂"),
        AiMode::Translate(to) => format!("[{to}] {text}"),
        AiMode::Tone(AiToneKey::Default(t)) if t == "formal" => format!("Dear colleague, {text}. Kind regards."),
        AiMode::Tone(AiToneKey::Default(t)) if t == "friendly" => format!("Hey! {text} 😊"),
        AiMode::Tone(AiToneKey::Default(t)) if t == "short" => text.split_whitespace().take(6).collect::<Vec<_>>().join(" "),
        AiMode::Tone(_) => format!("Arr, {text}, matey!"),
    };
    app.push_event(Ev::AiComposed { draft, mode, result: Ok(result) });
}
