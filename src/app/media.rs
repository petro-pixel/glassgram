//! Playing media in history: audio player, video clips, the photo viewer.

use super::*;

/// Playing media: the audio player, downloaded files, animated clips.
pub(super) struct Playback {
    /// Voice and audio through `ffplay`.
    pub(super) player: Player,
    /// Whether `ffplay` is available for sound.
    pub(super) available: bool,
    /// Media already downloaded to the cache.
    pub(super) files: HashMap<(PeerId, i32), PathBuf>,
    /// Media being downloaded, and what to do afterwards: `true` means open in the system player.
    pub(super) wanted: HashMap<(PeerId, i32), bool>,
    /// Media being downloaded for autoplay (GIFs, round videos, video stickers).
    pub(super) loading: HashSet<(PeerId, i32)>,
    /// Video frames currently on screen.
    pub(super) clips: HashMap<(PeerId, i32), Clip>,
    /// Messages with video visible in this frame (filled during drawing).
    pub(super) clip_wanted: RefCell<Vec<(PeerId, i32)>>,
}

impl App {
    pub(super) fn play(&mut self, peer: PeerId, id: i32) {
        if self.playback.player.is(peer, id) {
            if self.playback.player.playing() { self.playback.player.pause() } else { self.playback.player.resume() }
            return;
        }
        let Some(info) = self.find(peer, id).and_then(|m| m.media.clone()) else { return };
        // Our own voice message is marked as listened by the other side, not by us.
        if let Some(m) = self.find_mut(peer, id).filter(|m| m.media_unread && !m.out) {
            m.media_unread = false;
            self.send(Cmd::MarkListened { peer, id });
        }
        // Videos and round videos play in the bubble with sound; a GIF loops on its own anyway.
        let external = info.kind == MediaKind::Gif;
        if !external && !self.playback.available {
            self.toast = Some((l!("Для звуку потрібен ffplay (пакет ffmpeg)", "Sound needs ffplay (ffmpeg package)").into(), 0.0, true));
            return;
        }
        match self.playback.files.get(&(peer, id)) {
            Some(path) if external => {
                let path = path.clone();
                self.open_received(&path);
            }
            Some(path) => {
                let path = path.clone();
                self.playback.player.play(peer, id, &path, info.duration);
            }
            None => {
                if !external {
                    // While loading, the previous one doesn't play and an earlier pressed one won't start.
                    self.playback.player.stop();
                    self.playback.wanted.retain(|_, ext| *ext);
                }
                if self.playback.wanted.insert((peer, id), external).is_none() {
                    self.send(Cmd::LoadMedia { peer, id });
                }
            }
        }
    }

    /// Playback state for a bubble.
    pub(super) fn play_view(&self, peer: PeerId, id: i32) -> PlayView {
        let current = self.playback.player.is(peer, id);
        PlayView {
            current,
            playing: current && self.playback.player.playing(),
            loading: self.playback.wanted.contains_key(&(peer, id)),
            progress: if current { self.playback.player.progress() } else { 0.0 },
            position: if current { self.playback.player.position() } else { 0.0 },
            speed: self.playback.player.speed,
        }
    }

    /// After a voice message, the next voice message in the same chat, as in Telegram.
    pub(super) fn poll_player(&mut self) {
        let Some((peer, id)) = self.playback.player.poll() else { return };
        let next = self.history.get(&peer).and_then(|msgs| {
            let voice = |m: &Msg| m.media.as_ref().is_some_and(|i| i.kind == MediaKind::Voice);
            let cur = msgs.iter().find(|m| m.id == id)?;
            voice(cur).then(|| msgs.iter().filter(|m| m.id > id && voice(m)).min_by_key(|m| m.id).map(|m| m.id))?
        });
        if let Some(next) = next {
            self.play(peer, next);
        }
    }

