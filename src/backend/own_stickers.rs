//! Our own sticker and custom emoji sets, as in Telegram Desktop's "Create a new sticker set":
//! a title and a link name checked while typed, stickers made from pictures and videos (converted
//! to Telegram's formats here), and managing the sets we made (`messages.getMyStickers`, `stickers.*`).

use std::path::Path;

use super::*;
use super::stickers::input_doc;

/// Stickers a set holds at most; custom emoji sets take more.
pub const OWN_STICKERS_MAX: usize = 120;
pub const OWN_EMOJI_MAX: usize = 200;
/// Files offered in the picker: pictures become WEBP, videos and GIFs become WEBM.
pub const STICKER_FILE_EXTS: [&str; 9] = ["png", "jpg", "jpeg", "webp", "webm", "mp4", "mov", "gif", "mkv"];
/// Telegram's limits for a static and a video sticker file.
const STATIC_MAX_BYTES: usize = 512 * 1024;
const VIDEO_MAX_BYTES: u64 = 256 * 1024;
/// A video sticker plays for at most 3 s.
const VIDEO_MAX_SECS: f64 = 3.0;
const MY_SETS_MAX: i32 = 100;

/// A sticker to add: a picture or video file and its emoji.
#[derive(Debug, Clone)]
pub struct NewSticker {
    pub path: PathBuf,
    pub emoji: String,
}

/// A set we made; `emojis` for a custom emoji set.
#[derive(Debug, Clone)]
pub struct OwnSet {
    pub set: StickerSetInfo,
    pub emojis: bool,
}

/// What `Cmd::OwnStickers` asks for.
#[derive(Debug)]
pub enum OwnOp {
    /// The sets we made, with a few stickers each.
    Load,
    /// One of our sets in full, for editing.
    Open(i64),
    /// Whether a link name is free.
    CheckName(String),
    /// A free link name made from a title.
    SuggestName(String),
    Create { title: String, short_name: String, emojis: bool, stickers: Vec<NewSticker> },
    /// A sticker from a file.
    Add { set: i64, emojis: bool, sticker: NewSticker },
    /// A sticker or photo from a message, downloaded and converted like a file.
    AddFromMsg { set: i64, emojis: bool, peer: PeerId, msg: i32, emoji: String },
    Remove { set: i64, doc: i64 },
    ChangeEmoji { set: i64, doc: i64, emoji: String },
    /// Moves a sticker to `position` (from 0).
    Move { set: i64, doc: i64, position: i32 },
    Rename { set: i64, title: String },
    Delete(i64),
    /// A set the server returned after a background upload: remembered for previews and edits.
    Done { res: Box<tl::enums::messages::StickerSet>, created: bool },
}

/// Answers to `OwnOp`s.
pub enum OwnEv {
    Sets(Vec<OwnSet>),
    /// A set was made (`created`), changed or opened.
    Set { set: OwnSet, created: bool },
    Deleted(i64),
    /// `None`: the name is free; otherwise why it can't be used.
    NameChecked { name: String, problem: Option<String> },
    NameSuggested { title: String, name: String },
    /// Stickers converted and uploaded so far.
    Progress { done: usize, total: usize },
    /// A create or add failed; the box shows why and can try again.
    Failed(String),
}

/// A link name Telegram takes: a letter first, then Latin letters, digits and single underscores.
pub fn short_name_ok(name: &str) -> bool {
    let b = name.as_bytes();
    !b.is_empty() && b.len() <= 64 && b[0].is_ascii_alphabetic() && b.iter().all(|c| c.is_ascii_alphanumeric() || *c == b'_') && !name.contains("__")
}

/// Readable text for the errors these requests answer with.
fn rpc_text(e: grammers_client::InvocationError) -> String {
    let name = match &e {
        grammers_client::InvocationError::Rpc(r) => r.name.as_str(),
        _ => "",
    };
    match name {
        "SHORT_NAME_OCCUPIED" => l!("Це посилання вже зайняте", "This link is already taken").into(),
        "SHORT_NAME_INVALID" => l!("Неправильне посилання", "Invalid link").into(),
        "STICKERS_TOO_MUCH" => l!("У наборі вже забагато стікерів", "The set already has too many stickers").into(),
        "STICKER_EMOJI_INVALID" => l!("Виберіть емодзі для кожного стікера", "Choose an emoji for each sticker").into(),
        "STICKERSET_INVALID" => l!("Набір стікерів не знайдено", "Sticker set not found").into(),
        "STICKER_PNG_DIMENSIONS" | "STICKER_FILE_INVALID" | "STICKER_VIDEO_LONG" | "STICKER_VIDEO_BIG" => l!("Telegram не прийняв цей файл як стікер", "Telegram did not accept this file as a sticker").into(),
        _ => lf!("Стікери: {e}", "Stickers: {e}"),
    }
}

