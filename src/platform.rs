//! What differs between Linux, Windows and macOS: folders, opening files, external tools, notifications.
//! The rest of the code goes through here instead of calling OS tools (`xdg-open`, `notify-send`…) directly.

use std::path::PathBuf;
use std::process::Command;

/// Session, cache and settings: `~/.local/share/glassgram` (or `$XDG_DATA_HOME`) on Linux,
/// `%APPDATA%\glassgram` on Windows, `~/Library/Application Support/glassgram` on macOS.
pub fn data_dir() -> PathBuf {
    dirs::data_dir().unwrap_or_else(|| PathBuf::from(".")).join("glassgram")
}

/// The user's Downloads folder (localised names included, e.g. «Завантаження»).
pub fn download_dir() -> PathBuf {
    dirs::download_dir().or_else(|| dirs::home_dir().map(|h| h.join("Downloads"))).unwrap_or_else(|| PathBuf::from("."))
}

/// Opens a file or link in the default application without waiting for it.
pub fn open(target: impl AsRef<std::ffi::OsStr>) {
    let _ = open::that_detached(target);
}

/// `ffmpeg` / `ffplay`: next to our executable first (how they ship with the app on Windows and macOS),
/// then the usual places. On Windows the console window of the tool is hidden.
pub fn tool(name: &str) -> Command {
    let exe = format!("{name}{}", std::env::consts::EXE_SUFFIX);
    #[cfg_attr(not(target_os = "macos"), allow(unused_mut))]
    let mut places: Vec<PathBuf> = std::env::current_exe().ok().and_then(|p| p.parent().map(|d| vec![d.join(&exe)])).unwrap_or_default();
    // An app started from Finder doesn't get the shell's PATH, so Homebrew's folders are checked by hand.
    #[cfg(target_os = "macos")]
    places.extend(["/opt/homebrew/bin", "/usr/local/bin"].map(|d| PathBuf::from(d).join(&exe)));
    let path = places.into_iter().find(|p| p.is_file()).unwrap_or_else(|| PathBuf::from(name));
    #[allow(unused_mut)]
    let mut cmd = Command::new(path);
    #[cfg(target_os = "windows")]
    {
        use std::os::windows::process::CommandExt;
        const CREATE_NO_WINDOW: u32 = 0x0800_0000;
        cmd.creation_flags(CREATE_NO_WINDOW);
    }
    cmd
}

/// Whether `ffmpeg` / `ffplay` can be started at all.
pub fn has_tool(name: &str) -> bool {
    tool(name).arg("-version").stdout(std::process::Stdio::null()).stderr(std::process::Stdio::null()).status().is_ok_and(|s| s.success())
}

/// System notification (D-Bus on Linux, toast on Windows, Notification Center on macOS).
/// Shown from a short-lived thread: the call may take a moment and the UI must not wait.
pub fn notify(title: &str, body: &str) {
    let title = title.to_owned();
    let body: String = body.chars().take(180).collect();
    std::thread::spawn(move || {
        let mut n = notify_rust::Notification::new();
        n.appname("Glassgram").summary(&title).body(&body);
        #[cfg(target_os = "linux")]
        n.icon("mail-message-new");
        let _ = n.show();
    });
}
