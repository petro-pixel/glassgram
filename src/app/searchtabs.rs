//! Tabs of the chat list search, as in Telegram Desktop 7.3: Chats, Channels, Apps, Posts, Media.
//! The Chats tab is the plain chat list search in `chatlist.rs`.

use super::*;
use crate::backend::PostsFlood;

#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub(super) enum Tab {
    #[default]
    Chats,
    Channels,
    Apps,
    Posts,
    Media,
}

impl Tab {
    const ALL: [Self; 5] = [Self::Chats, Self::Channels, Self::Apps, Self::Posts, Self::Media];

    fn title(self) -> &'static str {
        match self {
            Self::Chats => l!("Чати", "Chats"),
            Self::Channels => l!("Канали", "Channels"),
            Self::Apps => l!("Застосунки", "Apps"),
            Self::Posts => l!("Публікації", "Posts"),
            Self::Media => l!("Медіа", "Media"),
        }
    }
}

/// Each tab keeps the query its results are for, so a stale answer is dropped.
#[derive(Default)]
pub(super) struct SearchTabs {
    pub(super) tab: Tab,
    /// The last (tab, query) asked for after a pause in typing.
    requested: Option<(Tab, String)>,
    channels: Option<(String, Vec<ChatInfo>)>,
    apps: Option<(String, Vec<ChatInfo>)>,
    /// Free post searches left for the query typed (`checkSearchPostsFlood`).
    flood: Option<(String, PostsFlood)>,
    /// A post search runs for this query (searches are counted, so only on demand).
    posts_asked: Option<String>,
    /// Found posts and the server's refusal, if any.
    posts: Option<(String, Vec<SearchHit>, Option<String>)>,
    media: Option<(String, Vec<SearchHit>)>,
}

/// What a tab click asks the chat list to do after drawing.
enum Pick {
    Chat(ChatInfo),
    Hit(SearchHit),
    SearchPosts,
}

impl App {
    /// Tabs are shown while there is a query, or a tab other than Chats is open.
    pub(super) fn search_tabs_shown(&self) -> bool {
        !self.search.query.trim().is_empty() || self.search_tabs.tab != Tab::Chats
    }

    /// Esc: clears the query and goes back to Chats; false when there was nothing to clear.
    pub(super) fn clear_search(&mut self) -> bool {
        if self.search.query.is_empty() && self.search_tabs.tab == Tab::Chats {
            return false;
        }
        self.search = Search::default();
        self.search_tabs.tab = Tab::Chats;
        true
    }

    /// The tab capsules under the search field.
    pub(super) fn search_tab_bar(&mut self, ui: &mut Ui, rect: Rect) {
        let mut picked = None;
        let mut child = ui.new_child(UiBuilder::new().max_rect(rect));
        child.set_clip_rect(rect);
        egui::ScrollArea::horizontal().id_salt("search-tabs").scroll_bar_visibility(egui::scroll_area::ScrollBarVisibility::AlwaysHidden).show(&mut child, |ui| {
            ui.horizontal(|ui| {
                ui.spacing_mut().item_spacing.x = 6.0;
                for t in Tab::ALL {
                    let resp = info::chip(ui, t.title(), self.search_tabs.tab == t);
                    if resp.clicked() {
                        picked = Some(t);
                        resp.scroll_to_me(None);
                    }
                }
            });
        });
        if let Some(t) = picked {
            self.search_tabs.tab = t;
        }
    }

    /// Asks the open tab for results after a pause in typing; the Chats tab searches in `sidebar`.
    pub(super) fn search_tabs_due(&mut self, now: f64) {
        let tab = self.search_tabs.tab;
        let q = self.search.query.trim().to_owned();
        if tab == Tab::Chats || now - self.search.edited_at <= SEARCH_DELAY {
            return;
        }
        let key = (tab, q.clone());
        if self.search_tabs.requested.as_ref() == Some(&key) {
            return;
        }
        self.search_tabs.requested = Some(key);
        match tab {
            // Without a query Apps shows the popular ones.
            Tab::Apps => self.send(Cmd::SearchApps(q)),
            _ if q.is_empty() => {}
            Tab::Channels => self.send(Cmd::SearchChannels(q)),
            Tab::Posts => self.send(Cmd::CheckPostsFlood(q)),
            Tab::Media => self.send(Cmd::Search { peer: None, query: q, kind: Some(SharedKind::Media), from: None }),
            Tab::Chats => {}
        }
    }

