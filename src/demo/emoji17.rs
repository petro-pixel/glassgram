//! Demo messages with the newest emoji (Emoji 15.1–17.0) in text, and big-emoji messages.

use crate::backend::Msg;

pub(super) fn messages(base: Msg) -> Vec<Msg> {
    let msg = |id, text: &str, out: bool| Msg {
        id,
        text: text.into(),
        out,
        sender: if out { String::new() } else { base.sender.clone() },
        sender_color: if out { None } else { base.sender_color },
        ..base.clone()
    };
    vec![
        msg(140, "Emoji 17 are here: 🫪 a distorted face, 🫍 an orca, 🪎 a treasure chest, 🧑‍🩰 a dancer and 🫯 a fight cloud. From 16: 🫩 🫆 🪉 🇨🇶, and from 15.1: 🐦‍🔥 🍋‍🟩 🙂‍↔️", false),
        msg(141, "🫪", false),
        msg(142, "👋🏽 🧑🏿‍🤝‍🧑🏿 🐦‍🔥", true),
    ]
}
