//! Posting a story, as in Telegram Desktop: a photo or a video with a caption, who can see it,
//! how long it stays, whether it goes to the profile, as ourselves or as a channel we may post
//! stories for (`stories.getChatsToSend`, `stories.canSendStory`, `stories.sendStory`).

use super::stories::{peers_by_id, story_of};
use super::*;
use grammers_client::InvocationError;

/// Who sees a story of ours; channels' stories are public.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum StoryAudience {
    #[default]
    Everyone,
    Contacts,
    CloseFriends,
    /// Only the people in `StoryDraft::allow`.
    Selected,
}

/// A story ready to go out.
#[derive(Debug, Clone)]
pub struct StoryDraft {
    /// A channel or group to post as; `None` is ourselves.
    pub target: Option<PeerId>,
    pub path: PathBuf,
    /// Caption markup, parsed like the composer's.
    pub caption: String,
    pub audience: StoryAudience,
    pub allow: Vec<PeerId>,
    /// Screenshots and forwarding forbidden.
    pub noforwards: bool,
    /// Seconds it stays in the strip.
    pub period: i32,
    /// Also kept on the profile after it expires.
    pub pinned: bool,
    /// The upload's progress id.
    pub upload: u64,
}

/// Story videos as the picker offers them.
pub fn is_story_video(path: &std::path::Path) -> bool {
    let ext = path.extension().map(|e| e.to_string_lossy().to_lowercase()).unwrap_or_default();
    matches!(ext.as_str(), "mp4" | "mov" | "m4v" | "webm" | "mkv")
}

/// Length in seconds and frame size of a video, read from `ffmpeg -i`: the server wants them for a
/// story and grammers doesn't fill them in.
pub fn probe_video(path: &std::path::Path) -> Option<(f64, i32, i32)> {
    let out = crate::platform::tool("ffmpeg").arg("-hide_banner").arg("-i").arg(path).stdout(std::process::Stdio::null()).stderr(std::process::Stdio::piped()).output().ok()?;
    parse_probe(&String::from_utf8_lossy(&out.stderr))
}

fn parse_probe(text: &str) -> Option<(f64, i32, i32)> {
    let dur = text.split("Duration: ").nth(1)?.split(',').next()?.trim();
    let mut parts = dur.split(':').map(|p| p.parse::<f64>().ok());
    let secs = parts.next()?? * 3600.0 + parts.next()?? * 60.0 + parts.next()??;
    let line = text.lines().find(|l| l.contains("Video:"))?;
    let (w, h) = line
        .split([' ', ','])
        .filter_map(|t| t.split_once('x'))
        .find_map(|(a, b)| Some((a.parse::<i32>().ok()?, b.parse::<i32>().ok()?)).filter(|(w, h)| *w > 0 && *h > 0))?;
    Some((secs, w, h))
}

/// Telegram's refusals in words the user can act on.
fn post_error(e: &InvocationError) -> String {
    if e.is("STORIES_TOO_MUCH") {
        l!("Забагато історій: дочекайтеся, поки якісь зникнуть, або оформіть Telegram Premium", "Too many stories: wait until some expire, or get Telegram Premium").into()
    } else if e.is("PREMIUM_ACCOUNT_REQUIRED") || e.is("STORY_PERIOD_INVALID") {
        l!("Для цього потрібен Telegram Premium", "This needs Telegram Premium").into()
    } else if e.is("BOOSTS_REQUIRED") {
        l!("Каналу потрібно більше бустів, щоб публікувати історії", "The channel needs more boosts to post stories").into()
    } else if e.is("CHAT_ADMIN_REQUIRED") {
        l!("Потрібні права адміністратора", "Admin rights are needed").into()
    } else {
        lf!("Історію не опубліковано: {e}", "The story was not posted: {e}")
    }
}

/// The new story in the answer to `stories.sendStory`, with its owner.
fn posted(updates: &tl::enums::Updates) -> Option<(PeerId, tl::types::StoryItem)> {
    let list = match updates {
        tl::enums::Updates::Updates(u) => &u.updates,
        tl::enums::Updates::Combined(u) => &u.updates,
        _ => return None,
    };
    list.iter().find_map(|u| match u {
        tl::enums::Update::Story(s) => match &s.story {
            tl::enums::StoryItem::Item(x) => Some((PeerId::from(&s.peer), (**x).clone())),
            _ => None,
        },
        _ => None,
    })
}

