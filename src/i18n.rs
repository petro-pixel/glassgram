//! Мова інтерфейсу. Переклади лежать поруч із кодом:
//! `l!("Скасувати", "Cancel")` — рядок, `lf!("Голосів: {n}", "Votes: {n}")` — як `format!`.
//! Обидва варіанти перевіряє компілятор, тож аргументи форматування не розійдуться.

use std::sync::atomic::{AtomicBool, Ordering};

static EN: AtomicBool = AtomicBool::new(false);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Lang {
    /// Як у системі: українська для `uk`, інакше англійська.
    System,
    Uk,
    En,
}

impl Lang {
    /// Чи вибрана мова в результаті англійська.
    fn english(self) -> bool {
        match self {
            Lang::Uk => false,
            Lang::En => true,
            // sys-locale читає мову з ОС на Linux, Windows і macOS.
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

/// Рядок інтерфейсу: українською й англійською.
macro_rules! l {
    ($uk:literal, $en:literal) => {
        if $crate::i18n::en() { $en } else { $uk }
    };
}

/// `format!` для обох мов з тими самими аргументами.
macro_rules! lf {
    ($uk:literal, $en:literal $(, $arg:expr)* $(,)?) => {
        if $crate::i18n::en() { format!($en $(, $arg)*) } else { format!($uk $(, $arg)*) }
    };
}

pub(crate) use {l, lf};
