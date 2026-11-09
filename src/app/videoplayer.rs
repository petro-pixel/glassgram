//! Full-screen video player, as in Telegram Desktop's media viewer: play/pause, a seek bar,
//! volume, speed, full screen, and picture-in-picture that keeps playing while chats are browsed.
//! Sound and time come from the shared `Player` (ffplay); frames from a `Clip` that is restarted
//! with `-ss` on every seek, speed or volume change (each of them is a new player run).

use super::*;
use eframe::egui::{Modifiers, Vec2};

const SPEEDS: [f32; 4] = [0.5, 1.0, 1.5, 2.0];
/// Arrow keys seek by this much, as in Telegram Desktop.
const STEP: f64 = 5.0;
/// Controls hide after this long without moving the mouse.
const IDLE: f64 = 2.5;
const PIP_W: f32 = 320.0;
const PIP_MARGIN: f32 = 16.0;

#[derive(Default)]
pub(super) struct VideoPlayer {
    open: Option<Session>
}

struct Session {
    peer: PeerId,
    id: i32,
    duration: f64,
    /// Width / height of the source, for fitting before the first frame.
    aspect: f32,
    /// Sound started once the file was there (it may still be downloading on open).
    started: bool,
    pip: bool,
    /// PiP corner: bit 0 is right, bit 1 is bottom.
    corner: u8,
    /// PiP being dragged: its offset from the corner position.
    drag: Option<Vec2>,
    fullscreen: bool,
    /// Position while the player isn't playing this video (paused, ended, or taken by a voice message).
    pos: f64,
    /// Seek bar under the pointer while dragged (fraction).
    scrub: Option<f32>,
    /// Volume slider while dragged; applied on release, since each change restarts ffplay.
    vol_drag: Option<f32>,
    /// Volume before muting, restored by the next click.
    unmuted: f32,
    clip: Option<Clip>,
    /// What the clip shows: (player run, its start, playing).
    clip_for: (u64, f64, bool),
    /// The previous clip's last frame, shown until the new clip's first one.
    last: Option<TextureHandle>,
    /// Last mouse movement, for hiding the controls.
    moved: f64,
}

impl VideoPlayer {
    /// The full-screen view owns the keyboard and the window.
    pub(super) fn full(&self) -> bool {
        self.open.as_ref().is_some_and(|s| !s.pip)
    }

    /// The bubble doesn't decode its own copy of a video shown here.
    pub(super) fn shows(&self, key: (PeerId, i32)) -> bool {
        self.open.as_ref().is_some_and(|s| (s.peer, s.id) == key)
    }
}

/// Keys of the full-screen view, taken at the start of the frame.
#[derive(Default)]
struct Keys {
    toggle: bool,
    back: bool,
    forward: bool,
    fullscreen: bool,
    esc: bool,
}

impl App {
    /// Opens a chat video in the player; downloads it first if needed.
    pub(super) fn open_video(&mut self, peer: PeerId, id: i32) {
        if let Some(s) = self.video.open.as_mut().filter(|s| (s.peer, s.id) == (peer, id)) {
            s.pip = false;
            return;
        }
        self.close_video();
        let Some(info) = self.find(peer, id).and_then(|m| m.media.clone()) else { return };
        if !self.playback.available {
            self.toast = Some((l!("Для звуку потрібен ffplay (пакет ffmpeg)", "Sound needs ffplay (ffmpeg package)").into(), 0.0, true));
            return;
        }
        let aspect = if info.size.0 > 0 && info.size.1 > 0 { info.size.0 as f32 / info.size.1 as f32 } else { 16.0 / 9.0 };
        self.video.open = Some(Session {
            peer,
            id,
            duration: info.duration,
            aspect,
            started: false,
            pip: false,
            corner: 3,
            drag: None,
            fullscreen: false,
            pos: 0.0,
            scrub: None,
            vol_drag: None,
            unmuted: 1.0,
            clip: None,
            clip_for: (0, -1.0, false),
            last: None,
            moved: 0.0,
        });
        // Space and arrows belong to the player now, not to the composer.
        self.ctx.memory_mut(|m| {
            if let Some(f) = m.focused() {
                m.surrender_focus(f);
            }
        });
        if !self.playback.files.contains_key(&(peer, id)) && self.playback.loading.insert((peer, id)) {
            self.send(Cmd::LoadMedia { peer, id });
        }
    }

