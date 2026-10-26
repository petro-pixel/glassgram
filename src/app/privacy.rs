//! Privacy and Security page extras, as in Telegram Desktop: "always" / "never" exceptions of each
//! rule, and the global switches — new chats from non-contacts, read times, who can message me.

use super::*;

/// Privacy and Security page: rules, their exceptions, the open rule, the people picker, global switches.
pub(super) struct PrivacyPage {
    /// Who may see each item.
    pub(super) rules: HashMap<PrivacyKey, PrivacyState>,
    /// "Always" / "never" lists of each privacy rule.
    pub(super) exceptions: HashMap<PrivacyKey, backend::PrivacyExceptions>,
    /// The rule and side whose exception list is unfolded.
    pub(super) open: Option<(PrivacyKey, bool)>,
    /// The people picker for an exception list.
    pub(super) pick: Option<PrivacyPick>,
    /// Global privacy switches (`None` — not loaded yet).
    pub(super) global: Option<backend::GlobalPrivacy>,
}

impl App {
    pub(super) fn global_privacy_section(&mut self, ui: &mut Ui) {
        let Some(before) = self.privacy.global else {
            section_title(ui, l!("Повідомлення", "Messages"));
            ui.add(egui::Spinner::new().color(ink_dim()));
            return;
        };
        let mut p = before;
        let note = |ui: &mut Ui, text: &str| {
            ui.label(egui::RichText::new(text).color(ink_dim()).size(12.5));
        };

        section_title(ui, l!("Хто може мені писати", "Who can message me"));
        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = 6.0;
            for (premium_only, label) in [(false, l!("Усі", "Everybody")), (true, l!("Контакти й Premium", "Contacts and Premium"))] {
                let (r, _) = ui.allocate_exact_size(vec2(180.0, 30.0), Sense::hover());
                if glass::button(ui, r, ("who-can-message", premium_only), label, p.premium_only == premium_only, true).clicked() {
                    p.premium_only = premium_only;
                }
            }
        });
        note(ui, l!("Обмежити нові чати контактами й користувачами Premium може лише Telegram Premium.", "Limiting new chats to contacts and Premium users needs Telegram Premium."));

        section_title(ui, l!("Нові чати не з контактів", "New chats from non-contacts"));
        ui.checkbox(&mut p.archive_new, l!("Архівувати й вимикати звук", "Archive and mute"));
        ui.checkbox(&mut p.keep_unmuted, l!("Залишати в архіві чати зі звуком", "Keep unmuted chats archived"));
        ui.checkbox(&mut p.keep_folders, l!("Залишати в архіві чати з папок", "Keep chats from folders archived"));
        note(ui, l!("Архівовані чати повертаються в список із новим повідомленням, якщо ці пункти вимкнено.", "With these off, archived chats come back to the list on a new message."));

        section_title(ui, l!("Час прочитання", "Read time"));
        ui.checkbox(&mut p.hide_read, l!("Приховувати час прочитання", "Hide read time"));
        note(ui, l!("Інші не бачитимуть, коли ви прочитали їхні повідомлення, а ви — їхній час прочитання.", "Others won't see when you've read their messages, and you won't see theirs."));

        if p != before {
            self.privacy.global = Some(p);
            self.send(Cmd::SetGlobalPrivacy(p));
        }
    }
}

/// "Add an exception" picker: a person or a group for one rule's "always" or "never" list.
pub(super) struct PrivacyPick {
    key: PrivacyKey,
    allow: bool,
    query: String,
}

impl App {
    fn exceptions(&self, key: PrivacyKey) -> (Vec<PeerId>, Vec<PeerId>) {
        self.privacy.exceptions.get(&key).map_or_else(Default::default, |e| (e.allow.iter().map(|c| c.id).collect(), e.deny.iter().map(|c| c.id).collect()))
    }

    /// A new base audience; exceptions stay (Telegram Desktop keeps them too).
    pub(super) fn set_privacy_audience(&mut self, key: PrivacyKey, audience: Audience) {
        let (allow, deny) = self.exceptions(key);
        self.send(Cmd::SetPrivacy { key, audience, allow, deny });
    }

