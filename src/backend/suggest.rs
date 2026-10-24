//! Composer helpers: link previews, members for @mentions, bot commands.

use grammers_client::media::{Media, Photo};
use grammers_client::message::Message;
use grammers_client::peer::Peer;
use grammers_client::session::types::{PeerId, PeerKind};
use grammers_client::{Client, tl};
use serde::{Deserialize, Serialize};

use super::{Ev, ImgKey, Loader, Worker, best_thumb, log_error, PHOTO_PT};
use crate::format;
use crate::i18n::lf;

/// How many members the @mention popup shows.
const MEMBERS_LIMIT: usize = 50;

/// Link preview (`webPage`), shown as a card under the text like in Telegram.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct WebPageInfo {
    pub id: i64,
    pub url: String,
    pub site: String,
    pub title: String,
    pub description: String,
    /// Has a picture: in a bubble it is loaded as the message thumbnail.
    pub photo: bool,
}

/// Chat member for the @mention popup.
#[derive(Debug, Clone)]
pub struct Mentionable {
    pub id: PeerId,
    pub name: String,
    /// Without `@`; empty when the user has no username.
    pub username: String,
    pub bot: bool,
}

/// Bot command for the `/` menu.
#[derive(Debug, Clone)]
pub struct BotCmd {
    /// Bot username without `@`: in groups with several bots the command goes as `/cmd@bot`.
    pub bot: String,
    pub command: String,
    pub description: String,
}

/// Card from `webPage`; pending or empty pages are not shown.
pub(super) fn web_page(w: &tl::enums::WebPage) -> Option<WebPageInfo> {
    let tl::enums::WebPage::Page(p) = w else { return None };
    let info = WebPageInfo {
        id: p.id,
        url: p.url.clone(),
        site: p.site_name.clone().unwrap_or_else(|| p.display_url.split('/').next().unwrap_or_default().to_owned()),
        title: p.title.clone().or_else(|| p.author.clone()).unwrap_or_default(),
        description: p.description.clone().unwrap_or_default(),
        photo: matches!(p.photo, Some(tl::enums::Photo::Photo(_))),
    };
    (!info.title.is_empty() || !info.description.is_empty() || info.photo).then_some(info)
}

pub(super) fn message_web_page(m: &Message) -> Option<WebPageInfo> {
    match m.media() {
        Some(Media::WebPage(w)) => web_page(&w.raw.webpage),
        _ => None,
    }
}

fn page_photo(w: &tl::enums::WebPage) -> Option<Photo> {
    match w {
        tl::enums::WebPage::Page(p) => p.photo.clone().map(Photo::from_raw),
        _ => None,
    }
}

impl Loader {
    /// Link preview picture, under the same key as a message photo.
    pub(super) fn web_page_photo(&self, peer: PeerId, m: &Message) {
        let Some(Media::WebPage(w)) = m.media() else { return };
        let Some(photo) = page_photo(&w.raw.webpage) else { return };
        let file = format!("w{}_{}.jpg", peer.bot_api_dialog_id_unchecked(), m.id());
        self.page_photo(ImgKey::Photo(peer, m.id()), file, photo);
    }

    fn page_photo(&self, key: ImgKey, file: String, photo: Photo) {
        match best_thumb(&photo) {
            Some(size) => self.fetch(key, file, PHOTO_PT, size),
            None => self.fetch(key, file, PHOTO_PT, photo),
        }
    }
}

impl Worker {
    /// Preview for the link being typed; always answers so the UI stops waiting.
    pub(super) fn web_preview(&self, text: String) -> Result<(), String> {
        let client = self.client()?;
        let (out, loader) = (self.out.clone(), self.loader.clone());
        tokio::spawn(async move {
            let (message, entities) = format::parse(&text);
            let req = tl::functions::messages::GetWebPagePreview { message, entities: (!entities.is_empty()).then_some(entities) };
            let page = match client.invoke(&req).await {
                Ok(tl::enums::messages::WebPagePreview::Preview(p)) => match p.media {
                    tl::enums::MessageMedia::WebPage(w) => {
                        let info = web_page(&w.webpage);
                        if let (Some(info), Some(photo), Some(loader)) = (&info, page_photo(&w.webpage), &loader) {
                            loader.page_photo(ImgKey::WebPage(info.id), format!("wp{}.jpg", info.id), photo);
                        }
                        info
                    }
                    _ => None,
                },
                Err(e) => {
                    log_error(&format!("webpage preview: {e}"));
                    None
                }
            };
            out.send(Ev::WebPreview { text, page });
        });
        Ok(())
    }

