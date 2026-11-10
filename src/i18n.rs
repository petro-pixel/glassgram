//! UI language. Ukrainian and English live next to the code:
//! `l!("Скасувати", "Cancel")` is a string, `lf!("Голосів: {n}", "Votes: {n}")` works like `format!`.
//! The compiler checks both variants, so format arguments can't drift apart.
//!
//! Every other language is a localisation file `locales/<code>.json` keyed by the English string:
//! `{"@name": "Deutsch", "Cancel": "Abbrechen", "Votes: {n}": "Stimmen: {n}"}`. Files ship inside the
//! binary (`BUILTIN`) or are dropped into `<data dir>/locales/` without rebuilding. Below a file come
//! Telegram's own translations: a language pack from the server (`backend/langpack.rs`, cached on disk)
//! is keyed by Telegram Desktop keys, and `locales/tdesktop-keys.json` says which key is which English
//! string. A missing or empty entry falls back to English. `locales/template.json` lists every string
//! (see the tests below).
//!
//! Counted words go through `lp!`: the Ukrainian forms for one / few / many, the English ones for one /
//! other, every form with `{n}`. Files key them by the English "other" form and give either one string
//! or an object of CLDR categories (`{"one": "{n} Nachricht", "other": "{n} Nachrichten"}`); a server
//! pack's `lng_xxx#one`… forms are picked the same way by `plural_category` of the language.

use std::collections::HashMap;
use std::sync::atomic::{AtomicU8, Ordering};
use std::sync::{Mutex, OnceLock, RwLock};

mod plural;
pub use plural::{Category, plural_category};

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
    for code in crate::backend::langpack::cached_codes() {
        if !matches!(code.as_str(), "uk" | "en") && !codes.contains(&code) {
            codes.push(code);
        }
    }
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
    /// Picks the plural rule.
    code: String,
    name: String,
    strings: HashMap<String, &'static str>,
    /// Counted strings by the English "other" form, forms indexed by `Category`.
    plurals: HashMap<String, [Option<&'static str>; 6]>,
}

/// Plural forms before they're leaked into a `Table`, indexed by `Category`.
type Forms = [Option<String>; 6];

/// A localisation file or a mapped server pack: plain strings and counted ones, by English string.
#[derive(Default)]
struct Strings {
    plain: HashMap<String, String>,
    plural: HashMap<String, Forms>,
}

const CATEGORIES: [&str; 6] = ["zero", "one", "two", "few", "many", "other"];

/// A plural form may drop `{n}` ("one message" → "Одне повідомлення") but not invent placeholders.
fn form_fits(en: &str, form: &str) -> bool {
    let ours = placeholders(en);
    placeholders(form).iter().all(|p| ours.contains(p))
}

/// Each file is read once; tables live for the whole run, so `l!` can hand out `&'static str`.
fn table(code: &str) -> Option<&'static Table> {
    *tables().entry(code.to_owned()).or_insert_with(|| {
        build(code, crate::backend::langpack::load_cached(code).as_ref()).map(|t| &*Box::leak(Box::new(t)))
    })
}

/// A downloaded language pack replaces the table of its language; call `set` after it to show it.
/// The old table stays leaked: strings already handed out by `l!` must remain valid.
pub fn install(pack: &crate::backend::LangPack) {
    let t = build(&pack.code, Some(pack)).map(|t| &*Box::leak(Box::new(t)));
    tables().insert(pack.code.clone(), t);
}

fn tables() -> std::sync::MutexGuard<'static, HashMap<String, Option<&'static Table>>> {
    static TABLES: OnceLock<Mutex<HashMap<String, Option<&'static Table>>>> = OnceLock::new();
    TABLES.get_or_init(Default::default).lock().unwrap_or_else(|e| e.into_inner())
}

