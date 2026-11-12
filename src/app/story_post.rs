//! Posting a story, as in Telegram Desktop: "+" on our avatar in the story strip (or "Add story"
//! in Settings → My Stories) picks a photo or a video, and a glass box shows it as the story will
//! look, with the caption, who can see it, how long it stays, screenshots, keeping it on the
//! profile and posting as a channel.

use std::sync::{Arc, Mutex};

use super::*;
use super::info::chip;
use super::stories::StripPick;
use crate::backend::{StoryAudience, StoryDraft, is_story_video};

/// Periods offered, in hours; all but a day need Premium.
const PERIODS: [i32; 4] = [6, 12, 24, 48];
const PICK_EXTS: [&str; 9] = ["jpg", "jpeg", "png", "webp", "mp4", "mov", "m4v", "webm", "mkv"];
/// Longest side of the decoded preview, in pixels.
const PREVIEW_PX: u32 = 900;

#[derive(Default)]
pub(super) struct StoryPost {
    /// The file the system picker returned, taken on the next frame.
    picked: Arc<Mutex<Option<PathBuf>>>,
    draft: Option<Draft>,
    /// Channels and groups we may post as; asked once per session.
    targets: Vec<ChatInfo>,
    targets_asked: bool,
    /// When to reload the strip after `updateStory`: they come in bursts.
    reload_at: Option<f64>,
}

struct Draft {
    path: PathBuf,
    video: bool,
    preview: Option<TextureHandle>,
    decoded: Option<mpsc::Receiver<egui::ColorImage>>,
    clip: Option<Clip>,
    caption: String,
    target: Option<PeerId>,
    audience: StoryAudience,
    allow: Vec<PeerId>,
    screenshots: bool,
    hours: i32,
    pinned: bool,
    /// The `stories.canSendStory` answer for `target`.
    quota: Option<Result<i32, String>>,
    /// The contacts list for "Selected contacts" replaces the options.
    picking: bool,
    query: String,
    /// The upload in flight and its share done.
    upload: Option<(u64, f32)>,
}

enum Act {
    Close,
    Post,
    Target(Option<PeerId>),
    PremiumOnly,
}

impl StoryPost {
    /// Upload progress of the story being posted.
    pub(super) fn progress(&mut self, id: u64, done: u64, total: u64) {
        if let Some((_, share)) = self.draft.as_mut().and_then(|d| d.upload.as_mut()).filter(|(up, _)| *up == id) {
            *share = done as f32 / total.max(1) as f32;
        }
    }
}

impl App {
    /// The system picker for a photo or a video, off the UI thread.
    pub(super) fn add_story(&mut self) {
        let (picked, ctx) = (self.story_post.picked.clone(), self.ctx.clone());
        let title = l!("Нова історія", "New story");
        let kinds = l!("Фото й відео", "Photos and videos");
        std::thread::spawn(move || {
            if let Some(path) = rfd::FileDialog::new().set_title(title).add_filter(kinds, &PICK_EXTS).pick_file()
                && let Ok(mut slot) = picked.lock()
            {
                *slot = Some(path);
            }
            ctx.request_repaint();
        });
    }

    /// Opens the box over a picked file (the demo hands one in directly).
    pub fn open_story_post(&mut self, path: PathBuf) {
        let video = is_story_video(&path);
        let decoded = (!video).then(|| {
            let (tx, rx) = mpsc::channel();
            let (src, ctx) = (path.clone(), self.ctx.clone());
            std::thread::spawn(move || {
                if let Ok(img) = image::open(&src) {
                    let rgba = img.thumbnail(PREVIEW_PX, PREVIEW_PX).to_rgba8();
                    let _ = tx.send(egui::ColorImage::from_rgba_unmultiplied([rgba.width() as usize, rgba.height() as usize], rgba.as_raw()));
                }
                ctx.request_repaint();
            });
            rx
        });
        self.story_post.draft = Some(Draft {
            path,
            video,
            preview: None,
            decoded,
            clip: None,
            caption: String::new(),
            target: None,
            audience: StoryAudience::Everyone,
            allow: Vec::new(),
            screenshots: true,
            hours: 24,
            pinned: true,
            quota: None,
            picking: false,
            query: String::new(),
            upload: None,
        });
        if !self.story_post.targets_asked {
            self.story_post.targets_asked = true;
            self.send(Cmd::LoadStoryTargets);
        }
        self.send(Cmd::CanPostStory(None));
    }

