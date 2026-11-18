//! Generated demo media: voice tones, video clips, an animated sticker, pictures and the outbox's files.

use super::*;

/// Demo sound for voice messages: a pulsing chord as WAV; music has its own tunes (`music::tone`).
pub(super) fn tone(id: i32) -> Option<std::path::PathBuf> {
    let secs = match id {
        80 => 4.0,
        81 => 3.0,
        _ => return music::tone(id),
    };
    const RATE: u32 = 22_050;
    let n = (RATE as f32 * secs) as u32;
    let base = 330.0;
    let samples = (0..n).map(|i| {
        let t = i as f32 / RATE as f32;
        let env = (t * 3.0 * std::f32::consts::PI).sin().abs() * (1.0 - t / secs).min(t * 20.0).min(1.0);
        let v: f32 = [1.0, 1.25, 1.5].iter().map(|k| (t * base * k * std::f32::consts::TAU).sin()).sum::<f32>() / 3.0;
        (v * env * 0.4 * i16::MAX as f32) as i16
    });
    write_wav(&format!("demo-{id}.wav"), RATE, samples)
}

/// Mono 16-bit WAV in our own temp folder.
pub(super) fn write_wav(name: &str, rate: u32, samples: impl ExactSizeIterator<Item = i16>) -> Option<std::path::PathBuf> {
    let n = samples.len() as u32;
    let mut wav = Vec::with_capacity(44 + n as usize * 2);
    wav.extend_from_slice(b"RIFF");
    wav.extend_from_slice(&(36 + n * 2).to_le_bytes());
    wav.extend_from_slice(b"WAVEfmt ");
    for v in [16u32.to_le_bytes(), [1, 0, 1, 0], rate.to_le_bytes(), (rate * 2).to_le_bytes(), [2, 0, 16, 0]] {
        wav.extend_from_slice(&v);
    }
    wav.extend_from_slice(b"data");
    wav.extend_from_slice(&(n * 2).to_le_bytes());
    samples.for_each(|s| wav.extend_from_slice(&s.to_le_bytes()));
    let path = crate::platform::temp_file(name);
    std::fs::write(&path, wav).ok()?;
    Some(path)
}

/// Demo videos from ffmpeg test sources: video with sound, silent GIF, round video.
pub(super) fn clip(id: i32) -> Option<std::path::PathBuf> {
    let (video, audio, secs) = match id {
        83 => ("testsrc2=size=640x360:rate=30", true, 6),
        84 => ("mandelbrot=size=480x270:rate=30", false, 3),
        85 => ("life=size=240x240:mold=10:rate=30:ratio=0.5:death_color=#5a3cc8:life_color=#ffd6f0", true, 5),
        86 => ("testsrc2=size=360x640:rate=30", true, 5),
        87 => ("mandelbrot=size=240x240:rate=30", false, 4),
        _ => return None,
    };
    let path = std::env::temp_dir().join(format!("glassgram-demo-{id}.mp4"));
    if path.exists() {
        return Some(path);
    }
    let mut cmd = crate::platform::tool("ffmpeg");
    cmd.args(["-loglevel", "quiet", "-y", "-f", "lavfi", "-i", video]);
    if audio {
        cmd.args(["-f", "lavfi", "-i", "sine=frequency=440:beep_factor=4"]);
    }
    cmd.args(["-t", &secs.to_string(), "-pix_fmt", "yuv420p", "-shortest"]).arg(&path);
    cmd.status().ok()?.success().then_some(path)
}

