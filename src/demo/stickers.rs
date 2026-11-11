//! Demo sticker sets, favourites and saved GIFs: installing, removing, reordering and faving
//! change this run's state, so the panel and the set box behave as with the server.

use std::cell::RefCell;

use eframe::egui::{Color32, ColorImage};

use super::{outgoing, replied};
use crate::app::App;
use crate::backend::{Cmd, DocRef, Ev, FAVED_SET, ImgKey, MediaInfo, MediaKind, Msg, RECENT_SET, StickerSetInfo, StickerSetRef};

/// (id, title, short name, stickers as emoji); a sticker's id is `set id * 100 + index`.
const SETS: [(i64, &str, &str, &str); 5] = [
    (2, "Cats", "GlassCats", "🐱😺😸😹😻😼😽🙀😿😾🐈"),
    (3, "Feelings", "GlassFaces", "😀😃😄😁😆😅🤣😂🙂🙃😉😊😇🥰😍🤩😘"),
    (4, "Treats", "GlassFood", "🍕🍔🍟🌭🥗🍣🍩🍪🍰☕"),
    (5, "Animals", "GlassAnimals", "🐶🐼🦊🐨🐯🦁🐸🐵🐧🦉"),
    (6, "Weather", "GlassWeather", "☀🌤⛅🌧⛈❄🌈🌊"),
];
/// Saved GIF ids; their previews are drawn here.
const GIFS: std::ops::RangeInclusive<i64> = 7001..=7006;

struct State {
    installed: Vec<i64>,
    faved: Vec<i64>,
    gifs: Vec<i64>,
}

thread_local! {
    static STATE: RefCell<State> = RefCell::new(State { installed: vec![2, 3], faved: vec![200, 302, 314], gifs: GIFS.collect() });
}

fn emoji_of(id: i64) -> String {
    SETS.iter().find(|s| s.0 == id / 100).and_then(|s| s.3.chars().nth((id % 100) as usize)).map(String::from).unwrap_or_default()
}

fn set(id: i64) -> Option<StickerSetInfo> {
    let (id, title, short, emoji) = *SETS.iter().find(|s| s.0 == id)?;
    let stickers: Vec<(i64, String)> = emoji.chars().enumerate().map(|(i, c)| (id * 100 + i as i64, c.to_string())).collect();
    let installed = STATE.with_borrow(|s| s.installed.contains(&id));
    Some(StickerSetInfo { id, title: title.into(), short_name: short.into(), count: stickers.len(), installed, stickers })
}

fn faved() -> Vec<(i64, String)> {
    STATE.with_borrow(|s| s.faved.iter().map(|id| (*id, emoji_of(*id))).collect())
}

/// A GIF preview: a soft diagonal gradient, different for each id.
fn gif_picture(id: i64) -> ColorImage {
    let (w, h) = (160, 120);
    let hue = (id % 6) as f32 / 6.0;
    let mut img = ColorImage::filled([w, h], Color32::BLACK);
    for y in 0..h {
        for x in 0..w {
            let t = (x + y) as f32 / (w + h) as f32;
            let c = |k: f32| (((hue + k) * std::f32::consts::TAU).sin() * 0.5 + 0.5) * 200.0 * (0.6 + 0.4 * t) + 40.0;
            img.pixels[y * w + x] = Color32::from_rgb(c(0.0) as u8, c(0.33) as u8, c(0.66) as u8);
        }
    }
    img
}

