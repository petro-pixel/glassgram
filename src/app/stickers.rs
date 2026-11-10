//! The sticker panel and sticker sets, as in Telegram Desktop: emoji, saved GIFs, set search and
//! the installed sets (favourite and recent first) in one row of tabs; the set box with Add /
//! Remove, opened from a sticker in a chat or a `t.me/addstickers/…` link; the order of installed sets.

use super::*;
use crate::backend::{DocRef, FAVED_SET, RECENT_SET, StickerSetRef};

/// A pause in typing before the set search goes out.
const SEARCH_DELAY: f64 = 0.4;

/// What the panel shows under its row of tabs.
#[derive(Default, Clone, Copy, PartialEq)]
pub(super) enum PanelTab {
    #[default]
    Emoji,
    Gifs,
    Search,
    /// Installed sets: order and removing.
    Manage,
    /// A set by its index in `StickerPanel::sets`.
    Set(usize),
}

/// The sticker and emoji panel, the set box and what they asked the server for.
#[derive(Default)]
pub(super) struct StickerPanel {
    /// Favourite and recent stickers (pseudo-sets), then the installed sets in the user's order.
    pub(super) sets: Vec<StickerSetInfo>,
    /// The panel is open.
    pub(super) open: bool,
    tab: PanelTab,
    /// Sticker and GIF previews already asked for.
    thumbs: HashSet<i64>,
    /// The sets were asked for once this session.
    pub(super) requested: bool,
    /// Saved GIFs; `None` until asked for.
    gifs: Option<Vec<i64>>,
    /// The set search field and when it last changed.
    query: String,
    query_at: f64,
    /// The query the last request went out for.
    searched: Option<String>,
    /// Found sets, or the trending ones for an empty query.
    found: Vec<StickerSetInfo>,
    /// The set box: `Some(None)` while the set loads.
    set_box: Option<Option<StickerSetInfo>>,
}

/// A real installed set, not the favourite or recent stickers.
fn installed(s: &StickerSetInfo) -> bool {
    s.id != FAVED_SET && s.id != RECENT_SET
}

impl App {
    pub(super) fn set_box_open(&self) -> bool {
        self.stickers.set_box.is_some()
    }

    pub(super) fn open_sticker_set(&mut self, set: StickerSetRef) {
        self.stickers.set_box = Some(None);
        self.send(Cmd::OpenStickerSet(set));
    }

    /// The panel's stickers are listed in the backend once asked for; previews too.
    pub(super) fn request_stickers(&mut self) {
        if !self.stickers.requested {
            self.stickers.requested = true;
            self.send(Cmd::LoadStickers);
        }
    }

    pub(super) fn want_sticker_thumbs(&mut self, ids: impl IntoIterator<Item = i64>) {
        let ids: Vec<i64> = ids.into_iter().filter(|id| self.stickers.thumbs.insert(*id)).collect();
        if !ids.is_empty() {
            self.send(Cmd::LoadStickerThumbs(ids));
        }
    }

    fn request_set_thumbs(&mut self) {
        let PanelTab::Set(i) = self.stickers.tab else { return };
        let ids: Vec<i64> = self.stickers.sets.get(i).map(|s| s.stickers.iter().map(|x| x.0).collect()).unwrap_or_default();
        self.want_sticker_thumbs(ids);
    }

    fn faved(&self, id: i64) -> bool {
        self.stickers.sets.iter().any(|s| s.id == FAVED_SET && s.stickers.iter().any(|x| x.0 == id))
    }

    /// A sticker or its emoji while the preview loads.
    fn draw_sticker(&self, p: &egui::Painter, r: Rect, id: i64, emoji: &str) {
        match self.textures.get(&ImgKey::Sticker(id)) {
            Some(t) => glass::image_contain(p, r, t),
            None => {
                p.text(r.center(), Align2::CENTER_CENTER, emoji, FontId::proportional(r.height() * 0.55), ink());
            }
        }
    }

