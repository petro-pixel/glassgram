//! The "Communities" box from the ✎ menu: the communities we are in, and under each one its
//! description and linked groups, channels and bots, which open like any chat. Read only.

use super::*;
use crate::backend::Community;

#[derive(Default)]
pub(super) struct Communities {
    open: bool,
    /// `None` — waiting for the server.
    list: Option<Vec<Community>>,
    /// The unfolded community.
    unfolded: Option<i64>,
    /// Description and linked chats of the communities asked about.
    chats: HashMap<i64, (String, Vec<ChatInfo>)>,
}

impl App {
    pub(super) fn open_communities(&mut self) {
        self.communities = Communities { open: true, ..Default::default() };
        self.send(Cmd::LoadCommunities);
    }

    pub(super) fn on_communities(&mut self, list: Vec<Community>) {
        self.communities.list = Some(list);
    }

    pub(super) fn on_community_chats(&mut self, id: i64, about: String, chats: Vec<ChatInfo>) {
        self.communities.chats.insert(id, (about, chats));
    }

    pub(super) fn communities_view(&mut self, ui: &mut Ui) {
        if !self.communities.open {
            return;
        }
        let screen = ui.max_rect();
        ui.painter().rect_filled(screen, 0.0, Color32::from_black_alpha(90));
        if ui.interact(screen, ui.scope_id().with("communities-bg"), Sense::click()).clicked() {
            self.communities.open = false;
            return;
        }
        let rect = Rect::from_center_size(screen.center(), vec2(screen.width().min(440.0) - 20.0, (screen.height() - 60.0).min(620.0)));
        glass::glass(ui.painter(), rect, Glass::panel(26.0).with_shadow());
        ui.interact(rect, ui.scope_id().with("communities-panel"), Sense::click());
        let inner = rect.shrink(18.0);
        ui.painter().text(inner.left_top() + vec2(0.0, 12.0), Align2::LEFT_CENTER, l!("Спільноти", "Communities"), FontId::proportional(17.0), ink());
        let close = Rect::from_min_size(pos2(inner.right() - 32.0, inner.top() - 4.0), vec2(32.0, 32.0));
        let esc = ui.input_mut(|i| i.consume_key(egui::Modifiers::NONE, Key::Escape));
        if glass::icon_button(ui, close, "communities-close", Icon::Close, false, true).clicked() || esc {
            self.communities.open = false;
            return;
        }
        let body = Rect::from_min_max(pos2(inner.left() - 6.0, inner.top() + 40.0), pos2(inner.right() + 6.0, inner.bottom()));
        let mut child = ui.new_child(UiBuilder::new().max_rect(body));
        child.set_clip_rect(body);
        let (mut fold, mut open) = (None, None);
        let c = &self.communities;
        let textures = &self.textures;
        egui::ScrollArea::vertical().id_salt("communities").auto_shrink(false).show(&mut child, |ui| {
            ui.spacing_mut().item_spacing.y = 2.0;
            let Some(list) = &c.list else {
                ui.add(egui::Spinner::new().color(ink_dim()));
                return;
            };
            if list.is_empty() {
                ui.add_space(24.0);
                ui.vertical_centered(|ui| {
                    icons::draw(ui.painter(), Rect::from_center_size(ui.cursor().center_top() + vec2(0.0, 18.0), vec2(36.0, 36.0)), Icon::Users, ink_dim());
                    ui.add_space(44.0);
                    let text = l!(
                        "Ви ще не в жодній спільноті. Спільнота об'єднує групи, канали й ботів в одне ціле.",
                        "You aren't in any community yet. A community joins groups, channels and bots into one."
                    );
                    ui.add(egui::Label::new(egui::RichText::new(text).color(ink_dim()).size(13.5)).wrap());
                });
                return;
            }
            for com in list {
                let unfolded = c.unfolded == Some(com.id);
                let (r, resp) = ui.allocate_exact_size(vec2(ui.available_width(), 52.0), Sense::click());
                let p = ui.painter();
                if resp.hovered() || unfolded {
                    p.rect_filled(r, 14.0, glass::veil(if unfolded { 20 } else { 14 }));
                    ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
                }
                glass::avatar(p, pos2(r.left() + 30.0, r.center().y), 19.0, &com.title, com.id, Some(Icon::Users), None);
                p.galley(pos2(r.left() + 58.0, r.center().y - 9.0), elided(ui, &com.title, 14.5, ink(), r.width() - 100.0), ink());
                icons::draw(p, Rect::from_center_size(pos2(r.right() - 20.0, r.center().y), vec2(18.0, 18.0)), if unfolded { Icon::ChevronDown } else { Icon::ChevronRight }, ink_dim());
                if resp.clicked() {
                    fold = Some(com.clone());
                }
                if !unfolded {
                    continue;
                }
                match c.chats.get(&com.id) {
                    None => {
                        ui.add(egui::Spinner::new().color(ink_dim()));
                    }
                    Some((about, chats)) => {
                        if !about.is_empty() {
                            ui.add_space(4.0);
                            ui.horizontal(|ui| {
                                ui.add_space(12.0);
                                ui.add(egui::Label::new(egui::RichText::new(about).color(ink_dim()).size(13.0)).wrap());
                            });
                            ui.add_space(4.0);
                        }
                        for chat in chats {
                            if linked_row(ui, chat, textures.get(&ImgKey::Avatar(chat.id))).clicked() {
                                open = Some(chat.clone());
                            }
                        }
                    }
                }
                ui.add_space(6.0);
            }
        });
        if let Some(com) = fold {
            if self.communities.unfolded == Some(com.id) {
                self.communities.unfolded = None;
            } else {
                self.communities.unfolded = Some(com.id);
                if !self.communities.chats.contains_key(&com.id) {
                    self.send(Cmd::LoadCommunityChats { id: com.id, access_hash: com.access_hash });
                }
            }
        }
        if let Some(chat) = open {
            self.communities.open = false;
            self.open_found_chat(chat);
        }
    }
}

/// A linked chat under its community, indented, with what kind of chat it is.
fn linked_row(ui: &mut Ui, c: &ChatInfo, tex: Option<&TextureHandle>) -> egui::Response {
    let (r, resp) = ui.allocate_exact_size(vec2(ui.available_width(), 44.0), Sense::click());
    let p = ui.painter();
    if resp.hovered() {
        p.rect_filled(r, 12.0, glass::veil(14));
        ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
    }
    glass::avatar(p, pos2(r.left() + 44.0, r.center().y), 15.0, &c.title, c.id.bot_api_dialog_id_unchecked(), kind_icon(c.kind), tex);
    p.galley(pos2(r.left() + 68.0, r.top() + 5.0), elided(ui, &c.title, 14.0, ink(), r.width() - 80.0), ink());
    let kind = match c.kind {
        ChatKind::Channel => l!("канал", "channel"),
        ChatKind::Bot => l!("бот", "bot"),
        ChatKind::Group => l!("група", "group"),
        _ => l!("чат", "chat"),
    };
    p.text(pos2(r.left() + 68.0, r.bottom() - 11.0), Align2::LEFT_CENTER, kind, FontId::proportional(12.5), ink_dim());
    resp
}
