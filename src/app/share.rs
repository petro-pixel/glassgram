//! "Location" and "Contact" in the 📎 menu. A desktop has no GPS, so a location is typed or pasted
//! as coordinates; a contact is picked from our contacts.

use super::*;

#[derive(Default)]
pub(super) struct Share {
    /// The chat and the coordinates typed so far.
    location: Option<(PeerId, String)>,
    /// The chat and the search text.
    contact: Option<(PeerId, String)>,
}

/// "50.45, 30.52" or "50.45 30.52" → latitude and longitude, when they are on the globe.
fn coordinates(text: &str) -> Option<(f64, f64)> {
    let mut parts = text.split(|c: char| c == ',' || c == ';' || c.is_whitespace()).filter(|s| !s.is_empty());
    let (lat, lon): (f64, f64) = (parts.next()?.parse().ok()?, parts.next()?.parse().ok()?);
    (parts.next().is_none() && (-90.0..=90.0).contains(&lat) && (-180.0..=180.0).contains(&lon)).then_some((lat, lon))
}

impl App {
    pub(super) fn share_open(&self) -> bool {
        self.share.location.is_some() || self.share.contact.is_some()
    }

    pub(super) fn share_menu_items(&mut self, ui: &mut Ui, peer: PeerId) {
        if glass::menu_button(ui, Icon::Location, l!("Геопозиція", "Location")).clicked() {
            self.share.location = Some((peer, String::new()));
        }
        if glass::menu_button(ui, Icon::User, l!("Контакт", "Contact")).clicked() {
            self.share.contact = Some((peer, String::new()));
            self.send(Cmd::LoadContacts);
        }
    }

    pub(super) fn share_view(&mut self, ui: &mut Ui) {
        if self.share.location.is_some() {
            self.location_view(ui);
        } else if self.share.contact.is_some() {
            self.contact_pick_view(ui);
        }
    }

    fn location_view(&mut self, ui: &mut Ui) {
        let inner = polls::modal(ui, "share-location", 400.0, 200.0);
        let Some((peer, text)) = &mut self.share.location else { return };
        let peer = *peer;
        ui.painter().text(inner.left_top(), Align2::LEFT_TOP, l!("Надіслати геопозицію", "Send location"), FontId::proportional(17.0), ink());
        let field = Rect::from_min_size(pos2(inner.left(), inner.top() + 36.0), vec2(inner.width(), 38.0));
        let resp = glass::text_field(ui, field, text, l!("Широта, довгота: 50.45, 30.52", "Latitude, longitude: 50.45, 30.52"), false);
        if !resp.has_focus() && text.is_empty() {
            resp.request_focus();
        }
        let point = coordinates(text);
        if point.is_none() && !text.trim().is_empty() {
            ui.painter().text(field.left_bottom() + vec2(4.0, 6.0), Align2::LEFT_TOP, l!("Це не координати", "These aren't coordinates"), FontId::proportional(12.5), glass::danger());
        }
        let buttons = Rect::from_min_max(pos2(inner.left(), inner.bottom() - 36.0), inner.max);
        let half = (buttons.width() - 8.0) / 2.0;
        let esc = ui.input_mut(|i| i.consume_key(egui::Modifiers::NONE, Key::Escape));
        let enter = resp.lost_focus() && ui.input(|i| i.key_pressed(Key::Enter));
        let cancel = glass::button(ui, Rect::from_min_size(buttons.min, vec2(half, buttons.height())), "share-loc-cancel", l!("Скасувати", "Cancel"), false, true).clicked();
        let send = glass::button(ui, Rect::from_min_size(pos2(buttons.right() - half, buttons.top()), vec2(half, buttons.height())), "share-loc-ok", l!("Надіслати", "Send"), true, point.is_some()).clicked();
        if let Some((lat, lon)) = point.filter(|_| send || enter) {
            self.share.location = None;
            let reply_to = self.take_reply();
            self.send(Cmd::SendLocation { peer, lat, lon, reply_to, send_as: self.send_as_for(peer) });
        } else if cancel || esc {
            self.share.location = None;
        }
    }

    fn contact_pick_view(&mut self, ui: &mut Ui) {
        let inner = polls::modal(ui, "share-contact", 400.0, 520.0);
        let (contacts, textures) = (&self.contacts, &self.textures);
        let Some((peer, query)) = &mut self.share.contact else { return };
        let peer = *peer;
        ui.painter().text(inner.left_top(), Align2::LEFT_TOP, l!("Надіслати контакт", "Send contact"), FontId::proportional(17.0), ink());
        let field = Rect::from_min_size(pos2(inner.left(), inner.top() + 36.0), vec2(inner.width(), 38.0));
        glass::text_field(ui, field, query, l!("Пошук", "Search"), false);
        let buttons = Rect::from_min_max(pos2(inner.left(), inner.bottom() - 36.0), inner.max);
        let body = Rect::from_min_max(pos2(inner.left(), field.bottom() + 8.0), pos2(inner.right(), buttons.top() - 8.0));
        let needle = query.to_lowercase();
        let mut chosen = None;
        let mut child = ui.new_child(UiBuilder::new().max_rect(body));
        child.set_clip_rect(body);
        egui::ScrollArea::vertical().id_salt("share-contacts").auto_shrink(false).show(&mut child, |ui| {
            for c in contacts.iter().filter(|c| c.title.to_lowercase().contains(&needle)) {
                let hit = SearchHit { chat: c.title.clone(), kind: c.kind, msg: Msg::placeholder(c.id) };
                if hit_row(ui, &hit, "", textures.get(&ImgKey::Avatar(c.id))).clicked() {
                    chosen = Some(c.id);
                }
            }
        });
        let esc = ui.input_mut(|i| i.consume_key(egui::Modifiers::NONE, Key::Escape));
        let cancel = glass::button(ui, buttons, "share-contact-cancel", l!("Скасувати", "Cancel"), false, true).clicked();
        if let Some(user) = chosen {
            self.share.contact = None;
            let reply_to = self.take_reply();
            self.send(Cmd::SendContact { peer, user, reply_to, send_as: self.send_as_for(peer) });
        } else if cancel || esc {
            self.share.contact = None;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::coordinates;

    #[test]
    fn reads_coordinates() {
        assert_eq!(coordinates("50.45, 30.52"), Some((50.45, 30.52)));
        assert_eq!(coordinates(" -33.9 151.2 "), Some((-33.9, 151.2)));
        assert_eq!(coordinates("91, 30"), None);
        assert_eq!(coordinates("50.45"), None);
        assert_eq!(coordinates("50, 30, 10"), None);
        assert_eq!(coordinates("Київ"), None);
    }
}
