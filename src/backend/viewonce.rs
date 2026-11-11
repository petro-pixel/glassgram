//! Self-destructing media: photos and videos with `ttl_seconds`, "view once" among them.
//! Received ones are read from the message media; ours go out with `ttl_seconds` set.

use super::*;

/// The `ttl_seconds` Telegram reads as "view once".
pub const VIEW_ONCE: i32 = 0x7FFF_FFFF;

/// The timer of a message's media.
pub(super) struct Ttl {
    pub(super) secs: i32,
    /// It has run out: the server dropped the file.
    pub(super) expired: bool,
    /// A video (or a round or voice message), not a photo.
    pub(super) video: bool,
}

pub(super) fn ttl(raw: &tl::enums::Message) -> Option<Ttl> {
    let tl::enums::Message::Message(x) = raw else { return None };
    match &x.media {
        Some(tl::enums::MessageMedia::Photo(p)) => p.ttl_seconds.map(|secs| Ttl { secs, expired: p.photo.is_none(), video: false }),
        Some(tl::enums::MessageMedia::Document(d)) => d.ttl_seconds.map(|secs| Ttl { secs, expired: d.document.is_none(), video: true }),
        _ => None,
    }
}

/// Sends the uploaded photo or video "view once".
pub(super) fn view_once(mut media: tl::enums::InputMedia) -> tl::enums::InputMedia {
    match &mut media {
        tl::enums::InputMedia::UploadedPhoto(p) => p.ttl_seconds = Some(VIEW_ONCE),
        tl::enums::InputMedia::UploadedDocument(d) if d.mime_type.starts_with("video/") => d.ttl_seconds = Some(VIEW_ONCE),
        _ => {}
    }
    media
}

impl Msg {
    /// The media is gone (viewed, or its timer ran out): a line saying so stays in its place.
    pub fn expire_media(&mut self) {
        let video = !self.photo;
        self.text = if video { l!("Відео більше недоступне", "Video has expired") } else { l!("Фото більше недоступне", "Photo has expired") }.to_owned();
        self.spans = Vec::new();
        self.photo = false;
        self.media = None;
        self.file = None;
        self.spoiler = false;
        self.ttl = None;
    }

    /// Hidden behind the "view once" or timer cover.
    pub fn self_destructing(&self) -> bool {
        self.ttl.is_some() && (self.photo || self.media.as_ref().is_some_and(|i| i.kind == MediaKind::Video))
    }
}