    pub(super) fn story_post_event(&mut self, ev: Ev) {
        match ev {
            Ev::StoryTargets(list) => self.story_post.targets = list,
            Ev::StoryCanPost { target, result } => {
                if let Some(d) = self.story_post.draft.as_mut().filter(|d| d.target == target) {
                    d.quota = Some(result);
                }
            }
            Ev::StoryPosted { peer, story } => {
                self.story_post.draft = None;
                self.toast = Some((l!("Історію опубліковано", "Story posted").into(), 0.0, false));
                let Some(owner) = peer.or(self.profile.id) else { return };
                if let Some(p) = self.stories.peers.iter_mut().find(|p| p.chat.id == owner) {
                    match story {
                        Some(s) if !p.stories.is_empty() => {
                            p.ids.push(s.id);
                            p.stories.push(s);
                        }
                        // The viewer asks for the whole list again.
                        _ => p.stories.clear(),
                    }
                }
                self.send(Cmd::LoadStories);
                self.open_profile_stories(owner);
            }
            Ev::StoryPostFailed(e) => {
                if let Some(d) = &mut self.story_post.draft {
                    d.upload = None;
                }
                self.toast = Some((e, 0.0, true));
            }
            // Only peers we know can show up in the strip.
            Ev::StoryChanged(peer) if self.stories.peers.iter().any(|p| p.chat.id == peer) || self.chats.iter().any(|c| c.id == peer) => {
                let now = self.ctx.input(|i| i.time);
                self.story_post.reload_at.get_or_insert(now + 2.0);
            }
            _ => {}
        }
    }

    /// Our own cell at the head of the strip: our stories if any, with a "+" to add one.
    pub(super) fn own_story_cell(&self, ui: &mut Ui) -> Option<StripPick> {
        let me = self.profile.id?;
        let own = self.stories.peers.iter().find(|p| p.chat.id == me);
        let (r, resp) = ui.allocate_exact_size(vec2(64.0, 84.0), Sense::click());
        let c = pos2(r.center().x, r.top() + 30.0);
        let painter = ui.painter();
        if let Some(p) = own {
            let ring = if p.unread() { ACCENT } else { glass::rgba(ink_dim(), 120) };
            painter.circle_stroke(c, 27.0, egui::Stroke::new(if p.unread() { 2.5 } else { 1.5 }, ring));
        }
        let name = format!("{} {}", self.profile.first, self.profile.last);
        glass::avatar(painter, c, 23.0, &name, me.bot_api_dialog_id_unchecked(), None, self.textures.get(&ImgKey::Avatar(me)));
        let badge = Rect::from_center_size(c + vec2(17.0, 17.0), vec2(20.0, 20.0));
        painter.circle_filled(badge.center(), 10.0, glass::chrome());
        painter.circle_filled(badge.center(), 8.5, ACCENT);
        icons::draw(painter, badge.shrink(4.5), Icon::Plus, glass::ON_DARK);
        let label = if own.is_some() { l!("Моя історія", "My story") } else { l!("Додати", "Add story") };
        let g = elided(ui, label, 11.5, ink(), 62.0);
        painter.galley(pos2(r.center().x - g.size().x / 2.0, r.top() + 62.0), g, ink());
        let plus = ui.interact(badge.expand(4.0), ui.scope_id().with("own-story-add"), Sense::click()).on_hover_text(l!("Додати історію", "Add story"));
        if resp.hovered() || plus.hovered() {
            ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
        }
        if plus.clicked() {
            return Some(StripPick::Add);
        }
        let tip = if own.is_some() { l!("Мої історії", "My stories") } else { l!("Додати історію", "Add story") };
        resp.on_hover_text(tip).clicked().then_some(if own.is_some() { StripPick::Open(me) } else { StripPick::Add })
    }

    /// "Add story" above our own stories in Settings → My Stories.
    pub(super) fn add_story_button(&mut self, ui: &mut Ui) {
        let (r, _) = ui.allocate_exact_size(vec2(ui.available_width().min(220.0), 36.0), Sense::hover());
        if glass::button(ui, r, "settings-add-story", l!("Додати історію", "Add story"), true, true).clicked() {
            self.add_story();
        }
    }

