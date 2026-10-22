//! Програвач голосових і аудіо: звук відтворює `ffplay` у фоновому процесі
//! (він уміє Opus/OGG, MP3, M4A без власних кодеків у програмі).
//! Пауза зупиняє процес і запам'ятовує позицію; продовження й перемотування
//! перезапускають його з потрібного місця.

use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::time::Instant;

use grammers_client::session::types::PeerId;

pub const SPEEDS: [f32; 3] = [1.0, 1.5, 2.0];

pub struct Player {
    /// Що зараз завантажено: (чат, повідомлення).
    pub current: Option<(PeerId, i32)>,
    path: PathBuf,
    duration: f64,
    /// Позиція (с) на момент останнього запуску чи паузи.
    offset: f64,
    started: Option<Instant>,
    child: Option<Child>,
    pub speed: f32,
    /// Лічильник запусків: відео перезапускає кадри, коли він змінюється.
    run: u64,
}

impl Player {
    pub fn new() -> Self {
        Self { current: None, path: PathBuf::new(), duration: 0.0, offset: 0.0, started: None, child: None, speed: 1.0, run: 0 }
    }

    pub fn is(&self, peer: PeerId, id: i32) -> bool {
        self.current == Some((peer, id))
    }

    pub fn playing(&self) -> bool {
        self.started.is_some()
    }

    /// Поточна позиція в секундах.
    pub fn position(&self) -> f64 {
        let run = self.started.map_or(0.0, |t| t.elapsed().as_secs_f64() * self.speed as f64);
        (self.offset + run).min(self.duration.max(0.0))
    }

    /// Номер поточного запуску і позиція, з якої він почався.
    pub fn run(&self) -> (u64, f64) {
        (self.run, self.offset)
    }

    /// Частка програного (0..1).
    pub fn progress(&self) -> f32 {
        if self.duration > 0.0 { (self.position() / self.duration) as f32 } else { 0.0 }
    }

    /// Почати нове повідомлення з початку.
    pub fn play(&mut self, peer: PeerId, id: i32, path: &Path, duration: f64) {
        self.stop();
        self.current = Some((peer, id));
        self.path = path.to_owned();
        self.duration = duration;
        self.offset = 0.0;
        self.spawn();
    }

    pub fn pause(&mut self) {
        self.offset = self.position();
        self.kill();
    }

    pub fn resume(&mut self) {
        if self.current.is_some() && !self.playing() {
            self.spawn();
        }
    }

    /// Перемотати до частки `frac` тривалості.
    pub fn seek(&mut self, frac: f32) {
        let was = self.playing();
        self.kill();
        self.offset = self.duration * frac.clamp(0.0, 1.0) as f64;
        if was {
            self.spawn();
        }
    }

    /// Наступна швидкість по колу 1× → 1.5× → 2×.
    pub fn cycle_speed(&mut self) {
        let i = SPEEDS.iter().position(|s| *s == self.speed).unwrap_or(0);
        let was = self.playing();
        self.offset = self.position();
        self.kill();
        self.speed = SPEEDS[(i + 1) % SPEEDS.len()];
        if was {
            self.spawn();
        }
    }

    pub fn stop(&mut self) {
        self.kill();
        self.current = None;
        self.offset = 0.0;
    }

    /// Перевіряє, чи дограно до кінця. Повертає повідомлення, що закінчилось.
    pub fn poll(&mut self) -> Option<(PeerId, i32)> {
        if let Some(c) = &mut self.child {
            if !matches!(c.try_wait(), Ok(Some(_))) {
                return None;
            }
            self.child = None;
        }
        // Відео без звукової доріжки: ffplay виходить одразу, а час іде далі беззвучно.
        if self.started.is_none() || self.position() < self.duration - 0.3 {
            return None;
        }
        self.started = None;
        let finished = self.current.take();
        self.offset = 0.0;
        finished
    }

    fn spawn(&mut self) {
        let mut cmd = Command::new("ffplay");
        cmd.args(["-nodisp", "-autoexit", "-vn", "-loglevel", "quiet"]);
        if self.offset > 0.05 {
            cmd.arg("-ss").arg(format!("{:.2}", self.offset));
        }
        if self.speed != 1.0 {
            cmd.arg("-af").arg(format!("atempo={}", self.speed));
        }
        cmd.arg(&self.path).stdin(Stdio::null()).stdout(Stdio::null()).stderr(Stdio::null());
        match cmd.spawn() {
            Ok(child) => {
                self.run += 1;
                self.child = Some(child);
                self.started = Some(Instant::now());
            }
            Err(e) => {
                eprintln!("glassgram: ffplay: {e}");
                self.current = None;
            }
        }
    }

    fn kill(&mut self) {
        if let Some(mut c) = self.child.take() {
            let _ = c.kill();
            let _ = c.wait();
        }
        self.started = None;
    }
}

impl Drop for Player {
    fn drop(&mut self) {
        self.kill();
    }
}

/// Чи є `ffplay` у системі.
pub fn available() -> bool {
    Command::new("ffplay").arg("-version").stdout(Stdio::null()).stderr(Stdio::null()).status().is_ok_and(|s| s.success())
}

/// «1:05» з секунд.
pub fn clock(secs: f64) -> String {
    let s = secs.max(0.0).round() as u64;
    if s >= 3600 { format!("{}:{:02}:{:02}", s / 3600, s / 60 % 60, s % 60) } else { format!("{}:{:02}", s / 60, s % 60) }
}

#[cfg(test)]
mod tests {
    #[test]
    fn clock_formats() {
        assert_eq!(super::clock(65.4), "1:05");
        assert_eq!(super::clock(3725.0), "1:02:05");
    }
}
