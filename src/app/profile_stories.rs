//! Stories in a profile, as in Telegram Desktop: the pinned ones as a grid in the info panel with
//! album chips that filter it, our own archive under Settings → My Stories, and for our own stories
//! the views counter and the viewers list in the viewer. Read only.

use super::*;
use super::info::chip;
use super::stories::{StoryDeck, story_time};
use crate::backend::{Story, StoryAlbum, StoryList, StoryViewer};

/// One list of a profile, paged.
#[derive(Default)]
struct Grid {
    stories: Vec<Story>,
    total: i32,
    /// What to ask for the following page.
    next: i32,
    loading: bool,
    done: bool,
}

/// Who saw one of our stories.
struct Viewers {
    peer: PeerId,
    id: i32,
    list: Vec<StoryViewer>,
    views: i32,
    reactions: i32,
    next: String,
    loading: bool,
}

#[derive(Default)]
pub(super) struct ProfileStories {
    grids: HashMap<(PeerId, StoryList), Grid>,
    albums: HashMap<PeerId, Vec<StoryAlbum>>,
    /// The chip picked in each profile: all pinned stories, an album or the archive.
    picked: HashMap<PeerId, StoryList>,
    /// The viewers list over our story in the viewer.
    viewers: Option<Viewers>,
}

impl ProfileStories {
    pub(super) fn viewers_open(&self) -> bool {
        self.viewers.is_some()
    }

    pub(super) fn close_viewers(&mut self) {
        self.viewers = None;
    }
}

enum Pick {
    List(StoryList),
    Open(usize),
    More,
}

impl App {
    /// Asks for the first page of a profile's pinned stories and its albums; the profile shows the
    /// stories tab only once some came.
    pub(super) fn open_profile_stories(&mut self, peer: PeerId) {
        self.profile_stories.picked.entry(peer).or_insert(StoryList::Pinned);
        self.load_story_list(peer, StoryList::Pinned, false);
        self.send(Cmd::LoadStoryAlbums(peer));
    }

    fn load_story_list(&mut self, peer: PeerId, list: StoryList, more: bool) {
        let g = self.profile_stories.grids.entry((peer, list)).or_default();
        if g.loading || (more && g.done) {
            return;
        }
        g.loading = true;
        let offset = if more { g.next } else { 0 };
        self.send(Cmd::LoadProfileStories { peer, list, offset });
    }

    /// How many pinned stories a profile has; `None` while unknown or none.
    pub(super) fn profile_story_count(&self, peer: PeerId) -> Option<i32> {
        self.profile_stories.grids.get(&(peer, StoryList::Pinned)).map(|g| g.total.max(g.stories.len() as i32)).filter(|n| *n > 0)
    }

    pub(super) fn profile_stories_event(&mut self, ev: Ev) {
        match ev {
            Ev::ProfileStories { peer, list, stories, total, next, done, append } => {
                let g = self.profile_stories.grids.entry((peer, list)).or_default();
                if !append {
                    g.stories.clear();
                }
                let known: HashSet<i32> = g.stories.iter().map(|s| s.id).collect();
                g.stories.extend(stories.into_iter().filter(|s| !known.contains(&s.id)));
                g.total = total;
                g.next = next;
                g.done = done;
                g.loading = false;
            }
            Ev::StoryAlbums { peer, albums } => {
                self.profile_stories.albums.insert(peer, albums);
            }
            Ev::StoryViewers { peer, id, viewers, views, reactions, next, append } => {
                let Some(v) = self.profile_stories.viewers.as_mut().filter(|v| v.peer == peer && v.id == id) else { return };
                if !append {
                    v.list.clear();
                }
                v.list.extend(viewers);
                (v.views, v.reactions, v.next, v.loading) = (views, reactions, next, false);
            }
            _ => {}
        }
    }