    /// The picked file, the delayed strip reload and the box itself; called every frame.
    pub(super) fn story_post_view(&mut self, ui: &mut Ui) {
        let now = ui.input(|i| i.time);
        if self.story_post.reload_at.is_some_and(|t| now >= t) {
            self.story_post.reload_at = None;
            self.send(Cmd::LoadStories);
        }
        let picked = self.story_post.picked.lock().ok().and_then(|mut s| s.take());
        if let Some(path) = picked {
            self.open_story_post(path);
        }
        // Out of `self` while drawn: answers that come inside `send` (the demo) find no box.
        let Some(mut d) = self.story_post.draft.take() else { return };
        let act = self.story_post_box(ui, &mut d, now);
        self.story_post.draft = Some(d);
        match act {
            Some(Act::Close) => self.story_post.draft = None,
            Some(Act::Target(t)) => {
                if let Some(d) = &mut self.story_post.draft {
                    (d.target, d.quota) = (t, None);
                }
                self.send(Cmd::CanPostStory(t));
            }
            Some(Act::PremiumOnly) => self.toast = Some((l!("Інші терміни — з Telegram Premium", "Other durations need Telegram Premium").into(), 0.0, true)),
            Some(Act::Post) => {
                let Some(d) = &mut self.story_post.draft else { return };
                let upload = jiff::Timestamp::now().as_nanosecond() as u64;
                d.upload = Some((upload, 0.0));
                let draft = StoryDraft {
                    target: d.target,
                    path: d.path.clone(),
                    caption: d.caption.clone(),
                    audience: d.audience,
                    allow: d.allow.clone(),
                    noforwards: !d.screenshots,
                    period: d.hours * 3600,
                    pinned: d.pinned,
                    upload,
                };
                self.send(Cmd::PostStory(draft));
            }
            None => {}
        }
    }

    fn story_post_box(&mut self, ui: &mut Ui, d: &mut Draft, now: f64) -> Option<Act> {
        if let Some(img) = d.decoded.as_ref().and_then(|rx| rx.try_recv().ok()) {
            d.preview = Some(ui.ctx().load_texture("story-post-preview", img, TextureOptions::LINEAR));
            d.decoded = None;
        }
        let mut act = None;
        let screen = ui.max_rect();
        ui.painter().rect_filled(screen, 0.0, Color32::from_black_alpha(90));
        ui.interact(screen, ui.scope_id().with("story-post-bg"), Sense::click());
        let rect = Rect::from_center_size(screen.center(), vec2(screen.width().min(720.0) - 20.0, (screen.height() - 60.0).min(620.0)));
        glass::glass(ui.painter(), rect, Glass::panel(26.0).with_shadow());
        let inner = rect.shrink(18.0);
        let title = if d.picking { l!("Вибрані контакти", "Selected contacts") } else { l!("Нова історія", "New story") };
        ui.painter().text(inner.left_top(), Align2::LEFT_TOP, title, FontId::proportional(17.0), ink());
        let close = Rect::from_min_size(pos2(inner.right() - 32.0, inner.top() - 4.0), vec2(32.0, 32.0));
        let esc = ui.input_mut(|i| i.consume_key(egui::Modifiers::NONE, Key::Escape));
        if glass::icon_button(ui, close, "story-post-close", Icon::Close, false, true).on_hover_text(l!("Закрити", "Close")).clicked() || (esc && !d.picking) {
            return Some(Act::Close);
        }
        if esc {
            d.picking = false;
        }
        let buttons = Rect::from_min_max(pos2(inner.left(), inner.bottom() - 36.0), inner.max);
        let content = Rect::from_min_max(pos2(inner.left(), inner.top() + 40.0), pos2(inner.right(), buttons.top() - 12.0));
        // The story as a 9:16 card on the left, the options on the right.
        let pw = (content.height() * 9.0 / 16.0).min(content.width() * 0.45);
        let card = Rect::from_center_size(pos2(content.left() + pw / 2.0, content.center().y), vec2(pw, pw * 16.0 / 9.0));
        self.story_post_preview(ui, d, card, now);
        let side = Rect::from_min_max(pos2(card.right() + 16.0, content.top()), content.max);
        let mut child = ui.new_child(UiBuilder::new().max_rect(side));
        child.set_clip_rect(side.expand2(vec2(4.0, 0.0)));
        if d.picking {
            self.story_post_people(&mut child, d);
        } else {
            act = self.story_post_options(&mut child, d);
        }

        let half = (buttons.width() - 8.0) / 2.0;
        let left = Rect::from_min_size(buttons.min, vec2(half, buttons.height()));
        let right = Rect::from_min_size(pos2(buttons.right() - half, buttons.top()), vec2(half, buttons.height()));
        if d.picking {
            if glass::button(ui, right, "story-post-done", l!("Готово", "Done"), true, true).clicked() {
                d.picking = false;
            }
            return act;
        }
        if glass::button(ui, left, "story-post-cancel", l!("Скасувати", "Cancel"), false, true).clicked() {
            return Some(Act::Close);
        }
        match d.upload {
            Some((_, share)) => {
                glass::glass(ui.painter(), right, Glass::tinted(right.height() / 2.0, ACCENT, 110));
                bubbles::spinner(ui.painter(), pos2(right.left() + 22.0, right.center().y), 7.0);
                let pct = (share * 100.0).round() as i32;
                ui.painter().text(pos2(right.left() + 38.0, right.center().y), Align2::LEFT_CENTER, lf!("Завантаження {pct}%", "Uploading {pct}%"), FontId::proportional(14.0), glass::ON_DARK);
                ui.ctx().request_repaint();
            }
            None => {
                let ready = !matches!(d.quota, Some(Err(_))) && !(d.target.is_none() && d.audience == StoryAudience::Selected && d.allow.is_empty());
                if glass::button(ui, right, "story-post-send", l!("Опублікувати", "Post story"), true, ready).clicked() && ready {
                    act = Some(Act::Post);
                }
            }
        }
        act
    }

