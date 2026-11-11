//! View-once and timed photos and videos, as Telegram Desktop shows them: a blurred cover with a
//! "1" (or the timer) instead of the picture; a click opens it in the viewer or the player, and
//! once that is closed the message says the media has expired. Ours stay covered for us too.

use super::*;
use crate::backend::VIEW_ONCE;

#[derive(Default)]
pub(super) struct ViewOnce {
    /// Opened media; `true` once the viewer or the player has shown it.
    opened: Vec<((PeerId, i32), bool)>,
}

/// The cover over self-destructing media: dimmed, with the "view once" mark or the timer.
pub(super) fn cover(p: &egui::Painter, r: Rect, radius: f32, m: &Msg) {
    p.rect_filled(r, radius, Color32::from_black_alpha(90));
    let c = Rect::from_center_size(r.center() - vec2(0.0, 12.0), vec2(54.0, 54.0));
    glass::glass(p, c, Glass::tinted(27.0, Color32::BLACK, 110));
    let once = m.ttl == Some(VIEW_ONCE);
    if once {
        // A dashed ring around a "1", Telegram's view-once mark.
        let n = 12;
        for k in 0..n {
            let a0 = k as f32 / n as f32 * std::f32::consts::TAU;
            let a1 = a0 + std::f32::consts::TAU / n as f32 * 0.6;
            let pts: Vec<_> = (0..=4).map(|i| a0 + (a1 - a0) * i as f32 / 4.0).map(|a| c.center() + vec2(a.cos(), a.sin()) * 14.0).collect();
            p.add(egui::Shape::line(pts, egui::Stroke::new(2.0, glass::ON_DARK)));
        }
        p.text(c.center(), Align2::CENTER_CENTER, "1", FontId::proportional(15.0), glass::ON_DARK);
    } else {
        icons::draw(p, Rect::from_center_size(c.center(), vec2(24.0, 24.0)), Icon::Clock, glass::ON_DARK);
    }
    let label = match m.ttl {
        Some(VIEW_ONCE) | None => l!("Одноразовий перегляд", "View once").to_owned(),
        Some(secs) => lf!("Зникне через {secs} с після перегляду", "Disappears {secs} s after viewing"),
    };
    let g = p.layout_no_wrap(label, FontId::proportional(12.5), glass::ON_DARK);
    let chip = Rect::from_center_size(pos2(r.center().x, c.bottom() + 18.0), g.size() + vec2(16.0, 8.0));
    p.rect_filled(chip, chip.height() / 2.0, Color32::from_black_alpha(120));
    p.galley(chip.min + vec2(8.0, 4.0), g, glass::ON_DARK);
}

impl App {
    /// A click on self-destructing media: received media opens (and is marked viewed, so it
    /// expires for both sides); ours can't be opened, as in the official apps.
    pub(super) fn open_view_once(&mut self, peer: PeerId, action: Action) -> Option<Action> {
        let (Action::View(id) | Action::Play(id)) = action else { return Some(action) };
        let out = self.find(peer, id).is_some_and(|m| m.out);
        if out {
            self.toast = Some((l!("Співрозмовник зможе переглянути це один раз", "The recipient can view this once").into(), 0.0, false));
            return None;
        }
        if !self.view_once.opened.iter().any(|(k, _)| *k == (peer, id)) {
            self.view_once.opened.push(((peer, id), false));
            self.send(Cmd::MarkListened { peer, id });
        }
        Some(action)
    }

    /// Whether a click on message `id` opens self-destructing media.
    pub(super) fn is_view_once(&self, peer: PeerId, id: i32) -> bool {
        self.find(peer, id).is_some_and(|m| m.self_destructing())
    }

    /// Media closed after being shown expires; its pictures are dropped.
    pub(super) fn view_once_frame(&mut self) {
        let viewer = self.viewer.as_ref().map(|v| (v.peer, v.id));
        let player = self.video.showing();
        let mut gone = Vec::new();
        for (key, shown) in &mut self.view_once.opened {
            if viewer == Some(*key) || player == Some(*key) {
                *shown = true;
            } else if *shown {
                gone.push(*key);
            }
        }
        if gone.is_empty() {
            return;
        }
        self.view_once.opened.retain(|(k, _)| !gone.contains(k));
        for (peer, id) in gone {
            if let Some(m) = self.find_mut(peer, id) {
                m.expire_media();
            }
            self.spoilers.forget((peer, id));
            self.textures.remove(&ImgKey::Photo(peer, id));
            self.textures.remove(&ImgKey::Full(peer, id));
        }
    }
}
