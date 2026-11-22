//! A text's language: `whatlang` when it is sure, and for short messages, which it can't judge, a
//! guess from the most common words. The server translates from any language, so this only decides
//! whether to offer the bar and which messages to ask for.

/// Shorter texts go to the word guess unless `whatlang` calls them reliable.
const MIN_CHARS: usize = 32;
const MIN_CONFIDENCE: f64 = 0.25;

/// `whatlang`'s ISO 639-3 codes → the ISO 639-1 codes Telegram takes.
const CODES: [(&str, &str); 44] = [
    ("afr", "af"), ("ara", "ar"), ("aze", "az"), ("bel", "be"), ("ben", "bn"), ("bul", "bg"), ("cat", "ca"), ("ces", "cs"), ("cmn", "zh"),
    ("dan", "da"), ("deu", "de"), ("ell", "el"), ("eng", "en"), ("est", "et"), ("fin", "fi"), ("fra", "fr"), ("heb", "he"), ("hin", "hi"),
    ("hrv", "hr"), ("hun", "hu"), ("hye", "hy"), ("ind", "id"), ("ita", "it"), ("jpn", "ja"), ("kat", "ka"), ("kor", "ko"), ("lit", "lt"),
    ("lav", "lv"), ("mkd", "mk"), ("nld", "nl"), ("nob", "no"), ("pes", "fa"), ("pol", "pl"), ("por", "pt"), ("ron", "ro"), ("rus", "ru"),
    ("slk", "sk"), ("slv", "sl"), ("spa", "es"), ("srp", "sr"), ("swe", "sv"), ("tur", "tr"), ("ukr", "uk"), ("vie", "vi"),
];

/// Frequent short words of each language the guess knows.
const WORDS: [(&str, &[&str]); 9] = [
    ("en", &["the", "and", "is", "are", "you", "to", "of", "it", "that", "this", "with", "for", "have", "what", "was", "not", "we", "be", "will", "on"]),
    ("de", &["der", "die", "das", "und", "ist", "nicht", "ich", "wir", "ein", "eine", "mit", "auf", "für", "sie", "es", "zu", "auch", "noch", "heute", "wer", "den", "dem", "habe", "schon"]),
    ("es", &["el", "la", "los", "las", "que", "y", "es", "de", "en", "un", "una", "por", "para", "con", "no", "se", "está", "hola", "pero", "muy", "del", "al"]),
    ("fr", &["le", "la", "les", "et", "est", "de", "des", "un", "une", "que", "pas", "pour", "dans", "avec", "je", "nous", "vous", "ce", "du", "sur", "bonjour"]),
    ("it", &["il", "lo", "la", "gli", "che", "e", "è", "di", "un", "una", "per", "non", "con", "sono", "ciao", "del", "della", "anche"]),
    ("pt", &["o", "a", "os", "as", "que", "e", "é", "de", "um", "uma", "para", "com", "não", "em", "você", "olá", "do", "da", "mas"]),
    ("pl", &["i", "w", "nie", "się", "to", "jest", "na", "że", "z", "do", "jak", "co", "ale", "czy", "już", "tak"]),
    ("uk", &["і", "й", "що", "не", "це", "на", "в", "у", "з", "як", "та", "але", "є", "ми", "ви", "ти", "вже", "привіт", "його", "так"]),
    ("ru", &["и", "что", "не", "это", "на", "в", "с", "как", "но", "мы", "вы", "ты", "уже", "привет", "он", "она", "есть", "так"]),
];

/// The likeliest language code of `text`, or `None` when nothing stands out.
pub(super) fn detect(text: &str) -> Option<&'static str> {
    // `is_reliable` turns down most chat-length messages whose top guess is right; the chat's
    // language is a majority of many messages, so a clear lead on a full sentence is enough.
    let sure = |i: &whatlang::Info| i.is_reliable() || (text.chars().count() >= MIN_CHARS && i.confidence() >= MIN_CONFIDENCE);
    if let Some(info) = whatlang::detect(text).filter(sure)
        && let Some((_, code)) = CODES.iter().find(|(iso3, _)| *iso3 == info.lang().code())
    {
        return Some(code);
    }
    by_words(text)
}

fn by_words(text: &str) -> Option<&'static str> {
    let lower = text.to_lowercase();
    // Letters only one of the two Cyrillic languages has settle it at once.
    if lower.chars().any(|c| matches!(c, 'і' | 'ї' | 'є' | 'ґ')) {
        return Some("uk");
    }
    if lower.chars().any(|c| matches!(c, 'ы' | 'э' | 'ъ' | 'ё')) {
        return Some("ru");
    }
    let cyrillic = lower.chars().any(|c| ('\u{400}'..='\u{4ff}').contains(&c));
    let mut best: Option<(&'static str, usize)> = None;
    let mut tie = false;
    for (code, words) in WORDS {
        // Latin stop words don't count in Cyrillic text and the other way round.
        if cyrillic != matches!(code, "uk" | "ru") {
            continue;
        }
        let n = lower.split(|c: char| !c.is_alphabetic()).filter(|w| words.contains(w)).count();
        match best {
            Some((_, b)) if n == b => tie = true,
            Some((_, b)) if n < b => {}
            _ if n > 0 => {
                best = Some((code, n));
                tie = false;
            }
            _ => {}
        }
    }
    best.filter(|_| !tie).map(|(code, _)| code)
}

/// The language most of `texts` are in: at least three guesses and more than half of them.
pub(super) fn majority<'a>(texts: impl Iterator<Item = &'a str>) -> Option<&'static str> {
    let mut counts: Vec<(&'static str, usize)> = Vec::new();
    let mut total = 0;
    for code in texts.filter_map(detect) {
        total += 1;
        match counts.iter_mut().find(|(c, _)| *c == code) {
            Some((_, n)) => *n += 1,
            None => counts.push((code, 1)),
        }
    }
    counts.into_iter().max_by_key(|(_, n)| *n).filter(|(_, n)| *n >= 3 && *n * 2 > total).map(|(c, _)| c)
}

#[cfg(test)]
mod tests {
    use super::{detect, majority};

    #[test]
    fn guesses_common_languages() {
        assert_eq!(detect("Wer kommt heute zum Treffen? Ich bringe die Folien mit."), Some("de"));
        assert_eq!(detect("Hola, ¿alguien tiene el enlace de la reunión?"), Some("es"));
        assert_eq!(detect("Have you seen the new design? It is great"), Some("en"));
        assert_eq!(detect("Привіт, як справи?"), Some("uk"));
        assert_eq!(detect("Это уже готово"), Some("ru"));
        assert_eq!(detect("👍"), None);
    }

    #[test]
    fn knows_languages_without_a_word_list() {
        assert_eq!(detect("De vergadering begint morgenochtend om negen uur in het gemeentehuis van Amsterdam."), Some("nl"));
        assert_eq!(detect("Toplantı yarın sabah dokuzda belediye binasında başlayacak, lütfen geç kalmayın."), Some("tr"));
    }

    #[test]
    fn majority_needs_enough_messages() {
        let de = ["Ich bin hier", "Das ist gut", "Wir sehen uns heute", "ok"];
        assert_eq!(majority(de.iter().copied()), Some("de"));
        assert_eq!(majority(["Das ist gut", "Ich bin da"].iter().copied()), None);
    }
}
