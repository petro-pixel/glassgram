//! Notification sounds, as in Telegram Desktop: "Play sound" in Settings → Notifications, the
//! account's saved ringtones (listen, add, delete), reaction notifications, and a chat's own
//! sound in its notification menu. The default sound is synthesised here, so no audio asset of
//! someone else's ships with the app.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::process::{Child, Stdio};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use super::*;
use super::settings_pages::choice;
use crate::backend::{NotifyState, RINGTONE_EXTS, ReactFrom, ReactionNotify, Ringtone, Sound};

/// Messages arriving together ring once.
const QUIET: Duration = Duration::from_millis(1200);
const ROW_H: f32 = 48.0;

#[derive(Default)]
pub(super) struct Sounds {
    /// `None` until the backend answers.
    ringtones: Option<Vec<Ringtone>>,
    /// Ringtone files already in the cache.
    files: HashMap<i64, PathBuf>,
    /// Chats that differ from the default: silent, or a sound of their own.
    chats: HashMap<PeerId, NotifyState>,
    reactions: Option<ReactionNotify>,
    /// A file chosen in the picker, taken on the next frame.
    picked: Arc<Mutex<Option<PathBuf>>>,
    uploading: bool,
    child: Option<Child>,
    last: Option<Instant>,
}

impl Drop for Sounds {
    fn drop(&mut self) {
        if let Some(mut c) = self.child.take() {
            let _ = c.kill();
            let _ = c.wait();
        }
    }
}

/// Name of a sound for menus.
pub(super) fn sound_label(sound: Option<Sound>, ringtones: &[Ringtone]) -> String {
    match sound {
        None | Some(Sound::Default) => l!("За замовчуванням", "Default").to_owned(),
        Some(Sound::None) => l!("Без звуку", "No sound").to_owned(),
        Some(Sound::Ringtone(id)) => ringtones.iter().find(|r| r.id == id).map_or_else(|| l!("Звук", "Sound").to_owned(), |r| r.title.clone()),
    }
}

fn from_label(f: ReactFrom) -> &'static str {
    match f {
        ReactFrom::Nobody => l!("Ніхто", "Nobody"),
        ReactFrom::Contacts => l!("Мої контакти", "My contacts"),
        ReactFrom::All => l!("Усі", "Everybody"),
    }
}

/// A short two-note chime as 16-bit mono WAV.
fn chime() -> Vec<u8> {
    const RATE: u32 = 22050;
    let notes = [(0.0, 1046.5), (0.09, 1568.0)];
    let n = (RATE as f32 * 0.65) as usize;
    let mut pcm = Vec::with_capacity(n * 2);
    for i in 0..n {
        let t = i as f32 / RATE as f32;
        let mut v = 0.0;
        for (start, f) in notes {
            let dt = t - start;
            if dt < 0.0 {
                continue;
            }
            // A 5 ms attack avoids a click, the exponential tail sounds like a bell.
            let env = (dt / 0.005).min(1.0) * (-dt * 9.0).exp();
            let w = std::f32::consts::TAU * f * dt;
            v += env * (w.sin() + 0.2 * (2.0 * w).sin());
        }
        pcm.extend_from_slice(&((v * 0.3).clamp(-1.0, 1.0) * i16::MAX as f32).to_le_bytes());
    }
    let mut wav = Vec::with_capacity(44 + pcm.len());
    let data = pcm.len() as u32;
    wav.extend_from_slice(b"RIFF");
    wav.extend_from_slice(&(36 + data).to_le_bytes());
    wav.extend_from_slice(b"WAVEfmt ");
    wav.extend_from_slice(&16u32.to_le_bytes());
    wav.extend_from_slice(&1u16.to_le_bytes());
    wav.extend_from_slice(&1u16.to_le_bytes());
    wav.extend_from_slice(&RATE.to_le_bytes());
    wav.extend_from_slice(&(RATE * 2).to_le_bytes());
    wav.extend_from_slice(&2u16.to_le_bytes());
    wav.extend_from_slice(&16u16.to_le_bytes());
    wav.extend_from_slice(b"data");
    wav.extend_from_slice(&data.to_le_bytes());
    wav.extend_from_slice(&pcm);
    wav
}

/// The default sound's file, written on first use.
fn default_sound() -> Option<PathBuf> {
    let path = backend::data_dir().join("sounds").join("default.wav");
    if !path.exists() {
        std::fs::create_dir_all(path.parent()?).ok()?;
        std::fs::write(&path, chime()).ok()?;
    }
    Some(path)
}

