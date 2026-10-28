//! The error log: errors shown to the user, background failures and panics go to
//! `logs/errors.log` in the data folder, so a problem can be reported without a terminal.
//! Lines carry no personal data beyond the error itself: the home folder is written as `~`.

use std::io::Write;
use std::path::PathBuf;
use std::sync::Mutex;

/// Past this the log moves to `errors.1.log` and starts again.
const MAX_BYTES: u64 = 512 * 1024;
/// Lines in a copied report.
const REPORT_LINES: usize = 60;

static LOCK: Mutex<()> = Mutex::new(());

pub fn dir() -> PathBuf {
    crate::platform::data_dir().join("logs")
}

fn path() -> PathBuf {
    dir().join("errors.log")
}

/// The home folder names the user: it is replaced with `~`.
fn scrub(text: &str) -> String {
    let one_line = text.replace(['\n', '\r'], " ");
    match dirs::home_dir().map(|h| h.to_string_lossy().into_owned()).filter(|h| h.len() > 1) {
        Some(home) => one_line.replace(&home, "~"),
        None => one_line,
    }
}

/// Appends a line; failing to write the log is not worth bothering anyone about.
pub fn record(what: &str) {
    eprintln!("glassgram: {what}");
    let _guard = LOCK.lock();
    let path = path();
    let _ = std::fs::create_dir_all(dir());
    if std::fs::metadata(&path).is_ok_and(|m| m.len() > MAX_BYTES) {
        let _ = std::fs::rename(&path, dir().join("errors.1.log"));
    }
    let fresh = !path.exists();
    if let Ok(mut f) = std::fs::OpenOptions::new().create(true).append(true).open(&path) {
        let _ = writeln!(f, "{} {}", jiff::Timestamp::now().strftime("%Y-%m-%d %H:%M:%S"), scrub(what));
        if fresh {
            crate::backend::make_private(&path);
        }
    }
}

/// Panics go to the log too, then to the default handler.
pub fn install_panic_hook() {
    let default = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        let at = info.location().map(|l| format!(" at {}:{}", l.file(), l.line())).unwrap_or_default();
        let what = info.payload().downcast_ref::<&str>().map(|s| s.to_string()).or_else(|| info.payload().downcast_ref::<String>().cloned()).unwrap_or_default();
        record(&format!("panic{at}: {what}"));
        default(info);
    }));
}

/// A report for the developer: version, system and the last lines of the log.
pub fn report() -> String {
    let log = std::fs::read_to_string(path()).unwrap_or_default();
    let lines: Vec<&str> = log.lines().collect();
    let tail = lines[lines.len().saturating_sub(REPORT_LINES)..].join("\n");
    format!("Glassgram {} · {} {}\n\n{tail}", env!("CARGO_PKG_VERSION"), std::env::consts::OS, std::env::consts::ARCH)
}

/// How many lines the log has.
pub fn count() -> usize {
    std::fs::read_to_string(path()).map(|s| s.lines().count()).unwrap_or(0)
}

pub fn clear() {
    let _ = std::fs::remove_file(path());
    let _ = std::fs::remove_file(dir().join("errors.1.log"));
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn scrubs_home_and_newlines() {
        let home = dirs::home_dir().unwrap_or_default().to_string_lossy().into_owned();
        let s = scrub(&format!("file gone: {home}/Downloads/a.txt\nnext"));
        assert!(!s.contains('\n'));
        if home.len() > 1 {
            assert!(s.contains("~/Downloads") && !s.contains(&home));
        }
    }
}
