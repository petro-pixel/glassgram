//! Music player, as in Telegram Desktop: a glass bar over the chat with the track, previous/next,
//! a seek line, volume, speed, repeat and shuffle; a click on the title opens the playlist. The
//! queue is the chat's music (the loaded history plus `messages.search` with the music filter),
//! or a user's profile music (`users.getSavedMusic`), whose tracks get ids `-1 - index`.
//! Sound comes from the shared `Player`; the bar stays while chats are switched.

use super::*;
use crate::backend::MediaInfo;

/// Height of the bar under the chat header.
const MUSIC_BAR_H: f32 = 46.0;
/// "Previous" within this many seconds of a track restarts it instead, as in Telegram Desktop.
const RESTART_AFTER: f64 = 3.0;
const ROW_H: f32 = 46.0;

#[derive(Clone, Copy, Default, PartialEq)]
enum Repeat {
    #[default]
    Off,
    All,
    One,
}

#[derive(Clone)]
struct Track {
    id: i32,
    info: MediaInfo,
}

#[derive(Default)]
pub(super) struct Music {
    /// The track in the bar; it stays after the queue ends, until the bar is closed.
    pub(super) current: Option<(PeerId, i32)>,
    /// The playlist of the current track, in order.
    queue: Vec<Track>,
    repeat: Repeat,
    shuffle: bool,
    /// Tracks played in shuffle, for "previous".
    played: Vec<i32>,
    playlist: bool,
    /// Seek line under the pointer while dragged (fraction).
    scrub: Option<f32>,
    /// Volume while dragged; applied on release, since each change restarts ffplay.
    vol_drag: Option<f32>,
    /// Chat music found by search, oldest first.
    found: HashMap<PeerId, Vec<Track>>,
    /// Chats whose music search has been sent.
    asked: HashSet<PeerId>,
    /// Profile music by user, and how many tracks there are in all.
    saved: HashMap<PeerId, (Vec<MediaInfo>, i32)>,
}

impl Music {
    fn track(&self, id: i32) -> Option<&Track> {
        self.queue.iter().find(|t| t.id == id)
    }

    /// Duration of a queued track that isn't in the loaded history.
    pub(super) fn duration(&self, peer: PeerId, id: i32) -> Option<f64> {
        (self.current.is_some_and(|c| c.0 == peer)).then(|| self.track(id).map(|t| t.info.duration)).flatten()
    }
}

fn is_music(m: &Msg) -> bool {
    m.media.as_ref().is_some_and(|i| i.kind == MediaKind::Audio)
}

/// "Performer — Title", or whichever of them there is.
fn track_title(info: &MediaInfo) -> String {
    match (info.performer.as_str(), info.title.as_str()) {
        ("", "") => l!("Аудіо", "Audio").to_owned(),
        ("", t) => t.to_owned(),
        (who, "") => who.to_owned(),
        (who, t) => format!("{who} — {t}"),
    }
}

impl App {
    pub(super) fn music_event(&mut self, ev: Ev) {
        match ev {
            Ev::Music { peer, msgs } => {
                let tracks = msgs.into_iter().filter_map(|m| Some(Track { id: m.id, info: m.media.filter(|i| i.kind == MediaKind::Audio)? })).collect();
                self.music.found.insert(peer, tracks);
                self.music_refresh();
            }
            Ev::SavedMusic { peer, tracks, total } => {
                self.music.saved.insert(peer, (tracks, total));
                self.music_refresh();
            }
            _ => {}
        }
    }

    /// A chat's music track was started from its bubble: it becomes the bar's track.
    pub(super) fn music_begin(&mut self, peer: PeerId, id: i32) {
        if self.music.current.is_none_or(|c| c.0 != peer || (c.1 < 0) != (id < 0)) {
            self.music.played.clear();
        }
        self.music.current = Some((peer, id));
        self.music.scrub = None;
        if id > 0 && self.music.asked.insert(peer) {
            self.send(Cmd::LoadMusic(peer));
        }
        self.music_refresh();
    }

