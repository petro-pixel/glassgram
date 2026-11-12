//! The "Send files" box, as in Telegram Desktop: what is attached (photo previews, file rows),
//! a caption, compression, a spoiler over photos and videos and the caption above them.
//! Picked, pasted and dropped files all come here; more files dropped on it join it.
//! Dropped folders, or several files on request, go as one zip archive (Telegram Desktop 7.2.6).

use super::*;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use eframe::egui::ColorImage;
use crate::backend::is_image;
use crate::zip;
use super::photoedit::{Edit, Outcome, PhotoEditor};

/// Long side of a preview in the box.
const THUMB_PX: u32 = 360;
const TILE_H: f32 = 150.0;
const ROW_H: f32 = 46.0;

pub(super) struct SendBox {
    peer: PeerId,
    paths: Vec<PathBuf>,
    caption: String,
    /// Photos go as photos (compressed); off — every file as a document.
    compress: bool,
    spoiler: bool,
    above: bool,
    /// A private chat with someone else: a single photo or video may go "view once".
    private: bool,
    once: bool,
    /// Previews by index in `paths`, decoded on a thread, and their blurred copies for the spoiler.
    thumbs: HashMap<usize, (TextureHandle, TextureHandle)>,
    tx: mpsc::Sender<(PathBuf, ColorImage)>,
    rx: mpsc::Receiver<(PathBuf, ColorImage)>,
    focused: bool,
    /// Everything goes as one archive. Folders are packed either way, each on its own when off.
    archive: bool,
    /// What each dropped folder holds, counted when it joins the box.
    folders: HashMap<PathBuf, zip::Summary>,
    /// Packing after "Send": the box stays open with a spinner until the archives are ready.
    packing: Option<Packing>,
    /// The photo editor over the box, and what each edited photo was made from (to edit it again).
    editor: Option<PhotoEditor>,
    edits: HashMap<PathBuf, (PathBuf, Edit)>,
    /// Edited photos: temporary files, deleted once queued or cancelled.
    temp: Vec<PathBuf>,
}

/// Archives being packed on a thread. Dropping it (the box closed) cancels the work.
struct Packing {
    cancel: Arc<AtomicBool>,
    rx: mpsc::Receiver<Result<Packed, String>>,
}

impl Drop for Packing {
    fn drop(&mut self) {
        self.cancel.store(true, Ordering::Relaxed);
    }
}

/// The files to send once packing is done, and the temporary folders to delete after queueing.
struct Packed {
    paths: Vec<PathBuf>,
    temp: Vec<PathBuf>,
}

/// Packs folders (or everything, with `all`) into zips in fresh temporary folders, so the archive
/// keeps a clean name: `Photos.zip`, not a temp-file stamp.
fn pack(paths: Vec<PathBuf>, all: bool, cancel: &AtomicBool) -> Result<Packed, String> {
    let mut out = Packed { paths: Vec::new(), temp: Vec::new() };
    let groups: Vec<Vec<PathBuf>> = if all { vec![paths] } else { paths.into_iter().map(|p| vec![p]).collect() };
    for group in groups {
        if group.len() == 1 && !group[0].is_dir() {
            out.paths.extend(group);
            continue;
        }
        let name = match &group[..] {
            [one] => one.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default(),
            _ => l!("Архів", "Archive").to_owned(),
        };
        let dir = crate::platform::temp_file("zip");
        let zipped = std::fs::create_dir(&dir).and_then(|_| {
            let file = dir.join(format!("{}.zip", crate::platform::safe_file_name(&name)));
            zip::write(&group, &file, cancel).map(|_| file)
        });
        out.temp.push(dir);
        match zipped {
            Ok(file) => out.paths.push(file),
            Err(e) => {
                out.cleanup();
                return Err(if e.kind() == std::io::ErrorKind::Interrupted { String::new() } else { e.to_string() });
            }
        }
    }
    Ok(out)
}

impl Packed {
    fn cleanup(&self) {
        for dir in &self.temp {
            let _ = std::fs::remove_dir_all(dir);
        }
    }
}

fn is_video(p: &std::path::Path) -> bool {
    let ext = p.extension().map(|e| e.to_string_lossy().to_lowercase()).unwrap_or_default();
    matches!(ext.as_str(), "mp4" | "mov" | "m4v" | "webm" | "mkv")
}

