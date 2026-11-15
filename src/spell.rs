//! Spell checking without the UI: Hunspell dictionaries (`spellbook`) downloaded on demand into
//! `dictionaries/` in the data folder, the personal word list, and finding the words of a draft.
//! Downloaded files are untrusted: fixed HTTPS addresses, size limits, UTF-8 only, and parsed
//! before they are kept.

use std::collections::{HashMap, HashSet};
use std::io::Read;
use std::ops::Range;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use crate::format::{self, Style};

/// The writing system a dictionary covers: a word is only checked by dictionaries of its own
/// script (the Ukrainian dictionary accepts any Latin word, an English one rejects all Cyrillic).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Script {
    Latin,
    Cyrillic,
}

/// A dictionary that can be downloaded.
pub struct Lang {
    /// Our code, also the file name: `en_US`, `uk_UA`…
    pub code: &'static str,
    /// The folder in wooorm/dictionaries.
    repo: &'static str,
    /// The language's name in itself.
    pub name: &'static str,
    pub script: Script,
}

const fn lang(code: &'static str, repo: &'static str, name: &'static str, script: Script) -> Lang {
    Lang { code, repo, name, script }
}

/// The list in Settings → Advanced → Manage dictionaries.
pub const LANGS: &[Lang] = &[
    lang("en_US", "en", "English (US)", Script::Latin),
    lang("en_GB", "en-GB", "English (UK)", Script::Latin),
    lang("uk_UA", "uk", "Українська", Script::Cyrillic),
    lang("de_DE", "de", "Deutsch", Script::Latin),
    lang("fr_FR", "fr", "Français", Script::Latin),
    lang("es_ES", "es", "Español", Script::Latin),
    lang("it_IT", "it", "Italiano", Script::Latin),
    lang("pl_PL", "pl", "Polski", Script::Latin),
    lang("pt_BR", "pt", "Português (Brasil)", Script::Latin),
    lang("pt_PT", "pt-PT", "Português (Portugal)", Script::Latin),
    lang("cs_CZ", "cs", "Čeština", Script::Latin),
];

pub fn find(code: &str) -> Option<&'static Lang> {
    LANGS.iter().find(|l| l.code == code)
}

/// The dictionary for a UI or system language code (`uk`, `en-GB`, `de_AT`…).
fn for_locale(locale: &str) -> Option<&'static str> {
    let l = locale.replace('-', "_").to_lowercase();
    let base = l.split('_').next().unwrap_or_default();
    match (base, l.as_str()) {
        ("en", "en_gb" | "en_ie" | "en_au" | "en_nz") => Some("en_GB"),
        ("pt", "pt_pt") => Some("pt_PT"),
        _ => LANGS.iter().find(|x| x.code.split('_').next().is_some_and(|b| b.eq_ignore_ascii_case(base))).map(|x| x.code),
    }
}

/// Dictionaries to use before the user picks any: the interface language, the system language
/// and English, as Telegram Desktop picks them.
pub fn default_langs(ui_lang: &str) -> Vec<String> {
    let sys = sys_locale::get_locale().unwrap_or_default();
    let mut out: Vec<String> = Vec::new();
    for code in [for_locale(ui_lang), for_locale(&sys), Some("en_US")].into_iter().flatten() {
        if !out.iter().any(|c| c == code) {
            out.push(code.to_owned());
        }
    }
    out
}

/// Larger files are refused: the biggest real ones are about 1 MB (.aff) and 8.5 MB (.dic).
const MAX_AFF: u64 = 4 << 20;
const MAX_DIC: u64 = 24 << 20;
/// Longer "words" are pasted junk, not worth checking.
const MAX_WORD: usize = 48;

fn dir() -> PathBuf {
    crate::platform::data_dir().join("dictionaries")
}

fn paths(code: &str) -> (PathBuf, PathBuf) {
    let d = dir();
    (d.join(format!("{code}.aff")), d.join(format!("{code}.dic")))
}

/// Both files of the dictionary are on disk.
pub fn downloaded(code: &str) -> bool {
    let (aff, dic) = paths(code);
    aff.is_file() && dic.is_file()
}

/// Size of a downloaded dictionary in bytes.
pub fn size_on_disk(code: &str) -> u64 {
    let (aff, dic) = paths(code);
    [aff, dic].iter().filter_map(|p| std::fs::metadata(p).ok()).map(|m| m.len()).sum()
}

