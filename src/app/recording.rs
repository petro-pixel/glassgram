//! Voice and round video messages: recording bars, the voice draft, sending.

use super::*;

/// Voice and round video recording: what is being recorded, a draft waiting to be sent.
pub(super) struct Recording {
    /// Voice message recording into a chat.
    pub(super) voice: Option<(PeerId, Recorder)>,
    /// Recorded but not yet sent voice message (preview playback).
    pub(super) draft: Option<(PeerId, Voice)>,
    /// Video message recording: chat, process, preview texture and number of the last frame.
    pub(super) round: Option<(PeerId, RoundRecorder)>,
    pub(super) round_tex: Option<TextureHandle>,
    pub(super) round_seen: u64,
    /// ffmpeg is there to record with.
    pub(super) available: bool,
}

impl App {
    pub(super) fn start_recording(&mut self, peer: PeerId) {
        if !self.rec.available {
            self.toast = Some((l!("Для запису потрібен ffmpeg", "Recording needs ffmpeg").into(), 0.0, true));
            return;
        }
        // Your own voice in headphones is distracting, so stop playback.
        self.playback.player.stop();
        match Recorder::start() {
            Ok(r) => self.rec.voice = Some((peer, r)),
            Err(e) => self.toast = Some((e, 0.0, true)),
        }
    }

    pub(super) fn cancel_recording(&mut self) {
        if let Some((_, r)) = self.rec.voice.take() {
            r.cancel();
        }
        if let Some((_, r)) = self.rec.round.take() {
            r.cancel();
        }
        if let Some((peer, v)) = self.rec.draft.take() {
            if self.playback.player.is(peer, PREVIEW_ID) {
                self.playback.player.stop();
            }
            v.discard();
        }
    }

    pub(super) fn start_round(&mut self, peer: PeerId) {
        if !self.rec.available {
            self.toast = Some((l!("Для запису потрібен ffmpeg", "Recording needs ffmpeg").into(), 0.0, true));
            return;
        }
        self.playback.player.stop();
        match RoundRecorder::start(&self.settings.camera) {
            Ok(r) => {
                self.rec.round = Some((peer, r));
                self.rec.round_tex = None;
                self.rec.round_seen = 0;
            }
            Err(e) => self.toast = Some((e, 0.0, true)),
        }
    }

    pub(super) fn send_round(&mut self) {
        let Some((peer, r)) = self.rec.round.take() else { return };
        self.rec.round_tex = None;
        let Some(round) = r.finish() else {
            self.toast = Some((l!("Відео закоротке або камера недоступна", "Video is too short or the camera is unavailable").into(), 0.0, true));
            return;
        };
        let reply_to = self.take_reply();
        self.next_upload += 1;
        let id = self.next_upload;
        let total = std::fs::metadata(&round.path).map_or(0, |m| m.len());
        self.uploads.push(Upload { id, name: l!("Відеоповідомлення", "Video message").into(), done: 0, total });
        self.send(Cmd::SendRound { id, peer, path: round.path, duration: round.duration, reply_to });
    }