    /// The chips and the grid of a profile's stories; `own` adds the archive.
    pub(super) fn story_grid(&mut self, ui: &mut Ui, peer: PeerId, title: &str, kind: ChatKind, own: bool) {
        let picked = self.profile_stories.picked.get(&peer).copied().unwrap_or(StoryList::Pinned);
        let mut pick = None;
        let albums = self.profile_stories.albums.get(&peer).map(Vec::as_slice).unwrap_or_default();
        if own || !albums.is_empty() {
            ui.horizontal_wrapped(|ui| {
                ui.spacing_mut().item_spacing = vec2(6.0, 6.0);
                ui.add_space(6.0);
                if chip(ui, l!("Усі історії", "All stories"), picked == StoryList::Pinned).clicked() {
                    pick = Some(Pick::List(StoryList::Pinned));
                }
                for a in albums {
                    if chip(ui, &a.title, picked == StoryList::Album(a.id)).clicked() {
                        pick = Some(Pick::List(StoryList::Album(a.id)));
                    }
                }
                if own && chip(ui, l!("Архів", "Archive"), picked == StoryList::Archive).clicked() {
                    pick = Some(Pick::List(StoryList::Archive));
                }
            });
            ui.add_space(6.0);
        }
        match self.profile_stories.grids.get(&(peer, picked)) {
            None => {}
            Some(g) if g.stories.is_empty() && g.loading => {
                ui.add_space(12.0);
                ui.vertical_centered(|ui| ui.add(egui::Spinner::new().color(ink())));
            }
            Some(g) if g.stories.is_empty() => {
                let text = match picked {
                    StoryList::Archive => l!("Тут з'являться ваші історії, що вже зникли", "Your expired stories will appear here"),
                    _ => l!("Тут поки порожньо", "Nothing here yet"),
                };
                ui.add_space(16.0);
                ui.vertical_centered(|ui| ui.label(egui::RichText::new(text).color(ink_dim()).size(13.5)));
                ui.add_space(8.0);
            }
            Some(g) => {
                if let Some(k) = self.story_tiles(ui, peer, &g.stories) {
                    pick = Some(Pick::Open(k));
                }
                if !g.done {
                    let (r, _) = ui.allocate_exact_size(vec2(ui.available_width(), 44.0), Sense::hover());
                    let b = Rect::from_center_size(r.center(), vec2(180.0, 34.0));
                    let label = if g.loading { l!("Завантаження…", "Loading…") } else { l!("Показати ще", "Show more") };
                    if glass::button(ui, b, ("stories-more", picked), label, false, !g.loading).clicked() {
                        pick = Some(Pick::More);
                    }
                }
            }
        }
        match pick {
            Some(Pick::List(list)) => {
                self.profile_stories.picked.insert(peer, list);
                if !self.profile_stories.grids.contains_key(&(peer, list)) {
                    self.load_story_list(peer, list, false);
                }
            }
            Some(Pick::More) => self.load_story_list(peer, picked, true),
            Some(Pick::Open(index)) => {
                let stories = self.profile_stories.grids.get(&(peer, picked)).map(|g| g.stories.clone()).unwrap_or_default();
                self.open_story_list(peer, StoryDeck { title: title.to_owned(), kind, stories }, index);
            }
            None => {}
        }
    }

    /// Upright tiles, three per row; the index of a clicked one.
    fn story_tiles(&self, ui: &mut Ui, peer: PeerId, stories: &[Story]) -> Option<usize> {
        const COLS: usize = 3;
        const GAP: f32 = 4.0;
        let w = ui.available_width();
        let side = ((w - GAP * (COLS as f32 - 1.0)) / COLS as f32).floor();
        let h = (side * 16.0 / 11.0).round();
        let mut open = None;
        for (row_k, row) in stories.chunks(COLS).enumerate() {
            let (r, _) = ui.allocate_exact_size(vec2(w, h + GAP), Sense::hover());
            for (i, s) in row.iter().enumerate() {
                let cell = Rect::from_min_size(pos2(r.left() + i as f32 * (side + GAP), r.top()), vec2(side, h));
                let resp = ui.interact(cell, ui.scope_id().with(("story-tile", s.id)), Sense::click());
                let p = ui.painter();
                match self.textures.get(&ImgKey::Story(peer, s.id)) {
                    Some(t) => glass::image_rounded(p, cell, 10.0, t.id(), t.size_vec2()),
                    None => {
                        p.rect_filled(cell, 10.0, glass::veil(22));
                        icons::draw(p, Rect::from_center_size(cell.center(), vec2(24.0, 24.0)), if s.video { Icon::Play } else { Icon::Camera }, ink_dim());
                    }
                }
                // Dark chips so the labels read over any picture.
                let label_chip = |at: egui::Pos2, icon: Option<Icon>, text: String| {
                    let g = p.layout_no_wrap(text, FontId::proportional(11.0), glass::ON_DARK);
                    let pad = if icon.is_some() { 16.0 } else { 0.0 };
                    let chip = Rect::from_min_size(at, g.size() + vec2(10.0 + pad, 4.0));
                    p.rect_filled(chip, chip.height() / 2.0, Color32::from_black_alpha(130));
                    if let Some(icon) = icon {
                        icons::draw(p, Rect::from_center_size(pos2(chip.left() + 11.0, chip.center().y), vec2(12.0, 12.0)), icon, glass::ON_DARK);
                    }
                    p.galley(chip.min + vec2(5.0 + pad, 2.0), g, glass::ON_DARK);
                };
                if s.video {
                    label_chip(cell.min + vec2(6.0, 6.0), None, player::clock(s.duration));
                }
                if let Some(n) = s.views {
                    label_chip(pos2(cell.left() + 6.0, cell.bottom() - 23.0), Some(Icon::Eye), n.to_string());
                }
                if resp.hovered() {
                    p.rect_filled(cell, 10.0, glass::veil(24));
                    ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
                }
                if resp.clicked() {
                    open = Some(row_k * COLS + i);
                }
            }
        }
        open
    }

