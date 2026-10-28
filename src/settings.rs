//! App settings: a simple `key=value` file in the data folder.

use crate::backend::data_dir;
use crate::i18n::{l};

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Theme {
    /// Follow the system (light or dark desktop scheme).
    System,
    Dark,
    Light,
}

/// How tall chat list rows are.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Density {
    Compact,
    Standard,
    Large,
}

impl Density {
    pub fn row_h(self) -> f32 {
        match self {
            Density::Compact => 52.0,
            Density::Standard => 66.0,
            Density::Large => 76.0,
        }
    }

    pub fn avatar_r(self) -> f32 {
        match self {
            Density::Compact => 19.0,
            Density::Standard => 24.0,
            Density::Large => 28.0,
        }
    }

    /// Offsets of the title (up) and the preview (down) from the row's middle.
    pub fn lines(self) -> (f32, f32) {
        match self {
            Density::Compact => (19.0, 1.0),
            Density::Standard => (21.0, 3.0),
            Density::Large => (23.0, 5.0),
        }
    }

    fn code(self) -> &'static str {
        match self {
            Density::Compact => "compact",
            Density::Standard => "standard",
            Density::Large => "large",
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct Settings {
    pub theme: Theme,
    pub lang: crate::i18n::Lang,
    /// UI scale (multiplier on top of screen density).
    pub scale: f32,
    pub notifications: bool,
    /// GIFs, round videos and video stickers autoplay.
    pub autoplay: bool,
    /// SOCKS5 proxy (`socks5://[user:password@]host:port`), empty means no proxy.
    pub proxy: String,
    /// Local passcode (`lock::make`), empty means disabled.
    pub passcode: String,
    /// Lock after this many minutes of inactivity (0 means only at startup).
    pub lock_minutes: u32,
    /// Camera for video messages (`camera::Camera::id`), empty means the first one found.
    pub camera: String,
    /// Folders as a vertical column on the left, as in Telegram Desktop, instead of tabs above the list.
    pub folders_side: bool,
    /// Message text size in points (one of `TEXT_SIZES`).
    pub text_size: f32,
    /// Send with Ctrl+Enter (Cmd+Enter on macOS); Enter then breaks the line.
    pub ctrl_enter: bool,
    /// Video stickers play over and over; off — once each time they come into view.
    pub loop_stickers: bool,
    /// Stickers for an emoji typed alone in the field, above it.
    pub suggest_stickers: bool,
    pub density: Density,
}

impl Default for Settings {
    fn default() -> Self {
        Self { theme: Theme::System, lang: crate::i18n::Lang::System, scale: 1.0, notifications: true, autoplay: true, proxy: String::new(), passcode: String::new(), lock_minutes: 0, camera: String::new(), folders_side: false, text_size: 15.0, ctrl_enter: false, loop_stickers: true, suggest_stickers: true, density: Density::Standard }
    }
}

pub const SCALES: [f32; 6] = [0.8, 0.9, 1.0, 1.1, 1.25, 1.5];
pub const TEXT_SIZES: [f32; 4] = [13.0, 15.0, 17.0, 19.0];

impl Settings {
    pub fn load() -> Self {
        let mut s = Self::default();
        let Ok(text) = std::fs::read_to_string(data_dir().join("settings")) else { return s };
        for line in text.lines() {
            let Some((k, v)) = line.split_once('=') else { continue };
            let (k, v) = (k.trim(), v.trim());
            let flag = v == "1" || v == "true";
            match k {
                "lang" => {
                    s.lang = crate::i18n::Lang::from_code(v);
                }
                "theme" => {
                    s.theme = match v {
                        "dark" => Theme::Dark,
                        "light" => Theme::Light,
                        _ => Theme::System,
                    }
                }
                "scale" => s.scale = v.parse().unwrap_or(1.0f32).clamp(0.5, 2.0),
                "notifications" => s.notifications = flag,
                "autoplay" => s.autoplay = flag,
                "proxy" => s.proxy = v.to_owned(),
                "passcode" => s.passcode = v.to_owned(),
                "camera" => s.camera = v.to_owned(),
                "lock_minutes" => s.lock_minutes = v.parse().unwrap_or(0),
                "folders_side" => s.folders_side = flag,
                "text_size" => s.text_size = v.parse().ok().filter(|v| TEXT_SIZES.contains(v)).unwrap_or(15.0),
                "ctrl_enter" => s.ctrl_enter = flag,
                "loop_stickers" => s.loop_stickers = flag,
                "suggest_stickers" => s.suggest_stickers = flag,
                "density" => {
                    s.density = match v {
                        "compact" => Density::Compact,
                        "large" => Density::Large,
                        _ => Density::Standard,
                    }
                }
                _ => {}
            }
        }
        s
    }

    pub fn save(&self) {
        let dir = data_dir();
        let _ = std::fs::create_dir_all(&dir);
        let b = |v: bool| if v { "1" } else { "0" };
        let text = format!(
            "lang={}\ntheme={}\nscale={}\nnotifications={}\nautoplay={}\nproxy={}\npasscode={}\nlock_minutes={}\ncamera={}\nfolders_side={}\ntext_size={}\nctrl_enter={}\nloop_stickers={}\nsuggest_stickers={}\ndensity={}\n",
            self.lang.code(),
            match self.theme {
                Theme::System => "system",
                Theme::Dark => "dark",
                Theme::Light => "light",
            },
            self.scale,
            b(self.notifications),
            b(self.autoplay),
            self.proxy.trim(),
            self.passcode,
            self.lock_minutes,
            self.camera,
            b(self.folders_side),
            self.text_size,
            b(self.ctrl_enter),
            b(self.loop_stickers),
            b(self.suggest_stickers),
            self.density.code()
        );
        // Holds the local passcode hash, so owner-only.
        let _ = std::fs::write(dir.join("settings"), text);
        crate::backend::make_private(&dir.join("settings"));
    }
}

/// Validates a proxy address: socks5 scheme, host and port. `None` means it's fine.
pub fn proxy_error(url: &str) -> Option<&'static str> {
    let url = url.trim();
    if url.is_empty() {
        return None;
    }
    let Some(rest) = url.strip_prefix("socks5://") else { return Some(l!("Адреса має починатися з socks5://", "The address must start with socks5://")) };
    let host_port = rest.rsplit('@').next().unwrap_or(rest);
    match host_port.rsplit_once(':') {
        Some((host, port)) if !host.is_empty() && port.parse::<u16>().is_ok_and(|p| p > 0) => None,
        _ => Some(l!("Потрібні хост і порт, напр. socks5://127.0.0.1:1080", "Host and port are required, e.g. socks5://127.0.0.1:1080")),
    }
}

#[cfg(test)]
mod tests {
    use super::proxy_error;

    #[test]
    fn proxy_validation() {
        assert!(proxy_error("").is_none());
        assert!(proxy_error("socks5://127.0.0.1:1080").is_none());
        assert!(proxy_error("socks5://user:pass@example.com:5678").is_none());
        assert!(proxy_error("http://127.0.0.1:1080").is_some());
        assert!(proxy_error("socks5://127.0.0.1").is_some());
    }
}