impl Worker {
    pub(super) async fn own_stickers_cmd(&mut self, cmd: Cmd) -> Result<(), String> {
        let Cmd::OwnStickers(op) = cmd else { return Ok(()) };
        let client = self.client()?;
        let ev = |e| Ev::OwnStickers(e);
        match op {
            OwnOp::Load => {
                let res = client.invoke(&tl::functions::messages::GetMyStickers { offset_id: 0, limit: MY_SETS_MAX }).await.map_err(rpc_text)?;
                let tl::enums::messages::MyStickers::Stickers(my) = res;
                let sets = my.sets.into_iter().map(|c| self.own_covered(c)).collect();
                self.out.send(ev(OwnEv::Sets(sets)));
            }
            OwnOp::Open(id) => {
                let input = self.set_input(id)?;
                let res = client.invoke(&tl::functions::messages::GetStickerSet { stickerset: input, hash: 0 }).await.map_err(rpc_text)?;
                let set = self.own_set(res)?;
                self.out.send(ev(OwnEv::Set { set, created: false }));
            }
            OwnOp::CheckName(name) => {
                let problem = if !short_name_ok(&name) {
                    Some(l!("Лише латинські літери, цифри й підкреслення, першою — літера", "Only Latin letters, digits and underscores, starting with a letter").to_owned())
                } else {
                    match client.invoke(&tl::functions::stickers::CheckShortName { short_name: name.clone() }).await {
                        Ok(true) => None,
                        Ok(false) => Some(l!("Це посилання вже зайняте", "This link is already taken").to_owned()),
                        Err(e) => Some(rpc_text(e)),
                    }
                };
                self.out.send(ev(OwnEv::NameChecked { name, problem }));
            }
            OwnOp::SuggestName(title) => {
                let res = client.invoke(&tl::functions::stickers::SuggestShortName { title: title.clone() }).await.map_err(rpc_text)?;
                let tl::enums::stickers::SuggestedShortName::Name(s) = res;
                self.out.send(ev(OwnEv::NameSuggested { title, name: s.short_name }));
            }
            OwnOp::Create { title, short_name, emojis, stickers } => {
                let (out, back) = (self.out.clone(), self.back.clone());
                tokio::spawn(async move {
                    let res = async {
                        let total = stickers.len();
                        let mut items = Vec::with_capacity(total);
                        for (done, s) in stickers.iter().enumerate() {
                            out.send(ev(OwnEv::Progress { done, total }));
                            let document = prepare(&client, s.path.clone(), emojis, &s.emoji).await?;
                            items.push(tl::types::InputStickerSetItem { document, emoji: s.emoji.clone(), mask_coords: None, keywords: None }.into());
                        }
                        out.send(ev(OwnEv::Progress { done: total, total }));
                        let req = tl::functions::stickers::CreateStickerSet {
                            masks: false,
                            emojis,
                            text_color: false,
                            user_id: tl::enums::InputUser::UserSelf,
                            title,
                            short_name,
                            thumb: None,
                            stickers: items,
                            software: Some("Glassgram".into()),
                        };
                        client.invoke(&req).await.map_err(rpc_text)
                    }
                    .await;
                    match res {
                        Ok(res) => drop(back.send(Cmd::OwnStickers(OwnOp::Done { res: Box::new(res), created: true }))),
                        Err(e) => out.send(ev(OwnEv::Failed(e))),
                    }
                });
            }
            OwnOp::Add { set, emojis, sticker } => {
                let input = self.set_input(set)?;
                self.add_in_background(client, input, emojis, sticker.emoji, Source::File(sticker.path));
            }
            OwnOp::AddFromMsg { set, emojis, peer, msg, emoji } => {
                let input = self.set_input(set)?;
                let r = self.peer_ref(peer)?;
                self.add_in_background(client, input, emojis, emoji, Source::Msg(r, msg));
            }
            OwnOp::Remove { set: _, doc } => {
                let res = client.invoke(&tl::functions::stickers::RemoveStickerFromSet { sticker: self.sticker_input(doc)? }).await.map_err(rpc_text)?;
                self.emit_own(res)?;
            }
            OwnOp::ChangeEmoji { set: _, doc, emoji } => {
                let req = tl::functions::stickers::ChangeSticker { sticker: self.sticker_input(doc)?, emoji: Some(emoji), mask_coords: None, keywords: None };
                let res = client.invoke(&req).await.map_err(rpc_text)?;
                self.emit_own(res)?;
            }
            OwnOp::Move { set: _, doc, position } => {
                let res = client.invoke(&tl::functions::stickers::ChangeStickerPosition { sticker: self.sticker_input(doc)?, position }).await.map_err(rpc_text)?;
                self.emit_own(res)?;
            }
            OwnOp::Rename { set, title } => {
                let res = client.invoke(&tl::functions::stickers::RenameStickerSet { stickerset: self.set_input(set)?, title }).await.map_err(rpc_text)?;
                self.emit_own(res)?;
            }
            OwnOp::Delete(set) => {
                client.invoke(&tl::functions::stickers::DeleteStickerSet { stickerset: self.set_input(set)? }).await.map_err(rpc_text)?;
                self.out.send(ev(OwnEv::Deleted(set)));
            }
            OwnOp::Done { res, created } => {
                let set = self.own_set(*res)?;
                self.out.send(ev(OwnEv::Set { set, created }));
            }
        }
        Ok(())
    }

