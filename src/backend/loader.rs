//! Background download of avatars, photos, stickers and thumbnails, with a disk cache and decoding off the UI thread.

use super::*;

/// Background loading of avatars and photos with a disk cache.
#[derive(Clone)]
pub(super) struct Loader {
    pub(super) client: Client,
    pub(super) out: Emitter,
    pub(super) limit: Arc<Semaphore>,
    pub(super) requested: Arc<Mutex<HashSet<String>>>,
    /// Cache file and size of every picture sent to the UI, so one it dropped can be decoded again.
    pub(super) files: Arc<Mutex<HashMap<ImgKey, (String, f32)>>>,
}

impl Loader {
    /// The UI dropped these pictures to save memory and needs them again: decode from the disk cache.
    pub(super) fn reload(&self, keys: Vec<ImgKey>) {
        for key in keys {
            let Some((file, max_pt)) = self.files.lock().unwrap().get(&key).cloned() else { continue };
            let this = self.clone();
            tokio::spawn(async move {
                let _permit = this.limit.acquire().await;
                let Ok(bytes) = tokio::fs::read(data_dir().join("cache").join(&file)).await else {
                    // Gone from the cache: the next request downloads it again.
                    this.requested.lock().unwrap().remove(&file);
                    return;
                };
                let max_px = (max_pt * this.out.ctx.pixels_per_point()).ceil() as u32;
                if let Ok(Some(image)) = tokio::task::spawn_blocking(move || decode(&bytes, max_px)).await {
                    this.out.send(Ev::Image { key, image });
                }
            });
        }
    }
}

impl Loader {
    pub(super) fn fetch<D>(&self, key: ImgKey, file: String, max_pt: f32, source: D)
    where
        D: Downloadable + Send + Sync + 'static,
    {
        if !self.requested.lock().unwrap().insert(file.clone()) {
            return;
        }
        self.files.lock().unwrap().insert(key, (file.clone(), max_pt));
        let this = self.clone();
        tokio::spawn(async move {
            let _permit = this.limit.acquire().await;
            let path = data_dir().join("cache").join(&file);
            let bytes = match tokio::fs::read(&path).await {
                Ok(b) => b,
                Err(_) => {
                    let mut data = Vec::new();
                    let mut it = this.client.iter_download(&source);
                    while let Ok(Some(chunk)) = it.next().await {
                        data.extend(chunk);
                    }
                    if data.is_empty() {
                        this.requested.lock().unwrap().remove(&file);
                        return;
                    }
                    let _ = tokio::fs::create_dir_all(path.parent().unwrap()).await;
                    let _ = tokio::fs::write(&path, &data).await;
                    data
                }
            };
            let max_px = (max_pt * this.out.ctx.pixels_per_point()).ceil() as u32;
            let decoded = tokio::task::spawn_blocking(move || decode(&bytes, max_px)).await;
            if let Ok(Some(image)) = decoded {
                this.out.send(Ev::Image { key, image });
            }
        });
    }

    pub(super) fn avatar(&self, peer: &Peer) {
        let photo_id = match peer {
            Peer::User(u) => u.photo().map(|p| p.photo_id),
            Peer::Group(g) => g.photo().map(|p| p.photo_id),
            Peer::Channel(c) => c.photo().map(|p| p.photo_id),
            Peer::Community(c) => c.photo().map(|p| p.photo_id),
        };
        let Some(photo_id) = photo_id else { return };
        let file = format!("a{}_{photo_id}.jpg", peer.id().bot_api_dialog_id_unchecked());
        if self.requested.lock().unwrap().contains(&file) {
            return;
        }
        let (this, peer) = (self.clone(), peer.clone());
        tokio::spawn(async move {
            if let Ok(Some(photo)) = peer.photo(false).await {
                this.fetch(ImgKey::Avatar(peer.id()), file, AVATAR_PT, photo);
            }
        });
    }

