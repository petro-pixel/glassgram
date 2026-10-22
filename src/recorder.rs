//! Запис голосових: `ffmpeg` бере звук із PulseAudio/PipeWire і пише OGG/Opus,
//! а паралельно віддає PCM 8 кГц у stdout — з нього рахуємо рівень для індикатора
//! і хвильову форму для Telegram.

use std::io::{Read, Write};
use std::path::PathBuf;
use std::process::{Child, Command, Stdio};
use std::sync::{Arc, Mutex};
use std::time::Instant;
use crate::i18n::{l, lf};

/// Скільки семплів (8 кГц) в одному стовпчику рівня — 50 мс.
const CHUNK: usize = 400;
/// Коротші записи не надсилаємо, як Telegram Desktop.
pub const MIN_SECS: f64 = 0.7;
/// Стовпчиків у хвильовій формі повідомлення.
const WAVE_BARS: usize = 100;

pub struct Recorder {
    child: Child,
    started: Instant,
    path: PathBuf,
    /// Пікові рівні 0..1 кожні 50 мс.
    levels: Arc<Mutex<Vec<f32>>>,
}

/// Готовий запис.
pub struct Voice {
    pub path: PathBuf,
    pub duration: f64,
    /// Рівні 0..31.
    pub waveform: Vec<u8>,
}

impl Voice {
    /// Видалити запис, не надсилаючи.
    pub fn discard(self) {
        let _ = std::fs::remove_file(&self.path);
    }
}

impl Recorder {
    pub fn start() -> Result<Self, String> {
        let dir = std::env::temp_dir();
        let path = dir.join(format!("glassgram-voice-{}-{}.ogg", std::process::id(), jiff::Timestamp::now().as_millisecond()));
        let mut child = Command::new("ffmpeg")
            .args(["-loglevel", "error", "-f", "pulse", "-i", "default", "-ac", "1", "-ar", "48000"])
            .args(["-c:a", "libopus", "-b:a", "32k", "-application", "voip", "-y"])
            .arg(&path)
            .args(["-ac", "1", "-ar", "8000", "-f", "s16le", "pipe:1"])
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()
            .map_err(|e| lf!("Не вдалося запустити ffmpeg: {e}", "Could not start ffmpeg: {e}"))?;
        let levels = Arc::new(Mutex::new(Vec::new()));
        let mut out = child.stdout.take().ok_or(l!("ffmpeg без stdout", "ffmpeg without stdout"))?;
        let sink = levels.clone();
        std::thread::spawn(move || {
            let mut buf = vec![0u8; CHUNK * 2];
            while out.read_exact(&mut buf).is_ok() {
                let peak = buf.chunks_exact(2).map(|b| i16::from_le_bytes([b[0], b[1]]).unsigned_abs()).max().unwrap_or(0);
                sink.lock().unwrap().push(peak as f32 / i16::MAX as f32);
            }
        });
        Ok(Self { child, started: Instant::now(), path, levels })
    }

    pub fn elapsed(&self) -> f64 {
        self.started.elapsed().as_secs_f64()
    }

    /// Останні `n` рівнів (0..1) для живого індикатора.
    pub fn recent(&self, n: usize) -> Vec<f32> {
        let l = self.levels.lock().unwrap();
        l[l.len().saturating_sub(n)..].to_vec()
    }

    /// Чи ffmpeg ще пише (інакше — немає мікрофона чи PulseAudio).
    pub fn alive(&mut self) -> bool {
        matches!(self.child.try_wait(), Ok(None))
    }

    /// Зупинити й дописати файл. `None` — запис закороткий або не вдався.
    pub fn finish(mut self) -> Option<Voice> {
        let duration = self.elapsed();
        // «q» — штатна зупинка ffmpeg: він закриває контейнер OGG коректно.
        if let Some(mut stdin) = self.child.stdin.take() {
            let _ = stdin.write_all(b"q");
        }
        let ok = self.child.wait().is_ok_and(|s| s.success());
        let levels = self.levels.lock().unwrap().clone();
        if !ok || duration < MIN_SECS || std::fs::metadata(&self.path).map_or(true, |m| m.len() == 0) {
            let _ = std::fs::remove_file(&self.path);
            return None;
        }
        Some(Voice { path: self.path.clone(), duration, waveform: waveform(&levels) })
    }

    pub fn cancel(mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
        let _ = std::fs::remove_file(&self.path);
    }
}

impl Drop for Recorder {
    fn drop(&mut self) {
        // Запис не завершено (вікно закрили посеред запису) — прибираємо недописаний файл.
        if matches!(self.child.try_wait(), Ok(None)) {
            let _ = self.child.kill();
            let _ = self.child.wait();
            let _ = std::fs::remove_file(&self.path);
        }
    }
}

/// Пікові рівні → `WAVE_BARS` значень 0..31, нормованих за найгучнішим місцем.
fn waveform(levels: &[f32]) -> Vec<u8> {
    if levels.is_empty() {
        return Vec::new();
    }
    let n = WAVE_BARS.min(levels.len());
    let bars: Vec<f32> = (0..n)
        .map(|k| {
            let (a, b) = (k * levels.len() / n, ((k + 1) * levels.len() / n).max(k * levels.len() / n + 1));
            levels[a..b].iter().copied().fold(0.0, f32::max)
        })
        .collect();
    let max = bars.iter().copied().fold(0.0, f32::max).max(0.05);
    bars.iter().map(|v| (v / max * 31.0).round() as u8).collect()
}

/// Чи є `ffmpeg` для запису.
pub fn available() -> bool {
    Command::new("ffmpeg").arg("-version").stdout(Stdio::null()).stderr(Stdio::null()).status().is_ok_and(|s| s.success())
}

#[cfg(test)]
mod tests {
    #[test]
    fn waveform_normalizes_to_31() {
        let w = super::waveform(&[0.1, 0.5, 0.25, 0.0]);
        assert_eq!(w, vec![6, 31, 16, 0]);
    }
}