    fn close_video(&mut self) {
        let Some(s) = self.video.open.take() else { return };
        if self.playback.player.is(s.peer, s.id) {
            self.playback.player.stop();
        }
        if s.fullscreen {
            self.ctx.send_viewport_cmd(egui::ViewportCommand::Fullscreen(false));
        }
    }

    /// Hotkeys as in Telegram Desktop: Space, ←/→ by 5 s, F, Esc. `true`: the full view took the keyboard.
    pub(super) fn video_keys(&mut self, ctx: &egui::Context) -> bool {
        if !self.video.full() {
            return false;
        }
        let k = ctx.input_mut(|i| Keys {
            toggle: i.consume_key(Modifiers::NONE, Key::Space),
            back: i.consume_key(Modifiers::NONE, Key::ArrowLeft),
            forward: i.consume_key(Modifiers::NONE, Key::ArrowRight),
            fullscreen: i.consume_key(Modifiers::NONE, Key::F),
            esc: i.consume_key(Modifiers::NONE, Key::Escape),
        });
        if k.toggle {
            self.video_toggle();
        }
        if k.back || k.forward {
            let at = self.video_position() + if k.forward { STEP } else { -STEP };
            self.video_seek(at);
        }
        if k.fullscreen {
            self.video_fullscreen(None);
        }
        if k.esc {
            // Full screen is left first, then the player closes.
            if self.video.open.as_ref().is_some_and(|s| s.fullscreen) {
                self.video_fullscreen(Some(false));
            } else {
                self.close_video();
            }
        }
        true
    }

    fn video_position(&self) -> f64 {
        let Some(s) = &self.video.open else { return 0.0 };
        if self.playback.player.is(s.peer, s.id) { self.playback.player.position() } else { s.pos }
    }

    fn video_toggle(&mut self) {
        let Some(s) = &self.video.open else { return };
        let (peer, id, duration, pos) = (s.peer, s.id, s.duration, s.pos);
        let player = &mut self.playback.player;
        if player.is(peer, id) {
            if player.playing() { player.pause() } else { player.resume() }
        } else if let Some(path) = self.playback.files.get(&(peer, id)) {
            // Ended or taken by another message: continue from where it stood, or from the start at the end.
            let from = if pos >= duration - 0.3 { 0.0 } else { pos };
            player.play_at(peer, id, path, duration, from);
        }
    }

    fn video_seek(&mut self, at: f64) {
        let Some(s) = &mut self.video.open else { return };
        let at = at.clamp(0.0, s.duration.max(0.0));
        s.pos = at;
        if self.playback.player.is(s.peer, s.id) && s.duration > 0.0 {
            self.playback.player.seek((at / s.duration) as f32);
        }
    }

    /// `None` switches; also leaving PiP for full screen.
    fn video_fullscreen(&mut self, on: Option<bool>) {
        let Some(s) = &mut self.video.open else { return };
        let on = on.unwrap_or(!s.fullscreen);
        if on != s.fullscreen {
            s.fullscreen = on;
            self.ctx.send_viewport_cmd(egui::ViewportCommand::Fullscreen(on));
        }
    }

