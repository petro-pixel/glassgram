//! Demo music: tracks in Olena's chat, older ones that only the music search finds, and her
//! profile music, each a short generated melody, so the player bar, queue and next/previous work.

use grammers_client::session::types::PeerId;

use crate::app::App;
use crate::backend::{Cmd, Ev, MediaInfo, MediaKind, Msg};

/// (id, title, performer, seconds, root note in Hz); negative ids are Olena's profile music.
const TRACKS: [(i32, &str, &str, f32, f32); 8] = [
    (63, "Kyiv Nights", "Glass Quartet", 6.0, 196.0),
    (64, "Morning Coffee", "Liquid Trio", 5.0, 262.0),
    (82, "Glass Morning", "Glassgram", 5.0, 220.0),
    (95, "Frosted Lights", "Glass Quartet", 6.0, 247.0),
    (96, "Blur & Shine", "Liquid Trio", 7.0, 294.0),
    (-1, "Lviv Rain", "Olena", 6.0, 175.0),
    (-2, "Carpathian Echo", "Olena", 5.0, 233.0),
    (-3, "Dnipro Sunset", "Olena", 6.0, 208.0),
];
/// Shown in the chat; the rest of her chat's music comes only from `LoadMusic`.
const IN_HISTORY: [i32; 2] = [95, 96];
const OLDER: [i32; 2] = [63, 64];

fn olena() -> PeerId {
    PeerId::user_unchecked(1)
}

fn info(id: i32) -> Option<MediaInfo> {
    let (_, title, performer, secs, _) = TRACKS.iter().find(|t| t.0 == id)?;
    Some(MediaInfo { kind: MediaKind::Audio, duration: *secs as f64, size: (0, 0), title: (*title).into(), performer: (*performer).into(), waveform: Vec::new(), bytes: 0 })
}

/// Music after "Glass Morning" in Olena's chat.
pub(super) fn messages(base: Msg) -> Vec<Msg> {
    IN_HISTORY.iter().map(|&id| Msg { id, media: info(id), ..base.clone() }).collect()
}

pub(super) fn handle(app: &mut App, cmd: Cmd) {
    match cmd {
        Cmd::LoadMusic(peer) => {
            let mut msgs: Vec<Msg> = app
                .demo_history()
                .filter(|(p, _, _)| *p == peer)
                .flat_map(|(_, _, msgs)| msgs.iter().filter(|m| m.media.as_ref().is_some_and(|i| i.kind == MediaKind::Audio)).cloned())
                .collect();
            if peer == olena() {
                let base = Msg { sender: "Olena".into(), sender_color: super::demo_color("Olena"), time: "11:20".into(), ..Msg::placeholder(peer) };
                msgs.extend(OLDER.iter().map(|&id| Msg { id, media: info(id), ..base.clone() }));
            }
            msgs.sort_by_key(|m| m.id);
            app.push_event(Ev::Music { peer, msgs });
        }
        Cmd::LoadSavedMusic(peer) if peer == olena() => {
            let tracks: Vec<MediaInfo> = (1..=3).filter_map(|k| info(-k)).collect();
            app.push_event(Ev::SavedMusic { peer, total: tracks.len() as i32, tracks });
        }
        Cmd::LoadSavedTrack { peer, index } => {
            if let Some(path) = tone(-1 - index) {
                app.push_event(Ev::MediaReady { peer, id: -1 - index, path });
            }
        }
        _ => {}
    }
}

/// A little pentatonic tune over the track's root note, with a fifth under it.
pub(super) fn tone(id: i32) -> Option<std::path::PathBuf> {
    let &(_, _, _, secs, root) = TRACKS.iter().find(|t| t.0 == id)?;
    const RATE: u32 = 22_050;
    const NOTE: f32 = 0.4;
    let steps = [1.0, 9.0 / 8.0, 5.0 / 4.0, 3.0 / 2.0, 5.0 / 3.0, 2.0];
    let n = (RATE as f32 * secs) as u32;
    let samples = (0..n).map(|i| {
        let t = i as f32 / RATE as f32;
        let k = (t / NOTE) as i32;
        let freq = root * steps[((k * 7 + id.abs() * 3 + k * k) % steps.len() as i32) as usize];
        let local = t - k as f32 * NOTE;
        let env = (local * 40.0).min(1.0) * (-local * 5.0).exp() * (1.0 - t / secs).min(t * 20.0).min(1.0);
        let v = (t * freq * std::f32::consts::TAU).sin() * 0.7 + (t * root * 0.75 * std::f32::consts::TAU).sin() * 0.3;
        (v * env * 0.4 * i16::MAX as f32) as i16
    });
    super::write_wav(&format!("music-{id}.wav"), RATE, samples)
}
