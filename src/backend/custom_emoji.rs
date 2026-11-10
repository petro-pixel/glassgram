//! Custom emoji inside message text (`messages.getCustomEmojiDocuments`): the documents are
//! looked up in batches and their pictures go through the loader as `ImgKey::Emoji`.

use grammers_client::tl;

use super::loader::DocLoc;
use super::{Cmd, EMOJI_PT, Ev, ImgKey, Worker};
use crate::i18n::lf;

/// The server answers at most this many documents per request.
const BATCH: usize = 200;

impl Worker {
    pub(super) async fn custom_emoji_cmd(&mut self, cmd: Cmd) -> Result<(), String> {
        let Cmd::LoadCustomEmoji(ids) = cmd else { return Ok(()) };
        let client = self.client()?;
        let Some(loader) = self.loader.clone() else { return Ok(()) };
        let out = self.out.clone();
        tokio::spawn(async move {
            for chunk in ids.chunks(BATCH) {
                let req = tl::functions::messages::GetCustomEmojiDocuments { document_id: chunk.to_vec() };
                match client.invoke(&req).await {
                    Ok(docs) => {
                        for doc in docs {
                            if let tl::enums::Document::Document(d) = doc
                                && let Some(thumb) = picture(&d)
                            {
                                // "st…" counts as stickers in Data and storage.
                                let file = format!("ste{}{}.img", d.id, if thumb.is_empty() { "" } else { "_t" });
                                if d.mime_type == "application/x-tgsticker" {
                                    // The loader writes the file before its first frame reaches the UI.
                                    out.send(Ev::CustomEmojiFile { id: d.id, path: crate::platform::data_dir().join("cache").join(&file) });
                                }
                                loader.fetch(ImgKey::Emoji(d.id), file, EMOJI_PT, DocLoc { doc: d, thumb });
                            }
                        }
                    }
                    // The fallback emoji stays in the text: nothing is lost.
                    Err(e) => super::log_error(&lf!("Власні емодзі: {e}", "Custom emoji: {e}")),
                }
            }
        });
        Ok(())
    }
}

/// What to download for a still picture: `""` is the file itself (webp, or `.tgs` whose first
/// frame `loader::decode` draws), otherwise a thumbnail size of a video emoji.
fn picture(d: &tl::types::Document) -> Option<String> {
    match d.mime_type.as_str() {
        "image/webp" | "application/x-tgsticker" => Some(String::new()),
        _ => d.thumbs.iter().flatten().find_map(|t| match t {
            tl::enums::PhotoSize::Size(s) => Some(s.r#type.clone()),
            _ => None,
        }),
    }
}
