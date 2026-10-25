//! Contacts as in Telegram Desktop: the "New contact" box (name and phone), adding a person
//! from their profile, deleting and blocking, and the blocked list in Privacy and Security.

use super::*;

/// "New contact" / "Add to contacts" box.
pub(super) struct ContactForm {
    /// Adding a known user from their profile; `None` — import by phone.
    user: Option<PeerId>,
    first: String,
    last: String,
    phone: String,
    busy: bool,
}

/// A phone number worth sending: enough digits, only phone characters.
fn phone_ok(s: &str) -> bool {
    let digits = s.chars().filter(char::is_ascii_digit).count();
    digits >= 5 && s.chars().all(|c| c.is_ascii_digit() || " +-()".contains(c))
}


impl App {
    /// `user`: a person from their profile, with their name to prefill.
    pub(super) fn open_contact_form(&mut self, user: Option<(PeerId, &str)>) {
        let (first, last) = user.map_or_else(Default::default, |(_, name)| match name.split_once(' ') {
            Some((f, l)) => (f.to_owned(), l.to_owned()),
            None => (name.to_owned(), String::new()),
        });
        self.contact_form = Some(ContactForm { user: user.map(|u| u.0), first, last, phone: String::new(), busy: false });
    }

    pub(super) fn contacts_event(&mut self, ev: Ev) {
        match ev {
            Ev::ContactSaved(c) => {
                let imported = self.contact_form.as_ref().is_some_and(|f| f.user.is_none());
                self.contact_form = None;
                let id = c.id;
                match self.chat_mut(id) {
                    Some(chat) => {
                        chat.contact = true;
                        chat.title = c.title.clone();
                    }
                    None if imported => self.chats.push(c.clone()),
                    None => {}
                }
                self.contacts.retain(|x| x.id != id);
                self.contacts.push(c);
                self.contacts.sort_by_key(|x| x.title.to_lowercase());
                self.chats_dirty = true;
                self.toast = Some((l!("Контакт збережено", "Contact saved").into(), 0.0, false));
                // A new number opens its chat, as in Telegram Desktop.
                if imported {
                    if self.backend.is_none() {
                        self.history.entry(id).or_default();
                    }
                    self.open_chat(id);
                }
            }
            Ev::ContactRemoved(peer) => {
                if let Some(c) = self.chat_mut(peer) {
                    c.contact = false;
                }
                self.contacts.retain(|c| c.id != peer);
                self.chats_dirty = true;
                self.toast = Some((l!("Контакт видалено", "Contact deleted").into(), 0.0, false));
            }
            Ev::Blocked { peer, blocked } => {
                self.info_blocked(peer, blocked);
                if let Some(list) = &mut self.blocked {
                    if blocked {
                        // The list needs the name and avatar: ask for it again.
                        self.send(Cmd::LoadBlocked);
                    } else {
                        list.retain(|c| c.id != peer);
                    }
                }
                let bot = self.chats.iter().any(|c| c.id == peer && c.kind == ChatKind::Bot);
                let text = match (blocked, bot) {
                    (true, true) => l!("Бота зупинено", "Bot stopped"),
                    (true, false) => l!("Користувача заблоковано", "User blocked"),
                    (false, true) => l!("Бота перезапущено", "Bot restarted"),
                    (false, false) => l!("Користувача розблоковано", "User unblocked"),
                };
                self.toast = Some((text.into(), 0.0, false));
            }
            Ev::BlockedList(list) => self.blocked = Some(list),
            _ => {}
        }
    }

    /// The request failed: let the box be sent again.
    pub(super) fn contact_failed(&mut self) {
        if let Some(f) = &mut self.contact_form {
            f.busy = false;
        }
    }