    /// Starts the sound once the file is there and keeps the frames in step with it.
    fn video_frames(&mut self, size: [usize; 2], now: f64) -> Option<TextureHandle> {
        let s = self.video.open.as_mut()?;
        let key = (s.peer, s.id);
        let path = self.playback.files.get(&key).cloned()?;
        let player = &mut self.playback.player;
        if !s.started {
            s.started = true;
            player.play(key.0, key.1, &path, s.duration);
        }
        let owns = player.is(key.0, key.1);
        let playing = owns && player.playing();
        if owns {
            s.pos = player.position();
        }
        let want = if playing {
            let (run, from) = player.run();
            (run, from, true)
        } else {
            (0, s.pos, false)
        };
        // A window resize decodes anew only when the size changes noticeably.
        let resized = s.clip.as_ref().is_some_and(|c| (c.size()[0] as f32 - size[0] as f32).abs() > size[0] as f32 * 0.2);
        if s.clip.is_none() || want != s.clip_for || resized {
            if let Some(t) = s.clip.take().and_then(|c| c.tex.clone()) {
                s.last = Some(t);
            }
            let speed = if playing { player.speed } else { 1.0 };
            let spec = Spec { path, size, from: s.pos, speed, looped: false, contain: false };
            s.clip = Some(Clip::start(spec, want.0, now));
            s.clip_for = want;
        }
        let c = s.clip.as_mut()?;
        c.advance(&self.ctx, &format!("video{}_{}", key.0.bot_api_dialog_id_unchecked(), key.1), now);
        // Paused: one frame for the new position is enough.
        if !playing && c.tex.is_some() && !c.done {
            c.stop();
        }
        c.tex.clone().or_else(|| s.last.clone())
    }

    /// Decoding size for a frame shown in `rect`: physical pixels, no larger than the source.
    fn video_px(&self, rect: Rect) -> [usize; 2] {
        let ppp = self.ctx.pixels_per_point();
        let src = self.video.open.as_ref().and_then(|s| self.find(s.peer, s.id)).and_then(|m| m.media.as_ref()).map_or(0, |i| i.size.0);
        let w = (rect.width() * ppp).min(if src > 0 { src as f32 } else { f32::MAX });
        let h = w * rect.height() / rect.width().max(1.0);
        let even = |v: f32| ((v / 2.0).round() as usize * 2).max(2);
        [even(w), even(h)]
    }

    /// The player over the window, or its PiP window in a corner.
    pub(super) fn video_view(&mut self, ui: &mut Ui) {
        let Some(s) = &self.video.open else { return };
        if s.pip {
            self.pip_view(ui.ctx().clone());
        } else {
            self.video_full_view(ui);
        }
    }

