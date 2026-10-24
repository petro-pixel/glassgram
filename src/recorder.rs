//! Voice recording: `ffmpeg` takes the microphone (`camera::Mic`) and writes OGG/Opus,
//! while also streaming 8 kHz PCM to stdout, used for the level meter
//! and the waveform for Telegram.

use std::io::Read;
use std::path::PathBuf;
use std::process::{Child, Stdio};
use std::sync::{Arc, Mutex};
use std::time::Instant;
use crate::i18n::{l, lf};

/// Samples (8 kHz) per level bar: 50 ms.
const CHUNK: usize = 400;
/// Shorter recordings are not sent, as in Telegram Desktop.
pub const MIN_SECS: f64 = 0.7;
/// Bars in the message waveform.
const WAVE_BARS: usize = 100;

pub struct Recorder {
    child: Child,
    mic: crate::camera::Mic,
    started: Instant,
    path: PathBuf,
    /// Peak levels 0..1 every 50 ms.
    levels: Arc<Mutex<Vec<f32>>>,
}

/// A finished recording.
pub struct Voice {
    pub path: PathBuf,
    pub duration: f64,
    /// Levels 0..31.
    pub waveform: Vec<u8>,
}

impl Voice {
    /// Delete the recording without sending.
    pub fn discard(self) {
        let _ = std::fs::remove_file(&self.path);
    }
}

impl Recorder {
    pub fn start() -> Result<Self, String> {
        let dir = std::env::temp_dir();
        let path = dir.join(format!("glassgram-voice-{}-{}.ogg", std::process::id(), jiff::Timestamp::now().as_millisecond()));
        let mut mic = crate::camera::Mic::open()?;
        let mut child = crate::platform::tool("ffmpeg")
            .args(["-loglevel", "error"])
            .args(&mic.args)
            .args(["-ac", "1", "-ar", "48000"])
            .args(["-c:a", "libopus", "-b:a", "32k", "-application", "voip", "-y"])
            .arg(&path)
            .args(["-ac", "1", "-ar", "8000", "-f", "s16le", "pipe:1"])
            .stdin(mic.stdin())
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
        Ok(Self { child, mic, started: Instant::now(), path, levels })
    }

    pub fn elapsed(&self) -> f64 {
        self.started.elapsed().as_secs_f64()
    }

    /// Last `n` levels (0..1) for the live meter.
    pub fn recent(&self, n: usize) -> Vec<f32> {
        let l = self.levels.lock().unwrap();
        l[l.len().saturating_sub(n)..].to_vec()
    }

    /// Whether ffmpeg is still writing (otherwise there is no microphone or PulseAudio).
    pub fn alive(&mut self) -> bool {
        matches!(self.child.try_wait(), Ok(None))
    }

    /// Stop and finalize the file. `None` if the recording is too short or failed.
    pub fn finish(mut self) -> Option<Voice> {
        let duration = self.elapsed();
        let signalled = self.mic.stop(&mut self.child);
        let ok = self.child.wait().is_ok_and(|s| s.success() || signalled);
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
        self.mic.kill();
        let _ = std::fs::remove_file(&self.path);
    }
}

impl Drop for Recorder {
    fn drop(&mut self) {
        // Recording not finished (window closed mid-recording), so remove the partial file.
        if matches!(self.child.try_wait(), Ok(None)) {
            let _ = self.child.kill();
            let _ = self.child.wait();
            let _ = std::fs::remove_file(&self.path);
        }
    }
}

/// Peak levels → `WAVE_BARS` values 0..31, normalized to the loudest point.
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

/// Whether `ffmpeg` is available for recording.
pub fn available() -> bool {
    crate::platform::has_tool("ffmpeg")
}

#[cfg(test)]
mod tests {
    #[test]
    fn waveform_normalizes_to_31() {
        let w = super::waveform(&[0.1, 0.5, 0.25, 0.0]);
        assert_eq!(w, vec![6, 31, 16, 0]);
    }
}