    /// Enter in the search field on the Posts tab starts the search.
    pub(super) fn search_tabs_enter(&mut self) {
        if self.search_tabs.tab == Tab::Posts {
            self.search_posts();
        }
    }

    fn search_posts(&mut self) {
        let q = self.search.query.trim().to_owned();
        let blocked = self.search_tabs.flood.as_ref().is_some_and(|(fq, f)| *fq == q && f.blocked());
        if q.is_empty() || blocked || self.search_tabs.posts_asked.as_ref() == Some(&q) {
            return;
        }
        self.search_tabs.posts_asked = Some(q.clone());
        self.send(Cmd::SearchPosts(q));
    }

    pub(super) fn search_tabs_event(&mut self, ev: Ev) {
        let s = &mut self.search_tabs;
        match ev {
            Ev::ChannelResults { query, chats } => s.channels = Some((query, chats)),
            Ev::AppResults { query, bots } => s.apps = Some((query, bots)),
            Ev::PostsFlood { query, flood } => s.flood = Some((query, flood)),
            Ev::PostResults { query, hits, flood, error } => {
                if let Some(f) = flood {
                    s.flood = Some((query.clone(), f));
                }
                if s.posts_asked.as_ref() == Some(&query) {
                    s.posts_asked = None;
                }
                s.posts = Some((query, hits, error));
            }
            Ev::SearchResults { query, hits, .. } => s.media = Some((query, hits)),
            _ => {}
        }
    }

    /// The open tab's results in place of the chat list.
    pub(super) fn search_tab_view(&mut self, ui: &mut Ui, rect: Rect) {
        let q = self.search.query.trim().to_owned();
        let mut pick = None;
        let mut child = ui.new_child(UiBuilder::new().max_rect(rect));
        child.set_clip_rect(rect);
        egui::ScrollArea::vertical().id_salt(("search-tab", self.search_tabs.tab as u8)).auto_shrink(false).show(&mut child, |ui| {
            ui.spacing_mut().item_spacing.y = 4.0;
            pick = match self.search_tabs.tab {
                Tab::Chats => None,
                Tab::Channels => self.channels_tab(ui, &q),
                Tab::Apps => self.apps_tab(ui, &q),
                Tab::Posts => self.posts_tab(ui, &q),
                Tab::Media => self.media_tab(ui, &q),
            };
        });
        match pick {
            Some(Pick::Chat(chat)) => {
                let id = chat.id;
                if !self.chats.iter().any(|c| c.id == id) {
                    self.chats.push(chat);
                }
                self.open_chat(id);
            }
            Some(Pick::Hit(hit)) => self.jump_to(&hit),
            Some(Pick::SearchPosts) => self.search_posts(),
            None => {}
        }
    }

    fn chat_list_rows(&self, ui: &mut Ui, chats: &[&ChatInfo]) -> Option<Pick> {
        let mut pick = None;
        for chat in chats {
            let tex = self.textures.get(&ImgKey::Avatar(chat.id));
            if chat_row(ui, chat, false, tex, None, None, &[], self.settings.density).clicked() {
                pick = Some(Pick::Chat((*chat).clone()));
            }
        }
        pick
    }

