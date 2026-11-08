//! Checklists: the card in a bubble (ticked items and who ticked them), the "New checklist" box from
//! the 📎 menu and the "Add items" box, as in Telegram Desktop.

use super::*;
use crate::backend::{NewTodo, TodoInfo};

/// Limits of the official clients (`todo_*_max` in the app config).
const MAX_ITEMS: usize = 30;
const TITLE_MAX: usize = 32;
const ITEM_MAX: usize = 64;

#[derive(Default)]
pub(super) struct Todo {
    /// "New checklist" box.
    form: Option<TodoForm>,
    /// "Add items" box.
    append: Option<Append>,
}

struct TodoForm {
    peer: PeerId,
    title: String,
    /// Ends with one empty row that becomes the next item.
    items: Vec<String>,
    others_can_append: bool,
    others_can_complete: bool,
}

struct Append {
    peer: PeerId,
    id: i32,
    items: Vec<String>,
    /// The first free item id.
    next_id: i32,
    /// How many more the list takes.
    room: usize,
}

/// We may tick items: our own list, or one open for others.
fn can_complete(m: &Msg, t: &TodoInfo) -> bool {
    m.out || t.others_can_complete
}

fn can_append(m: &Msg, t: &TodoInfo) -> bool {
    (m.out || t.others_can_append) && t.items.len() < MAX_ITEMS
}

fn title_galley(ui: &Ui, t: &TodoInfo, w: f32, k: f32) -> std::sync::Arc<egui::Galley> {
    ui.painter().layout(t.title.clone(), FontId::new(15.0 * k, egui::FontFamily::Name(format::BOLD.into())), ink(), w)
}

/// Height of the checklist card in a bubble.
pub(super) fn card_height(ui: &Ui, m: &Msg, t: &TodoInfo, w: f32, k: f32) -> f32 {
    let add = if can_append(m, t) { 40.0 } else { 0.0 };
    title_galley(ui, t, w, k).size().y + (22.0 + t.items.len() as f32 * 40.0 + add) * k
}

pub(super) fn draw_card(ui: &mut Ui, m: &Msg, t: &TodoInfo, rect: Rect, k: f32) -> Option<Action> {
    let p = ui.painter().clone();
    let mut action = None;
    let title = title_galley(ui, t, rect.width(), k);
    let mut y = rect.top();
    let th = title.size().y;
    p.galley(pos2(rect.left(), y), title, ink());
    y += th + 2.0;
    let done = t.items.iter().filter(|i| i.done.is_some()).count();
    let sub = lf!("Чеклист · виконано {done} з {}", "Checklist · {done} of {} done", t.items.len());
    p.galley(pos2(rect.left(), y), elided(ui, &sub, 12.0 * k, rgba(ink(), 160), rect.width()), ink());
    y += 20.0 * k;
    let clickable = can_complete(m, t);
    // The bubble tint differs for our messages: the tick takes the matching contrast colour.
    let (fill, mark) = if m.out { (ink(), glass::chrome()) } else { (ACCENT, glass::ON_DARK) };
    for item in &t.items {
        let r = Rect::from_min_size(pos2(rect.left(), y), vec2(rect.width(), 36.0 * k));
        let resp = ui.interact(r, ui.scope_id().with(("todo", m.id, item.id)), Sense::click());
        if clickable && resp.hovered() {
            p.rect_filled(r, 10.0, glass::veil(18));
            ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
        }
        let c = pos2(r.left() + 12.0 * k, r.center().y);
        if item.done.is_some() {
            p.circle_filled(c, 9.0 * k, fill);
            icons::draw(&p, Rect::from_center_size(c, vec2(12.0, 12.0) * k), Icon::Check, mark);
        } else {
            p.circle_stroke(c, 8.5 * k, egui::Stroke::new(1.5, rgba(ink(), 180)));
        }
        let x = r.left() + 30.0 * k;
        let w = r.right() - x;
        match &item.done {
            Some((who, ts)) => {
                p.galley(pos2(x, r.top() + 1.0), elided(ui, &item.title, 14.5 * k, ink(), w), ink());
                // Who ticked it matters in groups; in our own list it's always us.
                let when = jiff::Timestamp::from_second(*ts).map(|t| t.to_zoned(jiff::tz::TimeZone::system()).strftime("%d.%m %H:%M").to_string()).unwrap_or_default();
                let by = if m.out && !t.others_can_complete { when } else { format!("{who} · {when}") };
                p.galley(pos2(x, r.top() + 20.0 * k), elided(ui, &by, 11.5 * k, rgba(ink(), 150), w), ink());
            }
            None => {
                let g = elided(ui, &item.title, 14.5 * k, ink(), w);
                p.galley(pos2(x, r.center().y - g.size().y / 2.0), g, ink());
            }
        }
        if clickable && resp.clicked() {
            action = Some(Action::ToggleTodo(m.id, item.id));
        }
        y += 40.0 * k;
    }
    if can_append(m, t) {
        let r = Rect::from_min_size(pos2(rect.left(), y), vec2(rect.width(), 36.0 * k));
        let resp = ui.interact(r, ui.scope_id().with(("todo-add", m.id)), Sense::click());
        if resp.hovered() {
            p.rect_filled(r, 10.0, glass::veil(18));
            ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
        }
        icons::draw(&p, Rect::from_center_size(pos2(r.left() + 12.0 * k, r.center().y), vec2(16.0, 16.0) * k), Icon::Plus, glass::link(m.out));
        p.text(pos2(r.left() + 30.0 * k, r.center().y), Align2::LEFT_CENTER, l!("Додати пункт", "Add an item"), FontId::proportional(14.0 * k), glass::link(m.out));
        if resp.clicked() {
            action = Some(Action::AppendTodo(m.id));
        }
    }
    action
}