    fn edit_exceptions(&mut self, key: PrivacyKey, allow_side: bool, add: Option<ChatInfo>, remove: Option<PeerId>) {
        let Some(audience) = self.privacy.rules.get(&key).map(|s| s.audience) else { return };
        let ex = self.privacy.exceptions.entry(key).or_default();
        let (list, other) = if allow_side { (&mut ex.allow, &mut ex.deny) } else { (&mut ex.deny, &mut ex.allow) };
        if let Some(c) = add {
            // One person can't be both always and never allowed.
            other.retain(|x| x.id != c.id);
            if !list.iter().any(|x| x.id == c.id) {
                list.push(c);
            }
        }
        if let Some(p) = remove {
            list.retain(|x| x.id != p);
        }
        let (allow, deny) = self.exceptions(key);
        self.send(Cmd::SetPrivacy { key, audience, allow, deny });
    }

    /// "Always allow" / "Never allow" rows under a rule; the lists unfold on click.
    pub(super) fn privacy_exception_rows(&mut self, ui: &mut Ui, key: PrivacyKey) {
        let Some(audience) = self.privacy.rules.get(&key).map(|s| s.audience) else { return };
        let sides: &[bool] = match audience {
            Audience::Everybody => &[false],
            Audience::Contacts => &[true, false],
            Audience::Nobody => &[true],
        };
        let mut remove = None;
        for &allow in sides {
            let list = self.privacy.exceptions.get(&key).map(|e| if allow { e.allow.clone() } else { e.deny.clone() }).unwrap_or_default();
            let label = if allow { l!("Завжди дозволяти", "Always allow") } else { l!("Ніколи не дозволяти", "Never allow") };
            let open = self.privacy.open == Some((key, allow));
            let w = ui.available_width();
            let (row, resp) = ui.allocate_exact_size(vec2(w, 34.0), Sense::click());
            let p = ui.painter();
            if resp.hovered() {
                p.rect_filled(row, 12.0, glass::veil(16));
                ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
            }
            p.text(pos2(row.left() + 12.0, row.center().y), Align2::LEFT_CENTER, label, FontId::proportional(13.5), ink());
            let count = if list.is_empty() { l!("додати", "add").to_owned() } else { list.len().to_string() };
            p.text(pos2(row.right() - 34.0, row.center().y), Align2::RIGHT_CENTER, count, FontId::proportional(13.0), ink_dim());
            icons::draw(p, Rect::from_center_size(pos2(row.right() - 16.0, row.center().y), vec2(14.0, 14.0)), if open { Icon::ChevronDown } else { Icon::ChevronRight }, ink_dim());
            if resp.clicked() {
                // An empty list goes straight to adding someone.
                if list.is_empty() {
                    self.open_privacy_pick(key, allow);
                } else {
                    self.privacy.open = if open { None } else { Some((key, allow)) };
                }
            }
            if open {
                for c in &list {
                    let (r, _) = ui.allocate_exact_size(vec2(w, 40.0), Sense::hover());
                    let p = ui.painter();
                    glass::avatar(p, pos2(r.left() + 30.0, r.center().y), 14.0, &c.title, c.id.bot_api_dialog_id_unchecked(), None, self.textures.get(&ImgKey::Avatar(c.id)));
                    p.galley(pos2(r.left() + 52.0, r.center().y - 9.0), elided(ui, &c.title, 14.0, ink(), w - 110.0), ink());
                    let x = Rect::from_center_size(pos2(r.right() - 18.0, r.center().y), vec2(32.0, 32.0));
                    if glass::icon_button(ui, x, ("privacy-ex-remove", c.id.bot_api_dialog_id_unchecked(), allow), Icon::Close, false, true).on_hover_text(l!("Прибрати", "Remove")).clicked() {
                        remove = Some((allow, c.id));
                    }
                }
                let (r, _) = ui.allocate_exact_size(vec2(w, 34.0), Sense::hover());
                if glass::button(ui, Rect::from_min_size(r.min + vec2(18.0, 2.0), vec2(160.0, 30.0)), ("privacy-ex-add", key.title(), allow), l!("Додати", "Add"), false, true).clicked() {
                    self.open_privacy_pick(key, allow);
                }
            }
        }
        if let Some((allow, p)) = remove {
            self.edit_exceptions(key, allow, None, Some(p));
        }
    }


