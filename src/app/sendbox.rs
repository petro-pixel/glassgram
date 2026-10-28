//! The "Send files" box, as in Telegram Desktop: what is attached (photo previews, file rows),
//! a caption, compression, a spoiler over photos and videos and the caption above them.
//! Picked, pasted and dropped files all come here; more files dropped on it join it.

use super::*;
use eframe::egui::ColorImage;
use crate::backend::is_image;

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
    /// Previews by index in `paths`, decoded on a thread, and their blurred copies for the spoiler.
    thumbs: HashMap<usize, (TextureHandle, TextureHandle)>,
    tx: mpsc::Sender<(PathBuf, ColorImage)>,
    rx: mpsc::Receiver<(PathBuf, ColorImage)>,
    focused: bool,
}

fn is_video(p: &std::path::Path) -> bool {
    let ext = p.extension().map(|e| e.to_string_lossy().to_lowercase()).unwrap_or_default();
    matches!(ext.as_str(), "mp4" | "mov" | "m4v" | "webm" | "mkv")
}

impl SendBox {
    /// Decodes previews of new pictures off the UI thread.
    fn add(&mut self, ctx: &egui::Context, paths: Vec<PathBuf>) {
        let images: Vec<PathBuf> = paths.iter().filter(|p| is_image(p)).cloned().collect();
        self.paths.extend(paths);
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
        (0..self.paths.len()).filter(|&i| self.compress && is_image(&self.paths[i])).collect()
    }

    /// There is a photo or a video the spoiler and the caption position apply to.
    fn has_media(&self) -> bool {
        self.compress && self.paths.iter().any(|p| is_image(p) || is_video(p))
    }

    fn title(&self) -> String {
        let n = self.paths.len();
        let photos = self.compress && self.paths.iter().all(|p| is_image(p));
        match (n, photos) {
            (1, true) => l!("Надіслати фото", "Send a photo").to_owned(),
            (1, false) => l!("Надіслати файл", "Send a file").to_owned(),
            (_, true) => lf!("Надіслати фото: {n}", "Send {n} photos"),
            (_, false) => lf!("Надіслати файли: {n}", "Send {n} files"),
        }
    }

    fn remove(&mut self, i: usize) {
        self.paths.remove(i);
        // Previews are keyed by index: shift the ones after the removed file.
        self.thumbs = std::mem::take(&mut self.thumbs).into_iter().filter(|(k, _)| *k != i).map(|(k, t)| (if k > i { k - 1 } else { k }, t)).collect();
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
        let mut b = SendBox { peer, paths: Vec::new(), caption, compress: !as_file, spoiler: false, above: false, thumbs: HashMap::new(), tx, rx, focused: false };
        b.add(&self.ctx, paths);
        self.send_box = Some(b);
    }

    fn close_send_box(&mut self, send: bool) {
        let Some(b) = self.send_box.take() else { return };
        if !send {
            // Cancelling gives the caption back to the field.
            if self.selected == Some(b.peer) && self.draft.trim().is_empty() {
                self.draft = b.caption;
            }
            return;
        }
        let reply_to = self.take_reply();
        let media = QueuedMedia::Files { paths: b.paths, as_file: !b.compress, spoiler: b.spoiler && b.compress, above: b.above && b.compress };
        // Through the send queue: offline the files wait for the network, like text.
        self.enqueue_media(b.peer, b.caption.trim().to_owned(), reply_to, media, false);
    }

    pub(super) fn send_box_view(&mut self, ui: &mut Ui) {
        let mods = self.send_mods();
        let Some(b) = &mut self.send_box else { return };
        while let Ok((path, img)) = b.rx.try_recv() {
            if let Some(i) = b.paths.iter().position(|p| *p == path) {
                let blur = ui.ctx().load_texture(format!("send-box-blur-{i}"), spoiler::blurred(&img), TextureOptions::LINEAR);
                b.thumbs.insert(i, (ui.ctx().load_texture(format!("send-box-{i}"), img, TextureOptions::LINEAR), blur));
            }
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
        let options = if b.has_media() { 3 } else if tiles.is_empty() && !b.paths.iter().any(|p| is_image(p)) { 0 } else { 1 };
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
                }
            }
            for i in (0..b.paths.len()).filter(|i| !tiles.contains(i)) {
                let (r, _) = ui.allocate_exact_size(vec2(w, ROW_H), Sense::hover());
                let p = ui.painter();
                let icon = Rect::from_min_size(r.min + vec2(0.0, 3.0), vec2(40.0, 40.0));
                glass::glass(p, icon, Glass::tinted(20.0, ACCENT, 170));
                icons::draw(p, Rect::from_center_size(icon.center(), vec2(19.0, 19.0)), Icon::File, glass::ON_DARK);
                let path = &b.paths[i];
                let name = path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
                let size = std::fs::metadata(path).map(|m| crate::backend::human_size(m.len() as usize)).unwrap_or_default();
                let name = elided(ui, &name, 14.5, ink(), r.width() - 100.0);
                ui.painter().galley(r.min + vec2(52.0, 4.0), name, ink());
                ui.painter().text(r.min + vec2(52.0, 25.0), Align2::LEFT_TOP, size, FontId::proportional(12.0), ink_dim());
                let x = Rect::from_center_size(pos2(r.right() - 18.0, r.center().y), vec2(32.0, 32.0));
                if glass::icon_button(ui, x, ("send-box-remove", i), Icon::Close, false, true).on_hover_text(l!("Прибрати", "Remove")).clicked() {
                    remove = Some(i);
                }
            }
            ui.add_space(6.0);
            if b.paths.iter().any(|p| is_image(p)) {
                ui.checkbox(&mut b.compress, l!("Стиснути зображення", "Compress images"));
            }
            if b.has_media() {
                ui.checkbox(&mut b.spoiler, l!("Приховати під спойлер", "Hide with spoiler"));
                ui.checkbox(&mut b.above, l!("Підпис над медіа", "Caption above media"));
            }
        });
        if let Some(i) = remove {
            b.remove(i);
        }
        let caption = glass::text_field(ui, field, &mut b.caption, l!("Підпис", "Caption"), false);
        if !b.focused {
            b.focused = true;
            caption.request_focus();
        }
        let half = (buttons.width() - 8.0) / 2.0;
        let cancel = glass::button(ui, Rect::from_min_size(buttons.min, vec2(half, buttons.height())), "send-box-cancel", l!("Скасувати", "Cancel"), false, true).clicked();
        let send = glass::button(ui, Rect::from_min_size(pos2(buttons.right() - half, buttons.top()), vec2(half, buttons.height())), "send-box-send", l!("Надіслати", "Send"), true, true).clicked();
        let empty = b.paths.is_empty();
        if cancel || esc || empty {
            self.close_send_box(false);
        } else if send || send_key {
            self.close_send_box(true);
        }
    }
}
