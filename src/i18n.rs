//! UI language. Ukrainian and English live next to the code:
//! `l!("Скасувати", "Cancel")` is a string, `lf!("Голосів: {n}", "Votes: {n}")` works like `format!`.
//! The compiler checks both variants, so format arguments can't drift apart.
//!
//! Every other language is a localisation file `locales/<code>.json` keyed by the English string:
//! `{"@name": "Deutsch", "Cancel": "Abbrechen", "Votes: {n}": "Stimmen: {n}"}`. Files ship inside the
//! binary (`BUILTIN`) or are dropped into `<data dir>/locales/` without rebuilding. A missing or empty
//! entry falls back to English. `locales/template.json` lists every string (see the tests below).

use std::collections::HashMap;
use std::sync::atomic::{AtomicU8, Ordering};
use std::sync::{Mutex, OnceLock, RwLock};

/// Localisation files built into the binary: (code, JSON).
const BUILTIN: &[(&str, &str)] = &[("de", include_str!("../locales/de.json"))];

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Lang {
    /// Follow the system: Ukrainian, a language with a localisation file, or English.
    System,
    Uk,
    En,
    /// A localisation file by its code (`de`, `pl`…).
    File(String),
}

impl Lang {
    /// Settings key: `system`, `uk`, `en` or a file code.
    pub fn code(&self) -> &str {
        match self {
            Lang::System => "system",
            Lang::Uk => "uk",
            Lang::En => "en",
            Lang::File(c) => c,
        }
    }

    pub fn from_code(code: &str) -> Lang {
        match code {
            "uk" => Lang::Uk,
            "en" => Lang::En,
            "system" | "" => Lang::System,
            c => Lang::File(c.to_owned()),
        }
    }

    /// Name of the language in itself, for the language list.
    pub fn name(&self) -> String {
        match self {
            Lang::System => l!("Як у системі", "System").to_owned(),
            Lang::Uk => "Українська".to_owned(),
            Lang::En => "English".to_owned(),
            Lang::File(c) => table(c).map_or_else(|| c.clone(), |t| t.name.clone()),
        }
    }

    /// Code of the language actually shown (`uk`, `en`, `de`…), e.g. as a translation target.
    pub fn shown_code(&self) -> String {
        self.resolve().code().to_owned()
    }

    /// The language actually shown (System resolved, a broken file replaced with English).
    fn resolve(&self) -> Lang {
        match self {
            Lang::System => {
                // sys-locale reads the OS language on Linux, Windows and macOS.
                let sys = sys_locale::get_locale().unwrap_or_default().to_lowercase();
                let code = sys.split(['-', '_']).next().unwrap_or_default().to_owned();
                match code.as_str() {
                    "uk" => Lang::Uk,
                    "en" => Lang::En,
                    _ if table(&code).is_some() => Lang::File(code),
                    _ => Lang::En,
                }
            }
            Lang::File(c) if table(c).is_none() => Lang::En,
            l => l.clone(),
        }
    }
}

/// Languages for the settings list: System, Ukrainian, English, then the files by name.
pub fn available() -> Vec<Lang> {
    let mut codes: Vec<String> = BUILTIN.iter().map(|(c, _)| (*c).to_owned()).collect();
    if let Ok(dir) = std::fs::read_dir(user_dir()) {
        for e in dir.flatten() {
            let p = e.path();
            if p.extension().is_some_and(|x| x == "json")
                && let Some(code) = p.file_stem().and_then(|s| s.to_str())
                && !matches!(code, "uk" | "en" | "template")
                && !codes.iter().any(|c| c == code)
            {
                codes.push(code.to_owned());
            }
        }
    }
    let mut files: Vec<Lang> = codes.into_iter().filter(|c| table(c).is_some()).map(Lang::File).collect();
    files.sort_by_key(|l| l.name().to_lowercase());
    [Lang::System, Lang::Uk, Lang::En].into_iter().chain(files).collect()
}

fn user_dir() -> std::path::PathBuf {
    crate::backend::data_dir().join("locales")
}

const UK: u8 = 0;
const EN: u8 = 1;
const FILE: u8 = 2;

static MODE: AtomicU8 = AtomicU8::new(UK);
/// The table of the active file language (`MODE == FILE`).
static ACTIVE: RwLock<Option<&'static Table>> = RwLock::new(None);

