//! Sticker sets, favourite stickers and saved GIFs, as in Telegram Desktop's sticker panel:
//! installed sets after the favourite and recent ones, the set box with Add / Remove, search and
//! trending sets, the order of installed sets, and GIFs saved to the cloud.

use super::*;

/// Pseudo-set of the panel's "Recently used" tab.
pub const RECENT_SET: i64 = 0;
/// Pseudo-set of the panel's "Favourites" tab.
pub const FAVED_SET: i64 = -1;
/// Installed sets fetched in full for the panel: each is a request.
const SETS_MAX: usize = 40;
/// Stickers shown for a found or trending set before it is opened.
const COVERS_MAX: usize = 5;

/// A sticker set: installed, opened in the box, or found.
#[derive(Debug, Clone)]
pub struct StickerSetInfo {
    pub id: i64,
    pub title: String,
    /// For `t.me/addstickers/<short_name>`.
    pub short_name: String,
    /// Stickers in the whole set; a found set lists only a few of them.
    pub count: usize,
    pub installed: bool,
    /// (document id, emoji)
    pub stickers: Vec<(i64, String)>,
}

/// Which set the box opens.
#[derive(Debug, Clone, PartialEq)]
pub enum StickerSetRef {
    /// A set we have listed (installed, found or trending).
    Id(i64),
    /// From a `t.me/addstickers/<name>` link.
    Name(String),
    /// The set of a sticker.
    Doc(DocRef),
}

/// A sticker or GIF: one we have listed, or the one in a message.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum DocRef {
    Doc(i64),
    Msg(PeerId, i32),
}

/// The set name of a `t.me/addstickers/<name>` or `tg://addstickers?set=<name>` link.
pub fn addstickers_name(url: &str) -> Option<String> {
    let name = if let Some(q) = url.strip_prefix("tg://addstickers?") {
        q.split('&').find_map(|kv| kv.strip_prefix("set="))?
    } else {
        let rest = url.split_once("://").map_or(url, |(_, r)| r);
        let rest = rest.strip_prefix("t.me/").or_else(|| rest.strip_prefix("telegram.me/"))?;
        rest.strip_prefix("addstickers/")?.split(['/', '?', '#']).next()?
    };
    (!name.is_empty() && name.chars().all(|c| c.is_ascii_alphanumeric() || c == '_')).then(|| name.to_owned())
}

impl Worker {
    pub(super) async fn stickers_cmd(&mut self, cmd: Cmd) -> Result<(), String> {
        let client = self.client()?;
        let err = |e: grammers_client::InvocationError| lf!("Стікери: {e}", "Stickers: {e}");
        match cmd {
            Cmd::LoadStickers => self.load_stickers(&client).await?,
            Cmd::OpenStickerSet(set) => {
                let input = match set {
                    StickerSetRef::Id(id) => self.set_input(id)?,
                    StickerSetRef::Name(short_name) => tl::types::InputStickerSetShortName { short_name }.into(),
                    StickerSetRef::Doc(doc) => {
                        let d = self.doc_of(&client, doc).await?;
                        d.attributes
                            .iter()
                            .find_map(|a| match a {
                                tl::enums::DocumentAttribute::Sticker(s) if !matches!(s.stickerset, tl::enums::InputStickerSet::Empty) => Some(s.stickerset.clone()),
                                _ => None,
                            })
                            .ok_or(l!("Цей стікер не з набору", "This sticker is not from a set"))?
                    }
                };
                let set = self.full_set(&client, input).await?;
                self.out.send(Ev::StickerSet(set));
            }
            Cmd::InstallStickerSet { id, install } => {
                let input = self.set_input(id)?;
                if install {
                    let res = client.invoke(&tl::functions::messages::InstallStickerSet { stickerset: input.clone(), archived: false }).await.map_err(err)?;
                    if let tl::enums::messages::StickerSetInstallResult::Archive(a) = res
                        && !a.sets.is_empty()
                    {
                        // Over the limit of installed sets the server archives the oldest ones.
                        let n = a.sets.len();
                        self.out.send(Ev::Notice(lf!("Старі набори перенесено в архів: {n}", "Old sets moved to the archive: {n}")));
                    }
                    let set = self.full_set(&client, input).await?;
                    self.out.send(Ev::StickerSetChanged(StickerSetInfo { installed: true, ..set }));
                } else {
                    client.invoke(&tl::functions::messages::UninstallStickerSet { stickerset: input }).await.map_err(err)?;
                    let set = StickerSetInfo { id, title: String::new(), short_name: String::new(), count: 0, installed: false, stickers: Vec::new() };
                    self.out.send(Ev::StickerSetChanged(set));
                }
            }
            Cmd::SearchStickerSets(query) => {
                let covered = if query.trim().is_empty() {
                    match client.invoke(&tl::functions::messages::GetFeaturedStickers { hash: 0 }).await.map_err(err)? {
                        tl::enums::messages::FeaturedStickers::Stickers(f) => f.sets,
                        tl::enums::messages::FeaturedStickers::NotModified(_) => Vec::new(),
                    }
                } else {
                    let req = tl::functions::messages::SearchStickerSets { exclude_featured: false, q: query.trim().to_owned(), hash: 0 };
                    match client.invoke(&req).await.map_err(err)? {
                        tl::enums::messages::FoundStickerSets::Sets(f) => f.sets,
                        tl::enums::messages::FoundStickerSets::NotModified => Vec::new(),
                    }
                };
                let sets = covered.into_iter().filter_map(|c| self.covered(c)).collect();
                self.out.send(Ev::FoundStickerSets { query, sets });
            }
            Cmd::ReorderStickerSets(order) => {
                client.invoke(&tl::functions::messages::ReorderStickerSets { masks: false, emojis: false, order }).await.map_err(err)?;
            }
            Cmd::FaveSticker { doc, unfave } => {
                let d = self.doc_of(&client, doc).await?;
                client.invoke(&tl::functions::messages::FaveSticker { id: input_doc(&d), unfave }).await.map_err(err)?;
                let faved = self.faved(&client).await.map_err(err)?;
                self.out.send(Ev::FavedStickers(faved));
            }
            Cmd::LoadSavedGifs => {
                let gifs = self.saved_gifs(&client).await.map_err(err)?;
                self.out.send(Ev::SavedGifs(gifs));
            }
            Cmd::SaveGif { doc, unsave } => {
                let d = self.doc_of(&client, doc).await?;
                client.invoke(&tl::functions::messages::SaveGif { id: input_doc(&d), unsave }).await.map_err(err)?;
                let gifs = self.saved_gifs(&client).await.map_err(err)?;
                self.out.send(Ev::SavedGifs(gifs));
                if !unsave {
                    self.out.send(Ev::Notice(l!("GIF збережено", "GIF saved").into()));
                }
            }
            _ => {}
        }
        Ok(())
    }

