//! Chat list: the folder column, dragging pinned chats and folders.
//! A child module of `app`, so it works with `App` fields directly.

use eframe::egui::{self, Align2, Color32, FontId, Id, Rect, Sense, Ui, UiBuilder, pos2, vec2};
use grammers_client::session::types::PeerId;

use super::{App, ARCHIVE, FolderMenu, FolderEdit, TABS_H, chat_row, elided, section_title};
use crate::backend::{ChatInfo, Cmd, Ev, Folder, ImgKey};
use crate::glass::{self, ACCENT, Glass, ink, ink_dim, rgba};
use crate::i18n::l;
use crate::icons::{self, Icon};

/// Chat row height (as in `chat_row`) and the gap between rows.
const ROW_GAP: f32 = 4.0;
/// Folder column width and the height of one item.
pub(super) const COLUMN_W: f32 = 76.0;
const CELL_H: f32 = 62.0;
const CELL_GAP: f32 = 4.0;
const TAB_GAP: f32 = 6.0;

/// Drag in a list: what is dragged, where it was grabbed and the live order.
pub(super) struct Reorder<K> {
    key: K,
    /// Distance from the item start to the pointer along the axis.
    grab: f32,
    order: Vec<K>,
}

impl<K: Copy + PartialEq> Reorder<K> {
    /// Moves the item to where its centre now is (`pos` is the item start along the axis).
    fn follow(&mut self, pos: f32, size: impl Fn(K) -> f32, gap: f32) {
        let center = pos + size(self.key) / 2.0;
        loop {
            let Some(i) = self.order.iter().position(|k| *k == self.key) else { return };
            let mut starts = Vec::with_capacity(self.order.len());
            let mut x = 0.0;
            for k in &self.order {
                starts.push(x);
                x += size(*k) + gap;
            }
            let mid = |j: usize| starts[j] + size(self.order[j]) / 2.0;
            if i > 0 && center <= mid(i - 1) {
                self.order.swap(i, i - 1);
            } else if i + 1 < self.order.len() && center >= mid(i + 1) {
                self.order.swap(i, i + 1);
            } else {
                return;
            }
        }
    }
}

/// Where the press on `grip` began, once the pointer has moved far enough to be a drag, not a click.
/// The grip lies under its item: a drag-only widget on top would take every press, so the item would
/// never be clicked or get its context menu, and a plain click would lift it.
fn pulled(ui: &Ui, grip: &egui::Response) -> Option<egui::Pos2> {
    let pulling = grip.dragged_by(egui::PointerButton::Primary) && ui.input(|i| i.pointer.is_decidedly_dragging());
    pulling.then(|| ui.input(|i| i.pointer.press_origin())).flatten()
}

/// Spring-like approach to `target`: moves at once and settles softly (~200 ms).
/// Positions are relative to the list start, so scrolling does not animate them.
fn glide(ui: &Ui, id: Id, target: f32) -> f32 {
    let dt = ui.input(|i| i.stable_dt).min(0.05);
    let cur = ui.ctx().data_mut(|d| *d.get_temp_mut_or_insert_with(id, || target));
    let mut next = target + (cur - target) * (-dt * 18.0).exp();
    if (next - target).abs() < 0.3 {
        next = target;
    } else {
        ui.ctx().request_repaint();
    }
    ui.ctx().data_mut(|d| d.insert_temp(id, next));
    next
}

fn set_glide(ui: &Ui, id: Id, value: f32) {
    ui.ctx().data_mut(|d| d.insert_temp(id, value));
}

/// Backing for a lifted item: dense, so rows below do not show through.
fn lift(ui: &Ui, rect: Rect, radius: f32) {
    let p = ui.painter();
    p.rect_filled(rect.translate(vec2(0.0, 3.0)).expand(1.0), radius, Color32::from_black_alpha(40));
    p.rect_filled(rect, radius, rgba(glass::chrome(), 240));
}

/// What happened in the chat list this frame.
#[derive(Default)]
pub(super) struct RowsOut {
    pub clicked: Option<PeerId>,
    pub menu: Option<(PeerId, super::ChatAction)>,
    /// A dragged chat was released: the new pinned order.
    pub dropped: Option<Vec<PeerId>>,
}