    pub(super) fn contact_form_view(&mut self, ui: &mut Ui) {
        let Some(f) = &mut self.contact_form else { return };
        let screen = ui.max_rect();
        ui.painter().rect_filled(screen, 0.0, Color32::from_black_alpha(90));
        ui.interact(screen, ui.scope_id().with("contact-form-bg"), Sense::click());
        let by_phone = f.user.is_none();
        let rect = Rect::from_center_size(screen.center(), vec2(380.0, if by_phone { 300.0 } else { 250.0 }));
        glass::glass(ui.painter(), rect, Glass::panel(24.0).with_shadow());
        let inner = rect.shrink(18.0);
        let title = if by_phone { l!("Новий контакт", "New contact") } else { l!("Додати в контакти", "Add to contacts") };
        ui.painter().text(inner.left_top(), Align2::LEFT_TOP, title, FontId::proportional(17.0), ink());
        let mut y = inner.top() + 40.0;
        let mut field = |ui: &mut Ui, text: &mut String, hint: &str| {
            let r = Rect::from_min_size(pos2(inner.left(), y), vec2(inner.width(), 38.0));
            y += 46.0;
            glass::text_field(ui, r, text, hint, false)
        };
        let first = field(ui, &mut f.first, l!("Ім'я (обов'язково)", "First name (required)"));
        field(ui, &mut f.last, l!("Прізвище (необов'язково)", "Last name (optional)"));
        if by_phone {
            field(ui, &mut f.phone, l!("Номер телефону", "Phone number"));
        }
        if f.first.is_empty() && !first.has_focus() && !ui.memory(|m| m.focused().is_some()) {
            first.request_focus();
        }
        let ready = !f.busy && !f.first.trim().is_empty() && (!by_phone || phone_ok(&f.phone));
        let half = (inner.width() - 8.0) / 2.0;
        let by = inner.bottom() - 36.0;
        let esc = ui.input_mut(|i| i.consume_key(egui::Modifiers::NONE, Key::Escape));
        let cancel = glass::button(ui, Rect::from_min_size(pos2(inner.left(), by), vec2(half, 36.0)), "contact-cancel", l!("Скасувати", "Cancel"), false, true).clicked();
        let label = if f.busy { l!("Зберігаю…", "Saving…") } else if by_phone { l!("Додати", "Add") } else { l!("Готово", "Done") };
        let enter = ready && ui.input(|i| i.key_pressed(Key::Enter));
        let save = glass::button(ui, Rect::from_min_size(pos2(inner.right() - half, by), vec2(half, 36.0)), "contact-save", label, true, ready).clicked() || enter;
        if cancel || esc {
            self.contact_form = None;
        } else if save && ready {
            f.busy = true;
            let cmd = Cmd::AddContact { user: f.user, phone: f.phone.trim().to_owned(), first: f.first.trim().to_owned(), last: f.last.trim().to_owned() };
            self.send(cmd);
        }
    }

    /// "Blocked users" in Privacy and Security, with unblock buttons.
    pub(super) fn blocked_section(&mut self, ui: &mut Ui) {
        section_title(ui, l!("Заблоковані користувачі", "Blocked users"));
        let Some(list) = &self.blocked else {
            ui.add(egui::Spinner::new().color(ink_dim()));
            return;
        };
        if list.is_empty() {
            ui.label(egui::RichText::new(l!("Нікого не заблоковано", "Nobody is blocked")).color(ink_dim()).size(12.5));
            return;
        }
        let mut unblock = None;
        for c in list {
            let w = ui.available_width();
            let (r, _) = ui.allocate_exact_size(vec2(w, 48.0), Sense::hover());
            let p = ui.painter();
            glass::avatar(p, pos2(r.left() + 26.0, r.center().y), 18.0, &c.title, c.id.bot_api_dialog_id_unchecked(), None, self.textures.get(&ImgKey::Avatar(c.id)));
            p.galley(pos2(r.left() + 54.0, r.center().y - 9.0), elided(ui, &c.title, 14.5, ink(), w - 200.0), ink());
            let b = Rect::from_min_size(pos2(r.right() - 132.0, r.center().y - 16.0), vec2(128.0, 32.0));
            if glass::button(ui, b, ("unblock", c.id), l!("Розблокувати", "Unblock"), false, true).clicked() {
                unblock = Some(c.id);
            }
        }
        if let Some(peer) = unblock {
            self.send(Cmd::Block { peer, blocked: false });
        }
    }
}
