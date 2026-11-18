//! The AI editor of the message field, as in Telegram Desktop: from the send button's menu, a
//! window with the draft and its rewrite — proofread, with emoji, translated or in a tone — and
//! "Apply" puts the rewrite into the field.

use super::*;
use crate::backend::{AiMode, AiTone};

#[derive(Default)]
pub(super) struct AiCompose {
    /// The open window; `None` when closed.
    open: Option<Editing>,
    /// The tones from `aicompose.getTones`, asked once per run.
    tones: Vec<AiTone>,
    tones_asked: bool,
}

struct Editing {
    peer: PeerId,
    /// The draft as it was when the window opened (markup).
    source: String,
    mode: AiMode,
    /// `None` — waiting for the server.
    result: Option<Result<String, String>>,
}

impl App {
    pub(super) fn open_ai_compose(&mut self, peer: PeerId) {
        let source = self.draft.trim().to_owned();
        if source.is_empty() {
            return;
        }
        self.ai_compose.open = Some(Editing { peer, source, mode: AiMode::Proofread, result: None });
        if !self.ai_compose.tones_asked {
            self.ai_compose.tones_asked = true;
            self.send(Cmd::LoadAiTones);
        }
        self.ai_compose_ask(AiMode::Proofread);
    }

    fn ai_compose_ask(&mut self, mode: AiMode) {
        let Some(e) = &mut self.ai_compose.open else { return };
        e.mode = mode.clone();
        e.result = None;
        let draft = e.source.clone();
        self.send(Cmd::AiCompose { draft, mode });
    }

    pub(super) fn on_ai_composed(&mut self, draft: String, mode: AiMode, result: Result<String, String>) {
        // Only the answer to the latest question for this draft.
        if let Some(e) = self.ai_compose.open.as_mut().filter(|e| e.source == draft && e.mode == mode) {
            e.result = Some(result);
        }
    }

    pub(super) fn on_ai_tones(&mut self, tones: Vec<AiTone>) {
        self.ai_compose.tones = tones;
    }

