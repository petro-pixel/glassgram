//! Name colour, profile colour and emoji status, as in Telegram Desktop: authors' names and reply
//! stripes in their own colour, an emoji status after the name, and the pickers in My Account.

use super::*;
use crate::backend::{ColorOption, Profile};
use eframe::egui::{Painter, Pos2, Vec2};

/// Side of an emoji status after a name.
pub(super) const STATUS_PT: f32 = 18.0;
const SWATCH: f32 = 34.0;

/// The pickers of My Account.
#[derive(Default)]
pub(super) struct NameColors {
    /// Emoji statuses to pick from; `None` until they come.
    statuses: Option<Vec<i64>>,
    /// Name colour picked and not saved yet (`Some(None)`: back to the default).
    name: Option<Option<i32>>,
    /// The same for the profile colour.
    profile: Option<Option<i32>>,
}

impl App {
    pub(super) fn on_peer_colors(&mut self, names: Vec<ColorOption>, profiles: Vec<ColorOption>) {
        let plain = |l: Vec<ColorOption>| l.into_iter().map(|o| (o.id, o.light, o.dark)).collect();
        glass::set_peer_colors(plain(names), plain(profiles));
    }

    pub(super) fn on_emoji_statuses(&mut self, ids: Vec<i64>) {
        self.name_colors.statuses = Some(ids);
    }

    /// The name colour of a message's author; ours for our own messages.
    pub(super) fn author_color(&self, m: &Msg) -> Option<i32> {
        if m.out { self.profile.color.0.or(self.profile.id.map(crate::backend::default_color)) } else { m.sender_color }
    }

    /// My Account opened: pickers start from the saved values.
    pub(super) fn open_name_colors(&mut self) {
        self.name_colors.name = None;
        self.name_colors.profile = None;
        if self.name_colors.statuses.is_none() {
            self.send(Cmd::LoadEmojiStatuses);
        }
    }
}

/// The colour of a message author's name. Messages cached before colours were kept fall back
/// to a colour picked from the name.
pub(super) fn sender_color(m: &Msg) -> Color32 {
    match m.sender_color {
        Some(c) => glass::peer_color(c),
        None => glass::name_color(m.sender.len() as i64 * 31 + m.sender.chars().next().map_or(0, |c| c as i64)),
    }
}

/// Draws an emoji status at the left middle of `at`; returns the width it took (0 without one).
pub(super) fn status_after(p: &Painter, at: Pos2, status: Option<i64>, side: f32) -> f32 {
    let Some(id) = status else { return 0.0 };
    custom_emoji::paint_id(p, id, Rect::from_min_size(pos2(at.x + 4.0, at.y - side / 2.0), Vec2::splat(side)));
    side + 4.0
}

/// A round swatch; several tones share it in slices, like the multi-colour stripes.
fn swatch(ui: &mut Ui, tones: &[Color32], selected: bool, locked: bool) -> egui::Response {
    let (r, resp) = ui.allocate_exact_size(Vec2::splat(SWATCH), Sense::click());
    let p = ui.painter();
    let c = r.center();
    let rad = SWATCH / 2.0 - 4.0;
    match tones {
        [] => {}
        [one] => {
            p.circle_filled(c, rad, *one);
        }
        [a, rest @ ..] => {
            p.circle_filled(c, rad, *a);
            // The other tones fill the lower right, as Telegram Desktop shows two-tone colours.
            for (k, t) in rest.iter().enumerate() {
                let start = std::f32::consts::FRAC_PI_4 * (1.0 - 4.0 * k as f32 / rest.len() as f32) - std::f32::consts::PI;
                let span = std::f32::consts::PI / rest.len() as f32;
                let pts: Vec<Pos2> = std::iter::once(c).chain((0..=16).map(|i| c + Vec2::angled(-(start + span * i as f32 / 16.0)) * rad)).collect();
                p.add(egui::Shape::convex_polygon(pts, *t, egui::Stroke::NONE));
            }
        }
    }
    if selected {
        p.circle_stroke(c, SWATCH / 2.0 - 1.0, egui::Stroke::new(2.0, tones.first().copied().unwrap_or(ink())));
    }
    if locked {
        icons::draw(p, Rect::from_center_size(c, Vec2::splat(14.0)), Icon::Lock, glass::ON_DARK);
    }
    if resp.hovered() {
        ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
    }
    resp
}