impl Worker {
    pub(super) async fn story_post_cmd(&mut self, cmd: Cmd) -> Result<(), String> {
        let client = self.client()?;
        match cmd {
            Cmd::LoadStoryTargets => {
                let res = client.invoke(&tl::functions::stories::GetChatsToSend {}).await.map_err(|e| lf!("Історії: {e}", "Stories: {e}"))?;
                let chats = match res {
                    tl::enums::messages::Chats::Chats(c) => c.chats,
                    tl::enums::messages::Chats::Slice(c) => c.chats,
                };
                let mut list = Vec::new();
                for peer in peers_by_id(&client, Vec::new(), chats).into_values() {
                    if self.remember(&peer).await {
                        list.push(found_chat(&peer, String::new()));
                    }
                }
                list.sort_by_key(|c| c.title.to_lowercase());
                self.out.send(Ev::StoryTargets(list));
            }
            Cmd::CanPostStory(target) => {
                let peer = match target {
                    Some(p) => self.peer_ref(p)?.into(),
                    None => tl::enums::InputPeer::PeerSelf,
                };
                let res = client.invoke(&tl::functions::stories::CanSendStory { peer }).await;
                let result = match res {
                    Ok(tl::enums::stories::CanSendStoryCount::Count(c)) => Ok(c.count_remains),
                    Err(e) => Err(post_error(&e)),
                };
                self.out.send(Ev::StoryCanPost { target, result });
            }
            Cmd::PostStory(d) => {
                let peer: tl::enums::InputPeer = match d.target {
                    Some(p) => self.peer_ref(p)?.into(),
                    None => tl::enums::InputPeer::PeerSelf,
                };
                let allow = d
                    .allow
                    .iter()
                    .filter(|p| p.kind() == grammers_client::session::types::PeerKind::User)
                    .map(|p| self.peer_ref(*p).map(tl::enums::InputUser::from))
                    .collect::<Result<Vec<_>, _>>()?;
                let (out, loader) = (self.out.clone(), self.loader.clone());
                // Uploading a video takes a while: other commands go on meanwhile.
                tokio::spawn(async move {
                    let res = post(&client, peer, d.clone(), allow, &out).await;
                    out.send(Ev::UploadDone(d.upload));
                    out.send(match res {
                        Ok(Some((owner, item))) => Ev::StoryPosted { peer: Some(owner), story: Some(story_of(loader.as_ref(), owner, &item)) },
                        Ok(None) => Ev::StoryPosted { peer: d.target, story: None },
                        Err(e) => Ev::StoryPostFailed(e),
                    });
                });
            }
            _ => {}
        }
        Ok(())
    }
}

async fn post(client: &Client, peer: tl::enums::InputPeer, d: StoryDraft, allow: Vec<tl::enums::InputUser>, out: &Emitter) -> Result<Option<(PeerId, tl::types::StoryItem)>, String> {
    let video = is_story_video(&d.path);
    let media = if video {
        let src = d.path.clone();
        let (secs, w, h) = tokio::task::spawn_blocking(move || probe_video(&src)).await.ok().flatten().ok_or(l!("Не вдалося прочитати відео (потрібен ffmpeg)", "Could not read the video (ffmpeg is needed)"))?;
        let (up, _) = upload(client, &d.path, d.upload, 0, 0, out).await?;
        let name = d.path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_else(|| "story.mp4".into());
        let attr = tl::types::DocumentAttributeVideo { round_message: false, supports_streaming: true, nosound: false, duration: secs, w, h, preload_prefix_size: None, video_start_ts: None, video_codec: None };
        reply::document(up, &name, None, vec![attr.into()])
    } else {
        let src = d.path.clone();
        let path = tokio::task::spawn_blocking(move || compress_photo(&src)).await.ok().flatten();
        let (up, _) = upload(client, path.as_deref().unwrap_or(&d.path), d.upload, 0, 0, out).await?;
        if let Some(p) = path {
            let _ = tokio::fs::remove_file(p).await;
        }
        reply::photo(up, false)
    };
    let privacy_rules: Vec<tl::enums::InputPrivacyRule> = match (d.target, d.audience) {
        (Some(_), _) | (None, StoryAudience::Everyone) => vec![tl::types::InputPrivacyValueAllowAll {}.into()],
        (None, StoryAudience::Contacts) => vec![tl::types::InputPrivacyValueAllowContacts {}.into()],
        (None, StoryAudience::CloseFriends) => vec![tl::types::InputPrivacyValueAllowCloseFriends {}.into()],
        (None, StoryAudience::Selected) => vec![tl::types::InputPrivacyValueAllowUsers { users: allow }.into()],
    };
    let (caption, ents) = format::parse(d.caption.trim());
    let req = tl::functions::stories::SendStory {
        pinned: d.pinned,
        noforwards: d.noforwards,
        fwd_modified: false,
        peer,
        media,
        media_areas: None,
        caption: (!caption.is_empty()).then_some(caption),
        entities: (!ents.is_empty()).then_some(ents),
        privacy_rules,
        random_id: jiff::Timestamp::now().as_nanosecond() as i64,
        // A day is the default; the server leaves the field out then.
        period: (d.period != 86_400).then_some(d.period),
        fwd_from_id: None,
        fwd_from_story: None,
        albums: None,
        music: None,
    };
    let updates = client.invoke(&req).await.map_err(|e| post_error(&e))?;
    Ok(posted(&updates))
}

#[cfg(test)]
mod tests {
    use super::parse_probe;

    #[test]
    fn reads_ffmpeg_output() {
        let text = "Input #0, mov,mp4,m4a,3gp,3g2,mj2, from 'a.mp4':\n  Duration: 00:00:12.48, start: 0.000000, bitrate: 2000 kb/s\n  Stream #0:0[0x1](und): Video: h264 (High) (avc1 / 0x31637661), yuv420p(tv, bt709, progressive), 720x1280 [SAR 1:1 DAR 9:16], 30 fps\n";
        let (secs, w, h) = parse_probe(text).unwrap_or_default();
        assert!((secs - 12.48).abs() < 0.01);
        assert_eq!((w, h), (720, 1280));
        assert!(parse_probe("garbage").is_none());
    }
}
