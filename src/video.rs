//! Embedded video: `ffmpeg` decodes frames to RGBA at the needed size, a separate thread
//! reads them from a channel, and the UI shows a frame when its time comes (fixed 30 fps).
//! Video sound is played by `Player`; this is picture only.

use std::io::Read;
use std::path::PathBuf;
use std::process::{Child, Stdio};
use std::sync::mpsc::{Receiver, SyncSender, TryRecvError, sync_channel};
use std::sync::{Arc, Mutex};

use eframe::egui::{self, ColorImage, TextureHandle, TextureOptions};

const FPS: f64 = 30.0;

/// What to decode and how.
pub struct Spec {
    pub path: PathBuf,
    /// Frame size in pixels.
    pub size: [usize; 2],
    /// Second to start from.
    pub from: f64,
    pub speed: f32,
    /// Loop (GIFs, silent round videos, video stickers).
    pub looped: bool,
    /// Fit with transparent margins instead of stretching (stickers).
    pub contain: bool,
}

pub struct Clip {
    /// Player run number this clip is synced with (0 means no sound).
    pub run: u64,
    size: [usize; 2],
    rx: Receiver<Vec<u8>>,
    child: Arc<Mutex<Option<Child>>>,
    started: f64,
    frames: u64,
    pub tex: Option<TextureHandle>,
    /// The decoding thread finished (end of file without looping, or an error).
    pub done: bool,
}

impl Clip {
    pub fn start(spec: Spec, run: u64, now: f64) -> Self {
        let size = spec.size;
        let (tx, rx) = sync_channel(4);
        let child = Arc::new(Mutex::new(None));
        let slot = child.clone();
        std::thread::spawn(move || decode(spec, tx, slot));
        Self { run, size, rx, child, started: now, frames: 0, tex: None, done: false }
    }

    /// An animated (`.tgs`) sticker: frames drawn by `lottie.rs` on a thread of its own, at the
    /// same pace as video frames. `None`: the file is not an animation we can draw.
    pub fn lottie(path: &std::path::Path, side: usize, looped: bool, now: f64) -> Option<Self> {
        let data = std::fs::read(path).ok()?;
        let mut anim = crate::lottie::Lottie::load(&data, side as u16).ok()?;
        let (tx, rx) = sync_channel(4);
        std::thread::spawn(move || {
            let length = anim.duration();
            let mut k = 0u64;
            loop {
                let mut t = k as f64 / FPS;
                if t > length {
                    if !looped {
                        return;
                    }
                    k = 0;
                    t = 0.0;
                }
                // The clip is gone: so is the reason to draw.
                if tx.send(anim.frame(t)).is_err() {
                    return;
                }
                k += 1;
            }
        });
        Some(Self { run: 0, size: [side, side], rx, child: Arc::new(Mutex::new(None)), started: now, frames: 0, tex: None, done: false })
    }

    /// Takes frames whose time has come and updates the texture with the latest one.
    pub fn advance(&mut self, ctx: &egui::Context, key: &str, now: f64) {
        if self.done {
            return;
        }
        let due = ((now - self.started) * FPS) as u64;
        let mut latest = None;
        while self.frames <= due {
            match self.rx.try_recv() {
                Ok(f) => {
                    latest = Some(f);
                    self.frames += 1;
                }
                Err(TryRecvError::Empty) => break,
                Err(TryRecvError::Disconnected) => {
                    self.done = true;
                    break;
                }
            }
        }
        let Some(rgba) = latest else { return };
        let img = ColorImage::from_rgba_unmultiplied(self.size, &rgba);
        match &mut self.tex {
            Some(t) => t.set(img, TextureOptions::LINEAR),
            None => self.tex = Some(ctx.load_texture(key, img, TextureOptions::LINEAR)),
        }
    }

    /// Frame size in pixels.
    pub fn size(&self) -> [usize; 2] {
        self.size
    }

    /// Stop decoding, keeping the last frame (pause).
    pub fn stop(&mut self) {
        kill(&self.child);
        self.done = true;
    }
}

impl Drop for Clip {
    fn drop(&mut self) {
        kill(&self.child);
    }
}

fn kill(child: &Mutex<Option<Child>>) {
    if let Some(mut c) = child.lock().unwrap().take() {
        let _ = c.kill();
        let _ = c.wait();
    }
}

fn decode(spec: Spec, tx: SyncSender<Vec<u8>>, slot: Arc<Mutex<Option<Child>>>) {
    let [w, h] = spec.size;
    let scale = if spec.contain {
        format!("scale={w}:{h}:force_original_aspect_ratio=decrease,pad={w}:{h}:(ow-iw)/2:(oh-ih)/2:color=0x00000000")
    } else {
        format!("scale={w}:{h}:flags=bicubic")
    };
    let filter = format!("setpts=PTS/{},fps={FPS},{scale}", spec.speed);
    let mut from = spec.from;
    loop {
        let mut cmd = crate::platform::tool("ffmpeg");
        cmd.args(["-loglevel", "quiet", "-nostdin"]);
        // The built-in VP9 decoder drops the alpha channel, so use libvpx for webm.
        if spec.path.extension().is_some_and(|e| e == "webm") {
            cmd.args(["-c:v", "libvpx-vp9"]);
        }
        if from > 0.05 {
            cmd.arg("-ss").arg(format!("{from:.2}"));
        }
        cmd.arg("-i").arg(&spec.path);
        cmd.args(["-an", "-vf", &filter, "-f", "rawvideo", "-pix_fmt", "rgba", "-"]);
        let Ok(mut child) = cmd.stdin(Stdio::null()).stdout(Stdio::piped()).stderr(Stdio::null()).spawn() else { return };
        let Some(mut out) = child.stdout.take() else { return };
        *slot.lock().unwrap() = Some(child);
        let mut sent = 0;
        loop {
            let mut frame = vec![0u8; w * h * 4];
            if out.read_exact(&mut frame).is_err() {
                break;
            }
            if tx.send(frame).is_err() {
                // Clip closed.
                kill(&slot);
                return;
            }
            sent += 1;
        }
        // End of file or clip stopped (the process has already been removed from the slot).
        let Some(mut c) = slot.lock().unwrap().take() else { return };
        let _ = c.wait();
        if !spec.looped || sent == 0 {
            return;
        }
        from = 0.0;
    }
}