/// Folder icon from its emoji or rules, as in Telegram Desktop; `None` means draw the emoji itself.
fn folder_icon(id: Option<i32>, f: Option<&Folder>) -> Option<Icon> {
    match id {
        None => return Some(Icon::Chats),
        Some(ARCHIVE) => return Some(Icon::Archive),
        _ => {}
    }
    let f = f?;
    match f.emoticon.as_str() {
        "💬" => return Some(Icon::Chats),
        "👤" => return Some(Icon::User),
        "👥" => return Some(Icon::Users),
        "📢" => return Some(Icon::Channel),
        "🤖" => return Some(Icon::Bot),
        "✅" => return Some(Icon::Check),
        "🔔" => return Some(Icon::Bell),
        "📁" | "" => {}
        _ => return None,
    }
    let only = |a: bool, rest: &[bool]| a && rest.iter().all(|x| !x) && f.include.is_empty();
    Some(if f.exclude_read {
        Icon::Check
    } else if only(f.contacts || f.non_contacts, &[f.groups, f.broadcasts, f.bots]) {
        Icon::User
    } else if only(f.groups, &[f.contacts, f.non_contacts, f.broadcasts, f.bots]) {
        Icon::Users
    } else if only(f.broadcasts, &[f.contacts, f.non_contacts, f.groups, f.bots]) {
        Icon::Channel
    } else if only(f.bots, &[f.contacts, f.non_contacts, f.groups, f.broadcasts]) {
        Icon::Bot
    } else {
        Icon::Folder
    })
}

/// As in Telegram: pinned chats keep their own order on top, the rest go by time.
pub(super) fn sort_chats(chats: &mut [ChatInfo]) {
    // A stable sort keeps the pinned order intact.
    chats.sort_by_key(|c| (!c.pinned, if c.pinned { 0 } else { -c.last_ts }));
}

impl App {
    /// Whether there is anything to show as folders (a folder or the archive).
    pub(super) fn has_folder_bar(&self) -> bool {
        !self.folders.is_empty() || self.chats.iter().any(|c| c.archived)
    }

    /// Folder items: "All chats" first, the archive last; the live order while dragging.
    fn folder_items(&self) -> Vec<(Option<i32>, String, usize)> {
        let mut folders: Vec<&Folder> = self.folders.iter().collect();
        if let Some(d) = &self.folder_drag {
            folders.sort_by_key(|f| d.order.iter().position(|id| *id == f.id));
        }
        let mut items = vec![(None, l!("Усі чати", "All chats").to_owned(), self.folder_unread(None))];
        for f in folders {
            items.push((Some(f.id), f.title.clone(), self.folder_unread(Some(f.id))));
        }
        if self.chats.iter().any(|c| c.archived) {
            items.push((Some(ARCHIVE), l!("Архів", "Archive").to_owned(), self.folder_unread(Some(ARCHIVE))));
        }
        items
    }

    fn folder_menu(ui: &mut Ui, id: Option<i32>, menu: &mut Option<FolderMenu>) {
        if let Some(fid) = id.filter(|x| *x != ARCHIVE) {
            if glass::menu_button(ui, Icon::Pencil, l!("Редагувати папку", "Edit folder")).clicked() {
                *menu = Some(FolderMenu::Edit(fid));
            }
            if glass::menu_button(ui, Icon::Trash, l!("Видалити папку", "Delete folder")).clicked() {
                *menu = Some(FolderMenu::Delete(fid));
            }
        }
        if glass::menu_button(ui, Icon::FolderPlus, l!("Нова папка", "New folder")).clicked() {
            *menu = Some(FolderMenu::New);
        }
    }

    fn apply_folder_menu(&mut self, menu: Option<FolderMenu>) {
        match menu {
            Some(FolderMenu::New) => self.new_folder(),
            Some(FolderMenu::Edit(id)) => {
                if let Some(f) = self.folders.iter().find(|f| f.id == id) {
                    self.folder_edit = Some(FolderEdit { folder: f.clone(), new: false, query: String::new() });
                }
            }
            Some(FolderMenu::Delete(id)) => self.send(Cmd::DeleteFolder(id)),
            None => {}
        }
    }

    /// Folders were dropped in a new place: keep the order locally and on the server.
    fn drop_folders(&mut self, order: Vec<i32>) {
        let before: Vec<i32> = self.folders.iter().map(|f| f.id).collect();
        if before == order {
            return;
        }
        self.folders.sort_by_key(|f| order.iter().position(|id| *id == f.id));
        self.send(Cmd::ReorderFolders(order));
    }

