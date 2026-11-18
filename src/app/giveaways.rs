//! "Learn more" on a giveaway card, as in Telegram Desktop: a window that says whether we take part,
//! why not, or whether we won.

use super::*;

#[derive(Default)]
pub(super) struct GiveawayBox {
    /// The giveaway message asked about; `None` when closed.
    open: Option<(PeerId, i32)>,
    /// `None` — waiting for the server.
    text: Option<Result<String, String>>,
}

impl App {
    pub(super) fn open_giveaway(&mut self, peer: PeerId, id: i32) {
        self.giveaway = GiveawayBox { open: Some((peer, id)), text: None };
        self.send(Cmd::GiveawayInfo { peer, id });
    }

    pub(super) fn on_giveaway_info(&mut self, peer: PeerId, id: i32, result: Result<String, String>) {
        if self.giveaway.open == Some((peer, id)) {
            self.giveaway.text = Some(result);
        }
    }

    pub(super) fn giveaway_view(&mut self, ui: &mut Ui, peer: PeerId, rect: Rect) {
        let Some((at, _)) = self.giveaway.open else { return };
        if at != peer {
            self.giveaway.open = None;
            return;
        }
        let w = rect.width().min(380.0) - 16.0;
        let body = match &self.giveaway.text {
            Some(Ok(t)) => Some((t.clone(), ink())),
            Some(Err(e)) => Some((e.clone(), glass::danger())),
            None => None,
        };
        let g = body.map(|(t, c)| ui.painter().layout(t, FontId::proportional(14.0), c, w - 36.0));
        let h = 56.0 + g.as_ref().map_or(40.0, |g| g.size().y) + 18.0 + 50.0;
        let panel = Rect::from_center_size(rect.center(), vec2(w, h));
        glass::glass(ui.painter(), panel, Glass::panel(24.0).with_shadow());
        ui.interact(panel, ui.scope_id().with("giveaway-bg"), Sense::CLICK);
        ui.painter().text(pos2(panel.left() + 18.0, panel.top() + 28.0), Align2::LEFT_CENTER, l!("Про розіграш", "About the giveaway"), FontId::proportional(17.0), ink());
        let text_at = pos2(panel.left() + 18.0, panel.top() + 56.0);
        match g {
            Some(g) => ui.painter().galley(text_at, g, ink()),
            None => {
                ui.put(Rect::from_min_size(text_at, vec2(w - 36.0, 40.0)), egui::Spinner::new().size(22.0).color(ink()));
            }
        }
        let ok = Rect::from_min_size(pos2(panel.left() + 18.0, panel.bottom() - 52.0), vec2(w - 36.0, 36.0));
        let esc = ui.input_mut(|i| i.consume_key(egui::Modifiers::NONE, Key::Escape));
        if glass::button(ui, ok, "giveaway-ok", l!("Зрозуміло", "OK"), true, true).clicked() || esc {
            self.giveaway.open = None;
        }
    }
}