    /// Rebuilds the queue: chat music from history and search, or the user's profile music.
    fn music_refresh(&mut self) {
        let Some((peer, id)) = self.music.current else { return };
        self.music.queue = if id < 0 {
            let saved = self.music.saved.get(&peer).map_or(&[][..], |s| &s.0[..]);
            saved.iter().enumerate().map(|(k, info)| Track { id: -1 - k as i32, info: info.clone() }).collect()
        } else {
            let mut tracks: Vec<Track> = self.music.found.get(&peer).cloned().unwrap_or_default();
            let loaded = self.history.get(&peer).into_iter().flatten().filter(|m| is_music(m));
            let fresh: Vec<Track> = loaded.filter(|m| !tracks.iter().any(|t| t.id == m.id)).filter_map(|m| Some(Track { id: m.id, info: m.media.clone()? })).collect();
            tracks.extend(fresh);
            tracks.sort_by_key(|t| t.id);
            tracks
        };
    }

    /// Plays a queued track: from history the usual way, otherwise straight from the queue.
    fn music_play(&mut self, peer: PeerId, id: i32) {
        if self.find(peer, id).is_some() {
            // `play` toggles the track it already has; here a track always starts.
            if self.playback.player.is(peer, id) {
                self.playback.player.seek(0.0);
                self.playback.player.resume();
            } else {
                self.play(peer, id);
            }
            return;
        }
        self.music_begin(peer, id);
        let Some(duration) = self.music.track(id).map(|t| t.info.duration) else { return };
        if !self.playback.available {
            self.toast = Some((l!("Для звуку потрібен ffplay (пакет ffmpeg)", "Sound needs ffplay (ffmpeg package)").into(), 0.0, true));
            return;
        }
        if let Some(path) = self.playback.files.get(&(peer, id)).cloned() {
            self.playback.player.play(peer, id, &path, duration);
            return;
        }
        self.playback.player.stop();
        self.playback.wanted.retain(|_, ext| *ext);
        if self.playback.wanted.insert((peer, id), false).is_none() {
            self.send(if id < 0 { Cmd::LoadSavedTrack { peer, index: -1 - id } } else { Cmd::LoadMedia { peer, id } });
        }
    }

    /// Plays a user's profile music from track `index`.
    pub(super) fn music_play_profile(&mut self, peer: PeerId, index: i32) {
        let id = -1 - index;
        if self.playback.player.is(peer, id) {
            self.music_toggle();
        } else {
            self.music_play(peer, id);
        }
    }

    fn music_toggle(&mut self) {
        let Some((peer, id)) = self.music.current else { return };
        let player = &mut self.playback.player;
        match (player.is(peer, id), player.playing()) {
            (true, true) => player.pause(),
            (true, false) => player.resume(),
            _ => self.music_play(peer, id),
        }
    }

    /// The bar's track ended (`auto`) or "next" was pressed.
    pub(super) fn music_next(&mut self, auto: bool) {
        let Some((peer, id)) = self.music.current else { return };
        self.music_refresh();
        if auto && self.music.repeat == Repeat::One {
            self.music_play(peer, id);
            return;
        }
        let len = self.music.queue.len();
        let Some(i) = self.music.queue.iter().position(|t| t.id == id) else { return };
        let next = if self.music.shuffle && len > 1 {
            let mut buf = [0u8; 4];
            let r = getrandom::fill(&mut buf).map_or(i + 1, |_| u32::from_le_bytes(buf) as usize);
            // Any track but this one.
            Some((i + 1 + r % (len - 1)) % len)
        } else if i + 1 < len {
            Some(i + 1)
        } else {
            (self.music.repeat == Repeat::All).then_some(0)
        };
        if let Some(n) = next {
            if self.music.shuffle {
                self.music.played.push(id);
            }
            let to = self.music.queue[n].id;
            self.music_play(peer, to);
        }
    }

    fn music_prev(&mut self) {
        let Some((peer, id)) = self.music.current else { return };
        self.music_refresh();
        let player = &self.playback.player;
        let back = if player.is(peer, id) && player.position() > RESTART_AFTER {
            None
        } else if self.music.shuffle {
            self.music.played.pop()
        } else {
            let i = self.music.queue.iter().position(|t| t.id == id).unwrap_or(0);
            match i.checked_sub(1) {
                Some(p) => Some(self.music.queue[p].id),
                None if self.music.repeat == Repeat::All => self.music.queue.last().map(|t| t.id),
                None => None,
            }
        };
        self.music_play(peer, back.unwrap_or(id));
    }

    /// Room the bar takes under the chat header.
    pub(super) fn music_bar_h(&self) -> f32 {
        if self.music.current.is_some() { MUSIC_BAR_H + 8.0 } else { 0.0 }
    }