/// Item rows with one trailing empty row, like the answers of a poll.
fn item_rows(ui: &mut Ui, items: &mut Vec<String>, w: f32, room: usize, salt: &str) {
    let n = items.len();
    let mut remove = None;
    for i in 0..n {
        let (row, _) = ui.allocate_exact_size(vec2(w, 38.0), Sense::hover());
        let mut field = row;
        let last = i + 1 == n;
        if !last {
            field.max.x -= 40.0;
            let x = Rect::from_center_size(pos2(row.right() - 18.0, row.center().y), vec2(32.0, 32.0));
            if glass::icon_button(ui, x, (salt, "remove", i), Icon::Close, false, true).on_hover_text(l!("Прибрати", "Remove")).clicked() {
                remove = Some(i);
            }
        }
        let hint = if last { l!("Додати пункт", "Add an item") } else { l!("Пункт", "Item") };
        let resp = glass::text_field(ui, field, &mut items[i], hint, false);
        if n == 1 && items[i].is_empty() && !ui.memory(|m| m.focused().is_some()) {
            resp.request_focus();
        }
        if items[i].chars().count() > ITEM_MAX {
            items[i] = items[i].chars().take(ITEM_MAX).collect();
        }
    }
    if let Some(i) = remove {
        items.remove(i);
    }
    while items.len() > 1 && items[items.len() - 1].trim().is_empty() && items[items.len() - 2].trim().is_empty() {
        items.pop();
    }
    if items.last().is_none_or(|o| !o.trim().is_empty()) && items.len() < room {
        items.push(String::new());
    }
}

fn filled(items: &[String]) -> Vec<String> {
    items.iter().map(|i| i.trim()).filter(|i| !i.is_empty()).map(str::to_owned).collect()
}

impl App {
    pub(super) fn open_todo_form(&mut self, peer: PeerId) {
        self.todo.form = Some(TodoForm { peer, title: String::new(), items: vec![String::new()], others_can_append: false, others_can_complete: false });
    }

    /// Ticking an item (shown at once) and opening "Add items".
    pub(super) fn todo_action(&mut self, peer: PeerId, action: Action) {
        match action {
            Action::ToggleTodo(id, item) => {
                let me = if self.me.is_empty() { l!("Ви", "You").to_owned() } else { self.me.clone() };
                let Some(task) = self.find_mut(peer, id).and_then(|m| m.todo.as_mut()?.items.iter_mut().find(|i| i.id == item)) else { return };
                let now = jiff::Timestamp::now().as_second();
                let on = task.done.is_none();
                task.done = on.then_some((me, now));
                let (completed, incompleted) = if on { (vec![item], Vec::new()) } else { (Vec::new(), vec![item]) };
                self.send(Cmd::ToggleTodo { peer, id, completed, incompleted });
            }
            Action::AppendTodo(id) => {
                let Some(t) = self.find(peer, id).and_then(|m| m.todo.as_ref()) else { return };
                let next_id = t.items.iter().map(|i| i.id).max().unwrap_or(0) + 1;
                let room = MAX_ITEMS.saturating_sub(t.items.len());
                self.todo.append = Some(Append { peer, id, items: vec![String::new()], next_id, room });
            }
            _ => {}
        }
    }

    /// The checklist boxes.
    pub(super) fn todo_view(&mut self, ui: &mut Ui) {
        if self.todo.form.is_some() {
            self.todo_form_view(ui);
        } else if self.todo.append.is_some() {
            self.todo_append_view(ui);
        }
    }