    /// A grid of stickers; returns (clicked, favourite toggled, "view set" asked) sticker ids.
    fn sticker_grid(&self, ui: &mut Ui, rect: Rect, salt: impl std::hash::Hash + std::fmt::Debug, stickers: &[(i64, String)], with_set: bool) -> GridPick {
        let mut pick = GridPick::default();
        let mut child = ui.new_child(UiBuilder::new().max_rect(rect));
        child.set_clip_rect(rect);
        egui::ScrollArea::vertical().id_salt(salt).auto_shrink(false).show(&mut child, |ui| {
            let cols = ((rect.width() / 70.0).floor() as usize).max(1);
            let cell = rect.width() / cols as f32 - 2.0;
            for row in stickers.chunks(cols) {
                ui.horizontal(|ui| {
                    ui.spacing_mut().item_spacing.x = 2.0;
                    for (id, emoji) in row {
                        let (r, resp) = ui.allocate_exact_size(vec2(cell, cell), Sense::click());
                        let hover = ui.ctx().animate_bool(resp.id, resp.hovered());
                        if hover > 0.0 {
                            ui.painter().rect_filled(r, 14.0, glass::veil((hover * 22.0) as u8));
                        }
                        self.draw_sticker(ui.painter(), r.shrink(6.0 - hover * 2.0), *id, emoji);
                        let faved = self.faved(*id);
                        resp.context_menu(|ui| {
                            let label = if faved { l!("Прибрати з обраних", "Remove from Favourites") } else { l!("Додати в обрані", "Add to Favourites") };
                            if glass::menu_button(ui, Icon::Star, label).clicked() {
                                pick.fave = Some((*id, faved));
                            }
                            if with_set && glass::menu_button(ui, Icon::Smile, l!("Переглянути набір", "View sticker set")).clicked() {
                                pick.view = Some(*id);
                            }
                        });
                        if resp.on_hover_cursor(egui::CursorIcon::PointingHand).clicked() {
                            pick.send = Some(*id);
                        }
                    }
                });
            }
        });
        pick
    }

    fn apply_pick(&mut self, peer: Option<PeerId>, pick: GridPick) {
        if let Some((id, faved)) = pick.fave {
            self.send(Cmd::FaveSticker { doc: DocRef::Doc(id), unfave: faved });
        }
        if let Some(id) = pick.view {
            self.open_sticker_set(StickerSetRef::Doc(DocRef::Doc(id)));
        }
        if let (Some(doc), Some(peer)) = (pick.send, peer) {
            let reply_to = self.take_reply();
            self.compose = Compose::New;
            self.send(Cmd::SendSticker { peer, doc, reply_to });
        }
    }

