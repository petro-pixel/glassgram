//! UI language. Translations live next to the code:
//! `l!("Скасувати", "Cancel")` is a string, `lf!("Голосів: {n}", "Votes: {n}")` works like `format!`.
//! The compiler checks both variants, so format arguments can't drift apart.

use std::sync::atomic::{AtomicBool, Ordering};

static EN: AtomicBool = AtomicBool::new(false);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Lang {
    /// Follow the system: Ukrainian for `uk`, English otherwise.
    System,
    Uk,
    En,
}

impl Lang {
    /// Whether the resulting language is English.
    fn english(self) -> bool {
        match self {
            Lang::Uk => false,
            Lang::En => true,
            // sys-locale reads the OS language on Linux, Windows and macOS.
            Lang::System => !sys_locale::get_locale().is_some_and(|l| l.to_lowercase().starts_with("uk")),
        }
    }
}

pub fn set(lang: Lang) {
    EN.store(lang.english(), Ordering::Relaxed);
}

pub fn en() -> bool {
    EN.load(Ordering::Relaxed)
}

/// UI string in Ukrainian and English.
macro_rules! l {
    ($uk:literal, $en:literal) => {
        if $crate::i18n::en() { $en } else { $uk }
    };
}

/// `format!` for both languages with the same arguments.
macro_rules! lf {
    ($uk:literal, $en:literal $(, $arg:expr)* $(,)?) => {
        if $crate::i18n::en() { format!($en $(, $arg)*) } else { format!($uk $(, $arg)*) }
    };
}

pub(crate) use {l, lf};