    /// Starts dragging a folder if its grip was pulled.
    fn grab_folder(&mut self, ui: &Ui, grip: &egui::Response, id: Option<i32>, start: f32, vertical: bool) {
        let Some(fid) = id.filter(|x| *x != ARCHIVE) else { return };
        if self.folder_drag.is_none()
            && let Some(p) = pulled(ui, grip)
        {
            let at = if vertical { p.y } else { p.x };
            self.folder_drag = Some(Reorder { key: fid, grab: at - start, order: self.folders.iter().map(|f| f.id).collect() });
        }
    }

    /// Folder tabs above the list; folders can be dragged.
    pub(super) fn folder_tabs(&mut self, ui: &mut Ui, rect: Rect) {
        let items = self.folder_items();
        let mut child = ui.new_child(UiBuilder::new().max_rect(rect));
        child.set_clip_rect(rect.expand2(vec2(0.0, 4.0)));
        let mut chosen = None;
        let mut menu = None;
        let mut grabs = Vec::new();
        let mut lifted = None;
        let mut drop = None;
        let mut origin = 0.0;
        let label = |title: &str, unread: usize| if unread > 0 { format!("{title}  {unread}") } else { title.to_owned() };
        egui::ScrollArea::horizontal()
            .id_salt("folder-tabs")
            .scroll_bar_visibility(egui::scroll_area::ScrollBarVisibility::AlwaysHidden)
            .show(&mut child, |ui| {
                ui.horizontal(|ui| {
                    ui.spacing_mut().item_spacing.x = TAB_GAP;
                    origin = ui.cursor().left();
                    for (id, title, unread) in &items {
                        let text = label(title, *unread);
                        let w = elided(ui, &text, 14.0, ink(), 400.0).size().x + 26.0;
                        let (slot, _) = ui.allocate_exact_size(vec2(w, TABS_H), Sense::hover());
                        let gid = ui.scope_id().with(("tab-x", *id));
                        let rel = slot.left() - origin;
                        if self.folder_drag.as_ref().is_some_and(|d| Some(d.key) == *id) {
                            lifted = Some((*id, text, slot));
                            continue;
                        }
                        let r = slot.translate(vec2(glide(ui, gid, rel) - rel, 0.0));
                        let grip = ui.interact(r, gid.with("drag"), Sense::drag());
                        grabs.push((grip, *id, r.left()));
                        let resp = glass::button(ui, r, ("folder", *id), &text, self.folder == *id, true);
                        if resp.clicked() {
                            chosen = Some(*id);
                        }
                        resp.context_menu(|ui| Self::folder_menu(ui, *id, &mut menu));
                    }
                    let (r, _) = ui.allocate_exact_size(vec2(TABS_H, TABS_H), Sense::hover());
                    if glass::icon_button(ui, r, "folder-new", Icon::Plus, false, true).on_hover_text(l!("Нова папка", "New folder")).clicked() {
                        menu = Some(FolderMenu::New);
                    }
                    // The lifted tab is drawn last, on top of its neighbours.
                    if let Some((id, text, slot)) = lifted.take() {
                        let tab_w = |t: &str, u: usize| elided(ui, &label(t, u), 14.0, ink(), 400.0).size().x + 26.0;
                        // Only folders move: "All chats" stays first, the archive stays last.
                        let first = items.first().map_or(0.0, |(_, t, u)| tab_w(t, *u) + TAB_GAP);
                        let widths: Vec<(i32, f32)> = items.iter().filter_map(|(id, t, u)| id.filter(|x| *x != ARCHIVE).map(|id| (id, tab_w(t, *u)))).collect();
                        let span: f32 = widths.iter().map(|(_, w)| w + TAB_GAP).sum::<f32>() - TAB_GAP;
                        let gid = ui.scope_id().with(("tab-x", id));
                        let grip = ui.interact(slot, gid.with("drag"), Sense::drag());
                        if let (Some(d), Some(p)) = (self.folder_drag.as_mut(), ui.input(|i| i.pointer.interact_pos())) {
                            let pos = (p.x - d.grab - origin).clamp(first, first + span - slot.width());
                            d.follow(pos - first, |k| widths.iter().find(|(i, _)| *i == k).map_or(0.0, |(_, w)| *w), TAB_GAP);
                            set_glide(ui, gid, pos);
                            let r = Rect::from_min_size(pos2(origin + pos, slot.top()), slot.size());
                            glass::button(ui, r, ("folder-lift", id), &text, true, true);
                        }
                        if !grip.dragged() {
                            drop = self.folder_drag.take().map(|d| d.order);
                        }
                    }
                });
            });
        for (grip, id, start) in grabs {
            self.grab_folder(ui, &grip, id, start, false);
        }
        if let Some(order) = drop {
            self.drop_folders(order);
        }
        if let Some(id) = chosen {
            self.folder = id;
        }
        self.apply_folder_menu(menu);
    }

