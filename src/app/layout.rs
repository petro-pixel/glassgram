//! Window layout modes, as in Telegram Desktop: one column at a time in a narrow window, the chat
//! list beside the chat normally, and the chat info as a third column when the window is wide.

use eframe::egui::{Id, Rect, Ui, UiBuilder, pos2, vec2};
use grammers_client::session::types::PeerId;

use super::motion::{self, Spring};
use super::{App, GAP, chatlist, info};
use crate::glass;
use crate::i18n::l;
use crate::icons::Icon;

/// Narrowest chat list beside the chat.
pub(super) const SIDE_MIN_W: f32 = 250.0;
/// Widest chat list beside the chat.
pub(super) const SIDE_MAX_W: f32 = 370.0;
/// Narrowest chat beside the list: below `SIDE_MIN_W + GAP + CHAT_MIN_W` the window shows one column.
pub(super) const CHAT_MIN_W: f32 = 560.0;
/// The info panel is a third column only while the chat beside it keeps this width; otherwise it
/// overlays the chat's right edge.
pub(super) const THIRD_COLUMN_MIN_CHAT: f32 = 420.0;
/// Room for the back arrow before the avatar in a narrow window's chat header.
pub(super) const BACK_W: f32 = 44.0;

/// The mode chosen in the last frame; the chat header reads it to show the back arrow.
#[derive(Default)]
pub(super) struct Layout {
    /// One column: the chat list alone, or the open chat alone.
    pub(super) narrow: bool,
    /// The chat open in the last frame, and the one sliding out after "back" in a narrow window.
    last: Option<PeerId>,
    leaving: Option<PeerId>,
}

impl App {
    pub(super) fn main_view(&mut self, ui: &mut Ui) {
        let mut full = ui.max_rect().shrink(GAP);
        let folder_column = self.settings.folders_side && self.has_folder_bar();
        let list_w = if folder_column { full.width() - chatlist::COLUMN_W - GAP } else { full.width() };
        self.layout.narrow = list_w < SIDE_MIN_W + GAP + CHAT_MIN_W;
        self.sync_info();
        if self.layout.narrow {
            self.narrow_view(ui, full, folder_column);
            return;
        }
        if folder_column {
            let column = Rect::from_min_size(full.min, vec2(chatlist::COLUMN_W, full.height()));
            self.folder_column(ui, column);
            full.min.x = column.right() + GAP;
        }
        let side_w = (full.width() * 0.34).clamp(SIDE_MIN_W, SIDE_MAX_W);
        let side = Rect::from_min_size(full.min, vec2(side_w, full.height()));
        let mut chat = Rect::from_min_max(pos2(side.right() + GAP, full.top()), full.max);
        self.pane(ui, "side", side, |app, ui| app.sidebar(ui, side));
        // Info column: beside the chat when there is room, otherwise over its right edge.
        let wide = chat.width() - info::INFO_W - GAP >= THIRD_COLUMN_MIN_CHAT;
        let open = self.slide(ui, "info-column", self.info.is_some());
        let shift = (1.0 - open) * (info::INFO_W + GAP);
        let info = Rect::from_min_max(pos2(chat.right() - info::INFO_W.min(chat.width()), chat.top()), chat.max).translate(vec2(shift, 0.0));
        if wide {
            // The chat gives way as the column slides in, and takes the room back as it leaves.
            chat.max.x -= open * (info::INFO_W + GAP);
        }
        self.pane(ui, "chat", chat, |app, ui| app.chat_area(ui, chat));
        if self.info.is_some() {
            self.info_view(ui, info);
        }
    }

    /// One column: the list, or the open chat pushing it out to the left.
    fn narrow_view(&mut self, ui: &mut Ui, full: Rect, folder_column: bool) {
        if self.selected.is_none() && self.layout.last.is_some() {
            self.layout.leaving = self.layout.last;
        }
        self.layout.last = self.selected;
        let t = self.slide(ui, "narrow-chat", self.selected.is_some());
        let push = vec2(-t * (full.width() + GAP), 0.0);
        if t < 1.0 {
            let mut list = full.translate(push);
            if folder_column {
                let column = Rect::from_min_size(list.min, vec2(chatlist::COLUMN_W, list.height()));
                self.folder_column(ui, column);
                list.min.x = column.right() + GAP;
            }
            self.pane(ui, "side", list, |app, ui| app.sidebar(ui, list));
        }
        let chat = full.translate(push + vec2(full.width() + GAP, 0.0));
        if t > 0.0 && self.selected.is_some() {
            self.layout.leaving = None;
            self.pane(ui, "chat", chat, |app, ui| app.chat_area(ui, chat));
            let open = self.slide(ui, "info-overlay", self.info.is_some());
            if self.info.is_some() {
                let w = info::INFO_W.min(chat.width());
                let r = Rect::from_min_max(pos2(chat.right() - w, chat.top()), chat.max).translate(vec2((1.0 - open) * (w + GAP), 0.0));
                self.info_view(ui, r);
            }
        } else if t > 0.0 && let Some(peer) = self.layout.leaving {
            self.leaving_chat(ui, peer, chat, t);
        } else {
            self.layout.leaving = None;
        }
    }

    /// The chat we went back from, still sliding out: drawn as it was, without input, fading.
    fn leaving_chat(&mut self, ui: &mut Ui, peer: PeerId, rect: Rect, t: f32) {
        if !self.chats.iter().any(|c| c.id == peer) {
            return;
        }
        self.selected = Some(peer);
        let mut child = ui.new_child(UiBuilder::new().id_salt("chat").max_rect(rect));
        child.disable();
        child.set_opacity(0.4 + 0.6 * t);
        self.chat_area(&mut child, rect);
        self.selected = None;
    }

    /// A column drawn in its own child `Ui` with a fixed id, so scroll positions and other widget
    /// state survive switching between the layout modes.
    fn pane(&mut self, ui: &mut Ui, id: &str, rect: Rect, draw: impl FnOnce(&mut Self, &mut Ui)) {
        let mut child = ui.new_child(UiBuilder::new().id_salt(id).max_rect(rect));
        draw(self, &mut child);
    }

    /// 0 → 1 as `on` turns true, on a spring without overshoot; instant with "Reduce motion".
    fn slide(&self, ui: &Ui, id: &str, on: bool) -> f32 {
        motion::follow(ui.ctx(), Id::unique(("layout", id)), if on { 1.0 } else { 0.0 }, 0.001, Spring::SMOOTH).clamp(0.0, 1.0)
    }

    /// The back arrow at the left of a chat header in a narrow window; returns whether it was drawn.
    pub(super) fn back_to_list(&mut self, ui: &mut Ui, header: Rect) -> bool {
        if !self.layout.narrow {
            return false;
        }
        let back = Rect::from_center_size(pos2(header.left() + 30.0, header.center().y), vec2(40.0, 40.0));
        if glass::icon_button(ui, back, "chat-back", Icon::ChevronLeft, false, true).on_hover_text(l!("До списку чатів", "Back to chats")).clicked() {
            self.close_chat();
        }
        true
    }
}
