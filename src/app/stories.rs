//! Stories, as in Telegram Desktop: a strip of round avatars above the chat list (a coloured ring
//! while something is unseen) and a full-window viewer with progress segments that moves on by
//! itself, video stories with sound, reactions and private replies.

use std::path::PathBuf;

use super::*;
use crate::backend::{Story, StoryPeer};
use crate::video::{Clip, Spec};

/// A photo story stays on screen this long, like in Telegram.
const STORY_SECS: f64 = 6.0;
const STRIP_H: f32 = 84.0;
/// Reactions offered on right click of the heart, as in Telegram's quick row.
const QUICK: [&str; 6] = ["❤", "👍", "🔥", "😂", "😮", "😢"];

/// The strip and the viewer.
#[derive(Default)]
pub(super) struct Stories {
    /// People and channels with active stories, in the server's order.
    pub(super) peers: Vec<StoryPeer>,
    /// The story being watched.
    pub(super) view: Option<StoryView>,
    /// Video stories on disk, and those asked for.
    videos: HashMap<(PeerId, i32), PathBuf>,
    asked: HashSet<(PeerId, i32)>,
}

pub(super) struct StoryView {
    peer: PeerId,
    index: usize,
    /// When the current story appeared, shifted while paused.
    started: f64,
    paused_at: Option<f64>,
    /// The story the video below belongs to, and its frames; the player plays its sound.
    playing: Option<(PeerId, i32)>,
    clip: Option<Clip>,
    /// A private reply being typed; typing holds the story.
    reply: String,
    /// The pause comes from typing, not from Space.
    held: bool,
}

impl StoryView {
    fn new(peer: PeerId, now: f64) -> Self {
        Self { peer, index: usize::MAX, started: now, paused_at: None, playing: None, clip: None, reply: String::new(), held: false }
    }
}

/// The player key of a story's sound: below any message id, so the two never meet.
fn sound_key(id: i32) -> i32 {
    -1_000_000_000 - id
}

enum Step {
    Prev,
    Next,
    Close,
    TogglePause,
}

/// "14:05" today, "25.10 14:05" earlier.
fn story_time(ts: i64) -> String {
    let Ok(t) = jiff::Timestamp::from_second(ts) else { return String::new() };
    let z = t.to_zoned(jiff::tz::TimeZone::system());
    if z.date() == jiff::Zoned::now().date() { z.strftime("%H:%M").to_string() } else { z.strftime("%d.%m %H:%M").to_string() }
}

impl App {
    /// Story ids of a peer in the strip (for the demo's answers).
    pub fn stories_ids(&self, peer: PeerId) -> Vec<i32> {
        self.stories.peers.iter().find(|p| p.chat.id == peer).map(|p| p.ids.clone()).unwrap_or_default()
    }

    pub(super) fn on_stories(&mut self, mut list: Vec<StoryPeer>) {
        // Keep stories already loaded for the viewer.
        for p in &mut list {
            if let Some(old) = self.stories.peers.iter().find(|o| o.chat.id == p.chat.id) {
                p.stories = old.stories.clone();
            }
        }
        self.stories.peers = list;
    }

    pub(super) fn on_peer_stories(&mut self, peer: PeerId, stories: Vec<Story>, max_read: i32) {
        if let Some(p) = self.stories.peers.iter_mut().find(|p| p.chat.id == peer) {
            p.ids = stories.iter().map(|s| s.id).collect();
            p.stories = stories;
            p.max_read = p.max_read.max(max_read);
        }
        // Opened before the list came: start at the first unseen one.
        if let Some(v) = self.stories.view.as_mut().filter(|v| v.peer == peer && v.index == usize::MAX) {
            let p = self.stories.peers.iter().find(|p| p.chat.id == peer);
            v.index = p.and_then(|p| p.stories.iter().position(|s| s.id > p.max_read)).unwrap_or(0);
            v.started = self.ctx.input(|i| i.time);
        }
    }

    pub(super) fn open_stories(&mut self, peer: PeerId) {
        let now = self.ctx.input(|i| i.time);
        self.stop_story_video();
        self.stories.view = Some(StoryView::new(peer, now));
        match self.stories.peers.iter().find(|p| p.chat.id == peer).filter(|p| !p.stories.is_empty()) {
            Some(p) => {
                let first = p.stories.iter().position(|s| s.id > p.max_read).unwrap_or(0);
                if let Some(v) = &mut self.stories.view {
                    v.index = first;
                }
            }
            None => self.send(Cmd::LoadPeerStories(peer)),
        }
    }