impl App {
    pub(super) fn sounds_event(&mut self, ev: Ev) {
        let s = &mut self.sounds;
        match ev {
            Ev::ChatSounds(list) => {
                let need = list.iter().any(|(_, st)| matches!(st.sound, Some(Sound::Ringtone(_))));
                s.chats.extend(list);
                if need && s.ringtones.is_none() {
                    self.load_ringtones();
                }
            }
            Ev::Ringtones(list) => {
                s.files.retain(|id, _| list.iter().any(|r| r.id == *id));
                s.ringtones = Some(list);
                s.uploading = false;
            }
            Ev::RingtoneFile { id, path } => {
                s.files.insert(id, path);
            }
            Ev::RingtoneUploadFailed => s.uploading = false,
            Ev::RingtonesChanged => {
                if s.ringtones.is_some() {
                    self.load_ringtones();
                }
            }
            Ev::ReactionNotify(r) => s.reactions = Some(r),
            _ => {}
        }
    }

    /// A chat's settings changed: remember the ones that affect its sound.
    pub(super) fn chat_sound_changed(&mut self, peer: PeerId, state: NotifyState) {
        if state.silent || state.sound.is_some() {
            self.sounds.chats.insert(peer, state);
            if matches!(state.sound, Some(Sound::Ringtone(_))) && self.sounds.ringtones.is_none() {
                self.load_ringtones();
            }
        } else {
            self.sounds.chats.remove(&peer);
        }
    }

    pub(super) fn load_ringtones(&mut self) {
        self.sounds.ringtones.get_or_insert_with(Vec::new);
        self.send(Cmd::LoadRingtones);
    }

    /// Settings → Notifications was opened.
    pub(super) fn load_sounds(&mut self) {
        self.load_ringtones();
        self.send(Cmd::LoadReactionNotify);
    }

    /// Plays a sound once, over any that is still ringing.
    fn play_sound(&mut self, sound: Sound) {
        let path = match sound {
            Sound::None => return,
            Sound::Ringtone(id) => self.sounds.files.get(&id).filter(|p| p.exists()).cloned().or_else(default_sound),
            Sound::Default => default_sound(),
        };
        let Some(path) = path else { return };
        self.play_file(&path);
    }

    fn play_file(&mut self, path: &Path) {
        if let Some(mut c) = self.sounds.child.take() {
            let _ = c.kill();
            let _ = c.wait();
        }
        let mut cmd = crate::platform::tool("ffplay");
        cmd.args(["-nodisp", "-autoexit", "-loglevel", "quiet"]).arg(path).stdin(Stdio::null()).stdout(Stdio::null()).stderr(Stdio::null());
        match cmd.spawn() {
            Ok(c) => self.sounds.child = Some(c),
            Err(e) => crate::errlog::record(&format!("ffplay: {e}")),
        }
    }

    /// A new message that notifies: ring with the chat's sound.
    pub(super) fn message_sound(&mut self, peer: PeerId) {
        if !self.settings.sound || self.sounds.last.is_some_and(|t| t.elapsed() < QUIET) {
            return;
        }
        let state = self.sounds.chats.get(&peer).copied().unwrap_or_default();
        if state.silent {
            return;
        }
        self.sounds.last = Some(Instant::now());
        self.play_sound(state.sound.unwrap_or_default());
    }

    /// Picked in a chat's notification menu: save and let it be heard.
    pub(super) fn set_chat_sound(&mut self, peer: PeerId, state: NotifyState, sound: Sound) {
        self.send(Cmd::SetNotify { peer, mute_until: state.mute_until, silent: state.silent, sound: Some(sound) });
        self.play_sound(sound);
    }

    pub fn demo_ringtones(&self) -> Option<Vec<Ringtone>> {
        self.sounds.ringtones.clone().filter(|l| !l.is_empty())
    }

    pub fn demo_reaction_notify(&self) -> Option<ReactionNotify> {
        self.sounds.reactions
    }

    pub(super) fn ringtones(&self) -> &[Ringtone] {
        self.sounds.ringtones.as_deref().unwrap_or_default()
    }

    fn pick_ringtone(&mut self) {
        let (picked, ctx) = (self.sounds.picked.clone(), self.ctx.clone());
        std::thread::spawn(move || {
            let file = rfd::FileDialog::new().set_title(l!("Додати звук", "Add sound")).add_filter(l!("Аудіо", "Audio"), &RINGTONE_EXTS).pick_file();
            if let Some(path) = file {
                *picked.lock().unwrap() = Some(path);
            }
            ctx.request_repaint();
        });
    }