    /// Vertical folder column left of the chat list, as in Telegram Desktop.
    pub(super) fn folder_column(&mut self, ui: &mut Ui, rect: Rect) {
        glass::glass(ui.painter(), rect, Glass::panel(24.0));
        let items = self.folder_items();
        let inner = rect.shrink2(vec2(6.0, 12.0));
        let add = Rect::from_center_size(pos2(inner.center().x, inner.bottom() - 20.0), vec2(40.0, 40.0));
        let list = Rect::from_min_max(inner.min, pos2(inner.right(), add.top() - 8.0));
        let mut child = ui.new_child(UiBuilder::new().max_rect(list));
        child.set_clip_rect(list);
        let mut chosen = None;
        let mut menu = None;
        let mut grabs = Vec::new();
        let mut drop = None;
        let folders = self.folders.len() as f32;
        egui::ScrollArea::vertical()
            .id_salt("folder-column")
            .scroll_bar_visibility(egui::scroll_area::ScrollBarVisibility::AlwaysHidden)
            .auto_shrink(false)
            .show(&mut child, |ui| {
                ui.spacing_mut().item_spacing.y = CELL_GAP;
                let origin = ui.cursor().top();
                let mut lifted = None;
                for (id, title, unread) in &items {
                    let (slot, _) = ui.allocate_exact_size(vec2(ui.available_width(), CELL_H), Sense::hover());
                    let gid = ui.scope_id().with(("cell-y", *id));
                    let rel = slot.top() - origin;
                    if self.folder_drag.as_ref().is_some_and(|d| Some(d.key) == *id) {
                        lifted = Some((*id, title, *unread, slot));
                        continue;
                    }
                    let r = slot.translate(vec2(0.0, glide(ui, gid, rel) - rel));
                    let grip = ui.interact(r, gid.with("drag"), Sense::drag());
                    grabs.push((grip, *id, r.top()));
                    let resp = self.folder_cell(ui, r, *id, title, *unread, false);
                    if resp.clicked() {
                        chosen = Some(*id);
                    }
                    resp.context_menu(|ui| Self::folder_menu(ui, *id, &mut menu));
                }
                if let Some((id, title, unread, slot)) = lifted {
                    let gid = ui.scope_id().with(("cell-y", id));
                    let grip = ui.interact(slot, gid.with("drag"), Sense::drag());
                    let step = CELL_H + CELL_GAP;
                    if let (Some(d), Some(p)) = (self.folder_drag.as_mut(), ui.input(|i| i.pointer.interact_pos())) {
                        // Only folders move: "All chats" stays first, the archive stays last.
                        let pos = (p.y - d.grab - origin).clamp(step, step * folders.max(1.0));
                        d.follow(pos - step, |_| CELL_H, CELL_GAP);
                        set_glide(ui, gid, pos);
                        let r = Rect::from_min_size(pos2(slot.left(), origin + pos), slot.size());
                        lift(ui, r, 16.0);
                        self.folder_cell(ui, r, id, title, unread, true);
                    }
                    if !grip.dragged() {
                        drop = self.folder_drag.take().map(|d| d.order);
                    }
                }
            });
        for (grip, id, start) in grabs {
            self.grab_folder(ui, &grip, id, start, true);
        }
        if let Some(order) = drop {
            self.drop_folders(order);
        }
        if glass::icon_button(ui, add, "folder-new-col", Icon::Plus, false, true).on_hover_text(l!("Нова папка", "New folder")).clicked() {
            menu = Some(FolderMenu::New);
        }
        if let Some(id) = chosen {
            self.folder = id;
        }
        self.apply_folder_menu(menu);
    }

