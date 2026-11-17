//! Demo own sticker sets: creating one "uploads" nothing, the picked pictures become the
//! stickers' previews, and every edit changes this run's state as the server would.

use std::cell::RefCell;

use eframe::egui::ColorImage;

use crate::app::App;
use crate::backend::{Cmd, Ev, ImgKey, NewSticker, OwnEv, OwnOp, OwnSet, StickerSetInfo, short_name_ok};
use crate::i18n::l;

/// Link names of the demo's other sets, so checking one of them says "taken".
const TAKEN: [&str; 5] = ["GlassCats", "GlassFaces", "GlassFood", "GlassAnimals", "GlassWeather"];

struct State {
    sets: Vec<OwnSet>,
    next_set: i64,
    next_doc: i64,
}

thread_local! {
    static STATE: RefCell<State> = RefCell::new(State { sets: vec![doodles()], next_set: 61, next_doc: 6100 });
}

/// The set the demo account already made.
fn doodles() -> OwnSet {
    let stickers: Vec<(i64, String)> = "✏🎨🖌🖍📐".chars().enumerate().map(|(i, c)| (6000 + i as i64, c.to_string())).collect();
    let set = StickerSetInfo { id: 60, title: "Glass Doodles".into(), short_name: "GlassDoodles".into(), count: stickers.len(), installed: false, stickers };
    OwnSet { set, emojis: false }
}

/// One of our sets for the demo's set box.
pub(super) fn panel_set(id: i64) -> Option<StickerSetInfo> {
    STATE.with_borrow(|s| s.sets.iter().find(|x| x.set.id == id).map(|x| x.set.clone()))
}

/// Our installed sticker sets, for the panel.
pub(super) fn installed() -> Vec<StickerSetInfo> {
    STATE.with_borrow(|s| s.sets.iter().filter(|x| x.set.installed && !x.emojis).map(|x| x.set.clone()).collect())
}

/// A sticker "made" from a file: its picture becomes the preview, sized as the real conversion would.
fn make(app: &mut App, s: &NewSticker, emojis: bool) -> (i64, String) {
    let doc = STATE.with_borrow_mut(|st| {
        st.next_doc += 1;
        st.next_doc
    });
    let side = if emojis { 100 } else { 256 };
    if let Some(img) = image::ImageReader::open(&s.path).ok().and_then(|r| r.with_guessed_format().ok()).and_then(|r| r.decode().ok()) {
        let img = img.resize(side, side, image::imageops::FilterType::Triangle).to_rgba8();
        let image = ColorImage::from_rgba_unmultiplied([img.width() as usize, img.height() as usize], img.as_raw());
        app.push_event(Ev::Image { key: ImgKey::Sticker(doc), image });
    }
    (doc, s.emoji.clone())
}

/// Applies `f` to set `id` and reports it changed.
fn change(app: &mut App, id: i64, created: bool, f: impl FnOnce(&mut OwnSet)) {
    let set = STATE.with_borrow_mut(|s| {
        let set = s.sets.iter_mut().find(|x| x.set.id == id)?;
        f(set);
        set.set.count = set.set.stickers.len();
        Some(set.clone())
    });
    match set {
        Some(set) => app.push_event(Ev::OwnStickers(OwnEv::Set { set, created })),
        None => app.push_event(Ev::Error(l!("Набір стікерів не знайдено", "Sticker set not found").into())),
    }
}

fn taken(name: &str) -> bool {
    TAKEN.iter().any(|t| t.eq_ignore_ascii_case(name)) || STATE.with_borrow(|s| s.sets.iter().any(|x| x.set.short_name.eq_ignore_ascii_case(name)))
}

pub(super) fn handle(app: &mut App, cmd: Cmd) {
    let Cmd::OwnStickers(op) = cmd else { return };
    let ev = |app: &mut App, e| app.push_event(Ev::OwnStickers(e));
    match op {
        OwnOp::Load => {
            let sets = STATE.with_borrow(|s| s.sets.clone());
            ev(app, OwnEv::Sets(sets));
        }
        OwnOp::Open(id) => change(app, id, false, |_| {}),
        OwnOp::CheckName(name) => {
            let problem = if !short_name_ok(&name) {
                Some(l!("Лише латинські літери, цифри й підкреслення, першою — літера", "Only Latin letters, digits and underscores, starting with a letter").to_owned())
            } else if taken(&name) {
                Some(l!("Це посилання вже зайняте", "This link is already taken").to_owned())
            } else {
                None
            };
            ev(app, OwnEv::NameChecked { name, problem });
        }
        OwnOp::SuggestName(title) => {
            let mut base: String = title.split_whitespace().map(|w| w.chars().filter(char::is_ascii_alphanumeric).collect::<String>()).collect();
            if !base.starts_with(|c: char| c.is_ascii_alphabetic()) {
                base.insert_str(0, "Set");
            }
            base.truncate(40);
            let name = (1..).map(|k| if k == 1 { format!("{base}_by_glass") } else { format!("{base}{k}_by_glass") }).find(|n| !taken(n)).unwrap_or_default();
            ev(app, OwnEv::NameSuggested { title, name });
        }
        OwnOp::Create { title, short_name, emojis, stickers } => {
            if taken(&short_name) {
                return ev(app, OwnEv::Failed(l!("Це посилання вже зайняте", "This link is already taken").into()));
            }
            let total = stickers.len();
            let made: Vec<(i64, String)> = stickers.iter().enumerate().map(|(i, s)| {
                app.push_event(Ev::OwnStickers(OwnEv::Progress { done: i, total }));
                make(app, s, emojis)
            }).collect();
            let id = STATE.with_borrow_mut(|s| {
                s.next_set += 1;
                let set = StickerSetInfo { id: s.next_set, title, short_name, count: made.len(), installed: true, stickers: made };
                s.sets.insert(0, OwnSet { set, emojis });
                s.next_set
            });
            change(app, id, true, |_| {});
        }
        OwnOp::Add { set, emojis, sticker } => {
            let made = make(app, &sticker, emojis);
            change(app, set, false, |s| s.set.stickers.push(made));
        }
        OwnOp::AddFromMsg { set, emoji, .. } => {
            // A demo message has no file to convert: the sticker keeps only its emoji.
            let doc = STATE.with_borrow_mut(|st| {
                st.next_doc += 1;
                st.next_doc
            });
            change(app, set, false, |s| s.set.stickers.push((doc, emoji)));
        }
        OwnOp::Remove { set, doc } => change(app, set, false, |s| s.set.stickers.retain(|x| x.0 != doc)),
        OwnOp::ChangeEmoji { set, doc, emoji } => change(app, set, false, |s| {
            for x in s.set.stickers.iter_mut().filter(|x| x.0 == doc) {
                x.1 = emoji.clone();
            }
        }),
        OwnOp::Move { set, doc, position } => change(app, set, false, |s| {
            if let Some(from) = s.set.stickers.iter().position(|x| x.0 == doc) {
                let item = s.set.stickers.remove(from);
                let to = (position.max(0) as usize).min(s.set.stickers.len());
                s.set.stickers.insert(to, item);
            }
        }),
        OwnOp::Rename { set, title } => change(app, set, false, |s| s.set.title = title),
        OwnOp::Delete(id) => {
            STATE.with_borrow_mut(|s| s.sets.retain(|x| x.set.id != id));
            ev(app, OwnEv::Deleted(id));
        }
        OwnOp::Done { .. } => {}
    }
}