    /// Emoji, GIF and sticker panel: a row of tabs on top, the chosen one below.
    pub(super) fn sticker_view(&mut self, ui: &mut Ui, peer: PeerId, rect: Rect) {
        glass::glass(ui.painter(), rect, Glass::tinted(24.0, glass::chrome(), 120).frosted(1.0).with_shadow());
        // Swallow clicks so they don't reach the messages under the panel.
        ui.interact(rect, ui.scope_id().with("sticker-bg"), Sense::click());
        let inner = rect.shrink(10.0);
        let tabs = Rect::from_min_size(inner.min, vec2(inner.width() - 46.0, 40.0));
        let manage = Rect::from_min_size(pos2(inner.right() - 40.0, inner.top()), vec2(40.0, 40.0));
        let mut chosen = None;
        let mut child = ui.new_child(UiBuilder::new().max_rect(tabs));
        child.set_clip_rect(tabs);
        let tab = self.stickers.tab;
        egui::ScrollArea::horizontal()
            .id_salt("sticker-tabs")
            .scroll_bar_visibility(egui::scroll_area::ScrollBarVisibility::AlwaysHidden)
            .show(&mut child, |ui| {
                ui.horizontal(|ui| {
                    ui.spacing_mut().item_spacing.x = 6.0;
                    let tab_cell = |ui: &mut Ui, active: bool| {
                        let (r, resp) = ui.allocate_exact_size(vec2(40.0, 40.0), Sense::click());
                        if active {
                            glass::glass(ui.painter(), r, Glass::tinted(12.0, ACCENT, 120));
                        } else if resp.hovered() {
                            ui.painter().rect_filled(r, 12.0, glass::veil(18));
                        }
                        (r, resp)
                    };
                    let icon = |r: Rect| Rect::from_center_size(r.center(), vec2(22.0, 22.0));
                    let (r, resp) = tab_cell(ui, tab == PanelTab::Emoji);
                    icons::draw(ui.painter(), icon(r), Icon::Smile, ink());
                    if resp.on_hover_text(l!("Емодзі", "Emoji")).clicked() {
                        chosen = Some(PanelTab::Emoji);
                    }
                    let (r, resp) = tab_cell(ui, tab == PanelTab::Gifs);
                    ui.painter().text(r.center(), Align2::CENTER_CENTER, "GIF", FontId::proportional(13.0), ink());
                    if resp.on_hover_text(l!("Збережені GIF", "Saved GIFs")).clicked() {
                        chosen = Some(PanelTab::Gifs);
                    }
                    let (r, resp) = tab_cell(ui, tab == PanelTab::Search);
                    icons::draw(ui.painter(), icon(r), Icon::Search, ink());
                    if resp.on_hover_text(l!("Пошук наборів", "Search sets")).clicked() {
                        chosen = Some(PanelTab::Search);
                    }
                    ui.painter().vline(ui.cursor().left() + 1.0, r.y_range().shrink(8.0), egui::Stroke::new(1.0, glass::veil(40)));
                    ui.add_space(4.0);
                    for (i, set) in self.stickers.sets.iter().enumerate() {
                        if set.stickers.is_empty() && !installed(set) {
                            continue;
                        }
                        let (r, resp) = tab_cell(ui, tab == PanelTab::Set(i));
                        let first = set.stickers.first();
                        match set.id {
                            FAVED_SET => icons::draw(ui.painter(), icon(r), Icon::Star, ink()),
                            RECENT_SET => icons::draw(ui.painter(), icon(r), Icon::Clock, ink()),
                            _ => match first {
                                Some((id, emoji)) => self.draw_sticker(ui.painter(), r.shrink(5.0), *id, emoji),
                                None => {
                                    ui.painter().text(r.center(), Align2::CENTER_CENTER, glass::initials(&set.title), FontId::proportional(15.0), ink());
                                }
                            },
                        }
                        if resp.on_hover_text(&set.title).clicked() {
                            chosen = Some(PanelTab::Set(i));
                        }
                    }
                    if self.stickers.sets.is_empty() && self.stickers.requested && self.backend.is_some() {
                        ui.add(egui::Spinner::new().color(ink_dim()));
                    }
                });
            });
        let gear = glass::icon_button(ui, manage, "sticker-manage", Icon::Gear, tab == PanelTab::Manage, true);
        if gear.on_hover_text(l!("Мої набори", "My sets")).clicked() {
            chosen = Some(PanelTab::Manage);
        }
        if let Some(t) = chosen {
            self.choose_tab(t);
        }
        let body = Rect::from_min_max(pos2(inner.left(), tabs.bottom() + 6.0), inner.max);
        match self.stickers.tab {
            PanelTab::Emoji => self.emoji_grid(ui, body),
            PanelTab::Gifs => self.gifs_view(ui, peer, body),
            PanelTab::Search => self.set_search_view(ui, body),
            PanelTab::Manage => self.manage_sets_view(ui, body),
            PanelTab::Set(i) => self.panel_set_view(ui, peer, body, i),
        }
    }

    fn choose_tab(&mut self, t: PanelTab) {
        self.stickers.tab = t;
        match t {
            PanelTab::Gifs if self.stickers.gifs.is_none() => {
                self.stickers.gifs = Some(Vec::new());
                self.send(Cmd::LoadSavedGifs);
            }
            PanelTab::Search if self.stickers.searched.is_none() => {
                self.stickers.searched = Some(String::new());
                self.send(Cmd::SearchStickerSets(String::new()));
            }
            PanelTab::Set(_) => self.request_set_thumbs(),
            _ => {}
        }
    }

