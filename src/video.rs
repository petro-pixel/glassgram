//! Вбудоване відео: `ffmpeg` декодує кадри в RGBA потрібного розміру, окремий потік
//! читає їх із каналу, а UI показує кадр, коли настає його час (фіксовані 30 кадрів/с).
//! Звук до відео грає `Player` — тут лише картинка.

use std::io::Read;
use std::path::PathBuf;
use std::process::{Child, Command, Stdio};
use std::sync::mpsc::{Receiver, SyncSender, TryRecvError, sync_channel};
use std::sync::{Arc, Mutex};

use eframe::egui::{self, ColorImage, TextureHandle, TextureOptions};

const FPS: f64 = 30.0;

/// Що і як декодувати.
pub struct Spec {
    pub path: PathBuf,
    /// Розмір кадру в пікселях.
    pub size: [usize; 2],
    /// З якої секунди почати.
    pub from: f64,
    pub speed: f32,
    /// По колу (GIF, кружечки без звуку, відео-стікери).
    pub looped: bool,
    /// Вписати з прозорими полями замість розтягування (стікери).
    pub contain: bool,
}

pub struct Clip {
    /// Номер запуску програвача, з яким синхронізовано цей кліп (0 — без звуку).
    pub run: u64,
    size: [usize; 2],
    rx: Receiver<Vec<u8>>,
    child: Arc<Mutex<Option<Child>>>,
    started: f64,
    frames: u64,
    pub tex: Option<TextureHandle>,
    /// Потік декодування завершився (кінець файлу без повтору або помилка).
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

    /// Забирає кадри, час яких настав, і оновлює текстуру останнім із них.
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

    /// Зупинити декодування, лишивши останній кадр (пауза).
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
        let mut cmd = Command::new("ffmpeg");
        cmd.args(["-loglevel", "quiet", "-nostdin"]);
        // Вбудований декодер VP9 губить альфа-канал — для webm беремо libvpx.
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
                // Кліп закрито.
                kill(&slot);
                return;
            }
            sent += 1;
        }
        // Кінець файлу або кліп зупинено (процес уже прибрано зі слота).
        let Some(mut c) = slot.lock().unwrap().take() else { return };
        let _ = c.wait();
        if !spec.looped || sent == 0 {
            return;
        }
        from = 0.0;
    }
}