    fn video_full_view(&mut self, ui: &mut Ui) {
        let now = ui.input(|i| i.time);
        let screen = ui.max_rect();
        let Some(s) = &mut self.video.open else { return };
        if ui.input(|i| i.pointer.is_moving()) {
            s.moved = now;
        }
        let (aspect, fullscreen) = (s.aspect, s.fullscreen);
        let p = ui.painter().clone();
        p.rect_filled(screen, 0.0, Color32::from_black_alpha(if fullscreen { 255 } else { 235 }));
        let bg = ui.interact(screen, ui.scope_id().with("video-bg"), Sense::click());
        let area = if fullscreen { screen } else { screen.shrink2(vec2(40.0, 70.0)) };
        let fit = (area.width() / aspect).min(area.height());
        let rect = Rect::from_center_size(area.center(), vec2(fit * aspect, fit));
        let px = self.video_px(rect);
        let tex = self.video_frames(px, now);
        match &tex {
            Some(t) => glass::image_rounded(&p, rect, if fullscreen { 0.0 } else { 10.0 }, t.id(), t.size_vec2()),
            None => {
                p.rect_filled(rect, 10.0, Color32::from_black_alpha(255));
                ui.put(Rect::from_center_size(rect.center(), vec2(40.0, 40.0)), egui::Spinner::new().size(40.0).color(glass::ON_DARK));
            }
        }
        let playing = self.video_playing();
        let Some(s) = &self.video.open else { return };
        let over_bar = ui.input(|i| i.pointer.hover_pos()).is_some_and(|p| p.y > screen.bottom() - 110.0 || p.y < screen.top() + 80.0);
        let shown = !playing || now - s.moved < IDLE || s.scrub.is_some() || s.vol_drag.is_some() || over_bar;
        if !shown {
            ui.ctx().set_cursor_icon(egui::CursorIcon::None);
        }
        let fade = ui.ctx().animate_bool_with_time(ui.scope_id().with("video-controls"), shown, 0.2);

        // Click on the picture plays or pauses, outside it closes; double click switches full screen.
        let at = bg.interact_pointer_pos().unwrap_or_default();
        if bg.double_clicked() && rect.contains(at) {
            // The first click of the pair already paused: undo it.
            self.video_toggle();
            self.video_fullscreen(None);
        } else if bg.clicked() {
            if rect.contains(at) {
                self.video_toggle();
            } else {
                self.close_video();
                return;
            }
        }
        if fade <= 0.01 {
            return;
        }
        ui.scope(|ui| {
            ui.multiply_opacity(fade);
            let btn = |i: f32| Rect::from_min_size(pos2(screen.right() - 58.0 - i * 52.0, screen.top() + 16.0), vec2(42.0, 42.0));
            if dark_button(ui, btn(0.0), "video-close", Icon::Close).on_hover_text(l!("Закрити (Esc)", "Close (Esc)")).clicked() {
                self.close_video();
                return;
            }
            let Some(s) = &self.video.open else { return };
            let (peer, id) = (s.peer, s.id);
            if dark_button(ui, btn(1.0), "video-save", Icon::Download).on_hover_text(l!("Зберегти", "Save")).clicked() && !self.protected(peer, &[id]) {
                self.send(Cmd::Download { peer, id });
            }
            let w = (screen.width() - 32.0).min(760.0);
            let bar = Rect::from_center_size(pos2(screen.center().x, screen.bottom() - 46.0), vec2(w, 52.0));
            self.video_controls(ui, bar, playing);
        });
    }

    fn video_playing(&self) -> bool {
        self.video.open.as_ref().is_some_and(|s| self.playback.player.is(s.peer, s.id) && self.playback.player.playing())
    }