    fn panel_set_view(&mut self, ui: &mut Ui, peer: PeerId, body: Rect, i: usize) {
        let Some(set) = self.stickers.sets.get(i) else {
            let text = if self.stickers.requested && self.backend.is_some() { l!("Завантаження…", "Loading…") } else { l!("Немає встановлених стікерів", "No stickers installed") };
            ui.painter().text(body.center(), Align2::CENTER_CENTER, text, FontId::proportional(14.0), ink_dim());
            return;
        };
        ui.painter().text(pos2(body.left() + 4.0, body.top()), Align2::LEFT_TOP, &set.title, FontId::proportional(13.0), ink_dim());
        if set.stickers.is_empty() {
            let hint = match set.id {
                FAVED_SET => l!("Клацніть правою кнопкою на стікері, щоб додати його в обрані", "Right-click a sticker to add it to Favourites"),
                _ => l!("Тут поки порожньо", "Nothing here yet"),
            };
            let g = ui.painter().layout(hint.into(), FontId::proportional(13.0), ink_dim(), body.width() - 40.0);
            ui.painter().galley(body.center() - g.size() / 2.0, g, ink_dim());
            return;
        }
        let grid = Rect::from_min_max(pos2(body.left(), body.top() + 22.0), body.max);
        let pick = self.sticker_grid(ui, grid, ("sticker-grid", set.id), &set.stickers, !installed(set));
        self.apply_pick(Some(peer), pick);
    }

    /// Saved GIFs: previews in three columns, a click sends one.
    fn gifs_view(&mut self, ui: &mut Ui, peer: PeerId, body: Rect) {
        let gifs = self.stickers.gifs.clone().unwrap_or_default();
        ui.painter().text(pos2(body.left() + 4.0, body.top()), Align2::LEFT_TOP, l!("Збережені GIF", "Saved GIFs"), FontId::proportional(13.0), ink_dim());
        if gifs.is_empty() {
            let hint = l!("Клацніть правою кнопкою на GIF у чаті й виберіть «Зберегти GIF»", "Right-click a GIF in a chat and choose “Save GIF”");
            let g = ui.painter().layout(hint.into(), FontId::proportional(13.0), ink_dim(), body.width() - 40.0);
            ui.painter().galley(body.center() - g.size() / 2.0, g, ink_dim());
            return;
        }
        self.want_sticker_thumbs(gifs.iter().copied());
        let grid = Rect::from_min_max(pos2(body.left(), body.top() + 22.0), body.max);
        let (mut send, mut unsave) = (None, None);
        let mut child = ui.new_child(UiBuilder::new().max_rect(grid));
        child.set_clip_rect(grid);
        egui::ScrollArea::vertical().id_salt("gif-grid").auto_shrink(false).show(&mut child, |ui| {
            let w = (grid.width() - 8.0) / 3.0;
            for row in gifs.chunks(3) {
                ui.horizontal(|ui| {
                    ui.spacing_mut().item_spacing.x = 4.0;
                    for id in row {
                        let (r, resp) = ui.allocate_exact_size(vec2(w, w * 0.75), Sense::click());
                        match self.textures.get(&ImgKey::Sticker(*id)) {
                            Some(t) => glass::image_rounded(ui.painter(), r, 10.0, t.id(), t.size_vec2()),
                            None => {
                                ui.painter().rect_filled(r, 10.0, glass::veil(16));
                                ui.painter().text(r.center(), Align2::CENTER_CENTER, "GIF", FontId::proportional(13.0), ink_dim());
                            }
                        }
                        if resp.hovered() {
                            ui.painter().rect_filled(r, 10.0, glass::veil(24));
                        }
                        resp.context_menu(|ui| {
                            if glass::menu_button(ui, Icon::Trash, l!("Прибрати зі збережених", "Remove from saved")).clicked() {
                                unsave = Some(*id);
                            }
                        });
                        if resp.on_hover_cursor(egui::CursorIcon::PointingHand).clicked() {
                            send = Some(*id);
                        }
                    }
                });
                ui.add_space(4.0);
            }
        });
        if let Some(id) = unsave {
            self.send(Cmd::SaveGif { doc: DocRef::Doc(id), unsave: true });
        }
        if let Some(doc) = send {
            let reply_to = self.take_reply();
            self.compose = Compose::New;
            self.send(Cmd::SendSticker { peer, doc, reply_to });
        }
    }

