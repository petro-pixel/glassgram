//! Notification sounds: the account's saved ringtones (`account.getSavedRingtones`, uploading and
//! removing them) and reaction notification settings (`account.get/setReactionsNotifySettings`),
//! as in Telegram Desktop's Settings → Notifications. A chat's own sound travels in `NotifyState`.

use super::*;
use super::loader::DocLoc;

/// The server refuses ringtones over 300 KB (`ringtone_size_max` in the app config).
pub const RINGTONE_MAX_BYTES: u64 = 300 * 1024;

/// Audio files the server converts into ringtones.
pub const RINGTONE_EXTS: [&str; 5] = ["mp3", "ogg", "oga", "opus", "m4a"];

/// What a notification plays.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Sound {
    #[default]
    Default,
    None,
    /// A saved ringtone by document id.
    Ringtone(i64),
}

impl Sound {
    pub(super) fn from_raw(raw: &tl::enums::NotificationSound) -> Self {
        match raw {
            tl::enums::NotificationSound::None => Sound::None,
            tl::enums::NotificationSound::Ringtone(r) => Sound::Ringtone(r.id),
            // A local sound is a file name on another device: here it is the default one.
            tl::enums::NotificationSound::Default | tl::enums::NotificationSound::Local(_) => Sound::Default,
        }
    }

    pub(super) fn to_raw(self) -> tl::enums::NotificationSound {
        match self {
            Sound::Default => tl::enums::NotificationSound::Default,
            Sound::None => tl::enums::NotificationSound::None,
            Sound::Ringtone(id) => tl::types::NotificationSoundRingtone { id }.into(),
        }
    }
}

/// A ringtone saved to the account.
#[derive(Debug, Clone)]
pub struct Ringtone {
    pub id: i64,
    pub title: String,
    /// Seconds.
    pub duration: f64,
}

/// Whose reactions notify.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ReactFrom {
    Nobody,
    Contacts,
    #[default]
    All,
}

impl ReactFrom {
    fn from_raw(raw: &Option<tl::enums::ReactionNotificationsFrom>) -> Self {
        match raw {
            None => ReactFrom::Nobody,
            Some(tl::enums::ReactionNotificationsFrom::Contacts) => ReactFrom::Contacts,
            Some(tl::enums::ReactionNotificationsFrom::All) => ReactFrom::All,
        }
    }

    fn to_raw(self) -> Option<tl::enums::ReactionNotificationsFrom> {
        match self {
            ReactFrom::Nobody => None,
            ReactFrom::Contacts => Some(tl::enums::ReactionNotificationsFrom::Contacts),
            ReactFrom::All => Some(tl::enums::ReactionNotificationsFrom::All),
        }
    }
}

/// Reaction notification settings, shared by every device of the account.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct ReactionNotify {
    pub messages: ReactFrom,
    pub stories: ReactFrom,
    pub polls: ReactFrom,
    pub previews: bool,
    pub sound: Sound,
}

impl ReactionNotify {
    fn from_raw(raw: &tl::enums::ReactionsNotifySettings) -> Self {
        let tl::enums::ReactionsNotifySettings::Settings(s) = raw;
        Self {
            messages: ReactFrom::from_raw(&s.messages_notify_from),
            stories: ReactFrom::from_raw(&s.stories_notify_from),
            polls: ReactFrom::from_raw(&s.poll_votes_notify_from),
            previews: s.show_previews,
            sound: Sound::from_raw(&s.sound),
        }
    }

    fn to_raw(self) -> tl::enums::ReactionsNotifySettings {
        tl::types::ReactionsNotifySettings {
            messages_notify_from: self.messages.to_raw(),
            stories_notify_from: self.stories.to_raw(),
            poll_votes_notify_from: self.polls.to_raw(),
            sound: self.sound.to_raw(),
            show_previews: self.previews,
        }
        .into()
    }
}

/// Where a ringtone's file is cached; `ffplay` tells the format from the content.
fn ringtone_path(id: i64) -> PathBuf {
    data_dir().join("media").join(format!("ringtone_{id}.audio"))
}

fn ringtone(doc: &tl::types::Document) -> Ringtone {
    let mut title = String::new();
    let mut duration = 0.0;
    for a in &doc.attributes {
        match a {
            tl::enums::DocumentAttribute::Audio(x) => {
                duration = x.duration as f64;
                if let Some(t) = x.title.as_ref().filter(|t| !t.is_empty()) {
                    title = t.clone();
                }
            }
            tl::enums::DocumentAttribute::Filename(f) if title.is_empty() => {
                title = f.file_name.rsplit_once('.').map_or(f.file_name.as_str(), |(stem, _)| stem).to_owned();
            }
            _ => {}
        }
    }
    Ringtone { id: doc.id, title, duration }
}