    /// Starts, syncs with audio and removes video clips according to the visible list.
    pub(super) fn update_clips(&mut self, now: f64) {
        let wanted = std::mem::take(self.playback.clip_wanted.get_mut());
        self.playback.clips.retain(|k, _| wanted.contains(k));
        let ppp = self.ctx.pixels_per_point();
        for key in wanted {
            let Some(m) = self.find(key.0, key.1) else { continue };
            let info = m.media.clone();
            let kind = info.as_ref().map(|i| i.kind);
            let sound = self.playback.player.is(key.0, key.1) && matches!(kind, Some(MediaKind::Video | MediaKind::Round));
            let Some(path) = self.playback.files.get(&key).cloned() else {
                if kind != Some(MediaKind::Video) && self.playback.loading.insert(key) {
                    self.send(Cmd::LoadMedia { peer: key.0, id: key.1 });
                }
                continue;
            };
            let even = |v: f32| ((v / 2.0).round() as usize * 2).max(2);
            let size = match &info {
                None => [even(STICKER_PT * ppp); 2],
                Some(i) if i.kind == MediaKind::Round => [even(ROUND_PT * ppp); 2],
                Some(i) => {
                    let (sw, sh) = if i.size.0 > 0 && i.size.1 > 0 { (i.size.0 as f32, i.size.1 as f32) } else { (16.0, 9.0) };
                    let w = (VIDEO_PT * ppp).min(sw);
                    [even(w), even(w * sh / sw)]
                }
            };
            let spec = |from: f64, speed: f32, looped: bool| Spec { path: path.clone(), size, from, speed, looped, contain: info.is_none() };
            if sound {
                let (run, from) = self.playback.player.run();
                if self.playback.player.playing() {
                    if self.playback.clips.get(&key).is_none_or(|c| c.run != run) {
                        self.playback.clips.insert(key, Clip::start(spec(from, self.playback.player.speed, false), run, now));
                    }
                } else if let Some(c) = self.playback.clips.get_mut(&key) {
                    c.stop();
                }
            } else if kind == Some(MediaKind::Video) {
                self.playback.clips.remove(&key);
            } else if self.playback.clips.get(&key).is_none_or(|c| c.run != 0) {
                // A sticker that doesn't loop plays once each time it comes into view.
                let looped = info.is_some() || self.settings.loop_stickers;
                let clip = if path.extension().is_some_and(|e| e == "tgs") {
                    // A file we can't draw stays a still picture (its thumbnail).
                    match Clip::lottie(&path, size[0], looped, now) {
                        Some(c) => c,
                        None => continue,
                    }
                } else {
                    Clip::start(spec(0.0, 1.0, looped), 0, now)
                };
                self.playback.clips.insert(key, clip);
            }
            if let Some(c) = self.playback.clips.get_mut(&key) {
                c.advance(&self.ctx, &format!("clip{}_{}", key.0.bot_api_dialog_id_unchecked(), key.1), now);
            }
        }
    }

    /// Chat photos in order, for paging in the viewer.
    pub(super) fn photo_ids(&self, peer: PeerId) -> Vec<i32> {
        self.history.get(&peer).map(|l| l.iter().filter(|m| m.photo).map(|m| m.id).collect()).unwrap_or_default()
    }

    pub(super) fn open_viewer(&mut self, peer: PeerId, id: i32) {
        match &mut self.viewer {
            Some(v) if v.peer == peer => v.show_id(id),
            _ => self.viewer = Some(Viewer::new(peer, id)),
        }
        if !self.textures.contains_key(&ImgKey::Full(peer, id)) {
            self.send(Cmd::LoadFullPhoto { peer, id });
        }
    }

    pub(super) fn viewer_view(&mut self, ui: &mut Ui) {
        let Some(v) = &self.viewer else { return };
        let (peer, id) = (v.peer, v.id);
        let full = self.textures.get(&ImgKey::Full(peer, id));
        let tex = full.or_else(|| self.textures.get(&ImgKey::Photo(peer, id))).cloned();
        let loading = full.is_none() && self.backend.is_some();
        let ids = self.photo_ids(peer);
        let pos = ids.iter().position(|x| *x == id).map_or(0, |i| i + 1);
        let caption = self.find(peer, id).map(|m| m.text.replace('\n', " ")).unwrap_or_default();
        let Some(v) = &mut self.viewer else { return };
        match v.ui(ui, tex.as_ref(), &caption, (pos, ids.len()), loading) {
            Some(ViewerAction::Close) => self.viewer = None,
            Some(ViewerAction::Download) if !self.protected(peer, &[id]) => self.send(Cmd::Download { peer, id }),
            Some(ViewerAction::Prev) if pos > 1 => self.open_viewer(peer, ids[pos - 2]),
            Some(ViewerAction::Next) if pos < ids.len() => self.open_viewer(peer, ids[pos]),
            _ => {}
        }
    }
}

impl App {
    /// Media files downloaded for playing, voice messages listened to.
    pub(super) fn media_event(&mut self, ev: Ev) {
        match ev {
            Ev::Listened { peer, ids } => {
                use grammers_client::session::types::PeerKind;
                for (p, msgs) in &mut self.history {
                    // Without the chat, ids are unique within an account for everything except channels.
                    if peer.map_or(p.kind() != PeerKind::Channel, |x| x == *p) {
                        msgs.iter_mut().filter(|m| ids.contains(&m.id)).for_each(|m| m.media_unread = false);
                    }
                }
            }
            Ev::MediaReady { peer, id, path } => {
                self.playback.loading.remove(&(peer, id));
                self.playback.files.insert((peer, id), path.clone());
                match self.playback.wanted.remove(&(peer, id)) {
                    Some(true) => self.open_received(&path),
                    Some(false) => {
                        let duration = self.find(peer, id).and_then(|m| m.media.as_ref()).map_or(0.0, |i| i.duration);
                        self.playback.player.play(peer, id, &path, duration);
                    }
                    None => {}
                }
            }
            _ => {}
        }
    }
}
