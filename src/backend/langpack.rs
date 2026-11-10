//! Interface languages from Telegram: the list Telegram Desktop offers (`langpack.getLanguages`) and
//! a language's strings (`langpack.getLangPack`), cached in `langpacks/<code>.json` and brought up to
//! date with `langpack.getDifference` from the cached version. `i18n.rs` maps them onto our strings.

use super::*;

/// Telegram Desktop's pack: its keys are what `locales/tdesktop-keys.json` maps to.
const LANG_PACK: &str = "tdesktop";

/// A language from the server list.
#[derive(Debug, Clone, PartialEq)]
pub struct LangInfo {
    pub code: String,
    /// In English.
    pub name: String,
    /// In the language itself.
    pub native: String,
    pub official: bool,
    /// A community translation from translations.telegram.org, not reviewed yet.
    pub beta: bool,
    /// Share of the strings translated, 0–100.
    pub percent: u8,
}

/// A language's strings by tdesktop key, as cached on disk.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct LangPack {
    pub code: String,
    /// Native name, for the language list.
    pub name: String,
    pub version: i32,
    pub strings: HashMap<String, String>,
}

/// Server codes become file names: only `[a-z0-9_-]`, so a code can't point outside the folder.
pub fn valid_code(code: &str) -> bool {
    !code.is_empty() && code.len() <= 32 && code.bytes().all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-' || b == b'_')
}

fn cache_dir() -> PathBuf {
    data_dir().join("langpacks")
}

/// Codes of the packs cached on disk.
pub fn cached_codes() -> Vec<String> {
    let Ok(dir) = std::fs::read_dir(cache_dir()) else { return Vec::new() };
    dir.flatten()
        .filter_map(|e| {
            let p = e.path();
            let code = p.file_stem()?.to_str()?.to_owned();
            (p.extension().is_some_and(|x| x == "json") && valid_code(&code)).then_some(code)
        })
        .collect()
}

pub fn load_cached(code: &str) -> Option<LangPack> {
    if !valid_code(code) {
        return None;
    }
    load_from(&cache_dir().join(format!("{code}.json")))
}

fn load_from(path: &std::path::Path) -> Option<LangPack> {
    serde_json::from_str(&std::fs::read_to_string(path).ok()?).ok()
}

fn save_to(dir: &std::path::Path, pack: &LangPack) -> Result<(), String> {
    std::fs::create_dir_all(dir).map_err(|e| e.to_string())?;
    make_private(dir);
    let path = dir.join(format!("{}.json", pack.code));
    let text = serde_json::to_string(pack).map_err(|e| e.to_string())?;
    std::fs::write(&path, text).map_err(|e| e.to_string())?;
    make_private(&path);
    Ok(())
}

/// Applies a difference: new and changed strings replace, deleted ones go. Plural strings are skipped:
/// none of our strings is mapped to one (see `locales/tdesktop-keys.json`).
fn apply(pack: &mut LangPack, diff: tl::types::LangPackDifference) {
    pack.version = diff.version;
    for s in diff.strings {
        match s {
            tl::enums::LangPackString::String(s) => {
                pack.strings.insert(s.key, s.value);
            }
            tl::enums::LangPackString::Pluralized(s) => {
                pack.strings.remove(&s.key);
            }
            tl::enums::LangPackString::Deleted(s) => {
                pack.strings.remove(&s.key);
            }
        }
    }
}

fn language(l: tl::enums::LangPackLanguage) -> LangInfo {
    let tl::enums::LangPackLanguage::Language(l) = l;
    let percent = if l.strings_count > 0 { (l.translated_count.clamp(0, l.strings_count) as i64 * 100 / l.strings_count as i64) as u8 } else { 0 };
    LangInfo { code: l.lang_code, name: l.name, native: l.native_name, official: l.official, beta: l.beta, percent }
}

