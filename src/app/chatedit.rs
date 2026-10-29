//! "Edit" box of a group or channel, as in Telegram Desktop: photo, title, description,
//! default member permissions, slow mode, allowed reactions; hidden members and anti-spam (supergroups);
//! author signatures and the discussion group (channels); a way into "Recent actions".

use super::*;
use crate::backend::{ChatDetails, ChatRights, ReactionMode};

/// Slow mode steps of the official clients, in seconds.
const SLOW_MODES: [i32; 7] = [0, 10, 30, 60, 300, 900, 3600];
const TITLE_MAX: usize = 128;
const ABOUT_MAX: usize = 255;

pub(super) struct ChatEdit {
    peer: PeerId,
    title: String,
    about: String,
    rights: Option<ChatRights>,
    slowmode: Option<i32>,
    more: More,
    /// As loaded, to send only what changed.
    was: (String, String, Option<ChatRights>, Option<i32>),
    was_more: More,
    /// The linked discussion group (or the group's channel); changed at once, not on "Save".
    linked: Option<(PeerId, String)>,
    can_link: bool,
    admin_log: bool,
    /// Groups offered for linking, once asked for.
    groups: Option<Vec<ChatInfo>>,
}

/// Switches shown only when we may change them.
#[derive(Clone, PartialEq)]
struct More {
    reactions: Option<ReactionMode>,
    members_hidden: Option<bool>,
    antispam: Option<bool>,
    signatures: Option<bool>,
}

fn slow_label(s: i32) -> String {
    match s {
        0 => l!("Вимк.", "Off").to_owned(),
        s if s < 60 => lf!("{s} с", "{s}s"),
        s if s < 3600 => lf!("{} хв", "{}m", s / 60),
        s => lf!("{} год", "{}h", s / 3600),
    }
}

impl App {
    /// Opens the box with what the info panel has loaded.
    pub(super) fn open_chat_edit(&mut self, peer: PeerId, d: ChatDetails) {
        let title = self.chats.iter().find(|c| c.id == peer).map(|c| c.title.clone()).unwrap_or_default();
        let (about, rights, slowmode) = (d.about, d.rights, d.slowmode);
        let more = More { reactions: d.reactions, members_hidden: d.members_hidden, antispam: d.antispam, signatures: d.signatures };
        let was = (title.clone(), about.clone(), rights, slowmode);
        let (linked, can_link, admin_log) = (d.linked, d.can_link, d.admin_log);
        self.chat_edit = Some(ChatEdit { peer, title, about, rights, slowmode, was_more: more.clone(), more, was, linked, can_link, admin_log, groups: None });
    }

    pub(super) fn on_discussion_groups(&mut self, peer: PeerId, groups: Vec<ChatInfo>) {
        if let Some(e) = self.chat_edit.as_mut().filter(|e| e.peer == peer) {
            e.groups = Some(groups);
        }
    }

    fn pick_chat_photo(&self, peer: PeerId) {
        let (tx, ctx) = (self.chat_photo_tx.clone(), self.ctx.clone());
        let title = l!("Фото чату", "Chat photo");
        std::thread::spawn(move || {
            let file = rfd::FileDialog::new().set_title(title).add_filter("Images", &["jpg", "jpeg", "png", "webp"]).pick_file();
            if let Some(path) = file {
                let _ = tx.send((peer, path));
            }
            ctx.request_repaint();
        });
    }

    pub(super) fn take_chat_photos(&mut self) {
        let picked: Vec<(PeerId, PathBuf)> = self.chat_photo_rx.try_iter().collect();
        for (peer, path) in picked {
            self.send(Cmd::SetChatPhoto { peer, path });
        }
    }

