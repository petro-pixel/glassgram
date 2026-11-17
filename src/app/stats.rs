//! "Statistics" of a channel or supergroup, as in Telegram Desktop: the overview numbers with their
//! change, the graphs (drawn by `stats_chart`), recent posts and, in groups, the most active members.
//! A click on a recent post opens its own page: views and reactions graphs and public shares.

use std::collections::HashSet;

use super::*;
use super::stats_chart::{self, compact};
use crate::backend::{ChannelStats, GraphState, MessageStats, PostStat, PublicForward, StatTile, StatsGraph, TopMember};

#[derive(Default)]
pub(super) struct StatsView {
    /// The chat whose statistics are open.
    peer: Option<PeerId>,
    title: String,
    data: Option<ChannelStats>,
    error: Option<String>,
    /// The colours were asked for the dark theme.
    dark: bool,
    /// Series switched off in the legends: (graph, series).
    hidden: HashSet<(usize, usize)>,
    /// Lazy graphs already asked for.
    asked: HashSet<usize>,
    /// The page of one post, over the overview.
    post: Option<PostPage>,
}

/// One post's statistics.
struct PostPage {
    post: PostStat,
    data: Option<MessageStats>,
    error: Option<String>,
    hidden: HashSet<(usize, usize)>,
    asked: HashSet<usize>,
}

impl App {
    /// Opens the page; `dc` is `full.stats_dc`, where the server keeps the numbers.
    pub(super) fn open_stats(&mut self, peer: PeerId, title: &str, megagroup: bool, dc: Option<i32>) {
        let dark = !glass::is_light();
        self.stats = StatsView { peer: Some(peer), title: title.to_owned(), dark, ..Default::default() };
        self.send(Cmd::LoadStats { peer, megagroup, dc, dark });
    }

    pub(super) fn stats_open(&self) -> bool {
        self.stats.peer.is_some()
    }

    pub(super) fn on_stats(&mut self, peer: PeerId, stats: Result<Box<ChannelStats>, String>) {
        if self.stats.peer != Some(peer) {
            return;
        }
        match stats {
            Ok(s) => self.stats.data = Some(*s),
            Err(e) => self.stats.error = Some(e),
        }
    }

    pub(super) fn on_stats_graph(&mut self, peer: PeerId, msg: Option<i32>, index: usize, graph: GraphState) {
        if self.stats.peer != Some(peer) {
            return;
        }
        let graphs = match msg {
            None => self.stats.data.as_mut().map(|d| &mut d.graphs),
            Some(id) => self.stats.post.as_mut().filter(|p| p.post.id == id).and_then(|p| p.data.as_mut()).map(|d| &mut d.graphs),
        };
        if let Some(g) = graphs.and_then(|g| g.get_mut(index)) {
            g.state = graph;
        }
    }

    pub(super) fn on_message_stats(&mut self, peer: PeerId, msg: i32, stats: Result<Box<MessageStats>, String>) {
        let Some(page) = self.stats.post.as_mut().filter(|p| self.stats.peer == Some(peer) && p.post.id == msg) else { return };
        match stats {
            Ok(s) => page.data = Some(*s),
            Err(e) => page.error = Some(e),
        }
    }

    fn open_post_stats(&mut self, post: PostStat) {
        let Some(peer) = self.stats.peer else { return };
        let (msg, dark) = (post.id, self.stats.dark);
        let dc = self.stats.data.as_ref().and_then(|d| d.dc);
        self.stats.post = Some(PostPage { post, data: None, error: None, hidden: HashSet::new(), asked: HashSet::new() });
        self.send(Cmd::LoadMessageStats { peer, msg, dc, dark });
    }