/// The cached pack brought up to date, or downloaded whole when there's none or the difference doesn't fit.
async fn fetch(client: &Client, code: &str, name: &str) -> Result<LangPack, String> {
    let err = |e: String| lf!("Мовний пакет: {e}", "Language pack: {e}");
    let mut pack = load_cached(code).unwrap_or_default();
    let mut fresh = true;
    if pack.code == code && pack.version > 0 {
        let req = tl::functions::langpack::GetDifference { lang_pack: LANG_PACK.into(), lang_code: code.into(), from_version: pack.version };
        if let Ok(tl::enums::LangPackDifference::Difference(d)) = client.invoke(&req).await
            && d.from_version == pack.version
        {
            apply(&mut pack, d);
            fresh = false;
        }
    }
    if fresh {
        pack = LangPack { code: code.to_owned(), ..Default::default() };
        let req = tl::functions::langpack::GetLangPack { lang_pack: LANG_PACK.into(), lang_code: code.into() };
        let tl::enums::LangPackDifference::Difference(d) = client.invoke(&req).await.map_err(|e| err(e.to_string()))?;
        apply(&mut pack, d);
    }
    if !name.is_empty() {
        pack.name = name.to_owned();
    }
    if pack.name.is_empty() {
        pack.name = code.to_owned();
    }
    save_to(&cache_dir(), &pack).map_err(err)?;
    Ok(pack)
}

impl Worker {
    pub(super) async fn langpack_cmd(&mut self, cmd: Cmd) -> Result<(), String> {
        let client = self.client()?;
        let out = self.out.clone();
        match cmd {
            Cmd::LoadLanguages => {
                tokio::spawn(async move {
                    match client.invoke(&tl::functions::langpack::GetLanguages { lang_pack: LANG_PACK.into() }).await {
                        Ok(list) => out.send(Ev::Languages(list.into_iter().map(language).filter(|l| valid_code(&l.code)).collect())),
                        Err(e) => out.send(Ev::Error(lf!("Мови: {e}", "Languages: {e}"))),
                    }
                });
            }
            Cmd::LoadLangPack { code, name } => {
                if !valid_code(&code) {
                    return Err(l!("Невідома мова", "Unknown language").into());
                }
                tokio::spawn(async move {
                    let pack = fetch(&client, &code, &name).await;
                    out.send(Ev::LangPack { code, pack });
                });
            }
            _ => {}
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn diff(from: i32, version: i32, strings: Vec<tl::enums::LangPackString>) -> tl::types::LangPackDifference {
        tl::types::LangPackDifference { lang_code: "xx".into(), from_version: from, version, strings }
    }

    fn string(key: &str, value: &str) -> tl::enums::LangPackString {
        tl::types::LangPackString { key: key.into(), value: value.into() }.into()
    }

    #[test]
    fn difference_and_cache_round_trip() {
        let mut pack = LangPack { code: "xx".into(), name: "Xx".into(), ..Default::default() };
        apply(&mut pack, diff(0, 3, vec![string("lng_cancel", "Abbrechen"), string("lng_close", "Zu")]));
        apply(&mut pack, diff(3, 5, vec![string("lng_close", "Schließen"), tl::types::LangPackStringDeleted { key: "lng_cancel".into() }.into()]));
        assert_eq!(pack.version, 5);
        assert_eq!(pack.strings.get("lng_close").map(String::as_str), Some("Schließen"));
        assert!(!pack.strings.contains_key("lng_cancel"));

        let dir = std::env::temp_dir().join(format!("glassgram-langpack-test-{}", std::process::id()));
        save_to(&dir, &pack).unwrap();
        assert_eq!(load_from(&dir.join("xx.json")), Some(pack));
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            assert_eq!(std::fs::metadata(dir.join("xx.json")).unwrap().permissions().mode() & 0o777, 0o600);
        }
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn codes_stay_inside_the_folder() {
        assert!(valid_code("pt-br") && valid_code("zh-hans-raw"));
        assert!(!valid_code("../x") && !valid_code("") && !valid_code("De") && !valid_code("a/b"));
    }
}
