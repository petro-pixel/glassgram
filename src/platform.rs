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

/// Opens one of our own files or folders in the default application without waiting for it.
/// Never pass it anything that came from the network: links go through [`open_url`], received
/// files through [`open_file`].
pub fn open(target: impl AsRef<std::ffi::OsStr>) {
    let _ = open::that_detached(target);
}

/// A path for a temporary file (recordings, a cropped avatar) in this process's own folder inside
/// the system temp dir. The folder is created fresh and owner-only, so other users can't read the
/// recordings or plant a link under a predictable name.
pub fn temp_file(name: &str) -> PathBuf {
    static DIR: std::sync::OnceLock<PathBuf> = std::sync::OnceLock::new();
    let dir = DIR.get_or_init(|| {
        let base = std::env::temp_dir();
        let nanos = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.subsec_nanos()).unwrap_or_default();
        // `create_dir` fails on anything already there, so a pre-made folder or link is never reused.
        (0..16u32)
            .map(|k| base.join(format!("glassgram-{}-{:x}", std::process::id(), nanos.wrapping_add(k.wrapping_mul(7919)))))
            .find(|d| std::fs::create_dir(d).is_ok())
            .inspect(|d| crate::backend::make_private(d))
            .unwrap_or(base)
    });
    let stamp = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_millis()).unwrap_or_default();
    dir.join(format!("{stamp}-{name}"))
}

/// Link schemes we hand to the system. Anything else (`file:`, `smb:`, `search-ms:`, `ms-…:`
/// and other protocol handlers) can run programs or leak credentials, so it is refused,
/// as in Telegram Desktop.
const URL_SCHEMES: [&str; 4] = ["https", "http", "mailto", "tg"];

/// Whether a link from a message, a bot or a mini app may be opened.
pub fn safe_url(url: &str) -> bool {
    let Some((scheme, rest)) = url.split_once(':') else { return false };
    // Control characters and spaces can smuggle arguments past the opener on some systems.
    let clean = !url.chars().any(|c| c.is_control() || c.is_whitespace());
    clean && !rest.is_empty() && URL_SCHEMES.iter().any(|s| scheme.eq_ignore_ascii_case(s))
}

/// Opens a link from the network in the browser (or mail client); returns `false` if it was refused.
pub fn open_url(url: &str) -> bool {
    let ok = safe_url(url);
    if ok {
        let _ = open::that_detached(url);
    }
    ok
}

/// Extensions the system would run instead of showing: such files are only revealed in their folder.
const RUNNABLE: [&str; 38] = [
    "exe", "msi", "msix", "appx", "bat", "cmd", "com", "scr", "pif", "cpl", "msc", "hta", "ps1", "psm1", "vbs", "vbe", "js", "jse", "wsf", "wsh",
    "lnk", "url", "reg", "jar", "desktop", "sh", "bash", "run", "bin", "appimage", "command", "app", "pkg", "dmg", "deb", "rpm", "py", "pyw",
];

/// Whether opening the file would run it rather than show it.
pub fn runnable(path: &std::path::Path) -> bool {
    path.extension().and_then(|e| e.to_str()).is_some_and(|e| RUNNABLE.iter().any(|r| e.eq_ignore_ascii_case(r)))
}

/// Opens a received file; a program or script is not run but shown in its folder.
/// Returns `false` when only the folder was opened.
pub fn open_file(path: &std::path::Path) -> bool {
    if runnable(path) {
        reveal(path);
        return false;
    }
    let _ = open::that_detached(path);
    true
}

/// Shows the folder a file is in.
pub fn reveal(path: &std::path::Path) {
    if let Some(dir) = path.parent() {
        let _ = open::that_detached(dir);
    }
}

/// Opens a received file even if it is a program: only after the user said so.
pub fn run_file(path: &std::path::Path) {
    let _ = open::that_detached(path);
}

/// A file name from the network made safe to save: only the last path component, no characters
/// Windows forbids, no reserved device names, no leading dots, not too long.
pub fn safe_file_name(name: &str) -> String {
    let last = name.rsplit(['/', '\\']).next().unwrap_or_default();
    let mut out: String = last.chars().map(|c| if c.is_control() || "<>:\"|?*".contains(c) { '_' } else { c }).collect();
    out = out.trim_start_matches(['.', ' ']).trim_end_matches(['.', ' ']).to_owned();
    let stem = out.split('.').next().unwrap_or_default().to_ascii_uppercase();
    let reserved = matches!(stem.as_str(), "CON" | "PRN" | "AUX" | "NUL") || (stem.len() == 4 && (stem.starts_with("COM") || stem.starts_with("LPT")) && stem.as_bytes()[3].is_ascii_digit());
    if reserved {
        out.insert(0, '_');
    }
    // Most file systems allow 255 bytes; keep the extension when cutting.
    if out.len() > 200 {
        let ext = std::path::Path::new(&out).extension().map(|e| format!(".{}", e.to_string_lossy())).filter(|e| e.len() < 20).unwrap_or_default();
        let mut cut = 200 - ext.len();
        while !out.is_char_boundary(cut) {
            cut -= 1;
        }
        out = format!("{}{ext}", &out[..cut]);
    }
    if out.is_empty() { "file".to_owned() } else { out }
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn urls() {
        assert!(safe_url("https://t.me/durov"));
        assert!(safe_url("HTTP://example.com"));
        assert!(safe_url("mailto:a@b.c"));
        assert!(safe_url("tg://resolve?domain=x"));
        for bad in ["file:///etc/passwd", "smb://host/share", "search-ms:query=x", "ms-msdt:/id", "javascript:alert(1)", "/usr/bin/xterm", "C:\\Windows\\System32\\cmd.exe", "https://a b", "https:", "https://x\n-arg"] {
            assert!(!safe_url(bad), "{bad}");
        }
    }

    #[test]
    fn file_names() {
        assert_eq!(safe_file_name("../../.config/autostart/x.desktop"), "x.desktop");
        assert_eq!(safe_file_name("/etc/passwd"), "passwd");
        assert_eq!(safe_file_name("..\\..\\evil.exe"), "evil.exe");
        assert_eq!(safe_file_name("C:\\Windows\\a.txt"), "a.txt");
        assert_eq!(safe_file_name(".bashrc"), "bashrc");
        assert_eq!(safe_file_name("con.txt"), "_con.txt");
        assert_eq!(safe_file_name("a<b>:c?.png"), "a_b__c_.png");
        assert_eq!(safe_file_name(".."), "file");
        assert_eq!(safe_file_name("звіт 2026.pdf"), "звіт 2026.pdf");
        let long = format!("{}.pdf", "я".repeat(300));
        let cut = safe_file_name(&long);
        assert!(cut.len() <= 200 && cut.ends_with(".pdf"));
    }

    #[test]
    fn runnable_files() {
        assert!(runnable(std::path::Path::new("x.EXE")));
        assert!(runnable(std::path::Path::new("x.desktop")));
        assert!(!runnable(std::path::Path::new("x.pdf")));
        assert!(!runnable(std::path::Path::new("noext")));
    }
}
