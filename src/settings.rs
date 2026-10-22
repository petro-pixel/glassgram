//! Налаштування програми: простий файл `ключ=значення` у теці даних.

use crate::backend::data_dir;
use crate::i18n::{l};

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Theme {
    /// Як у системі (світла чи темна схема робочого столу).
    System,
    Dark,
    Light,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Settings {
    pub theme: Theme,
    pub lang: crate::i18n::Lang,
    /// Масштаб інтерфейсу (множник до щільності екрана).
    pub scale: f32,
    pub notifications: bool,
    /// GIF, кружечки й відео-стікери грають самі.
    pub autoplay: bool,
    /// SOCKS5-проксі (`socks5://[логін:пароль@]хост:порт`), порожній — без проксі.
    pub proxy: String,
    /// Локальний пароль (`lock::make`), порожній — вимкнено.
    pub passcode: String,
    /// Блокувати після стількох хвилин бездіяльності (0 — лише на старті).
    pub lock_minutes: u32,
    /// Камера для відеоповідомлень (`camera::Camera::id`), порожня — перша знайдена.
    pub camera: String,
}

impl Default for Settings {
    fn default() -> Self {
        Self { theme: Theme::System, lang: crate::i18n::Lang::System, scale: 1.0, notifications: true, autoplay: true, proxy: String::new(), passcode: String::new(), lock_minutes: 0, camera: String::new() }
    }
}

pub const SCALES: [f32; 6] = [0.8, 0.9, 1.0, 1.1, 1.25, 1.5];

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
                    s.lang = match v {
                        "uk" => crate::i18n::Lang::Uk,
                        "en" => crate::i18n::Lang::En,
                        _ => crate::i18n::Lang::System,
                    }
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
            "lang={}\ntheme={}\nscale={}\nnotifications={}\nautoplay={}\nproxy={}\npasscode={}\nlock_minutes={}\ncamera={}\n",
            match self.lang {
                crate::i18n::Lang::System => "system",
                crate::i18n::Lang::Uk => "uk",
                crate::i18n::Lang::En => "en",
            },
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
            self.camera
        );
        // Тут хеш локального пароля — тільки для власника.
        let _ = std::fs::write(dir.join("settings"), text);
        crate::backend::make_private(&dir.join("settings"));
    }
}

/// Перевірка адреси проксі: схема socks5, хост і порт. `None` — усе гаразд.
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