/// Files, voice and round video messages from the send queue, as the server would echo them.
pub(super) fn queued_media(app: &mut App, q: &Queued) {
    let (peer, reply) = (q.peer, q.reply());
    match q.media.clone() {
        Some(QueuedMedia::Files { paths, as_file, spoiler, above, once }) => {
            for (i, path) in paths.iter().enumerate() {
                let name = path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
                let size = std::fs::metadata(path).map(|m| crate::backend::human_size(m.len() as usize)).unwrap_or_default();
                let text = if i == 0 { q.text.clone() } else { String::new() };
                let mut m = sendas::posted_as(replied(app, outgoing(peer, text, None), &reply), q.send_as);
                m.id += i as i32;
                // Pictures come back as photos, like from the server, with the options chosen.
                let picture = (!as_file).then(|| image::open(path).ok()).flatten().map(|img| img.thumbnail(800, 800).to_rgba8());
                match picture {
                    Some(rgba) => {
                        let (mid, size) = (m.id, [rgba.width() as usize, rgba.height() as usize]);
                        app.push_event(Ev::NewMsg(Msg { photo: true, spoiler, invert_media: above, ttl: once.then_some(crate::backend::VIEW_ONCE), ..m }));
                        app.push_event(Ev::Image { key: ImgKey::Photo(peer, mid), image: ColorImage::from_rgba_unmultiplied(size, rgba.as_raw()) });
                    }
                    None => app.push_event(Ev::NewMsg(Msg { file: Some((name, size)), ..m })),
                }
            }
            let _ = std::fs::remove_dir_all(crate::backend::outbox_dir(&crate::accounts::session_path(0), q.key));
        }
        Some(QueuedMedia::Voice { path, duration, waveform }) => {
            let info = MediaInfo { kind: MediaKind::Voice, duration, size: (0, 0), title: String::new(), performer: String::new(), waveform, bytes: 0 };
            let m = Msg { media: Some(info), ..sendas::posted_as(replied(app, outgoing(peer, String::new(), None), &reply), q.send_as) };
            let mid = m.id;
            app.push_event(Ev::NewMsg(m));
            // The file stays on disk: that's what we will play.
            app.push_event(Ev::MediaReady { peer, id: mid, path });
        }
        Some(QueuedMedia::Round { path, duration }) => {
            let info = MediaInfo { kind: MediaKind::Round, duration, size: (384, 384), title: String::new(), performer: String::new(), waveform: Vec::new(), bytes: 0 };
            let m = Msg { media: Some(info), ..sendas::posted_as(replied(app, outgoing(peer, String::new(), None), &reply), q.send_as) };
            let mid = m.id;
            app.push_event(Ev::NewMsg(m));
            app.push_event(Ev::MediaReady { peer, id: mid, path });
        }
        None => {}
    }
}

/// A beating heart as a `.tgs` (gzipped Lottie), for the demo's animated sticker.
pub(super) fn demo_tgs() -> Option<std::path::PathBuf> {
    use std::io::Write;
    let path = std::env::temp_dir().join("glassgram-demo-heart.tgs");
    if path.exists() {
        return Some(path);
    }
    let scale = r#"{"a":1,"k":[{"t":0,"s":[80,80,100],"i":{"x":[0.4],"y":[1]},"o":{"x":[0.6],"y":[0]}},{"t":15,"s":[105,105,100],"i":{"x":[0.4],"y":[1]},"o":{"x":[0.6],"y":[0]}},{"t":30,"s":[80,80,100]}]}"#;
    let heart = r#"{"ty":"sh","ks":{"a":0,"k":{"c":true,"v":[[0,40],[-60,-10],[-30,-55],[0,-30],[30,-55],[60,-10]],"i":[[0,0],[0,25],[-25,0],[0,0],[0,0],[0,-25]],"o":[[0,0],[0,-25],[0,0],[0,0],[25,0],[0,25]]}}}"#;
    let json = format!(
        r#"{{"v":"5.5.2","fr":30,"ip":0,"op":30,"w":200,"h":200,"layers":[{{"ty":4,"ind":1,"ip":0,"op":30,"st":0,"ks":{{"o":{{"a":0,"k":100}},"r":{{"a":0,"k":0}},"p":{{"a":0,"k":[100,105,0]}},"a":{{"a":0,"k":[0,0,0]}},"s":{scale}}},"shapes":[{heart},{{"ty":"fl","c":{{"a":0,"k":[0.95,0.25,0.4,1]}},"o":{{"a":0,"k":100}}}}]}}]}}"#
    );
    let mut gz = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::default());
    gz.write_all(json.as_bytes()).ok()?;
    std::fs::write(&path, gz.finish().ok()?).ok()?;
    Some(path)
}