/// The file (data folder over built-in) on top of the server pack mapped to our strings.
fn build(code: &str, pack: Option<&crate::backend::LangPack>) -> Option<Table> {
    // A file in the data folder wins over the built-in one: translators can try changes without a rebuild.
    let user = std::fs::read_to_string(user_dir().join(format!("{code}.json"))).ok();
    let text = user.as_deref().or_else(|| BUILTIN.iter().find(|(c, _)| *c == code).map(|(_, t)| *t));
    let (mut name, file) = text.and_then(parse).unwrap_or_default();
    let mut strings = pack.map(|p| from_pack(tdesktop_keys(), p)).unwrap_or_default();
    // A file's entry replaces the pack's whole, plain or plural.
    for k in file.plain.keys().chain(file.plural.keys()) {
        strings.plain.remove(k);
        strings.plural.remove(k);
    }
    strings.plain.extend(file.plain);
    strings.plural.extend(file.plural);
    if name.is_empty() {
        name = pack.map(|p| p.name.clone()).unwrap_or_default();
    }
    if name.is_empty() {
        return None;
    }
    let leak = |v: String| &*String::leak(v);
    let plain = strings.plain.into_iter().map(|(k, v)| (k, leak(v))).collect();
    let plurals = strings.plural.into_iter().map(|(k, f)| (k, f.map(|v| v.map(leak)))).collect();
    Some(Table { code: code.to_owned(), name, strings: plain, plurals })
}

/// (`@name`, English → translation) of a localisation file.
fn parse(text: &str) -> Option<(String, Strings)> {
    let map: serde_json::Map<String, serde_json::Value> = serde_json::from_str(text).ok()?;
    let mut name = String::new();
    let mut strings = Strings::default();
    for (k, v) in map {
        if let Some(o) = v.as_object() {
            // `{"one": …, "few": …, "other": …}` for a counted string.
            let forms: Forms = std::array::from_fn(|i| o.get(CATEGORIES[i]).and_then(|v| v.as_str()).filter(|v| !v.is_empty() && form_fits(&k, v)).map(str::to_owned));
            if !k.starts_with('@') && forms.iter().any(Option::is_some) {
                strings.plural.insert(k, forms);
            }
            continue;
        }
        let Some(v) = v.as_str() else { continue };
        if k == "@name" {
            name = v.to_owned();
        } else if !k.starts_with('@') && !v.is_empty() && placeholders(&k) == placeholders(v) {
            // A translation with other placeholders would lose or invent values: English is safer.
            strings.plain.insert(k, v.to_owned());
        }
    }
    Some((name, strings))
}

/// A tdesktop key → the English strings it translates, each with (tdesktop placeholder, ours) renames.
type KeyMap = HashMap<String, Vec<(String, Vec<(String, String)>)>>;

fn tdesktop_keys() -> &'static KeyMap {
    static KEYS: OnceLock<KeyMap> = OnceLock::new();
    KEYS.get_or_init(|| parse_keys(include_str!("../locales/tdesktop-keys.json")))
}

/// `"English": "lng_key"` or `"English": {"key": "lng_key", "args": {"{user}": "{t}"}}`.
fn parse_keys(text: &str) -> KeyMap {
    let map: serde_json::Map<String, serde_json::Value> = serde_json::from_str(text).unwrap_or_default();
    let mut out = KeyMap::new();
    for (en, v) in map {
        let (key, args) = match &v {
            serde_json::Value::String(k) => (k.as_str(), Vec::new()),
            serde_json::Value::Object(o) => {
                let Some(k) = o.get("key").and_then(|k| k.as_str()) else { continue };
                let args = o.get("args").and_then(|a| a.as_object()).map(|a| a.iter().filter_map(|(f, t)| Some((f.clone(), t.as_str()?.to_owned()))).collect()).unwrap_or_default();
                (k, args)
            }
            _ => continue,
        };
        if !en.starts_with('@') {
            out.entry(key.to_owned()).or_default().push((en, args));
        }
    }
    out
}

