//! Sending as a channel, as in Telegram Desktop: in supergroups and channels that offer a choice,
//! a round avatar left of the message field opens a menu of our account and the channels we run.

use super::*;
use crate::backend::SendAsPeer;

/// Rows of the menu.
const ROW_H: f32 = 48.0;

/// What one chat lets us post as.
struct Offer {
    peers: Vec<SendAsPeer>,
    /// The one new messages go as.
    current: PeerId,
}

#[derive(Default)]
pub(super) struct SendAs {
    /// Chats already asked this session.
    asked: HashSet<PeerId>,
    /// Only chats with a real choice: more than our own account.
    offers: HashMap<PeerId, Offer>,
}

impl App {
    pub(super) fn on_send_as(&mut self, peer: PeerId, peers: Vec<SendAsPeer>, current: Option<PeerId>) {
        match current.filter(|_| peers.len() > 1) {
            Some(current) => {
                // Premium-only channels are locked by Premium, which comes with the own profile.
                let premium_only = peers.iter().any(|p| p.premium);
                self.send_as.offers.insert(peer, Offer { peers, current });
                if premium_only && self.profile.id.is_none() {
                    self.send(Cmd::LoadProfile);
                }
            }
            None => {
                self.send_as.offers.remove(&peer);
            }
        }
    }

    /// Who a new message in `peer` goes as; `None` where the chat offers no choice.
    pub(super) fn send_as_for(&self, peer: PeerId) -> Option<PeerId> {
        self.send_as.offers.get(&peer).map(|o| o.current)
    }

    /// The channel a message posted as `send_as` names, for its bubble until the server has it.
    pub(super) fn send_as_channel(&self, peer: PeerId, send_as: Option<PeerId>) -> Option<String> {
        let offer = self.send_as.offers.get(&peer)?;
        offer.peers.iter().find(|p| Some(p.id) == send_as && !p.is_me()).map(|p| p.title.clone())
    }

    /// Room the button takes left of the field. The chat's choice is asked for the first time it opens.
    pub(super) fn send_as_width(&mut self, chat: &ChatInfo) -> f32 {
        if matches!(chat.kind, ChatKind::Group | ChatKind::Channel) && self.send_as.asked.insert(chat.id) {
            self.send(Cmd::LoadSendAs(chat.id));
        }
        if self.send_as.offers.contains_key(&chat.id) { COMPOSER_H + 8.0 } else { 0.0 }
    }

    /// The current choice's avatar; a click opens the menu above it.
    pub(super) fn send_as_button(&mut self, ui: &mut Ui, peer: PeerId, rect: Rect) {
        let Some(offer) = self.send_as.offers.get(&peer) else { return };
        let resp = ui.interact(rect, ui.scope_id().with(("send-as", peer)), Sense::click());
        let hover = ui.ctx().animate_bool(resp.id.with("hover"), resp.hovered());
        let p = ui.painter();
        glass::glass(p, rect, Glass::tinted(rect.height() / 2.0, glass::chrome(), (110.0 + hover * 40.0) as u8).with_shadow().frosted(0.96));
        if let Some(cur) = offer.peers.iter().find(|x| x.id == offer.current) {
            let tex = self.textures.get(&ImgKey::Avatar(cur.id));
            glass::avatar(p, rect.center(), rect.height() / 2.0 - 7.0 + hover, &cur.title, cur.id.bot_api_dialog_id_unchecked(), None, tex);
        }
        let resp = resp.on_hover_cursor(egui::CursorIcon::PointingHand).on_hover_text(l!("Надсилати повідомлення як…", "Send message as…"));
        let premium = self.profile.premium;
        let mut picked = None;
        egui::Popup::menu(&resp).align(egui::RectAlign::TOP_START).gap(8.0).width(280.0).show(|ui| {
            ui.label(egui::RichText::new(l!("Надсилати повідомлення як…", "Send message as…")).size(12.5).color(ink_dim()));
            ui.add_space(4.0);
            for x in &offer.peers {
                if self.send_as_row(ui, x, x.id == offer.current, premium) {
                    picked = Some(x.id);
                    ui.close();
                }
            }
        });
        if let Some(id) = picked {
            self.choose_send_as(peer, id);
        }
    }

    /// One row of the menu; returns `true` when it was chosen.
    fn send_as_row(&self, ui: &mut Ui, x: &SendAsPeer, current: bool, premium: bool) -> bool {
        let locked = x.premium && !premium;
        let w = ui.available_width();
        let (row, resp) = ui.allocate_exact_size(vec2(w, ROW_H), if locked { Sense::hover() } else { Sense::click() });
        let hover = ui.ctx().animate_bool(resp.id, resp.hovered() && !locked);
        let p = ui.painter();
        if hover > 0.0 {
            p.rect_filled(row, 12.0, glass::veil((hover * 16.0) as u8));
        }
        let center = pos2(row.left() + 24.0, row.center().y);
        // The current one is ringed, as in Telegram Desktop.
        if current {
            p.circle_stroke(center, 19.5, egui::Stroke::new(2.0, ACCENT));
        }
        glass::avatar(p, center, if current { 15.5 } else { 17.0 }, &x.title, x.id.bot_api_dialog_id_unchecked(), None, self.textures.get(&ImgKey::Avatar(x.id)));
        let text_x = row.left() + 52.0;
        let text_w = row.right() - text_x - 30.0;
        let dim = if locked { ink_dim() } else { ink() };
        p.galley(pos2(text_x, row.top() + 7.0), elided(ui, &x.title, 14.0, dim, text_w), dim);
        p.galley(pos2(text_x, row.top() + 26.0), elided(ui, &x.about, 12.5, ink_dim(), text_w), ink_dim());
        let mark = Rect::from_center_size(pos2(row.right() - 14.0, row.center().y), vec2(16.0, 16.0));
        match (current, x.premium) {
            (true, _) => icons::draw(p, mark, Icon::Check, ACCENT),
            (false, true) => icons::draw(p, mark, if locked { Icon::Lock } else { Icon::Star }, ink_dim()),
            (false, false) => {}
        }
        if locked {
            resp.on_hover_text(l!("Доступно з Telegram Premium", "Available with Telegram Premium"));
            return false;
        }
        resp.on_hover_cursor(egui::CursorIcon::PointingHand).clicked()
    }

    /// Remembered at once; the server keeps it as the chat's default for our other devices too.
    fn choose_send_as(&mut self, peer: PeerId, id: PeerId) {
        let Some(offer) = self.send_as.offers.get_mut(&peer) else { return };
        if offer.current == id {
            return;
        }
        offer.current = id;
        self.send(Cmd::SaveSendAs { peer, send_as: id });
    }
}