    /// Our channels by title, then public ones from the server.
    fn channels_tab(&self, ui: &mut Ui, q: &str) -> Option<Pick> {
        let lower = q.to_lowercase();
        let mine: Vec<&ChatInfo> = self.chats.iter().filter(|c| c.kind == ChatKind::Channel && c.title.to_lowercase().contains(&lower)).collect();
        let mut pick = None;
        if !mine.is_empty() {
            section_title(ui, l!("Ваші канали", "Your channels"));
            pick = self.chat_list_rows(ui, &mine);
        }
        if q.is_empty() {
            if mine.is_empty() {
                hint(ui, l!("Введіть назву каналу", "Type a channel name"));
            }
            return pick;
        }
        match &self.search_tabs.channels {
            Some((fq, found)) if fq == q => {
                let public: Vec<&ChatInfo> = found.iter().filter(|p| !mine.iter().any(|c| c.id == p.id)).collect();
                if !public.is_empty() {
                    section_title(ui, l!("Публічні канали", "Public channels"));
                    pick = self.chat_list_rows(ui, &public).or(pick);
                } else if mine.is_empty() {
                    hint(ui, l!("Каналів не знайдено", "No channels found"));
                }
            }
            _ => spinner(ui),
        }
        pick
    }

    /// Bots with mini apps: popular ones, or those matching the query.
    fn apps_tab(&self, ui: &mut Ui, q: &str) -> Option<Pick> {
        match &self.search_tabs.apps {
            Some((fq, bots)) if fq == q => {
                if bots.is_empty() {
                    hint(ui, l!("Застосунків не знайдено", "No apps found"));
                    return None;
                }
                section_title(ui, if q.is_empty() { l!("Популярні застосунки", "Popular apps") } else { l!("Застосунки", "Apps") });
                self.chat_list_rows(ui, &bots.iter().collect::<Vec<_>>())
            }
            _ => {
                spinner(ui);
                None
            }
        }
    }

    /// Public posts: the free quota first, the search itself only on demand.
    fn posts_tab(&self, ui: &mut Ui, q: &str) -> Option<Pick> {
        if q.is_empty() {
            hint(ui, l!("Шукайте публікації в усіх публічних каналах", "Search posts in all public channels"));
            return None;
        }
        let s = &self.search_tabs;
        let flood = s.flood.as_ref().filter(|(fq, _)| fq == q).map(|f| f.1);
        let asked = s.posts_asked.as_deref() == Some(q);
        let mut pick = None;
        ui.add_space(6.0);
        if let Some(f) = flood {
            hint(ui, &flood_text(&f));
        }
        let done = s.posts.as_ref().is_some_and(|(pq, _, _)| pq == q);
        if !done {
            let w = ui.available_width();
            let (r, _) = ui.allocate_exact_size(vec2(w, 44.0), Sense::hover());
            let b = Rect::from_center_size(r.center(), vec2((w - 24.0).min(240.0), 36.0));
            let enabled = !asked && !flood.is_some_and(|f| f.blocked());
            if glass::button(ui, b, "search-posts", l!("Шукати публікації", "Search posts"), true, enabled).clicked() {
                pick = Some(Pick::SearchPosts);
            }
            if asked {
                spinner(ui);
            }
            return pick;
        }
        let Some((_, hits, error)) = &s.posts else { return None };
        if let Some(e) = error {
            hint(ui, e);
        } else if hits.is_empty() {
            hint(ui, l!("Публікацій не знайдено", "No posts found"));
        } else {
            section_title(ui, l!("Публікації", "Posts"));
            for hit in hits {
                if super::hit_row(ui, hit, q, self.textures.get(&ImgKey::Avatar(hit.msg.peer))).clicked() {
                    pick = Some(Pick::Hit(hit.clone()));
                }
            }
        }
        pick
    }

