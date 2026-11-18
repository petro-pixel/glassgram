//! Bot messages only we can see (2026): a stack of cards over the bottom of the chat, each with
//! "only you can see this", the bot's buttons and a close cross. They disappear when the bot
//! deletes them or we close them, and never go into the history.

use super::*;
use crate::backend::EphemeralMsg;

/// At most this many are shown per chat, the newest at the bottom.
const SHOWN: usize = 3;
const CARD_PAD: f32 = 12.0;

#[derive(Default)]
pub(super) struct Ephemeral {
    msgs: Vec<EphemeralMsg>,
}

impl App {
    pub(super) fn on_ephemeral(&mut self, m: EphemeralMsg) {
        let list = &mut self.ephemeral.msgs;
        match list.iter_mut().find(|x| x.peer == m.peer && x.id == m.id) {
            Some(old) => *old = m,
            None => list.push(m),
        }
    }

    pub(super) fn on_ephemeral_gone(&mut self, peer: PeerId, ids: Vec<i32>) {
        self.ephemeral.msgs.retain(|m| m.peer != peer || !ids.contains(&m.id));
    }

    /// The cards, stacked up from `bottom`.
    pub(super) fn ephemeral_view(&mut self, ui: &mut Ui, peer: PeerId, rect: Rect, bottom: f32) {
        let mine: Vec<EphemeralMsg> = self.ephemeral.msgs.iter().filter(|m| m.peer == peer).rev().take(SHOWN).cloned().collect();
        let w = (rect.width() - 24.0).min(420.0);
        let mut y = bottom;
        let mut close = None;
        let mut press = None;
        for m in &mine {
            let inner = w - CARD_PAD * 2.0;
            let body = ui.painter().layout_job(format::layout(&m.text, &m.spans, 14.5, ink(), glass::link(false), inner, true));
            let rows = m.buttons.len() as f32;
            let h = CARD_PAD + 18.0 + 6.0 + body.size().y + rows * 38.0 + CARD_PAD;
            let card = Rect::from_min_size(pos2(rect.left() + 12.0, y - h), vec2(w, h));
            y = card.top() - 8.0;
            glass::glass(ui.painter(), card, Glass::tinted(18.0, glass::chrome(), 215).with_shadow().frosted(1.0));
            ui.interact(card, ui.scope_id().with(("ephemeral-bg", m.id)), Sense::CLICK);
            let p = ui.painter();
            let head = pos2(card.left() + CARD_PAD, card.top() + CARD_PAD + 9.0);
            icons::draw(p, Rect::from_center_size(head + vec2(7.0, 0.0), vec2(14.0, 14.0)), Icon::EyeOff, ink_dim());
            let label = if m.from.is_empty() { l!("Бачите лише ви", "Only you can see this").to_owned() } else { lf!("{} · бачите лише ви", "{} · only you can see this", m.from) };
            p.galley(head + vec2(20.0, -8.0), elided(ui, &label, 12.5, ink_dim(), inner - 50.0), ink_dim());
            let x = Rect::from_center_size(pos2(card.right() - 22.0, card.top() + 22.0), vec2(28.0, 28.0));
            if glass::icon_button(ui, x, ("ephemeral-close", m.id), Icon::Close, false, true).on_hover_text(l!("Закрити", "Close")).clicked() {
                close = Some(m.id);
            }
            let text_top = card.top() + CARD_PAD + 24.0;
            ui.painter().galley(pos2(card.left() + CARD_PAD, text_top), body.clone(), ink());
            let mut by = text_top + body.size().y + 4.0;
            for (i, row) in m.buttons.iter().enumerate() {
                let bw = (inner - 4.0 * (row.len() as f32 - 1.0)) / row.len().max(1) as f32;
                for (j, b) in row.iter().enumerate() {
                    let br = Rect::from_min_size(pos2(card.left() + CARD_PAD + j as f32 * (bw + 4.0), by + 2.0), vec2(bw, 34.0));
                    if glass::button(ui, br, ("ephemeral-btn", m.id, i, j), &b.text, false, true).clicked() {
                        press = Some((m.id, b.action.clone()));
                    }
                }
                by += 38.0;
            }
        }
        if let Some(id) = close {
            self.on_ephemeral_gone(peer, vec![id]);
        }
        match press {
            Some((id, ButtonAction::Callback(data))) => self.send(Cmd::PressEphemeral { peer, id, data }),
            Some((_, ButtonAction::Url(url))) => self.open_link(&url),
            Some((_, ButtonAction::Copy(text))) => {
                self.ctx.copy_text(text);
                self.toast = Some((l!("Скопійовано", "Copied").into(), 0.0, false));
            }
            Some(_) => self.toast = Some((l!("Цей тип кнопки ще не підтримується", "This button type is not supported yet").into(), 0.0, true)),
            None => {}
        }
    }
}