    /// Converts, uploads and adds one sticker on a task of its own.
    fn add_in_background(&self, client: Client, input: tl::enums::InputStickerSet, emojis: bool, emoji: String, source: Source) {
        let (out, back) = (self.out.clone(), self.back.clone());
        tokio::spawn(async move {
            let res = async {
                out.send(Ev::OwnStickers(OwnEv::Progress { done: 0, total: 1 }));
                let path = match source {
                    Source::File(p) => p,
                    Source::Msg(r, id) => cache_media(&client, r, id).await?,
                };
                let document = prepare(&client, path, emojis, &emoji).await?;
                let sticker = tl::types::InputStickerSetItem { document, emoji, mask_coords: None, keywords: None }.into();
                client.invoke(&tl::functions::stickers::AddStickerToSet { stickerset: input, sticker }).await.map_err(rpc_text)
            }
            .await;
            match res {
                Ok(res) => drop(back.send(Cmd::OwnStickers(OwnOp::Done { res: Box::new(res), created: false }))),
                Err(e) => out.send(Ev::OwnStickers(OwnEv::Failed(e))),
            }
        });
    }

    fn sticker_input(&self, doc: i64) -> Result<tl::enums::InputDocument, String> {
        self.docs.get(&doc).map(input_doc).ok_or_else(|| l!("Стікер не знайдено", "Sticker not found").to_owned())
    }

    fn emit_own(&mut self, res: tl::enums::messages::StickerSet) -> Result<(), String> {
        let set = self.own_set(res)?;
        self.out.send(Ev::OwnStickers(OwnEv::Set { set, created: false }));
        Ok(())
    }

    fn own_set(&mut self, res: tl::enums::messages::StickerSet) -> Result<OwnSet, String> {
        let tl::enums::messages::StickerSet::Set(full) = res else { return Err(l!("Набір стікерів не знайдено", "Sticker set not found").into()) };
        let tl::enums::StickerSet::Set(set) = full.set;
        let emojis = set.emojis;
        let stickers = self.remember_docs(full.documents);
        Ok(OwnSet { set: self.set_info(set, stickers), emojis })
    }

    fn own_covered(&mut self, c: tl::enums::StickerSetCovered) -> OwnSet {
        let (set, docs) = match c {
            tl::enums::StickerSetCovered::Covered(c) => (c.set, vec![c.cover]),
            tl::enums::StickerSetCovered::StickerSetMultiCovered(c) => (c.set, c.covers),
            tl::enums::StickerSetCovered::StickerSetFullCovered(c) => (c.set, c.documents),
            tl::enums::StickerSetCovered::StickerSetNoCovered(c) => (c.set, Vec::new()),
        };
        let tl::enums::StickerSet::Set(set) = set;
        let emojis = set.emojis;
        let stickers = self.remember_docs(docs);
        OwnSet { set: self.set_info(set, stickers), emojis }
    }
}

/// Where an added sticker comes from.
enum Source {
    File(PathBuf),
    Msg(PeerRef, i32),
}