    /// Column item: icon, the name below it and the unread badge at the icon corner.
    fn folder_cell(&self, ui: &mut Ui, rect: Rect, id: Option<i32>, title: &str, unread: usize, lifted: bool) -> egui::Response {
        let resp = ui.interact(rect, ui.scope_id().with(("folder-cell", id)), Sense::click());
        let hover = ui.ctx().animate_bool(resp.id, resp.hovered());
        let sel = ui.ctx().animate_bool(resp.id.with("sel"), self.folder == id);
        let p = ui.painter();
        if sel > 0.0 {
            glass::glass(p, rect, Glass::tinted(16.0, ACCENT, (sel * 110.0) as u8));
        } else if hover > 0.0 && !lifted {
            p.rect_filled(rect, 16.0, glass::veil((hover * 14.0) as u8));
        }
        let color = if sel > 0.5 { ink() } else { ink_dim() };
        let icon_rect = Rect::from_center_size(pos2(rect.center().x, rect.top() + 22.0), vec2(24.0, 24.0));
        let folder = id.and_then(|i| self.folders.iter().find(|f| f.id == i));
        match folder_icon(id, folder) {
            Some(icon) => icons::draw(p, icon_rect, icon, color),
            None => {
                let emoji = folder.map(|f| f.emoticon.as_str()).unwrap_or_default();
                p.text(icon_rect.center(), Align2::CENTER_CENTER, emoji, FontId::proportional(20.0), color);
            }
        }
        let name = elided(ui, title, 11.0, color, rect.width() - 6.0);
        p.galley(pos2(rect.center().x - name.size().x / 2.0, rect.top() + 39.0), name, color);
        if unread > 0 {
            let text = if unread > 99 { "99+".to_owned() } else { unread.to_string() };
            let g = elided(ui, &text, 11.0, glass::ON_DARK, 40.0);
            let w = (g.size().x + 10.0).max(18.0);
            let b = Rect::from_min_size(pos2(icon_rect.right() - 6.0, icon_rect.top() - 6.0), vec2(w, 18.0));
            glass::glass(p, b, Glass::tinted(9.0, ACCENT, 230));
            p.galley(b.center() - g.size() / 2.0, g, glass::ON_DARK);
        }
        if resp.hovered() {
            ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
        }
        resp.on_hover_text(title)
    }

    /// Chats of the current folder (or found by name) and how many are pinned on top.
    pub(super) fn list_rows(&self, query: &str, drag: Option<&(Option<i32>, Reorder<PeerId>)>) -> (Vec<&ChatInfo>, usize) {
        // Search covers every folder, the archive included.
        if !query.is_empty() {
            return (self.chats.iter().filter(|c| c.title.to_lowercase().contains(query)).collect(), 0);
        }
        let mut rows: Vec<&ChatInfo> = self.chats.iter().filter(|c| self.in_folder(c, self.folder)).collect();
        let pins: Vec<PeerId> = match drag {
            Some((ctx, d)) if *ctx == self.folder => d.order.clone(),
            _ => match self.folder {
                Some(id) if id != ARCHIVE => self.folders.iter().find(|f| f.id == id).map(|f| f.pinned.clone()).unwrap_or_default(),
                _ => rows.iter().filter(|c| c.pinned).map(|c| c.id).collect(),
            },
        };
        let rank = |c: &ChatInfo| pins.iter().position(|p| *p == c.id).unwrap_or(usize::MAX);
        rows.sort_by_key(|c| rank(c));
        let n = rows.iter().take_while(|c| rank(c) != usize::MAX).count();
        (rows, n)
    }

