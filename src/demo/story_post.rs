//! Demo story posting: we may post as ourselves or as "Rust Team News"; "Glassgram Dev" lacks
//! boosts. Posted stories stay in the strip for the session, the picked file as their picture.

use std::cell::RefCell;
use std::path::PathBuf;

use eframe::egui::ColorImage;
use grammers_client::session::types::PeerId;

use crate::app::App;
use crate::backend::{ChatInfo, ChatKind, Cmd, Ev, ImgKey, Presence, Story, StoryPeer, probe_video};
use crate::i18n::l;

/// Channels offered by `stories.getChatsToSend`: (id, title, can post now).
const CHANNELS: [(i64, &str, bool); 2] = [(40, "Rust Team News", true), (41, "Glassgram Dev", false)];

thread_local! {
    /// Posted this session: the owner and its stories with their files.
    static POSTED: RefCell<Vec<(ChatInfo, Vec<(Story, PathBuf)>)>> = const { RefCell::new(Vec::new()) };
}

fn channel(id: i64, title: &str) -> ChatInfo {
    ChatInfo { id: PeerId::channel_unchecked(id), kind: ChatKind::Channel, ..super::person(0, title, "", Presence::None) }
}

/// Whether a peer's stories are the ones posted here.
pub(super) fn owns(peer: PeerId) -> bool {
    POSTED.with_borrow(|p| p.iter().any(|(c, _)| c.id == peer))
}

/// Our posted stories at the head of the strip.
pub(super) fn strip() -> Vec<StoryPeer> {
    POSTED.with_borrow(|p| {
        p.iter()
            .map(|(chat, list)| {
                let ids: Vec<i32> = list.iter().map(|(s, _)| s.id).collect();
                StoryPeer { chat: chat.clone(), max_read: ids.last().copied().unwrap_or(0), ids, stories: Vec::new() }
            })
            .collect()
    })
}

fn picture(path: &std::path::Path) -> Option<ColorImage> {
    let img = image::open(path).ok()?.thumbnail(900, 900).to_rgba8();
    Some(ColorImage::from_rgba_unmultiplied([img.width() as usize, img.height() as usize], img.as_raw()))
}

pub(super) fn handle(app: &mut App, cmd: Cmd) {
    match cmd {
        Cmd::LoadStoryTargets => app.push_event(Ev::StoryTargets(CHANNELS.iter().map(|&(id, title, _)| channel(id, title)).collect())),
        Cmd::CanPostStory(target) => {
            let open = target.is_none_or(|t| CHANNELS.iter().any(|c| PeerId::channel_unchecked(c.0) == t && c.2));
            let result = if open { Ok(if target.is_some() { 3 } else { 97 }) } else { Err(l!("Каналу потрібно більше бустів, щоб публікувати історії", "The channel needs more boosts to post stories").into()) };
            app.push_event(Ev::StoryCanPost { target, result });
        }
        Cmd::PostStory(d) => {
            let chat = match d.target {
                Some(t) => CHANNELS.iter().find(|c| PeerId::channel_unchecked(c.0) == t).map(|c| channel(c.0, c.1)),
                None => Some(super::person(super::ME, "Demo User", "", Presence::None)),
            };
            let Some(chat) = chat else { return app.push_event(Ev::StoryPostFailed(l!("Невідомий канал", "Unknown channel").into())) };
            let owner = chat.id;
            let video = crate::backend::is_story_video(&d.path);
            let duration = if video { probe_video(&d.path).map_or(5.0, |p| p.0) } else { 0.0 };
            let id = POSTED.with_borrow(|p| p.iter().flat_map(|(_, l)| l.iter().map(|(s, _)| s.id)).max().unwrap_or(100)) + 1;
            let (caption, _) = crate::format::parse(d.caption.trim());
            let story = Story { id, date: jiff::Timestamp::now().as_second(), video, duration, caption, reaction: None, views: Some(0), reactions: 0 };
            POSTED.with_borrow_mut(|p| match p.iter_mut().find(|(c, _)| c.id == owner) {
                Some((_, list)) => list.push((story.clone(), d.path.clone())),
                None => p.insert(0, (chat, vec![(story.clone(), d.path.clone())])),
            });
            if let Some(image) = picture(&d.path) {
                app.push_event(Ev::Image { key: ImgKey::Story(owner, id), image });
            }
            app.push_event(Ev::UploadDone(d.upload));
            app.push_event(Ev::StoryPosted { peer: Some(owner), story: Some(story) });
        }
        Cmd::LoadPeerStories(peer) => {
            let stories = POSTED.with_borrow(|p| p.iter().find(|(c, _)| c.id == peer).map(|(_, l)| l.iter().map(|(s, _)| s.clone()).collect()).unwrap_or_default());
            app.push_event(Ev::PeerStories { peer, stories, max_read: 0 });
        }
        Cmd::LoadStoryVideo { peer, id } => {
            let path = POSTED.with_borrow(|p| p.iter().find(|(c, _)| c.id == peer).and_then(|(_, l)| l.iter().find(|(s, _)| s.id == id)).map(|(_, f)| f.clone()));
            if let Some(path) = path {
                app.push_event(Ev::StoryVideo { peer, id, path });
            }
        }
        _ => {}
    }
}