pub fn set(lang: &Lang) {
    let (mode, t) = match lang.resolve() {
        Lang::Uk => (UK, None),
        Lang::File(c) => (FILE, table(&c)),
        _ => (EN, None),
    };
    *ACTIVE.write().unwrap_or_else(|e| e.into_inner()) = t;
    MODE.store(if mode == FILE && t.is_none() { EN } else { mode }, Ordering::Relaxed);
}

/// Ukrainian is shown.
pub fn uk() -> bool {
    MODE.load(Ordering::Relaxed) == UK
}

/// A loaded localisation file.
struct Table {
    name: String,
    strings: HashMap<String, &'static str>,
}

/// Each file is read once; tables live for the whole run, so `l!` can hand out `&'static str`.
fn table(code: &str) -> Option<&'static Table> {
    static TABLES: OnceLock<Mutex<HashMap<String, Option<&'static Table>>>> = OnceLock::new();
    let mut tables = TABLES.get_or_init(Default::default).lock().unwrap_or_else(|e| e.into_inner());
    *tables.entry(code.to_owned()).or_insert_with(|| {
        // A file in the data folder wins over the built-in one: translators can try changes without a rebuild.
        let user = std::fs::read_to_string(user_dir().join(format!("{code}.json"))).ok();
        let text = user.as_deref().or_else(|| BUILTIN.iter().find(|(c, _)| *c == code).map(|(_, t)| *t))?;
        let t = parse(text)?;
        Some(&*Box::leak(Box::new(t)))
    })
}

fn parse(text: &str) -> Option<Table> {
    let map: serde_json::Map<String, serde_json::Value> = serde_json::from_str(text).ok()?;
    let mut name = String::new();
    let mut strings = HashMap::new();
    for (k, v) in map {
        let Some(v) = v.as_str() else { continue };
        if k == "@name" {
            name = v.to_owned();
        } else if !k.starts_with('@') && !v.is_empty() && placeholders(&k) == placeholders(v) {
            // A translation with other placeholders would lose or invent values: English is safer.
            strings.insert(k, &*String::leak(v.to_owned()));
        }
    }
    (!name.is_empty()).then_some(Table { name, strings })
}

/// The translation of an English string in the active file language.
fn lookup(en: &str) -> Option<&'static str> {
    if MODE.load(Ordering::Relaxed) != FILE {
        return None;
    }
    let t = (*ACTIVE.read().unwrap_or_else(|e| e.into_inner()))?;
    t.strings.get(en).copied()
}

/// `l!` at run time.
pub fn pick(uk: &'static str, en: &'static str) -> &'static str {
    match MODE.load(Ordering::Relaxed) {
        UK => uk,
        EN => en,
        _ => lookup(en).unwrap_or(en),
    }
}

#[doc(hidden)]
pub fn file_mode() -> bool {
    MODE.load(Ordering::Relaxed) == FILE
}

/// `lf!` in a file language: `rendered` is the English template already formatted by `format!`.
/// The values are cut back out of it between the template's literal parts and put into the translation.
pub fn refill(template: &str, rendered: String) -> String {
    let Some(tr) = lookup(template) else { return rendered };
    let Some(values) = extract(template, &rendered) else { return rendered };
    let mut used = vec![false; values.len()];
    let mut out = String::new();
    for tok in tokens(tr) {
        match tok {
            Tok::Lit(s) => out.push_str(&s),
            Tok::Ph(p) => {
                // Same placeholder text, first unused occurrence: positional `{}` keep their order.
                match values.iter().enumerate().find(|(i, (q, _))| !used[*i] && *q == p) {
                    Some((i, (_, v))) => {
                        used[i] = true;
                        out.push_str(v);
                    }
                    None => return rendered,
                }
            }
        }
    }
    out
}

#[derive(Debug, PartialEq)]
enum Tok {
    Lit(String),
    /// A placeholder as written, braces included: `{n}`, `{}`, `{n:.1}`.
    Ph(String),
}