    /// The bar at `top`, and the playlist under it; `above` is where the composer starts.
    pub(super) fn music_bar(&mut self, ui: &mut Ui, rect: Rect, top: f32, above: f32) {
        let Some((peer, id)) = self.music.current else { return };
        let Some(info) = self.music.track(id).map(|t| t.info.clone()) else {
            // A track deleted from the chat leaves the queue: so does the bar.
            if self.find(peer, id).is_none() {
                self.music_close();
            }
            return;
        };
        let bar = Rect::from_min_size(pos2(rect.left(), top), vec2(rect.width(), MUSIC_BAR_H));
        let p = ui.painter().clone();
        glass::glass(&p, bar, Glass::tinted(bar.height() / 2.0, glass::chrome(), 110).with_shadow().frosted(0.96));
        ui.interact(bar, ui.scope_id().with("music-bar"), Sense::click());
        let player = &self.playback.player;
        let current = player.is(peer, id);
        let playing = current && player.playing();
        let loading = self.playback.wanted.contains_key(&(peer, id));
        let cy = bar.center().y;
        let square = |x: f32| Rect::from_center_size(pos2(x, cy), vec2(34.0, 34.0));

        // Left: previous, play/pause, next.
        if flat_button(ui, square(bar.left() + 26.0), "music-prev", Icon::SkipBack, false).on_hover_text(l!("Попередній трек", "Previous track")).clicked() {
            self.music_prev();
        }
        let play_icon = if playing { Icon::Pause } else { Icon::Play };
        let play_tip = if playing { l!("Пауза", "Pause") } else { l!("Відтворити", "Play") };
        let play_rect = square(bar.left() + 62.0);
        if loading {
            spinner(&p, play_rect.center(), 9.0);
        } else if flat_button(ui, play_rect, "music-play", play_icon, false).on_hover_text(play_tip).clicked() {
            self.music_toggle();
        }
        if flat_button(ui, square(bar.left() + 98.0), "music-next", Icon::SkipForward, false).on_hover_text(l!("Наступний трек", "Next track")).clicked() {
            self.music_next(false);
        }
        let cover = Rect::from_center_size(pos2(bar.left() + 136.0, cy), vec2(30.0, 30.0));
        glass::glass(&p, cover, Glass::tinted(15.0, ACCENT, 170));
        icons::draw(&p, Rect::from_center_size(cover.center(), vec2(16.0, 16.0)), Icon::Music, glass::ON_DARK);

        // Right, from the edge: close, volume, speed, shuffle, repeat; the narrow bar drops the extras.
        let wide = bar.width() > 600.0;
        let mut x = bar.right() - 26.0;
        if flat_button(ui, square(x), "music-close", Icon::Close, false).on_hover_text(l!("Закрити плеєр", "Close player")).clicked() {
            self.music_close();
            return;
        }
        if wide {
            x -= 24.0 + 70.0;
            self.music_volume(ui, Rect::from_min_size(pos2(x, cy - 10.0), vec2(70.0, 20.0)));
            x -= 22.0;
            let volume = self.music.vol_drag.unwrap_or(self.playback.player.volume);
            let (vicon, vtip) = if volume <= 0.0 { (Icon::Muted, l!("Увімкнути звук", "Unmute")) } else { (Icon::Volume, l!("Вимкнути звук", "Mute")) };
            if flat_button(ui, square(x), "music-mute", vicon, false).on_hover_text(vtip).clicked() {
                self.playback.player.set_volume(if volume > 0.0 { 0.0 } else { 1.0 });
            }
        }
        x -= 42.0;
        let speed = self.playback.player.speed;
        let chip = Rect::from_center_size(pos2(x, cy), vec2(40.0, 26.0));
        let resp = ui.interact(chip, ui.scope_id().with("music-speed"), Sense::click()).on_hover_text(l!("Швидкість", "Speed"));
        if resp.hovered() {
            p.rect_filled(chip, 13.0, glass::veil(24));
            ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
        }
        p.text(chip.center(), Align2::CENTER_CENTER, format!("{speed}×"), FontId::proportional(13.0), if speed != 1.0 { ACCENT } else { ink() });
        if resp.clicked() {
            self.playback.player.cycle_speed();
        }
        if wide {
            x -= 40.0;
            let on = self.music.shuffle;
            let tip = if on { l!("Вимкнути перемішування", "Shuffle off") } else { l!("Перемішати", "Shuffle") };
            if flat_button(ui, square(x), "music-shuffle", Icon::Shuffle, on).on_hover_text(tip).clicked() {
                self.music.shuffle = !on;
                self.music.played.clear();
            }
        }
        x -= 38.0;
        let (ricon, rtip) = match self.music.repeat {
            Repeat::Off => (Icon::Repeat, l!("Повторювати плейлист", "Repeat playlist")),
            Repeat::All => (Icon::Repeat, l!("Повторювати трек", "Repeat track")),
            Repeat::One => (Icon::RepeatOne, l!("Не повторювати", "Don't repeat")),
        };
        if flat_button(ui, square(x), "music-repeat", ricon, self.music.repeat != Repeat::Off).on_hover_text(rtip).clicked() {
            self.music.repeat = match self.music.repeat {
                Repeat::Off => Repeat::All,
                Repeat::All => Repeat::One,
                Repeat::One => Repeat::Off,
            };
        }

        // Middle: the title opens the playlist; position and length under it.
        let tx = cover.right() + 10.0;
        let text_w = (x - 24.0 - tx).max(20.0);
        let bold = FontId::new(13.5, egui::FontFamily::Name(format::BOLD.into()));
        let mut job = LayoutJob::single_section(if info.title.is_empty() { track_title(&info) } else { info.title.clone() }, egui::TextFormat::simple(bold, ink()));
        if !info.performer.is_empty() && !info.title.is_empty() {
            job.append(&format!(" — {}", info.performer), 0.0, egui::TextFormat::simple(FontId::proportional(13.5), ink_dim()));
        }
        job.wrap = TextWrapping::truncate_at_width(text_w);
        p.galley(pos2(tx, cy - 17.0), p.layout_job(job), ink());
        let pos = match self.music.scrub {
            Some(f) => f as f64 * info.duration,
            None if current => self.playback.player.position(),
            None => 0.0,
        };
        let n = self.music.queue.len();
        let k = self.music.queue.iter().position(|t| t.id == id).map_or(0, |i| i + 1);
        let time = format!("{} / {} · {}", player::clock(pos), player::clock(info.duration), lf!("{k} з {n}", "{k} of {n}"));
        p.galley(pos2(tx, cy + 2.0), elided(ui, &time, 12.0, ink_dim(), text_w), ink_dim());
        let title_rect = Rect::from_min_max(pos2(tx, bar.top()), pos2(tx + text_w, bar.bottom() - 8.0));
        let title = ui.interact(title_rect, ui.scope_id().with("music-title"), Sense::click()).on_hover_text(l!("Плейлист", "Playlist"));
        if title.hovered() {
            ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
        }
        if title.clicked() {
            self.music.playlist = !self.music.playlist;
            self.music_refresh();
        }
        self.music_seek_line(ui, bar, info.duration, pos);

        if self.music.playlist {
            let w = (bar.width() - (tx - 8.0 - bar.left())).clamp(220.0, 400.0);
            let left = (tx - 8.0).min(bar.right() - w);
            let h = (n as f32 * ROW_H + 60.0).min(above - bar.bottom() - 16.0).max(ROW_H + 60.0);
            let panel = Rect::from_min_size(pos2(left, bar.bottom() + 8.0), vec2(w, h));
            self.music_playlist(ui, panel, peer);
            let outside = ui.input(|i| i.pointer.any_pressed() && i.pointer.interact_pos().is_some_and(|p| !panel.contains(p) && !bar.contains(p)));
            if outside || ui.input_mut(|i| i.consume_key(egui::Modifiers::NONE, Key::Escape)) {
                self.music.playlist = false;
            }
        }
        if playing {
            ui.ctx().request_repaint_after(Duration::from_millis(250));
        }
    }