/// A file made ready for Telegram, in a temporary folder.
struct Converted {
    path: PathBuf,
    video: bool,
    w: i32,
    h: i32,
    duration: f64,
}

/// Converts a file on a blocking thread, uploads it as a sticker document and deletes the copy.
async fn prepare(client: &Client, path: PathBuf, emojis: bool, emoji: &str) -> Result<tl::enums::InputDocument, String> {
    let name = path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
    let c = tokio::task::spawn_blocking(move || convert(&path, emojis)).await.map_err(|e| e.to_string())?.map_err(|e| format!("{name}: {e}"))?;
    let res = upload_sticker(client, &c, emoji).await;
    let _ = std::fs::remove_file(&c.path);
    res
}

async fn upload_sticker(client: &Client, c: &Converted, emoji: &str) -> Result<tl::enums::InputDocument, String> {
    let up = client.upload_file(&c.path).await.map_err(|e| e.to_string())?;
    let sticker = tl::types::DocumentAttributeSticker { mask: false, alt: emoji.to_owned(), stickerset: tl::enums::InputStickerSet::Empty, mask_coords: None }.into();
    let (name, mime, size) = if c.video {
        let video = tl::types::DocumentAttributeVideo {
            round_message: false,
            supports_streaming: false,
            nosound: true,
            duration: c.duration,
            w: c.w,
            h: c.h,
            preload_prefix_size: None,
            video_start_ts: None,
            video_codec: None,
        };
        ("sticker.webm", "video/webm", video.into())
    } else {
        ("sticker.webp", "image/webp", tl::types::DocumentAttributeImageSize { w: c.w, h: c.h }.into())
    };
    let media = reply::document(up, name, Some(mime), vec![size, sticker]);
    let stored = client.invoke(&tl::functions::messages::UploadMedia { business_connection_id: None, peer: tl::enums::InputPeer::PeerSelf, media }).await.map_err(rpc_text)?;
    match stored {
        tl::enums::MessageMedia::Document(tl::types::MessageMediaDocument { document: Some(tl::enums::Document::Document(d)), .. }) => Ok(input_doc(&d)),
        _ => Err(l!("Сервер не прийняв файл", "The server did not accept the file").into()),
    }
}

/// A picture (by its content, whatever the name) becomes WEBP, anything else goes through ffmpeg as video.
fn convert(src: &Path, emojis: bool) -> Result<Converted, String> {
    match image::ImageReader::open(src).map_err(|e| e.to_string())?.with_guessed_format().ok().and_then(|r| r.decode().ok()) {
        Some(img) => convert_image(img, emojis),
        None => convert_video(src, emojis),
    }
}

/// A picture scaled to 512 px on its longest side (a custom emoji to 100×100, centred), lossless WEBP.
fn sticker_picture(img: image::DynamicImage, emojis: bool) -> image::RgbaImage {
    let side = if emojis { 100 } else { 512 };
    // `resize` fits both ways, so small pictures grow to the size Telegram asks for too.
    let img = img.resize(side, side, image::imageops::FilterType::Lanczos3).to_rgba8();
    if !emojis {
        return img;
    }
    let mut canvas = image::RgbaImage::new(side, side);
    let (x, y) = ((side - img.width()) / 2, (side - img.height()) / 2);
    image::imageops::overlay(&mut canvas, &img, x.into(), y.into());
    canvas
}

fn convert_image(img: image::DynamicImage, emojis: bool) -> Result<Converted, String> {
    let img = sticker_picture(img, emojis);
    let (w, h) = img.dimensions();
    let mut buf = Vec::new();
    image::codecs::webp::WebPEncoder::new_lossless(&mut buf).encode(&img, w, h, image::ExtendedColorType::Rgba8).map_err(|e| e.to_string())?;
    let path = crate::platform::temp_file("sticker.webp");
    if buf.len() > STATIC_MAX_BYTES {
        // A busy photo doesn't fit losslessly; ffmpeg's lossy WEBP (when it has libwebp) usually does.
        let png = crate::platform::temp_file("sticker.png");
        img.save_with_format(&png, image::ImageFormat::Png).map_err(|e| e.to_string())?;
        let ok = crate::platform::tool("ffmpeg").args(["-v", "error", "-y", "-i"]).arg(&png).args(["-c:v", "libwebp", "-quality", "80"]).arg(&path).status().is_ok_and(|s| s.success());
        let _ = std::fs::remove_file(&png);
        let small = std::fs::metadata(&path).is_ok_and(|m| m.len() as usize <= STATIC_MAX_BYTES);
        if !ok || !small {
            let _ = std::fs::remove_file(&path);
            return Err(l!("Зображення завелике для стікера (до 512 КБ)", "The picture is too large for a sticker (512 KB at most)").into());
        }
    } else {
        std::fs::write(&path, &buf).map_err(|e| e.to_string())?;
    }
    Ok(Converted { path, video: false, w: w as i32, h: h as i32, duration: 0.0 })
}