    /// Chat list rows; the first `pinned` ones can be dragged.
    pub(super) fn chat_rows(&self, ui: &mut Ui, rows: &[&ChatInfo], pinned: usize, drag: &mut Option<(Option<i32>, Reorder<PeerId>)>) -> RowsOut {
        let mut out = RowsOut::default();
        let density = self.settings.density;
        let row_h = density.row_h();
        let origin = ui.cursor().top();
        let w = ui.available_width();
        let mut lifted = None;
        for (k, chat) in rows.iter().enumerate() {
            let tex = self.textures.get(&ImgKey::Avatar(chat.id));
            let typing = self.typing.get(&chat.id).map(|(who, _)| who.as_str());
            let draft = (self.selected != Some(chat.id)).then_some(chat.draft.as_str()).filter(|d| !d.trim().is_empty());
            let selected = self.selected == Some(chat.id);
            let tags = self.chat_tags(chat);
            // In a folder the pin mark follows the folder's own pins, not the main list's.
            let fixed = (chat.pinned != (k < pinned)).then(|| ChatInfo { pinned: k < pinned, ..(*chat).clone() });
            let chat: &ChatInfo = fixed.as_ref().unwrap_or(chat);
            let resp = if k < pinned {
                let (slot, _) = ui.allocate_exact_size(vec2(w, row_h), Sense::hover());
                let gid = ui.scope_id().with(("pin-y", self.folder, chat.id));
                if drag.as_ref().is_some_and(|(_, d)| d.key == chat.id) {
                    lifted = Some((rows[k], slot));
                    continue;
                }
                let rel = slot.top() - origin;
                let r = slot.translate(vec2(0.0, glide(ui, gid, rel) - rel));
                let grip = ui.interact(r, gid.with("drag"), Sense::drag());
                let mut row = ui.new_child(UiBuilder::new().max_rect(r).id_salt(("pin-row", chat.id)));
                let resp = chat_row(&mut row, chat, selected, tex, typing, draft, &tags, density);
                if drag.is_none()
                    && let Some(p) = pulled(ui, &grip)
                {
                    let order = rows[..pinned].iter().map(|c| c.id).collect();
                    *drag = Some((self.folder, Reorder { key: chat.id, grab: p.y - r.top(), order }));
                }
                resp
            } else {
                chat_row(ui, chat, selected, tex, typing, draft, &tags, density)
            };
            if resp.clicked() {
                out.clicked = Some(chat.id);
            }
            resp.context_menu(|ui| {
                if let Some(a) = super::chat_menu(ui, chat) {
                    out.menu = Some((chat.id, a));
                }
            });
        }
        // The lifted row is drawn last, on top; it follows the pointer within the pinned block.
        if let Some((chat, slot)) = lifted {
            let gid = ui.scope_id().with(("pin-y", self.folder, chat.id));
            let grip = ui.interact(slot, gid.with("drag"), Sense::drag());
            let step = row_h + ROW_GAP;
            if let (Some((_, d)), Some(p)) = (drag.as_mut(), ui.input(|i| i.pointer.interact_pos())) {
                let pos = (p.y - d.grab - origin).clamp(0.0, step * (pinned as f32 - 1.0).max(0.0));
                d.follow(pos, |_| row_h, ROW_GAP);
                set_glide(ui, gid, pos);
                let r = Rect::from_min_size(pos2(slot.left(), origin + pos), slot.size());
                lift(ui, r, 20.0);
                let mut row = ui.new_child(UiBuilder::new().max_rect(r).id_salt(("pin-row", chat.id)));
                let tex = self.textures.get(&ImgKey::Avatar(chat.id));
                chat_row(&mut row, chat, self.selected == Some(chat.id), tex, None, None, &self.chat_tags(chat), density);
                ui.ctx().set_cursor_icon(egui::CursorIcon::Grabbing);
            }
            if !grip.dragged() {
                out.dropped = drag.take().map(|(_, d)| d.order);
            }
        }
        out
    }

    /// Pinned chats were dropped: apply the new order locally and on the server.
    pub(super) fn drop_pins(&mut self, order: Vec<PeerId>) {
        match self.folder {
            Some(id) if id != ARCHIVE => {
                let Some(f) = self.folders.iter_mut().find(|f| f.id == id) else { return };
                if f.pinned == order {
                    return;
                }
                f.pinned = order;
                let f = f.clone();
                self.send(Cmd::SaveFolder(f));
            }
            folder => {
                let slots: Vec<usize> = (0..self.chats.len()).filter(|&i| order.contains(&self.chats[i].id)).collect();
                if slots.iter().map(|&i| self.chats[i].id).eq(order.iter().copied()) {
                    return;
                }
                let moved: Vec<ChatInfo> = order.iter().filter_map(|id| self.chats.iter().find(|c| c.id == *id).cloned()).collect();
                if moved.len() != slots.len() {
                    return;
                }
                for (i, c) in slots.into_iter().zip(moved) {
                    self.chats[i] = c;
                }
                self.chats_dirty = true;
                self.send(Cmd::ReorderPinned { archived: folder == Some(ARCHIVE), order });
            }
        }
    }
}