/// The live preview of a name colour: our name and a reply to us, as others will see them.
fn name_preview(ui: &mut Ui, name: &str, color: i32) {
    let w = ui.available_width();
    let (r, _) = ui.allocate_exact_size(vec2(w, 92.0), Sense::hover());
    let p = ui.painter();
    p.rect_filled(r, 18.0, glass::veil(10));
    let bubble = Rect::from_min_size(r.min + vec2(12.0, 10.0), vec2((w - 24.0).min(320.0), 72.0));
    glass::glass(p, bubble, Glass::tinted(16.0, glass::bubble_in().0, glass::bubble_in().1));
    let tone = glass::peer_color(color);
    let x = bubble.left() + 12.0;
    p.galley(pos2(x, bubble.top() + 6.0), elided(ui, name, 13.0, tone, bubble.width() - 24.0), tone);
    let quote = Rect::from_min_size(pos2(x, bubble.top() + 26.0), vec2(bubble.width() - 24.0, 36.0));
    p.rect_filled(quote, 6.0, glass::rgba(tone, 30));
    glass::stripe(p, Rect::from_min_size(quote.min, vec2(3.0, quote.height())), &glass::peer_stripe(color));
    p.galley(quote.min + vec2(10.0, 3.0), elided(ui, name, 12.5, tone, quote.width() - 16.0), tone);
    let text = l!("Відповідь на ваше повідомлення", "Reply to your message");
    p.galley(quote.min + vec2(10.0, 19.0), elided(ui, text, 12.5, ink_dim(), quote.width() - 16.0), ink_dim());
}

/// The live preview of a profile colour: the cover with our name on it.
fn profile_preview(ui: &mut Ui, name: &str, color: Option<i32>) {
    let w = ui.available_width();
    let (r, _) = ui.allocate_exact_size(vec2(w, 64.0), Sense::hover());
    let p = ui.painter();
    match color.map(glass::profile_tones) {
        Some(t) => {
            let (top, bottom) = (t[0], t.get(1).copied().unwrap_or(t[0]));
            glass::gradient_rect(p, r, 18.0, top, bottom);
            p.galley(pos2(r.left() + 16.0, r.center().y - 9.0), elided(ui, name, 15.0, glass::ON_DARK, w - 32.0), glass::ON_DARK);
        }
        None => {
            p.rect_filled(r, 18.0, glass::veil(10));
            p.galley(pos2(r.left() + 16.0, r.center().y - 9.0), elided(ui, name, 15.0, ink(), w - 32.0), ink());
        }
    }
}

/// Save and reset under a picker; returns the colour to save.
fn save_row(ui: &mut Ui, id: &str, picked: Option<i32>, saved: Option<i32>) -> Option<Option<i32>> {
    let mut out = None;
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = 6.0;
        let (r, _) = ui.allocate_exact_size(vec2(160.0, 34.0), Sense::hover());
        if glass::button(ui, r, (id, "save"), l!("Зберегти", "Save"), true, picked != saved).clicked() && picked != saved {
            out = Some(picked);
        }
        let (r, _) = ui.allocate_exact_size(vec2(160.0, 34.0), Sense::hover());
        if glass::button(ui, r, (id, "reset"), l!("Типовий", "Default"), false, saved.is_some()).clicked() {
            out = Some(None);
        }
    });
    out
}

