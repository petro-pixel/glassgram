//! Forum groups: the topic list instead of history, opening topics, the new / rename topic box.

use super::*;

/// Forum state of the open chat.
#[derive(Default)]
pub(super) struct Forum {
    /// Topics of each forum chat.
    pub(super) topics: HashMap<PeerId, Vec<Topic>>,
    /// The open topic; `None` shows the topic list.
    pub(super) open: Option<i32>,
    /// Topics whose history is fully loaded.
    pub(super) exhausted: HashSet<(PeerId, i32)>,
    /// New topic or rename box: (chat, topic or `None` for a new one, name, colour).
    pub(super) edit: Option<(PeerId, Option<i32>, String, u32)>,
}

impl App {
    pub(super) fn open_topic(&mut self, peer: PeerId, topic: i32) {
        self.forum.open = Some(topic);
        self.compose = Compose::New;
        self.picked.clear();
        self.send(Cmd::OpenTopic { peer, topic, before: None });
    }

    /// Forum topic list instead of the history.
    pub(super) fn forum_view(&mut self, ui: &mut Ui, peer: PeerId, chat: &ChatInfo, rect: Rect) {
        let header = Rect::from_min_size(rect.min, vec2(rect.width(), HEADER_H));
        let list = Rect::from_min_max(pos2(rect.left(), header.bottom() + 10.0), rect.max);
        glass::glass(ui.painter(), list, Glass::panel(24.0));
        let mut chosen = None;
        let mut edit = None;
        let mut child = ui.new_child(UiBuilder::new().max_rect(list.shrink2(vec2(8.0, 10.0))));
        child.set_clip_rect(list.shrink(4.0));
        egui::ScrollArea::vertical().id_salt(("topics", peer)).auto_shrink(false).show(&mut child, |ui| {
            let Some(topics) = self.forum.topics.get(&peer) else {
                ui.add_space(30.0);
                ui.vertical_centered(|ui| ui.add(egui::Spinner::new().color(ink())));
                return;
            };
            ui.spacing_mut().item_spacing.y = 2.0;
            for t in topics {
                let resp = topic_row(ui, t);
                if resp.clicked() {
                    chosen = Some(t.id);
                }
                resp.context_menu(|ui| {
                    if glass::menu_button(ui, Icon::Pencil, l!("Перейменувати", "Rename")).clicked() {
                        edit = Some(Cmd::EditTopic { peer, id: t.id, title: None, closed: None });
                    }
                    if t.id != GENERAL_TOPIC {
                        let (icon, label) = if t.closed { (Icon::LockOpen, l!("Відкрити тему", "Reopen topic")) } else { (Icon::Lock, l!("Закрити тему", "Close topic")) };
                        if glass::menu_button(ui, icon, label).clicked() {
                            edit = Some(Cmd::EditTopic { peer, id: t.id, title: None, closed: Some(!t.closed) });
                        }
                    }
                });
            }
        });
        glass::glass(ui.painter(), header, Glass::tinted(HEADER_H / 2.0, glass::chrome(), 110).with_shadow().frosted(0.96));
        let p = ui.painter();
        let tex = self.textures.get(&ImgKey::Avatar(peer));
        glass::avatar(p, pos2(header.left() + 34.0, header.center().y), 21.0, &chat.title, peer.bot_api_dialog_id_unchecked(), kind_icon(chat.kind), tex);
        let text_x = header.left() + 66.0;
        p.galley(pos2(text_x, header.center().y - 20.0), elided(ui, &chat.title, 16.5, ink(), header.right() - text_x - 20.0), ink());
        let n = self.forum.topics.get(&peer).map_or(0, Vec::len);
        p.text(pos2(text_x, header.center().y + 11.0), Align2::LEFT_CENTER, lf!("форум · тем: {n}", "forum · topics: {n}"), FontId::proportional(12.5), ink_dim());
        let add = Rect::from_center_size(pos2(header.right() - 32.0, header.center().y), vec2(38.0, 38.0));
        if glass::icon_button(ui, add, "topic-new", Icon::Plus, false, true).on_hover_text(l!("Нова тема", "New topic")).clicked() {
            self.forum.edit = Some((peer, None, String::new(), TOPIC_COLORS[0]));
        }
        if let Some(t) = chosen {
            self.open_topic(peer, t);
        }
        match edit {
            // Renaming opens the window with the current name.
            Some(Cmd::EditTopic { id, title: None, closed: None, .. }) => {
                let t = self.forum.topics.get(&peer).and_then(|ts| ts.iter().find(|t| t.id == id));
                self.forum.edit = t.map(|t| (peer, Some(id), t.title.clone(), t.color));
            }
            Some(cmd) => self.send(cmd),
            None => {}
        }
        if self.forum.edit.is_some() {
            self.topic_editor(ui);
        }
    }