    pub(super) fn chat_edit_view(&mut self, ui: &mut Ui) {
        let Some(e) = &mut self.chat_edit else { return };
        let peer = e.peer;
        let chat = self.chats.iter().find(|c| c.id == peer).cloned();
        let screen = ui.max_rect();
        ui.painter().rect_filled(screen, 0.0, Color32::from_black_alpha(90));
        ui.interact(screen, ui.scope_id().with("chat-edit-bg"), Sense::click());
        let rect = Rect::from_center_size(screen.center(), vec2(screen.width().min(440.0) - 20.0, (screen.height() - 60.0).min(640.0)));
        glass::glass(ui.painter(), rect, Glass::panel(26.0).with_shadow());
        let inner = rect.shrink(18.0);
        let channel = chat.as_ref().is_some_and(|c| c.kind == ChatKind::Channel);
        let title = if channel { l!("Редагувати канал", "Edit channel") } else { l!("Редагувати групу", "Edit group") };
        ui.painter().text(inner.left_top(), Align2::LEFT_TOP, title, FontId::proportional(17.0), ink());

        let buttons = Rect::from_min_max(pos2(inner.left(), inner.bottom() - 36.0), inner.max);
        let body = Rect::from_min_max(pos2(inner.left() - 4.0, inner.top() + 36.0), pos2(inner.right() + 4.0, buttons.top() - 10.0));
        let mut child = ui.new_child(UiBuilder::new().max_rect(body));
        child.set_clip_rect(body);
        let mut pick_photo = false;
        let (mut link_cmd, mut recent) = (None, false);
        egui::ScrollArea::vertical().id_salt("chat-edit").auto_shrink(false).show(&mut child, |ui| {
            ui.spacing_mut().item_spacing.y = 6.0;
            let w = ui.available_width() - 8.0;
            // Avatar with a "change photo" button next to it.
            let (row, _) = ui.allocate_exact_size(vec2(w, 72.0), Sense::hover());
            let tex = self.textures.get(&ImgKey::Avatar(peer));
            glass::avatar(ui.painter(), pos2(row.left() + 36.0, row.center().y), 32.0, &e.title, peer.bot_api_dialog_id_unchecked(), chat.as_ref().and_then(|c| kind_icon(c.kind)), tex);
            let b = Rect::from_min_size(pos2(row.left() + 84.0, row.center().y - 17.0), vec2(180.0, 34.0));
            pick_photo = glass::button(ui, b, "chat-edit-photo", l!("Змінити фото", "Set new photo"), false, true).clicked();

            section_title(ui, if channel { l!("Назва каналу", "Channel name") } else { l!("Назва групи", "Group name") });
            let r = ui.allocate_exact_size(vec2(w, 38.0), Sense::hover()).0;
            glass::text_field(ui, r, &mut e.title, l!("Назва", "Name"), false);
            section_title(ui, l!("Опис", "Description"));
            let r = ui.allocate_exact_size(vec2(w, 76.0), Sense::hover()).0;
            // Enter breaks lines here: the box is saved with its button.
            glass::text_area(ui, r, &mut e.about, l!("Опис (необов'язково)", "Description (optional)"), egui::Modifiers::COMMAND);
            if e.title.chars().count() > TITLE_MAX {
                e.title = e.title.chars().take(TITLE_MAX).collect();
            }
            if e.about.chars().count() > ABOUT_MAX {
                e.about = e.about.chars().take(ABOUT_MAX).collect();
            }

            if let Some(r) = &mut e.rights {
                section_title(ui, l!("Що можуть учасники", "What members can do"));
                ui.checkbox(&mut r.send_messages, l!("Надсилати повідомлення", "Send messages"));
                ui.checkbox(&mut r.send_media, l!("Надсилати медіа", "Send media"));
                ui.checkbox(&mut r.send_stickers, l!("Надсилати стікери й GIF", "Send stickers and GIFs"));
                ui.checkbox(&mut r.send_polls, l!("Надсилати опитування", "Send polls"));
                ui.checkbox(&mut r.embed_links, l!("Попередній перегляд посилань", "Embed links"));
                ui.checkbox(&mut r.invite_users, l!("Додавати учасників", "Add members"));
                ui.checkbox(&mut r.pin_messages, l!("Закріплювати повідомлення", "Pin messages"));
                ui.checkbox(&mut r.change_info, l!("Змінювати інформацію про групу", "Change group info"));
            }
            if let Some(slow) = &mut e.slowmode {
                section_title(ui, l!("Повільний режим", "Slow mode"));
                ui.horizontal_wrapped(|ui| {
                    ui.spacing_mut().item_spacing = vec2(6.0, 6.0);
                    for s in SLOW_MODES {
                        let (r, _) = ui.allocate_exact_size(vec2(56.0, 30.0), Sense::hover());
                        if glass::button(ui, r, ("slow-mode", s), &slow_label(s), *slow == s, true).clicked() {
                            *slow = s;
                        }
                    }
                });
                ui.label(egui::RichText::new(l!("Учасники зможуть писати одне повідомлення за вибраний проміжок.", "Members will be able to send one message per chosen interval.")).color(ink_dim()).size(12.5));
            }
            if let Some(mode) = &mut e.more.reactions {
                reactions_section(ui, mode);
            }
            if e.more.members_hidden.is_some() || e.more.antispam.is_some() {
                section_title(ui, l!("Учасники", "Members"));
            }
            if let Some(on) = &mut e.more.members_hidden {
                ui.checkbox(on, l!("Приховати учасників", "Hide members"));
                ui.label(egui::RichText::new(l!("Список учасників бачитимуть лише адміністратори.", "Only admins will see the list of members.")).color(ink_dim()).size(12.5));
            }
            if let Some(on) = &mut e.more.antispam {
                ui.checkbox(on, l!("Агресивний антиспам", "Aggressive anti-spam"));
                ui.label(egui::RichText::new(l!("Telegram сам видалятиме більше спаму.", "Telegram will remove more spam on its own.")).color(ink_dim()).size(12.5));
            }
            if let Some(on) = &mut e.more.signatures {
                section_title(ui, l!("Дописи", "Posts"));
                ui.checkbox(on, l!("Підписувати повідомлення", "Sign messages"));
                ui.label(egui::RichText::new(l!("Під дописами адміністраторів буде їхнє ім'я.", "Admins' names will be shown under their posts.")).color(ink_dim()).size(12.5));
            }
            if e.can_link || e.linked.is_some() {
                link_cmd = discussion_section(ui, e, channel);
            }
            if e.admin_log {
                ui.add_space(6.0);
                let r = ui.allocate_exact_size(vec2(w, 36.0), Sense::hover()).0;
                recent = glass::button(ui, r, "chat-edit-log", l!("Останні дії", "Recent actions"), false, true).clicked();
            }
        });

        let half = (buttons.width() - 8.0) / 2.0;
        let ready = !e.title.trim().is_empty();
        let esc = ui.input_mut(|i| i.consume_key(egui::Modifiers::NONE, Key::Escape));
        let cancel = glass::button(ui, Rect::from_min_size(buttons.min, vec2(half, buttons.height())), "chat-edit-cancel", l!("Скасувати", "Cancel"), false, true).clicked();
        let save = glass::button(ui, Rect::from_min_size(pos2(buttons.right() - half, buttons.top()), vec2(half, buttons.height())), "chat-edit-save", l!("Зберегти", "Save"), true, ready).clicked();
        if pick_photo {
            self.pick_chat_photo(peer);
        }
        if let Some(cmd) = link_cmd {
            self.send(cmd);
        }
        if recent {
            self.chat_edit = None;
            self.open_admin_log(peer);
            return;
        }
        if cancel || esc {
            self.chat_edit = None;
        } else if save && ready {
            let Some(e) = self.chat_edit.take() else { return };
            let (title, about) = (e.title.trim().to_owned(), e.about.trim().to_owned());
            let title = (title != e.was.0).then_some(title);
            let about = (about != e.was.1.trim()).then_some(about);
            if title.is_some() || about.is_some() {
                self.send(Cmd::EditChat { peer, title, about });
            }
            if let Some(rights) = e.rights.filter(|r| Some(*r) != e.was.2) {
                self.send(Cmd::SetChatRights { peer, rights });
            }
            if let Some(seconds) = e.slowmode.filter(|s| Some(*s) != e.was.3) {
                self.send(Cmd::SetSlowMode { peer, seconds });
            }
            let (now, was) = (e.more, e.was_more);
            if let Some(mode) = now.reactions.filter(|m| Some(m) != was.reactions.as_ref()) {
                self.send(Cmd::SetChatReactions { peer, mode });
            }
            if let Some(hidden) = now.members_hidden.filter(|h| Some(*h) != was.members_hidden) {
                self.send(Cmd::SetMembersHidden { peer, hidden });
            }
            if let Some(on) = now.antispam.filter(|a| Some(*a) != was.antispam) {
                self.send(Cmd::SetAntiSpam { peer, on });
            }
            if let Some(on) = now.signatures.filter(|s| Some(*s) != was.signatures) {
                self.send(Cmd::SetSignatures { peer, on });
            }
        }
    }
}