    /// "Notification sounds": the saved ringtones with listen and delete, and "Add sound".
    pub(super) fn ringtones_section(&mut self, ui: &mut Ui) {
        let picked = self.sounds.picked.lock().unwrap().take();
        if let Some(path) = picked {
            self.sounds.uploading = true;
            self.send(Cmd::UploadRingtone(path));
        }
        section_title(ui, l!("Звуки сповіщень", "Notification sounds"));
        let mut play = None;
        let mut delete = None;
        let w = ui.available_width();
        let (r, _) = ui.allocate_exact_size(vec2(w, ROW_H), Sense::hover());
        let p = ui.painter().clone();
        p.rect_filled(r, 14.0, glass::veil(10));
        icons::draw(&p, Rect::from_center_size(pos2(r.left() + 22.0, r.center().y), vec2(20.0, 20.0)), Icon::Bell, ink_dim());
        p.text(pos2(r.left() + 46.0, r.center().y), Align2::LEFT_CENTER, l!("За замовчуванням", "Default"), FontId::proportional(14.0), ink());
        let b = Rect::from_center_size(pos2(r.right() - 26.0, r.center().y), vec2(36.0, 36.0));
        if glass::icon_button(ui, b, "sound-default", Icon::Volume, false, true).on_hover_text(l!("Прослухати", "Listen")).clicked() {
            play = Some(Sound::Default);
        }
        ui.add_space(4.0);
        match &self.sounds.ringtones {
            None => {
                ui.add(egui::Spinner::new().color(ink_dim()));
            }
            Some(list) => {
                for t in list {
                    let (r, _) = ui.allocate_exact_size(vec2(w, ROW_H), Sense::hover());
                    p.rect_filled(r, 14.0, glass::veil(10));
                    icons::draw(&p, Rect::from_center_size(pos2(r.left() + 22.0, r.center().y), vec2(20.0, 20.0)), Icon::Mic, ink_dim());
                    p.galley(pos2(r.left() + 46.0, r.top() + 6.0), elided(ui, &t.title, 14.0, ink(), w - 150.0), ink());
                    p.text(pos2(r.left() + 46.0, r.top() + 34.0), Align2::LEFT_CENTER, crate::player::clock(t.duration), FontId::proportional(12.5), ink_dim());
                    let b = Rect::from_center_size(pos2(r.right() - 66.0, r.center().y), vec2(36.0, 36.0));
                    if glass::icon_button(ui, b, ("sound-play", t.id), Icon::Volume, false, true).on_hover_text(l!("Прослухати", "Listen")).clicked() {
                        play = Some(Sound::Ringtone(t.id));
                    }
                    let b = Rect::from_center_size(pos2(r.right() - 26.0, r.center().y), vec2(36.0, 36.0));
                    if glass::icon_button(ui, b, ("sound-del", t.id), Icon::Trash, false, true).on_hover_text(l!("Видалити", "Delete")).clicked() {
                        delete = Some(t.id);
                    }
                    ui.add_space(4.0);
                }
            }
        }
        let r = ui.allocate_exact_size(vec2(w.min(260.0), 36.0), Sense::hover()).0;
        let label = if self.sounds.uploading { l!("Завантаження…", "Uploading…") } else { l!("Додати звук", "Add sound") };
        if glass::button(ui, r, "sound-add", label, false, !self.sounds.uploading).clicked() {
            self.pick_ringtone();
        }
        ui.label(egui::RichText::new(l!("MP3 або OGG до 300 КБ і 5 секунд. Звуки зберігаються в акаунті й доступні на всіх пристроях; звук чату вибирають у його сповіщеннях.", "MP3 or OGG up to 300 KB and 5 seconds. Sounds are kept in the account on every device; a chat's sound is picked in its notifications.")).color(ink_dim()).size(12.5));
        if let Some(s) = play {
            self.play_sound(s);
        }
        if let Some(id) = delete {
            self.send(Cmd::DeleteRingtone(id));
        }
    }

    /// "Reactions": whose reactions to messages, stories and polls notify, as in Telegram Desktop.
    pub(super) fn reactions_section(&mut self, ui: &mut Ui) {
        section_title(ui, l!("Сповіщення про реакції", "Reaction notifications"));
        let Some(before) = self.sounds.reactions else {
            ui.add(egui::Spinner::new().color(ink_dim()));
            return;
        };
        let mut r = before;
        let froms = [ReactFrom::Nobody, ReactFrom::Contacts, ReactFrom::All].map(|f| (f, from_label(f)));
        for (id, title, value) in [("react-msg", l!("Реакції на повідомлення", "Reactions to messages"), &mut r.messages), ("react-story", l!("Реакції на історії", "Reactions to stories"), &mut r.stories), ("react-poll", l!("Голоси в опитуваннях", "Poll votes"), &mut r.polls)] {
            ui.label(egui::RichText::new(title).color(ink()).size(14.0));
            choice(ui, id, value, &froms, 122.0);
            ui.add_space(4.0);
        }
        ui.checkbox(&mut r.previews, l!("Показувати, хто відреагував", "Show who reacted"));
        let mut sound_on = r.sound != Sound::None;
        ui.checkbox(&mut sound_on, l!("Звук", "Sound"));
        if sound_on != (r.sound != Sound::None) {
            r.sound = if sound_on { Sound::Default } else { Sound::None };
        }
        if r != before {
            self.sounds.reactions = Some(r);
            self.send(Cmd::SetReactionNotify(r));
        }
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn chime_is_valid_wav() {
        let w = super::chime();
        assert_eq!(&w[..4], b"RIFF");
        assert_eq!(u32::from_le_bytes([w[4], w[5], w[6], w[7]]) as usize, w.len() - 8);
        assert_eq!(u32::from_le_bytes([w[40], w[41], w[42], w[43]]) as usize, w.len() - 44);
    }
}