    /// The bottom of our own story: the views counter, and above it the list of who saw it.
    /// `true` while the list is open (it holds the story).
    pub(super) fn story_stats_bar(&mut self, ui: &mut Ui, peer: PeerId, story: &Story, bar: Rect, card: Rect) -> bool {
        let views = story.views.unwrap_or(0);
        let mut label = lp!(views, ["{n} перегляд", "{n} перегляди", "{n} переглядів"], ["{n} view", "{n} views"]);
        if story.reactions > 0 {
            label = format!("{label}  ❤ {}", story.reactions);
        }
        let painter = ui.painter().clone();
        let g = painter.layout_no_wrap(label, FontId::proportional(13.5), glass::ON_DARK);
        let pill = Rect::from_min_size(pos2(bar.left(), bar.center().y - 18.0), vec2(g.size().x + 44.0, 36.0));
        let resp = ui.interact(pill, ui.scope_id().with(("story-views", story.id)), Sense::click());
        let open = self.profile_stories.viewers.as_ref().is_some_and(|v| v.peer == peer && v.id == story.id);
        painter.rect_filled(pill, 18.0, Color32::from_black_alpha(if resp.hovered() || open { 170 } else { 130 }));
        icons::draw(&painter, Rect::from_center_size(pos2(pill.left() + 20.0, pill.center().y), vec2(16.0, 16.0)), Icon::Eye, glass::ON_DARK);
        painter.galley(pos2(pill.left() + 34.0, pill.center().y - g.size().y / 2.0), g, glass::ON_DARK);
        if resp.hovered() {
            ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
        }
        let resp = resp.on_hover_text(l!("Хто переглянув", "Who viewed"));
        if resp.clicked() {
            if open {
                self.profile_stories.viewers = None;
            } else {
                self.profile_stories.viewers = Some(Viewers { peer, id: story.id, list: Vec::new(), views, reactions: story.reactions, next: String::new(), loading: true });
                self.send(Cmd::LoadStoryViewers { peer, id: story.id, offset: String::new() });
            }
            return !open;
        }
        if !open {
            return false;
        }
        if ui.input(|i| i.key_pressed(Key::Escape)) {
            self.profile_stories.viewers = None;
            return false;
        }
        let panel = Rect::from_min_max(pos2(card.left() + 10.0, card.top() + card.height() * 0.38), pos2(card.right() - 10.0, bar.top() - 8.0));
        self.story_viewers_panel(ui, panel);
        true
    }

