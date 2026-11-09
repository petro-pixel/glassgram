//! Chat backgrounds and themes: the gallery (`account.getWallPapers`), chat themes
//! (`account.getChatThemes`, gift ones from `account.getUniqueGiftChatThemes`), a chat's own wallpaper
//! and theme from its full info, and setting them (`messages.setChatWallPaper`, `messages.setChatTheme`).
//! Files go to `wallpapers/` in the data folder, out of reach of the cache limit.

use super::*;
use crate::wallpaper::Spec;
use grammers_client::session::types::PeerKind;

/// The width thumbnails are decoded at, in points.
const THUMB_PT: f32 = 160.0;

/// A wallpaper from the server.
#[derive(Clone, Debug, PartialEq, Default)]
pub struct WallPaper {
    pub id: i64,
    pub access_hash: i64,
    /// Document id of its picture or pattern; 0 — a fill without a file.
    pub doc: i64,
    pub pattern: bool,
    /// Made for dark themes.
    pub dark: bool,
    /// Fill colours, 0xRRGGBB.
    pub colors: Vec<u32>,
    pub rotation: i32,
    /// Pattern strength, −100…100 (negative: dark).
    pub intensity: i32,
    /// "tgv" for patterns, otherwise the picture's extension.
    pub ext: String,
}

impl WallPaper {
    /// Where its file is kept once downloaded.
    pub fn file(&self) -> PathBuf {
        data_dir().join("wallpapers").join(format!("w{}.{}", self.doc, self.ext))
    }

    /// What to draw; a picture or pattern needs its file at `path`, until then only its fill shows.
    pub fn spec(&self, path: Option<PathBuf>) -> Spec {
        let fill = || if self.colors.is_empty() { Spec::Live } else { Spec::Fill { colors: self.colors.clone(), rotation: self.rotation } };
        match path {
            _ if self.doc == 0 => fill(),
            Some(path) if self.pattern => Spec::Pattern { path, colors: self.colors.clone(), rotation: self.rotation, intensity: self.intensity },
            Some(path) => Spec::Image { path },
            None if self.pattern => Spec::Fill { colors: if self.colors.is_empty() { crate::wallpaper::DEFAULT_COLORS.to_vec() } else { self.colors.clone() }, rotation: self.rotation },
            None => fill(),
        }
    }

    fn settings(&self) -> tl::enums::WallPaperSettings {
        let c = |i: usize| self.colors.get(i).map(|c| *c as i32);
        tl::types::WallPaperSettings {
            blur: false,
            motion: false,
            background_color: c(0),
            second_background_color: c(1),
            third_background_color: c(2),
            fourth_background_color: c(3),
            intensity: self.pattern.then_some(self.intensity),
            rotation: (self.colors.len() == 2).then_some(self.rotation),
            emoticon: None,
        }
        .into()
    }

    fn input(&self) -> tl::enums::InputWallPaper {
        if self.doc == 0 {
            tl::types::InputWallPaperNoFile { id: self.id }.into()
        } else {
            tl::types::InputWallPaper { id: self.id, access_hash: self.access_hash }.into()
        }
    }
}

/// A chat theme: an emoji theme or a unique gift's.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub enum ThemeKey {
    Emoticon(String),
    /// A unique gift's slug.
    Gift(String),
}

#[derive(Clone, Debug, PartialEq, Default)]
pub struct ChatTheme {
    /// `None` only in a placeholder that is looked up in the list by `title`.
    pub key: Option<ThemeKey>,
    /// The emoji, or the gift's name and number.
    pub title: String,
    /// The emoji on its tile (a gift theme's comes from its wallpaper, if any).
    pub emoticon: String,
    /// Its wallpapers for light and dark themes.
    pub light: Option<WallPaper>,
    pub dark: Option<WallPaper>,
    /// Accent colours (0xRRGGBB) for light and dark.
    pub accent: (u32, u32),
}