/// Generated "photos": a gradient sunset and an avatar.
pub fn images() -> Vec<(ImgKey, ColorImage)> {
    let sunset = |w: usize, h: usize| {
        let mut img = ColorImage::filled([w, h], Color32::BLACK);
        for y in 0..h {
            for x in 0..w {
                let (fx, fy) = (x as f32 / w as f32, y as f32 / h as f32);
                let d = ((fx - 0.5).powi(2) + (fy - 0.62).powi(2)).sqrt();
                let sun = (1.0 - d * 6.0).clamp(0.0, 1.0);
                let sea = if fy > 0.65 { 0.45 } else { 1.0 };
                let r = ((255.0 * (0.55 + 0.45 * fy) + 255.0 * sun) * sea).min(255.0);
                let g = ((90.0 + 120.0 * fy * fy + 200.0 * sun) * sea).min(255.0);
                let b = ((170.0 - 110.0 * fy + 120.0 * sun) * sea).min(255.0);
                img.pixels[y * w + x] = Color32::from_rgb(r as u8, g as u8, b as u8);
            }
        }
        img
    };
    let id = PeerId::user_unchecked(1);
    let mut list = vec![
        (ImgKey::Photo(id, 6), sunset(480, 320)),
        (ImgKey::Photo(id, 70), sunset(400, 300)),
        (ImgKey::Photo(id, 71), sunset(300, 300)),
        (ImgKey::Photo(id, 72), sunset(320, 420)),
        (ImgKey::Photo(id, 74), sunset(420, 300)),
        (ImgKey::Photo(id, 75), sunset(400, 260)),
        (ImgKey::Photo(id, effects::VIEW_ONCE_PHOTO), sunset(380, 300)),
        (ImgKey::Avatar(PeerId::user_unchecked(5)), sunset(96, 96)),
        (ImgKey::Photo(PeerId::user_unchecked(2), 5), sunset(480, 250)),
        (ImgKey::Photo(PeerId::user_unchecked(6), 50), sunset(480, 270)),
        (ImgKey::Sponsored(DEMO_AD_PHOTO), sunset(112, 112)),
        (ImgKey::Sponsored(DEMO_AD_MEDIA), sunset(640, 360)),
    ];
    list.extend(instant_view::images());
    list
}

pub(super) fn story_picture(id: i32) -> ColorImage {
    let (w, h) = (360, 640);
    let mut img = ColorImage::filled([w, h], Color32::BLACK);
    let hue = id as f32 * 0.9;
    for y in 0..h {
        for x in 0..w {
            let (fx, fy) = (x as f32 / w as f32, y as f32 / h as f32);
            let wave = ((fx * 6.0 + fy * 3.0 + hue).sin() * 0.5 + 0.5) * 0.35;
            let r = (120.0 + 120.0 * (hue + fy * 2.0).sin().abs() + 60.0 * wave).min(255.0);
            let g = (60.0 + 90.0 * (hue * 0.7 + fx).cos().abs() + 80.0 * wave).min(255.0);
            let b = (150.0 + 100.0 * (1.0 - fy) + 40.0 * wave).min(255.0);
            img.pixels[y * w + x] = Color32::from_rgb(r as u8, g as u8, b as u8);
        }
    }
    img
}

/// A sunset picture in a temporary file, for trying the send box and the photo editor in the demo.
pub fn sample_photo() -> Result<std::path::PathBuf, String> {
    let (w, h) = (1200u32, 800u32);
    let img = image::RgbImage::from_fn(w, h, |x, y| {
        let (fx, fy) = (x as f32 / w as f32, y as f32 / h as f32);
        let d = ((fx - 0.5).powi(2) + (fy - 0.62).powi(2)).sqrt();
        let sun = (1.0 - d * 6.0).clamp(0.0, 1.0);
        let sea = if fy > 0.65 { 0.45 } else { 1.0 };
        let r = ((255.0 * (0.55 + 0.45 * fy) + 255.0 * sun) * sea).min(255.0);
        let g = ((90.0 + 120.0 * fy * fy + 200.0 * sun) * sea).min(255.0);
        let b = ((170.0 - 110.0 * fy + 120.0 * sun) * sea).min(255.0);
        image::Rgb([r as u8, g as u8, b as u8])
    });
    let path = crate::platform::temp_file("sunset.jpg");
    img.save(&path).map_err(|e| e.to_string())?;
    Ok(path)
}
