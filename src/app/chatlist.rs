//! Chat list: the folder column, dragging pinned chats and folders, "New chat".
//! A child module of `app`, so it works with `App` fields directly.

use eframe::egui::{self, Align2, Color32, FontId, Id, Key, Rect, Sense, Ui, UiBuilder, pos2, vec2};
use grammers_client::session::types::PeerId;

use super::{App, ARCHIVE, FolderMenu, FolderEdit, SEARCH_DELAY, TABS_H, chat_row, elided, section_title};
use crate::backend::{ChatInfo, ChatKind, Cmd, Ev, Folder, ImgKey};
use crate::glass::{self, ACCENT, Glass, ink, ink_dim, rgba};
use crate::i18n::{l, lf};
use crate::icons::{self, Icon};

/// Chat row height (as in `chat_row`) and the gap between rows.
const ROW_H: f32 = 66.0;
const ROW_GAP: f32 = 4.0;
/// Folder column width and the height of one item.
pub(super) const COLUMN_W: f32 = 76.0;
const CELL_H: f32 = 62.0;
const CELL_GAP: f32 = 4.0;
const TAB_GAP: f32 = 6.0;
/// Most people a new group gets in one request (as in Telegram Desktop; the rest are invited later).
const GROUP_MAX: usize = 199;

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

/// The "New chat" window: contacts, a new group or a new channel.
pub(super) struct NewChat {
    kind: NewKind,
    query: String,
    edited_at: f64,
    requested: String,
    /// Members of the new group.
    picked: Vec<PeerId>,
    /// Group: members are chosen, now the name.
    named: bool,
    title: String,
    about: String,
    /// The create request is in flight.
    pub(super) busy: bool,
}