impl App {
    pub(super) fn sidebar(&mut self, ui: &mut Ui, rect: Rect) {
        glass::glass(ui.painter(), rect, Glass::panel(28.0));
        let p = ui.painter().clone();
        let inner = rect.shrink(14.0);

        p.text(inner.left_top() + vec2(6.0, 2.0), Align2::LEFT_TOP, l!("Чати", "Chats"), FontId::proportional(24.0), ink());
        if !self.me.is_empty() {
            // The name is the accounts menu button.
            let label = format!("{}  ▾", self.me);
            let r = p.text(inner.left_top() + vec2(6.0, 32.0), Align2::LEFT_TOP, label, FontId::proportional(12.5), ink_dim());
            let resp = ui.interact(r.expand(4.0), ui.scope_id().with("accounts-btn"), Sense::click());
            if resp.hovered() {
                ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
            }
            if resp.on_hover_text(l!("Акаунти", "Accounts")).clicked() {
                self.accounts_open = !self.accounts_open;
            }
            self.accounts_anchor = r;
            // Something unread in another account: a dot by the name, as in Telegram Desktop.
            if self.background_unread() > 0 {
                p.circle_filled(pos2(r.right() + 8.0, r.center().y), 4.0, ACCENT);
            }
        }
        let btn = |i: f32| Rect::from_min_size(pos2(inner.right() - 36.0 - i * 44.0, inner.top() + 4.0), vec2(36.0, 36.0));
        if glass::icon_button(ui, btn(0.0), "logout", Icon::Power, false, true).on_hover_text(l!("Вийти", "Log out")).clicked() {
            self.close_web_apps();
            self.send(Cmd::Logout);
        }
        if glass::icon_button(ui, btn(1.0), "reload", Icon::Reload, false, true).on_hover_text(l!("Оновити", "Refresh")).clicked() {
            self.send(Cmd::ReloadDialogs);
        }
        let (bell, hint) = if self.settings.notifications { (Icon::Bell, l!("Вимкнути сповіщення", "Turn off notifications")) } else { (Icon::Muted, l!("Увімкнути сповіщення", "Turn on notifications")) };
        if glass::icon_button(ui, btn(3.0), "settings", Icon::Gear, self.settings_open, true).on_hover_text(l!("Налаштування", "Settings")).clicked() {
            self.open_settings();
        }
        if glass::icon_button(ui, btn(2.0), "notify", bell, false, true).on_hover_text(hint).clicked() {
            self.settings.notifications = !self.settings.notifications;
            self.settings.save();
            if let Some(t) = &self.tray {
                t.set_notifications(self.settings.notifications);
            }
        }

        let search = Rect::from_min_size(inner.left_top() + vec2(0.0, 56.0), vec2(inner.width() - 48.0, 40.0));
        self.new_chat_button(ui, Rect::from_min_size(pos2(search.right() + 8.0, search.top()), vec2(40.0, 40.0)));
        let now = ui.input(|i| i.time);
        let search_resp = glass::search_field(ui, search, &mut self.search.query, l!("Пошук", "Search"));
        if std::mem::take(&mut self.focus_search) {
            search_resp.request_focus();
        }
        if search_resp.changed() {
            self.search.edited_at = now;
        }
        if let Some(query) = self.search.due(now) {
            self.search.requested = (query.clone(), None, None);
            self.public_hits.clear();
            if query.chars().count() >= 3 && !self.demo {
                self.send(Cmd::SearchPublic(query.clone()));
            }
            self.send(Cmd::Search { peer: None, query, kind: None, from: None });
        }

        let mut list_top = search.bottom() + 10.0;
        if !self.settings.folders_side && self.has_folder_bar() {
            let tabs = Rect::from_min_size(pos2(inner.left(), list_top), vec2(inner.width(), TABS_H));
            self.folder_tabs(ui, tabs);
            list_top = tabs.bottom() + 8.0;
        }
        let list_rect = Rect::from_min_max(pos2(inner.left() - 6.0, list_top), pos2(inner.right() + 6.0, inner.bottom()));
        let query = self.search.query.trim().to_lowercase();
        let mut clicked = None;
        let mut jump = None;
        let mut menu = None;
        let mut found = None;
        let mut dropped = None;
        let mut drag = self.pin_drag.take();
        let mut birthday = None;
        let mut story = None;
        let (rows, pinned) = self.list_rows(&query, drag.as_ref());
        let mut child = ui.new_child(UiBuilder::new().max_rect(list_rect));
        child.set_clip_rect(list_rect);
        egui::ScrollArea::vertical()
            .id_salt("chat-list")
            .auto_shrink(false)
            .show(&mut child, |ui| {
                ui.spacing_mut().item_spacing.y = 4.0;
                if query.is_empty() && self.folder.is_none() {
                    story = self.story_strip(ui);
                    birthday = self.birthday_banner(ui);
                }
                let rows_out = self.chat_rows(ui, &rows, pinned, &mut drag);
                clicked = rows_out.clicked;
                menu = rows_out.menu;
                dropped = rows_out.dropped;
                // Public chats not yet in the list.
                let public: Vec<&ChatInfo> = self.public_hits.iter().filter(|p| !self.chats.iter().any(|c| c.id == p.id)).collect();
                if !query.is_empty() && !public.is_empty() {
                    section_title(ui, l!("Публічні чати", "Public chats"));
                    for chat in public {
                        let tex = self.textures.get(&ImgKey::Avatar(chat.id));
                        if chat_row(ui, chat, false, tex, None, None, &[], self.settings.density).clicked() {
                            found = Some(chat.clone());
                        }
                    }
                }
                if !query.is_empty() && self.search.hits_for.0.to_lowercase() == query {
                    section_title(ui, if self.search.hits.is_empty() { l!("Повідомлень не знайдено", "No messages found") } else { l!("Повідомлення", "Messages") });
                    for hit in &self.search.hits {
                        if super::hit_row(ui, hit, &self.search.hits_for.0, self.textures.get(&ImgKey::Avatar(hit.msg.peer))).clicked() {
                            jump = Some(hit.clone());
                        }
                    }
                } else if query.chars().count() >= 2 && self.backend.is_some() {
                    ui.add_space(12.0);
                    ui.vertical_centered(|ui| ui.add(egui::Spinner::new().color(ink_dim())));
                }
                if self.chats.is_empty() {
                    ui.add_space(30.0);
                    ui.vertical_centered(|ui| ui.add(egui::Spinner::new().color(ink_dim())));
                }
            });
        drop(rows);
        self.pin_drag = drag;
        if let Some(order) = dropped {
            self.drop_pins(order);
        }
        if let Some(id) = clicked {
            self.open_chat(id);
        }
        if let Some(a) = birthday {
            self.birthday_action(a);
        }
        if let Some(peer) = story {
            self.open_stories(peer);
        }
        if let Some(chat) = found {
            let id = chat.id;
            self.chats.push(chat);
            self.open_chat(id);
        }
        if let Some(hit) = jump {
            self.jump_to(&hit);
        }
        if let Some((peer, a)) = menu {
            self.apply_chat(peer, a);
        }
    }
}