    pub(super) fn ai_compose_view(&mut self, ui: &mut Ui, peer: PeerId, rect: Rect) {
        let Some(e) = &self.ai_compose.open else { return };
        if e.peer != peer {
            self.ai_compose.open = None;
            return;
        }
        let panel = Rect::from_center_size(rect.center(), vec2((rect.width() - 32.0).min(460.0), (rect.height() - 40.0).min(470.0)));
        glass::glass(ui.painter(), panel, Glass::panel(24.0).with_shadow());
        ui.interact(panel, ui.scope_id().with("ai-compose-bg"), Sense::CLICK);
        ui.painter().text(pos2(panel.left() + 18.0, panel.top() + 28.0), Align2::LEFT_CENTER, l!("Редактор ШІ", "AI editor"), FontId::proportional(17.0), ink());
        let close = Rect::from_center_size(pos2(panel.right() - 30.0, panel.top() + 28.0), vec2(34.0, 34.0));
        let esc = ui.input_mut(|i| i.consume_key(egui::Modifiers::NONE, Key::Escape));
        if glass::icon_button(ui, close, "ai-compose-close", Icon::Close, false, true).clicked() || esc {
            self.ai_compose.open = None;
            return;
        }
        let body = Rect::from_min_max(pos2(panel.left() + 16.0, panel.top() + 56.0), pos2(panel.right() - 16.0, panel.bottom() - 62.0));
        let mut ask = None;
        let mut child = ui.new_child(UiBuilder::new().max_rect(body));
        child.spacing_mut().item_spacing = vec2(8.0, 8.0);
        child.horizontal_wrapped(|ui| {
            if info::chip(ui, l!("Виправити", "Proofread"), e.mode == AiMode::Proofread).clicked() {
                ask = Some(AiMode::Proofread);
            }
            if info::chip(ui, l!("Додати емодзі", "Add emoji"), e.mode == AiMode::Emojify).clicked() {
                ask = Some(AiMode::Emojify);
            }
            let to = match &e.mode {
                AiMode::Translate(to) => Some(to.as_str()),
                _ => None,
            };
            let label = to.map_or_else(|| l!("Перекласти", "Translate").to_owned(), |c| {
                let up = c.to_uppercase();
                lf!("Переклад: {up}", "Translate: {up}")
            });
            let tr = info::chip(ui, &label, to.is_some());
            egui::Popup::menu(&tr).gap(6.0).width(200.0).show(|ui| {
                for (code, name) in translate::LANGS {
                    if glass::menu_button(ui, if Some(code) == to { Icon::Check } else { Icon::Globe }, name).clicked() {
                        ask = Some(AiMode::Translate(code.to_owned()));
                    }
                }
            });
            for t in &self.ai_compose.tones {
                if info::chip(ui, &t.title, e.mode == AiMode::Tone(t.key.clone())).clicked() {
                    ask = Some(AiMode::Tone(t.key.clone()));
                }
            }
        });
        let wrap = body.width() - 24.0;
        let caption = |ui: &mut Ui, text: &str| {
            ui.add_space(4.0);
            ui.label(egui::RichText::new(text).size(12.5).color(ink_dim()));
        };
        caption(&mut child, l!("Оригінал", "Original"));
        egui::ScrollArea::vertical().id_salt("ai-compose-source").max_height(96.0).auto_shrink([false, true]).show(&mut child, |ui| {
            ui.label(format::markup_layout(&e.source, 14.0, ink_dim(), ink_dim(), ink_dim(), wrap));
        });
        caption(&mut child, l!("Результат", "Result"));
        let left = child.available_rect_before_wrap();
        let shown = Rect::from_min_size(left.min, vec2(left.width(), left.height().max(40.0)));
        child.painter().rect_filled(shown, 14.0, glass::veil(10));
        let inner = shown.shrink(12.0);
        match &e.result {
            None => {
                child.put(Rect::from_center_size(shown.center(), vec2(22.0, 22.0)), egui::Spinner::new().size(22.0).color(ink()));
            }
            Some(Err(err)) => {
                let g = child.painter().layout(err.clone(), FontId::proportional(13.5), glass::danger(), inner.width());
                child.painter().galley(inner.min, g, glass::danger());
            }
            Some(Ok(text)) => {
                let mut res = child.new_child(UiBuilder::new().max_rect(inner));
                egui::ScrollArea::vertical().id_salt("ai-compose-result").auto_shrink(false).show(&mut res, |ui| {
                    ui.label(format::markup_layout(text, 15.0, ink(), ink_dim(), glass::link(false), inner.width() - 12.0));
                });
            }
        }
        let ready = matches!(e.result, Some(Ok(_)));
        let mode_now = e.mode.clone();
        let half = (panel.width() - 16.0 * 2.0 - 8.0) / 2.0;
        let cancel = Rect::from_min_size(pos2(panel.left() + 16.0, panel.bottom() - 50.0), vec2(half, 36.0));
        let apply = Rect::from_min_size(pos2(cancel.right() + 8.0, cancel.top()), vec2(half, 36.0));
        if glass::button(ui, cancel, "ai-compose-cancel", l!("Скасувати", "Cancel"), false, true).clicked() {
            self.ai_compose.open = None;
            return;
        }
        if glass::button(ui, apply, "ai-compose-apply", l!("Застосувати", "Apply"), true, ready).clicked()
            && let Some(Editing { result: Some(Ok(text)), .. }) = self.ai_compose.open.take()
        {
            self.draft = text;
            self.draft_edited_at = ui.input(|i| i.time);
            self.focus_composer();
            return;
        }
        if let Some(mode) = ask.filter(|m| *m != mode_now) {
            self.ai_compose_ask(mode);
        }
    }
}
