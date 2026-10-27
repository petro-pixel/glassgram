//! Chat folders: unread counts, membership, the folder editor with its tag colour, folder tags on
//! chat rows and the folders Telegram suggests (Settings → Folders), as in Telegram Desktop.

use super::*;

/// Folder extras beyond the list itself.
#[derive(Default)]
pub(super) struct FolderMeta {
    /// Chat rows show the tags of the folders they are in (Premium).
    pub(super) tags: bool,
    /// Recommended folders and why.
    pub(super) suggested: Vec<(Folder, String)>,
}

/// Height of a tag capsule on a chat row.
pub(super) const TAG_H: f32 = 16.0;

/// Draws folder tags right to left from `right`; returns the width taken.
pub(super) fn draw_tags(ui: &Ui, tags: &[(String, i32)], right: f32, center_y: f32, max_w: f32) -> f32 {
    let mut x = right;
    for (title, color) in tags {
        let c = glass::tag_color(*color);
        let g = elided(ui, &title.to_uppercase(), 10.0, c, 90.0);
        let w = g.size().x + 10.0;
        if right - (x - w) > max_w {
            break;
        }
        let r = Rect::from_min_size(pos2(x - w, center_y - TAG_H / 2.0), vec2(w, TAG_H));
        ui.painter().rect_filled(r, TAG_H / 2.0, glass::rgba(c, 40));
        ui.painter().galley(r.center() - g.size() / 2.0, g, c);
        x -= w + 4.0;
    }
    right - x
}

impl App {
    pub(super) fn folder_unread(&self, folder: Option<i32>) -> usize {
        self.chats.iter().filter(|c| c.is_unread() && !c.muted && self.in_folder(c, folder)).count()
    }

    /// "All" excludes the archive, `ARCHIVE` is the archive only, otherwise the folder's rules.
    pub(super) fn in_folder(&self, c: &ChatInfo, folder: Option<i32>) -> bool {
        match folder {
            None => !c.archived,
            Some(ARCHIVE) => c.archived,
            Some(id) => self.folders.iter().find(|f| f.id == id).is_none_or(|f| f.contains(c)),
        }
    }

    /// Tags for a chat row: the coloured folders it is in, except the one open.
    pub(super) fn chat_tags(&self, c: &ChatInfo) -> Vec<(String, i32)> {
        if !self.folder_meta.tags {
            return Vec::new();
        }
        self.folders.iter().filter(|f| Some(f.id) != self.folder && f.contains(c)).filter_map(|f| Some((f.title.clone(), f.color?))).collect()
    }

    pub(super) fn add_suggested_folder(&mut self, i: usize) {
        if i >= self.folder_meta.suggested.len() {
            return;
        }
        let (mut f, _) = self.folder_meta.suggested.remove(i);
        // Ids 0 and 1 are reserved by Telegram ("All chats" and archive).
        f.id = self.folders.iter().map(|f| f.id).max().unwrap_or(1).max(1) + 1;
        self.send(Cmd::SaveFolder(f));
    }

    /// Settings → Folders extras: the tags switch and the recommended folders.
    pub(super) fn folder_extras(&mut self, ui: &mut Ui) {
        ui.add_space(10.0);
        section_title(ui, l!("Теги папок", "Folder tags"));
        let mut tags = self.folder_meta.tags;
        if ui.checkbox(&mut tags, l!("Показувати теги папок у списку чатів", "Show folder tags in the chat list")).changed() {
            self.folder_meta.tags = tags;
            self.send(Cmd::SetFolderTags(tags));
        }
        ui.label(egui::RichText::new(l!("Колір тегу задається в налаштуваннях папки. Потрібен Telegram Premium.", "A tag's colour is set in the folder's settings. Needs Telegram Premium.")).color(ink_dim()).size(12.5));
        if self.folder_meta.suggested.is_empty() {
            return;
        }
        ui.add_space(10.0);
        section_title(ui, l!("Рекомендовані папки", "Recommended folders"));
        let mut add = None;
        for (i, (f, about)) in self.folder_meta.suggested.iter().enumerate() {
            let (row, _) = ui.allocate_exact_size(vec2(ui.available_width(), 52.0), Sense::hover());
            let p = ui.painter();
            p.rect_filled(row, 14.0, glass::veil(10));
            icons::draw(p, Rect::from_center_size(pos2(row.left() + 22.0, row.center().y), vec2(20.0, 20.0)), Icon::Folder, ink_dim());
            let title = if f.emoticon.is_empty() { f.title.clone() } else { format!("{} {}", f.emoticon, f.title) };
            p.galley(pos2(row.left() + 48.0, row.top() + 8.0), elided(ui, &title, 14.5, ink(), row.width() - 160.0), ink());
            p.galley(pos2(row.left() + 48.0, row.top() + 28.0), elided(ui, about, 12.5, ink_dim(), row.width() - 160.0), ink_dim());
            let b = Rect::from_min_size(pos2(row.right() - 100.0, row.center().y - 15.0), vec2(90.0, 30.0));
            if glass::button(ui, b, ("folder-suggest", i), l!("Додати", "Add"), true, true).clicked() {
                add = Some(i);
            }
            ui.add_space(4.0);
        }
        if let Some(i) = add {
            self.add_suggested_folder(i);
        }
    }