#[derive(Clone, Copy, PartialEq)]
enum NewKind {
    Contacts,
    Group,
    Channel,
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
    fn grab_folder(&mut self, grip: &egui::Response, id: Option<i32>, start: f32, vertical: bool) {
        let Some(fid) = id.filter(|x| *x != ARCHIVE) else { return };
        if grip.drag_started()
            && let Some(p) = grip.interact_pointer_pos()
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
                        let resp = glass::button(ui, r, ("folder", *id), &text, self.folder == *id, true);
                        if resp.clicked() {
                            chosen = Some(*id);
                        }
                        resp.context_menu(|ui| Self::folder_menu(ui, *id, &mut menu));
                        let grip = ui.interact(r, gid.with("drag"), Sense::drag());
                        grabs.push((grip, *id, r.left()));
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
            self.grab_folder(&grip, id, start, false);
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
                    let resp = self.folder_cell(ui, r, *id, title, *unread, false);
                    if resp.clicked() {
                        chosen = Some(*id);
                    }
                    resp.context_menu(|ui| Self::folder_menu(ui, *id, &mut menu));
                    let grip = ui.interact(r, gid.with("drag"), Sense::drag());
                    grabs.push((grip, *id, r.top()));
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
            self.grab_folder(&grip, id, start, true);
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
        let origin = ui.cursor().top();
        let w = ui.available_width();
        let mut lifted = None;
        for (k, chat) in rows.iter().enumerate() {
            let tex = self.textures.get(&ImgKey::Avatar(chat.id));
            let typing = self.typing.get(&chat.id).map(|(who, _)| who.as_str());
            let draft = (self.selected != Some(chat.id)).then_some(chat.draft.as_str()).filter(|d| !d.trim().is_empty());
            let selected = self.selected == Some(chat.id);
            // In a folder the pin mark follows the folder's own pins, not the main list's.
            let fixed = (chat.pinned != (k < pinned)).then(|| ChatInfo { pinned: k < pinned, ..(*chat).clone() });
            let chat: &ChatInfo = fixed.as_ref().unwrap_or(chat);
            let resp = if k < pinned {
                let (slot, _) = ui.allocate_exact_size(vec2(w, ROW_H), Sense::hover());
                let gid = ui.scope_id().with(("pin-y", self.folder, chat.id));
                if drag.as_ref().is_some_and(|(_, d)| d.key == chat.id) {
                    lifted = Some((rows[k], slot));
                    continue;
                }
                let rel = slot.top() - origin;
                let r = slot.translate(vec2(0.0, glide(ui, gid, rel) - rel));
                let mut row = ui.new_child(UiBuilder::new().max_rect(r).id_salt(("pin-row", chat.id)));
                let resp = chat_row(&mut row, chat, selected, tex, typing, draft);
                let grip = ui.interact(r, gid.with("drag"), Sense::drag());
                if grip.drag_started()
                    && let Some(p) = grip.interact_pointer_pos()
                {
                    let order = rows[..pinned].iter().map(|c| c.id).collect();
                    *drag = Some((self.folder, Reorder { key: chat.id, grab: p.y - r.top(), order }));
                }
                resp
            } else {
                chat_row(ui, chat, selected, tex, typing, draft)
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
            let step = ROW_H + ROW_GAP;
            if let (Some((_, d)), Some(p)) = (drag.as_mut(), ui.input(|i| i.pointer.interact_pos())) {
                let pos = (p.y - d.grab - origin).clamp(0.0, step * (pinned as f32 - 1.0).max(0.0));
                d.follow(pos, |_| ROW_H, ROW_GAP);
                set_glide(ui, gid, pos);
                let r = Rect::from_min_size(pos2(slot.left(), origin + pos), slot.size());
                lift(ui, r, 20.0);
                let mut row = ui.new_child(UiBuilder::new().max_rect(r).id_salt(("pin-row", chat.id)));
                let tex = self.textures.get(&ImgKey::Avatar(chat.id));
                chat_row(&mut row, chat, self.selected == Some(chat.id), tex, None, None);
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

    // ───────────────────────────── New chat ─────────────────────────────

    /// Pencil button next to search: a menu with "New group", "New channel", "Contacts".
    pub(super) fn new_chat_button(&mut self, ui: &mut Ui, rect: Rect) {
        let open = self.new_chat_menu.is_some();
        // The anchor follows the button when the window is resized.
        if open {
            self.new_chat_menu = Some(rect);
        }
        if glass::icon_button(ui, rect, "new-chat", Icon::Pencil, open, true).on_hover_text(l!("Новий чат", "New chat")).clicked() {
            self.new_chat_menu = if open { None } else { Some(rect) };
        }
    }

    fn new_chat_menu_view(&mut self, ui: &mut Ui) {
        let Some(anchor) = self.new_chat_menu else { return };
        let screen = ui.max_rect();
        if ui.interact(screen, ui.scope_id().with("new-chat-bg"), Sense::click()).clicked() {
            self.new_chat_menu = None;
            return;
        }
        let items = [(NewKind::Group, Icon::Users, l!("Нова група", "New group")), (NewKind::Channel, Icon::Channel, l!("Новий канал", "New channel")), (NewKind::Contacts, Icon::User, l!("Контакти", "Contacts"))];
        let rect = Rect::from_min_size(pos2(anchor.right() - 220.0, anchor.bottom() + 8.0), vec2(220.0, items.len() as f32 * 40.0 + 16.0));
        glass::glass(ui.painter(), rect, Glass::panel(20.0).with_shadow());
        ui.interact(rect, ui.scope_id().with("new-chat-panel"), Sense::click());
        let mut pick = None;
        for (k, (kind, icon, label)) in items.into_iter().enumerate() {
            let r = Rect::from_min_size(pos2(rect.left() + 8.0, rect.top() + 8.0 + k as f32 * 40.0), vec2(rect.width() - 16.0, 40.0));
            let resp = ui.interact(r, ui.scope_id().with(("new-chat-item", k)), Sense::click());
            let p = ui.painter();
            if resp.hovered() {
                p.rect_filled(r, 12.0, glass::veil(16));
                ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
            }
            icons::draw(p, Rect::from_center_size(pos2(r.left() + 20.0, r.center().y), vec2(20.0, 20.0)), icon, ink());
            p.text(pos2(r.left() + 44.0, r.center().y), Align2::LEFT_CENTER, label, FontId::proportional(14.5), ink());
            if resp.clicked() {
                pick = Some(kind);
            }
        }
        if let Some(kind) = pick {
            self.new_chat_menu = None;
            if kind != NewKind::Channel && self.contacts.is_empty() {
                self.send(Cmd::LoadContacts);
            }
            self.new_chat = Some(NewChat { kind, query: String::new(), edited_at: 0.0, requested: String::new(), picked: Vec::new(), named: false, title: String::new(), about: String::new(), busy: false });
        } else if ui.input(|i| i.key_pressed(Key::Escape)) {
            self.new_chat_menu = None;
        }
    }

    /// "New chat" menu and windows, above everything else.
    pub(super) fn chatlist_overlays(&mut self, ui: &mut Ui) {
        if self.new_chat.is_some() {
            self.new_chat_view(ui);
        }
        if self.new_chat_menu.is_some() {
            self.new_chat_menu_view(ui);
        }
    }

    pub(super) fn chatlist_event(&mut self, ev: Ev) {
        match ev {
            Ev::Contacts(list) => self.contacts = list,
            Ev::ContactResults { query, chats } => self.contact_hits = (query, chats),
            Ev::ChatCreated(info) => {
                self.new_chat = None;
                let id = info.id;
                if !self.chats.iter().any(|c| c.id == id) {
                    self.chats.insert(0, info);
                    sort_chats(&mut self.chats);
                }
                self.folder = None;
                self.open_chat(id);
            }
            _ => {}
        }
    }

    /// Opens a chat with a person from contacts or search.
    fn open_person(&mut self, person: ChatInfo) {
        let id = person.id;
        if !self.chats.iter().any(|c| c.id == id) {
            self.chats.push(person);
        }
        self.new_chat = None;
        self.open_chat(id);
    }

    /// People for the window: matching contacts, plus global results that are not contacts.
    fn people(&self, query: &str) -> (Vec<&ChatInfo>, Vec<&ChatInfo>) {
        let q = query.trim().to_lowercase();
        let mine: Vec<&ChatInfo> = self.contacts.iter().filter(|c| q.is_empty() || c.title.to_lowercase().contains(&q) || c.last_text.to_lowercase().contains(&q)).collect();
        let global = if !q.is_empty() && self.contact_hits.0.to_lowercase() == q {
            self.contact_hits.1.iter().filter(|c| !self.contacts.iter().any(|x| x.id == c.id) && c.kind != ChatKind::Saved).collect()
        } else {
            Vec::new()
        };
        (mine, global)
    }

    fn new_chat_view(&mut self, ui: &mut Ui) {
        let Some(nc) = &mut self.new_chat else { return };
        let now = ui.input(|i| i.time);
        let screen = ui.max_rect();
        ui.painter().rect_filled(screen, 0.0, Color32::from_black_alpha(90));
        ui.interact(screen, ui.scope_id().with("new-chat-modal-bg"), Sense::click());
        let tall = nc.kind != NewKind::Channel;
        let rect = Rect::from_center_size(screen.center(), vec2(screen.width().min(440.0) - 20.0, if tall { (screen.height() - 60.0).min(620.0) } else { 300.0 }));
        glass::glass(ui.painter(), rect, Glass::panel(26.0).with_shadow());
        let inner = rect.shrink(18.0);
        let title = match (nc.kind, nc.named) {
            (NewKind::Contacts, _) => l!("Контакти", "Contacts").to_owned(),
            (NewKind::Group, false) => l!("Нова група", "New group").to_owned(),
            (NewKind::Group, true) => l!("Назва групи", "Group name").to_owned(),
            (NewKind::Channel, _) => l!("Новий канал", "New channel").to_owned(),
        };
        ui.painter().text(inner.left_top() + vec2(4.0, 2.0), Align2::LEFT_TOP, &title, FontId::proportional(17.0), ink());
        if nc.kind == NewKind::Group && !nc.named {
            let n = nc.picked.len();
            let sub = if n == 0 { l!("Виберіть учасників", "Select members").to_owned() } else { lf!("Вибрано: {n} з {GROUP_MAX}", "Selected: {n} of {GROUP_MAX}") };
            ui.painter().text(inner.left_top() + vec2(4.0, 24.0), Align2::LEFT_TOP, sub, FontId::proportional(12.5), ink_dim());
        }
        let close = Rect::from_min_size(pos2(inner.right() - 32.0, inner.top() - 4.0), vec2(32.0, 32.0));
        let mut done = glass::icon_button(ui, close, "new-chat-close", Icon::Close, false, true).clicked();
        let buttons = Rect::from_min_max(pos2(inner.left(), inner.bottom() - 38.0), inner.max);
        let top = inner.top() + 48.0;

        // Contacts and the first group step show search and a list of people.
        let picking = nc.kind == NewKind::Contacts || (nc.kind == NewKind::Group && !nc.named);
        let mut open = None;
        let mut toggle = None;
        if picking {
            let field = Rect::from_min_size(pos2(inner.left(), top), vec2(inner.width(), 38.0));
            let r = glass::search_field(ui, field, &mut nc.query, l!("Пошук за іменем чи @username", "Search by name or @username"));
            if r.changed() {
                nc.edited_at = now;
            }
            if !r.has_focus() && nc.query.is_empty() && !ui.memory(|m| m.focused().is_some()) {
                r.request_focus();
            }
            let q = nc.query.trim().to_owned();
            let search = (q.chars().count() >= 2 && q != nc.requested && now - nc.edited_at > SEARCH_DELAY).then(|| q.clone());
            if let Some(q) = &search {
                nc.requested = q.clone();
            }
            // Groups have Cancel / Next under the list, contacts the "New contact" button.
            let bottom = buttons.top() - 10.0;
            let body = Rect::from_min_max(pos2(inner.left() - 6.0, field.bottom() + 10.0), pos2(inner.right() + 6.0, bottom));
            let mut child = ui.new_child(UiBuilder::new().max_rect(body));
            child.set_clip_rect(body);
            let picked = nc.picked.clone();
            let group = nc.kind == NewKind::Group;
            let query = nc.query.clone();
            let (mine, global) = self.people(&query);
            egui::ScrollArea::vertical().id_salt("new-chat-people").auto_shrink(false).show(&mut child, |ui| {
                ui.spacing_mut().item_spacing.y = 2.0;
                if mine.is_empty() && global.is_empty() {
                    ui.add_space(24.0);
                    ui.vertical_centered(|ui| {
                        if self.contacts.is_empty() && query.trim().is_empty() {
                            ui.add(egui::Spinner::new().color(ink_dim()));
                        } else {
                            ui.label(egui::RichText::new(l!("Нікого не знайдено", "No one found")).color(ink_dim()));
                        }
                    });
                }
                let mut row = |ui: &mut Ui, c: &ChatInfo| {
                    let mark = group.then(|| picked.contains(&c.id));
                    if person_row(ui, c, self.textures.get(&ImgKey::Avatar(c.id)), mark).clicked() {
                        if group {
                            toggle = Some(c.id);
                        } else {
                            open = Some(c.clone());
                        }
                    }
                };
                for c in &mine {
                    row(ui, c);
                }
                if !global.is_empty() {
                    section_title(ui, l!("Глобальний пошук", "Global search"));
                    for c in &global {
                        row(ui, c);
                    }
                }
            });
            if let Some(q) = search {
                self.send(Cmd::SearchContacts(q));
            }
        }
        let Some(nc) = &mut self.new_chat else { return };
        if let Some(id) = toggle {
            if let Some(i) = nc.picked.iter().position(|p| *p == id) {
                nc.picked.remove(i);
            } else if nc.picked.len() < GROUP_MAX {
                nc.picked.push(id);
            }
        }

        let mut cmd = None;
        match (nc.kind, nc.named) {
            (NewKind::Contacts, _) => {
                if glass::button(ui, buttons, "new-contact", l!("Новий контакт", "New contact"), false, true).clicked() {
                    self.new_chat = None;
                    self.open_contact_form(None);
                    return;
                }
            }
            (NewKind::Group, false) => {
                let w = (buttons.width() - 8.0) / 2.0;
                if glass::button(ui, Rect::from_min_size(buttons.min, vec2(w, buttons.height())), "group-cancel", l!("Скасувати", "Cancel"), false, true).clicked() {
                    done = true;
                }
                let next = Rect::from_min_size(pos2(buttons.right() - w, buttons.top()), vec2(w, buttons.height()));
                if glass::button(ui, next, "group-next", l!("Далі", "Next"), true, !nc.picked.is_empty()).clicked() {
                    nc.named = true;
                }
            }
            (NewKind::Group, true) => {
                let field = Rect::from_min_size(pos2(inner.left(), top), vec2(inner.width(), 40.0));
                let r = glass::text_field(ui, field, &mut nc.title, l!("Назва групи", "Group name"), false);
                if nc.title.is_empty() && !r.has_focus() {
                    r.request_focus();
                }
                let n = nc.picked.len();
                let p = ui.painter();
                p.text(pos2(inner.left() + 4.0, field.bottom() + 16.0), Align2::LEFT_TOP, lf!("Учасників: {n}", "Members: {n}"), FontId::proportional(12.5), ink_dim());
                let mut y = field.bottom() + 40.0;
                for id in &nc.picked {
                    if y > buttons.top() - 44.0 {
                        break;
                    }
                    let c = self.contacts.iter().chain(&self.contact_hits.1).find(|c| c.id == *id);
                    let name = c.map(|c| c.title.as_str()).unwrap_or("…");
                    let seed = id.bot_api_dialog_id_unchecked();
                    glass::avatar(p, pos2(inner.left() + 18.0, y + 18.0), 16.0, name, seed, None, self.textures.get(&ImgKey::Avatar(*id)));
                    p.galley(pos2(inner.left() + 44.0, y + 9.0), elided(ui, name, 14.5, ink(), inner.width() - 50.0), ink());
                    y += 40.0;
                }
                let w = (buttons.width() - 8.0) / 2.0;
                if glass::button(ui, Rect::from_min_size(buttons.min, vec2(w, buttons.height())), "group-back", l!("Назад", "Back"), false, !nc.busy).clicked() {
                    nc.named = false;
                }
                let ok = !nc.title.trim().is_empty() && !nc.busy;
                let create = Rect::from_min_size(pos2(buttons.right() - w, buttons.top()), vec2(w, buttons.height()));
                let enter = r.lost_focus() && ui.input(|i| i.key_pressed(Key::Enter));
                if (glass::button(ui, create, "group-create", l!("Створити", "Create"), true, ok).clicked() || enter) && ok {
                    nc.busy = true;
                    cmd = Some(Cmd::CreateGroup { title: nc.title.trim().to_owned(), users: nc.picked.clone() });
                }
            }
            (NewKind::Channel, _) => {
                let field = Rect::from_min_size(pos2(inner.left(), top), vec2(inner.width(), 40.0));
                let r = glass::text_field(ui, field, &mut nc.title, l!("Назва каналу", "Channel name"), false);
                if nc.title.is_empty() && !r.has_focus() && !ui.memory(|m| m.focused().is_some()) {
                    r.request_focus();
                }
                let about = Rect::from_min_size(pos2(inner.left(), field.bottom() + 10.0), vec2(inner.width(), 40.0));
                glass::text_field(ui, about, &mut nc.about, l!("Опис (необов'язково)", "Description (optional)"), false);
                ui.painter().text(
                    pos2(inner.left() + 4.0, about.bottom() + 12.0),
                    Align2::LEFT_TOP,
                    l!("Канал — щоб розповідати новини необмеженій аудиторії.", "Channels are a tool for broadcasting your messages to unlimited audiences."),
                    FontId::proportional(12.5),
                    ink_dim(),
                );
                let ok = !nc.title.trim().is_empty() && !nc.busy;
                if glass::button(ui, buttons, "channel-create", l!("Створити канал", "Create channel"), true, ok).clicked() && ok {
                    nc.busy = true;
                    cmd = Some(Cmd::CreateChannel { title: nc.title.trim().to_owned(), about: nc.about.trim().to_owned() });
                }
            }
        }
        if let Some(cmd) = cmd {
            self.send(cmd);
        }
        if let Some(person) = open {
            self.open_person(person);
            return;
        }
        if done || ui.input(|i| i.key_pressed(Key::Escape)) {
            self.new_chat = None;
        }
    }
}

/// A person in the "New chat" list: avatar, name, status; a check mark when picking group members.
fn person_row(ui: &mut Ui, c: &ChatInfo, tex: Option<&egui::TextureHandle>, mark: Option<bool>) -> egui::Response {
    let (rect, resp) = ui.allocate_exact_size(vec2(ui.available_width(), 52.0), Sense::click());
    let hover = ui.ctx().animate_bool(resp.id, resp.hovered());
    let p = ui.painter();
    if hover > 0.0 {
        p.rect_filled(rect, 16.0, glass::veil((hover * 14.0) as u8));
    }
    let center = pos2(rect.left() + 30.0, rect.center().y);
    glass::avatar(p, center, 20.0, &c.title, c.id.bot_api_dialog_id_unchecked(), None, tex);
    let x = rect.left() + 60.0;
    let right = if mark.is_some() { 40.0 } else { 12.0 };
    p.galley(pos2(x, rect.top() + 8.0), elided(ui, &c.title, 14.5, ink(), rect.right() - x - right), ink());
    let status = c.presence.text().unwrap_or_else(|| c.last_text.clone());
    let color = if c.presence.online() { ACCENT } else { ink_dim() };
    p.galley(pos2(x, rect.top() + 28.0), elided(ui, &status, 12.5, color, rect.right() - x - right), color);
    if let Some(on) = mark {
        let r = Rect::from_center_size(pos2(rect.right() - 22.0, rect.center().y), vec2(22.0, 22.0));
        if on {
            p.circle_filled(r.center(), 11.0, ACCENT);
            icons::draw(p, r.shrink(4.0), Icon::Check, glass::ON_DARK);
        } else {
            p.circle_stroke(r.center(), 10.0, egui::Stroke::new(1.5, ink_dim()));
        }
    }
    if resp.hovered() {
        ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
    }
    resp
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