    /// The photo or the playing video, cropped to the card like the viewer does, with the caption.
    fn story_post_preview(&mut self, ui: &mut Ui, d: &mut Draft, card: Rect, now: f64) {
        let painter = ui.painter().clone();
        painter.rect_filled(card, 18.0, Color32::from_gray(18));
        if d.video && d.clip.is_none() {
            let ppp = ui.ctx().pixels_per_point();
            let even = |x: f32| ((x / 2.0).round() as usize * 2).max(2);
            let spec = Spec { path: d.path.clone(), size: [even(card.width() * ppp), even(card.height() * ppp)], from: 0.0, speed: 1.0, looped: true, contain: true };
            d.clip = Some(Clip::start(spec, 0, now));
        }
        if let Some(c) = &mut d.clip {
            c.advance(ui.ctx(), "story-post-clip", now);
            ui.ctx().request_repaint();
        }
        let tex = d.clip.as_ref().and_then(|c| c.tex.clone()).or_else(|| d.preview.clone());
        match tex {
            Some(tex) => {
                let size = tex.size_vec2();
                let scale = (card.width() / size.x).max(card.height() / size.y);
                let shown = vec2(card.width() / (size.x * scale), card.height() / (size.y * scale));
                let uv = Rect::from_center_size(pos2(0.5, 0.5), shown);
                painter.add(egui::epaint::RectShape::filled(card, 18.0, Color32::WHITE).with_texture(tex.id(), uv));
            }
            None => {
                ui.put(Rect::from_center_size(card.center(), vec2(28.0, 28.0)), egui::Spinner::new().size(28.0).color(glass::ON_DARK));
            }
        }
        if d.video {
            let chip = Rect::from_min_size(card.min + vec2(10.0, 10.0), vec2(34.0, 26.0));
            painter.rect_filled(chip, 13.0, Color32::from_black_alpha(130));
            icons::draw(&painter, Rect::from_center_size(chip.center(), vec2(16.0, 16.0)), Icon::Video, glass::ON_DARK);
        }
        // The caption as plain text: the viewer shows it the same way.
        let (text, _) = format::parse(d.caption.trim());
        if !text.is_empty() {
            let g = painter.layout(text, FontId::proportional(13.5), glass::ON_DARK, card.width() - 40.0);
            let h = g.size().y.min(card.height() * 0.4);
            let chip = Rect::from_min_size(pos2(card.left() + 10.0, card.bottom() - 10.0 - h - 18.0), vec2(card.width() - 20.0, h + 18.0));
            painter.rect_filled(chip, 14.0, Color32::from_black_alpha(140));
            painter.with_clip_rect(chip).galley(chip.min + vec2(10.0, 9.0), g, glass::ON_DARK);
        }
        if let Some((_, share)) = d.upload {
            painter.rect_filled(card, 18.0, Color32::from_black_alpha(110));
            let r = 26.0;
            painter.circle_stroke(card.center(), r, egui::Stroke::new(3.0, Color32::from_white_alpha(60)));
            let n = 48;
            let end = (share.clamp(0.02, 1.0) * n as f32) as usize;
            let pts: Vec<egui::Pos2> = (0..=end).map(|k| {
                let a = -std::f32::consts::FRAC_PI_2 + k as f32 / n as f32 * std::f32::consts::TAU;
                card.center() + vec2(a.cos(), a.sin()) * r
            }).collect();
            painter.add(egui::Shape::line(pts, egui::Stroke::new(3.0, glass::ON_DARK)));
        }
    }

