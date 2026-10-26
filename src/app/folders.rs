//! Chat folders: unread counts, membership and the folder editor.

use super::*;

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

        let buttons = Rect::from_min_max(pos2(inner.left(), inner.bottom() - 38.0), inner.max);
        let body = Rect::from_min_max(pos2(inner.left(), emoji.bottom() + 12.0), pos2(inner.right(), buttons.top() - 10.0));
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