impl SendBox {
    /// Decodes previews of new pictures off the UI thread.
    fn add(&mut self, ctx: &egui::Context, paths: Vec<PathBuf>) {
        let images: Vec<PathBuf> = paths.iter().filter(|p| is_image(p)).cloned().collect();
        for dir in paths.iter().filter(|p| p.is_dir()) {
            self.folders.insert(dir.clone(), zip::summary(std::slice::from_ref(dir)));
        }
        self.paths.extend(paths);
        self.decode(ctx, images);
    }

    fn decode(&self, ctx: &egui::Context, images: Vec<PathBuf>) {
        let (tx, ctx) = (self.tx.clone(), ctx.clone());
        std::thread::spawn(move || {
            for path in images {
                let Ok(img) = image::open(&path) else { continue };
                let rgba = img.thumbnail(THUMB_PX, THUMB_PX).to_rgba8();
                let size = [rgba.width() as usize, rgba.height() as usize];
                if tx.send((path, ColorImage::from_rgba_unmultiplied(size, rgba.as_raw()))).is_err() {
                    return;
                }
                ctx.request_repaint();
            }
        });
    }

    /// Files shown as photo tiles: pictures, when they go compressed.
    fn tiles(&self) -> Vec<usize> {
        (0..self.paths.len()).filter(|&i| self.compress && !self.archive && is_image(&self.paths[i])).collect()
    }

    /// There is a photo or a video the spoiler and the caption position apply to.
    fn has_media(&self) -> bool {
        self.compress && !self.archive && self.paths.iter().any(|p| is_image(p) || is_video(p))
    }

    /// "View once" applies: one photo or video for one person, as in Telegram Desktop.
    fn can_view_once(&self) -> bool {
        self.private && self.paths.len() == 1 && self.has_media()
    }

    /// The "one archive" choice: for two or more items. A single folder is packed anyway.
    fn can_archive(&self) -> bool {
        self.paths.len() > 1
    }

    fn needs_packing(&self) -> bool {
        (self.archive && self.can_archive()) || self.paths.iter().any(|p| p.is_dir())
    }

    /// How many option checkboxes the box shows, for its height.
    fn options(&self) -> usize {
        let compress = !self.archive && self.paths.iter().any(|p| is_image(p));
        self.can_archive() as usize + compress as usize + if self.has_media() { 2 } else { 0 } + self.can_view_once() as usize
    }

    fn title(&self) -> String {
        let n = self.paths.len();
        if (self.archive && self.can_archive()) || (n == 1 && self.paths[0].is_dir()) {
            return l!("Надіслати архів", "Send an archive").to_owned();
        }
        let photos = self.compress && self.paths.iter().all(|p| is_image(p));
        match (n, photos) {
            (1, true) => l!("Надіслати фото", "Send a photo").to_owned(),
            (1, false) => l!("Надіслати файл", "Send a file").to_owned(),
            (_, true) => lf!("Надіслати фото: {n}", "Send {n} photos"),
            (_, false) => lf!("Надіслати файли: {n}", "Send {n} files"),
        }
    }

    fn remove(&mut self, i: usize) {
        let path = self.paths.remove(i);
        self.forget_edit(&path);
        // Previews are keyed by index: shift the ones after the removed file.
        self.thumbs = std::mem::take(&mut self.thumbs).into_iter().filter(|(k, _)| *k != i).map(|(k, t)| (if k > i { k - 1 } else { k }, t)).collect();
    }
}

impl SendBox {
    /// Drops what is kept for an edited photo leaving the box, and its file.
    fn forget_edit(&mut self, path: &PathBuf) {
        self.edits.remove(path);
        if let Some(k) = self.temp.iter().position(|p| p == path) {
            let _ = std::fs::remove_file(self.temp.remove(k));
        }
    }

    fn drop_temp(&self) {
        for p in &self.temp {
            let _ = std::fs::remove_file(p);
        }
    }

    fn open_editor(&mut self, ctx: &egui::Context, i: usize) {
        let target = self.paths[i].clone();
        let (source, edit) = match self.edits.get(&target) {
            Some((s, e)) => (s.clone(), Some(e.clone())),
            None => (target.clone(), None),
        };
        self.editor = Some(PhotoEditor::open(ctx, target, source, edit));
    }

    /// The editor's result takes the photo's place; its preview is decoded again.
    fn on_edited(&mut self, ctx: &egui::Context, target: PathBuf, file: PathBuf, edit: Edit) {
        let Some(i) = self.paths.iter().position(|p| *p == target) else {
            let _ = std::fs::remove_file(file);
            return;
        };
        let source = self.edits.get(&target).map_or_else(|| target.clone(), |(s, _)| s.clone());
        self.forget_edit(&target);
        self.paths[i] = file.clone();
        self.thumbs.remove(&i);
        self.edits.insert(file.clone(), (source, edit));
        self.temp.push(file.clone());
        self.decode(ctx, vec![file]);
    }
}

