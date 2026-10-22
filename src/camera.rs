//! Запис відеоповідомлень («кружечків»): `ffmpeg` бере камеру (v4l2) і мікрофон
//! (PulseAudio/PipeWire), обрізає кадр до квадрата й пише MP4 (H.264 + AAC), а паралельно
//! віддає в stdout маленьке дзеркальне прев'ю RGBA для круглого вікна в інтерфейсі.
//!
//! Камеру вибирають у налаштуваннях; `GLASSGRAM_CAMERA` її перекриває (`test` — тестова картинка).
//! Пристрої й формати входу ffmpeg свої на кожній ОС: v4l2 + PulseAudio на Linux,
//! DirectShow на Windows, AVFoundation на macOS.

use std::io::{Read, Write};
use std::path::PathBuf;
use std::process::{Child, Command, Stdio};
use std::sync::{Arc, Mutex};
use std::time::Instant;
use crate::i18n::{l, lf};

/// Сторона записаного відео (як у Telegram).
pub const SIDE: u32 = 384;
/// Сторона прев'ю в пікселях.
pub const PREVIEW: usize = 240;
/// Найдовше відеоповідомлення.
pub const MAX_SECS: f64 = 60.0;
const MIN_SECS: f64 = 1.0;

/// Камера, яку можна вибрати в налаштуваннях.
#[derive(Debug, Clone, PartialEq)]
pub struct Camera {
    /// Що передати ffmpeg як вхід (шлях, ім'я DirectShow чи індекс AVFoundation).
    pub id: String,
    pub name: String,
}

/// Камери в системі.
#[cfg(target_os = "linux")]
pub fn list() -> Vec<Camera> {
    // Кожна веб-камера має кілька вузлів /dev/video*; знімає кадри той, що з index 0.
    let Ok(dir) = std::fs::read_dir("/sys/class/video4linux") else { return Vec::new() };
    let mut out: Vec<Camera> = dir
        .flatten()
        .filter(|e| std::fs::read_to_string(e.path().join("index")).is_ok_and(|i| i.trim() == "0"))
        .map(|e| Camera {
            id: format!("/dev/{}", e.file_name().to_string_lossy()),
            name: std::fs::read_to_string(e.path().join("name")).map(|n| n.trim().to_owned()).unwrap_or_default(),
        })
        .collect();
    out.sort_by(|a, b| a.id.cmp(&b.id));
    out
}

/// Камери в системі (зі списку пристроїв DirectShow чи AVFoundation, який друкує ffmpeg).
#[cfg(not(target_os = "linux"))]
pub fn list() -> Vec<Camera> {
    ffmpeg_devices(true).into_iter().map(|(id, name)| Camera { id, name }).collect()
}

/// Пристрої з виводу `ffmpeg -list_devices`: (id для `-i`, назва); `video` — камери чи мікрофони.
#[cfg(not(target_os = "linux"))]
fn ffmpeg_devices(video: bool) -> Vec<(String, String)> {
    #[cfg(target_os = "windows")]
    let args = ["-hide_banner", "-list_devices", "true", "-f", "dshow", "-i", "dummy"];
    #[cfg(not(target_os = "windows"))]
    let args = ["-hide_banner", "-f", "avfoundation", "-list_devices", "true", "-i", ""];
    let Ok(out) = Command::new("ffmpeg").args(args).stderr(Stdio::piped()).stdout(Stdio::null()).output() else { return Vec::new() };
    parse_devices(&String::from_utf8_lossy(&out.stderr), video)
}