    pub(super) fn stats_view(&mut self, ui: &mut Ui) {
        let Some(peer) = self.stats.peer else { return };
        let screen = ui.max_rect();
        ui.painter().rect_filled(screen, 0.0, Color32::from_black_alpha(90));
        ui.interact(screen, ui.scope_id().with("stats-bg"), Sense::click());
        let rect = Rect::from_center_size(screen.center(), vec2(screen.width().min(620.0) - 20.0, (screen.height() - 40.0).min(860.0)));
        glass::glass(ui.painter(), rect, Glass::panel(26.0).with_shadow());
        let inner = rect.shrink(18.0);
        // A post's page has a back arrow in front of the title, as in Telegram Desktop.
        let left = if self.stats.post.is_some() {
            let back = Rect::from_min_size(pos2(inner.left() - 4.0, inner.top() - 4.0), vec2(32.0, 32.0));
            if glass::icon_button(ui, back, "stats-back", Icon::ChevronLeft, false, true).on_hover_text(l!("Назад", "Back")).clicked() {
                self.stats.post = None;
            }
            36.0
        } else {
            0.0
        };
        let (title, caption) = match &self.stats.post {
            Some(page) => (l!("Статистика допису", "Message statistics"), post_title(&page.post)),
            None => (l!("Статистика", "Statistics"), self.stats.title.clone()),
        };
        let p = ui.painter();
        p.text(inner.left_top() + vec2(left, 10.0), Align2::LEFT_CENTER, title, FontId::proportional(17.0), ink());
        p.galley(inner.left_top() + vec2(left, 22.0), elided(ui, &caption, 12.5, ink_dim(), inner.width() - 48.0 - left), ink_dim());
        let close = Rect::from_min_size(pos2(inner.right() - 32.0, inner.top() - 4.0), vec2(32.0, 32.0));
        let esc = ui.input_mut(|i| i.consume_key(egui::Modifiers::NONE, Key::Escape));
        if esc && self.stats.post.is_some() {
            self.stats.post = None;
        } else if glass::icon_button(ui, close, "stats-close", Icon::Close, false, true).clicked() || esc {
            self.stats = StatsView::default();
            return;
        }
        let body = Rect::from_min_max(pos2(inner.left(), inner.top() + 48.0), inner.max);
        let mut child = ui.new_child(UiBuilder::new().max_rect(body));
        child.set_clip_rect(body);
        if self.stats.post.is_some() {
            self.post_stats_body(&mut child, peer);
            return;
        }
        let st = &mut self.stats;
        let mut ask = Vec::new();
        let mut open = None;
        egui::ScrollArea::vertical().id_salt(("stats", peer)).auto_shrink(false).show(&mut child, |ui| {
            ui.spacing_mut().item_spacing.y = 6.0;
            let Some(d) = &st.data else {
                match &st.error {
                    Some(e) => {
                        ui.label(egui::RichText::new(e).color(glass::danger()).size(13.5));
                    }
                    None => {
                        ui.add(egui::Spinner::new().color(ink_dim()));
                    }
                }
                return;
            };
            section(ui, l!("Огляд", "Overview"), &d.period);
            tiles(ui, &d.tiles);
            graph_list(ui, &d.graphs, &mut st.hidden, &mut st.asked, st.dark, &mut ask);
            if !d.recent.is_empty() {
                ui.add_space(10.0);
                section(ui, l!("Нещодавні дописи", "Recent posts"), "");
                for post in &d.recent {
                    if post_row(ui, post).clicked() {
                        open = Some(post.clone());
                    }
                }
            }
            for (title, list) in [
                (l!("Найактивніші учасники", "Top members"), &d.posters),
                (l!("Найактивніші адміністратори", "Top admins"), &d.admins),
                (l!("Найактивніші запрошувачі", "Top inviters"), &d.inviters),
            ] {
                if !list.is_empty() {
                    ui.add_space(10.0);
                    section(ui, title, "");
                    for m in list {
                        member_row(ui, m);
                    }
                }
            }
            ui.add_space(8.0);
        });
        let dc = self.stats.data.as_ref().and_then(|d| d.dc);
        for (index, token) in ask {
            self.send(Cmd::LoadStatsGraph { peer, dc, msg: None, index, token });
        }
        if let Some(post) = open {
            self.open_post_stats(post);
        }
    }

    /// A post's page: its counters, the views and reactions graphs, then the public shares.
    fn post_stats_body(&mut self, ui: &mut Ui, peer: PeerId) {
        let dark = self.stats.dark;
        let dc = self.stats.data.as_ref().and_then(|d| d.dc);
        let Some(page) = self.stats.post.as_mut() else { return };
        let mut ask = Vec::new();
        egui::ScrollArea::vertical().id_salt(("stats-post", peer, page.post.id)).auto_shrink(false).show(ui, |ui| {
            ui.spacing_mut().item_spacing.y = 6.0;
            section(ui, l!("Огляд", "Overview"), &page.post.date);
            let mut counts = vec![
                StatTile { label: l!("Перегляди", "Views").into(), value: page.post.views as f64, prev: None, percent: false },
                StatTile { label: l!("Поширення", "Shares").into(), value: page.post.forwards as f64, prev: None, percent: false },
                StatTile { label: l!("Реакції", "Reactions").into(), value: page.post.reactions as f64, prev: None, percent: false },
            ];
            if let Some(n) = page.data.as_ref().and_then(|d| d.forwards_count) {
                counts.push(StatTile { label: l!("Публічні поширення", "Public shares").into(), value: n as f64, prev: None, percent: false });
            }
            tiles(ui, &counts);
            let Some(d) = &page.data else {
                match &page.error {
                    Some(e) => {
                        ui.label(egui::RichText::new(e).color(glass::danger()).size(13.5));
                    }
                    None => {
                        ui.add(egui::Spinner::new().color(ink_dim()));
                    }
                }
                return;
            };
            graph_list(ui, &d.graphs, &mut page.hidden, &mut page.asked, dark, &mut ask);
            if !d.forwards.is_empty() {
                ui.add_space(10.0);
                section(ui, l!("Публічні поширення", "Public shares"), "");
                for f in &d.forwards {
                    forward_row(ui, f);
                }
            }
            ui.add_space(8.0);
        });
        let msg = Some(page.post.id);
        for (index, token) in ask {
            self.send(Cmd::LoadStatsGraph { peer, dc, msg, index, token });
        }
    }
}