    /// Set search by name; with an empty field, the trending sets.
    fn set_search_view(&mut self, ui: &mut Ui, body: Rect) {
        let now = ui.input(|i| i.time);
        let field = Rect::from_min_size(body.min, vec2(body.width(), 36.0));
        if glass::search_field(ui, field, &mut self.stickers.query, l!("Пошук наборів стікерів", "Search sticker sets")).changed() {
            self.stickers.query_at = now;
        }
        let q = self.stickers.query.trim().to_owned();
        if self.stickers.searched.as_deref() != Some(q.as_str()) {
            if now - self.stickers.query_at >= SEARCH_DELAY {
                self.stickers.searched = Some(q.clone());
                self.send(Cmd::SearchStickerSets(q.clone()));
            } else {
                ui.ctx().request_repaint_after(std::time::Duration::from_secs_f64(SEARCH_DELAY));
            }
        }
        let title = if q.is_empty() { l!("Популярні набори", "Trending stickers") } else { l!("Знайдені набори", "Found sets") };
        ui.painter().text(pos2(body.left() + 4.0, field.bottom() + 8.0), Align2::LEFT_TOP, title, FontId::proportional(13.0), ink_dim());
        let list = Rect::from_min_max(pos2(body.left(), field.bottom() + 30.0), body.max);
        if self.stickers.found.is_empty() {
            let text = if self.backend.is_none() || self.stickers.searched.as_deref() == Some(q.as_str()) { l!("Нічого не знайдено", "Nothing found") } else { l!("Пошук…", "Searching…") };
            ui.painter().text(list.center(), Align2::CENTER_CENTER, text, FontId::proportional(14.0), ink_dim());
            return;
        }
        let found = self.stickers.found.clone();
        self.want_sticker_thumbs(found.iter().flat_map(|s| s.stickers.iter().map(|x| x.0)));
        let (mut open, mut install) = (None, None);
        let mut child = ui.new_child(UiBuilder::new().max_rect(list));
        child.set_clip_rect(list);
        egui::ScrollArea::vertical().id_salt("set-search").auto_shrink(false).show(&mut child, |ui| {
            for set in &found {
                let (row, resp) = ui.allocate_exact_size(vec2(list.width(), 92.0), Sense::click());
                if resp.hovered() {
                    ui.painter().rect_filled(row, 14.0, glass::veil(12));
                }
                let btn = Rect::from_min_size(pos2(row.right() - 96.0, row.top() + 8.0), vec2(88.0, 30.0));
                let p = ui.painter();
                p.galley(pos2(row.left() + 8.0, row.top() + 8.0), elided(ui, &set.title, 14.5, ink(), btn.left() - row.left() - 16.0), ink());
                let count = set.count;
                p.text(pos2(row.left() + 8.0, row.top() + 28.0), Align2::LEFT_TOP, lp!(count, ["{n} стікер", "{n} стікери", "{n} стікерів"], ["{n} sticker", "{n} stickers"]), FontId::proportional(12.5), ink_dim());
                for (k, (id, emoji)) in set.stickers.iter().enumerate() {
                    let r = Rect::from_min_size(pos2(row.left() + 6.0 + k as f32 * 46.0, row.top() + 46.0), vec2(42.0, 42.0));
                    if r.right() < row.right() {
                        self.draw_sticker(p, r, *id, emoji);
                    }
                }
                let (label, accent) = if set.installed { (l!("Додано", "Added"), false) } else { (l!("Додати", "Add"), true) };
                if glass::button(ui, btn, ("set-add", set.id), label, accent, true).clicked() {
                    install = Some((set.id, !set.installed));
                } else if resp.on_hover_cursor(egui::CursorIcon::PointingHand).clicked() {
                    open = Some(set.id);
                }
            }
        });
        if let Some((id, install)) = install {
            self.send(Cmd::InstallStickerSet { id, install });
        }
        if let Some(id) = open {
            self.open_sticker_set(StickerSetRef::Id(id));
        }
    }