    fn open_privacy_pick(&mut self, key: PrivacyKey, allow: bool) {
        if self.contacts.is_empty() {
            self.send(Cmd::LoadContacts);
        }
        self.privacy.pick = Some(PrivacyPick { key, allow, query: String::new() });
    }

    /// The picker over the whole window: contacts and groups, filtered by the search field.
    pub(super) fn privacy_pick_view(&mut self, ui: &mut Ui) {
        let Some(pick) = &mut self.privacy.pick else { return };
        let screen = ui.max_rect();
        ui.painter().rect_filled(screen, 0.0, Color32::from_black_alpha(90));
        ui.interact(screen, ui.scope_id().with("privacy-pick-bg"), Sense::click());
        let rect = Rect::from_center_size(screen.center(), vec2(screen.width().min(420.0) - 20.0, (screen.height() - 60.0).min(560.0)));
        glass::glass(ui.painter(), rect, Glass::panel(26.0).with_shadow());
        let inner = rect.shrink(18.0);
        let title = if pick.allow { l!("Завжди дозволяти", "Always allow") } else { l!("Ніколи не дозволяти", "Never allow") };
        ui.painter().text(inner.left_top(), Align2::LEFT_TOP, title, FontId::proportional(17.0), ink());
        let close = Rect::from_min_size(pos2(inner.right() - 32.0, inner.top() - 4.0), vec2(32.0, 32.0));
        let mut done = glass::icon_button(ui, close, "privacy-pick-close", Icon::Close, false, true).clicked() || ui.input_mut(|i| i.consume_key(egui::Modifiers::NONE, Key::Escape));
        let field = Rect::from_min_size(pos2(inner.left(), inner.top() + 40.0), vec2(inner.width(), 38.0));
        let r = glass::search_field(ui, field, &mut pick.query, l!("Пошук людей і груп", "Search people and groups"));
        if !r.has_focus() && pick.query.is_empty() && !ui.memory(|m| m.focused().is_some()) {
            r.request_focus();
        }
        let (key, allow, q) = (pick.key, pick.allow, pick.query.to_lowercase());
        let taken: Vec<PeerId> = self.privacy.exceptions.get(&key).map(|e| if allow { &e.allow } else { &e.deny }).map_or_else(Vec::new, |l| l.iter().map(|c| c.id).collect());
        // Groups' members can be an exception as a whole, so groups are listed after people.
        let candidates: Vec<ChatInfo> = self
            .contacts
            .iter()
            .chain(self.chats.iter().filter(|c| c.kind == ChatKind::Group))
            .filter(|c| !taken.contains(&c.id) && (q.is_empty() || c.title.to_lowercase().contains(&q)))
            .cloned()
            .collect();
        let body = Rect::from_min_max(pos2(inner.left() - 6.0, field.bottom() + 10.0), pos2(inner.right() + 6.0, inner.bottom()));
        let mut child = ui.new_child(UiBuilder::new().max_rect(body));
        child.set_clip_rect(body);
        let mut chosen = None;
        egui::ScrollArea::vertical().id_salt("privacy-pick").auto_shrink(false).show(&mut child, |ui| {
            if candidates.is_empty() {
                ui.add_space(20.0);
                ui.vertical_centered(|ui| ui.label(egui::RichText::new(l!("Нікого не знайдено", "No one found")).color(ink_dim())));
            }
            for c in &candidates {
                let w = ui.available_width();
                let (r, resp) = ui.allocate_exact_size(vec2(w, 46.0), Sense::click());
                let p = ui.painter();
                if resp.hovered() {
                    p.rect_filled(r, 14.0, glass::veil(16));
                    ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
                }
                glass::avatar(p, pos2(r.left() + 26.0, r.center().y), 17.0, &c.title, c.id.bot_api_dialog_id_unchecked(), kind_icon(c.kind), self.textures.get(&ImgKey::Avatar(c.id)));
                p.galley(pos2(r.left() + 54.0, r.center().y - 9.0), elided(ui, &c.title, 14.5, ink(), w - 70.0), ink());
                if resp.clicked() {
                    chosen = Some(c.clone());
                }
            }
        });
        if let Some(c) = chosen {
            self.edit_exceptions(key, allow, Some(c), None);
            self.privacy.open = Some((key, allow));
            done = true;
        }
        if done {
            self.privacy.pick = None;
        }
    }
}