/// The graphs of a page, each under its title. Lazy ones are asked for (into `ask`) once they
/// scroll into view, as Telegram Desktop does; `hidden` keeps legend toggles per (graph, series).
fn graph_list(ui: &mut Ui, graphs: &[StatsGraph], hidden: &mut HashSet<(usize, usize)>, asked: &mut HashSet<usize>, dark: bool, ask: &mut Vec<(usize, String)>) {
    for (gi, g) in graphs.iter().enumerate() {
        ui.add_space(10.0);
        section(ui, &g.title, "");
        match &g.state {
            GraphState::Ready(c) => {
                let mut off: HashSet<usize> = hidden.iter().filter(|(g, _)| *g == gi).map(|(_, s)| *s).collect();
                stats_chart::chart(ui, c, &mut off, dark);
                hidden.retain(|(g, _)| *g != gi);
                hidden.extend(off.into_iter().map(|s| (gi, s)));
            }
            GraphState::Pending(token) => {
                let (r, _) = ui.allocate_exact_size(vec2(ui.available_width(), 120.0), Sense::hover());
                ui.put(Rect::from_center_size(r.center(), vec2(24.0, 24.0)), egui::Spinner::new().color(ink_dim()));
                if ui.is_rect_visible(r) && asked.insert(gi) {
                    ask.push((gi, token.clone()));
                }
            }
            GraphState::Failed(e) => {
                ui.label(egui::RichText::new(e).color(ink_dim()).size(12.5));
            }
        }
    }
}

/// What a post row and the post page call a post: its first line, else "Post #N" or "Story".
fn post_title(post: &PostStat) -> String {
    match (post.story, post.text.is_empty()) {
        (true, _) => l!("Історія", "Story").to_owned(),
        (false, true) => lf!("Допис №{}", "Post #{}", post.id),
        (false, false) => post.text.clone(),
    }
}

/// A chat that shared the post: avatar, name, its views.
fn forward_row(ui: &mut Ui, f: &PublicForward) {
    let (r, _) = ui.allocate_exact_size(vec2(ui.available_width(), 46.0), Sense::hover());
    let p = ui.painter();
    glass::avatar(p, pos2(r.left() + 22.0, r.center().y), 17.0, &f.title, 0, None, None);
    let w = r.width() - 60.0;
    p.galley(pos2(r.left() + 50.0, r.top() + 5.0), elided(ui, &f.title, 14.0, ink(), w), ink());
    let mut line = if f.story { l!("Історія", "Story").to_owned() } else { l!("Допис", "Post").to_owned() };
    if let Some(v) = f.views {
        line = format!("{line} · {}", lp!(v, ["{n} перегляд", "{n} перегляди", "{n} переглядів"], ["{n} view", "{n} views"]));
    }
    p.galley(pos2(r.left() + 50.0, r.top() + 25.0), elided(ui, &line, 12.5, ink_dim(), w), ink_dim());
}

/// A section title with an optional caption on the right (the period).
fn section(ui: &mut Ui, title: &str, caption: &str) {
    let (r, _) = ui.allocate_exact_size(vec2(ui.available_width(), 24.0), Sense::hover());
    let p = ui.painter();
    p.text(pos2(r.left() + 2.0, r.center().y), Align2::LEFT_CENTER, title, FontId::proportional(14.5), ink());
    if !caption.is_empty() {
        p.text(pos2(r.right() - 2.0, r.center().y), Align2::RIGHT_CENTER, caption, FontId::proportional(12.5), ink_dim());
    }
}

/// The change against the previous period: "+52 (1.2%)", coloured up or down.
fn change(t: &StatTile) -> Option<(String, Color32)> {
    let prev = t.prev?;
    let diff = t.value - prev;
    let sign = if diff > 0.0 { "+" } else if diff < 0.0 { "−" } else { "" };
    let pct = if prev != 0.0 { format!(" ({:.1}%)", (diff / prev * 100.0).abs()) } else { String::new() };
    let color = if diff > 0.0 { glass::positive() } else if diff < 0.0 { glass::danger() } else { ink_dim() };
    Some((format!("{sign}{}{pct}", compact(diff.abs())), color))
}