/// Telegram's strings by tdesktop key → ours by English string, placeholders renamed to ours.
/// A plural key (`lng_xxx#one`… in the pack) maps all its forms onto our counted string.
fn from_pack(keys: &KeyMap, pack: &crate::backend::LangPack) -> Strings {
    // Markup (**bold**, __italic__, [links](…)) would show as raw characters.
    let usable = |v: &String| !v.is_empty() && !v.contains("**") && !v.contains("__") && !v.contains("](");
    let rename = |value: &str, args: &[(String, String)]| args.iter().fold(value.to_owned(), |v, (from, to)| v.replace(from.as_str(), to));
    let mut out = Strings::default();
    for (key, ours) in keys {
        if let Some(forms) = pack.plurals.get(key) {
            for (en, args) in ours {
                let forms: Forms = std::array::from_fn(|i| Some(rename(&forms[i], args)).filter(|v| usable(v) && form_fits(en, v)));
                // "other" is the fallback for every category the language doesn't fill.
                if forms[Category::Other as usize].as_ref().is_some_and(|v| placeholders(en) == placeholders(v)) {
                    out.plural.insert(en.clone(), forms);
                }
            }
            continue;
        }
        let Some(value) = pack.strings.get(key).filter(|v| usable(v)) else { continue };
        for (en, args) in ours {
            let v = rename(value, args);
            if placeholders(en) == placeholders(&v) {
                out.plain.insert(en.clone(), v);
            }
        }
    }
    out
}

/// The translation of an English string in the active file language.
fn lookup(en: &str) -> Option<&'static str> {
    if MODE.load(Ordering::Relaxed) != FILE {
        return None;
    }
    let t = (*ACTIVE.read().unwrap_or_else(|e| e.into_inner()))?;
    t.strings.get(en).copied()
}

/// The form of a counted string for `n` in the active file language; a plain entry serves every count.
fn lookup_plural(en_other: &str, n: u64) -> Option<&'static str> {
    if MODE.load(Ordering::Relaxed) != FILE {
        return None;
    }
    let t = (*ACTIVE.read().unwrap_or_else(|e| e.into_inner()))?;
    t.plural(en_other, n)
}

impl Table {
    fn plural(&self, en_other: &str, n: u64) -> Option<&'static str> {
        match self.plurals.get(en_other) {
            Some(forms) => forms[plural_category(&self.code, n) as usize].or(forms[Category::Other as usize]),
            None => self.strings.get(en_other).copied(),
        }
    }
}

/// Index of the Ukrainian form for `lp!`: one, few, many.
#[doc(hidden)]
pub fn uk_form(n: u64) -> usize {
    match plural_category("uk", n) {
        Category::One => 0,
        Category::Few => 1,
        _ => 2,
    }
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
    fill(tr, template, &rendered).unwrap_or(rendered)
}

/// `lp!` in a file language: the values are cut out of the English "other" form (`other`, rendered by
/// `format!`) and put into the translation's form for `n`; with no translation, `shown` stays English.
#[doc(hidden)]
pub fn refill_plural(template: &str, n: u64, other: &str, shown: String) -> String {
    lookup_plural(template, n).and_then(|tr| fill(tr, template, other)).unwrap_or(shown)
}

/// The translation `tr` of `template`, with the values cut out of the English `rendered`.
fn fill(tr: &str, template: &str, rendered: &str) -> Option<String> {
    let values = extract(template, rendered)?;
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
                    None => return None,
                }
            }
        }
    }
    Some(out)
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

/// A counted string: `lp!(n, ["{n} повідомлення", "{n} повідомлення", "{n} повідомлень"], ["{n} message", "{n} messages"])`.
/// Ukrainian gives the one / few / many forms, English one / other; every form must use `{n}` (other
/// values are captured like in `format!`). `n` is a whole number; other languages pick their form by CLDR.
/// A count as people read it: "12,480" in English, "12 480" elsewhere; short numbers stay as they are.
pub fn group(n: u64) -> String {
    let digits = n.to_string();
    if n < 10_000 {
        return digits;
    }
    let sep = if MODE.load(Ordering::Relaxed) == EN { ',' } else { '\u{a0}' };
    let mut out = String::with_capacity(digits.len() + digits.len() / 3);
    for (i, d) in digits.chars().enumerate() {
        if i > 0 && (digits.len() - i) % 3 == 0 {
            out.push(sep);
        }
        out.push(d);
    }
    out
}

