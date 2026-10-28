//! Pictures on the GPU with a memory budget: the ones not drawn for a while are dropped when the
//! budget is exceeded and decoded again from the disk cache when they come back into view.

use std::cell::{Cell, RefCell};

use super::*;

/// GPU memory for pictures; above it the least recently drawn ones go.
const BUDGET: usize = 192 << 20;
/// Dropping stops at this share of the budget, so it doesn't run every frame.
const LOW_WATER: usize = BUDGET / 4 * 3;
/// A picture drawn this recently stays even over the budget.
const KEEP_SECS: f64 = 30.0;
/// How often the budget is checked.
const CHECK_SECS: f64 = 5.0;

#[derive(Default)]
pub(super) struct Textures {
    map: HashMap<ImgKey, TextureHandle>,
    /// When each picture was last drawn.
    used: RefCell<HashMap<ImgKey, f64>>,
    /// Dropped pictures; asking for one again brings it back.
    dropped: HashSet<ImgKey>,
    /// Dropped pictures asked for this frame.
    wanted: RefCell<HashSet<ImgKey>>,
    now: Cell<f64>,
    checked_at: f64,
}

fn bytes(t: &TextureHandle) -> usize {
    let [w, h] = t.size();
    w * h * 4
}

impl Textures {
    /// The picture to draw, noting that it is in use.
    pub(super) fn get(&self, key: &ImgKey) -> Option<&TextureHandle> {
        match self.map.get(key) {
            Some(t) => {
                self.used.borrow_mut().insert(*key, self.now.get());
                Some(t)
            }
            None => {
                if self.dropped.contains(key) {
                    self.wanted.borrow_mut().insert(*key);
                }
                None
            }
        }
    }

    pub(super) fn contains_key(&self, key: &ImgKey) -> bool {
        self.map.contains_key(key)
    }

    pub(super) fn insert(&mut self, key: ImgKey, tex: TextureHandle) {
        self.dropped.remove(&key);
        self.used.borrow_mut().insert(key, self.now.get());
        self.map.insert(key, tex);
    }

    /// A picture that is no longer right (its message's media was replaced).
    pub(super) fn remove(&mut self, key: &ImgKey) {
        self.map.remove(key);
        self.dropped.remove(key);
        self.used.borrow_mut().remove(key);
    }

    pub(super) fn clear(&mut self) {
        *self = Self::default();
    }

    /// Once a frame: keeps the budget and returns the dropped pictures that are wanted again.
    pub(super) fn frame(&mut self, now: f64) -> Vec<ImgKey> {
        self.now.set(now);
        if now - self.checked_at > CHECK_SECS {
            self.checked_at = now;
            self.trim(now);
        }
        let wanted: Vec<ImgKey> = self.wanted.borrow_mut().drain().collect();
        // Asked once: until it arrives, don't ask every frame.
        for key in &wanted {
            self.dropped.remove(key);
        }
        wanted
    }

    fn trim(&mut self, now: f64) {
        let mut total: usize = self.map.values().map(bytes).sum();
        if total <= BUDGET {
            return;
        }
        let used = self.used.borrow();
        let mut old: Vec<(f64, ImgKey)> = self.map.keys().map(|k| (used.get(k).copied().unwrap_or(0.0), *k)).filter(|(t, _)| now - t > KEEP_SECS).collect();
        drop(used);
        old.sort_by(|a, b| a.0.total_cmp(&b.0));
        for (_, key) in old {
            if total <= LOW_WATER {
                break;
            }
            if let Some(t) = self.map.remove(&key) {
                total -= bytes(&t);
                self.dropped.insert(key);
                self.used.borrow_mut().remove(&key);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use grammers_client::session::types::PeerId;

    fn tex(ctx: &egui::Context, side: usize) -> TextureHandle {
        ctx.load_texture("t", egui::ColorImage::filled([side, side], Color32::WHITE), TextureOptions::LINEAR)
    }

    #[test]
    fn drops_old_pictures_over_budget_and_wants_them_back() {
        let ctx = egui::Context::default();
        let mut t = Textures::default();
        let side = 2048; // 16 MiB each
        let keys: Vec<ImgKey> = (0..16).map(|i| ImgKey::Photo(PeerId::user_unchecked(1), i)).collect();
        // Each one arrives a second later, so the oldest is well defined.
        for (i, k) in keys.iter().enumerate() {
            t.frame(i as f64);
            t.insert(*k, tex(&ctx, side));
        }
        // The last one is drawn recently and must stay.
        t.frame(100.0);
        assert!(t.get(&keys[15]).is_some());
        t.frame(200.0);
        let left: usize = t.map.values().map(bytes).sum();
        assert!(left <= LOW_WATER, "{left}");
        assert!(t.contains_key(&keys[15]));
        assert!(!t.contains_key(&keys[0]));
        // Asking for a dropped one reports it once.
        assert!(t.get(&keys[0]).is_none());
        assert_eq!(t.frame(201.0), vec![keys[0]]);
        assert!(t.frame(202.0).is_empty());
    }
}