async fn saved(client: &Client) -> Result<Vec<tl::types::Document>, String> {
    let res = client.invoke(&tl::functions::account::GetSavedRingtones { hash: 0 }).await.map_err(|e| lf!("Звуки: {e}", "Sounds: {e}"))?;
    Ok(match res {
        tl::enums::account::SavedRingtones::Ringtones(r) => r
            .ringtones
            .into_iter()
            .filter_map(|d| match d {
                tl::enums::Document::Document(d) => Some(d),
                tl::enums::Document::Empty(_) => None,
            })
            .collect(),
        tl::enums::account::SavedRingtones::NotModified => Vec::new(),
    })
}

fn mime(path: &std::path::Path) -> &'static str {
    match path.extension().and_then(|e| e.to_str()).map(str::to_lowercase).as_deref() {
        Some("mp3") => "audio/mpeg",
        Some("m4a") => "audio/mp4",
        _ => "audio/ogg",
    }
}

impl Worker {
    pub(super) async fn sounds_cmd(&mut self, cmd: Cmd) -> Result<(), String> {
        let client = self.client()?;
        let (out, back) = (self.out.clone(), self.back.clone());
        match cmd {
            Cmd::LoadRingtones => {
                tokio::spawn(async move {
                    let docs = match saved(&client).await {
                        Ok(d) => d,
                        Err(e) => return out.send(Ev::Error(e)),
                    };
                    out.send(Ev::Ringtones(docs.iter().map(ringtone).collect()));
                    let _ = tokio::fs::create_dir_all(data_dir().join("media")).await;
                    // Small files: fetched one by one, so a chat's sound is ready before it plays.
                    for doc in docs {
                        let (id, path) = (doc.id, ringtone_path(doc.id));
                        if tokio::fs::metadata(&path).await.is_err() {
                            let part = path.with_extension("part");
                            if let Err(e) = client.download_media(&DocLoc { doc, thumb: String::new() }, &part).await {
                                log_error(&format!("ringtone: {e}"));
                                continue;
                            }
                            if tokio::fs::rename(&part, &path).await.is_err() {
                                continue;
                            }
                        }
                        out.send(Ev::RingtoneFile { id, path });
                    }
                });
            }
            Cmd::UploadRingtone(path) => {
                tokio::spawn(async move {
                    let res = async {
                        let size = tokio::fs::metadata(&path).await.map_err(|e| e.to_string())?.len();
                        if size > RINGTONE_MAX_BYTES {
                            return Err(l!("Звук має бути не більшим за 300 КБ", "The sound must be 300 KB or smaller").to_owned());
                        }
                        let up = client.upload_file(&path).await.map_err(|e| e.to_string())?;
                        let file_name = path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
                        let req = tl::functions::account::UploadRingtone { file: up.raw, file_name, mime_type: mime(&path).to_owned() };
                        client.invoke(&req).await.map_err(|e| e.to_string())?;
                        Ok::<_, String>(())
                    }
                    .await;
                    match res {
                        Ok(()) => {
                            out.send(Ev::Notice(l!("Звук додано", "Sound added").into()));
                            let _ = back.send(Cmd::LoadRingtones);
                        }
                        Err(e) => {
                            out.send(Ev::RingtoneUploadFailed);
                            out.send(Ev::Error(lf!("Не вдалося додати звук: {e}", "Could not add the sound: {e}")));
                        }
                    }
                });
            }
            Cmd::DeleteRingtone(id) => {
                let doc = saved(&client).await?.into_iter().find(|d| d.id == id).ok_or(l!("Звук уже видалено", "The sound is already gone"))?;
                let input = tl::types::InputDocument { id: doc.id, access_hash: doc.access_hash, file_reference: doc.file_reference };
                client
                    .invoke(&tl::functions::account::SaveRingtone { id: input.into(), unsave: true })
                    .await
                    .map_err(|e| lf!("Не вдалося видалити звук: {e}", "Could not delete the sound: {e}"))?;
                let _ = std::fs::remove_file(ringtone_path(id));
                let _ = back.send(Cmd::LoadRingtones);
            }
            Cmd::LoadReactionNotify => {
                let raw = client.invoke(&tl::functions::account::GetReactionsNotifySettings {}).await.map_err(|e| lf!("Реакції: {e}", "Reactions: {e}"))?;
                out.send(Ev::ReactionNotify(ReactionNotify::from_raw(&raw)));
            }
            Cmd::SetReactionNotify(s) => {
                let raw = client
                    .invoke(&tl::functions::account::SetReactionsNotifySettings { settings: s.to_raw() })
                    .await
                    .map_err(|e| lf!("Не вдалося змінити сповіщення: {e}", "Could not change notifications: {e}"))?;
                out.send(Ev::ReactionNotify(ReactionNotify::from_raw(&raw)));
            }
            _ => {}
        }
        Ok(())
    }
}