    pub(super) fn message_photo(&self, peer: PeerId, m: &Message) {
        if let (Some(Media::Sticker(_)), Some(doc)) = (m.media(), raw_document(m)) {
            return self.sticker(&doc);
        }
        if let (Some(Media::Document(_)), Some(doc)) = (m.media(), raw_document(m))
            && let Some(thumb) = doc.thumbs.iter().flatten().filter_map(|t| match t {
                tl::enums::PhotoSize::Size(s) => Some((s.w, s.r#type.clone())),
                tl::enums::PhotoSize::Progressive(s) => Some((s.w, s.r#type.clone())),
                _ => None,
            }).max_by_key(|t| t.0)
        {
            // Video or GIF thumbnail: shown as a photo until the file itself is downloaded.
            let file = format!("v{}_{}.jpg", peer.bot_api_dialog_id_unchecked(), m.id());
            return self.fetch(ImgKey::Photo(peer, m.id()), file, PHOTO_PT, DocLoc { doc, thumb: thumb.1 });
        }
        if matches!(m.media(), Some(Media::WebPage(_))) {
            return self.web_page_photo(peer, m);
        }
        let Some(Media::Photo(photo)) = m.media() else { return };
        let file = format!("p{}_{}.jpg", peer.bot_api_dialog_id_unchecked(), m.id());
        let key = ImgKey::Photo(peer, m.id());
        match best_thumb(&photo) {
            Some(size) => self.fetch(key, file, PHOTO_PT, size),
            None => self.fetch(key, file, PHOTO_PT, photo),
        }
    }
}

/// Location of a document (sticker) to download: the file itself or its thumbnail.
pub(super) struct DocLoc {
    pub(super) doc: tl::types::Document,
    pub(super) thumb: String,
}

impl Downloadable for DocLoc {
    fn to_raw_input_location(&self) -> Option<tl::enums::InputFileLocation> {
        Some(tl::types::InputDocumentFileLocation {
            id: self.doc.id,
            access_hash: self.doc.access_hash,
            file_reference: self.doc.file_reference.clone(),
            thumb_size: self.thumb.clone(),
        }.into())
    }

    fn dc_id(&self) -> Option<i32> {
        Some(self.doc.dc_id)
    }
}

impl Loader {
    /// Static webp stickers are downloaded whole, animated ones (tgs/webm) as a thumbnail.
    pub(super) fn sticker(&self, doc: &tl::types::Document) {
        let thumb = if doc.mime_type == "image/webp" {
            String::new()
        } else {
            let Some(t) = doc.thumbs.iter().flatten().find_map(|t| match t {
                tl::enums::PhotoSize::Size(s) => Some(s.r#type.clone()),
                _ => None,
            }) else {
                return;
            };
            t
        };
        let file = format!("s{}{}.img", doc.id, if thumb.is_empty() { "" } else { "_t" });
        self.fetch(ImgKey::Sticker(doc.id), file, STICKER_PT, DocLoc { doc: doc.clone(), thumb });
    }
}

pub(super) fn raw_document(m: &Message) -> Option<tl::types::Document> {
    let doc = match m.media()? {
        Media::Sticker(s) => s.document.raw.document,
        Media::Document(d) => d.raw.document,
        _ => return None,
    };
    match doc? {
        tl::enums::Document::Document(d) => Some(d),
        _ => None,
    }
}

pub(super) fn best_thumb(photo: &Photo) -> Option<grammers_client::media::PhotoSize> {
    let thumbs = photo.thumbs();
    ["x", "m", "y", "w"]
        .iter()
        .find_map(|t| thumbs.iter().find(|s| s.photo_type() == *t).cloned())
}

pub(super) fn decode(bytes: &[u8], max_px: u32) -> Option<ColorImage> {
    let img = image::load_from_memory(bytes).ok()?;
    let img = if img.width() > max_px || img.height() > max_px {
        // Lanczos gives clean downscaling without moiré on fine details.
        img.resize(max_px, max_px, image::imageops::FilterType::Lanczos3)
    } else {
        img
    };
    let rgba = img.to_rgba8();
    Some(ColorImage::from_rgba_unmultiplied([rgba.width() as usize, rgba.height() as usize], rgba.as_raw()))
}