    /// Round video recording: a round preview above the input field and a time row in the field.
    pub(super) fn round_bar(&mut self, ui: &mut Ui, input: Rect, chat: Rect, now: f64) {
        let Some((_, rec)) = self.rec.round.as_mut() else { return };
        let elapsed = rec.elapsed();
        if !rec.alive() {
            // ffmpeg exits by itself at the limit, then we send; earlier means the camera failed to open.
            if elapsed >= camera::MAX_SECS - 1.0 {
                self.send_round();
            } else {
                if let Some((_, r)) = self.rec.round.take() {
                    r.cancel();
                }
                self.toast = Some((l!("Камера недоступна", "Camera unavailable").into(), 0.0, true));
            }
            return;
        }
        if let Some((n, rgba)) = rec.frame_after(self.rec.round_seen) {
            self.rec.round_seen = n;
            let img = egui::ColorImage::from_rgba_unmultiplied([camera::PREVIEW, camera::PREVIEW], &rgba);
            match &mut self.rec.round_tex {
                Some(t) => t.set(img, TextureOptions::LINEAR),
                None => self.rec.round_tex = Some(ui.ctx().load_texture("round-preview", img, TextureOptions::LINEAR)),
            }
        }
        let p = ui.painter().clone();
        // Round preview.
        let side = (chat.height() * 0.5).clamp(160.0, 300.0);
        let c = pos2(chat.center().x, input.top() - 24.0 - side / 2.0);
        let r = Rect::from_center_size(c, vec2(side, side));
        p.rect_filled(chat, 0.0, Color32::from_black_alpha(70));
        glass::glass(&p, r.expand(6.0), Glass::panel(side / 2.0 + 6.0));
        match &self.rec.round_tex {
            Some(t) => glass::image_rounded(&p, r, side / 2.0, t.id(), t.size_vec2()),
            None => {
                p.circle_filled(c, side / 2.0, Color32::from_black_alpha(160));
                spinner(&p, c, 18.0);
            }
        }
        let frac = (elapsed / camera::MAX_SECS) as f32;
        let a0 = -std::f32::consts::FRAC_PI_2;
        let n = (frac * 128.0).ceil() as usize;
        let pts: Vec<_> = (0..=n).map(|k| a0 + k as f32 / 128.0 * std::f32::consts::TAU).map(|a| c + vec2(a.cos(), a.sin()) * (side / 2.0 + 3.0)).collect();
        if pts.len() > 1 {
            p.add(egui::Shape::line(pts, egui::Stroke::new(3.5, Color32::from_rgb(255, 80, 90))));
        }
        // Input field: dot, time and hint.
        glass::glass(&p, input, Glass::tinted(COMPOSER_H / 2.0, glass::chrome(), 110).with_shadow().frosted(0.96));
        let pulse = 0.55 + 0.45 * (now * 4.0).sin().abs() as f32;
        let dot = pos2(input.left() + 24.0, input.center().y);
        p.circle_filled(dot, 6.0, rgba(Color32::from_rgb(255, 80, 90), (255.0 * pulse) as u8));
        let time = format!("{} / {}", player::clock(elapsed), player::clock(camera::MAX_SECS));
        p.text(pos2(dot.x + 16.0, dot.y), Align2::LEFT_CENTER, time, FontId::proportional(15.0), ink());
        p.text(pos2(input.right() - 20.0, dot.y), Align2::RIGHT_CENTER, l!("Esc — скасувати", "Esc — cancel"), FontId::proportional(12.5), ink_dim());
    }

    /// Stop recording; `None` means too short.
    pub(super) fn finish_recording(&mut self) -> Option<(PeerId, Voice)> {
        let (peer, r) = self.rec.voice.take()?;
        let voice = r.finish();
        if voice.is_none() {
            self.toast = Some((l!("Запис закороткий", "Recording is too short").into(), 0.0, true));
        }
        Some((peer, voice?))
    }

    /// Send the recording: straight from recording mode or from preview playback.
    pub(super) fn send_recording(&mut self) {
        let next = match self.rec.draft.take() {
            Some(d) => Some(d),
            None => self.finish_recording(),
        };
        let Some((peer, voice)) = next else { return };
        if self.playback.player.is(peer, PREVIEW_ID) {
            self.playback.player.stop();
        }
        let reply_to = self.take_reply();
        self.next_upload += 1;
        let id = self.next_upload;
        let total = std::fs::metadata(&voice.path).map_or(0, |m| m.len());
        self.uploads.push(Upload { id, name: l!("Голосове повідомлення", "Voice message").into(), done: 0, total });
        self.send(Cmd::SendVoice { id, peer, path: voice.path, duration: voice.duration, waveform: voice.waveform, reply_to });
    }