/// Splits a format string into literal parts and placeholders; `{{` and `}}` are literal braces.
fn tokens(s: &str) -> Vec<Tok> {
    let mut out = Vec::new();
    let mut lit = String::new();
    let mut chars = s.chars().peekable();
    while let Some(c) = chars.next() {
        match c {
            '{' if chars.peek() == Some(&'{') => {
                chars.next();
                lit.push('{');
            }
            '}' if chars.peek() == Some(&'}') => {
                chars.next();
                lit.push('}');
            }
            '{' => {
                if !lit.is_empty() {
                    out.push(Tok::Lit(std::mem::take(&mut lit)));
                }
                let mut ph = String::from("{");
                for c in chars.by_ref() {
                    ph.push(c);
                    if c == '}' {
                        break;
                    }
                }
                out.push(Tok::Ph(ph));
            }
            c => lit.push(c),
        }
    }
    if !lit.is_empty() {
        out.push(Tok::Lit(lit));
    }
    out
}

/// Placeholders of a format string, sorted: a translation must use the same set.
fn placeholders(s: &str) -> Vec<String> {
    let mut v: Vec<String> = tokens(s).into_iter().filter_map(|t| if let Tok::Ph(p) = t { Some(p) } else { None }).collect();
    v.sort();
    v
}

/// (placeholder, value) pairs in template order, or `None` when the text can't be split unambiguously.
fn extract(template: &str, rendered: &str) -> Option<Vec<(String, String)>> {
    let toks = tokens(template);
    let mut rest = rendered;
    let mut values = Vec::new();
    for (i, tok) in toks.iter().enumerate() {
        match tok {
            Tok::Lit(s) => rest = rest.strip_prefix(s.as_str())?,
            Tok::Ph(p) => {
                let end = match toks.get(i + 1) {
                    Some(Tok::Lit(next)) => rest.find(next.as_str())?,
                    // Two placeholders in a row: no way to tell where one value ends.
                    Some(Tok::Ph(_)) => return None,
                    None => rest.len(),
                };
                values.push((p.clone(), rest[..end].to_owned()));
                rest = &rest[end..];
            }
        }
    }
    rest.is_empty().then_some(values)
}

/// UI string in Ukrainian and English (other languages: by the English string from a localisation file).
macro_rules! l {
    ($uk:literal, $en:literal) => {
        $crate::i18n::pick($uk, $en)
    };
}

/// `format!` for both languages with the same arguments.
macro_rules! lf {
    ($uk:literal, $en:literal $(, $arg:expr)* $(,)?) => {
        if $crate::i18n::uk() {
            format!($uk $(, $arg)*)
        } else if $crate::i18n::file_mode() {
            $crate::i18n::refill($en, format!($en $(, $arg)*))
        } else {
            format!($en $(, $arg)*)
        }
    };
}