    /// A thin progress line along the bottom of the bar; click or drag to seek.
    fn music_seek_line(&mut self, ui: &mut Ui, bar: Rect, duration: f64, pos: f64) {
        let r = bar.height() / 2.0;
        let line = Rect::from_min_max(pos2(bar.left() + r, bar.bottom() - 4.0), pos2(bar.right() - r, bar.bottom() - 2.0));
        let hit = Rect::from_min_max(pos2(line.left(), bar.bottom() - 9.0), pos2(line.right(), bar.bottom() + 3.0));
        let resp = ui.interact(hit, ui.scope_id().with("music-seek"), Sense::click_and_drag());
        let frac = |x: f32| ((x - line.left()) / line.width()).clamp(0.0, 1.0);
        if (resp.is_pointer_button_down_on() || resp.dragged())
            && let Some(p) = resp.interact_pointer_pos()
        {
            self.music.scrub = Some(frac(p.x));
        }
        if (resp.drag_stopped() || resp.clicked())
            && let Some(f) = self.music.scrub.take()
            && let Some((peer, id)) = self.music.current
        {
            if self.playback.player.is(peer, id) {
                self.playback.player.seek(f);
            }
        } else if !resp.is_pointer_button_down_on() && !resp.dragged() {
            self.music.scrub = None;
        }
        let active = resp.hovered() || self.music.scrub.is_some();
        let shown = if duration > 0.0 { (pos / duration) as f32 } else { 0.0 };
        let h = if active { 4.0 } else { 2.0 };
        let line = Rect::from_center_size(line.center(), vec2(line.width(), h));
        let p = ui.painter();
        p.rect_filled(line, h / 2.0, rgba(ink(), 40));
        p.rect_filled(Rect::from_min_max(line.min, pos2(line.left() + line.width() * shown.clamp(0.0, 1.0), line.bottom())), h / 2.0, ACCENT);
        if let Some(p) = resp.hover_pos() {
            ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
            resp.on_hover_text_at_pointer(player::clock(frac(p.x) as f64 * duration));
        }
    }

