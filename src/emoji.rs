//! Emoji set for the panel: categories as in Telegram Desktop, plus quick reactions.
//!
//! The panel list is `assets/emoji.txt`, generated from Unicode's `emoji-test.txt` (Emoji 17.0):
//! one line per category, fully-qualified emoji without skin tones, `~` after those that take one.
//! For a new Emoji version: `UPDATE_EMOJI=path/to/emoji-test.txt cargo test emoji_table`.

use std::collections::HashSet;
use std::sync::LazyLock;

use crate::i18n::l;

/// Reactions available in most chats (Telegram's standard ones).
pub const QUICK_REACTIONS: [&str; 10] = ["👍", "❤", "🔥", "😂", "😮", "😢", "🎉", "🙏", "👏", "👎"];

/// Telegram's standard reactions offered when a chat allows only some, in the official order.
pub const STANDARD_REACTIONS: [&str; 36] = [
    "👍", "👎", "❤", "🔥", "🥰", "👏", "😁", "🤔", "🤯", "😱", "🤬", "😢", "🎉", "🤩", "🤮", "💩", "🙏", "👌",
    "🤡", "🥱", "😍", "🐳", "💯", "🤣", "🍌", "🏆", "💔", "🤨", "😐", "🍓", "🍾", "💋", "😈", "😴", "😭", "👀",
];

const DATA: &str = include_str!("../assets/emoji.txt");

/// Category titles in the order of the lines of `assets/emoji.txt`, as in Telegram Desktop.
const TITLES: [fn() -> &'static str; 8] = [
    || l!("Смайли та люди", "Smileys & People"),
    || l!("Тварини та природа", "Animals & Nature"),
    || l!("Їжа та напої", "Food & Drink"),
    || l!("Активності", "Activity"),
    || l!("Подорожі та місця", "Travel & Places"),
    || l!("Предмети", "Objects"),
    || l!("Символи", "Symbols"),
    || l!("Прапори", "Flags"),
];

/// Unicode groups of `emoji-test.txt` for each line; "Component" (bare skin tones, hair) is left out.
#[cfg(test)]
const GROUPS: [&[&str]; 8] = [
    &["Smileys & Emotion", "People & Body"],
    &["Animals & Nature"],
    &["Food & Drink"],
    &["Activities"],
    &["Travel & Places"],
    &["Objects"],
    &["Symbols"],
    &["Flags"],
];

/// Skin tone modifiers, light to dark (Fitzpatrick 1-2 … 6).
pub const TONES: [char; 5] = ['\u{1F3FB}', '\u{1F3FC}', '\u{1F3FD}', '\u{1F3FE}', '\u{1F3FF}'];

/// An emoji in the panel.
pub struct Item {
    pub emoji: &'static str,
    /// It comes in the five skin tones.
    pub toned: bool,
}

pub struct Category {
    pub title: fn() -> &'static str,
    pub items: Vec<Item>,
}

static PANEL: LazyLock<Vec<Category>> = LazyLock::new(|| {
    DATA.lines()
        .zip(TITLES)
        .map(|(line, title)| Category {
            title,
            items: line
                .split(' ')
                .filter(|e| !e.is_empty())
                .map(|e| match e.strip_suffix('~') {
                    Some(emoji) => Item { emoji, toned: true },
                    None => Item { emoji: e, toned: false },
                })
                .collect(),
        })
        .collect()
});

/// Every panel emoji without U+FE0F, so unqualified forms from other clients match too.
static KNOWN: LazyLock<HashSet<String>> =
    LazyLock::new(|| PANEL.iter().flat_map(|c| &c.items).map(|i| bare(i.emoji)).collect());

/// The panel, by category.
pub fn categories() -> &'static [Category] {
    &PANEL
}

fn bare(s: &str) -> String {
    s.chars().filter(|c| *c != '\u{FE0F}').collect()
}

/// The emoji in a skin tone, as Unicode spells it: the modifier after every person in the
/// sequence, replacing its U+FE0F. The 🤝 between two people in "people holding hands" stays plain.
pub fn with_tone(emoji: &str, tone: Option<char>) -> String {
    use unicode_properties::emoji::{EmojiStatus, UnicodeEmoji as _};
    let Some(tone) = tone else { return emoji.to_owned() };
    let zwj = emoji.contains('\u{200D}');
    let mut out = String::with_capacity(emoji.len() + 8);
    let mut toned = false;
    for c in emoji.chars() {
        if c == '\u{FE0F}' && toned {
            continue;
        }
        out.push(c);
        let base = matches!(c.emoji_status(), EmojiStatus::EmojiModifierBase | EmojiStatus::EmojiPresentationAndModifierBase);
        toned = base && !(zwj && c == '\u{1F91D}');
        if toned {
            out.push(tone);
        }
    }
    out
}