    /// Favourite and recent stickers first, then the installed sets in the user's order.
    async fn load_stickers(&mut self, client: &Client) -> Result<(), String> {
        let mut sets = Vec::new();
        let pseudo = |id, title: &str, stickers: Vec<(i64, String)>| StickerSetInfo { id, title: title.into(), short_name: String::new(), count: stickers.len(), installed: true, stickers };
        if let Ok(faved) = self.faved(client).await {
            sets.push(pseudo(FAVED_SET, l!("Обрані", "Favourites"), faved));
        }
        if let Ok(tl::enums::messages::RecentStickers::Stickers(r)) = client.invoke(&tl::functions::messages::GetRecentStickers { attached: false, hash: 0 }).await {
            sets.push(pseudo(RECENT_SET, l!("Нещодавні", "Recent"), self.remember_docs(r.stickers)));
        }
        let all = client.invoke(&tl::functions::messages::GetAllStickers { hash: 0 }).await.map_err(|e| lf!("Стікери: {e}", "Stickers: {e}"))?;
        if let tl::enums::messages::AllStickers::Stickers(all) = all {
            for tl::enums::StickerSet::Set(set) in all.sets.into_iter().filter(|tl::enums::StickerSet::Set(s)| !s.archived && !s.masks && !s.emojis).take(SETS_MAX) {
                self.set_hashes.insert(set.id, set.access_hash);
                let input = tl::types::InputStickerSetId { id: set.id, access_hash: set.access_hash };
                if let Ok(full) = self.full_set(client, input.into()).await {
                    sets.push(full);
                }
            }
        }
        self.out.send(Ev::Stickers(sets));
        Ok(())
    }

    /// The input of a set we have listed.
    fn set_input(&self, id: i64) -> Result<tl::enums::InputStickerSet, String> {
        let access_hash = *self.set_hashes.get(&id).ok_or(l!("Набір стікерів не знайдено", "Sticker set not found"))?;
        Ok(tl::types::InputStickerSetId { id, access_hash }.into())
    }

    /// A whole set with its stickers, remembered for sending and for the Add / Remove button.
    async fn full_set(&mut self, client: &Client, input: tl::enums::InputStickerSet) -> Result<StickerSetInfo, String> {
        let not_found = || l!("Набір стікерів не знайдено", "Sticker set not found").to_owned();
        let res = client.invoke(&tl::functions::messages::GetStickerSet { stickerset: input, hash: 0 }).await.map_err(|e| match &e {
            grammers_client::InvocationError::Rpc(r) if r.name == "STICKERSET_INVALID" => not_found(),
            _ => lf!("Стікери: {e}", "Stickers: {e}"),
        })?;
        let tl::enums::messages::StickerSet::Set(full) = res else { return Err(not_found()) };
        let tl::enums::StickerSet::Set(set) = full.set;
        let stickers = self.remember_docs(full.documents);
        Ok(self.set_info(set, stickers))
    }