    pub(super) fn on_story_video(&mut self, peer: PeerId, id: i32, path: PathBuf) {
        self.stories.videos.insert((peer, id), path);
    }

    /// The sound and frames of the video story that was on screen.
    fn stop_story_video(&mut self) {
        let Some(v) = &mut self.stories.view else { return };
        if let Some((peer, id)) = v.playing.take()
            && self.playback.player.is(peer, sound_key(id))
        {
            self.playback.player.stop();
        }
        v.clip = None;
    }

    /// The picture or video of a story over the card; `false` while a video is still downloading
    /// (time doesn't run until it plays).
    fn story_media(&mut self, ui: &mut Ui, peer: PeerId, story: Option<&Story>, card: Rect, now: f64) -> bool {
        let painter = ui.painter().clone();
        let spinner = |ui: &mut Ui| {
            ui.put(Rect::from_center_size(card.center(), vec2(28.0, 28.0)), egui::Spinner::new().size(28.0).color(glass::ON_DARK));
        };
        let Some(s) = story else {
            spinner(ui);
            return false;
        };
        let mut ready = true;
        if s.video {
            let key = (peer, s.id);
            match self.stories.videos.get(&key).cloned() {
                Some(path) => self.start_story_video(key, path, s.duration, card, now),
                None => {
                    if self.stories.asked.insert(key) {
                        self.send(Cmd::LoadStoryVideo { peer, id: s.id });
                    }
                    // Hold the timer at the start until the video is there.
                    if let Some(v) = &mut self.stories.view {
                        v.started = now;
                    }
                    ready = false;
                }
            }
        }
        let frame = self.stories.view.as_ref().and_then(|v| v.clip.as_ref()).and_then(|c| c.tex.clone());
        match frame.or_else(|| self.textures.get(&ImgKey::Story(peer, s.id)).cloned()) {
            Some(tex) => {
                // Cover the card, cropping what sticks out.
                let size = tex.size_vec2();
                let scale = (card.width() / size.x).max(card.height() / size.y);
                let shown = vec2(card.width() / (size.x * scale), card.height() / (size.y * scale));
                let uv = Rect::from_center_size(pos2(0.5, 0.5), shown);
                painter.add(egui::epaint::RectShape::filled(card, 18.0, Color32::WHITE).with_texture(tex.id(), uv));
            }
            None => spinner(ui),
        }
        if !ready {
            spinner(ui);
        }
        ready
    }

    /// The video of the story on screen: its sound in the player, its frames into the card.
    fn start_story_video(&mut self, key: (PeerId, i32), path: PathBuf, duration: f64, card: Rect, now: f64) {
        let ppp = self.ctx.pixels_per_point();
        let Some(v) = &mut self.stories.view else { return };
        if v.playing != Some(key) {
            let even = |x: f32| ((x / 2.0).round() as usize * 2).max(2);
            v.playing = Some(key);
            v.started = now;
            self.playback.player.play(key.0, sound_key(key.1), &path, duration.max(1.0));
            let run = self.playback.player.run().0;
            let spec = Spec { path, size: [even(card.width() * ppp), even(card.height() * ppp)], from: 0.0, speed: 1.0, looped: false, contain: false };
            v.clip = Some(Clip::start(spec, run, now));
        }
        if let Some(c) = &mut v.clip {
            c.advance(&self.ctx, &format!("story{}_{}", key.0.bot_api_dialog_id_unchecked(), key.1), now);
        }
    }

