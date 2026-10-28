//! Editing a message with media, as in Telegram Desktop: its caption, and the photo or file
//! replaced from the "Editing" bar (or by dropping a file on the chat while editing).

use std::sync::{Arc, Mutex};

use super::*;

#[derive(Default)]
pub(super) struct EditMedia {
    /// The file that replaces the media of the message being edited, and whether it goes uncompressed.
    replace: Option<(PathBuf, bool)>,
    /// What the system picker returned, taken on the next frame.
    picked: Arc<Mutex<Option<(PathBuf, bool)>>>,
}

/// Media whose caption can be edited and which can be replaced; voice and round video
/// messages, stickers, polls and the like can't.
pub(super) fn editable_media(m: &Msg) -> bool {
    m.photo || m.file.is_some() || m.media.as_ref().is_some_and(|i| matches!(i.kind, MediaKind::Video | MediaKind::Gif | MediaKind::Audio))
}

impl App {
    /// The message being edited has media.
    pub(super) fn editing_media(&self) -> Option<&Msg> {
        let (Compose::Edit(id), Some(peer)) = (self.compose, self.selected) else { return None };
        self.find(peer, id).filter(|m| editable_media(m))
    }

    /// Files dropped or pasted while editing a media message replace it instead of being sent.
    pub(super) fn take_replacement(&mut self, peer: PeerId, paths: &[PathBuf], as_file: bool) -> bool {
        if self.selected != Some(peer) || paths.len() != 1 || self.editing_media().is_none() {
            return false;
        }
        self.edit_media.replace = Some((paths[0].clone(), as_file));
        true
    }

    pub(super) fn clear_replacement(&mut self) {
        self.edit_media.replace = None;
    }

    /// The system file picker on a thread; the choice is taken in `replace_button`.
    fn pick_replacement(&self, as_file: bool) {
        let (picked, ctx) = (self.edit_media.picked.clone(), self.ctx.clone());
        std::thread::spawn(move || {
            if let Some(path) = rfd::FileDialog::new().set_title(l!("Замінити файл", "Replace file")).pick_file() {
                *picked.lock().unwrap() = Some((path, as_file));
            }
            ctx.request_repaint();
        });
    }

    /// The "Replace" button in the "Editing" bar; the second line names the new file.
    /// Returns that line when a replacement is chosen.
    pub(super) fn replace_button(&mut self, ui: &mut Ui, r: Rect) -> Option<String> {
        if let Some(p) = self.edit_media.picked.lock().unwrap().take() {
            self.edit_media.replace = Some(p);
        }
        let resp = glass::icon_button(ui, r, "edit-replace", Icon::Reload, self.edit_media.replace.is_some(), true)
            .on_hover_text(l!("Замінити файл (правий клік — без стиснення)", "Replace file (right click — without compression)"));
        if resp.clicked() {
            self.pick_replacement(false);
        } else if resp.secondary_clicked() {
            self.pick_replacement(true);
        }
        let (path, _) = self.edit_media.replace.as_ref()?;
        let name = path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
        Some(lf!("Заміна: {name}", "Replace with: {name}"))
    }

    /// Sends an edit of a media message: the caption alone, or with the new file.
    pub(super) fn send_media_edit(&mut self, peer: PeerId, id: i32, text: String) {
        let Some((path, as_file)) = self.edit_media.replace.take() else {
            return self.send(Cmd::Edit { peer, id, text });
        };
        let (spoiler, above) = self.find(peer, id).map_or((false, false), |m| (m.spoiler, m.invert_media));
        self.send(Cmd::EditMedia { peer, id, text, path, as_file, spoiler, above });
    }

    /// The new media arrived: forget what was drawn or played for the old one.
    pub(super) fn on_media_replaced(&mut self, m: Msg) {
        let key = (m.peer, m.id);
        self.textures.remove(&ImgKey::Photo(m.peer, m.id));
        self.textures.remove(&ImgKey::Full(m.peer, m.id));
        self.spoilers.forget(key);
        self.playback.files.remove(&key);
        self.playback.clips.remove(&key);
        if let Some(x) = self.history.get_mut(&m.peer).and_then(|l| l.iter_mut().find(|x| x.id == m.id)) {
            *x = m;
        }
    }
}