pub fn remove(code: &str) {
    let (aff, dic) = paths(code);
    let _ = std::fs::remove_file(aff);
    let _ = std::fs::remove_file(dic);
}

/// A loaded dictionary, shared with the thread that makes suggestions.
pub struct Dict {
    pub code: &'static str,
    script: Script,
    inner: spellbook::Dictionary,
}

fn parse(lang: &'static Lang, aff: &str, dic: &str) -> Result<Dict, String> {
    let inner = spellbook::Dictionary::new(aff, dic).map_err(|e| format!("{}: {e}", lang.code))?;
    Ok(Dict { code: lang.code, script: lang.script, inner })
}

/// Reads a downloaded dictionary; slow for big ones (Ukrainian: ~0.2 s), so off the UI thread.
pub fn load(code: &str) -> Result<Dict, String> {
    let lang = find(code).ok_or_else(|| format!("unknown dictionary {code}"))?;
    let (aff, dic) = paths(code);
    let read = |p: &PathBuf, max: u64| -> Result<String, String> {
        let len = std::fs::metadata(p).map_err(|e| e.to_string())?.len();
        if len > max {
            return Err(format!("{}: too large", lang.code));
        }
        std::fs::read_to_string(p).map_err(|e| e.to_string())
    };
    parse(lang, &read(&aff, MAX_AFF)?, &read(&dic, MAX_DIC)?)
}

/// Downloads a dictionary, checks it parses, keeps it and returns it loaded. `progress` gets the
/// bytes received so far and the expected total (0 while unknown).
pub fn download(code: &str, mut progress: impl FnMut(u64, u64)) -> Result<Dict, String> {
    let lang = find(code).ok_or_else(|| format!("unknown dictionary {code}"))?;
    let config = ureq::Agent::config_builder().timeout_global(Some(Duration::from_secs(120))).max_redirects(3).build();
    let agent = ureq::Agent::new_with_config(config);
    let base = format!("https://raw.githubusercontent.com/wooorm/dictionaries/main/dictionaries/{}/index", lang.repo);
    let mut total = 0u64;
    let mut got = 0u64;
    let mut fetch = |ext: &str, max: u64| -> Result<String, String> {
        let resp = agent.get(format!("{base}.{ext}")).call().map_err(|e| e.to_string())?;
        let len: u64 = resp.headers().get("content-length").and_then(|v| v.to_str().ok()).and_then(|v| v.parse().ok()).unwrap_or(0);
        if len > max {
            return Err(format!("{}: too large", lang.code));
        }
        total += len;
        let mut reader = resp.into_body().into_reader();
        let mut buf = Vec::with_capacity(len as usize);
        let mut chunk = [0u8; 64 * 1024];
        loop {
            let n = reader.read(&mut chunk).map_err(|e| e.to_string())?;
            if n == 0 {
                break;
            }
            buf.extend_from_slice(&chunk[..n]);
            got += n as u64;
            if buf.len() as u64 > max {
                return Err(format!("{}: too large", lang.code));
            }
            progress(got, total.max(got));
        }
        String::from_utf8(buf).map_err(|_| format!("{}: not UTF-8", lang.code))
    };
    let aff = fetch("aff", MAX_AFF)?;
    let dic = fetch("dic", MAX_DIC)?;
    let dict = parse(lang, &aff, &dic)?;
    let d = dir();
    std::fs::create_dir_all(&d).map_err(|e| e.to_string())?;
    crate::backend::make_private(&d);
    let (aff_path, dic_path) = paths(code);
    // Through temporary names, so a half-written pair never counts as downloaded.
    for (path, text) in [(&dic_path, &dic), (&aff_path, &aff)] {
        let tmp = path.with_extension("part");
        std::fs::write(&tmp, text).map_err(|e| e.to_string())?;
        std::fs::rename(&tmp, path).map_err(|e| e.to_string())?;
    }
    Ok(dict)
}

fn personal_path() -> PathBuf {
    dir().join("personal.txt")
}

/// The words added with "Add to dictionary".
pub fn load_personal() -> HashSet<String> {
    let text = std::fs::read_to_string(personal_path()).unwrap_or_default();
    text.lines().map(str::trim).filter(|w| !w.is_empty() && w.chars().count() <= MAX_WORD).map(str::to_owned).collect()
}

