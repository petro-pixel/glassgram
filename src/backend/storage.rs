//! Local storage, as in Telegram Desktop's "Manage local storage": what the picture cache
//! (`cache/`) and the downloaded media (`media/`) take by type, clearing a type, and keeping
//! the whole under the size limit from the settings (oldest files go first).

use super::*;

/// What a cached file is, judged by its name (see the loader and `cache_media`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum CacheKind {
    Photos,
    Videos,
    Audio,
    Stickers,
    /// Profile photos and stories.
    Profiles,
    Other,
}

impl CacheKind {
    pub const ALL: [CacheKind; 6] = [CacheKind::Photos, CacheKind::Videos, CacheKind::Audio, CacheKind::Stickers, CacheKind::Profiles, CacheKind::Other];

    /// `media`: the file is in `media/` (named `<chat>_<id>.<ext>`), else in `cache/`.
    fn of(name: &str, media: bool) -> Self {
        let ext = name.rsplit_once('.').map_or("", |(_, e)| e);
        if media {
            return match ext {
                _ if name.starts_with("av") || name.starts_with('s') => CacheKind::Profiles,
                "mp4" | "gif" => CacheKind::Videos,
                "ogg" | "mp3" | "m4a" => CacheKind::Audio,
                "webm" | "tgs" => CacheKind::Stickers,
                _ => CacheKind::Other,
            };
        }
        match name.as_bytes() {
            [b's', b't', ..] => CacheKind::Stickers,
            [b'p' | b'f' | b'w', ..] if ext == "jpg" => CacheKind::Photos,
            _ if name.starts_with("send-") => CacheKind::Photos,
            [b'v', ..] => CacheKind::Videos,
            [b'a' | b's', ..] => CacheKind::Profiles,
            _ => CacheKind::Other,
        }
    }
}

/// Every cached file: (kind, path, size, last modified).
fn files() -> Vec<(CacheKind, PathBuf, u64, std::time::SystemTime)> {
    let mut out = Vec::new();
    for (dir, media) in [("cache", false), ("media", true)] {
        let Ok(list) = std::fs::read_dir(data_dir().join(dir)) else { continue };
        for e in list.flatten() {
            let Ok(meta) = e.metadata() else { continue };
            // Half-downloaded files are still being written.
            let name = e.file_name().to_string_lossy().into_owned();
            if !meta.is_file() || name.ends_with(".part") {
                continue;
            }
            out.push((CacheKind::of(&name, media), e.path(), meta.len(), meta.modified().unwrap_or(std::time::UNIX_EPOCH)));
        }
    }
    out
}

fn usage() -> Vec<(CacheKind, u64, usize)> {
    let all = files();
    CacheKind::ALL
        .iter()
        .map(|k| {
            let of_kind = all.iter().filter(|f| f.0 == *k);
            (*k, of_kind.clone().map(|f| f.2).sum(), of_kind.count())
        })
        .collect()
}

/// Removes the oldest files until the cache fits in `limit` bytes; returns how many went.
pub(super) fn trim(limit: u64) -> usize {
    let mut all = files();
    let mut total: u64 = all.iter().map(|f| f.2).sum();
    if total <= limit {
        return 0;
    }
    all.sort_by_key(|f| f.3);
    let mut removed = 0;
    for (_, path, size, _) in all {
        if total <= limit {
            break;
        }
        if std::fs::remove_file(&path).is_ok() {
            total -= size;
            removed += 1;
        }
    }
    removed
}

impl Worker {
    pub(super) async fn storage_cmd(&mut self, cmd: Cmd) -> Result<(), String> {
        let (kinds, limit) = match cmd {
            Cmd::LoadCacheUsage => (Vec::new(), None),
            Cmd::ClearCache(kinds) => (kinds, None),
            Cmd::TrimCache(limit) => (Vec::new(), Some(limit)),
            _ => return Ok(()),
        };
        let cleared = !kinds.is_empty() || limit.is_some();
        let out = self.out.clone();
        let loader = self.loader.clone();
        tokio::task::spawn_blocking(move || {
            if let Some(limit) = limit {
                trim(limit);
            }
            for (kind, path, _, _) in files() {
                if kinds.contains(&kind) {
                    let _ = std::fs::remove_file(path);
                }
            }
            // Files asked for once aren't asked for again: forget them so they download anew.
            if cleared && let Some(l) = loader {
                l.requested.lock().unwrap().clear();
                l.files.lock().unwrap().clear();
            }
            out.send(Ev::CacheUsage(usage()));
        });
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn kinds_by_file_name() {
        assert_eq!(CacheKind::of("p-100123_45.jpg", false), CacheKind::Photos);
        assert_eq!(CacheKind::of("v77_3.jpg", false), CacheKind::Videos);
        assert_eq!(CacheKind::of("st123.img", false), CacheKind::Stickers);
        assert_eq!(CacheKind::of("s77_3.jpg", false), CacheKind::Profiles);
        assert_eq!(CacheKind::of("a77.jpg", false), CacheKind::Profiles);
        assert_eq!(CacheKind::of("77_3.ogg", true), CacheKind::Audio);
        assert_eq!(CacheKind::of("-10077_3.mp4", true), CacheKind::Videos);
        assert_eq!(CacheKind::of("av77_9.mp4", true), CacheKind::Profiles);
        assert_eq!(CacheKind::of("77_3.webm", true), CacheKind::Stickers);
    }
}