impl App {
    /// Files to send open the box first; more files for the same chat join an open box.
    pub(super) fn send_files(&mut self, peer: PeerId, paths: Vec<PathBuf>, as_file: bool) {
        if paths.is_empty() || self.take_replacement(peer, &paths, as_file) {
            return;
        }
        if let Some(b) = self.send_box.as_mut().filter(|b| b.peer == peer) {
            b.add(&self.ctx, paths);
            return;
        }
        // The text typed so far becomes the caption, as in Telegram Desktop.
        let caption = if self.selected == Some(peer) { std::mem::take(&mut self.draft) } else { String::new() };
        let (tx, rx) = mpsc::channel();
        let private = self.chats.iter().any(|c| c.id == peer && c.kind == ChatKind::User);
        let mut b = SendBox { peer, paths: Vec::new(), caption, compress: !as_file, spoiler: false, above: false, private, once: false, thumbs: HashMap::new(), tx, rx, focused: false, archive: false, folders: HashMap::new(), packing: None, editor: None, edits: HashMap::new(), temp: Vec::new() };
        b.add(&self.ctx, paths);
        self.send_box = Some(b);
    }

    fn close_send_box(&mut self, send: bool) {
        let Some(b) = self.send_box.take() else { return };
        if !send {
            // Cancelling gives the caption back to the field.
            b.drop_temp();
            if self.selected == Some(b.peer) && self.draft.trim().is_empty() {
                self.draft = b.caption;
            }
            return;
        }
        if b.needs_packing() {
            // Packing a big folder takes a while: on a thread, the box waits and can be cancelled.
            let (tx, rx) = mpsc::channel();
            let cancel = Arc::new(AtomicBool::new(false));
            let (paths, all, flag, ctx) = (b.paths.clone(), b.archive && b.can_archive(), cancel.clone(), self.ctx.clone());
            std::thread::spawn(move || {
                // The box was closed meanwhile: nobody will send these archives.
                if let Err(mpsc::SendError(Ok(packed))) = tx.send(pack(paths, all, &flag)) {
                    packed.cleanup();
                }
                ctx.request_repaint();
            });
            self.send_box = Some(SendBox { packing: Some(Packing { cancel, rx }), ..b });
            return;
        }
        self.queue_box(b, None);
    }

    /// Puts the box's files (or the archives packed from them) into the send queue.
    fn queue_box(&mut self, b: SendBox, packed: Option<Packed>) {
        let reply_to = self.take_reply();
        let archive = packed.is_some() && b.archive;
        let paths = packed.as_ref().map(|p| p.paths.clone()).unwrap_or_else(|| b.paths.clone());
        let compress = b.compress && !archive;
        let once = b.once && b.can_view_once();
        let media = QueuedMedia::Files { paths, as_file: !compress, spoiler: b.spoiler && compress, above: b.above && compress, once };
        // Through the send queue: offline the files wait for the network, like text.
        self.enqueue_media(b.peer, b.caption.trim().to_owned(), reply_to, media, false);
        // The queue keeps its own copy of the archives and edited photos.
        if let Some(p) = packed {
            p.cleanup();
        }
        b.drop_temp();
    }