    fn story_post_options(&mut self, ui: &mut Ui, d: &mut Draft) -> Option<Act> {
        let mut act = None;
        let mods = self.send_mods();
        egui::ScrollArea::vertical().id_salt("story-post").auto_shrink(false).show(ui, |ui| {
            ui.spacing_mut().item_spacing.y = 6.0;
            let w = ui.available_width() - 8.0;
            if !self.story_post.targets.is_empty() {
                section_title(ui, l!("Опублікувати як", "Post as"));
                ui.horizontal_wrapped(|ui| {
                    ui.spacing_mut().item_spacing = vec2(6.0, 6.0);
                    let me = format!("{} {}", self.profile.first, self.profile.last);
                    if chip(ui, me.trim(), d.target.is_none()).clicked() && d.target.is_some() {
                        act = Some(Act::Target(None));
                    }
                    for t in &self.story_post.targets {
                        if chip(ui, &t.title, d.target == Some(t.id)).clicked() && d.target != Some(t.id) {
                            act = Some(Act::Target(Some(t.id)));
                        }
                    }
                });
            }
            section_title(ui, l!("Підпис", "Caption"));
            let (r, _) = ui.allocate_exact_size(vec2(w, 76.0), Sense::hover());
            let (_, submit) = glass::rich_area(ui, r, &mut d.caption, l!("Додати підпис…", "Add a caption…"), mods);
            if submit && d.upload.is_none() {
                act = Some(Act::Post);
            }
            if d.target.is_none() {
                section_title(ui, l!("Хто бачитиме цю історію", "Who can see this story"));
                ui.horizontal_wrapped(|ui| {
                    ui.spacing_mut().item_spacing = vec2(6.0, 6.0);
                    let opts = [
                        (StoryAudience::Everyone, l!("Усі", "Everyone")),
                        (StoryAudience::Contacts, l!("Мої контакти", "My contacts")),
                        (StoryAudience::CloseFriends, l!("Близькі друзі", "Close friends")),
                        (StoryAudience::Selected, l!("Вибрані контакти", "Selected contacts")),
                    ];
                    for (a, label) in opts {
                        if chip(ui, label, d.audience == a).clicked() {
                            d.audience = a;
                            if a == StoryAudience::Selected && d.allow.is_empty() {
                                self.story_post_pick(d);
                            }
                        }
                    }
                });
                if d.audience == StoryAudience::Selected {
                    ui.horizontal(|ui| {
                        let n = d.allow.len() as i32;
                        let text = if n == 0 { l!("Нікого не вибрано", "No one selected").to_owned() } else { lp!(n, ["Вибрано {n} людину", "Вибрано {n} людини", "Вибрано {n} людей"], ["{n} person selected", "{n} people selected"]) };
                        ui.label(egui::RichText::new(text).color(ink_dim()).size(12.5));
                        if ui.link(l!("Вибрати…", "Choose…")).clicked() {
                            self.story_post_pick(d);
                        }
                    });
                }
            }
            section_title(ui, l!("Тривалість", "Duration"));
            let premium = self.profile.premium;
            ui.horizontal_wrapped(|ui| {
                ui.spacing_mut().item_spacing = vec2(6.0, 6.0);
                for h in PERIODS {
                    let locked = h != 24 && !premium;
                    let label = lp!(h, ["{n} година", "{n} години", "{n} годин"], ["{n} hour", "{n} hours"]);
                    let resp = ui.scope(|ui| {
                        if locked {
                            ui.multiply_opacity(0.45);
                        }
                        chip(ui, &label, d.hours == h)
                    }).inner;
                    if resp.clicked() {
                        if locked { act = Some(Act::PremiumOnly) } else { d.hours = h }
                    }
                }
            });
            ui.add_space(4.0);
            ui.checkbox(&mut d.screenshots, l!("Дозволити знімки екрана й пересилання", "Allow screenshots and forwarding"));
            let keep = if d.target.is_some() { l!("Показати в профілі каналу", "Post to the channel profile") } else { l!("Показати в моєму профілі", "Post to my profile") };
            ui.checkbox(&mut d.pinned, keep);
            match &d.quota {
                Some(Ok(n)) => {
                    let n = *n;
                    ui.label(egui::RichText::new(lp!(n, ["Сьогодні можна опублікувати ще {n} історію", "Сьогодні можна опублікувати ще {n} історії", "Сьогодні можна опублікувати ще {n} історій"], ["You can post {n} more story today", "You can post {n} more stories today"])).color(ink_dim()).size(12.5));
                }
                Some(Err(e)) => {
                    ui.label(egui::RichText::new(e).color(glass::danger()).size(12.5));
                }
                None => {}
            }
        });
        act
    }