pub fn save_personal(words: &HashSet<String>) {
    let d = dir();
    let _ = std::fs::create_dir_all(&d);
    crate::backend::make_private(&d);
    let mut list: Vec<&str> = words.iter().map(String::as_str).collect();
    list.sort_unstable();
    let path = personal_path();
    let _ = std::fs::write(&path, list.join("\n") + "\n");
    // What a person writes about is private.
    crate::backend::make_private(&path);
}

fn script_of(word: &str) -> Option<Script> {
    let c = word.chars().find(|c| c.is_alphabetic())?;
    match c as u32 {
        0x0400..=0x052F => Some(Script::Cyrillic),
        _ if c.is_ascii_alphabetic() => Some(Script::Latin),
        0x00C0..=0x024F | 0x1E00..=0x1EFF => Some(Script::Latin),
        _ => None,
    }
}

/// Typographic apostrophes and stress marks, as the dictionaries expect the word.
fn normalize(word: &str) -> String {
    word.chars().filter(|&c| c != '\u{301}').map(|c| if matches!(c, '’' | 'ʼ') { '\'' } else { c }).collect()
}

/// Checks words against the loaded dictionaries, or the demo list, with a cache per word.
#[derive(Default)]
pub struct Checker {
    dicts: Vec<Arc<Dict>>,
    /// `--demo`: a small built-in list instead of downloaded dictionaries.
    demo: bool,
    personal: HashSet<String>,
    /// "Ignore": until the app closes.
    ignored: HashSet<String>,
    cache: HashMap<String, bool>,
}

impl Checker {
    pub fn demo() -> Self {
        Self { demo: true, ..Self::default() }
    }

    pub fn set_personal(&mut self, words: HashSet<String>) {
        self.personal = words;
        self.cache.clear();
    }

    pub fn personal(&self) -> &HashSet<String> {
        &self.personal
    }

    pub fn add_personal(&mut self, word: &str) {
        self.personal.insert(word.to_owned());
        self.cache.remove(word);
    }

    pub fn ignore(&mut self, word: &str) {
        self.ignored.insert(word.to_owned());
        self.cache.remove(word);
    }

    pub fn dicts(&self) -> &[Arc<Dict>] {
        &self.dicts
    }

    /// Replaces the set of dictionaries in use; earlier answers no longer hold.
    pub fn set_dicts(&mut self, dicts: Vec<Arc<Dict>>) {
        self.dicts = dicts;
        self.cache.clear();
    }

    /// Whether checking can say anything at all.
    pub fn active(&self) -> bool {
        self.demo || !self.dicts.is_empty()
    }

    /// The cached answer for a word, if it was checked already.
    pub fn cached(&self, word: &str) -> Option<bool> {
        self.cache.get(word).copied()
    }

    /// Whether the word is spelled right; remembered, so a long draft costs little per frame.
    pub fn check(&mut self, word: &str) -> bool {
        if let Some(ok) = self.cached(word) {
            return ok;
        }
        let ok = self.check_uncached(word);
        // A bound on memory for someone typing for hours.
        if self.cache.len() > 50_000 {
            self.cache.clear();
        }
        self.cache.insert(word.to_owned(), ok);
        ok
    }

    fn check_uncached(&self, word: &str) -> bool {
        let lower = word.to_lowercase();
        if self.ignored.contains(word) || self.personal.contains(word) || self.personal.contains(&lower) {
            return true;
        }
        let w = normalize(word);
        if self.demo {
            return demo_ok(&w);
        }
        let Some(script) = script_of(&w) else { return true };
        let dicts: Vec<&Dict> = self.dicts.iter().map(|d| &**d).filter(|d| d.script == script).collect();
        // No dictionary for this alphabet: nothing to say about the word.
        if dicts.is_empty() {
            return true;
        }
        let ok = |w: &str| dicts.iter().any(|d| d.inner.check(w));
        // "well-known": the whole word, or each part on its own.
        ok(&w) || (w.contains('-') && w.split('-').all(|p| p.is_empty() || ok(p)))
    }

    /// What `suggest` needs on another thread.
    pub fn suggester(&self, word: &str) -> Suggester {
        let script = script_of(word);
        Suggester { dicts: self.dicts.iter().filter(|d| Some(d.script) == script).cloned().collect(), demo: self.demo }
    }
}