    /// The bottom of the card: a private reply (to people only) and our reaction.
    /// `true` while the reply field is in use.
    fn story_bar(&mut self, ui: &mut Ui, peer: PeerId, kind: ChatKind, story: &Story, bar: Rect) -> bool {
        let Some(v) = &mut self.stories.view else { return false };
        let heart = Rect::from_center_size(pos2(bar.right() - 20.0, bar.center().y), vec2(40.0, 40.0));
        let mut typing = false;
        let mut send = None;
        if kind == ChatKind::User {
            let field = Rect::from_min_max(bar.min, pos2(heart.left() - 6.0, bar.bottom()));
            let resp = glass::text_field(ui, field, &mut v.reply, l!("Відповісти приватно…", "Reply privately…"), false);
            let enter = resp.lost_focus() && ui.input(|i| i.key_pressed(Key::Enter));
            typing = resp.has_focus() || !v.reply.trim().is_empty();
            if enter && !v.reply.trim().is_empty() {
                send = Some(std::mem::take(&mut v.reply).trim().to_owned());
                typing = false;
            }
        }
        let mine = story.reaction.clone();
        let tip = l!("Реакція (права кнопка — інші)", "React (right click for more)");
        let resp = glass::icon_button(ui, heart, ("story-react", story.id), Icon::Heart, mine.is_some(), true).on_hover_text(tip);
        let mut react = resp.clicked().then(|| if mine.is_some() { None } else { Some("❤".to_owned()) });
        resp.context_menu(|ui| {
            ui.horizontal(|ui| {
                for e in QUICK {
                    if ui.button(egui::RichText::new(e).size(20.0)).clicked() {
                        react = Some(Some(e.to_owned()));
                        ui.close();
                    }
                }
            });
        });
        if let Some(text) = send {
            self.send(Cmd::ReplyStory { peer, id: story.id, text });
        }
        if let Some(emoji) = react {
            if let Some(s) = self.stories.peers.iter_mut().find(|p| p.chat.id == peer).and_then(|p| p.stories.iter_mut().find(|s| s.id == story.id)) {
                s.reaction = emoji.clone();
            }
            self.send(Cmd::ReactStory { peer, id: story.id, emoji });
        }
        typing
    }

    /// Round avatars of everyone with active stories; `None` when there are none.
    pub(super) fn story_strip(&self, ui: &mut Ui) -> Option<PeerId> {
        if self.stories.peers.is_empty() {
            return None;
        }
        let mut open = None;
        egui::ScrollArea::horizontal().id_salt("story-strip").auto_shrink([false, true]).scroll_bar_visibility(egui::scroll_area::ScrollBarVisibility::AlwaysHidden).show(ui, |ui| {
            ui.horizontal(|ui| {
                ui.spacing_mut().item_spacing.x = 4.0;
                ui.add_space(4.0);
                for p in &self.stories.peers {
                    let (r, resp) = ui.allocate_exact_size(vec2(64.0, STRIP_H), Sense::click());
                    let c = pos2(r.center().x, r.top() + 30.0);
                    let painter = ui.painter();
                    let ring = if p.unread() { ACCENT } else { glass::rgba(ink_dim(), 120) };
                    painter.circle_stroke(c, 27.0, egui::Stroke::new(if p.unread() { 2.5 } else { 1.5 }, ring));
                    let chat = &p.chat;
                    glass::avatar(painter, c, 23.0, &chat.title, chat.id.bot_api_dialog_id_unchecked(), kind_icon(chat.kind), self.textures.get(&ImgKey::Avatar(chat.id)));
                    let name = chat.title.split_whitespace().next().unwrap_or_default();
                    let g = elided(ui, name, 11.5, ink(), 62.0);
                    painter.galley(pos2(r.center().x - g.size().x / 2.0, r.top() + 62.0), g, ink());
                    if resp.hovered() {
                        ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
                    }
                    if resp.on_hover_text(&chat.title).clicked() {
                        open = Some(chat.id);
                    }
                }
            });
        });
        open
    }