    fn story_viewers_panel(&mut self, ui: &mut Ui, panel: Rect) {
        // Swallow clicks so they don't step the story underneath.
        ui.interact(panel, ui.scope_id().with("story-viewers-bg"), Sense::click());
        let painter = ui.painter().clone();
        painter.rect_filled(panel, 18.0, Color32::from_black_alpha(205));
        let Some(v) = &self.profile_stories.viewers else { return };
        let head = lp!(v.views, ["{n} перегляд", "{n} перегляди", "{n} переглядів"], ["{n} view", "{n} views"]);
        let head = if v.reactions > 0 { format!("{head} · {}", lp!(v.reactions, ["{n} реакція", "{n} реакції", "{n} реакцій"], ["{n} reaction", "{n} reactions"])) } else { head };
        painter.text(pos2(panel.left() + 16.0, panel.top() + 22.0), Align2::LEFT_CENTER, head, FontId::proportional(14.5), glass::ON_DARK);
        let body = Rect::from_min_max(pos2(panel.left() + 8.0, panel.top() + 42.0), pos2(panel.right() - 8.0, panel.bottom() - 8.0));
        let mut child = ui.new_child(UiBuilder::new().max_rect(body));
        child.set_clip_rect(body);
        let mut more = false;
        egui::ScrollArea::vertical().id_salt(("story-viewers", v.id)).auto_shrink(false).show(&mut child, |ui| {
            ui.spacing_mut().item_spacing.y = 2.0;
            if v.list.is_empty() {
                ui.add_space(12.0);
                if v.loading {
                    ui.vertical_centered(|ui| ui.add(egui::Spinner::new().color(glass::ON_DARK)));
                } else {
                    let text = l!("Ще ніхто не переглянув", "No views yet");
                    ui.vertical_centered(|ui| ui.label(egui::RichText::new(text).color(glass::rgba(glass::ON_DARK, 170)).size(13.5)));
                }
                return;
            }
            for x in &v.list {
                let (r, _) = ui.allocate_exact_size(vec2(ui.available_width(), 46.0), Sense::hover());
                let p = ui.painter();
                glass::avatar(p, pos2(r.left() + 22.0, r.center().y), 17.0, &x.name, x.peer.bot_api_dialog_id_unchecked(), None, self.textures.get(&ImgKey::Avatar(x.peer)));
                let right = if x.reaction.is_some() { 40.0 } else { 8.0 };
                let name = elided(ui, &x.name, 14.0, glass::ON_DARK, r.width() - 56.0 - right);
                p.galley(pos2(r.left() + 48.0, r.top() + 5.0), name, glass::ON_DARK);
                if x.date > 0 {
                    p.text(pos2(r.left() + 48.0, r.bottom() - 11.0), Align2::LEFT_CENTER, story_time(x.date), FontId::proportional(12.0), glass::rgba(glass::ON_DARK, 160));
                }
                if let Some(e) = &x.reaction {
                    p.text(pos2(r.right() - 12.0, r.center().y), Align2::RIGHT_CENTER, e, FontId::proportional(20.0), glass::ON_DARK);
                }
            }
            if !v.next.is_empty() {
                let (r, _) = ui.allocate_exact_size(vec2(ui.available_width(), 44.0), Sense::hover());
                let b = Rect::from_center_size(r.center(), vec2(160.0, 32.0));
                let label = if v.loading { l!("Завантаження…", "Loading…") } else { l!("Показати ще", "Show more") };
                more = glass::button(ui, b, "story-viewers-more", label, false, !v.loading).clicked();
            }
        });
        if more && let Some(v) = &mut self.profile_stories.viewers {
            v.loading = true;
            let (peer, id, offset) = (v.peer, v.id, v.next.clone());
            self.send(Cmd::LoadStoryViewers { peer, id, offset });
        }
    }

    /// Settings → My Stories: our pinned stories, albums and the archive.
    pub(super) fn my_stories_page(&mut self, ui: &mut Ui) {
        let Some(me) = self.profile.id else {
            super::settings_pages::hint(ui, l!("Профіль ще завантажується", "The profile is still loading"));
            return;
        };
        if !self.profile_stories.grids.contains_key(&(me, StoryList::Pinned)) {
            self.open_profile_stories(me);
        }
        let name = format!("{} {}", self.profile.first, self.profile.last).trim().to_owned();
        egui::ScrollArea::vertical().id_salt("settings-stories").auto_shrink(false).show(ui, |ui| {
            super::settings_pages::hint(ui, l!(
                "Історії, збережені в профілі, лишаються після доби. В архіві — усі ваші історії, що вже зникли; бачите їх лише ви.",
                "Stories saved to the profile stay after the day is over. The archive keeps all your expired stories; only you see it."
            ));
            ui.add_space(8.0);
            self.story_grid(ui, me, &name, ChatKind::User, true);
        });
    }
}
