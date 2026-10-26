//! Asking before something from a stranger leaves the app: a link whose text hides its real
//! address, or a received file that the system would run as a program. As in Telegram Desktop.

use super::*;

/// What waits for the user's answer.
pub(super) enum Opening {
    Link(String),
    Program(PathBuf),
}

impl App {
    /// A file we received or downloaded: documents open, programs and scripts ask first.
    pub(super) fn open_received(&mut self, path: &std::path::Path) {
        if crate::platform::runnable(path) {
            self.opening = Some(Opening::Program(path.to_owned()));
        } else {
            crate::platform::open_file(path);
        }
    }

    pub(super) fn opening_view(&mut self, ui: &mut Ui) {
        let Some(what) = &self.opening else { return };
        let (title, body, target, go) = match what {
            Opening::Link(url) => (l!("Відкрити посилання?", "Open this link?"), l!("Текст посилання відрізняється від адреси, куди воно веде:", "The link's text differs from where it leads:").to_owned(), url.clone(), l!("Відкрити", "Open")),
            Opening::Program(path) => {
                let ext = path.extension().map(|e| e.to_string_lossy().to_lowercase()).unwrap_or_default();
                let body = lf!(
                    "Файл з розширенням .{ext} може бути програмою і запуститься на вашому комп'ютері. Відкривайте лише файли від тих, кому довіряєте.",
                    "A .{ext} file may be a program and will run on your computer. Only open files from people you trust."
                );
                (l!("Запустити файл?", "Run this file?"), body, path.display().to_string(), l!("Запустити", "Run"))
            }
        };
        let screen = ui.max_rect();
        ui.painter().rect_filled(screen, 0.0, Color32::from_black_alpha(90));
        ui.interact(screen, ui.scope_id().with("opening-bg"), Sense::click());
        let w = 400.0;
        let text_w = w - 36.0;
        let body_g = ui.painter().layout(body, FontId::proportional(14.0), ink_dim(), text_w);
        let target_g = ui.painter().layout(target, FontId::monospace(12.5), ink(), text_w - 20.0);
        let h = 18.0 + 26.0 + body_g.size().y + 12.0 + target_g.size().y + 20.0 + 12.0 + 34.0 + 18.0;
        let rect = Rect::from_center_size(screen.center(), vec2(w, h));
        glass::glass(ui.painter(), rect, Glass::panel(24.0).with_shadow());
        let inner = rect.shrink(18.0);
        let p = ui.painter();
        p.text(inner.left_top(), Align2::LEFT_TOP, title, FontId::proportional(17.0), ink());
        let mut y = inner.top() + 30.0;
        let body_h = body_g.size().y;
        p.galley(pos2(inner.left(), y), body_g, ink_dim());
        y += body_h + 12.0;
        // The address itself on a plain chip, so it can't be mistaken for part of the text.
        let chip = Rect::from_min_size(pos2(inner.left(), y), vec2(inner.width(), target_g.size().y + 20.0));
        p.rect_filled(chip, 12.0, glass::veil(22));
        let chip_h = chip.height();
        p.galley(chip.min + vec2(10.0, 10.0), target_g, ink());
        y += chip_h + 12.0;
        let half = (inner.width() - 8.0) / 2.0;
        let (cancel_label, cancel_is_reveal) = match what {
            Opening::Program(_) => (l!("Показати в теці", "Show in folder"), true),
            Opening::Link(_) => (l!("Скасувати", "Cancel"), false),
        };
        // For a program the highlighted button is the safe one.
        let cancel = glass::button(ui, Rect::from_min_size(pos2(inner.left(), y), vec2(half, 34.0)), "opening-cancel", cancel_label, cancel_is_reveal, true).clicked();
        let ok = glass::button(ui, Rect::from_min_size(pos2(inner.right() - half, y), vec2(half, 34.0)), "opening-ok", go, !cancel_is_reveal, true).clicked();
        let esc = ui.input_mut(|i| i.consume_key(egui::Modifiers::NONE, Key::Escape));
        if !(cancel || ok || esc) {
            return;
        }
        match self.opening.take() {
            Some(Opening::Link(url)) if ok => {
                crate::platform::open_url(&url);
            }
            Some(Opening::Program(path)) if ok => crate::platform::run_file(&path),
            Some(Opening::Program(path)) if cancel && cancel_is_reveal => crate::platform::reveal(&path),
            _ => {}
        }
    }
}
