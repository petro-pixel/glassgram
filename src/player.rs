//! Voice and audio player: sound is played by `ffplay` in a background process
//! (it handles Opus/OGG, MP3, M4A without codecs of our own).
//! Pause stops the process and remembers the position; resume and seek
//! restart it from the right place.

use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::time::Instant;

use grammers_client::session::types::PeerId;

pub const SPEEDS: [f32; 3] = [1.0, 1.5, 2.0];

pub struct Player {
    /// What is loaded now: (chat, message).
    pub current: Option<(PeerId, i32)>,
    path: PathBuf,
    duration: f64,
    /// Position (s) at the last start or pause.
    offset: f64,
    started: Option<Instant>,
    child: Option<Child>,
    pub speed: f32,
    /// Run counter: video restarts its frames when it changes.
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

    /// Current position in seconds.
    pub fn position(&self) -> f64 {
        let run = self.started.map_or(0.0, |t| t.elapsed().as_secs_f64() * self.speed as f64);
        (self.offset + run).min(self.duration.max(0.0))
    }

    /// Current run number and the position it started from.
    pub fn run(&self) -> (u64, f64) {
        (self.run, self.offset)
    }

    /// Fraction played (0..1).
    pub fn progress(&self) -> f32 {
        if self.duration > 0.0 { (self.position() / self.duration) as f32 } else { 0.0 }
    }

    /// Start a new message from the beginning.
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

    /// Seek to fraction `frac` of the duration.
    pub fn seek(&mut self, frac: f32) {
        let was = self.playing();
        self.kill();
        self.offset = self.duration * frac.clamp(0.0, 1.0) as f64;
        if was {
            self.spawn();
        }
    }

    /// Next speed in the cycle 1× → 1.5× → 2×.
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

    /// Checks whether playback reached the end. Returns the message that finished.
    pub fn poll(&mut self) -> Option<(PeerId, i32)> {
        if let Some(c) = &mut self.child {
            if !matches!(c.try_wait(), Ok(Some(_))) {
                return None;
            }
            self.child = None;
        }
        // Video without an audio track: ffplay exits immediately, while time keeps running silently.
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

/// Whether `ffplay` is in the system.
pub fn available() -> bool {
    Command::new("ffplay").arg("-version").stdout(Stdio::null()).stderr(Stdio::null()).status().is_ok_and(|s| s.success())
}

/// “1:05” from seconds.
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