    /// Photos and videos from all chats, three per row.
    fn media_tab(&self, ui: &mut Ui, q: &str) -> Option<Pick> {
        if q.is_empty() {
            hint(ui, l!("Шукайте фото й відео в усіх чатах", "Search photos and videos in all chats"));
            return None;
        }
        let Some((_, hits)) = self.search_tabs.media.as_ref().filter(|(mq, _)| mq == q) else {
            spinner(ui);
            return None;
        };
        if hits.is_empty() {
            hint(ui, l!("Медіа не знайдено", "No media found"));
            return None;
        }
        const COLS: usize = 3;
        const GAP: f32 = 4.0;
        let mut pick = None;
        ui.add_space(4.0);
        let w = ui.available_width();
        let side = ((w - GAP * (COLS as f32 - 1.0)) / COLS as f32).floor();
        for row in hits.chunks(COLS) {
            let (r, _) = ui.allocate_exact_size(vec2(w, side + GAP), Sense::hover());
            for (i, hit) in row.iter().enumerate() {
                let m = &hit.msg;
                let cell = Rect::from_min_size(pos2(r.left() + i as f32 * (side + GAP), r.top()), vec2(side, side));
                let resp = ui.interact(cell, ui.scope_id().with(("search-media", m.peer, m.id)), Sense::click());
                let p = ui.painter();
                match self.media_tex(m.peer, m) {
                    Some(t) => glass::image_rounded(p, cell, 10.0, t.id(), t.size_vec2()),
                    None => {
                        p.rect_filled(cell, 10.0, glass::veil(22));
                        let icon = if m.photo { Icon::Camera } else { Icon::Play };
                        icons::draw(p, Rect::from_center_size(cell.center(), vec2(24.0, 24.0)), icon, ink_dim());
                    }
                }
                if self.media_hidden(m.peer, m) {
                    super::spoiler::cover(p, cell, 10.0, ui.input(|i| i.time));
                }
                if let Some(info) = m.media.as_ref().filter(|i| i.duration > 0.0) {
                    // Dark chip so the length reads over any picture.
                    let g = p.layout_no_wrap(player::clock(info.duration), FontId::proportional(11.0), glass::ON_DARK);
                    let chip = Rect::from_min_size(cell.min + vec2(6.0, 6.0), g.size() + vec2(10.0, 4.0));
                    p.rect_filled(chip, chip.height() / 2.0, Color32::from_black_alpha(130));
                    p.galley(chip.min + vec2(5.0, 2.0), g, glass::ON_DARK);
                }
                if resp.hovered() {
                    p.rect_filled(cell, 10.0, glass::veil(24));
                    ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
                }
                if resp.on_hover_text(&hit.chat).clicked() {
                    pick = Some(Pick::Hit(hit.clone()));
                }
            }
        }
        pick
    }
}

/// "Free searches left today: 7 of 10", or when the next free one comes.
fn flood_text(f: &PostsFlood) -> String {
    if f.free {
        return l!("Цей пошук безкоштовний", "This search is free").to_owned();
    }
    if !f.blocked() {
        let (n, total) = (f.remains, f.total);
        return lf!("Безкоштовних пошуків на сьогодні: {n} з {total}", "Free searches left today: {n} of {total}");
    }
    let stars = f.stars;
    let when = f.wait_till.and_then(|t| jiff::Timestamp::from_second(t as i64).ok()).map(|t| t.to_zoned(jiff::tz::TimeZone::system()).strftime("%H:%M").to_string());
    match when {
        Some(at) => lf!(
            "Безкоштовні пошуки на сьогодні вичерпано. Наступний — о {at}; платний пошук ({stars} ⭐) поки не підтримується.",
            "You've used all free searches for today. The next one is at {at}; paid search ({stars} ⭐) isn't supported yet."
        ),
        None => lf!(
            "Безкоштовні пошуки на сьогодні вичерпано; платний пошук ({stars} ⭐) поки не підтримується.",
            "You've used all free searches for today; paid search ({stars} ⭐) isn't supported yet."
        ),
    }
}

fn hint(ui: &mut Ui, text: &str) {
    ui.add_space(14.0);
    ui.vertical_centered(|ui| ui.label(egui::RichText::new(text).color(ink_dim()).size(13.5)));
    ui.add_space(6.0);
}

fn spinner(ui: &mut Ui) {
    ui.add_space(12.0);
    ui.vertical_centered(|ui| ui.add(egui::Spinner::new().color(ink_dim())));
}