/// Розбір списку пристроїв ffmpeg для DirectShow (`"Назва" (video)`) і AVFoundation (`[0] Назва`).
#[cfg_attr(target_os = "linux", allow(dead_code))]
fn parse_devices(log: &str, video: bool) -> Vec<(String, String)> {
    let mut out = Vec::new();
    // AVFoundation ділить список заголовками «video devices» / «audio devices».
    let mut section_video = None;
    for line in log.lines() {
        if line.contains("AVFoundation video devices") {
            section_video = Some(true);
        } else if line.contains("AVFoundation audio devices") {
            section_video = Some(false);
        }
        let quoted = line.split('"').nth(1);
        let kind = if line.contains("(video)") { Some(true) } else if line.contains("(audio)") { Some(false) } else { None };
        if let (Some(name), Some(k)) = (quoted, kind) {
            if k == video {
                let prefix = if video { "video" } else { "audio" };
                out.push((format!("{prefix}={name}"), name.to_owned()));
            }
            continue;
        }
        if section_video == Some(video)
            && let Some(rest) = line.split("] [").nth(1)
            && let Some((idx, name)) = rest.split_once("] ")
            && idx.chars().all(|c| c.is_ascii_digit())
        {
            out.push((idx.to_owned(), name.trim().to_owned()));
        }
    }
    out
}

/// Аргументи ffmpeg для входу камери (відео) і мікрофона (аудіо) — по одному входу кожен.
fn input_args(camera: &str) -> Vec<String> {
    let s = |v: &[&str]| v.iter().map(|x| x.to_string()).collect::<Vec<_>>();
    let mut a = Vec::new();
    if camera == "test" {
        a.extend(s(&["-re", "-f", "lavfi", "-i", "testsrc2=size=640x480:rate=30"]));
    } else {
        #[cfg(target_os = "linux")]
        // 640×480 при 30 кадрах/с підтримують майже всі веб-камери і в YUYV, і в MJPEG.
        a.extend(s(&["-f", "v4l2", "-framerate", "30", "-video_size", "640x480", "-i", camera]));
        #[cfg(target_os = "windows")]
        a.extend(s(&["-f", "dshow", "-video_size", "640x480", "-i", camera]));
        #[cfg(target_os = "macos")]
        a.extend(s(&["-f", "avfoundation", "-framerate", "30", "-video_size", "640x480", "-i", &format!("{camera}:none")]));
    }
    #[cfg(target_os = "linux")]
    a.extend(s(&["-f", "pulse", "-i", "default"]));
    #[cfg(target_os = "windows")]
    {
        let mic = ffmpeg_devices(false).into_iter().next().map(|d| d.0).unwrap_or_else(|| "audio=default".into());
        a.extend(s(&["-f", "dshow", "-i", &mic]));
    }
    #[cfg(target_os = "macos")]
    a.extend(s(&["-f", "avfoundation", "-i", "none:0"]));
    a
}

pub struct RoundRecorder {
    child: Child,
    started: Instant,
    path: PathBuf,
    /// Останній кадр прев'ю і його номер (щоб не вантажити текстуру двічі).
    frame: Arc<Mutex<(u64, Vec<u8>)>>,
}

pub struct Round {
    pub path: PathBuf,
    pub duration: f64,
}