    /// Installed sets in the panel's order: move up or down, remove.
    fn manage_sets_view(&mut self, ui: &mut Ui, body: Rect) {
        ui.painter().text(pos2(body.left() + 4.0, body.top()), Align2::LEFT_TOP, l!("Мої набори стікерів", "My sticker sets"), FontId::proportional(13.0), ink_dim());
        let list = Rect::from_min_max(pos2(body.left(), body.top() + 22.0), body.max);
        let rows: Vec<(usize, i64, String, usize, Option<(i64, String)>)> =
            self.stickers.sets.iter().enumerate().filter(|(_, s)| installed(s)).map(|(i, s)| (i, s.id, s.title.clone(), s.count, s.stickers.first().cloned())).collect();
        if rows.is_empty() {
            ui.painter().text(list.center(), Align2::CENTER_CENTER, l!("Немає встановлених стікерів", "No stickers installed"), FontId::proportional(14.0), ink_dim());
            return;
        }
        self.want_sticker_thumbs(rows.iter().filter_map(|r| r.4.as_ref().map(|x| x.0)));
        let (mut moved, mut removed, mut open) = (None, None, None);
        let mut child = ui.new_child(UiBuilder::new().max_rect(list));
        child.set_clip_rect(list);
        egui::ScrollArea::vertical().id_salt("sets-manage").auto_shrink(false).show(&mut child, |ui| {
            for (k, (i, id, title, count, first)) in rows.iter().enumerate() {
                let (row, resp) = ui.allocate_exact_size(vec2(list.width(), 52.0), Sense::click());
                if resp.hovered() {
                    ui.painter().rect_filled(row, 12.0, glass::veil(12));
                }
                let thumb = Rect::from_min_size(pos2(row.left() + 6.0, row.top() + 6.0), vec2(40.0, 40.0));
                if let Some((sid, emoji)) = first {
                    self.draw_sticker(ui.painter(), thumb, *sid, emoji);
                }
                let side = 32.0;
                let btn = |n: f32| Rect::from_min_size(pos2(row.right() - (side + 2.0) * n, row.center().y - side / 2.0), vec2(side, side));
                let text_w = btn(3.0).left() - thumb.right() - 16.0;
                ui.painter().galley(pos2(thumb.right() + 10.0, row.top() + 8.0), elided(ui, title, 14.0, ink(), text_w), ink());
                ui.painter().text(pos2(thumb.right() + 10.0, row.top() + 28.0), Align2::LEFT_TOP, lp!(*count, ["{n} стікер", "{n} стікери", "{n} стікерів"], ["{n} sticker", "{n} stickers"]), FontId::proportional(12.0), ink_dim());
                if glass::icon_button(ui, btn(3.0), ("set-up", id), Icon::ChevronUp, false, k > 0).on_hover_text(l!("Вище", "Move up")).clicked() && k > 0 {
                    moved = Some((*i, rows[k - 1].0));
                }
                let last = k + 1 == rows.len();
                if glass::icon_button(ui, btn(2.0), ("set-down", id), Icon::ChevronDown, false, !last).on_hover_text(l!("Нижче", "Move down")).clicked() && !last {
                    moved = Some((*i, rows[k + 1].0));
                }
                if glass::icon_button(ui, btn(1.0), ("set-remove", id), Icon::Trash, false, true).on_hover_text(l!("Видалити набір", "Remove set")).clicked() {
                    removed = Some(*id);
                } else if resp.on_hover_cursor(egui::CursorIcon::PointingHand).clicked() {
                    open = Some(*id);
                }
            }
        });
        if let Some((a, b)) = moved {
            self.stickers.sets.swap(a, b);
            let order = self.stickers.sets.iter().filter(|s| installed(s)).map(|s| s.id).collect();
            self.send(Cmd::ReorderStickerSets(order));
        }
        if let Some(id) = removed {
            self.send(Cmd::InstallStickerSet { id, install: false });
        }
        if let Some(id) = open {
            self.open_sticker_set(StickerSetRef::Id(id));
        }
    }