    fn story_post_pick(&mut self, d: &mut Draft) {
        if self.contacts.is_empty() {
            self.send(Cmd::LoadContacts);
        }
        d.picking = true;
        d.query.clear();
    }

    /// Contacts with check marks for "Selected contacts".
    fn story_post_people(&mut self, ui: &mut Ui, d: &mut Draft) {
        let w = ui.available_width();
        let (field, _) = ui.allocate_exact_size(vec2(w, 38.0), Sense::hover());
        let r = glass::search_field(ui, field, &mut d.query, l!("Пошук контактів", "Search contacts"));
        if !r.has_focus() && d.query.is_empty() && !ui.memory(|m| m.focused().is_some()) {
            r.request_focus();
        }
        ui.add_space(8.0);
        let q = d.query.to_lowercase();
        let list: Vec<&ChatInfo> = self.contacts.iter().filter(|c| q.is_empty() || c.title.to_lowercase().contains(&q)).collect();
        let mut toggle = None;
        egui::ScrollArea::vertical().id_salt("story-post-people").auto_shrink(false).show(ui, |ui| {
            if list.is_empty() {
                ui.add_space(20.0);
                ui.vertical_centered(|ui| ui.label(egui::RichText::new(l!("Нікого не знайдено", "No one found")).color(ink_dim())));
            }
            for c in &list {
                let w = ui.available_width();
                let (r, resp) = ui.allocate_exact_size(vec2(w, 46.0), Sense::click());
                let p = ui.painter();
                if resp.hovered() {
                    p.rect_filled(r, 14.0, glass::veil(16));
                    ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
                }
                glass::avatar(p, pos2(r.left() + 26.0, r.center().y), 17.0, &c.title, c.id.bot_api_dialog_id_unchecked(), kind_icon(c.kind), self.textures.get(&ImgKey::Avatar(c.id)));
                p.galley(pos2(r.left() + 54.0, r.center().y - 9.0), elided(ui, &c.title, 14.5, ink(), w - 100.0), ink());
                let mark = Rect::from_center_size(pos2(r.right() - 22.0, r.center().y), vec2(22.0, 22.0));
                if d.allow.contains(&c.id) {
                    p.circle_filled(mark.center(), 11.0, ACCENT);
                    icons::draw(p, mark.shrink(4.0), Icon::Check, glass::ON_DARK);
                } else {
                    p.circle_stroke(mark.center(), 10.0, egui::Stroke::new(1.5, glass::rgba(ink_dim(), 150)));
                }
                if resp.clicked() {
                    toggle = Some(c.id);
                }
            }
        });
        if let Some(id) = toggle {
            match d.allow.iter().position(|p| *p == id) {
                Some(i) => {
                    d.allow.remove(i);
                }
                None => d.allow.push(id),
            }
        }
    }
}