    /// The viewer over the whole window.
    pub(super) fn story_viewer(&mut self, ui: &mut Ui) {
        let Some(v) = &self.stories.view else { return };
        let now = ui.input(|i| i.time);
        let Some(pi) = self.stories.peers.iter().position(|p| p.chat.id == v.peer) else {
            self.stories.view = None;
            return;
        };
        let screen = ui.max_rect();
        let painter = ui.painter().clone();
        painter.rect_filled(screen, 0.0, Color32::from_black_alpha(235));
        let bg = ui.interact(screen, ui.scope_id().with("story-bg"), Sense::click());
        let mut step = None;
        let close = Rect::from_min_size(pos2(screen.right() - 58.0, screen.top() + 16.0), vec2(42.0, 42.0));
        if glass::icon_button(ui, close, "story-close", Icon::Close, false, true).on_hover_text(l!("Закрити (Esc)", "Close (Esc)")).clicked() {
            step = Some(Step::Close);
        }
        // Keys go to the reply field while it has focus.
        let field = ui.memory(|m| m.focused().is_some());
        ui.input(|i| {
            if field {
                // Esc first leaves the field (egui does that itself).
            } else if i.key_pressed(Key::Escape) {
                step = Some(Step::Close);
            } else if i.key_pressed(Key::ArrowRight) {
                step = Some(Step::Next);
            } else if i.key_pressed(Key::ArrowLeft) {
                step = Some(Step::Prev);
            } else if i.key_pressed(Key::Space) {
                step = Some(Step::TogglePause);
            }
        });

        // A 9:16 card, as tall as the window allows.
        let h = (screen.height() - 48.0).min(820.0);
        let card = Rect::from_center_size(screen.center(), vec2(h * 9.0 / 16.0, h));
        let sp = &self.stories.peers[pi];
        let chat = sp.chat.clone();
        let n = sp.stories.len().max(1);
        painter.rect_filled(card, 18.0, Color32::from_gray(18));
        let story = sp.stories.get(v.index).cloned();
        let ready = self.story_media(ui, chat.id, story.as_ref(), card, now);

        // Progress segments and who posted it, on a dark chip for any picture.
        let top = Rect::from_min_size(card.min + vec2(10.0, 10.0), vec2(card.width() - 20.0, 3.0));
        let seg_w = (top.width() - (n as f32 - 1.0) * 4.0) / n as f32;
        let Some(v) = &self.stories.view else { return };
        let secs = story.as_ref().filter(|s| s.video).map_or(STORY_SECS, |s| s.duration.max(1.0));
        let progress = if ready { ((v.paused_at.unwrap_or(now) - v.started) / secs).clamp(0.0, 1.0) as f32 } else { 0.0 };
        for k in 0..n {
            let r = Rect::from_min_size(pos2(top.left() + k as f32 * (seg_w + 4.0), top.top()), vec2(seg_w, 3.0));
            painter.rect_filled(r, 1.5, Color32::from_white_alpha(70));
            let fill = if k < v.index { 1.0 } else if k == v.index { progress } else { 0.0 };
            if fill > 0.0 {
                painter.rect_filled(Rect::from_min_size(r.min, vec2(r.width() * fill, 3.0)), 1.5, glass::ON_DARK);
            }
        }
        let head = Rect::from_min_size(card.min + vec2(10.0, 22.0), vec2(card.width() - 20.0, 44.0));
        painter.rect_filled(head, 22.0, Color32::from_black_alpha(110));
        glass::avatar(&painter, pos2(head.left() + 22.0, head.center().y), 17.0, &chat.title, chat.id.bot_api_dialog_id_unchecked(), kind_icon(chat.kind), self.textures.get(&ImgKey::Avatar(chat.id)));
        let when = story.as_ref().map(|s| story_time(s.date)).unwrap_or_default();
        let name = elided(ui, &chat.title, 14.0, glass::ON_DARK, head.width() - 140.0);
        let name_w = name.size().x;
        painter.galley(pos2(head.left() + 46.0, head.center().y - name.size().y / 2.0), name, glass::ON_DARK);
        painter.text(pos2(head.left() + 54.0 + name_w, head.center().y), Align2::LEFT_CENTER, when, FontId::proportional(12.0), glass::rgba(glass::ON_DARK, 170));
        if v.paused_at.is_some() {
            painter.text(pos2(head.right() - 14.0, head.center().y), Align2::RIGHT_CENTER, l!("пауза", "paused"), FontId::proportional(12.0), glass::ON_DARK);
        }
        // The reply bar sits at the bottom of the card; the caption above it.
        let bar = Rect::from_min_size(pos2(card.left() + 10.0, card.bottom() - 50.0), vec2(card.width() - 20.0, 40.0));
        if let Some(s) = story.as_ref().filter(|s| !s.caption.is_empty()) {
            let g = ui.painter().layout(s.caption.clone(), FontId::proportional(14.0), glass::ON_DARK, card.width() - 44.0);
            let chip = Rect::from_min_size(pos2(card.left() + 10.0, bar.top() - 10.0 - g.size().y - 20.0), vec2(card.width() - 20.0, g.size().y + 20.0));
            painter.rect_filled(chip, 16.0, Color32::from_black_alpha(140));
            painter.galley(chip.min + vec2(12.0, 10.0), g, glass::ON_DARK);
        }
        let typing = story.as_ref().is_some_and(|s| self.story_bar(ui, chat.id, chat.kind, s, bar));
        // Left third goes back, the rest forward; outside the card closes.
        if bg.clicked()
            && let Some(pos) = bg.interact_pointer_pos()
        {
            step = Some(if !card.contains(pos) { Step::Close } else if pos.x < card.left() + card.width() / 3.0 { Step::Prev } else { Step::Next });
        }
        // Typing a reply holds the story, like a pause, and goes on once the field is left empty.
        let held = self.stories.view.as_ref().map(|v| (v.paused_at.is_some(), v.held));
        match held {
            Some((false, _)) if typing => self.story_pause(true, now, true),
            Some((true, true)) if !typing => self.story_pause(false, now, false),
            _ => {}
        }
        if typing && !matches!(step, Some(Step::Close)) {
            step = None;
        }
        if let Some(v) = &self.stories.view
            && step.is_none()
            && ready
            && v.paused_at.is_none()
            && now - v.started > secs
        {
            step = Some(Step::Next);
        }
        // Seen: the owner gets us in the viewers list, as with any client.
        if let Some(s) = &story
            && s.id > self.stories.peers[pi].max_read
        {
            self.stories.peers[pi].max_read = s.id;
            self.send(Cmd::ReadStories { peer: chat.id, max_id: s.id });
        }
        if let Some(step) = step {
            self.story_step(step, pi, now);
        }
    }