    /// The set box: all its stickers, Add or Remove, a link to share it.
    pub(super) fn sticker_set_box(&mut self, ui: &mut Ui) {
        let Some(slot) = &self.stickers.set_box else { return };
        let screen = ui.max_rect();
        ui.painter().rect_filled(screen, 0.0, Color32::from_black_alpha(90));
        let bg = ui.interact(screen, ui.scope_id().with("set-box-bg"), Sense::click());
        let rect = Rect::from_center_size(screen.center(), vec2(screen.width().min(440.0) - 20.0, (screen.height() - 60.0).min(560.0)));
        glass::glass(ui.painter(), rect, Glass::panel(26.0).with_shadow());
        let inner = rect.shrink(18.0);
        let close = Rect::from_center_size(pos2(inner.right() - 14.0, inner.top() + 14.0), vec2(32.0, 32.0));
        let esc = ui.input_mut(|i| i.consume_key(egui::Modifiers::NONE, Key::Escape));
        let closed = glass::icon_button(ui, close, "set-box-close", Icon::Close, false, true).clicked();
        let outside = bg.clicked() && !ui.input(|i| i.pointer.interact_pos()).is_some_and(|q| rect.contains(q));
        if esc || closed || outside {
            self.stickers.set_box = None;
            return;
        }
        let Some(set) = slot.clone() else {
            ui.put(Rect::from_center_size(rect.center(), vec2(32.0, 32.0)), egui::Spinner::new().color(ink_dim()));
            return;
        };
        let link = Rect::from_center_size(pos2(close.left() - 22.0, close.center().y), vec2(32.0, 32.0));
        let title_w = link.left() - inner.left() - 8.0;
        ui.painter().galley(inner.min, elided(ui, &set.title, 17.0, ink(), title_w), ink());
        let count = set.count;
        ui.painter().text(inner.min + vec2(0.0, 24.0), Align2::LEFT_TOP, lp!(count, ["{n} стікер", "{n} стікери", "{n} стікерів"], ["{n} sticker", "{n} stickers"]), FontId::proportional(12.5), ink_dim());
        if !set.short_name.is_empty() && glass::icon_button(ui, link, "set-box-link", Icon::Link, false, true).on_hover_text(l!("Копіювати посилання", "Copy link")).clicked() {
            self.ctx.copy_text(format!("https://t.me/addstickers/{}", set.short_name));
            self.toast = Some((l!("Посилання скопійовано", "Link copied").into(), 0.0, false));
        }
        self.want_sticker_thumbs(set.stickers.iter().map(|x| x.0));
        let footer = Rect::from_min_max(pos2(inner.left(), inner.bottom() - 38.0), inner.max);
        let grid = Rect::from_min_max(pos2(inner.left(), inner.top() + 50.0), pos2(inner.right(), footer.top() - 10.0));
        let mut pick = self.sticker_grid(ui, grid, ("set-box", set.id), &set.stickers, false);
        let (label, accent) = if set.installed { (lf!("Видалити стікери ({count})", "Remove {count} stickers"), false) } else { (lf!("Додати стікери ({count})", "Add {count} stickers"), true) };
        if glass::button(ui, footer, "set-box-install", &label, accent, self.backend.is_some() || self.demo).clicked() {
            self.send(Cmd::InstallStickerSet { id: set.id, install: !set.installed });
        }
        // A click sends the sticker to the open chat and closes the box, as in Telegram Desktop.
        let peer = self.selected.filter(|p| self.can_send_here(*p));
        if pick.send.is_some() {
            if peer.is_none() {
                pick.send = None;
            } else {
                self.stickers.set_box = None;
            }
        }
        self.apply_pick(peer, pick);
    }