    fn music_volume(&mut self, ui: &mut Ui, track: Rect) {
        let resp = ui.interact(track, ui.scope_id().with("music-volume"), Sense::click_and_drag()).on_hover_text(l!("Гучність", "Volume"));
        if (resp.is_pointer_button_down_on() || resp.dragged())
            && let Some(p) = resp.interact_pointer_pos()
        {
            self.music.vol_drag = Some(((p.x - track.left()) / track.width()).clamp(0.0, 1.0));
        }
        if resp.drag_stopped() || resp.clicked() {
            if let Some(v) = self.music.vol_drag.take() {
                self.playback.player.set_volume(v);
            }
        } else if !resp.is_pointer_button_down_on() && !resp.dragged() {
            self.music.vol_drag = None;
        }
        let active = resp.hovered() || self.music.vol_drag.is_some();
        let shown = self.music.vol_drag.unwrap_or(self.playback.player.volume);
        let h = if active { 5.0 } else { 3.0 };
        let line = Rect::from_center_size(track.center(), vec2(track.width(), h));
        let p = ui.painter();
        p.rect_filled(line, h / 2.0, rgba(ink(), 40));
        let x = line.left() + line.width() * shown.clamp(0.0, 1.0);
        p.rect_filled(Rect::from_min_max(line.min, pos2(x, line.bottom())), h / 2.0, ink());
        if active {
            p.circle_filled(pos2(x, line.center().y), 6.0, ink());
            ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
        }
    }