/// Makes suggestions off the UI thread: they take up to a few hundred ms in a big dictionary.
pub struct Suggester {
    dicts: Vec<Arc<Dict>>,
    demo: bool,
}

/// As many as Telegram Desktop shows at the top of the menu.
pub const MAX_SUGGESTIONS: usize = 5;

impl Suggester {
    pub fn suggest(&self, word: &str) -> Vec<String> {
        let w = normalize(word);
        let mut out: Vec<String> = Vec::new();
        if self.demo {
            out = demo_suggest(&w);
        }
        for d in &self.dicts {
            let mut s = Vec::new();
            d.inner.suggest(&w, &mut s);
            for x in s {
                if !out.contains(&x) {
                    out.push(x);
                }
            }
            if out.len() >= MAX_SUGGESTIONS {
                break;
            }
        }
        out.truncate(MAX_SUGGESTIONS);
        out
    }
}

/// The words of a draft worth checking, as byte ranges: letters with apostrophes and hyphens
/// inside, outside markup markers, code, links, @mentions, #hashtags and /commands, not glued to
/// digits, and not the word still being typed at the very end.
pub fn words(text: &str) -> Vec<Range<usize>> {
    let (spans, marks) = format::markup_ranges(text);
    let mut skip: Vec<(usize, usize)> = marks;
    skip.extend(spans.iter().filter(|s| matches!(s.style, Style::Code | Style::Pre)).map(|s| (s.start, s.end)));
    let hidden = |at: usize| skip.iter().any(|&(a, b)| a <= at && at < b);
    let mut out = Vec::new();
    // Runs of text between spaces and hidden stretches.
    let mut run_start = None;
    let chars: Vec<(usize, char)> = text.char_indices().collect();
    for k in 0..=chars.len() {
        let sep = chars.get(k).is_none_or(|&(at, c)| c.is_whitespace() || hidden(at));
        match (sep, run_start) {
            (false, None) => run_start = Some(k),
            (true, Some(s)) => {
                run_words(text, &chars[s..k], &mut out);
                run_start = None;
            }
            _ => {}
        }
    }
    out.retain(|r: &Range<usize>| r.end < text.len());
    out
}

fn is_letter(c: char) -> bool {
    c.is_alphabetic() || c == '\u{301}'
}

fn run_words(text: &str, run: &[(usize, char)], out: &mut Vec<Range<usize>>) {
    let (Some(&(start, _)), Some(&(last, lc))) = (run.first(), run.last()) else { return };
    let s = &text[start..last + lc.len_utf8()];
    let dotted = s.char_indices().any(|(i, c)| c == '.' && s[..i].ends_with(char::is_alphabetic) && s[i + 1..].starts_with(char::is_alphabetic));
    if s.contains("://") || s.starts_with("www.") || s.contains('@') || s.starts_with(['#', '/', '$']) || dotted {
        return;
    }
    let mut k = 0;
    while k < run.len() {
        if !run[k].1.is_alphabetic() {
            k += 1;
            continue;
        }
        let begin = k;
        while k < run.len() {
            let c = run[k].1;
            let joiner = matches!(c, '\'' | '’' | 'ʼ' | '-') && run.get(k + 1).is_some_and(|n| n.1.is_alphabetic()) && k > begin;
            if is_letter(c) || joiner {
                k += 1;
            } else {
                break;
            }
        }
        let glued = |c: Option<&(usize, char)>| c.is_some_and(|&(_, c)| c.is_numeric() || c == '_');
        let len = k - begin;
        if len >= 2 && len <= MAX_WORD && !glued(begin.checked_sub(1).and_then(|b| run.get(b))) && !glued(run.get(k)) {
            let end = run.get(k).map_or(start + s.len(), |&(at, _)| at);
            out.push(run[begin].0..end);
        }
    }
}

/// The built-in list for `--demo`, so the feature shows without a network.
const DEMO_WORDS: &str = "a about after again all also am an and any are as at back be because been before being better but by call can come could day did do don't down each even every few find first for from get give go good got great had has have he her here him his how i i'll i'm if in into is it it's its just know last let like little long look made make many may me more most much my need new no not now of off old on one only or other our out over people please really right said same say screenshot see send she should show so some still such sure take than thank thanks that the their them then there these they thing think this those time to today tomorrow tonight too two up us use very want was way we well were what when where which who why will with work would year yes yet you your \
    а або але без би біля був була були було бути в вам вас ваш весь вже ви від він вона вони воно все всі де для до дуже дякую є же за завтра звісно і із її їх й його коли куди ми мене мені мій може можна моя на навіть нам нас наш не немає ні ну о обов'язково от по під після привіт про раз разом сьогодні так також там те терпіння ти тобі того той тут у уже хто це цей ця час через що щось як який якщо";