    pub fn demo_folders(&self) -> Vec<Folder> {
        self.folders.clone()
    }

    pub(super) fn new_folder(&mut self) {
        // Ids 0 and 1 are reserved by Telegram ("All chats" and archive).
        let id = self.folders.iter().map(|f| f.id).max().unwrap_or(1).max(1) + 1;
        let folder = Folder { id, ..Default::default() };
        self.folder_edit = Some(FolderEdit { folder, new: true, query: String::new() });
    }

    /// Folder window: name, chat types, exceptions and individual chats.
    pub(super) fn folder_editor(&mut self, ui: &mut Ui) {
        let Some(ed) = &mut self.folder_edit else { return };
        let screen = ui.max_rect();
        ui.painter().rect_filled(screen, 0.0, Color32::from_black_alpha(90));
        ui.interact(screen, ui.scope_id().with("folder-bg"), Sense::click());
        let rect = Rect::from_center_size(screen.center(), vec2(screen.width().min(460.0) - 20.0, (screen.height() - 60.0).min(640.0)));
        glass::glass(ui.painter(), rect, Glass::panel(26.0).with_shadow());
        let inner = rect.shrink(18.0);
        let title = if ed.new { l!("Нова папка", "New folder") } else { l!("Папка", "Folder") };
        ui.painter().text(inner.left_top() + vec2(4.0, 2.0), Align2::LEFT_TOP, title, FontId::proportional(18.0), ink());
        let close = Rect::from_min_size(pos2(inner.right() - 32.0, inner.top() - 4.0), vec2(32.0, 32.0));
        let mut done = glass::icon_button(ui, close, "folder-close", Icon::Close, false, true).clicked();

        let emoji = Rect::from_min_size(inner.left_top() + vec2(0.0, 40.0), vec2(52.0, 38.0));
        glass::text_field(ui, emoji, &mut ed.folder.emoticon, "📁", false);
        if ed.folder.emoticon.chars().count() > 2 {
            ed.folder.emoticon = ed.folder.emoticon.chars().take(2).collect();
        }
        let name = Rect::from_min_max(pos2(emoji.right() + 8.0, emoji.top()), pos2(inner.right(), emoji.bottom()));
        let r = glass::text_field(ui, name, &mut ed.folder.title, l!("Назва папки", "Folder name"), false);
        if ed.new && ed.folder.title.is_empty() && !r.has_focus() {
            r.request_focus();
        }
        if ed.folder.title.chars().count() > FOLDER_TITLE_MAX {
            ed.folder.title = ed.folder.title.chars().take(FOLDER_TITLE_MAX).collect();
        }

        // Tag colour: a row of dots, the crossed one first for "no tag".
        let dots_y = emoji.bottom() + 22.0;
        ui.painter().text(pos2(inner.left() + 4.0, dots_y), Align2::LEFT_CENTER, l!("Колір тегу", "Tag colour"), FontId::proportional(13.0), ink_dim());
        for k in -1..glass::TAG_COLORS as i32 {
            let c = pos2(inner.right() - 14.0 - (glass::TAG_COLORS as i32 - 1 - k) as f32 * 30.0, dots_y);
            let r = Rect::from_center_size(c, vec2(26.0, 26.0));
            let resp = ui.interact(r, ui.scope_id().with(("tag-color", k)), Sense::click()).on_hover_cursor(egui::CursorIcon::PointingHand);
            let on = ed.folder.color.unwrap_or(-1) == k;
            let p = ui.painter();
            if k < 0 {
                p.circle_stroke(c, 9.0, egui::Stroke::new(1.5, ink_dim()));
                p.line_segment([c + vec2(-6.0, 6.0), c + vec2(6.0, -6.0)], egui::Stroke::new(1.5, ink_dim()));
            } else {
                p.circle_filled(c, 9.0, glass::tag_color(k));
            }
            if on {
                p.circle_stroke(c, 12.5, egui::Stroke::new(2.0, ink()));
            }
            if resp.clicked() {
                ed.folder.color = (k >= 0).then_some(k);
            }
        }

        let buttons = Rect::from_min_max(pos2(inner.left(), inner.bottom() - 38.0), inner.max);
        let body = Rect::from_min_max(pos2(inner.left(), dots_y + 22.0), pos2(inner.right(), buttons.top() - 10.0));
        let mut child = ui.new_child(UiBuilder::new().max_rect(body));
        child.set_clip_rect(body);
        let q = ed.query.trim().to_lowercase();
        let chats = &self.chats;
        let textures = &self.textures;
        egui::ScrollArea::vertical().id_salt("folder-body").auto_shrink(false).show(&mut child, |ui| {
            let f = &mut ed.folder;
            if f.shared {
                ui.label(egui::RichText::new(l!("Це спільна папка за посиланням — її правила змінюються в офіційному клієнті.", "This is a shared folder from a link — its rules are changed in the official app.")).color(ink_dim()));
                return;
            }
            section_title(ui, l!("Включити типи чатів", "Include chat types"));
            ui.checkbox(&mut f.contacts, l!("Контакти", "Contacts"));
            ui.checkbox(&mut f.non_contacts, l!("Не контакти", "Non-contacts"));
            ui.checkbox(&mut f.groups, l!("Групи", "Groups"));
            ui.checkbox(&mut f.broadcasts, l!("Канали", "Channels"));
            ui.checkbox(&mut f.bots, l!("Боти", "Bots"));
            ui.add_space(6.0);
            section_title(ui, l!("Не показувати", "Exclude"));
            ui.checkbox(&mut f.exclude_muted, l!("Без сповіщень", "Muted"));
            ui.checkbox(&mut f.exclude_read, l!("Прочитані", "Read"));
            ui.checkbox(&mut f.exclude_archived, l!("Архівовані", "Archived"));
            ui.add_space(6.0);
            section_title(ui, l!("Окремі чати (клік: додати → виключити → прибрати)", "Specific chats (click: include → exclude → remove)"));
            let field = ui.allocate_exact_size(vec2(ui.available_width(), 36.0), Sense::hover()).0;
            glass::search_field(ui, field, &mut ed.query, l!("Пошук чату…", "Search chats…"));
            ui.add_space(4.0);
            // First those already in the rules, then the rest.
            let state = |f: &Folder, id: &PeerId| if f.include.contains(id) { 1 } else if f.exclude.contains(id) { 2 } else { 0 };
            let mut list: Vec<&ChatInfo> = chats.iter().filter(|c| c.title.to_lowercase().contains(&q)).collect();
            list.sort_by_key(|c| state(f, &c.id) == 0);
            for c in list {
                let st = state(f, &c.id);
                let hit = SearchHit { chat: c.title.clone(), kind: c.kind, msg: Msg::placeholder(c.id) };
                let resp = hit_row(ui, &hit, "", textures.get(&ImgKey::Avatar(c.id)));
                let mark = Rect::from_center_size(pos2(resp.rect.right() - 22.0, resp.rect.center().y), vec2(22.0, 22.0));
                match st {
                    1 => icons::draw(ui.painter(), mark, Icon::Check, Color32::from_rgb(110, 230, 150)),
                    2 => icons::draw(ui.painter(), mark, Icon::Close, Color32::from_rgb(255, 110, 120)),
                    _ => {}
                }
                if resp.clicked() {
                    match st {
                        0 => f.include.push(c.id),
                        1 => {
                            f.include.retain(|x| *x != c.id);
                            f.pinned.retain(|x| *x != c.id);
                            f.exclude.push(c.id);
                        }
                        _ => f.exclude.retain(|x| *x != c.id),
                    }
                }
            }
        });

        let f = &ed.folder;
        let has_rules = f.contacts || f.non_contacts || f.groups || f.broadcasts || f.bots || !f.include.is_empty();
        let valid = !f.title.trim().is_empty() && has_rules && !f.shared;
        let w = if ed.new { buttons.width() } else { (buttons.width() - 8.0) / 2.0 };
        let save = Rect::from_min_size(pos2(buttons.right() - w, buttons.top()), vec2(w, buttons.height()));
        let save_resp = glass::button(ui, save, "folder-save", l!("Зберегти", "Save"), true, valid);
        let hint = if f.title.trim().is_empty() { l!("Вкажіть назву", "Enter a name") } else { l!("Додайте хоча б один тип чатів або чат", "Add at least one chat type or chat") };
        let save_clicked = if valid { save_resp.clicked() } else { save_resp.on_disabled_hover_text(hint).clicked() };
        let mut delete = false;
        if !ed.new {
            let del = Rect::from_min_size(buttons.min, vec2(w, buttons.height()));
            delete = glass::button(ui, del, "folder-delete", l!("Видалити папку", "Delete folder"), false, true).clicked();
        }
        let cmd = if save_clicked && valid {
            let mut f = ed.folder.clone();
            f.title = f.title.trim().to_owned();
            Some(Cmd::SaveFolder(f))
        } else if delete {
            Some(Cmd::DeleteFolder(ed.folder.id))
        } else {
            None
        };
        if let Some(cmd) = cmd {
            self.send(cmd);
            done = true;
        }
        if done || ui.input(|i| i.key_pressed(Key::Escape)) {
            self.folder_edit = None;
        }
    }
}
