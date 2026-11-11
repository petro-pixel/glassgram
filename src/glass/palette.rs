//! Telegram's peer colours, kept in one place. Name colours 0–6 are built into every client;
//! the server (`help.getPeerColors`) adds two- and three-tone ones from 7 up, and
//! `help.getPeerProfileColors` the profile covers. Until those arrive, an unknown index falls
//! back to `index % 7`.

use std::sync::RwLock;

use eframe::egui::{Color32, Painter, Rect, Shape, pos2};

use super::is_light;

/// (day, night) name tones of the built-in colours as in Telegram Desktop: red, orange, violet,
/// green, cyan, blue, pink.
const NAMES: [(u32, u32); 7] = [
    (0xc03d33, 0xff8e86),
    (0xce671b, 0xffab61),
    (0x8544d6, 0xbe9cff),
    (0x4fad2d, 0x7ed95f),
    (0x2996ad, 0x5fd2ea),
    (0x168acd, 0x6cb9ff),
    (0xcd4073, 0xff86b4),
];

/// Profile cover tones before the server's list comes (the same eight hues, a gray last).
const PROFILES: [u32; 8] = [0xba5650, 0xc27c3e, 0x956ac8, 0x49a355, 0x3e97ad, 0x5a8fbb, 0xb85378, 0x7f8b95];

/// (index, day tones, night tones).
type Option3 = (i32, Vec<u32>, Vec<u32>);

struct Server {
    names: Vec<Option3>,
    profiles: Vec<Option3>,
}

static SERVER: RwLock<Server> = RwLock::new(Server { names: Vec::new(), profiles: Vec::new() });

fn rgb(c: u32) -> Color32 {
    Color32::from_rgb((c >> 16) as u8, (c >> 8) as u8, c as u8)
}

fn tones(list: &[Option3], idx: i32) -> Option<Vec<Color32>> {
    let (_, day, night) = list.iter().find(|o| o.0 == idx)?;
    let t = if is_light() { day } else { night };
    (!t.is_empty()).then(|| t.iter().copied().map(rgb).collect())
}

/// Keeps the server's colours; an empty list keeps what was there.
pub fn set_peer_colors(names: Vec<Option3>, profiles: Vec<Option3>) {
    if let Ok(mut s) = SERVER.write() {
        if !names.is_empty() {
            s.names = names;
        }
        if !profiles.is_empty() {
            s.profiles = profiles;
        }
    }
}

/// The stripe of a name colour: one tone, or two or three for the server's extra colours.
pub fn peer_stripe(idx: i32) -> Vec<Color32> {
    if !(0..7).contains(&idx)
        && let Some(t) = SERVER.read().ok().and_then(|s| tones(&s.names, idx))
    {
        return t;
    }
    let (day, night) = NAMES[idx.rem_euclid(7) as usize];
    vec![rgb(if is_light() { day } else { night })]
}

/// The colour an author's name is written in.
pub fn peer_color(idx: i32) -> Color32 {
    peer_stripe(idx).first().copied().unwrap_or(Color32::GRAY)
}

/// Name colours to pick from: the built-in ones and then the server's.
pub fn name_color_ids() -> Vec<i32> {
    let mut ids: Vec<i32> = (0..7).collect();
    if let Ok(s) = SERVER.read() {
        ids.extend(s.names.iter().filter(|o| o.0 >= 7 && !o.1.is_empty()).map(|o| o.0));
    }
    ids
}

/// Profile colours to pick from.
pub fn profile_color_ids() -> Vec<i32> {
    match SERVER.read() {
        Ok(s) if !s.profiles.is_empty() => s.profiles.iter().filter(|o| !o.1.is_empty()).map(|o| o.0).collect(),
        _ => (0..PROFILES.len() as i32).collect(),
    }
}

/// The cover of a profile colour: one tone or a top-to-bottom pair.
pub fn profile_tones(idx: i32) -> Vec<Color32> {
    SERVER.read().ok().and_then(|s| tones(&s.profiles, idx)).unwrap_or_else(|| vec![rgb(PROFILES[idx.rem_euclid(PROFILES.len() as i32) as usize])])
}

/// A reply or quote stripe: several tones go in dashes down it, like Telegram's.
pub fn stripe(p: &Painter, rect: Rect, colors: &[Color32]) {
    let Some(first) = colors.first() else { return };
    p.rect_filled(rect, rect.width() / 2.0, *first);
    if colors.len() < 2 {
        return;
    }
    const DASH: f32 = 6.0;
    let mut y = rect.top() + DASH;
    let mut k = 1;
    while y < rect.bottom() {
        let r = Rect::from_min_max(pos2(rect.left(), y), pos2(rect.right(), (y + DASH).min(rect.bottom())));
        p.rect_filled(r, 0.0, colors[k % colors.len()]);
        // The first tone keeps every other dash.
        y += DASH * 2.0;
        k = if k + 1 >= colors.len() { 1 } else { k + 1 };
    }
}

/// A rounded rectangle shading from `top` to `bottom`, for profile covers.
pub fn gradient_rect(p: &Painter, rect: Rect, radius: f32, top: Color32, bottom: Color32) {
    let ppp = p.pixels_per_point();
    let pts = super::rounded_points(rect, radius, ppp);
    p.add(Shape::mesh(super::fan(&pts, rect.center(), ppp, |q| super::mix(top, bottom, (q.y - rect.top()) / rect.height()))));
}