    pub(super) fn send_box_view(&mut self, ui: &mut Ui) {
        let mods = self.send_mods();
        let Some(b) = &mut self.send_box else { return };
        let done = b.packing.as_ref().and_then(|p| match p.rx.try_recv() {
            Ok(r) => Some(r),
            Err(mpsc::TryRecvError::Empty) => None,
            Err(mpsc::TryRecvError::Disconnected) => Some(Err(String::new())),
        });
        match done {
            Some(Ok(packed)) => {
                if let Some(b) = self.send_box.take() {
                    self.queue_box(b, Some(packed));
                }
                return;
            }
            Some(Err(e)) => {
                b.packing = None;
                if !e.is_empty() {
                    self.toast = Some((lf!("Не вдалося створити архів: {e}", "Could not create the archive: {e}"), 0.0, true));
                }
                return;
            }
            None => {}
        }
        while let Ok((path, img)) = b.rx.try_recv() {
            if let Some(i) = b.paths.iter().position(|p| *p == path) {
                let blur = ui.ctx().load_texture(format!("send-box-blur-{i}"), spoiler::blurred(&img), TextureOptions::LINEAR);
                b.thumbs.insert(i, (ui.ctx().load_texture(format!("send-box-{i}"), img, TextureOptions::LINEAR), blur));
            }
        }
        if let Some(ed) = &mut b.editor {
            match ed.view(ui) {
                Some(Outcome::Done { file, edit }) => {
                    let target = ed.target.clone();
                    b.editor = None;
                    b.on_edited(ui.ctx(), target, file, edit);
                }
                Some(Outcome::Cancel) => b.editor = None,
                Some(Outcome::Failed(e)) => {
                    b.editor = None;
                    self.toast = Some((lf!("Не вдалося змінити фото: {e}", "Could not edit the photo: {e}"), 0.0, true));
                }
                None => {}
            }
            return;
        }
        let screen = ui.max_rect();
        ui.painter().rect_filled(screen, 0.0, Color32::from_black_alpha(90));
        ui.interact(screen, ui.scope_id().with("send-box-bg"), Sense::click());
        let w = screen.width().min(460.0) - 20.0;
        let tiles = b.tiles();
        let rows = b.paths.len() - tiles.len();
        // Two photos per row, one alone when it is the only one.
        let tile_rows = if tiles.len() == 1 { 1 } else { tiles.len().div_ceil(2) };
        let tile_h = if tiles.len() == 1 { TILE_H * 1.6 } else { TILE_H };
        let options = b.options();
        let content = tile_rows as f32 * (tile_h + 6.0) + rows as f32 * (ROW_H + 4.0) + options as f32 * 26.0 + 60.0;
        let h = (content + 36.0 + 46.0 + 36.0).min(screen.height() - 60.0);
        let rect = Rect::from_center_size(screen.center(), vec2(w, h));
        glass::glass(ui.painter(), rect, Glass::panel(26.0).with_shadow());
        let inner = rect.shrink(18.0);
        ui.painter().text(inner.left_top(), Align2::LEFT_TOP, b.title(), FontId::proportional(17.0), ink());

        let send_key = ui.input_mut(|i| i.consume_key(mods, Key::Enter));
        let esc = ui.input_mut(|i| i.consume_key(egui::Modifiers::NONE, Key::Escape));
        let buttons = Rect::from_min_max(pos2(inner.left(), inner.bottom() - 36.0), inner.max);
        let field = Rect::from_min_max(pos2(inner.left(), buttons.top() - 48.0), pos2(inner.right(), buttons.top() - 10.0));
        let body = Rect::from_min_max(pos2(inner.left() - 4.0, inner.top() + 34.0), pos2(inner.right() + 4.0, field.top() - 8.0));
        let mut child = ui.new_child(UiBuilder::new().max_rect(body));
        child.set_clip_rect(body);
        let mut remove = None;
        let mut edit = None;
        let now = ui.input(|i| i.time);
        egui::ScrollArea::vertical().id_salt("send-box").auto_shrink(false).show(&mut child, |ui| {
            ui.spacing_mut().item_spacing.y = 4.0;
            let w = ui.available_width() - 8.0;
            for pair in tiles.chunks(2) {
                let (row, _) = ui.allocate_exact_size(vec2(w, tile_h + 2.0), Sense::hover());
                let cw = if pair.len() == 1 { w } else { (w - 6.0) / 2.0 };
                for (k, &i) in pair.iter().enumerate() {
                    let r = Rect::from_min_size(pos2(row.left() + k as f32 * (cw + 6.0), row.top()), vec2(cw, tile_h));
                    let p = ui.painter();
                    match b.thumbs.get(&i).map(|(t, blur)| if b.spoiler { blur } else { t }) {
                        Some(t) => glass::image_rounded(p, r, 14.0, t.id(), t.size_vec2()),
                        None => {
                            p.rect_filled(r, 14.0, glass::veil(14));
                            bubbles::spinner(p, r.center(), 10.0);
                        }
                    }
                    if b.spoiler {
                        spoiler::cover(p, r, 14.0, now);
                    }
                    let x = Rect::from_min_size(pos2(r.right() - 38.0, r.top() + 6.0), vec2(32.0, 32.0));
                    if glass::icon_button(ui, x, ("send-box-remove", i), Icon::Close, false, true).on_hover_text(l!("Прибрати", "Remove")).clicked() {
                        remove = Some(i);
                    }
                    let pen = x.translate(vec2(-38.0, 0.0));
                    if b.packing.is_none() && glass::icon_button(ui, pen, ("send-box-edit", i), Icon::Pencil, false, true).on_hover_text(l!("Редагувати", "Edit")).clicked() {
                        edit = Some(i);
                    }
                }
            }
            for i in (0..b.paths.len()).filter(|i| !tiles.contains(i)) {
                let (r, _) = ui.allocate_exact_size(vec2(w, ROW_H), Sense::hover());
                let p = ui.painter();
                let icon = Rect::from_min_size(r.min + vec2(0.0, 3.0), vec2(40.0, 40.0));
                let path = &b.paths[i];
                let folder = b.folders.get(path);
                glass::glass(p, icon, Glass::tinted(20.0, ACCENT, 170));
                icons::draw(p, Rect::from_center_size(icon.center(), vec2(19.0, 19.0)), if folder.is_some() { Icon::Folder } else { Icon::File }, glass::ON_DARK);
                let name = path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
                let size = match folder {
                    Some(s) => {
                        let n = s.files;
                        format!("{} · {}", lp!(n, ["{n} файл", "{n} файли", "{n} файлів"], ["{n} file", "{n} files"]), crate::backend::human_size(s.bytes as usize))
                    }
                    None => std::fs::metadata(path).map(|m| crate::backend::human_size(m.len() as usize)).unwrap_or_default(),
                };
                let name = elided(ui, &name, 14.5, ink(), r.width() - 100.0);
                ui.painter().galley(r.min + vec2(52.0, 4.0), name, ink());
                ui.painter().text(r.min + vec2(52.0, 25.0), Align2::LEFT_TOP, size, FontId::proportional(12.0), ink_dim());
                let x = Rect::from_center_size(pos2(r.right() - 18.0, r.center().y), vec2(32.0, 32.0));
                if glass::icon_button(ui, x, ("send-box-remove", i), Icon::Close, false, true).on_hover_text(l!("Прибрати", "Remove")).clicked() {
                    remove = Some(i);
                }
            }
            ui.add_space(6.0);
            if b.can_archive() {
                ui.checkbox(&mut b.archive, l!("Надіслати одним архівом", "Send as one archive"));
            }
            if !b.archive && b.paths.iter().any(|p| is_image(p)) {
                ui.checkbox(&mut b.compress, l!("Стиснути зображення", "Compress images"));
            }
            if b.has_media() {
                ui.checkbox(&mut b.spoiler, l!("Приховати під спойлер", "Hide with spoiler"));
                ui.checkbox(&mut b.above, l!("Підпис над медіа", "Caption above media"));
            }
            if b.can_view_once() {
                ui.checkbox(&mut b.once, l!("Одноразовий перегляд", "View once"));
            }
        });
        if let Some(i) = remove {
            b.remove(i);
        } else if let Some(i) = edit {
            b.open_editor(ui.ctx(), i);
        }
        let caption = glass::text_field(ui, field, &mut b.caption, l!("Підпис", "Caption"), false);
        if !b.focused {
            b.focused = true;
            caption.request_focus();
        }
        let half = (buttons.width() - 8.0) / 2.0;
        let cancel = glass::button(ui, Rect::from_min_size(buttons.min, vec2(half, buttons.height())), "send-box-cancel", l!("Скасувати", "Cancel"), false, true).clicked();
        let send_rect = Rect::from_min_size(pos2(buttons.right() - half, buttons.top()), vec2(half, buttons.height()));
        let packing = b.packing.is_some();
        let send = if packing {
            glass::glass(ui.painter(), send_rect, Glass::tinted(send_rect.height() / 2.0, ACCENT, 110));
            bubbles::spinner(ui.painter(), pos2(send_rect.left() + 22.0, send_rect.center().y), 7.0);
            ui.painter().text(pos2(send_rect.left() + 38.0, send_rect.center().y), Align2::LEFT_CENTER, l!("Пакую…", "Packing…"), FontId::proportional(14.0), glass::ON_DARK);
            ui.ctx().request_repaint();
            false
        } else {
            glass::button(ui, send_rect, "send-box-send", l!("Надіслати", "Send"), true, true).clicked()
        };
        let empty = b.paths.is_empty();
        if cancel || esc || empty {
            // Cancelling while packing drops `Packing`, which stops the thread.
            self.close_send_box(false);
        } else if (send || send_key) && !packing {
            self.close_send_box(true);
        }
    }
}