    fn todo_form_view(&mut self, ui: &mut Ui) {
        let inner = polls::modal(ui, "todo-form", 440.0, 640.0);
        let Some(f) = &mut self.todo.form else { return };
        ui.painter().text(inner.left_top(), Align2::LEFT_TOP, l!("Новий чеклист", "New checklist"), FontId::proportional(17.0), ink());
        let buttons = Rect::from_min_max(pos2(inner.left(), inner.bottom() - 36.0), inner.max);
        let body = Rect::from_min_max(pos2(inner.left() - 4.0, inner.top() + 36.0), pos2(inner.right() + 4.0, buttons.top() - 10.0));
        let mut child = ui.new_child(UiBuilder::new().max_rect(body));
        child.set_clip_rect(body);
        egui::ScrollArea::vertical().id_salt("todo-form").auto_shrink(false).show(&mut child, |ui| {
            ui.spacing_mut().item_spacing.y = 6.0;
            let w = ui.available_width() - 8.0;
            section_title(ui, l!("Назва", "Title"));
            let r = ui.allocate_exact_size(vec2(w, 38.0), Sense::hover()).0;
            let q = glass::text_field(ui, r, &mut f.title, l!("Назва чеклиста", "Checklist title"), false);
            if f.title.is_empty() && !q.has_focus() && !ui.memory(|m| m.focused().is_some()) {
                q.request_focus();
            }
            if f.title.chars().count() > TITLE_MAX {
                f.title = f.title.chars().take(TITLE_MAX).collect();
            }
            section_title(ui, l!("Пункти", "Items"));
            item_rows(ui, &mut f.items, w, MAX_ITEMS, "todo-form");
            let left = MAX_ITEMS.saturating_sub(filled(&f.items).len());
            ui.label(egui::RichText::new(lf!("Можна додати ще {left}", "You can add {left} more")).color(ink_dim()).size(12.5));
            section_title(ui, l!("Налаштування", "Settings"));
            ui.checkbox(&mut f.others_can_append, l!("Інші можуть додавати пункти", "Others can add items"));
            ui.checkbox(&mut f.others_can_complete, l!("Інші можуть позначати виконане", "Others can mark items as done"));
            ui.label(egui::RichText::new(l!("Чеклисти — функція Telegram Premium.", "Checklists are a Telegram Premium feature.")).color(ink_dim()).size(12.5));
        });
        let problem = if f.title.trim().is_empty() {
            Some(l!("Введіть назву", "Enter a title"))
        } else if filled(&f.items).is_empty() {
            Some(l!("Додайте хоча б один пункт", "Add at least one item"))
        } else {
            None
        };
        let half = (buttons.width() - 8.0) / 2.0;
        let esc = ui.input_mut(|i| i.consume_key(egui::Modifiers::NONE, Key::Escape));
        let cancel = glass::button(ui, Rect::from_min_size(buttons.min, vec2(half, buttons.height())), "todo-cancel", l!("Скасувати", "Cancel"), false, true).clicked();
        let create = glass::button(ui, Rect::from_min_size(pos2(buttons.right() - half, buttons.top()), vec2(half, buttons.height())), "todo-create", l!("Створити", "Create"), true, problem.is_none());
        let create = if let Some(p) = problem { create.on_disabled_hover_text(p) } else { create };
        if cancel || esc {
            self.todo.form = None;
        } else if create.clicked() && problem.is_none() {
            let todo = NewTodo { title: f.title.trim().to_owned(), items: filled(&f.items), others_can_append: f.others_can_append, others_can_complete: f.others_can_complete };
            let peer = f.peer;
            self.todo.form = None;
            let reply_to = self.take_reply();
            self.send(Cmd::SendTodo { peer, todo, reply_to });
        }
    }

    fn todo_append_view(&mut self, ui: &mut Ui) {
        let inner = polls::modal(ui, "todo-append", 420.0, 420.0);
        let Some(a) = &mut self.todo.append else { return };
        ui.painter().text(inner.left_top(), Align2::LEFT_TOP, l!("Додати пункти", "Add items"), FontId::proportional(17.0), ink());
        let buttons = Rect::from_min_max(pos2(inner.left(), inner.bottom() - 36.0), inner.max);
        let body = Rect::from_min_max(pos2(inner.left() - 4.0, inner.top() + 36.0), pos2(inner.right() + 4.0, buttons.top() - 10.0));
        let mut child = ui.new_child(UiBuilder::new().max_rect(body));
        child.set_clip_rect(body);
        egui::ScrollArea::vertical().id_salt("todo-append").auto_shrink(false).show(&mut child, |ui| {
            ui.spacing_mut().item_spacing.y = 6.0;
            let w = ui.available_width() - 8.0;
            item_rows(ui, &mut a.items, w, a.room, "todo-append");
        });
        let items = filled(&a.items);
        let half = (buttons.width() - 8.0) / 2.0;
        let esc = ui.input_mut(|i| i.consume_key(egui::Modifiers::NONE, Key::Escape));
        let cancel = glass::button(ui, Rect::from_min_size(buttons.min, vec2(half, buttons.height())), "todo-append-cancel", l!("Скасувати", "Cancel"), false, true).clicked();
        let add = glass::button(ui, Rect::from_min_size(pos2(buttons.right() - half, buttons.top()), vec2(half, buttons.height())), "todo-append-ok", l!("Додати", "Add"), true, !items.is_empty()).clicked();
        if cancel || esc {
            self.todo.append = None;
        } else if add && !items.is_empty() {
            let (peer, id, next) = (a.peer, a.id, a.next_id);
            let items: Vec<(i32, String)> = items.into_iter().enumerate().map(|(i, t)| (next + i as i32, t)).collect();
            // Shown at once; the reloaded message replaces it.
            if let Some(t) = self.find_mut(peer, id).and_then(|m| m.todo.as_mut()) {
                t.items.extend(items.iter().map(|(id, title)| crate::backend::TodoTask { id: *id, title: title.clone(), done: None }));
            }
            self.todo.append = None;
            self.send(Cmd::AppendTodo { peer, id, items });
        }
    }
}
