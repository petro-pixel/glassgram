//! Demo stories: the strip, a peer's stories, stealth mode, profile grids, albums and viewers.

use super::*;

pub(super) fn handle(app: &mut App, cmd: Cmd) {
    match cmd {
        Cmd::LoadStories => {
            // Olena and the channel have unseen stories, Andriy's were watched.
            let demo = [(1, vec![1, 2, 3], 0), (4, vec![7], 0), (5, vec![4, 5], 5)]
                .into_iter()
                .filter_map(|(id, ids, max_read)| app.demo_chat(PeerId::user_unchecked(id)).cloned().map(|chat| StoryPeer { chat, ids, stories: Vec::new(), max_read }));
            let list = story_post::strip().into_iter().chain(demo).collect();
            app.push_event(Ev::Stories(list));
        }
        cmd @ (Cmd::LoadStoryTargets | Cmd::CanPostStory(_) | Cmd::PostStory(_)) => story_post::handle(app, cmd),
        cmd @ (Cmd::LoadPeerStories(peer) | Cmd::LoadStoryVideo { peer, .. }) if story_post::owns(peer) => story_post::handle(app, cmd),
        Cmd::LoadPeerStories(peer) => {
            let ids = app.stories_ids(peer);
            let now = jiff::Timestamp::now().as_second();
            let captions = ["The new chat screen in glass ✨", "", "Sunset from the balcony 🌅", "Rust 1.95 is here", "", "", "Tech week in review"];
            let stories = ids
                .iter()
                .enumerate()
                .map(|(k, id)| Story { id: *id, date: now - 3600 * (ids.len() - k) as i64, video: *id == 2, duration: if *id == 2 { 5.0 } else { 0.0 }, caption: captions.get(*id as usize - 1).copied().unwrap_or_default().into(), reaction: None, views: None, reactions: 0 })
                .collect();
            for id in &ids {
                app.push_event(Ev::Image { key: ImgKey::Story(peer, *id), image: story_picture(*id) });
            }
            let max_read = if peer == PeerId::user_unchecked(5) { 5 } else { 0 };
            app.push_event(Ev::PeerStories { peer, stories, max_read });
        }
        Cmd::ReadStories { .. } | Cmd::ReactStory { .. } => {}
        Cmd::StoryStealth => {
            // 5 minutes back and 25 ahead, then an hour's wait, as the server's defaults.
            let now = jiff::Timestamp::now().as_second();
            app.push_event(Ev::StoryStealth { active_until: now + 25 * 60, cooldown_until: now + 60 * 60 });
        }
        Cmd::LoadProfileStories { peer, list, offset } => {
            let all = profile_story_ids(peer, list);
            let rest: Vec<i32> = match list {
                StoryList::Album(_) => all.iter().copied().skip(offset as usize).collect(),
                _ => all.iter().copied().filter(|id| offset == 0 || *id < offset).collect(),
            };
            let page: Vec<i32> = rest.iter().copied().take(STORY_PAGE as usize).collect();
            let now = jiff::Timestamp::now().as_second();
            let own = peer == PeerId::user_unchecked(ME);
            let captions = ["Morning in the Carpathians 🏔", "", "Glassgram in light theme", "", "Coffee and code ☕"];
            let stories = page
                .iter()
                .map(|&id| {
                    let video = id % 10 == 4;
                    // Our own stories carry the counters, as the server sends them only to the owner.
                    let views = own.then_some((id * 37) % 180 + 12);
                    Story { id, date: now - 86_400 * (1 + (id % 100) as i64), video, duration: if video { 5.0 } else { 0.0 }, caption: captions[id as usize % captions.len()].into(), reaction: None, views, reactions: if own { id % 9 } else { 0 } }
                })
                .collect();
            for id in &page {
                app.push_event(Ev::Image { key: ImgKey::Story(peer, *id), image: story_picture(*id) });
            }
            let next = match list {
                StoryList::Album(_) => offset + page.len() as i32,
                _ => page.iter().copied().min().unwrap_or(0),
            };
            let done = rest.len() <= STORY_PAGE as usize;
            app.push_event(Ev::ProfileStories { peer, list, stories, total: all.len() as i32, next, done, append: offset != 0 });
        }
        Cmd::LoadStoryAlbums(peer) => {
            let albums = match peer.bot_api_dialog_id_unchecked() {
                1 => vec![(1, "Travel ✈"), (2, "Glassgram")],
                4 => vec![(3, "Releases")],
                ME => vec![(4, "Kyiv")],
                _ => Vec::new(),
            };
            let albums = albums.into_iter().map(|(id, title)| StoryAlbum { id, title: title.into() }).collect();
            app.push_event(Ev::StoryAlbums { peer, albums });
        }
        Cmd::LoadStoryViewers { peer, id, offset: _ } => {
            let now = jiff::Timestamp::now().as_second();
            let people = [(1, "Olena Koval", Some("❤")), (8, "Maria Shevchenko", Some("🔥")), (5, "Andriy", None), (21, "Taras Bondar", None), (22, "Iryna Melnyk", Some("👍")), (23, "Dmytro Hnatiuk", None)];
            let viewers = people
                .iter()
                .enumerate()
                .map(|(k, (uid, name, r))| StoryViewer { peer: PeerId::user_unchecked(*uid), name: (*name).into(), date: now - 600 * (k as i64 + 1), reaction: r.map(Into::into) })
                .collect();
            let views = (id * 37) % 180 + 12;
            app.push_event(Ev::StoryViewers { peer, id, viewers, views, reactions: id % 9, next: String::new(), append: false });
        }
        // Story 2 is a video: an upright test picture with a tone.
        Cmd::LoadStoryVideo { peer, id } => match clip(86) {
            Some(path) => app.push_event(Ev::StoryVideo { peer, id, path }),
            None => app.push_event(Ev::Notice(l!("Для демо-відео потрібен ffmpeg", "The demo video needs ffmpeg").into())),
        },
        Cmd::ReplyStory { .. } => app.push_event(Ev::Notice(l!("Відповідь надіслано", "Reply sent").into())),
        _ => {}
    }
}