impl RoundRecorder {
    /// `camera` — id з `list()`; порожній — перша знайдена камера.
    pub fn start(camera: &str) -> Result<Self, String> {
        let path = std::env::temp_dir().join(format!("glassgram-round-{}-{}.mp4", std::process::id(), jiff::Timestamp::now().as_millisecond()));
        let camera = match std::env::var("GLASSGRAM_CAMERA") {
            Ok(c) => c,
            Err(_) if !camera.is_empty() => camera.to_owned(),
            Err(_) => list().into_iter().next().map(|c| c.id).ok_or(l!("Камеру не знайдено", "No camera found"))?,
        };
        let mut cmd = Command::new("ffmpeg");
        cmd.args(["-loglevel", "error"]);
        cmd.args(input_args(&camera));
        let side = SIDE;
        let pv = PREVIEW;
        // Квадрат по центру кадру; прев'ю дзеркальне, як у фронтальній камері, запис — ні.
        let graph = format!(
            "[0:v]crop='min(iw,ih)':'min(iw,ih)',scale={side}:{side},split=2[rec][p];[p]hflip,scale={pv}:{pv},fps=15[prev]"
        );
        cmd.args(["-filter_complex", &graph]);
        cmd.args(["-map", "[rec]", "-map", "1:a", "-c:v", "libx264", "-preset", "veryfast", "-crf", "26", "-pix_fmt", "yuv420p"]);
        cmd.args(["-c:a", "aac", "-b:a", "64k", "-ac", "1", "-t", &MAX_SECS.to_string(), "-movflags", "+faststart", "-y"]);
        cmd.arg(&path);
        // `-t` діє лише на наступний вихід — прев'ю обмежуємо окремо, інакше ffmpeg не завершиться сам.
        cmd.args(["-map", "[prev]", "-t", &MAX_SECS.to_string(), "-f", "rawvideo", "-pix_fmt", "rgba", "pipe:1"]);
        let mut child = cmd
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()
            .map_err(|e| lf!("Не вдалося запустити ffmpeg: {e}", "Could not start ffmpeg: {e}"))?;
        let mut out = child.stdout.take().ok_or(l!("ffmpeg без stdout", "ffmpeg without stdout"))?;
        let frame = Arc::new(Mutex::new((0u64, Vec::new())));
        let sink = frame.clone();
        std::thread::spawn(move || {
            let mut buf = vec![0u8; PREVIEW * PREVIEW * 4];
            while out.read_exact(&mut buf).is_ok() {
                let mut f = sink.lock().unwrap();
                f.0 += 1;
                f.1.clone_from(&buf);
            }
        });
        Ok(Self { child, started: Instant::now(), path, frame })
    }

    pub fn elapsed(&self) -> f64 {
        self.started.elapsed().as_secs_f64()
    }

    /// Новий кадр прев'ю, якщо він з'явився після `seen`.
    pub fn frame_after(&self, seen: u64) -> Option<(u64, Vec<u8>)> {
        let f = self.frame.lock().unwrap();
        (f.0 > seen).then(|| (f.0, f.1.clone()))
    }

    /// Чи ffmpeg ще пише (інакше — камера зайнята чи відсутня, або досягнуто ліміт).
    pub fn alive(&mut self) -> bool {
        matches!(self.child.try_wait(), Ok(None))
    }

    /// Зупинити й дописати файл. `None` — закоротко або не вдалося.
    pub fn finish(mut self) -> Option<Round> {
        let duration = self.elapsed().min(MAX_SECS);
        if let Some(mut stdin) = self.child.stdin.take() {
            let _ = stdin.write_all(b"q");
        }
        // Після ліміту ffmpeg уже вийшов сам — це теж успіх.
        let ok = self.child.wait().is_ok_and(|s| s.success());
        let size = std::fs::metadata(&self.path).map_or(0, |m| m.len());
        if !ok || duration < MIN_SECS || size == 0 {
            let _ = std::fs::remove_file(&self.path);
            return None;
        }
        Some(Round { path: self.path.clone(), duration })
    }

    pub fn cancel(mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
        let _ = std::fs::remove_file(&self.path);
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn parses_dshow_and_avfoundation_lists() {
        let dshow = "[dshow @ 0] \"Integrated Webcam\" (video)\n[dshow @ 0] \"Microphone (Realtek)\" (audio)";
        assert_eq!(super::parse_devices(dshow, true), vec![("video=Integrated Webcam".into(), "Integrated Webcam".into())]);
        assert_eq!(super::parse_devices(dshow, false)[0].0, "audio=Microphone (Realtek)");
        let avf = "[AVFoundation indev @ 0] AVFoundation video devices:\n[AVFoundation indev @ 0] [0] FaceTime HD Camera\n[AVFoundation indev @ 0] AVFoundation audio devices:\n[AVFoundation indev @ 0] [0] MacBook Microphone";
        assert_eq!(super::parse_devices(avf, true), vec![("0".into(), "FaceTime HD Camera".into())]);
    }
}

impl Drop for RoundRecorder {
    fn drop(&mut self) {
        if matches!(self.child.try_wait(), Ok(None)) {
            let _ = self.child.kill();
            let _ = self.child.wait();
            let _ = std::fs::remove_file(&self.path);
        }
    }
}