    /// The control row on a dark glass chip: play, time, seek bar, volume, speed, PiP, full screen.
    fn video_controls(&mut self, ui: &mut Ui, bar: Rect, playing: bool) {
        let p = ui.painter().clone();
        glass::glass(&p, bar, Glass::tinted(bar.height() / 2.0, Color32::BLACK, 190).frosted(1.0));
        // Clicks between the controls don't reach the picture under the chip.
        ui.interact(bar, ui.scope_id().with("video-bar"), Sense::click());
        let Some(s) = &self.video.open else { return };
        let (duration, fullscreen, scrub) = (s.duration, s.fullscreen, s.scrub);
        let cy = bar.center().y;
        let square = |x: f32| Rect::from_center_size(pos2(x, cy), vec2(36.0, 36.0));

        // Left: play/pause and the time.
        let icon = if playing { Icon::Pause } else { Icon::Play };
        let tip = if playing { l!("Пауза (Пробіл)", "Pause (Space)") } else { l!("Відтворити (Пробіл)", "Play (Space)") };
        if dark_button(ui, square(bar.left() + 30.0), "video-play", icon).on_hover_text(tip).clicked() {
            self.video_toggle();
        }
        let pos = scrub.map_or(self.video_position(), |f| f as f64 * duration);
        let time = format!("{} / {}", player::clock(pos), player::clock(duration));
        let g = p.layout_no_wrap(time, FontId::proportional(12.5), glass::ON_DARK);
        let tx = bar.left() + 56.0;
        p.galley(pos2(tx, cy - g.size().y / 2.0), g.clone(), glass::ON_DARK);

        // Right, from the edge: full screen, PiP, speed, volume.
        let mut x = bar.right() - 30.0;
        let (fs_icon, fs_tip) = if fullscreen { (Icon::Minimize, l!("Вийти з повного екрана (F)", "Exit full screen (F)")) } else { (Icon::Maximize, l!("На весь екран (F)", "Full screen (F)")) };
        if dark_button(ui, square(x), "video-fs", fs_icon).on_hover_text(fs_tip).clicked() {
            self.video_fullscreen(None);
        }
        x -= 40.0;
        if dark_button(ui, square(x), "video-pip", Icon::Pip).on_hover_text(l!("Картинка в картинці", "Picture-in-picture")).clicked() {
            self.video_fullscreen(Some(false));
            if let Some(s) = &mut self.video.open {
                s.pip = true;
            }
        }
        x -= 44.0;
        let speed = self.playback.player.speed;
        let label = format!("{}×", speed);
        let r = Rect::from_center_size(pos2(x, cy), vec2(44.0, 30.0));
        let resp = ui.interact(r, ui.scope_id().with("video-speed"), Sense::click()).on_hover_text(l!("Швидкість", "Speed"));
        if resp.hovered() {
            p.rect_filled(r, 15.0, glass::rgba(glass::ON_DARK, 30));
            ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
        }
        p.text(r.center(), Align2::CENTER_CENTER, label, FontId::proportional(13.0), glass::ON_DARK);
        if resp.clicked() {
            let i = SPEEDS.iter().position(|v| *v == speed).unwrap_or(0);
            self.playback.player.set_speed(SPEEDS[(i + 1) % SPEEDS.len()]);
        }
        x -= 34.0 + 84.0;
        let vol_rect = Rect::from_min_size(pos2(x, cy - 10.0), vec2(80.0, 20.0));
        self.volume_slider(ui, vol_rect);
        x -= 22.0;
        let Some(s) = &mut self.video.open else { return };
        let volume = s.vol_drag.unwrap_or(self.playback.player.volume);
        let (vicon, vtip) = if volume <= 0.0 { (Icon::Muted, l!("Увімкнути звук", "Unmute")) } else { (Icon::Volume, l!("Вимкнути звук", "Mute")) };
        if dark_button(ui, square(x), "video-mute", vicon).on_hover_text(vtip).clicked() {
            let to = if volume > 0.0 {
                s.unmuted = volume;
                0.0
            } else {
                s.unmuted.max(0.1)
            };
            self.playback.player.set_volume(to);
        }

        // The seek bar takes the room between the time and the volume.
        let left = tx + g.size().x + 16.0;
        let right = x - 26.0;
        if right - left > 40.0 {
            let track = Rect::from_min_max(pos2(left, cy - 10.0), pos2(right, cy + 10.0));
            self.seek_bar(ui, track, duration);
        }
    }

    fn seek_bar(&mut self, ui: &mut Ui, track: Rect, duration: f64) {
        let resp = ui.interact(track, ui.scope_id().with("video-seek"), Sense::click_and_drag());
        let frac = |x: f32| ((x - track.left()) / track.width()).clamp(0.0, 1.0);
        let pos = self.video_position();
        let Some(s) = &mut self.video.open else { return };
        if resp.is_pointer_button_down_on() || resp.dragged() {
            if let Some(p) = resp.interact_pointer_pos() {
                s.scrub = Some(frac(p.x));
            }
        }
        // Seek once on release: every seek restarts ffplay and ffmpeg.
        if (resp.drag_stopped() || resp.clicked()) && s.scrub.is_some() {
            let f = s.scrub.take().unwrap_or(0.0);
            self.video_seek(f as f64 * duration);
        } else if !resp.is_pointer_button_down_on() && !resp.dragged() {
            s.scrub = None;
        }
        let Some(s) = &self.video.open else { return };
        let shown = s.scrub.unwrap_or(if duration > 0.0 { (pos / duration) as f32 } else { 0.0 });
        slider(ui, track, shown, resp.hovered() || s.scrub.is_some());
        if let Some(p) = resp.hover_pos() {
            ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
            resp.on_hover_text_at_pointer(player::clock(frac(p.x) as f64 * duration));
        }
    }