impl ChatTheme {
    /// A theme known only by its key (from a chat's info or our own choice); the list fills the rest.
    pub fn named(key: ThemeKey) -> Self {
        let title = match &key {
            ThemeKey::Emoticon(e) => e.clone(),
            ThemeKey::Gift(slug) => slug.clone(),
        };
        let emoticon = if matches!(key, ThemeKey::Emoticon(_)) { title.clone() } else { String::new() };
        Self { key: Some(key), title, emoticon, ..Default::default() }
    }
}

fn convert_wall(w: tl::enums::WallPaper, docs: &mut HashMap<i64, tl::types::Document>) -> WallPaper {
    let (id, access_hash, doc, pattern, dark, settings) = match w {
        tl::enums::WallPaper::Paper(w) => (w.id, w.access_hash, Some(w.document), w.pattern, w.dark, w.settings),
        tl::enums::WallPaper::NoFile(w) => (w.id, 0, None, false, w.dark, w.settings),
    };
    let mut out = WallPaper { id, access_hash, pattern, dark, ..Default::default() };
    if let Some(tl::enums::WallPaperSettings::Settings(s)) = settings {
        let u = |c: i32| c as u32 & 0xff_ffff;
        out.colors = [s.background_color, s.second_background_color, s.third_background_color, s.fourth_background_color].into_iter().map_while(|c| c.map(u)).collect();
        out.rotation = s.rotation.unwrap_or(0);
        out.intensity = s.intensity.unwrap_or(if pattern { 50 } else { 0 }).clamp(-100, 100);
    }
    if let Some(tl::enums::Document::Document(d)) = doc {
        out.ext = match d.mime_type.as_str() {
            "application/x-tgwallpattern" => "tgv",
            "image/png" => "png",
            _ => "jpg",
        }
        .into();
        out.pattern |= out.ext == "tgv";
        out.doc = d.id;
        docs.insert(d.id, d);
    }
    out
}

/// A theme's wallpapers and accents from its per-base-theme settings.
fn theme_from(key: ThemeKey, title: String, emoticon: String, settings: Vec<tl::enums::ThemeSettings>, docs: &mut HashMap<i64, tl::types::Document>) -> ChatTheme {
    let mut t = ChatTheme { key: Some(key), title, emoticon, ..Default::default() };
    for tl::enums::ThemeSettings::Settings(s) in settings {
        let dark = matches!(s.base_theme, tl::enums::BaseTheme::Night | tl::enums::BaseTheme::Tinted);
        let wall = s.wallpaper.map(|w| convert_wall(w, docs));
        let accent = s.accent_color as u32 & 0xff_ffff;
        if dark {
            t.dark = t.dark.take().or(wall);
            t.accent.1 = accent;
        } else {
            t.light = t.light.take().or(wall);
            t.accent.0 = accent;
        }
    }
    t
}

fn theme_of(theme: tl::enums::ChatTheme, docs: &mut HashMap<i64, tl::types::Document>) -> Option<ChatTheme> {
    match theme {
        tl::enums::ChatTheme::Theme(t) => Some(ChatTheme::named(ThemeKey::Emoticon(t.emoticon))),
        tl::enums::ChatTheme::UniqueGift(g) => {
            let tl::enums::StarGift::Unique(u) = g.gift else { return None };
            Some(theme_from(ThemeKey::Gift(u.slug), format!("{} #{}", u.title, u.num), String::new(), g.theme_settings, docs))
        }
    }
}