    /// The queue with the current track marked; a click plays a track.
    fn music_playlist(&mut self, ui: &mut Ui, panel: Rect, peer: PeerId) {
        glass::glass(ui.painter(), panel, Glass::panel(24.0).with_shadow());
        ui.interact(panel, ui.scope_id().with("music-playlist-bg"), Sense::click());
        let n = self.music.queue.len();
        let profile = self.music.current.is_some_and(|c| c.1 < 0);
        let title = if profile { l!("Музика профілю", "Profile music") } else { l!("Плейлист", "Playlist") };
        let p = ui.painter();
        p.text(pos2(panel.left() + 18.0, panel.top() + 26.0), Align2::LEFT_CENTER, title, FontId::proportional(15.0), ink());
        p.text(pos2(panel.right() - 18.0, panel.top() + 26.0), Align2::RIGHT_CENTER, lp!(n, ["{n} трек", "{n} треки", "{n} треків"], ["{n} track", "{n} tracks"]), FontId::proportional(12.5), ink_dim());
        let body = Rect::from_min_max(pos2(panel.left() + 8.0, panel.top() + 48.0), pos2(panel.right() - 8.0, panel.bottom() - 8.0));
        let cur = self.music.current.map(|c| c.1);
        let playing = cur.is_some_and(|id| self.playback.player.is(peer, id) && self.playback.player.playing());
        let mut chosen = None;
        let mut child = ui.new_child(UiBuilder::new().max_rect(body));
        child.set_clip_rect(body);
        egui::ScrollArea::vertical().id_salt("music-playlist").auto_shrink(false).show(&mut child, |ui| {
            ui.spacing_mut().item_spacing.y = 0.0;
            for t in &self.music.queue {
                let w = ui.available_width();
                let (r, resp) = ui.allocate_exact_size(vec2(w, ROW_H), Sense::click());
                let here = cur == Some(t.id);
                let hover = ui.ctx().animate_bool(resp.id, resp.hovered());
                let p = ui.painter();
                if here || hover > 0.0 {
                    p.rect_filled(r, 14.0, glass::veil(if here { 18 } else { (hover * 14.0) as u8 }));
                }
                let icon = match (here, playing) {
                    (true, true) => Icon::Pause,
                    (true, false) => Icon::Play,
                    _ => Icon::Music,
                };
                let dot = Rect::from_center_size(pos2(r.left() + 24.0, r.center().y), vec2(32.0, 32.0));
                p.circle_filled(dot.center(), 16.0, if here { ACCENT } else { glass::veil(20) });
                icons::draw(p, Rect::from_center_size(dot.center(), vec2(15.0, 15.0)), icon, if here { glass::ON_DARK } else { ink_dim() });
                let dur = p.layout_no_wrap(player::clock(t.info.duration), FontId::proportional(12.0), ink_dim());
                let tw = (r.right() - 12.0 - dur.size().x - 8.0 - (dot.right() + 10.0)).max(10.0);
                let name = if t.info.title.is_empty() { track_title(&t.info) } else { t.info.title.clone() };
                // The accent dot marks the current track; its title stays ink for contrast on the tint.
                let font = if here { FontId::new(14.0, egui::FontFamily::Name(format::BOLD.into())) } else { FontId::proportional(14.0) };
                let mut job = LayoutJob::single_section(name, egui::TextFormat::simple(font, ink()));
                job.wrap = TextWrapping::truncate_at_width(tw);
                p.galley(pos2(dot.right() + 10.0, r.top() + 6.0), p.layout_job(job), ink());
                if !t.info.performer.is_empty() {
                    p.galley(pos2(dot.right() + 10.0, r.top() + 25.0), elided(ui, &t.info.performer, 12.5, ink_dim(), tw), ink_dim());
                }
                p.galley(pos2(r.right() - 12.0 - dur.size().x, r.center().y - dur.size().y / 2.0), dur, ink_dim());
                if resp.hovered() {
                    ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
                }
                if resp.clicked() {
                    chosen = Some(t.id);
                }
            }
        });
        match chosen {
            Some(id) if cur == Some(id) => self.music_toggle(),
            Some(id) => {
                if self.music.shuffle && let Some(c) = cur {
                    self.music.played.push(c);
                }
                self.music_play(peer, id);
            }
            None => {}
        }
    }

    fn music_close(&mut self) {
        if let Some((peer, id)) = self.music.current.take()
            && self.playback.player.is(peer, id)
        {
            self.playback.player.stop();
        }
        self.music.playlist = false;
        self.music.queue.clear();
        self.music.played.clear();
    }

    /// The profile music row in a user's info panel; a click plays it from the first track.
    pub(super) fn profile_music_row(&self, ui: &mut Ui, peer: PeerId) -> bool {
        let Some((tracks, total)) = self.music.saved.get(&peer).filter(|s| !s.0.is_empty()) else { return false };
        let n = *total;
        let caption = lp!(n, ["Музика · {n} трек", "Музика · {n} треки", "Музика · {n} треків"], ["Music · {n} track", "Music · {n} tracks"]);
        super::info::info_row(ui, Icon::Music, &track_title(&tracks[0]), &caption).clicked()
    }
}

/// A flat icon button for the bar (no glass on glass); `on` marks a toggle that is active.
fn flat_button(ui: &mut Ui, rect: Rect, id: &str, icon: Icon, on: bool) -> egui::Response {
    let resp = ui.interact(rect, ui.scope_id().with(id), Sense::click());
    let hover = ui.ctx().animate_bool(resp.id.with("hover"), resp.hovered());
    let p = ui.painter();
    if hover > 0.0 {
        p.circle_filled(rect.center(), rect.width() / 2.0, glass::veil((hover * 22.0) as u8));
    }
    if resp.hovered() {
        ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
    }
    let side = if matches!(icon, Icon::Play | Icon::Pause) { 18.0 } else { 17.0 };
    // The play triangle looks centred a little to the right of its box.
    let nudge = if icon == Icon::Play { vec2(1.5, 0.0) } else { vec2(0.0, 0.0) };
    icons::draw(p, Rect::from_center_size(rect.center() + nudge, vec2(side, side)), icon, if on { ACCENT } else { ink() });
    resp
}