    /// Whether the open chat takes messages from us (a channel we can't post in doesn't).
    fn can_send_here(&self, peer: PeerId) -> bool {
        self.chats.iter().find(|c| c.id == peer).is_none_or(|c| c.can_post || c.kind != ChatKind::Channel)
    }

    /// Message menu items that act on a sticker or GIF.
    pub(super) fn sticker_msg_action(&mut self, peer: PeerId, action: Action) {
        match action {
            Action::StickerSet(id) => self.open_sticker_set(StickerSetRef::Doc(DocRef::Msg(peer, id))),
            Action::FaveSticker(id) => self.send(Cmd::FaveSticker { doc: DocRef::Msg(peer, id), unfave: false }),
            Action::SaveGif(id) => self.send(Cmd::SaveGif { doc: DocRef::Msg(peer, id), unsave: false }),
            _ => {}
        }
    }

    pub(super) fn stickers_event(&mut self, ev: Ev) {
        match ev {
            Ev::Stickers(sets) => {
                self.stickers.sets = sets;
                // Open the first set with stickers, unless another tab is chosen.
                if let PanelTab::Set(i) = self.stickers.tab
                    && self.stickers.sets.get(i).is_none_or(|s| s.stickers.is_empty())
                {
                    self.stickers.tab = self.stickers.sets.iter().position(|s| !s.stickers.is_empty()).map_or(PanelTab::Emoji, PanelTab::Set);
                }
                self.request_set_thumbs();
            }
            Ev::StickerSet(set) => {
                if self.stickers.set_box.is_some() {
                    self.stickers.set_box = Some(Some(set));
                }
            }
            Ev::StickerSetChanged(set) => self.on_set_changed(set),
            Ev::FoundStickerSets { query, sets } => {
                if self.stickers.searched.as_deref() == Some(query.as_str()) {
                    self.stickers.found = sets;
                }
            }
            Ev::FavedStickers(faved) => {
                match self.stickers.sets.iter_mut().find(|s| s.id == FAVED_SET) {
                    Some(s) => {
                        s.count = faved.len();
                        s.stickers = faved;
                    }
                    None => self.stickers.sets.insert(
                        0,
                        StickerSetInfo { id: FAVED_SET, title: l!("Обрані", "Favourites").into(), short_name: String::new(), count: faved.len(), installed: true, stickers: faved },
                    ),
                }
                self.request_set_thumbs();
            }
            Ev::SavedGifs(gifs) => self.stickers.gifs = Some(gifs),
            _ => {}
        }
    }

    fn on_set_changed(&mut self, set: StickerSetInfo) {
        let (id, now) = (set.id, set.installed);
        let shown = match self.stickers.tab {
            PanelTab::Set(i) => self.stickers.sets.get(i).map(|s| s.id),
            _ => None,
        };
        self.stickers.sets.retain(|s| s.id != id);
        if now {
            // A new set goes on top of the installed ones, as in Telegram Desktop.
            let at = self.stickers.sets.iter().position(installed).unwrap_or(self.stickers.sets.len());
            self.stickers.sets.insert(at, set.clone());
            let title = &set.title;
            self.toast = Some((lf!("Набір «{title}» додано", "Sticker set “{title}” added"), 0.0, false));
        } else {
            self.toast = Some((l!("Набір стікерів видалено", "Sticker set removed").into(), 0.0, false));
        }
        // Keep the same set open in the panel after the list moved.
        if let PanelTab::Set(_) = self.stickers.tab {
            self.stickers.tab = shown.and_then(|sid| self.stickers.sets.iter().position(|s| s.id == sid)).map_or(PanelTab::Emoji, PanelTab::Set);
        }
        for s in self.stickers.found.iter_mut().filter(|s| s.id == id) {
            s.installed = now;
        }
        if let Some(Some(b)) = &mut self.stickers.set_box
            && b.id == id
        {
            b.installed = now;
        }
    }
}

/// What a click or menu did in a sticker grid.
#[derive(Default)]
struct GridPick {
    send: Option<i64>,
    /// (sticker, it was a favourite)
    fave: Option<(i64, bool)>,
    view: Option<i64>,
}