fn demo_ok(word: &str) -> bool {
    let lower = word.to_lowercase();
    // Unknown words count as right unless they are one slip away from a known one, so typing
    // freely in the demo doesn't paint everything red.
    DEMO_WORDS.split_whitespace().any(|w| w == lower) || !DEMO_WORDS.split_whitespace().any(|w| w.chars().count() >= 4 && slips(w, &lower) == 1)
}

fn demo_suggest(word: &str) -> Vec<String> {
    let lower = word.to_lowercase();
    let mut near: Vec<(usize, &str)> = DEMO_WORDS.split_whitespace().map(|w| (slips(w, &lower), w)).filter(|(d, _)| *d <= 2).collect();
    near.sort_by_key(|(d, _)| *d);
    let capital = word.chars().next().is_some_and(char::is_uppercase);
    near.into_iter()
        .map(|(_, w)| if capital { w.chars().take(1).flat_map(char::to_uppercase).chain(w.chars().skip(1)).collect() } else { w.to_owned() })
        .take(MAX_SUGGESTIONS)
        .collect()
}

/// Edit distance counting a swap of two neighbours as one slip (Damerau–Levenshtein, restricted).
fn slips(a: &str, b: &str) -> usize {
    let (a, b): (Vec<char>, Vec<char>) = (a.chars().collect(), b.chars().collect());
    let mut d = vec![vec![0usize; b.len() + 1]; a.len() + 1];
    for (i, row) in d.iter_mut().enumerate() {
        row[0] = i;
    }
    for j in 0..=b.len() {
        d[0][j] = j;
    }
    for i in 1..=a.len() {
        for j in 1..=b.len() {
            let cost = usize::from(a[i - 1] != b[j - 1]);
            d[i][j] = (d[i - 1][j] + 1).min(d[i][j - 1] + 1).min(d[i - 1][j - 1] + cost);
            if i > 1 && j > 1 && a[i - 1] == b[j - 2] && a[i - 2] == b[j - 1] {
                d[i][j] = d[i][j].min(d[i - 2][j - 2] + 1);
            }
        }
    }
    d[a.len()][b.len()]
}

#[cfg(test)]
mod tests {
    use super::*;

    fn found(text: &str) -> Vec<&str> {
        words(text).into_iter().map(|r| &text[r]).collect()
    }

    #[test]
    fn finds_words() {
        assert_eq!(found("Hi there, don't  stop "), ["Hi", "there", "don't", "stop"]);
        assert_eq!(found("обов’язково будь-який день"), ["обов’язково", "будь-який"]);
        // Markup, code, links, mentions, numbers and the word being typed are left alone.
        assert_eq!(found("**bold** `codez` [label](https://x.io/abc) @user #tag v2 x_y www.a.com typing"), ["bold", "label"]);
        assert!(found("/start now").is_empty());
        assert!(found("a b").is_empty());
    }

    #[test]
    fn demo_list() {
        let mut c = Checker::demo();
        assert!(c.check("Tonight"));
        assert!(!c.check("tonihgt"));
        assert!(!c.check("терпіня"));
        // Far from every known word: not flagged.
        assert!(c.check("Glassgram"));
        c.ignore("tonihgt");
        assert!(c.check("tonihgt"));
        assert_eq!(c.suggester("терпіня").suggest("терпіня"), ["терпіння"]);
        assert_eq!(Suggester { dicts: Vec::new(), demo: true }.suggest("Tonihgt")[0], "Tonight");
    }

    #[test]
    fn locales() {
        assert_eq!(for_locale("uk"), Some("uk_UA"));
        assert_eq!(for_locale("en-GB"), Some("en_GB"));
        assert_eq!(for_locale("en_US"), Some("en_US"));
        assert_eq!(for_locale("pt-BR"), Some("pt_BR"));
        assert_eq!(for_locale("ja"), None);
        assert!(default_langs("uk").starts_with(&["uk_UA".to_owned()]));
    }
}
