//! Settings → Advanced → "Data and storage", as Telegram Desktop's local storage box: what the
//! cache takes by type with "Clear" for each, "Clear all", and the cache size limit.

use super::*;
use crate::backend::{CacheKind, human_size};

/// Limits offered, in GB (0 — no limit).
const LIMITS: [u32; 4] = [1, 5, 20, 0];

#[derive(Default)]
pub(super) struct Storage {
    /// (type, bytes, files); `None` while counting.
    usage: Option<Vec<(CacheKind, u64, usize)>>,
    /// "Clear all" was pressed once and asks again.
    confirm_all: bool,
}

fn kind_label(k: CacheKind) -> &'static str {
    match k {
        CacheKind::Photos => l!("Фото", "Photos"),
        CacheKind::Videos => l!("Відео й GIF", "Videos and GIFs"),
        CacheKind::Audio => l!("Голосові й музика", "Voice and music"),
        CacheKind::Stickers => l!("Стікери", "Stickers"),
        CacheKind::Profiles => l!("Фото профілів і історії", "Profile photos and stories"),
        CacheKind::Other => l!("Інше", "Other"),
    }
}

fn kind_icon(k: CacheKind) -> Icon {
    match k {
        CacheKind::Photos => Icon::Camera,
        CacheKind::Videos => Icon::Video,
        CacheKind::Audio => Icon::Mic,
        CacheKind::Stickers => Icon::Smile,
        CacheKind::Profiles => Icon::User,
        CacheKind::Other => Icon::File,
    }
}

impl App {
    pub(super) fn on_cache_usage(&mut self, usage: Vec<(CacheKind, u64, usize)>) {
        self.storage.usage = Some(usage);
        self.storage.confirm_all = false;
    }

    pub(super) fn load_cache_usage(&mut self) {
        self.storage.usage = None;
        self.send(Cmd::LoadCacheUsage);
    }

    /// "Error log": how many entries, copying a report, the folder, clearing.
    pub(super) fn error_log_section(&mut self, ui: &mut Ui) {
        section_title(ui, l!("Журнал помилок", "Error log"));
        let n = crate::errlog::count();
        ui.label(egui::RichText::new(if n == 0 { l!("Помилок не було", "No errors recorded").to_owned() } else { lf!("Записів: {n}", "Entries: {n}") }).color(ink()).size(14.0));
        let mut copied = false;
        ui.horizontal_wrapped(|ui| {
            ui.spacing_mut().item_spacing = vec2(6.0, 6.0);
            let w = ((ui.available_width() - 12.0) / 3.0).clamp(100.0, 150.0);
            let (r, _) = ui.allocate_exact_size(vec2(w, 32.0), Sense::hover());
            if glass::button(ui, r, "errlog-copy", l!("Копіювати звіт", "Copy report"), false, true).clicked() {
                ui.ctx().copy_text(crate::errlog::report());
                copied = true;
            }
            let (r, _) = ui.allocate_exact_size(vec2(w, 32.0), Sense::hover());
            if glass::button(ui, r, "errlog-open", l!("Відкрити теку", "Open folder"), false, true).clicked() {
                let _ = std::fs::create_dir_all(crate::errlog::dir());
                crate::platform::open(crate::errlog::dir());
            }
            let (r, _) = ui.allocate_exact_size(vec2(w, 32.0), Sense::hover());
            if glass::button(ui, r, "errlog-clear", l!("Очистити", "Clear"), false, n > 0).clicked() {
                crate::errlog::clear();
            }
        });
        if copied {
            self.toast = Some((l!("Звіт скопійовано", "Report copied").to_owned(), 0.0, false));
        }
        ui.label(egui::RichText::new(l!("Звіт містить версію, систему й останні помилки — без повідомлень, номерів і паролів. Надішліть його розробнику, якщо щось не працює.", "The report has the version, system and recent errors — no messages, numbers or passwords. Send it to the developer if something doesn't work.")).color(ink_dim()).size(12.5));
    }