/// "Reactions": all, some (picked from the standard set) or none, as in Telegram Desktop.
fn reactions_section(ui: &mut Ui, mode: &mut ReactionMode) {
    section_title(ui, l!("Реакції", "Reactions"));
    ui.horizontal_wrapped(|ui| {
        ui.spacing_mut().item_spacing = vec2(6.0, 6.0);
        let w = ((ui.available_width() - 20.0) / 3.0).clamp(90.0, 130.0);
        let picked = match mode {
            ReactionMode::All => 0,
            ReactionMode::Some(_) => 1,
            ReactionMode::Off => 2,
        };
        let labels = [l!("Усі", "All"), l!("Деякі", "Some"), l!("Жодних", "None")];
        for (k, label) in labels.into_iter().enumerate() {
            let (r, _) = ui.allocate_exact_size(vec2(w, 30.0), Sense::hover());
            if glass::button(ui, r, ("reactions-mode", k), label, picked == k, true).clicked() && picked != k {
                *mode = match k {
                    0 => ReactionMode::All,
                    // Starts from the quick set, which every chat knows.
                    1 => ReactionMode::Some(emoji::QUICK_REACTIONS.iter().map(|e| (*e).to_owned()).collect()),
                    _ => ReactionMode::Off,
                };
            }
        }
    });
    let ReactionMode::Some(list) = mode else { return };
    ui.horizontal_wrapped(|ui| {
        ui.spacing_mut().item_spacing = vec2(4.0, 4.0);
        for e in emoji::STANDARD_REACTIONS {
            let on = list.iter().any(|x| x == e);
            let (r, _) = ui.allocate_exact_size(vec2(36.0, 36.0), Sense::hover());
            if glass::button(ui, r, ("reaction-pick", e), e, on, true).clicked() {
                if on {
                    list.retain(|x| x != e);
                } else {
                    list.push(e.to_owned());
                }
            }
        }
    });
    ui.label(egui::RichText::new(l!("Учасники зможуть ставити лише вибрані реакції.", "Members will be able to use only the chosen reactions.")).color(ink_dim()).size(12.5));
}

