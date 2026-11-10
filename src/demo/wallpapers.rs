//! Demo backgrounds and chat themes: a gallery of fills, made-up patterns and one generated
//! "photo"; Олена's chat has its own wallpaper, Андрій's an emoji theme, Марія's a gift theme.
//! Setting a chat's wallpaper or theme changes this run's state.

use std::cell::RefCell;
use std::collections::HashMap;

use eframe::egui::ColorImage;

use crate::app::App;
use crate::backend::{ChatTheme, Cmd, Ev, ImgKey, ThemeKey, WallPaper, white_mask};
use crate::wallpaper;

/// Document ids of the made-up files: patterns, then the photo.
const PATTERN: i64 = 901;
const PHOTO: i64 = 905;

fn fill(id: i64, colors: &[u32], rotation: i32) -> WallPaper {
    WallPaper { id, colors: colors.to_vec(), rotation, ..Default::default() }
}

fn pattern(id: i64, colors: &[u32], intensity: i32) -> WallPaper {
    WallPaper { id, doc: PATTERN, pattern: true, dark: intensity < 0, colors: colors.to_vec(), intensity, ext: "svg".into(), ..Default::default() }
}

fn gallery() -> Vec<WallPaper> {
    vec![
        pattern(11, &wallpaper::DEFAULT_COLORS, 50),
        pattern(12, &[0x8dc0eb, 0xb9d1ea, 0xc6b1ef, 0xebd7ef], 45),
        pattern(13, &[0xfec496, 0xdd6cb9, 0x962fbf, 0x4f5bd5], -55),
        WallPaper { id: 14, doc: PHOTO, ext: "png".into(), ..Default::default() },
        fill(15, &[0xdbddbb, 0x6ba587, 0xd5d88d, 0x88b884], 0),
        fill(16, &[0xfec496, 0xdd6cb9], 45),
        fill(17, &[0x7ec4cf], 0),
        fill(18, &[0x1b2836, 0x2c3e50, 0x16222d], 0),
        fill(19, &[0xe4d4ff, 0xb5d5ff], 135),
    ]
}

fn themes() -> Vec<ChatTheme> {
    let emoji = [
        ("🏠", [0xf5e6c8, 0xe2c590], [0x2b2a3a, 0x3a2e1f], 0xe88a3c),
        ("🐥", [0xfff3b0, 0xffd76a], [0x2e2a14, 0x46391a], 0xf2b400),
        ("⛄", [0xd7ecff, 0xa8d1ff], [0x14223a, 0x1f3556], 0x3c8de8),
        ("💎", [0xd8f5f0, 0x9fe3d8], [0x0f2c2a, 0x194540], 0x16a89a),
        ("🌸", [0xffe0ef, 0xf7b2d4], [0x3a1a2c, 0x522540], 0xe0569b),
        ("🌌", [0xd9d2ff, 0xa79bff], [0x17123a, 0x2a1f66], 0x7a5cff),
    ];
    let mut list: Vec<ChatTheme> = emoji
        .iter()
        .enumerate()
        .map(|(i, (e, light, dark, accent))| ChatTheme {
            key: Some(ThemeKey::Emoticon((*e).into())),
            title: (*e).into(),
            emoticon: (*e).into(),
            light: Some(fill(100 + i as i64, light, 30)),
            dark: Some(fill(200 + i as i64, dark, 30)),
            accent: (*accent, *accent),
        })
        .collect();
    list.push(ChatTheme {
        key: Some(ThemeKey::Gift("PlushPepe-1234".into())),
        title: "Plush Pepe #1234".into(),
        light: Some(pattern(301, &[0xc8e6a0, 0x7fbf5f, 0xa6d785, 0x5e9e48], 40)),
        dark: Some(pattern(302, &[0x7fbf5f, 0x3f6f2f, 0x5e9e48, 0x2a4a20], 50)),
        accent: (0x4f9a3a, 0x7fbf5f),
        ..Default::default()
    });
    list
}

thread_local! {
    /// Each chat's wallpaper and theme in this run.
    static LOOKS: RefCell<HashMap<i64, (Option<WallPaper>, Option<ChatTheme>)>> = RefCell::new(HashMap::from([
        (5, (None, Some(ChatTheme::named(ThemeKey::Emoticon("🌸".into()))))),
        (8, (None, themes().pop())),
    ]));
}