    /// New topic or rename window: name and icon color.
    pub(super) fn topic_editor(&mut self, ui: &mut Ui) {
        let Some((peer, id, title, color)) = &mut self.forum.edit else { return };
        let screen = ui.max_rect();
        ui.painter().rect_filled(screen, 0.0, Color32::from_black_alpha(90));
        ui.interact(screen, ui.scope_id().with("topic-bg"), Sense::click());
        let rect = Rect::from_center_size(screen.center(), vec2(360.0, if id.is_none() { 210.0 } else { 160.0 }));
        glass::glass(ui.painter(), rect, Glass::panel(24.0).with_shadow());
        let inner = rect.shrink(18.0);
        let head = if id.is_none() { l!("Нова тема", "New topic") } else { l!("Перейменувати тему", "Rename topic") };
        ui.painter().text(inner.left_top(), Align2::LEFT_TOP, head, FontId::proportional(17.0), ink());
        let field = Rect::from_min_size(inner.left_top() + vec2(0.0, 34.0), vec2(inner.width(), 38.0));
        let resp = glass::text_field(ui, field, title, l!("Назва теми", "Topic name"), false);
        if title.is_empty() && !resp.has_focus() {
            resp.request_focus();
        }
        let mut y = field.bottom() + 12.0;
        if id.is_none() {
            // The icon color can only be set on creation.
            for (k, c) in TOPIC_COLORS.into_iter().enumerate() {
                let center = pos2(inner.left() + 16.0 + k as f32 * 40.0, y + 14.0);
                let r = Rect::from_center_size(center, vec2(30.0, 30.0));
                let resp = ui.interact(r, ui.scope_id().with(("topic-color", k)), Sense::click());
                let col = Color32::from_rgb((c >> 16) as u8, (c >> 8) as u8, c as u8);
                ui.painter().circle_filled(center, 13.0, col);
                if *color == c {
                    ui.painter().circle_stroke(center, 16.0, egui::Stroke::new(2.0, ink()));
                }
                if resp.clicked() {
                    *color = c;
                }
            }
            y += 40.0;
        }
        let ok = !title.trim().is_empty();
        let half = (inner.width() - 8.0) / 2.0;
        let cancel = glass::button(ui, Rect::from_min_size(pos2(inner.left(), y), vec2(half, 34.0)), "topic-cancel", l!("Скасувати", "Cancel"), false, true).clicked();
        let save = glass::button(ui, Rect::from_min_size(pos2(inner.right() - half, y), vec2(half, 34.0)), "topic-save", l!("Зберегти", "Save"), true, ok).clicked();
        let enter = resp.lost_focus() && ui.input(|i| i.key_pressed(Key::Enter));
        if (save || enter) && ok {
            let (peer, title, color) = (*peer, title.trim().to_owned(), *color);
            let cmd = match id {
                Some(id) => Cmd::EditTopic { peer, id: *id, title: Some(title), closed: None },
                None => Cmd::CreateTopic { peer, title, color },
            };
            self.forum.edit = None;
            self.send(cmd);
        } else if cancel || ui.input(|i| i.key_pressed(Key::Escape)) {
            self.forum.edit = None;
        }
    }
}

impl App {
    /// Topic lists and pages of a topic's history.
    pub(super) fn forum_event(&mut self, ev: Ev) {
        match ev {
            Ev::Topics { peer, topics } => {
                self.forum.topics.insert(peer, topics);
            }
            Ev::TopicHistory { peer, topic, msgs, older } => {
                if older {
                    self.loading_older = false;
                }
                // Incomplete page: there are no earlier messages in the topic.
                if msgs.len() < backend::PAGE {
                    self.forum.exhausted.insert((peer, topic));
                }
                let last = msgs.last().map(|m| m.id);
                // Topics share the chat history: merge without duplicates and keep order by id.
                let list = self.history.entry(peer).or_default();
                for m in msgs {
                    if !list.iter().any(|x| x.id == m.id) {
                        list.push(m);
                    }
                }
                list.sort_by_key(|m| m.id);
                if !older
                    && let Some(max_id) = last
                    && let Some(t) = self.forum.topics.get_mut(&peer).and_then(|ts| ts.iter_mut().find(|t| t.id == topic))
                    && t.unread > 0
                {
                    t.unread = 0;
                    self.send(Cmd::ReadTopic { peer, topic, max_id });
                }
            }
            _ => {}
        }
    }
}