impl Worker {
    pub(super) async fn wallpapers_cmd(&mut self, cmd: Cmd) -> Result<(), String> {
        let client = self.client()?;
        let failed = |e: &dyn std::fmt::Display| lf!("Фон чату: {e}", "Chat background: {e}");
        match cmd {
            Cmd::LoadWallPapers => {
                let tl::enums::account::WallPapers::Papers(r) = client.invoke(&tl::functions::account::GetWallPapers { hash: 0 }).await.map_err(|e| failed(&e))? else {
                    return Ok(());
                };
                let list: Vec<WallPaper> = r.wallpapers.into_iter().map(|w| convert_wall(w, &mut self.docs)).collect();
                for w in &list {
                    self.wall_thumb(w);
                }
                self.out.send(Ev::WallPapers(list));
            }
            Cmd::LoadChatThemes => {
                let mut themes = Vec::new();
                if let tl::enums::account::Themes::Themes(r) = client.invoke(&tl::functions::account::GetChatThemes { hash: 0 }).await.map_err(|e| failed(&e))? {
                    for tl::enums::Theme::Theme(t) in r.themes {
                        let Some(e) = t.emoticon else { continue };
                        themes.push(theme_from(ThemeKey::Emoticon(e.clone()), e.clone(), e, t.settings.unwrap_or_default(), &mut self.docs));
                    }
                }
                // Gift themes are the user's own collectibles; without any the list is just shorter.
                let gifts = client.invoke(&tl::functions::account::GetUniqueGiftChatThemes { offset: String::new(), limit: 50, hash: 0 }).await;
                if let Ok(tl::enums::account::ChatThemes::Themes(r)) = gifts {
                    themes.extend(r.themes.into_iter().filter_map(|t| theme_of(t, &mut self.docs)));
                }
                for w in themes.iter().flat_map(|t| [&t.light, &t.dark]).flatten() {
                    self.wall_thumb(w);
                }
                self.out.send(Ev::ChatThemes(themes));
            }
            Cmd::LoadChatWallPaper(peer) => {
                let r = self.peer_ref(peer)?;
                let (wall, theme) = match r.id.kind() {
                    PeerKind::User => {
                        let tl::enums::users::UserFull::Full(f) = client.invoke(&tl::functions::users::GetFullUser { id: r.into() }).await.map_err(|e| failed(&e))?;
                        let tl::enums::UserFull::Full(full) = f.full_user;
                        (full.wallpaper, full.theme.and_then(|t| theme_of(t, &mut self.docs)))
                    }
                    PeerKind::Chat => {
                        let req = tl::functions::messages::GetFullChat { chat_id: r.id.bare_id_unchecked() };
                        let tl::enums::messages::ChatFull::Full(f) = client.invoke(&req).await.map_err(|e| failed(&e))?;
                        match f.full_chat {
                            tl::enums::ChatFull::Full(c) => (None, c.theme_emoticon.map(|e| ChatTheme::named(ThemeKey::Emoticon(e)))),
                            tl::enums::ChatFull::ChannelFull(c) => (c.wallpaper, c.theme_emoticon.map(|e| ChatTheme::named(ThemeKey::Emoticon(e)))),
                            _ => (None, None),
                        }
                    }
                    PeerKind::Channel => {
                        let tl::enums::messages::ChatFull::Full(f) = client.invoke(&tl::functions::channels::GetFullChannel { channel: r.into() }).await.map_err(|e| failed(&e))?;
                        match f.full_chat {
                            tl::enums::ChatFull::ChannelFull(c) => (c.wallpaper, c.theme_emoticon.map(|e| ChatTheme::named(ThemeKey::Emoticon(e)))),
                            tl::enums::ChatFull::Full(c) => (None, c.theme_emoticon.map(|e| ChatTheme::named(ThemeKey::Emoticon(e)))),
                            _ => (None, None),
                        }
                    }
                };
                let wall = wall.map(|w| convert_wall(w, &mut self.docs));
                self.out.send(Ev::ChatWallPaper { peer, wall });
                self.out.send(Ev::ChatTheme { peer, theme });
            }
            Cmd::DownloadWallPaper(w) => {
                let Some(doc) = self.docs.get(&w.doc).cloned() else { return Err(failed(&l!("файл не знайдено", "file not found"))) };
                let (client, out) = (client.clone(), self.out.clone());
                tokio::spawn(async move {
                    let path = w.file();
                    if tokio::fs::metadata(&path).await.is_err() {
                        let mut data = Vec::new();
                        let mut it = client.iter_download(&DocLoc { doc, thumb: String::new() });
                        while let Ok(Some(chunk)) = it.next().await {
                            data.extend(chunk);
                        }
                        if data.is_empty() {
                            out.send(Ev::Error(failed(&l!("не вдалося завантажити", "download failed"))));
                            return;
                        }
                        let _ = tokio::fs::create_dir_all(path.parent().unwrap_or(&path)).await;
                        if let Err(e) = tokio::fs::write(&path, &data).await {
                            out.send(Ev::Error(failed(&e)));
                            return;
                        }
                    }
                    out.send(Ev::WallPaperFile { doc: w.doc, path });
                });
            }
            Cmd::SetChatWallPaper { peer, wall } => {
                let r = self.peer_ref(peer)?;
                let req = tl::functions::messages::SetChatWallPaper {
                    for_both: false,
                    revert: false,
                    peer: r.into(),
                    wallpaper: wall.as_ref().map(WallPaper::input),
                    settings: wall.as_ref().map(WallPaper::settings),
                    id: None,
                };
                client.invoke(&req).await.map_err(|e| failed(&e))?;
                self.out.send(Ev::ChatWallPaper { peer, wall });
            }
            Cmd::SetChatTheme { peer, theme } => {
                let r = self.peer_ref(peer)?;
                let input: tl::enums::InputChatTheme = match &theme {
                    None => tl::types::InputChatThemeEmpty {}.into(),
                    Some(ThemeKey::Emoticon(e)) => tl::types::InputChatTheme { emoticon: e.clone() }.into(),
                    Some(ThemeKey::Gift(slug)) => tl::types::InputChatThemeUniqueGift { slug: slug.clone() }.into(),
                };
                client.invoke(&tl::functions::messages::SetChatTheme { peer: r.into(), theme: input }).await.map_err(|e| failed(&e))?;
                self.out.send(Ev::ChatTheme { peer, theme: theme.map(ChatTheme::named) });
            }
            _ => {}
        }
        Ok(())
    }