pub(crate) use {l, lf};

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn refill_moves_values_into_the_translation() {
        let v = extract("{user} · members: {n}", "@rust · members: 106").unwrap();
        assert_eq!(v, vec![("{user}".into(), "@rust".into()), ("{n}".into(), "106".into())]);
        assert_eq!(extract("Votes: {n}", "Votes: 5"), Some(vec![("{n}".into(), "5".into())]));
        assert_eq!(extract("{}{}", "12"), None);
        assert_eq!(tokens("{{x}} {n:.1}"), vec![Tok::Lit("{x} ".into()), Tok::Ph("{n:.1}".into())]);
        assert_eq!(placeholders("{b} {a}"), placeholders("{a} – {b}"));
    }

    /// `l!`/`lf!` pairs in the sources: (Ukrainian, English), in order of appearance.
    fn source_strings() -> Vec<(String, String)> {
        fn walk(dir: &std::path::Path, files: &mut Vec<std::path::PathBuf>) {
            let mut entries: Vec<_> = std::fs::read_dir(dir).unwrap().flatten().map(|e| e.path()).collect();
            entries.sort();
            for p in entries {
                if p.is_dir() {
                    walk(&p, files);
                } else if p.extension().is_some_and(|x| x == "rs") {
                    files.push(p);
                }
            }
        }
        let mut files = Vec::new();
        walk(&std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("src"), &mut files);
        let mut out = Vec::new();
        for f in files {
            let text = std::fs::read_to_string(&f).unwrap();
            let mut rest = text.as_str();
            while let Some(i) = rest.find("!(") {
                let head = &rest[..i];
                rest = &rest[i + 2..];
                let name = if head.ends_with("lf") { 2 } else if head.ends_with('l') { 1 } else { continue };
                let before = head[..head.len() - name].chars().last();
                let line = &head[head.rfind('\n').map_or(0, |n| n + 1)..];
                // Only `l!(` and `lf!(` themselves (not `xl!(`), and not inside `//` comments.
                if before.is_some_and(|c| c.is_alphanumeric() || c == '_') || line.trim_start().starts_with("//") {
                    continue;
                }
                let Some((uk, after)) = literal(rest.trim_start()) else { continue };
                let after = after.trim_start().strip_prefix(',').unwrap_or(after).trim_start();
                let Some((en, _)) = literal(after) else { continue };
                out.push((uk, en));
            }
        }
        out
    }

    /// A string literal at the start of `s` (unescaped) and the text after it.
    fn literal(s: &str) -> Option<(String, &str)> {
        let s = s.strip_prefix('"')?;
        let mut out = String::new();
        let mut chars = s.char_indices();
        while let Some((i, c)) = chars.next() {
            match c {
                '"' => return Some((out, &s[i + 1..])),
                '\\' => match chars.next()?.1 {
                    'n' => out.push('\n'),
                    't' => out.push('\t'),
                    '0' => out.push('\0'),
                    'u' => {
                        let hex: String = chars.by_ref().map(|(_, c)| c).skip(1).take_while(|c| *c != '}').collect();
                        out.push(char::from_u32(u32::from_str_radix(&hex, 16).ok()?)?);
                    }
                    '\n' => {
                        // Line continuation: skip the leading whitespace of the next line.
                        let skip = s[i + 2..].len() - s[i + 2..].trim_start().len();
                        for _ in 0..skip {
                            chars.next();
                        }
                    }
                    c => out.push(c),
                },
                c => out.push(c),
            }
        }
        None
    }

    fn json_template(strings: &[(String, String)]) -> String {
        let mut seen = std::collections::HashSet::new();
        let mut out = String::from("{\n  \"@name\": \"\",\n  \"@about\": \"Copy to <code>.json, set @name, translate the values. Empty values fall back to English.\"");
        for (_, en) in strings {
            if seen.insert(en.clone()) {
                out.push_str(&format!(",\n  {}: \"\"", serde_json::to_string(en).unwrap()));
            }
        }
        out.push_str("\n}\n");
        out
    }

    /// `locales/template.json` must list exactly the strings in the code.
    /// After changing UI strings: `UPDATE_LOCALES=1 cargo test locales` rewrites it.
    #[test]
    fn locales_template_is_current() {
        let strings = source_strings();
        assert!(strings.len() > 300, "the scanner found only {} strings", strings.len());
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("locales/template.json");
        let want = json_template(&strings);
        if std::env::var_os("UPDATE_LOCALES").is_some() {
            std::fs::write(&path, &want).unwrap();
        }
        let have = std::fs::read_to_string(&path).unwrap_or_default();
        assert!(have == want, "locales/template.json is out of date: run `UPDATE_LOCALES=1 cargo test locales`");
    }

    /// Two placeholders in a row can't be cut back out of the English text (see `extract`).
    #[test]
    fn no_adjacent_placeholders() {
        for (_, en) in source_strings() {
            let toks = tokens(&en);
            assert!(!toks.windows(2).any(|w| matches!(w, [Tok::Ph(_), Tok::Ph(_)])), "{en:?}: put text between the placeholders");
        }
    }

    /// Every file: a name, only known keys, the same placeholders as English.
    #[test]
    fn locales_files_match_the_code() {
        let known: std::collections::HashSet<String> = source_strings().into_iter().map(|(_, en)| en).collect();
        for (code, text) in BUILTIN {
            let map: serde_json::Map<String, serde_json::Value> = serde_json::from_str(text).unwrap_or_else(|e| panic!("{code}.json: {e}"));
            assert!(map.get("@name").and_then(|v| v.as_str()).is_some_and(|s| !s.is_empty()), "{code}.json: no @name");
            for (k, v) in &map {
                if k.starts_with('@') {
                    continue;
                }
                assert!(known.contains(k), "{code}.json: unknown string {k:?} (renamed in the code?)");
                let v = v.as_str().unwrap_or_else(|| panic!("{code}.json: {k:?} is not a string"));
                assert!(v.is_empty() || placeholders(k) == placeholders(v), "{code}.json: placeholders differ in {k:?}");
            }
        }
    }
}