    fn volume_slider(&mut self, ui: &mut Ui, track: Rect) {
        let resp = ui.interact(track, ui.scope_id().with("video-volume"), Sense::click_and_drag()).on_hover_text(l!("Гучність", "Volume"));
        let Some(s) = &mut self.video.open else { return };
        if (resp.is_pointer_button_down_on() || resp.dragged())
            && let Some(p) = resp.interact_pointer_pos()
        {
            s.vol_drag = Some(((p.x - track.left()) / track.width()).clamp(0.0, 1.0));
        }
        if resp.drag_stopped() || resp.clicked() {
            if let Some(v) = s.vol_drag.take() {
                self.playback.player.set_volume(v);
            }
        } else if !resp.is_pointer_button_down_on() && !resp.dragged() {
            s.vol_drag = None;
        }
        let Some(s) = &self.video.open else { return };
        let shown = s.vol_drag.unwrap_or(self.playback.player.volume);
        slider(ui, track, shown, resp.hovered() || s.vol_drag.is_some());
        if resp.hovered() {
            ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
        }
    }

    /// A small glass window in a corner of the main window; it keeps playing while chats are browsed.
    fn pip_view(&mut self, ctx: egui::Context) {
        let now = ctx.input(|i| i.time);
        let screen = ctx.content_rect();
        let Some(s) = &self.video.open else { return };
        let w = PIP_W.min(screen.width() * 0.5);
        let h = (w / s.aspect).clamp(120.0, PIP_W);
        let size = vec2(w, h);
        let corner = |c: u8| {
            let x = if c & 1 == 1 { screen.right() - PIP_MARGIN - w } else { screen.left() + PIP_MARGIN };
            let y = if c & 2 == 2 { screen.bottom() - PIP_MARGIN - h } else { screen.top() + PIP_MARGIN };
            pos2(x, y)
        };
        let at = corner(s.corner) + s.drag.unwrap_or_default();
        let px = self.video_px(Rect::from_min_size(at, size));
        let tex = self.video_frames(px, now);
        let playing = self.video_playing();
        let progress = match &self.video.open {
            Some(s) if s.duration > 0.0 => (self.video_position() / s.duration) as f32,
            _ => 0.0,
        };
        let mut act = PipAct::None;
        egui::Area::new(egui::Id::unique("video-pip")).order(egui::Order::Foreground).fixed_pos(at).show(&ctx, |ui| {
            let (frame, resp) = ui.allocate_exact_size(size, Sense::click_and_drag());
            let p = ui.painter().clone();
            let p = &p;
            glass::glass(p, frame, Glass::panel(18.0));
            let pic = frame.shrink(6.0);
            match &tex {
                Some(t) => glass::image_rounded(p, pic, 12.0, t.id(), t.size_vec2()),
                None => {
                    p.rect_filled(pic, 12.0, Color32::from_black_alpha(255));
                    ui.put(Rect::from_center_size(pic.center(), vec2(28.0, 28.0)), egui::Spinner::new().size(28.0).color(glass::ON_DARK));
                }
            }
            let bar = Rect::from_min_max(pos2(pic.left() + 10.0, pic.bottom() - 7.0), pos2(pic.right() - 10.0, pic.bottom() - 4.0));
            p.rect_filled(bar, 1.5, glass::rgba(glass::ON_DARK, 70));
            p.rect_filled(Rect::from_min_size(bar.min, vec2(bar.width() * progress.clamp(0.0, 1.0), bar.height())), 1.5, glass::ON_DARK);
            let hovered = ui.rect_contains_pointer(frame);
            if hovered || !playing {
                // A dark chip under the buttons keeps them readable on any picture.
                let mid = Rect::from_center_size(pic.center(), vec2(48.0, 48.0));
                glass::glass(p, mid, Glass::tinted(24.0, Color32::BLACK, 140).frosted(1.0));
                let corner_btn = |x: f32| Rect::from_min_size(pos2(x, pic.top() + 6.0), vec2(32.0, 32.0));
                if dark_button(ui, corner_btn(pic.right() - 38.0), "pip-close", Icon::Close).on_hover_text(l!("Закрити", "Close")).clicked() {
                    act = PipAct::Close;
                }
                if dark_button(ui, corner_btn(pic.left() + 6.0), "pip-expand", Icon::Maximize).on_hover_text(l!("Розгорнути", "Expand")).clicked() {
                    act = PipAct::Expand;
                }
                let icon = if playing { Icon::Pause } else { Icon::Play };
                if dark_button(ui, mid, "pip-play", icon).clicked() {
                    act = PipAct::Toggle;
                }
            }
            if matches!(act, PipAct::None) {
                if resp.double_clicked() {
                    act = PipAct::Expand;
                } else if resp.clicked() {
                    act = PipAct::Toggle;
                } else if resp.dragged() {
                    act = PipAct::Drag(resp.drag_delta());
                } else if resp.drag_stopped() {
                    act = PipAct::Drop;
                }
            }
        });
        match act {
            PipAct::None => {}
            PipAct::Toggle => self.video_toggle(),
            PipAct::Close => self.close_video(),
            PipAct::Expand => {
                if let Some(s) = &mut self.video.open {
                    s.pip = false;
                    s.drag = None;
                }
            }
            PipAct::Drag(d) => {
                if let Some(s) = &mut self.video.open {
                    s.drag = Some(s.drag.unwrap_or_default() + d);
                }
            }
            PipAct::Drop => {
                // Snaps to the nearest corner.
                if let Some(s) = &mut self.video.open {
                    let c = Rect::from_min_size(at, size).center();
                    s.corner = (c.x > screen.center().x) as u8 | ((c.y > screen.center().y) as u8) << 1;
                    s.drag = None;
                }
            }
        }
    }
}

