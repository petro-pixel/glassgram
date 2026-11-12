//! The call log, as Telegram Desktop's "Calls" box: calls from every private chat, newest first,
//! consecutive calls with one person of one kind on one day merged into a row with a count.
//! A click opens the chat at the call, the context menu deletes it. Calls themselves aren't
//! implemented yet, so the history bubble of a call is a card without a "call back" button.

use std::ops::Range;

use super::*;
use crate::backend::{CallDir, CallInfo};

/// The "Calls" box.
#[derive(Default)]
pub(super) struct Calls {
    open: bool,
    /// Every loaded call, newest first, each with its chat.
    list: Vec<SearchHit>,
    /// A page is being requested.
    loading: bool,
    /// The server has older calls.
    more: bool,
    /// A row waiting for the delete confirmation: its calls, the person, "for them too".
    delete: Option<(Vec<(PeerId, i32)>, String, bool)>,
}

/// The arrow and its colour: red for a missed call, as in Telegram.
fn arrow(dir: CallDir) -> (Icon, Color32) {
    match dir {
        CallDir::Out => (Icon::ArrowUpRight, ink_dim()),
        CallDir::In => (Icon::ArrowDownLeft, ink_dim()),
        CallDir::Missed => (Icon::ArrowDownLeft, glass::danger()),
    }
}

/// Rows of the box: consecutive calls with the same person, direction and day.
fn rows(list: &[SearchHit]) -> Vec<Range<usize>> {
    let key = |h: &SearchHit| (h.msg.peer, h.msg.call.map(|c| c.dir(h.msg.out)), h.msg.day.clone());
    let mut rows: Vec<Range<usize>> = Vec::new();
    for (i, h) in list.iter().enumerate() {
        match rows.last_mut() {
            Some(r) if key(&list[r.start]) == key(h) => r.end = i + 1,
            _ => rows.push(i..i + 1),
        }
    }
    rows
}

impl App {
    /// Opens the box with a fresh first page.
    pub(super) fn open_calls(&mut self) {
        self.calls.open = true;
        self.calls.loading = true;
        self.send(Cmd::LoadCalls { offset_id: 0 });
    }

    pub(super) fn calls_event(&mut self, ev: Ev) {
        match ev {
            Ev::CallLog { calls, append, more } => {
                let c = &mut self.calls;
                if !append {
                    c.list.clear();
                }
                c.list.extend(calls);
                c.more = more;
                c.loading = false;
            }
            Ev::CallsDeleted(ids) => {
                self.calls.list.retain(|h| !ids.contains(&(h.msg.peer, h.msg.id)));
                for (peer, id) in ids {
                    if let Some(list) = self.history.get_mut(&peer) {
                        list.retain(|m| m.id != id);
                    }
                }
            }
            _ => {}
        }
    }