/// One grapheme cluster that Telegram shows as an emoji: anything with emoji presentation
/// (skin tones, flags, keycaps, ZWJ sequences, new emoji) or a panel emoji typed without U+FE0F (❤).
pub fn is_emoji(cluster: &str) -> bool {
    !cluster.is_ascii() && (epaint::text::has_emoji_presentation(cluster) || KNOWN.contains(&bare(cluster)))
}

/// How many emoji the text is, if it is nothing but one to three of them: Telegram Desktop draws
/// such messages large and without a bubble.
pub fn big_count(text: &str) -> Option<usize> {
    use unicode_segmentation::UnicodeSegmentation as _;
    let mut n = 0;
    for g in text.graphemes(true).filter(|g| !g.trim().is_empty()) {
        if !is_emoji(g) {
            return None;
        }
        n += 1;
        if n > 3 {
            return None;
        }
    }
    (n > 0).then_some(n)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Rebuilds `assets/emoji.txt` from `emoji-test.txt` when `UPDATE_EMOJI` points at it,
    /// and checks that every toned variant `with_tone` makes is a real fully-qualified emoji.
    #[test]
    fn emoji_table() {
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("assets/emoji.txt");
        let Some(src) = std::env::var_os("UPDATE_EMOJI") else { return };
        let text = std::fs::read_to_string(src).unwrap();
        let mut group = "";
        let mut all = HashSet::new();
        let mut rows: Vec<(String, &str)> = Vec::new();
        for line in text.lines() {
            if let Some(g) = line.strip_prefix("# group: ") {
                group = g;
            }
            let Some((cps, rest)) = line.split_once(';') else { continue };
            if !rest.trim_start().starts_with("fully-qualified") {
                continue;
            }
            let e: String = cps.split_whitespace().filter_map(|h| u32::from_str_radix(h, 16).ok()).filter_map(char::from_u32).collect();
            all.insert(e.clone());
            if !e.chars().any(|c| TONES.contains(&c)) {
                rows.push((e, group));
            }
        }
        let mut out = String::new();
        for groups in GROUPS {
            let line: Vec<String> = rows
                .iter()
                .filter(|(_, g)| groups.contains(g))
                .map(|(e, _)| {
                    let toned = TONES.iter().all(|t| {
                        let v = with_tone(e, Some(*t));
                        v != *e && all.contains(&v)
                    });
                    if toned { format!("{e}~") } else { e.clone() }
                })
                .collect();
            out.push_str(&line.join(" "));
            out.push('\n');
        }
        std::fs::write(&path, out).unwrap();
    }

    #[test]
    fn panel_has_emoji_17() {
        let all: Vec<&str> = categories().iter().flat_map(|c| &c.items).map(|i| i.emoji).collect();
        assert_eq!(categories().len(), TITLES.len());
        assert!(all.len() > 1800, "only {} emoji", all.len());
        // 15.1, 16.0 and 17.0 additions, including ZWJ sequences.
        for e in ["🐦‍🔥", "🍋‍🟩", "🫩", "🫆", "🇨🇶", "🫪", "🫍", "🪎", "🧑‍🩰"] {
            assert!(all.contains(&e), "{e} missing");
        }
        assert!(categories()[0].items.iter().any(|i| i.emoji == "👋" && i.toned));
        assert!(categories()[0].items.iter().any(|i| i.emoji == "😀" && !i.toned));
    }

    #[test]
    fn tones() {
        assert_eq!(with_tone("👋", Some(TONES[0])), "👋🏻");
        assert_eq!(with_tone("👋", None), "👋");
        // U+FE0F after a person gives way to the tone.
        assert_eq!(with_tone("🕵️‍♀️", Some(TONES[2])), "🕵🏽‍♀️");
        assert_eq!(with_tone("🧑‍🤝‍🧑", Some(TONES[4])), "🧑🏿‍🤝‍🧑🏿");
        assert_eq!(with_tone("🤝", Some(TONES[1])), "🤝🏼");
    }

    #[test]
    fn big_emoji() {
        assert_eq!(big_count("🫪"), Some(1));
        assert_eq!(big_count("❤ 👍🏽"), Some(2));
        assert_eq!(big_count("👨‍👩‍👧🇺🇦🐦‍🔥"), Some(3));
        assert_eq!(big_count("1️⃣"), Some(1));
        assert_eq!(big_count("😀😀😀😀"), None);
        assert_eq!(big_count("ok 😀"), None);
        assert_eq!(big_count("1"), None);
        assert_eq!(big_count("  "), None);
    }
}