    /// Group or channel members matching a search (for @mentions).
    pub(super) fn load_members(&self, peer: PeerId, query: String) -> Result<(), String> {
        let client = self.client()?;
        let r = self.peer_ref(peer)?;
        let (out, loader) = (self.out.clone(), self.loader.clone());
        tokio::spawn(async move {
            // A basic group returns everyone at once, so we filter locally; supergroups search server-side.
            let filter = tl::types::ChannelParticipantsSearch { q: query.clone() }.into();
            let mut iter = client.iter_participants(r).filter(filter);
            let needle = query.to_lowercase();
            let mut members = Vec::new();
            loop {
                match iter.next().await {
                    Ok(Some(p)) => {
                        let u = p.user;
                        let (name, username) = (u.full_name(), u.username().unwrap_or_default().to_owned());
                        if u.is_self() || !(name.to_lowercase().contains(&needle) || username.to_lowercase().starts_with(&needle)) {
                            continue;
                        }
                        members.push(Mentionable { id: u.id(), name, username, bot: u.is_bot() });
                        if let Some(l) = &loader {
                            l.avatar(&Peer::User(u));
                        }
                        if members.len() >= MEMBERS_LIMIT {
                            break;
                        }
                    }
                    Ok(None) => break,
                    Err(e) => {
                        log_error(&format!("participants: {e}"));
                        break;
                    }
                }
            }
            out.send(Ev::Mentions { peer, query, members });
        });
        Ok(())
    }

    /// Chat bot commands, from `bot_info` of the full user or chat.
    pub(super) fn load_bot_commands(&self, peer: PeerId) -> Result<(), String> {
        let client = self.client()?;
        let r = self.peer_ref(peer)?;
        let out = self.out.clone();
        tokio::spawn(async move {
            match bot_commands(&client, r).await {
                Ok(commands) => out.send(Ev::BotCommands { peer, commands }),
                Err(e) => {
                    log_error(&e);
                    out.send(Ev::BotCommands { peer, commands: Vec::new() });
                }
            }
        });
        Ok(())
    }
}

async fn bot_commands(client: &Client, r: grammers_client::session::types::PeerRef) -> Result<Vec<BotCmd>, String> {
    let err = |e| lf!("Команди бота: {e}", "Bot commands: {e}");
    let (infos, users) = match r.id.kind() {
        PeerKind::User => {
            let tl::enums::users::UserFull::Full(f) = client.invoke(&tl::functions::users::GetFullUser { id: r.into() }).await.map_err(err)?;
            let tl::enums::UserFull::Full(full) = f.full_user;
            (full.bot_info.into_iter().collect::<Vec<_>>(), f.users)
        }
        PeerKind::Chat => {
            let req = tl::functions::messages::GetFullChat { chat_id: r.id.bare_id_unchecked() };
            let tl::enums::messages::ChatFull::Full(f) = client.invoke(&req).await.map_err(err)?;
            (chat_bot_info(f.full_chat), f.users)
        }
        PeerKind::Channel => {
            let tl::enums::messages::ChatFull::Full(f) = client.invoke(&tl::functions::channels::GetFullChannel { channel: r.into() }).await.map_err(err)?;
            (chat_bot_info(f.full_chat), f.users)
        }
    };
    let username = |id: i64| {
        users.iter().find_map(|u| match u {
            tl::enums::User::User(u) if u.id == id => u.username.clone(),
            _ => None,
        })
    };
    let mut out = Vec::new();
    for tl::enums::BotInfo::Info(info) in infos {
        let bot = info.user_id.and_then(username).unwrap_or_default();
        for tl::enums::BotCommand::Command(c) in info.commands.unwrap_or_default() {
            out.push(BotCmd { bot: bot.clone(), command: c.command, description: c.description });
        }
    }
    Ok(out)
}

fn chat_bot_info(full: tl::enums::ChatFull) -> Vec<tl::enums::BotInfo> {
    match full {
        tl::enums::ChatFull::Full(c) => c.bot_info.unwrap_or_default(),
        tl::enums::ChatFull::ChannelFull(c) => c.bot_info,
        tl::enums::ChatFull::CommunityFull(_) => Vec::new(),
    }
}