    /// Pause or go on; a video's sound and frames follow.
    fn story_pause(&mut self, pause: bool, now: f64, held: bool) {
        let Some(v) = &mut self.stories.view else { return };
        let sound = v.playing.filter(|(p, id)| self.playback.player.is(*p, sound_key(*id)));
        if pause {
            v.paused_at.get_or_insert(now);
            v.held = held;
            if let Some(c) = &mut v.clip {
                c.stop();
            }
            if sound.is_some() {
                self.playback.player.pause();
            }
            return;
        }
        let Some(at) = v.paused_at.take() else { return };
        // Where the story stopped: the frames start again from there.
        let from = at - v.started;
        v.started += now - at;
        v.held = false;
        if let (Some(key), Some(c)) = (v.playing, &mut v.clip)
            && let Some(path) = self.stories.videos.get(&key).cloned()
        {
            if sound.is_some() {
                self.playback.player.resume();
            }
            let run = if sound.is_some() { self.playback.player.run().0 } else { 0 };
            *c = Clip::start(Spec { path, size: c.size(), from, speed: 1.0, looped: false, contain: false }, run, now);
        }
    }

    fn story_step(&mut self, step: Step, pi: usize, now: f64) {
        if matches!(step, Step::TogglePause) {
            let paused = self.stories.view.as_ref().is_some_and(|v| v.paused_at.is_some());
            return self.story_pause(!paused, now, false);
        }
        self.stop_story_video();
        let Some(v) = &mut self.stories.view else { return };
        let count = self.stories.peers[pi].stories.len();
        match step {
            Step::Close | Step::TogglePause => self.stories.view = None,
            Step::Prev if v.index > 0 && v.index != usize::MAX => {
                v.index -= 1;
                v.started = now;
                v.paused_at = None;
                v.reply.clear();
            }
            Step::Prev => match pi.checked_sub(1) {
                Some(prev) => {
                    let peer = self.stories.peers[prev].chat.id;
                    self.open_stories(peer);
                }
                None => v.started = now,
            },
            Step::Next if v.index != usize::MAX && v.index + 1 < count => {
                v.index += 1;
                v.started = now;
                v.paused_at = None;
                v.reply.clear();
            }
            Step::Next => match self.stories.peers.get(pi + 1).map(|p| p.chat.id) {
                Some(next) => self.open_stories(next),
                None => self.stories.view = None,
            },
        }
    }
}