/// VP9 WEBM without sound, up to 3 s at 30 fps, in a 512×512 frame (100×100 for emoji) with a
/// transparent border; the bitrate drops until the file fits Telegram's 256 KB.
fn convert_video(src: &Path, emojis: bool) -> Result<Converted, String> {
    let side = if emojis { 100 } else { 512 };
    let vf = format!("scale={side}:{side}:force_original_aspect_ratio=decrease,pad={side}:{side}:(ow-iw)/2:(oh-ih)/2:color=black@0,fps=30,format=yuva420p");
    let path = crate::platform::temp_file("sticker.webm");
    let mut duration = VIDEO_MAX_SECS;
    for kbps in [600, 300, 150, 80] {
        let res = crate::platform::tool("ffmpeg")
            .args(["-hide_banner", "-y", "-i"])
            .arg(src)
            .args(["-t", "3", "-an", "-vf", &vf, "-c:v", "libvpx-vp9", "-b:v", &format!("{kbps}k"), "-pix_fmt", "yuva420p", "-f", "webm"])
            .arg(&path)
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::piped())
            .output();
        let out = match res {
            Ok(o) if o.status.success() => o,
            Ok(_) => break,
            Err(_) => return Err(l!("Для відео потрібен ffmpeg", "Videos need ffmpeg").into()),
        };
        if let Some(d) = ffmpeg_duration(&String::from_utf8_lossy(&out.stderr)) {
            duration = d.min(VIDEO_MAX_SECS);
        }
        if std::fs::metadata(&path).is_ok_and(|m| m.len() <= VIDEO_MAX_BYTES) {
            return Ok(Converted { path, video: true, w: side as i32, h: side as i32, duration });
        }
    }
    let _ = std::fs::remove_file(&path);
    Err(l!("Не вдалося зробити стікер із цього файлу", "Could not make a sticker from this file").into())
}

/// The input's length from ffmpeg's "Duration: 00:00:02.50" line.
fn ffmpeg_duration(log: &str) -> Option<f64> {
    let t = log.split("Duration: ").nth(1)?.split(',').next()?;
    let mut parts = t.trim().split(':').map(|p| p.parse::<f64>().ok());
    let (h, m, s) = (parts.next()??, parts.next()??, parts.next()??);
    Some(h * 3600.0 + m * 60.0 + s)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn short_names() {
        assert!(short_name_ok("GlassCats"));
        assert!(short_name_ok("cats_2026"));
        assert!(!short_name_ok("2cats"));
        assert!(!short_name_ok("cats__x"));
        assert!(!short_name_ok("коти"));
        assert!(!short_name_ok(""));
        assert!(!short_name_ok(&"a".repeat(65)));
    }

    #[test]
    fn durations() {
        assert_eq!(ffmpeg_duration("  Duration: 00:00:02.50, start: 0.0"), Some(2.5));
        assert_eq!(ffmpeg_duration("  Duration: 00:01:00.00, start"), Some(60.0));
        assert_eq!(ffmpeg_duration("nothing"), None);
    }

    #[test]
    fn pictures_fit_telegram_sizes() {
        let wide = image::DynamicImage::new_rgba8(300, 150);
        let s = sticker_picture(wide.clone(), false);
        assert_eq!(s.dimensions(), (512, 256));
        assert_eq!(sticker_picture(wide, true).dimensions(), (100, 100));
        let tall = image::DynamicImage::new_rgba8(1000, 2000);
        assert_eq!(sticker_picture(tall, false).dimensions(), (256, 512));
        let mut buf = Vec::new();
        let img = sticker_picture(image::DynamicImage::new_rgba8(64, 64), false);
        image::codecs::webp::WebPEncoder::new_lossless(&mut buf).encode(&img, 512, 512, image::ExtendedColorType::Rgba8).unwrap();
        assert!(buf.len() < STATIC_MAX_BYTES);
    }
}
