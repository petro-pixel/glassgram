//! Demo: a few Telegram languages with a handful of strings, so Settings → Language works offline.

use std::collections::HashMap;

use crate::app::App;
use crate::backend::{Cmd, Ev, LangInfo, LangPack};

fn lang(code: &str, name: &str, native: &str, beta: bool, percent: u8) -> LangInfo {
    LangInfo { code: code.into(), name: name.into(), native: native.into(), official: !beta, beta, percent }
}

/// A few strings by tdesktop key; everything else falls back to English, as with a real partial pack.
fn strings(code: &str) -> &'static [(&'static str, &'static str)] {
    match code {
        "pl" => &[
            ("lng_menu_settings", "Ustawienia"),
            ("lng_settings_my_account", "Moje konto"),
            ("lng_settings_section_privacy", "Prywatność i bezpieczeństwo"),
            ("lng_settings_section_chat_settings", "Ustawienia czatu"),
            ("lng_settings_section_filters", "Foldery"),
            ("lng_settings_advanced", "Zaawansowane"),
            ("lng_settings_default_scale", "Domyślna skala interfejsu"),
            ("lng_dlg_filter", "Szukaj"),
            ("lng_cancel", "Anuluj"),
            ("lng_settings_save", "Zapisz"),
            ("lng_close", "Zamknij"),
        ],
        "es" => &[
            ("lng_menu_settings", "Ajustes"),
            ("lng_settings_my_account", "Mi cuenta"),
            ("lng_settings_section_privacy", "Privacidad y seguridad"),
            ("lng_settings_section_chat_settings", "Ajustes de chats"),
            ("lng_settings_section_filters", "Carpetas"),
            ("lng_settings_advanced", "Avanzados"),
            ("lng_dlg_filter", "Buscar"),
            ("lng_cancel", "Cancelar"),
            ("lng_close", "Cerrar"),
        ],
        _ => &[("lng_dlg_filter", "Serĉi"), ("lng_cancel", "Nuligi"), ("lng_menu_settings", "Agordoj")],
    }
}

fn languages() -> Vec<LangInfo> {
    vec![
        lang("en", "English", "English", false, 100),
        lang("es", "Spanish", "Español", false, 100),
        lang("pl", "Polish", "Polski", false, 100),
        lang("uk", "Ukrainian", "Українська", false, 100),
        lang("eo", "Esperanto", "Esperanto", true, 64),
        lang("tlh", "Klingon", "tlhIngan Hol", true, 12),
    ]
}

pub fn handle(app: &mut App, cmd: Cmd) {
    match cmd {
        Cmd::LoadLanguages => app.push_event(Ev::Languages(languages())),
        Cmd::LoadLangPack { code, name } => {
            let strings: HashMap<String, String> = strings(&code).iter().map(|(k, v)| ((*k).to_owned(), (*v).to_owned())).collect();
            let name = if name.is_empty() { languages().into_iter().find(|l| l.code == code).map_or_else(|| code.clone(), |l| l.native) } else { name };
            app.push_event(Ev::LangPack { code: code.clone(), pack: Ok(LangPack { code, name, version: 1, strings }) });
        }
        _ => {}
    }
}
