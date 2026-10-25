//! "Translate" from the message menu, as in Telegram Desktop: a window with the original,
//! the translation and a choice of target language.

use super::*;

/// Target languages offered in the window: code and the name in that language.
const LANGS: [(&str, &str); 8] = [
    ("uk", "Українська"),
    ("en", "English"),
    ("de", "Deutsch"),
    ("pl", "Polski"),
    ("fr", "Français"),
    ("es", "Español"),
    ("it", "Italiano"),
    ("pt", "Português"),
];

pub(super) struct TranslateView {
    peer: PeerId,
    id: i32,
    to: String,
    /// `None` — waiting for the server.
    result: Option<Result<(String, Vec<format::Span>), String>>,
}

impl App {
    pub(super) fn open_translate(&mut self, peer: PeerId, id: i32) {
        // The last chosen language, or the interface language.
        let to = self.translate_to.clone().unwrap_or_else(|| self.settings.lang.shown_code());
        self.translate = Some(TranslateView { peer, id, to: to.clone(), result: None });
        self.send(Cmd::Translate { peer, id, to });
    }

    pub(super) fn translated(&mut self, peer: PeerId, id: i32, to: String, result: Result<(String, Vec<format::Span>), String>) {
        if let Some(t) = &mut self.translate
            && t.peer == peer
            && t.id == id
            && t.to == to
        {
            t.result = Some(result);
        }
    }

    pub(super) fn translate_view(&mut self, ui: &mut Ui, peer: PeerId, rect: Rect) {
        let Some(t) = &self.translate else { return };
        if t.peer != peer || ui.input_mut(|i| i.consume_key(egui::Modifiers::NONE, Key::Escape)) {
            self.translate = None;
            return;
        }
        let (id, to) = (t.id, t.to.clone());
        let original = self.find(peer, id).map(|m| (m.text.clone(), m.spans.clone()));
        let protected = self.protected(peer, &[id]);
        let panel = Rect::from_center_size(rect.center(), vec2(rect.width().min(440.0), (rect.height() - 100.0).min(520.0)));
        glass::glass(ui.painter(), panel, Glass::panel(24.0).with_shadow());
        let p = ui.painter().clone();
        p.text(pos2(panel.left() + 18.0, panel.top() + 26.0), Align2::LEFT_CENTER, l!("Переклад", "Translation"), FontId::proportional(16.0), ink());
        let close = Rect::from_center_size(pos2(panel.right() - 26.0, panel.top() + 26.0), vec2(32.0, 32.0));
        if glass::icon_button(ui, close, "tr-close", Icon::Close, false, true).clicked() {
            self.translate = None;
            return;
        }

        let inner = panel.shrink2(vec2(18.0, 0.0));
        let mut child = ui.new_child(UiBuilder::new().max_rect(Rect::from_min_max(pos2(inner.left(), panel.top() + 52.0), pos2(inner.right(), panel.bottom() - 60.0))));
        let mut pick = None;
        egui::ScrollArea::vertical().id_salt("translate").auto_shrink(false).show(&mut child, |ui| {
            let w = ui.available_width();
            if let Some((text, spans)) = &original {
                ui.label(egui::RichText::new(l!("Оригінал", "Original")).size(12.5).color(ink_dim()));
                let job = format::layout(text, spans, 14.0, ink_dim(), glass::link(false), w, true);
                ui.label(job);
                ui.add_space(12.0);
            }
            // Target language: capsules, the chosen one accented.
            ui.horizontal_wrapped(|ui| {
                ui.spacing_mut().item_spacing = vec2(6.0, 6.0);
                for (code, name) in LANGS {
                    let (r, _) = ui.allocate_exact_size(vec2(96.0, 30.0), Sense::hover());
                    if glass::button(ui, r, ("tr-lang", code), name, code == to, true).clicked() && code != to {
                        pick = Some(code.to_owned());
                    }
                }
            });
            ui.add_space(12.0);
            match self.translate.as_ref().and_then(|t| t.result.as_ref()) {
                None => {
                    ui.add_space(12.0);
                    ui.vertical_centered(|ui| ui.add(egui::Spinner::new().color(ink())));
                }
                Some(Ok((text, spans))) => {
                    let job = format::layout(text, spans, 15.0, ink(), glass::link(false), w, true);
                    ui.label(job);
                }
                Some(Err(e)) => {
                    ui.label(egui::RichText::new(e).size(14.0).color(glass::danger()));
                }
            }
        });

        let done = self.translate.as_ref().and_then(|t| t.result.as_ref()).and_then(|r| r.as_ref().ok()).map(|(text, _)| text.clone());
        let b = Rect::from_min_max(pos2(inner.left(), panel.bottom() - 52.0), pos2(inner.right(), panel.bottom() - 14.0));
        if !protected && glass::button(ui, b, "tr-copy", l!("Копіювати переклад", "Copy translation"), false, done.is_some()).clicked() {
            if let Some(text) = done {
                self.ctx.copy_text(text);
                self.toast = Some((l!("Скопійовано", "Copied").into(), 0.0, false));
            }
        }
        if let Some(code) = pick {
            self.translate_to = Some(code.clone());
            self.translate = Some(TranslateView { peer, id, to: code.clone(), result: None });
            self.send(Cmd::Translate { peer, id, to: code });
        }
    }
}