    fn set_info(&mut self, set: tl::types::StickerSet, stickers: Vec<(i64, String)>) -> StickerSetInfo {
        self.set_hashes.insert(set.id, set.access_hash);
        StickerSetInfo {
            id: set.id,
            title: set.title,
            short_name: set.short_name,
            count: (set.count.max(0) as usize).max(stickers.len()),
            installed: set.installed_date.is_some() && !set.archived,
            stickers,
        }
    }

    /// A found or trending set with the few stickers the server previews.
    fn covered(&mut self, c: tl::enums::StickerSetCovered) -> Option<StickerSetInfo> {
        let (set, docs) = match c {
            tl::enums::StickerSetCovered::Covered(c) => (c.set, vec![c.cover]),
            tl::enums::StickerSetCovered::StickerSetMultiCovered(c) => (c.set, c.covers),
            tl::enums::StickerSetCovered::StickerSetFullCovered(c) => (c.set, c.documents),
            tl::enums::StickerSetCovered::StickerSetNoCovered(c) => (c.set, Vec::new()),
        };
        let tl::enums::StickerSet::Set(set) = set;
        if set.masks || set.emojis {
            return None;
        }
        let stickers = self.remember_docs(docs.into_iter().take(COVERS_MAX).collect());
        Some(self.set_info(set, stickers))
    }

    async fn faved(&mut self, client: &Client) -> Result<Vec<(i64, String)>, grammers_client::InvocationError> {
        Ok(match client.invoke(&tl::functions::messages::GetFavedStickers { hash: 0 }).await? {
            tl::enums::messages::FavedStickers::Stickers(f) => self.remember_docs(f.stickers),
            tl::enums::messages::FavedStickers::NotModified => Vec::new(),
        })
    }

    async fn saved_gifs(&mut self, client: &Client) -> Result<Vec<i64>, grammers_client::InvocationError> {
        Ok(match client.invoke(&tl::functions::messages::GetSavedGifs { hash: 0 }).await? {
            tl::enums::messages::SavedGifs::Gifs(g) => self.remember_docs(g.gifs).into_iter().map(|(id, _)| id).collect(),
            tl::enums::messages::SavedGifs::NotModified => Vec::new(),
        })
    }

    /// The document of a listed sticker or GIF, or of the one in a message (then remembered).
    async fn doc_of(&mut self, client: &Client, doc: DocRef) -> Result<tl::types::Document, String> {
        match doc {
            DocRef::Doc(id) => self.docs.get(&id).cloned().ok_or_else(|| l!("Файл не знайдено", "File not found").to_owned()),
            DocRef::Msg(peer, id) => {
                let r = self.peer_ref(peer)?;
                let m = client.get_messages_by_id(r, &[id]).await.map_err(|e| e.to_string())?.into_iter().flatten().next();
                let d = m.as_ref().and_then(raw_document).ok_or(l!("Повідомлення не знайдено", "Message not found"))?;
                self.docs.insert(d.id, d.clone());
                Ok(d)
            }
        }
    }

    /// Stickers and GIFs kept for sending and previews, with each one's emoji.
    pub(super) fn remember_docs(&mut self, docs: Vec<tl::enums::Document>) -> Vec<(i64, String)> {
        docs.into_iter()
            .filter_map(|d| match d {
                tl::enums::Document::Document(d) => Some(d),
                _ => None,
            })
            .map(|d| {
                let emoji = d
                    .attributes
                    .iter()
                    .find_map(|a| match a {
                        tl::enums::DocumentAttribute::Sticker(s) => Some(s.alt.clone()),
                        _ => None,
                    })
                    .unwrap_or_default();
                let id = d.id;
                self.docs.insert(id, d);
                (id, emoji)
            })
            .collect()
    }
}

fn input_doc(d: &tl::types::Document) -> tl::enums::InputDocument {
    tl::types::InputDocument { id: d.id, access_hash: d.access_hash, file_reference: d.file_reference.clone() }.into()
}

#[cfg(test)]
mod tests {
    use super::addstickers_name;

    #[test]
    fn addstickers_links() {
        assert_eq!(addstickers_name("https://t.me/addstickers/Cats_by_x"), Some("Cats_by_x".into()));
        assert_eq!(addstickers_name("t.me/addstickers/Cats?x=1"), Some("Cats".into()));
        assert_eq!(addstickers_name("tg://addstickers?set=Cats"), Some("Cats".into()));
        assert_eq!(addstickers_name("https://t.me/addstickers/"), None);
        assert_eq!(addstickers_name("https://t.me/addstickers/a%2Fb"), None);
        assert_eq!(addstickers_name("https://t.me/durov"), None);
    }
}