/// Name colour, profile colour and emoji status in My Account.
pub(super) fn appearance_section(ui: &mut Ui, s: &mut NameColors, p: &Profile, name: &str) -> Option<Cmd> {
    let mut cmd = None;
    let locked = !p.premium;
    let own = p.id.map_or(0, crate::backend::default_color);

    section_title(ui, l!("Колір імені", "Name color"));
    let saved = p.color.0;
    let picked = *s.name.get_or_insert(saved);
    name_preview(ui, name, picked.unwrap_or(own));
    ui.horizontal_wrapped(|ui| {
        ui.spacing_mut().item_spacing = vec2(2.0, 2.0);
        for id in glass::name_color_ids() {
            if premium_tip(swatch(ui, &glass::peer_stripe(id), picked.unwrap_or(own) == id, locked), locked).clicked() {
                s.name = Some(Some(id));
            }
        }
    });
    if let Some(color) = save_row(ui, "name-color", picked, saved) {
        s.name = Some(color);
        cmd = Some(Cmd::SetNameColor { profile: false, color, emoji: p.color.1 });
    }
    super::settings_pages::hint(ui, l!(
        "Цим кольором ваше ім'я та цитати з ваших повідомлень показуються в групах і відповідях.",
        "Your name and quotes of your messages are shown in this color in groups and replies."
    ));

    section_title(ui, l!("Колір профілю", "Profile color"));
    let saved = p.profile_color.0;
    let picked = *s.profile.get_or_insert(saved);
    profile_preview(ui, name, picked);
    ui.horizontal_wrapped(|ui| {
        ui.spacing_mut().item_spacing = vec2(2.0, 2.0);
        for id in glass::profile_color_ids() {
            if premium_tip(swatch(ui, &glass::profile_tones(id), picked == Some(id), locked), locked).clicked() {
                s.profile = Some(Some(id));
            }
        }
    });
    if let Some(color) = save_row(ui, "profile-color", picked, saved) {
        s.profile = Some(color);
        cmd = Some(Cmd::SetNameColor { profile: true, color, emoji: p.profile_color.1 });
    }

    section_title(ui, l!("Емодзі-статус", "Emoji status"));
    match &s.statuses {
        None => super::settings_pages::hint(ui, l!("Завантаження…", "Loading…")),
        Some(ids) => {
            ui.horizontal_wrapped(|ui| {
                ui.spacing_mut().item_spacing = vec2(2.0, 2.0);
                // "None" first, then the statuses: one click sets it, like in Telegram.
                let (r, resp) = ui.allocate_exact_size(Vec2::splat(40.0), Sense::click());
                status_cell(ui, r, &resp, p.emoji_status.is_none());
                icons::draw(ui.painter(), Rect::from_center_size(r.center(), Vec2::splat(18.0)), Icon::Close, ink_dim());
                if resp.on_hover_text(l!("Без статусу", "No status")).clicked() && p.emoji_status.is_some() {
                    cmd = Some(Cmd::SetEmojiStatus(None));
                }
                for &id in ids {
                    let (r, resp) = ui.allocate_exact_size(Vec2::splat(40.0), Sense::click());
                    status_cell(ui, r, &resp, p.emoji_status == Some(id));
                    custom_emoji::paint_id(ui.painter(), id, Rect::from_center_size(r.center(), Vec2::splat(26.0)));
                    if locked {
                        icons::draw(ui.painter(), Rect::from_min_size(r.right_bottom() - vec2(14.0, 14.0), Vec2::splat(12.0)), Icon::Lock, ink_dim());
                    }
                    if premium_tip(resp, locked).clicked() && p.emoji_status != Some(id) {
                        cmd = Some(Cmd::SetEmojiStatus(Some(id)));
                    }
                }
            });
        }
    }
    super::settings_pages::hint(ui, l!("Емодзі-статус стоїть після вашого імені у списку чатів і профілі.", "Your emoji status is shown after your name in chat lists and your profile."));
    if locked {
        ui.label(egui::RichText::new(l!(
            "Кольори та емодзі-статус доступні з Telegram Premium.",
            "Name colors and emoji status are available with Telegram Premium."
        )).color(ACCENT).size(12.5));
    }
    cmd
}

/// Locked choices say why on hover.
fn premium_tip(resp: egui::Response, locked: bool) -> egui::Response {
    if locked { resp.on_hover_text(l!("Потрібен Telegram Premium", "Needs Telegram Premium")) } else { resp }
}

fn status_cell(ui: &Ui, r: Rect, resp: &egui::Response, selected: bool) {
    if selected {
        ui.painter().rect_filled(r, 12.0, glass::rgba(ACCENT, 60));
    } else if resp.hovered() {
        ui.painter().rect_filled(r, 12.0, glass::veil(16));
        ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
    }
}