    pub(super) fn calls_view(&mut self, ui: &mut Ui) {
        if !self.calls.open {
            return;
        }
        let screen = ui.max_rect();
        ui.painter().rect_filled(screen, 0.0, Color32::from_black_alpha(90));
        if ui.interact(screen, ui.scope_id().with("calls-bg"), Sense::click()).clicked() && self.calls.delete.is_none() {
            self.calls.open = false;
            return;
        }
        let rect = Rect::from_center_size(screen.center(), vec2(screen.width().min(440.0) - 20.0, (screen.height() - 60.0).min(620.0)));
        glass::glass(ui.painter(), rect, Glass::panel(26.0).with_shadow());
        ui.interact(rect, ui.scope_id().with("calls-panel"), Sense::click());
        let inner = rect.shrink(18.0);
        ui.painter().text(inner.left_top() + vec2(0.0, 12.0), Align2::LEFT_CENTER, l!("Дзвінки", "Calls"), FontId::proportional(17.0), ink());
        let close = Rect::from_min_size(pos2(inner.right() - 32.0, inner.top() - 4.0), vec2(32.0, 32.0));
        let esc = self.calls.delete.is_none() && ui.input_mut(|i| i.consume_key(egui::Modifiers::NONE, Key::Escape));
        if glass::icon_button(ui, close, "calls-close", Icon::Close, false, true).clicked() || esc {
            self.calls = Calls::default();
            return;
        }
        let body = Rect::from_min_max(pos2(inner.left() - 6.0, inner.top() + 40.0), pos2(inner.right() + 6.0, inner.bottom()));
        let mut child = ui.new_child(UiBuilder::new().max_rect(body));
        child.set_clip_rect(body);
        let (mut open, mut delete, mut more) = (None, None, false);
        let calls = &self.calls;
        let textures = &self.textures;
        egui::ScrollArea::vertical().id_salt("calls").auto_shrink(false).show(&mut child, |ui| {
            ui.spacing_mut().item_spacing.y = 2.0;
            if calls.list.is_empty() {
                if calls.loading {
                    ui.add(egui::Spinner::new().color(ink_dim()));
                } else {
                    ui.add_space(24.0);
                    ui.vertical_centered(|ui| {
                        icons::draw(ui.painter(), Rect::from_center_size(ui.cursor().center_top() + vec2(0.0, 18.0), vec2(36.0, 36.0)), Icon::Phone, ink_dim());
                        ui.add_space(44.0);
                        ui.label(egui::RichText::new(l!("Ви ще не дзвонили в Telegram.", "You haven't made any Telegram calls yet.")).color(ink_dim()).size(13.5));
                    });
                }
            }
            let rows = rows(&calls.list);
            let last = rows.len().saturating_sub(1);
            for (k, r) in rows.into_iter().enumerate() {
                let group = &calls.list[r.clone()];
                let resp = call_row(ui, group, textures.get(&ImgKey::Avatar(group[0].msg.peer)));
                if resp.clicked() {
                    open = Some(group[0].clone());
                }
                resp.context_menu(|ui| {
                    if glass::menu_button(ui, Icon::Trash, l!("Видалити", "Delete")).clicked() {
                        delete = Some((group.iter().map(|h| (h.msg.peer, h.msg.id)).collect(), group[0].chat.clone()));
                        ui.close();
                    }
                });
                // The last row came into view: the next page.
                if k == last && calls.more && !calls.loading && ui.is_rect_visible(resp.rect) {
                    more = true;
                }
            }
            if calls.loading && !calls.list.is_empty() {
                ui.add(egui::Spinner::new().color(ink_dim()));
            }
        });
        if more {
            let offset_id = self.calls.list.last().map_or(0, |h| h.msg.id);
            self.calls.loading = true;
            self.send(Cmd::LoadCalls { offset_id });
        }
        if let Some((ids, name)) = delete {
            self.calls.delete = Some((ids, name, false));
        }
        if let Some(hit) = open {
            self.calls.open = false;
            self.jump_to(&hit);
        }
        self.calls_delete_ask(ui);
    }

    /// "Delete call?" with "Also delete for <name>", as Telegram asks for messages.
    fn calls_delete_ask(&mut self, ui: &mut Ui) {
        let Some((ids, name, revoke)) = &mut self.calls.delete else { return };
        let n = ids.len();
        let title = if n == 1 { l!("Видалити дзвінок?", "Delete call?").to_owned() } else { lp!(n, ["Видалити {n} дзвінок?", "Видалити {n} дзвінки?", "Видалити {n} дзвінків?"], ["Delete {n} call?", "Delete {n} calls?"]) };
        let body = l!("Запис зникне з журналу дзвінків і з історії чату.", "The entry will disappear from the call log and the chat history.");
        let check = lf!("Також видалити для {name}", "Also delete for {name}");
        let buttons = [l!("Скасувати", "Cancel"), l!("Видалити", "Delete")];
        let Some(i) = confirm::ask_box(ui, "calls-delete", &title, body, Some((&check, revoke)), &buttons) else { return };
        let Some((ids, _, revoke)) = self.calls.delete.take() else { return };
        if i == 1 {
            self.send(Cmd::DeleteCalls { ids, revoke });
        }
    }
}