    pub(super) fn storage_section(&mut self, ui: &mut Ui) {
        section_title(ui, l!("Дані та сховище", "Data and storage"));
        let mut cmd = None;
        match &self.storage.usage {
            None => {
                ui.add(egui::Spinner::new().color(ink_dim()));
            }
            Some(usage) => {
                let total: u64 = usage.iter().map(|u| u.1).sum();
                ui.label(egui::RichText::new(lf!("Кеш займає {}", "The cache takes {}", human_size(total as usize))).color(ink()).size(14.0));
                for &(kind, bytes, count) in usage.iter().filter(|u| u.2 > 0) {
                    let (row, _) = ui.allocate_exact_size(vec2(ui.available_width(), 48.0), Sense::hover());
                    let p = ui.painter();
                    p.rect_filled(row, 14.0, glass::veil(10));
                    icons::draw(p, Rect::from_center_size(pos2(row.left() + 22.0, row.center().y), vec2(20.0, 20.0)), kind_icon(kind), ink_dim());
                    let text_w = row.width() - 160.0;
                    p.galley(pos2(row.left() + 46.0, row.top() + 6.0), elided(ui, kind_label(kind), 14.0, ink(), text_w), ink());
                    let size = lf!("{} · файлів: {count}", "{} · {count} files", human_size(bytes as usize));
                    p.galley(pos2(row.left() + 46.0, row.top() + 26.0), elided(ui, &size, 12.5, ink_dim(), text_w), ink_dim());
                    let b = Rect::from_min_size(pos2(row.right() - 100.0, row.top() + 8.0), vec2(92.0, 32.0));
                    if glass::button(ui, b, ("cache-clear", kind as u8), l!("Очистити", "Clear"), false, true).clicked() {
                        cmd = Some(Cmd::ClearCache(vec![kind]));
                    }
                    ui.add_space(4.0);
                }
                if total > 0 {
                    let r = ui.allocate_exact_size(vec2(ui.available_width(), 36.0), Sense::hover()).0;
                    let confirm = self.storage.confirm_all;
                    let label = if confirm { l!("Точно очистити весь кеш?", "Really clear the whole cache?") } else { l!("Очистити все", "Clear all") };
                    if glass::button(ui, r, "cache-clear-all", label, confirm, true).clicked() {
                        if confirm {
                            cmd = Some(Cmd::ClearCache(CacheKind::ALL.to_vec()));
                        }
                        self.storage.confirm_all = !confirm;
                    }
                }
            }
        }
        ui.label(egui::RichText::new(l!("Медіа залишаються в хмарі Telegram і завантажаться знову, коли знадобляться.", "Media stay in the Telegram cloud and download again when needed.")).color(ink_dim()).size(12.5));
        ui.add_space(6.0);
        section_title(ui, l!("Найбільший розмір кешу", "Maximum cache size"));
        let before = self.settings.cache_limit_gb;
        ui.horizontal_wrapped(|ui| {
            ui.spacing_mut().item_spacing = vec2(6.0, 6.0);
            for gb in LIMITS {
                let label = if gb == 0 { l!("Без обмежень", "No limit").to_owned() } else { format!("{gb} GB") };
                let (r, _) = ui.allocate_exact_size(vec2(if gb == 0 { 130.0 } else { 76.0 }, 30.0), Sense::hover());
                if glass::button(ui, r, ("cache-limit", gb), &label, self.settings.cache_limit_gb == gb, true).clicked() {
                    self.settings.cache_limit_gb = gb;
                }
            }
        });
        ui.label(egui::RichText::new(l!("Понад цю межу спершу видаляються найдавніші файли.", "Over this limit the oldest files go first.")).color(ink_dim()).size(12.5));
        let after = self.settings.cache_limit_gb;
        if after != before && after > 0 {
            cmd = Some(Cmd::TrimCache(u64::from(after) << 30));
        }
        if let Some(c) = cmd {
            self.storage.usage = None;
            self.send(c);
        }
    }
}