macro_rules! lp {
    ($n:expr, [$uk1:literal, $uk2:literal, $uk5:literal $(,)?], [$en1:literal, $en2:literal $(,)?] $(,)?) => {{
        let __c = $n as u64;
        let __n = $crate::i18n::group(__c);
        if $crate::i18n::uk() {
            match $crate::i18n::uk_form(__c) {
                0 => format!($uk1, n = __n),
                1 => format!($uk2, n = __n),
                _ => format!($uk5, n = __n),
            }
        } else {
            let __other = format!($en2, n = __n);
            let __shown = if __c == 1 { format!($en1, n = __n) } else { __other.clone() };
            if $crate::i18n::file_mode() { $crate::i18n::refill_plural($en2, __c, &__other, __shown) } else { __shown }
        }
    }};
}

pub(crate) use {l, lf, lp};

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn counts_are_grouped() {
        assert_eq!(group(2310), "2310");
        let g = group(1_234_567);
        assert!(g == "1,234,567" || g == "1\u{a0}234\u{a0}567", "{g}");
    }

    #[test]
    fn refill_moves_values_into_the_translation() {
        let v = extract("{user} · members: {n}", "@rust · members: 106").unwrap();
        assert_eq!(v, vec![("{user}".into(), "@rust".into()), ("{n}".into(), "106".into())]);
        assert_eq!(extract("Votes: {n}", "Votes: 5"), Some(vec![("{n}".into(), "5".into())]));
        assert_eq!(extract("{}{}", "12"), None);
        assert_eq!(tokens("{{x}} {n:.1}"), vec![Tok::Lit("{x} ".into()), Tok::Ph("{n:.1}".into())]);
        assert_eq!(placeholders("{b} {a}"), placeholders("{a} – {b}"));
    }

    /// A string in the sources; for `lp!` the last Ukrainian and English forms (English "other" is the key).
    struct Src {
        uk: String,
        en: String,
        plural: bool,
    }

    /// `l!`/`lf!`/`lp!` pairs in the sources: (Ukrainian, English), in order of appearance.
    fn source_strings() -> Vec<(String, String)> {
        scan().into_iter().map(|s| (s.uk, s.en)).collect()
    }

    /// `[ "a", "b", … ]` at the start of `s`: the literals and the text after the bracket.
    fn literal_list(s: &str) -> Option<(Vec<String>, &str)> {
        let mut rest = s.trim_start().strip_prefix('[')?;
        let mut out = Vec::new();
        loop {
            rest = rest.trim_start();
            if let Some(r) = rest.strip_prefix(']') {
                return Some((out, r));
            }
            let (lit, after) = literal(rest)?;
            out.push(lit);
            rest = after.trim_start();
            rest = rest.strip_prefix(',').unwrap_or(rest);
        }
    }

    /// The text after the first top-level comma: skips `lp!`'s count expression.
    fn after_count(s: &str) -> Option<&str> {
        let mut depth = 0i32;
        for (i, c) in s.char_indices() {
            match c {
                '(' | '[' | '{' => depth += 1,
                ')' | ']' | '}' => depth -= 1,
                ',' if depth == 0 => return Some(&s[i + 1..]),
                _ => {}
            }
        }
        None
    }

    fn scan() -> Vec<Src> {
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
                // From the start of the file: the line of a second macro on it starts before `rest`.
                let head = &text[..text.len() - rest.len() + i];
                rest = &rest[i + 2..];
                let plural = head.ends_with("lp");
                let name = if head.ends_with("lf") || plural { 2 } else if head.ends_with('l') { 1 } else { continue };
                let before = head[..head.len() - name].chars().last();
                let line = &head[head.rfind('\n').map_or(0, |n| n + 1)..];
                // Only `l!(`, `lf!(` and `lp!(` themselves (not `xl!(`), and not inside `//` comments.
                if before.is_some_and(|c| c.is_alphanumeric() || c == '_') || line.trim_start().starts_with("//") {
                    continue;
                }
                if plural {
                    let Some((uk, after)) = after_count(rest).and_then(literal_list) else { continue };
                    let after = after.trim_start().strip_prefix(',').unwrap_or(after);
                    let Some((en, _)) = literal_list(after) else { continue };
                    let (Some(uk), Some(en)) = (uk.last(), en.last()) else { continue };
                    out.push(Src { uk: uk.clone(), en: en.clone(), plural: true });
                    continue;
                }
                let Some((uk, after)) = literal(rest.trim_start()) else { continue };
                let after = after.trim_start().strip_prefix(',').unwrap_or(after).trim_start();
                let Some((en, _)) = literal(after) else { continue };
                out.push(Src { uk, en, plural: false });
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

    fn json_template(strings: &[Src]) -> String {
        let mut seen = std::collections::HashSet::new();
        let mut out = String::from("{\n  \"@name\": \"\",\n  \"@about\": \"Copy to <code>.json, set @name, translate the values. Empty values fall back to English. Counted strings take the CLDR forms your language uses: zero, one, two, few, many, other.\"");
        for s in strings {
            if seen.insert(s.en.clone()) {
                let value = if s.plural { r#"{"one": "", "other": ""}"# } else { "\"\"" };
                out.push_str(&format!(",\n  {}: {value}", serde_json::to_string(&s.en).unwrap()));
            }
        }
        out.push_str("\n}\n");
        out
    }

    /// `locales/template.json` must list exactly the strings in the code.
    /// After changing UI strings: `UPDATE_LOCALES=1 cargo test locales` rewrites it.
    #[test]
    fn locales_template_is_current() {
        let strings = scan();
        assert!(strings.iter().any(|s| s.plural), "the scanner found no lp! strings");
        assert!(strings.len() > 300, "the scanner found only {} strings", strings.len());
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("locales/template.json");
        let want = json_template(&strings);
        if std::env::var_os("UPDATE_LOCALES").is_some() {
            std::fs::write(&path, &want).unwrap();
        }
        let have = std::fs::read_to_string(&path).unwrap_or_default();
        assert!(have == want, "locales/template.json is out of date: run `UPDATE_LOCALES=1 cargo test locales`");
    }

    #[test]
    fn server_pack_maps_onto_our_strings() {
        let keys = parse_keys(
            r#"{"@about": "x", "Cancel": "lng_cancel", "Forwarded from {from}": {"key": "lng_forwarded", "args": {"{user}": "{from}"}}, "Close": "lng_close", "Save": "lng_save",
                "{n} messages": {"key": "lng_messages", "args": {"{count}": "{n}"}}, "{n} votes": {"key": "lng_votes", "args": {"{count}": "{n}"}}}"#,
        );
        let strings: HashMap<String, String> = [
            ("lng_cancel", "Abbrechen"),
            ("lng_forwarded", "Weitergeleitet von {user}"),
            // Markup and foreign placeholders are dropped rather than shown raw or filled wrongly.
            ("lng_close", "**Schließen**"),
            ("lng_save", "Speichern {count}"),
        ]
        .into_iter()
        .map(|(k, v)| (k.to_owned(), v.to_owned()))
        .collect();
        let forms = |f: [&str; 6]| f.map(str::to_owned);
        let plurals = [
            // A form may drop the count; one with a foreign placeholder is dropped.
            ("lng_messages".to_owned(), forms(["", "Одно сообщение", "", "{count} сообщения", "{count} сообщений {x}", "{count} сообщения"])),
            // Without a usable "other" the whole string stays English.
            ("lng_votes".to_owned(), forms(["", "{count} голос", "", "", "", "{x}"])),
        ]
        .into();
        let pack = crate::backend::LangPack { code: "ru".into(), name: "Русский".into(), version: 1, strings, plurals };
        let ours = from_pack(&keys, &pack);
        assert_eq!(ours.plain.get("Cancel").map(String::as_str), Some("Abbrechen"));
        assert_eq!(ours.plain.get("Forwarded from {from}").map(String::as_str), Some("Weitergeleitet von {from}"));
        assert!(!ours.plain.contains_key("Close") && !ours.plain.contains_key("Save"));
        let f = &ours.plural["{n} messages"];
        assert_eq!((f[1].as_deref(), f[3].as_deref(), f[4].as_deref(), f[5].as_deref()), (Some("Одно сообщение"), Some("{n} сообщения"), None, Some("{n} сообщения")));
        assert!(!ours.plural.contains_key("{n} votes"));
    }

    /// `lp!` in a file language: the form by the language's rule, "other" for a missing one, English without any.
    #[test]
    fn plural_lookup_by_category() {
        let (_, file) = parse(r#"{"@name": "Polski", "{n} messages": {"one": "{n} wiadomość", "few": "{n} wiadomości", "other": "{n} wiadomości*"}}"#).unwrap();
        let leak = |v: String| &*String::leak(v);
        let t = Table {
            code: "pl".into(),
            name: "Polski".into(),
            strings: [("Votes: {n}".to_owned(), "Głosy: {n}")].into(),
            plurals: file.plural.into_iter().map(|(k, f)| (k, f.map(|v| v.map(leak)))).collect(),
        };
        // `lp!` cuts the values out of the English "other" form and fills the chosen one.
        let form = |n: u64| fill(t.plural("{n} messages", n).unwrap(), "{n} messages", &format!("{n} messages")).unwrap();
        assert_eq!([form(1), form(22), form(25)], ["1 wiadomość", "22 wiadomości", "25 wiadomości*"]);
        assert_eq!(t.plural("Votes: {n}", 5), Some("Głosy: {n}"));
        assert_eq!(t.plural("{n} files", 5), None);
        assert_eq!([uk_form(1), uk_form(3), uk_form(11), uk_form(21), uk_form(25)], [0, 1, 2, 0, 2]);
    }

    /// Order: a localisation file over the server pack, the pack over English.
    #[test]
    fn file_wins_over_the_pack() {
        let de = parse(BUILTIN.iter().find(|(c, _)| *c == "de").unwrap().1).unwrap().1.plain;
        // A mapped string the German file doesn't have.
        let (key, en) = tdesktop_keys()
            .iter()
            .find_map(|(k, v)| v.iter().find(|(en, a)| a.is_empty() && !de.contains_key(en)).map(|(en, _)| (k.clone(), en.clone())))
            .unwrap();
        let pack = crate::backend::LangPack {
            code: "de".into(),
            name: "Deutsch (Telegram)".into(),
            version: 1,
            strings: [("lng_cancel".to_owned(), "PACK".to_owned()), (key, "AUS DEM PAKET".to_owned())].into(),
            plurals: HashMap::new(),
        };
        let t = build("de", Some(&pack)).unwrap();
        assert_eq!(t.name, "Deutsch");
        assert_eq!(t.strings.get("Cancel").copied(), Some("Abbrechen"));
        assert_eq!(t.strings.get(en.as_str()).copied(), Some("AUS DEM PAKET"));
        let t = build("xx-test", Some(&crate::backend::LangPack { code: "xx-test".into(), name: "Xx".into(), ..pack })).unwrap();
        assert_eq!((t.name.as_str(), t.strings.get("Cancel").copied()), ("Xx", Some("PACK")));
        assert!(build("xx-none", None).is_none());
    }

    /// The mapping names real strings with tdesktop keys, and renames only to placeholders we have.
    #[test]
    fn tdesktop_keys_match_the_code() {
        let known: std::collections::HashSet<String> = source_strings().into_iter().map(|(_, en)| en).collect();
        let keys = tdesktop_keys();
        assert!(keys.values().map(Vec::len).sum::<usize>() > 300);
        for (key, ours) in keys {
            assert!(key.starts_with("lng_") && !key.contains('#'), "{key}: not a plain tdesktop key");
            for (en, args) in ours {
                assert!(known.contains(en), "tdesktop-keys.json: unknown string {en:?}");
                for (_, to) in args {
                    assert!(placeholders(en).contains(to), "tdesktop-keys.json: {en:?} has no {to}");
                }
            }
        }
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
                if let Some(forms) = v.as_object() {
                    for (c, f) in forms {
                        assert!(CATEGORIES.contains(&c.as_str()), "{code}.json: {k:?} has no plural category {c:?}");
                        assert!(f.as_str().is_some_and(|f| form_fits(k, f)), "{code}.json: placeholders differ in {k:?} ({c})");
                    }
                    continue;
                }
                let v = v.as_str().unwrap_or_else(|| panic!("{code}.json: {k:?} is not a string"));
                assert!(v.is_empty() || placeholders(k) == placeholders(v), "{code}.json: placeholders differ in {k:?}");
            }
        }
    }
}
