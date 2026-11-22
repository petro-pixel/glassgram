//! Sharing a location and a contact from the 📎 menu (`inputMediaGeoPoint`, `inputMediaContact`).

use super::*;

impl Worker {
    pub(super) async fn share_cmd(&mut self, cmd: Cmd) -> Result<(), String> {
        let (peer, media, reply_to, send_as): (_, tl::enums::InputMedia, _, _) = match cmd {
            Cmd::SendLocation { peer, lat, lon, reply_to, send_as } => {
                let geo_point = tl::types::InputGeoPoint { lat, long: lon, accuracy_radius: None }.into();
                (peer, tl::types::InputMediaGeoPoint { geo_point }.into(), reply_to, send_as)
            }
            Cmd::SendContact { peer, user, reply_to, send_as } => (peer, self.contact_media(user).await?, reply_to, send_as),
            _ => return Ok(()),
        };
        let client = self.client()?;
        let r = self.peer_ref(peer)?;
        let send_as = self.send_as_ref(send_as)?;
        let not_sent = |e: &dyn std::fmt::Display| lf!("Не надіслано: {e}", "Not sent: {e}");
        let m = match self.raw_reply(&reply_to, &send_as)? {
            Some(x) => reply::send_one(&client, r, "", Some(media), x, SendOpts::default(), send_as).await.map_err(|e| not_sent(&e))?,
            None => Some(client.send_message(r, InputMessage::new().media(media).reply_to(reply_to.as_ref().map(|x| x.id))).await.map_err(|e| not_sent(&e))?),
        };
        if let Some(m) = m {
            self.sent().emit(peer, &m, ReplyTo::local(&reply_to), SendOpts::default());
        }
        Ok(())
    }

    /// A contact card needs the number, which the server only gives for people who show it to us.
    async fn contact_media(&self, user: PeerId) -> Result<tl::enums::InputMedia, String> {
        let client = self.client()?;
        let r = self.peer_ref(user)?;
        let users = client.invoke(&tl::functions::users::GetUsers { id: vec![r.into()] }).await.map_err(|e| lf!("Не надіслано: {e}", "Not sent: {e}"))?;
        let Some(tl::enums::User::User(u)) = users.into_iter().next() else {
            return Err(l!("Контакт не знайдено", "Contact not found").to_owned());
        };
        let Some(phone_number) = u.phone.filter(|p| !p.is_empty()) else {
            return Err(l!("Ця людина приховує свій номер", "This person keeps their number hidden").to_owned());
        };
        Ok(tl::types::InputMediaContact { phone_number, first_name: u.first_name.unwrap_or_default(), last_name: u.last_name.unwrap_or_default(), vcard: String::new() }.into())
    }
}
