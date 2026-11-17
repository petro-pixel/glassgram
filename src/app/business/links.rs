//! Links to chat (short links that open a chat with us and suggest the first message) and
//! connected chatbots.

use super::*;

/// The link form: a new one has no slug.
#[derive(Default)]
pub(in crate::app) struct LinkEdit {
    slug: Option<String>,
    title: String,
    message: String,
}

impl App {
    pub(in crate::app) fn chat_links_page(&mut self, ui: &mut Ui) {
        hint(ui, l!(
            "Дайте клієнтам короткі посилання, що відкривають чат з вами й пропонують перше повідомлення.",
            "Give your customers short links that start a chat with you — and suggest the first message from them to you."
        ));
        ui.add_space(8.0);
        let mut cmd = None;
        let mut copy = None;
        let b = &mut self.business;
        let mut close = false;
        if let Some(edit) = &mut b.link_edit {
            egui::Frame::new().fill(glass::veil(10)).corner_radius(14.0).inner_margin(egui::Margin::same(12)).show(ui, |ui| {
                ui.set_width(ui.available_width());
                let r = ui.allocate_exact_size(vec2(ui.available_width(), 72.0), Sense::hover()).0;
                glass::text_area(ui, r, &mut edit.message, l!("Повідомлення, яке запропонує посилання", "Message the link suggests"), egui::Modifiers::CTRL);
                ui.add_space(6.0);
                field(ui, &mut edit.title, l!("Назва посилання (бачите лише ви)", "Link name (only you see it)"));
                ui.add_space(6.0);
                ui.horizontal(|ui| {
                    let r = ui.allocate_exact_size(vec2(130.0, 32.0), Sense::hover()).0;
                    if glass::button(ui, r, "link-save", l!("Зберегти", "Save"), true, true).clicked() {
                        cmd = Some(BusinessCmd::SaveLink { slug: edit.slug.clone(), title: edit.title.clone(), message: edit.message.trim().to_owned() });
                        close = true;
                    }
                    let r = ui.allocate_exact_size(vec2(130.0, 32.0), Sense::hover()).0;
                    if glass::button(ui, r, "link-cancel", l!("Скасувати", "Cancel"), false, true).clicked() {
                        close = true;
                    }
                });
            });
        } else if wide_button(ui, "link-new", l!("Створити посилання на чат", "Create a Link to Chat"), true, true) {
            b.link_edit = Some(LinkEdit::default());
        }
        if close {
            b.link_edit = None;
        }
        ui.add_space(8.0);
        match &b.links {
            None => {
                ui.add(egui::Spinner::new().color(ink_dim()));
            }
            Some(list) if list.is_empty() => hint(ui, l!("Посилань ще немає.", "No links yet.")),
            Some(list) => {
                for link in list {
                    let open = b.link_open.as_deref() == Some(link.link.as_str());
                    if link_row(ui, link, open).clicked() {
                        b.link_open = if open { None } else { Some(link.link.clone()) };
                        b.armed = None;
                    }
                    if open {
                        let slug = link.slug().to_owned();
                        ui.horizontal(|ui| {
                            ui.spacing_mut().item_spacing.x = 6.0;
                            if info::chip(ui, l!("Копіювати", "Copy"), false).clicked() {
                                copy = Some(link.link.clone());
                            }
                            if info::chip(ui, l!("Змінити", "Edit"), false).clicked() {
                                b.link_edit = Some(LinkEdit { slug: Some(slug.clone()), title: link.title.clone(), message: link.message.clone() });
                            }
                            let armed = b.armed.as_deref() == Some(slug.as_str());
                            if info::chip(ui, if armed { l!("Точно видалити?", "Delete for sure?") } else { l!("Видалити", "Delete") }, armed).clicked() {
                                if armed {
                                    cmd = Some(BusinessCmd::DeleteLink(slug.clone()));
                                    b.armed = None;
                                } else {
                                    b.armed = Some(slug.clone());
                                }
                            }
                        });
                        ui.add_space(4.0);
                    }
                    ui.add_space(4.0);
                }
            }
        }
        if let Some(link) = copy {
            self.ctx.copy_text(link);
            self.toast = Some((l!("Посилання скопійовано", "Link copied").into(), 0.0, false));
        }
        if let Some(cmd) = cmd {
            self.send(Cmd::Business(cmd));
        }
    }