    /// Input field while recording: red dot, time, live level and hint.
    pub(super) fn recording_bar(&mut self, ui: &mut Ui, input: Rect, now: f64) {
        let Some((_, rec)) = self.rec.voice.as_mut() else { return };
        if !rec.alive() {
            self.rec.voice = None;
            self.toast = Some((l!("Мікрофон недоступний", "Microphone unavailable").into(), 0.0, true));
            return;
        }
        let p = ui.painter().clone();
        glass::glass(&p, input, Glass::tinted(COMPOSER_H / 2.0, glass::chrome(), 110).with_shadow().frosted(0.96));
        // ■ stops and lets you listen before sending.
        let stop = Rect::from_center_size(pos2(input.left() + 25.0, input.center().y), vec2(36.0, 36.0));
        let stop_clicked = glass::icon_button(ui, stop, "rec-stop", Icon::Stop, false, true).on_hover_text(l!("Зупинити й прослухати", "Stop and listen")).clicked();
        let pulse = 0.55 + 0.45 * (now * 4.0).sin().abs() as f32;
        let dot = pos2(stop.right() + 16.0, input.center().y);
        p.circle_filled(dot, 6.0, rgba(Color32::from_rgb(255, 80, 90), (255.0 * pulse) as u8));
        let time = format!("{},{}", player::clock(rec.elapsed()), (rec.elapsed().fract() * 10.0) as u32);
        p.text(pos2(dot.x + 16.0, dot.y), Align2::LEFT_CENTER, time, FontId::proportional(15.0), ink());
        let hint = p.layout_no_wrap(l!("Esc — скасувати", "Esc — cancel").into(), FontId::proportional(12.5), ink_dim());
        let hint_x = input.right() - hint.size().x - 20.0;
        p.galley(pos2(hint_x, input.center().y - hint.size().y / 2.0), hint, ink_dim());
        // Level over the last seconds: bars from right to left, the newest on the right.
        let wave = Rect::from_min_max(pos2(dot.x + 80.0, input.top() + 12.0), pos2(hint_x - 16.0, input.bottom() - 12.0));
        let n = ((wave.width() / 4.0) as usize).max(1);
        let levels = rec.recent(n);
        for (k, v) in levels.iter().rev().enumerate() {
            let h = (2.0 + v.sqrt() * wave.height()).min(wave.height());
            let x = wave.right() - k as f32 * 4.0 - 2.4;
            p.rect_filled(Rect::from_min_max(pos2(x, wave.center().y - h / 2.0), pos2(x + 2.4, wave.center().y + h / 2.0)), 1.2, rgba(ink(), 200));
        }
        if stop_clicked {
            self.rec.draft = self.finish_recording();
        }
    }

    /// Recording preview: delete, play, waveform with progress, duration.
    pub(super) fn voice_draft_bar(&mut self, ui: &mut Ui, input: Rect) {
        let Some((peer, v)) = &self.rec.draft else { return };
        let (peer, path, duration) = (*peer, v.path.clone(), v.duration);
        let play = self.play_view(peer, PREVIEW_ID);
        let p = ui.painter().clone();
        glass::glass(&p, input, Glass::tinted(COMPOSER_H / 2.0, glass::chrome(), 110).with_shadow().frosted(0.96));
        let trash = Rect::from_center_size(pos2(input.left() + 25.0, input.center().y), vec2(36.0, 36.0));
        if glass::icon_button(ui, trash, "rec-trash", Icon::Trash, false, true).on_hover_text(l!("Видалити запис", "Delete recording")).clicked() {
            self.cancel_recording();
            return;
        }
        let btn = Rect::from_center_size(pos2(trash.right() + 24.0, input.center().y), vec2(36.0, 36.0));
        let icon = if play.playing { Icon::Pause } else { Icon::Play };
        if glass::icon_button(ui, btn, "rec-play", icon, true, true).clicked() {
            if play.current {
                if play.playing { self.playback.player.pause() } else { self.playback.player.resume() }
            } else {
                self.playback.player.play(peer, PREVIEW_ID, &path, duration);
            }
        }
        let time = if play.current { player::clock(play.position) } else { player::clock(duration) };
        let tg = p.layout_no_wrap(time, FontId::proportional(14.0), ink());
        let tx = input.right() - tg.size().x - 20.0;
        p.galley(pos2(tx, input.center().y - tg.size().y / 2.0), tg, ink());
        let wave = Rect::from_min_max(pos2(btn.right() + 14.0, input.top() + 13.0), pos2(tx - 16.0, input.bottom() - 13.0));
        let levels = self.rec.draft.as_ref().map(|(_, v)| v.waveform.clone()).unwrap_or_default();
        draw_wave(&p, wave, &levels, play.current.then_some(play.progress));
        if let Some(a) = seek_area(ui, PREVIEW_ID, wave, play) {
            if let Action::Seek(_, frac) = a {
                self.playback.player.seek(frac);
            }
        }
    }
}