/// The overview numbers, two or three per row.
fn tiles(ui: &mut Ui, tiles: &[StatTile]) {
    let w = ui.available_width();
    let cols = if w > 420.0 { 3 } else { 2 };
    let tw = (w - 8.0 * (cols - 1) as f32) / cols as f32;
    for row in tiles.chunks(cols) {
        let (r, _) = ui.allocate_exact_size(vec2(w, 62.0), Sense::hover());
        for (j, t) in row.iter().enumerate() {
            let cell = Rect::from_min_size(pos2(r.left() + j as f32 * (tw + 8.0), r.top()), vec2(tw, 62.0));
            let p = ui.painter();
            p.rect_filled(cell, 14.0, glass::veil(10));
            let value = if t.percent { format!("{:.1}%", t.value) } else { compact(t.value) };
            let big = p.layout_no_wrap(value, FontId::proportional(18.0), ink());
            let bw = big.size().x;
            p.galley(pos2(cell.left() + 12.0, cell.top() + 9.0), big, ink());
            if let Some((text, color)) = change(t) {
                let room = cell.width() - bw - 30.0;
                p.galley(pos2(cell.left() + 18.0 + bw, cell.top() + 14.0), elided(ui, &text, 12.0, color, room), color);
            }
            p.galley(pos2(cell.left() + 12.0, cell.top() + 36.0), elided(ui, &t.label, 12.5, ink_dim(), cell.width() - 24.0), ink_dim());
        }
    }
}

/// A recent post: its first line and date, views on the right, shares and reactions under them.
/// Posts (not stories) open their own page on click.
fn post_row(ui: &mut Ui, post: &PostStat) -> egui::Response {
    let w = ui.available_width();
    let (r, resp) = ui.allocate_exact_size(vec2(w, 50.0), if post.story { Sense::hover() } else { Sense::click() });
    let hot = resp.hovered() && !post.story;
    if hot {
        ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
    }
    let p = ui.painter();
    p.rect_filled(r, 12.0, glass::veil(if hot { 16 } else { 8 }));
    // Big counts are shortened as in Telegram ("12.8K views"), where the plural is always "many".
    let views = if post.views < 10_000 {
        lp!(post.views, ["{n} перегляд", "{n} перегляди", "{n} переглядів"], ["{n} view", "{n} views"])
    } else {
        lf!("{} переглядів", "{} views", compact(post.views as f64))
    };
    let right = p.layout_no_wrap(views, FontId::proportional(13.5), ink());
    let rw = right.size().x;
    p.galley(pos2(r.right() - 12.0 - rw, r.top() + 8.0), right, ink());
    let mut extra = Vec::new();
    if post.forwards > 0 {
        extra.push(lp!(post.forwards, ["{n} поширення", "{n} поширення", "{n} поширень"], ["{n} share", "{n} shares"]));
    }
    if post.reactions > 0 {
        extra.push(lp!(post.reactions, ["{n} реакція", "{n} реакції", "{n} реакцій"], ["{n} reaction", "{n} reactions"]));
    }
    let extra = p.layout_no_wrap(extra.join(" · "), FontId::proportional(12.0), ink_dim());
    let ew = extra.size().x;
    p.galley(pos2(r.right() - 12.0 - ew, r.top() + 28.0), extra, ink_dim());
    let text = post_title(post);
    let tw = r.width() - rw.max(ew) - 36.0;
    p.galley(pos2(r.left() + 12.0, r.top() + 8.0), elided(ui, &text, 13.5, ink(), tw), ink());
    if !post.date.is_empty() {
        p.galley(pos2(r.left() + 12.0, r.top() + 28.0), elided(ui, &post.date, 12.0, ink_dim(), tw), ink_dim());
    }
    resp
}

/// A member in a top list: avatar, name and what they did.
fn member_row(ui: &mut Ui, m: &TopMember) {
    let (r, _) = ui.allocate_exact_size(vec2(ui.available_width(), 46.0), Sense::hover());
    let p = ui.painter();
    glass::avatar(p, pos2(r.left() + 22.0, r.center().y), 17.0, &m.name, m.user, None, None);
    let w = r.width() - 60.0;
    p.galley(pos2(r.left() + 50.0, r.top() + 5.0), elided(ui, &m.name, 14.0, ink(), w), ink());
    p.galley(pos2(r.left() + 50.0, r.top() + 25.0), elided(ui, &m.line, 12.5, ink_dim(), w), ink_dim());
}