    pub(in crate::app) fn bots_page(&mut self, ui: &mut Ui) {
        hint(ui, l!(
            "Додайте бота, який читатиме ваші особисті чати й відповідатиме від вашого імені. Він не матиме доступу до контактів.",
            "Add a bot to your account to help you automatically process and respond to the messages you receive. It won't reach your contacts."
        ));
        ui.add_space(8.0);
        let mut cmd = None;
        let b = &mut self.business;
        ui.horizontal(|ui| {
            let r = ui.allocate_exact_size(vec2(ui.available_width() - 96.0, 36.0), Sense::hover()).0;
            glass::text_field(ui, r, &mut b.new_name, l!("Ім'я бота, @bot", "Bot username, @bot"), false);
            let name = b.new_name.trim().trim_start_matches('@').to_owned();
            let r = ui.allocate_exact_size(vec2(88.0, 36.0), Sense::hover()).0;
            if glass::button(ui, r, "bot-add", l!("Додати", "Add"), true, name.len() >= 5).clicked() {
                cmd = Some(BusinessCmd::ConnectBot(name));
                b.new_name.clear();
            }
        });
        ui.add_space(8.0);
        match &b.bots {
            None => {
                ui.add(egui::Spinner::new().color(ink_dim()));
            }
            Some(list) if list.is_empty() => hint(ui, l!("Жодного бота не підключено.", "No bots connected.")),
            Some(list) => {
                section_title(ui, l!("Підключені боти", "Connected bots"));
                for bot in list {
                    let avatar = self.textures.get(&ImgKey::Avatar(bot.id));
                    let (r, _) = ui.allocate_exact_size(vec2(ui.available_width(), 52.0), Sense::hover());
                    let p = ui.painter();
                    glass::avatar(p, pos2(r.left() + 24.0, r.center().y), 18.0, &bot.name, bot.id.bot_api_dialog_id_unchecked(), Some(Icon::Bot), avatar);
                    p.galley(pos2(r.left() + 52.0, r.top() + 8.0), elided(ui, &bot.name, 14.5, ink(), r.width() - 170.0), ink());
                    let rights = if bot.can_reply { l!("Може відповідати", "Can reply") } else { l!("Лише читає", "Reads only") };
                    let sub = if bot.username.is_empty() { rights.to_owned() } else { format!("@{} · {rights}", bot.username) };
                    p.galley(pos2(r.left() + 52.0, r.top() + 30.0), elided(ui, &sub, 12.5, ink_dim(), r.width() - 170.0), ink_dim());
                    let key = format!("bot-{}", bot.id.bot_api_dialog_id_unchecked());
                    let armed = b.armed.as_deref() == Some(key.as_str());
                    let button = Rect::from_min_size(pos2(r.right() - 110.0, r.center().y - 16.0), vec2(106.0, 32.0));
                    let label = if armed { l!("Точно?", "Sure?") } else { l!("Відключити", "Remove") };
                    if glass::tone_button(ui, button, ("bot-remove", &key), label, glass::danger()).clicked() {
                        if armed {
                            cmd = Some(BusinessCmd::DisconnectBot(bot.id));
                            b.armed = None;
                        } else {
                            b.armed = Some(key);
                        }
                    }
                }
            }
        }
        if let Some(cmd) = cmd {
            self.send(Cmd::Business(cmd));
        }
    }
}

/// Title (or the link), the suggested message and the clicks.
fn link_row(ui: &mut Ui, link: &ChatLink, open: bool) -> egui::Response {
    let (r, resp) = ui.allocate_exact_size(vec2(ui.available_width(), 58.0), Sense::click());
    let p = ui.painter();
    p.rect_filled(r, 14.0, glass::veil(if open || resp.hovered() { 18 } else { 8 }));
    if resp.hovered() {
        ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
    }
    let shown = link.link.trim_start_matches("https://");
    let top = if link.title.is_empty() { shown.to_owned() } else { link.title.clone() };
    let n = link.views;
    let views = lp!(n, ["{n} перехід", "{n} переходи", "{n} переходів"], ["{n} click", "{n} clicks"]);
    let vg = p.layout_no_wrap(views, FontId::proportional(12.0), ink_dim());
    let vw = vg.size().x;
    p.galley(pos2(r.right() - 12.0 - vw, r.top() + 11.0), vg, ink_dim());
    p.galley(pos2(r.left() + 12.0, r.top() + 9.0), elided(ui, &top, 14.5, ink(), r.width() - vw - 36.0), ink());
    let sub = if link.message.is_empty() { shown.to_owned() } else { link.message.replace('\n', " ") };
    p.galley(pos2(r.left() + 12.0, r.top() + 32.0), elided(ui, &sub, 12.5, ink_dim(), r.width() - 24.0), ink_dim());
    resp
}