/// A soft made-up landscape: sky, sun and hills.
fn photo(w: u32, h: u32) -> ColorImage {
    let mut px = Vec::with_capacity((w * h * 4) as usize);
    for y in 0..h {
        for x in 0..w {
            let (u, v) = (x as f32 / w as f32, y as f32 / h as f32);
            let sky = [255.0 - 90.0 * v, 170.0 + 40.0 * v, 140.0 + 100.0 * v];
            let sun = (1.0 - (((u - 0.62).powi(2) + (v - 0.42).powi(2)).sqrt() / 0.12)).clamp(0.0, 1.0);
            let hill = |base: f32, amp: f32, k: f32| v > base + amp * (u * k).sin();
            let c = if hill(0.78, 0.04, 9.0) {
                [40.0, 70.0 + 30.0 * v, 90.0]
            } else if hill(0.66, 0.05, 5.0) {
                [90.0, 110.0, 150.0]
            } else {
                [sky[0] + (255.0 - sky[0]) * sun, sky[1] + (230.0 - sky[1]) * sun, sky[2] * (1.0 - sun) + 160.0 * sun]
            };
            px.extend([c[0] as u8, c[1] as u8, c[2] as u8, 255]);
        }
    }
    ColorImage::from_rgba_unmultiplied([w as usize, h as usize], &px)
}

fn pattern_thumb() -> Option<ColorImage> {
    let (mask, w) = wallpaper::pattern_mask(&wallpaper::demo_pattern(), 256)?;
    let rgba: Vec<u8> = mask.iter().flat_map(|a| [0, 0, 0, *a]).collect();
    Some(white_mask(ColorImage::from_rgba_unmultiplied([w as usize, 256], &rgba)))
}

pub fn handle(app: &mut App, cmd: Cmd) {
    match cmd {
        Cmd::LoadWallPapers => {
            if let Some(image) = pattern_thumb() {
                app.push_event(Ev::Image { key: ImgKey::Wall(PATTERN), image });
            }
            app.push_event(Ev::Image { key: ImgKey::Wall(PHOTO), image: photo(120, 200) });
            app.push_event(Ev::WallPapers(gallery()));
        }
        Cmd::LoadChatThemes => app.push_event(Ev::ChatThemes(themes())),
        Cmd::LoadChatWallPaper(peer) => {
            let (wall, theme) = LOOKS.with_borrow(|l| l.get(&peer.bare_id_unchecked()).cloned()).unwrap_or_default();
            app.push_event(Ev::ChatWallPaper { peer, wall });
            app.push_event(Ev::ChatTheme { peer, theme });
        }
        Cmd::DownloadWallPaper(w) => {
            let path = crate::platform::temp_file(&format!("w{}.{}", w.doc, w.ext));
            let written = if w.doc == PHOTO {
                let img = photo(720, 1280);
                image::RgbaImage::from_raw(720, 1280, img.as_raw().to_vec()).is_some_and(|i| i.save(&path).is_ok())
            } else {
                std::fs::write(&path, wallpaper::demo_pattern()).is_ok()
            };
            match written {
                true => app.push_event(Ev::WallPaperFile { doc: w.doc, path }),
                false => app.push_event(Ev::Error("Не вдалося записати демо-фон".into())),
            }
        }
        Cmd::SetChatWallPaper { peer, wall } => {
            LOOKS.with_borrow_mut(|l| l.entry(peer.bare_id_unchecked()).or_default().0 = wall.clone());
            app.push_event(Ev::ChatWallPaper { peer, wall });
        }
        Cmd::SetChatTheme { peer, theme } => {
            let theme = theme.map(|k| themes().into_iter().find(|t| t.key.as_ref() == Some(&k)).unwrap_or_else(|| ChatTheme::named(k)));
            LOOKS.with_borrow_mut(|l| l.entry(peer.bare_id_unchecked()).or_default().1 = theme.clone());
            app.push_event(Ev::ChatTheme { peer, theme });
        }
        _ => {}
    }
}