enum PipAct {
    None,
    Toggle,
    Close,
    Expand,
    Drag(Vec2),
    Drop,
}

/// A round icon button for dark surfaces: light icon, a faint disc on hover.
fn dark_button(ui: &mut Ui, rect: Rect, id: &str, icon: Icon) -> egui::Response {
    let resp = ui.interact(rect, ui.scope_id().with(id), Sense::click());
    let hover = ui.ctx().animate_bool(resp.id.with("hover"), resp.hovered());
    let p = ui.painter();
    if hover > 0.0 {
        p.circle_filled(rect.center(), rect.width() / 2.0, glass::rgba(glass::ON_DARK, (hover * 34.0) as u8));
    }
    if resp.hovered() {
        ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
    }
    let side = (rect.height() * 0.5).min(22.0);
    // The play triangle looks centred a little to the right of its box.
    let nudge = if icon == Icon::Play { vec2(side * 0.08, 0.0) } else { Vec2::ZERO };
    icons::draw(p, Rect::from_center_size(rect.center() + nudge, Vec2::splat(side)), icon, glass::ON_DARK);
    resp
}

/// A thin track with its filled part and, when active, a knob.
fn slider(ui: &Ui, track: Rect, frac: f32, active: bool) {
    let p = ui.painter();
    let h = if active { 5.0 } else { 3.0 };
    let line = Rect::from_center_size(track.center(), vec2(track.width(), h));
    p.rect_filled(line, h / 2.0, glass::rgba(glass::ON_DARK, 60));
    let x = line.left() + line.width() * frac.clamp(0.0, 1.0);
    p.rect_filled(Rect::from_min_max(line.min, pos2(x, line.bottom())), h / 2.0, glass::ON_DARK);
    if active {
        p.circle_filled(pos2(x, line.center().y), 6.5, glass::ON_DARK);
    }
}