/// A row of the box: avatar, name with the count, arrow, date and the call kind on the right.
fn call_row(ui: &mut Ui, group: &[SearchHit], avatar: Option<&TextureHandle>) -> egui::Response {
    let first = &group[0];
    let Some(call) = first.msg.call else { return ui.allocate_response(vec2(0.0, 0.0), Sense::hover()) };
    let (rect, resp) = ui.allocate_exact_size(vec2(ui.available_width(), 56.0), Sense::click());
    let hover = ui.ctx().animate_bool(resp.id, resp.hovered());
    let p = ui.painter();
    if hover > 0.0 {
        p.rect_filled(rect, 16.0, glass::veil((hover * 16.0) as u8));
    }
    let seed = first.msg.peer.bot_api_dialog_id_unchecked();
    glass::avatar(p, pos2(rect.left() + 30.0, rect.center().y), 19.0, &first.chat, seed, None, avatar);
    let x = rect.left() + 58.0;
    let w = rect.right() - x - 52.0;
    let name = if group.len() > 1 { format!("{} ({})", first.chat, group.len()) } else { first.chat.clone() };
    let dir = call.dir(first.msg.out);
    let name_color = if dir == CallDir::Missed { glass::danger() } else { ink() };
    p.galley(pos2(x, rect.top() + 9.0), elided(ui, &name, 14.5, name_color, w), name_color);
    let (icon, color) = arrow(dir);
    icons::draw(p, Rect::from_min_size(pos2(x - 1.0, rect.top() + 31.0), vec2(15.0, 15.0)), icon, color);
    let mut status = format!("{}, {} {}", call.label(first.msg.out), first.msg.day, first.msg.time);
    let duration = call.duration_text();
    if group.len() == 1 && !duration.is_empty() {
        status = format!("{status} · {duration}");
    }
    p.galley(pos2(x + 18.0, rect.top() + 31.0), elided(ui, &status, 12.5, ink_dim(), w - 18.0), ink_dim());
    let kind = if call.video { Icon::Video } else { Icon::Phone };
    icons::draw(p, Rect::from_center_size(pos2(rect.right() - 26.0, rect.center().y), vec2(20.0, 20.0)), kind, ink_dim());
    if resp.hovered() {
        ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
    }
    resp
}

/// Height of the call card in a bubble.
pub(super) fn card_height(k: f32) -> f32 {
    44.0 * k
}

/// The call bubble, as in Telegram: the kind of call, then the arrow with the duration or time.
pub(super) fn draw_card(ui: &Ui, m: &Msg, call: CallInfo, rect: Rect, k: f32) {
    let p = ui.painter();
    let icon_w = 40.0 * k;
    let w = rect.width() - icon_w;
    p.galley(pos2(rect.left(), rect.top() + 2.0 * k), elided(ui, call.label(m.out), 15.0 * k, ink(), w), ink());
    let (icon, color) = arrow(call.dir(m.out));
    icons::draw(p, Rect::from_min_size(pos2(rect.left() - 1.0, rect.top() + 24.0 * k), vec2(15.0 * k, 15.0 * k)), icon, color);
    let duration = call.duration_text();
    let sub = if duration.is_empty() { m.time.clone() } else { format!("{}, {duration}", m.time) };
    p.galley(pos2(rect.left() + 18.0 * k, rect.top() + 24.0 * k), elided(ui, &sub, 12.5 * k, rgba(ink(), 170), w - 18.0 * k), ink());
    let kind = if call.video { Icon::Video } else { Icon::Phone };
    let r = Rect::from_center_size(pos2(rect.right() - icon_w / 2.0, rect.center().y), vec2(24.0 * k, 24.0 * k));
    icons::draw(p, r, kind, rgba(ink(), 200));
}
