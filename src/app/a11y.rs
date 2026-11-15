//! Accessibility: names for custom-painted rows (AccessKit reads them to screen readers) and the
//! keyboard glue egui lacks — Esc closing a menu before anything under it, arrows entering an
//! open menu, and a visible focus ring on every focusable widget while navigating by keyboard.

use eframe::egui::{self, Key, Modifiers, Response, Role, StrokeKind, WidgetInfo};

use super::{App, l, lf, lp};
use crate::backend::{ChatInfo, Msg};
use crate::glass;

impl App {
    /// Start of the frame, before `hotkeys`: an open popup takes Esc first, so one press closes the
    /// menu and not also the chat under it.
    pub(super) fn a11y_keys(&self, ctx: &egui::Context) {
        let (nav, pointer) = ctx.input(|i| {
            let nav = [Key::Tab, Key::ArrowUp, Key::ArrowDown, Key::ArrowLeft, Key::ArrowRight].iter().any(|k| i.key_pressed(*k));
            (nav, i.pointer.any_pressed())
        });
        if pointer {
            glass::set_keyboard(false);
        } else if nav && !ctx.input(|i| i.modifiers.command || i.modifiers.alt) {
            glass::set_keyboard(true);
        }
        if egui::Popup::is_any_open(ctx) && ctx.input_mut(|i| i.consume_key(Modifiers::NONE, Key::Escape)) {
            egui::Popup::close_all(ctx);
        }
    }

    /// End of the frame, when every widget has its rect: arrows step into an open menu that has no
    /// focus yet, and the focused widget gets its ring.
    pub(super) fn a11y_after(&self, ctx: &egui::Context) {
        let focused = ctx.memory(|m| m.focused());
        if focused.is_none() && egui::Popup::is_any_open(ctx) {
            let (down, up) = ctx.input(|i| (i.key_pressed(Key::ArrowDown), i.key_pressed(Key::ArrowUp)));
            if (down || up)
                && let Some(id) = menu_item(ctx, down)
            {
                ctx.memory_mut(|m| m.request_focus(id));
                glass::set_keyboard(true);
            }
        }
        // Text fields draw their own focus outline.
        if !glass::keyboard() || ctx.text_edit_focused() {
            return;
        }
        let Some(w) = focused.and_then(|id| ctx.viewport(|v| v.this_pass.widgets.get(id).copied())) else { return };
        if !w.visible || w.interact_rect.area() <= 0.0 {
            return;
        }
        // Just outside the widget, so the glass rim under it doesn't swallow the ring.
        let r = w.interact_rect.expand(1.5);
        let radius = (r.height() / 2.0).min(20.0);
        ctx.layer_painter(w.layer_id).rect_stroke(r, radius, egui::Stroke::new(2.5, glass::ACCENT), StrokeKind::Outside);
    }
}

/// The first (or last) focusable item of the topmost open menu.
fn menu_item(ctx: &egui::Context, first: bool) -> Option<egui::Id> {
    let layers: Vec<egui::LayerId> = ctx.memory(|m| m.layer_ids().filter(|l| l.order == egui::Order::Foreground).collect());
    ctx.viewport(|v| {
        layers.iter().rev().find_map(|layer| {
            let mut items = v.this_pass.widgets.get_layer(*layer).filter(|w| w.enabled && w.visible && w.sense.is_focusable());
            if first { items.next() } else { items.last() }.map(|w| w.id)
        })
    })
}

/// Keeps the keyboard-focused row on screen inside its scroll area.
fn follow(resp: &Response) {
    if resp.gained_focus() && glass::keyboard() {
        resp.scroll_to_me(None);
    }
}

/// A chat list row: name, unread count and the last message, as Telegram Desktop announces it.
pub(super) fn describe_chat(resp: &Response, chat: &ChatInfo, selected: bool) {
    follow(resp);
    resp.widget_info(|| {
        let mut label = chat.title.clone();
        if chat.unread > 0 {
            let n = chat.unread;
            label += ", ";
            label += &lp!(n, ["{n} непрочитане", "{n} непрочитані", "{n} непрочитаних"], ["{n} unread message", "{n} unread messages"]);
        } else if chat.unread_mark {
            label += ", ";
            label += l!("позначено непрочитаним", "marked as unread");
        }
        if !chat.last_text.is_empty() {
            let text = chat.last_text.replace('\n', " ");
            label += ". ";
            label += &lf!("Останнє: {text}", "Last message: {text}");
        }
        WidgetInfo::selected(Role::ListItem, true, selected, label)
    });
}

/// A message bubble: author, text (or what the media is) and time.
pub(super) fn describe_msg(resp: &Response, m: &Msg) {
    follow(resp);
    resp.widget_info(|| {
        let who = super::author(m, "");
        let text = super::chatview::preview(m);
        let time = &m.time;
        WidgetInfo::labeled(Role::ListItem, true, lf!("{who}: {text}. Надіслано о {time}", "{who}: {text}. Sent at {time}"))
    });
}

/// A dialog's title read out as its heading; custom-painted boxes have no window title of their own.
pub(super) fn title(ui: &egui::Ui, rect: egui::Rect, id: &str, text: &str) {
    let resp = ui.interact(rect, ui.scope_id().with((id, "a11y-title")), egui::Sense::hover());
    resp.widget_info(|| WidgetInfo::labeled(Role::Heading, true, text));
}