/// "Discussion": the linked group of a channel, picked from the groups that may be linked.
fn discussion_section(ui: &mut Ui, e: &mut ChatEdit, channel: bool) -> Option<Cmd> {
    section_title(ui, if channel { l!("Обговорення", "Discussion") } else { l!("Пов'язаний канал", "Linked channel") });
    let w = ui.available_width() - 8.0;
    let name = e.linked.as_ref().map_or(l!("Не додано", "Not added").to_owned(), |l| l.1.clone());
    ui.label(egui::RichText::new(name).color(ink()).size(14.0));
    let mut cmd = None;
    if !e.can_link {
        return None;
    }
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = 6.0;
        let half = ((w - 6.0) / 2.0).min(180.0);
        let (r, _) = ui.allocate_exact_size(vec2(half, 32.0), Sense::hover());
        if glass::button(ui, r, "discussion-pick", l!("Вибрати групу", "Choose a group"), false, e.groups.is_none()).clicked() {
            e.groups = Some(Vec::new());
            cmd = Some(Cmd::LoadDiscussionGroups(e.peer));
        }
        let (r, _) = ui.allocate_exact_size(vec2(half, 32.0), Sense::hover());
        if glass::button(ui, r, "discussion-unlink", l!("Відв'язати", "Unlink"), false, e.linked.is_some()).clicked() {
            e.linked = None;
            cmd = Some(Cmd::SetDiscussionGroup { peer: e.peer, group: None });
        }
    });
    if let Some(groups) = &e.groups {
        if groups.is_empty() {
            ui.label(egui::RichText::new(l!("Немає груп, де ви адміністратор із правом змінювати інформацію.", "No groups where you are an admin who can change info.")).color(ink_dim()).size(12.5));
        }
        let mut picked = None;
        for g in groups {
            let r = ui.allocate_exact_size(vec2(w, 34.0), Sense::hover()).0;
            let on = e.linked.as_ref().is_some_and(|l| l.0 == g.id);
            if glass::button(ui, r, ("discussion-group", g.id.bot_api_dialog_id_unchecked()), &g.title, on, true).clicked() && !on {
                picked = Some((g.id, g.title.clone()));
            }
        }
        if let Some((id, title)) = picked {
            e.linked = Some((id, title));
            e.groups = None;
            cmd = Some(Cmd::SetDiscussionGroup { peer: e.peer, group: Some(id) });
        }
    }
    ui.label(egui::RichText::new(l!("Коментарі до дописів з'являтимуться в цій групі.", "Comments on posts will appear in this group.")).color(ink_dim()).size(12.5));
    cmd
}