impl App {
    /// The chat list, folders, found and joined chats.
    pub(super) fn list_event(&mut self, ev: Ev) {
        match ev {
            Ev::Dialogs(chats) => {
                self.chats = chats;
                // Once per session, when the account's chats are in.
                if !self.session_extras {
                    self.session_extras = true;
                    self.send(Cmd::LoadBirthdays);
                    self.send(Cmd::LoadStories);
                }
            }
            Ev::Folders(f) => {
                if self.folder.is_some_and(|id| id != ARCHIVE && !f.iter().any(|x| x.id == id)) {
                    self.folder = None;
                }
                self.folders = f;
            }
            Ev::SearchResults { peer: None, query, kind, from, hits } => self.search.accept((query, kind, from), hits),
            Ev::SearchResults { peer: Some(peer), query, kind, from, hits } => {
                if self.selected == Some(peer)
                    && let Some(s) = &mut self.chat_search
                {
                    s.accept((query, kind, from), hits);
                }
            }
            Ev::PublicResults { query, chats } => {
                if query == self.search.query.trim() {
                    self.public_hits = chats;
                }
            }
            Ev::Joined(peer) => {
                if let Some(c) = self.chat_mut(peer) {
                    c.joined = true;
                }
                self.toast = Some((l!("Ви приєдналися", "You joined").into(), 0.0, false));
            }
            Ev::ChatFound(info) => {
                let id = info.id;
                if !self.chats.iter().any(|c| c.id == id) {
                    self.chats.push(info);
                }
                self.open_chat(id);
            }
            Ev::ChatTitle { peer, title } => {
                if let Some(c) = self.chat_mut(peer) {
                    c.title = title;
                }
                self.chats_dirty = true;
            }
            _ => {}
        }
    }
}

#[cfg(test)]
mod tests {
    use super::Reorder;

    #[test]
    fn follow_moves_past_neighbour_centres() {
        let mut d = Reorder { key: 1, grab: 0.0, order: vec![1, 2, 3, 4] };
        // Row start at 1.6 steps: its centre passed the second row's centre, so it becomes second.
        d.follow(1.6 * 70.0, |_| 66.0, 4.0);
        assert_eq!(d.order, vec![2, 1, 3, 4]);
        d.follow(3.0 * 70.0 + 5.0, |_| 66.0, 4.0);
        assert_eq!(d.order, vec![2, 3, 4, 1]);
        d.follow(0.0, |_| 66.0, 4.0);
        assert_eq!(d.order, vec![1, 2, 3, 4]);
    }

    #[test]
    fn follow_with_different_widths() {
        let w = |k: i32| if k == 2 { 120.0 } else { 40.0 };
        let mut d = Reorder { key: 1, grab: 0.0, order: vec![1, 2, 3] };
        d.follow(50.0, w, 6.0);
        assert_eq!(d.order, vec![1, 2, 3]);
        d.follow(90.0, w, 6.0);
        assert_eq!(d.order, vec![2, 1, 3]);
    }

}