pub(super) fn handle(app: &mut App, cmd: Cmd) {
    match cmd {
        Cmd::LoadStickers => {
            let pseudo = |id, title: &str, stickers: Vec<(i64, String)>| StickerSetInfo { id, title: title.into(), short_name: String::new(), count: stickers.len(), installed: true, stickers };
            let recent = [200, 301, 207, 309, 203].map(|id| (id, emoji_of(id))).to_vec();
            let mut sets = vec![pseudo(FAVED_SET, "Favourites", faved()), pseudo(RECENT_SET, "Recent", recent)];
            sets.extend(STATE.with_borrow(|s| s.installed.clone()).into_iter().filter_map(set));
            app.push_event(Ev::Stickers(sets));
        }
        Cmd::OpenStickerSet(r) => {
            let id = match r {
                StickerSetRef::Id(id) => id,
                StickerSetRef::Name(name) => SETS.iter().find(|s| s.2.eq_ignore_ascii_case(&name)).map_or(4, |s| s.0),
                StickerSetRef::Doc(DocRef::Doc(doc)) => doc / 100,
                // The cat in the demo chat comes from the cat set.
                StickerSetRef::Doc(DocRef::Msg(..)) => 2,
            };
            match set(id) {
                Some(s) => app.push_event(Ev::StickerSet(s)),
                None => app.push_event(Ev::Error(crate::i18n::l!("Набір стікерів не знайдено", "Sticker set not found").into())),
            }
        }
        Cmd::InstallStickerSet { id, install } => {
            STATE.with_borrow_mut(|s| {
                s.installed.retain(|x| *x != id);
                if install {
                    s.installed.insert(0, id);
                }
            });
            if let Some(s) = set(id) {
                app.push_event(Ev::StickerSetChanged(s));
            }
        }
        Cmd::SearchStickerSets(query) => {
            let q = query.trim().to_lowercase();
            // Trending: the sets not installed at start; a query matches titles, names and emoji.
            let sets = SETS
                .iter()
                .filter(|s| if q.is_empty() { s.0 >= 4 } else { s.1.to_lowercase().contains(&q) || s.2.to_lowercase().contains(&q) || s.3.contains(q.as_str()) })
                .filter_map(|s| set(s.0))
                .map(|s| StickerSetInfo { stickers: s.stickers.into_iter().take(5).collect(), ..s })
                .collect();
            app.push_event(Ev::FoundStickerSets { query, sets });
        }
        Cmd::ReorderStickerSets(order) => STATE.with_borrow_mut(|s| s.installed = order),
        Cmd::FaveSticker { doc, unfave } => {
            // A sticker in a demo message is the cat.
            let id = match doc {
                DocRef::Doc(id) => id,
                DocRef::Msg(..) => 200,
            };
            STATE.with_borrow_mut(|s| {
                s.faved.retain(|x| *x != id);
                if !unfave {
                    s.faved.insert(0, id);
                }
            });
            app.push_event(Ev::FavedStickers(faved()));
        }
        Cmd::LoadSavedGifs => app.push_event(Ev::SavedGifs(STATE.with_borrow(|s| s.gifs.clone()))),
        Cmd::SaveGif { doc, unsave } => {
            let id = match doc {
                DocRef::Doc(id) => id,
                DocRef::Msg(..) => *GIFS.end(),
            };
            STATE.with_borrow_mut(|s| {
                s.gifs.retain(|x| *x != id);
                if !unsave {
                    s.gifs.insert(0, id);
                }
            });
            app.push_event(Ev::SavedGifs(STATE.with_borrow(|s| s.gifs.clone())));
            if !unsave {
                app.push_event(Ev::Notice(crate::i18n::l!("GIF збережено", "GIF saved").into()));
            }
        }
        Cmd::LoadStickerThumbs(ids) => {
            for id in ids.into_iter().filter(|id| GIFS.contains(id)) {
                app.push_event(Ev::Image { key: ImgKey::Sticker(id), image: gif_picture(id) });
            }
        }
        Cmd::SendSticker { peer, doc, reply_to, send_as } if GIFS.contains(&doc) => {
            let gif = MediaInfo { kind: MediaKind::Gif, duration: 3.0, size: (160, 120), title: String::new(), performer: String::new(), waveform: Vec::new(), bytes: 0 };
            let m = Msg { media: Some(gif), ..super::sendas::posted_as(replied(app, outgoing(peer, String::new(), None), &reply_to), send_as) };
            let id = m.id;
            app.push_event(Ev::NewMsg(m));
            app.push_event(Ev::Image { key: ImgKey::Photo(peer, id), image: gif_picture(doc) });
        }
        Cmd::SendSticker { peer, doc, reply_to, send_as } => {
            let emoji = Some(emoji_of(doc)).filter(|e| !e.is_empty()).unwrap_or_else(|| "🐱".into());
            app.push_event(Ev::NewMsg(Msg { sticker: Some((doc, emoji)), ..super::sendas::posted_as(replied(app, outgoing(peer, String::new(), None), &reply_to), send_as) }));
        }
        _ => {}
    }
}
