//! Emoji suggestions while typing `:word`, from Telegram's keyword lists (English and the
//! interface language), as in Telegram Desktop.

use std::collections::HashSet;

/// How many emoji the popup offers at most.
const LIMIT: usize = 24;

#[derive(Default)]
pub(super) struct EmojiKeywords {
    /// Keyword → its emoji, sorted by keyword for prefix search; all languages together.
    words: Vec<(String, Vec<String>)>,
    /// Languages already asked for.
    pub(super) asked: HashSet<String>,
}

impl EmojiKeywords {
    pub(super) fn add(&mut self, words: Vec<(String, Vec<String>)>) {
        self.words.extend(words);
        self.words.sort_by(|a, b| a.0.cmp(&b.0));
    }

    /// Emoji whose keyword starts with `query`, shortest keywords first: (emoji, keyword).
    pub(super) fn find(&self, query: &str) -> Vec<(String, String)> {
        let q = query.to_lowercase();
        let from = self.words.partition_point(|(k, _)| k.as_str() < q.as_str());
        let mut hits: Vec<&(String, Vec<String>)> = self.words[from..].iter().take_while(|(k, _)| k.starts_with(&q)).collect();
        // An exact or short keyword is a closer match than a long one that merely starts the same.
        hits.sort_by_key(|(k, _)| k.chars().count());
        let mut seen = HashSet::new();
        let mut out = Vec::new();
        for (k, emoji) in hits {
            for e in emoji {
                if seen.insert(e.clone()) {
                    out.push((e.clone(), k.clone()));
                }
            }
            if out.len() >= LIMIT {
                break;
            }
        }
        out.truncate(LIMIT);
        out
    }
}

#[cfg(test)]
mod tests {
    use super::EmojiKeywords;

    #[test]
    fn prefix_search() {
        let mut k = EmojiKeywords::default();
        k.add(vec![("heart".into(), vec!["❤".into(), "💖".into()]), ("heartbreak".into(), vec!["💔".into()]), ("hello".into(), vec!["👋".into()])]);
        k.add(vec![("серце".into(), vec!["❤".into()]), ("сердитий".into(), vec!["😠".into()])]);
        let e = |q: &str| k.find(q).into_iter().map(|(e, _)| e).collect::<Vec<_>>();
        assert_eq!(e("heart"), vec!["❤", "💖", "💔"]);
        assert_eq!(e("He"), vec!["❤", "💖", "👋", "💔"]);
        assert_eq!(e("сер"), vec!["❤", "😠"]);
        assert!(e("zzz").is_empty());
    }
}