    /// A gallery tile's picture: the document's thumbnail (a pattern's is its own shapes).
    fn wall_thumb(&self, w: &WallPaper) {
        let (Some(loader), Some(doc)) = (&self.loader, self.docs.get(&w.doc)) else { return };
        let sizes: Vec<(i32, String)> = doc.thumbs.iter().flatten().filter_map(|t| match t {
            tl::enums::PhotoSize::Size(s) => Some((s.w, s.r#type.clone())),
            tl::enums::PhotoSize::Progressive(s) => Some((s.w, s.r#type.clone())),
            _ => None,
        }).collect();
        let pick = sizes.iter().filter(|s| s.0 <= 420).max_by_key(|s| s.0).or_else(|| sizes.iter().min_by_key(|s| s.0));
        let Some((_, thumb)) = pick else { return };
        let (file, loc) = (format!("wt{}.img", w.doc), DocLoc { doc: doc.clone(), thumb: thumb.clone() });
        if !w.pattern {
            return loader.fetch(ImgKey::Wall(w.doc), file, THUMB_PT, loc);
        }
        // A pattern's thumbnail is a mask: made white, so the tile can tint it with the fill.
        let (client, out, key) = (loader.client.clone(), self.out.clone(), ImgKey::Wall(w.doc));
        tokio::spawn(async move {
            let path = data_dir().join("cache").join(file);
            let bytes = match tokio::fs::read(&path).await {
                Ok(b) => b,
                Err(_) => {
                    let mut data = Vec::new();
                    let mut it = client.iter_download(&loc);
                    while let Ok(Some(chunk)) = it.next().await {
                        data.extend(chunk);
                    }
                    let _ = tokio::fs::create_dir_all(path.parent().unwrap_or(&path)).await;
                    let _ = tokio::fs::write(&path, &data).await;
                    data
                }
            };
            let px = (THUMB_PT * out.ctx.pixels_per_point()).ceil() as u32;
            if let Ok(Some(image)) = tokio::task::spawn_blocking(move || decode(&bytes, px).map(white_mask)).await {
                out.send(Ev::Image { key, image });
            }
        });
    }
}

/// A pattern picture as white with its coverage in alpha: dark or opaque-on-transparent alike.
pub fn white_mask(mut image: ColorImage) -> ColorImage {
    for p in image.pixels.iter_mut() {
        let [r, g, b, a] = p.to_srgba_unmultiplied();
        let lum = (r as u32 * 3 + g as u32 * 6 + b as u32) / 10;
        let alpha = (a as u32 * (255 - lum.min(255)) / 255) as u8;
        *p = eframe::egui::Color32::from_rgba_unmultiplied(255, 255, 255, alpha);
    }
    image
}
