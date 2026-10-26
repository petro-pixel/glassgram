//! Composer helpers: link preview above the field, @mention and `/command` popups,
//! the bot command menu button, the "Start" bar of an empty bot chat, and link cards in bubbles.

use std::collections::{HashMap, HashSet};

use eframe::egui::{self, FontId, Key, Rect, Sense, TextureHandle, Ui, pos2, text::LayoutJob, text::TextWrapping, vec2};
use grammers_client::session::types::PeerId;

use super::{Action, App, COMPOSE_BAR_H, COMPOSER_H, SEARCH_DELAY, elided};
use crate::backend::{BotCmd, ChatKind, Cmd, Ev, ImgKey, Mentionable, Msg, WebPageInfo};
use crate::format;
use crate::glass::{self, ACCENT, Glass, ink, ink_dim};
use crate::i18n::l;
use crate::icons::{self, Icon};

/// Height of one popup row.
const ROW_H: f32 = 46.0;
/// Rows visible in the popup before it scrolls.
const ROWS: usize = 6;
/// Member search is cheap, so it waits less than the general search delay.
const MEMBERS_DELAY: f64 = 0.2;

/// Composer helper state; kept in one field of `App`.
#[derive(Default)]
pub(super) struct Suggest {
    /// First link in the draft and when it last changed (for debouncing).
    link: Option<String>,
    link_changed_at: f64,
    /// Link a preview request was sent for.
    link_requested: Option<String>,
    /// Link → its preview (`None`: the server has no preview for it).
    pages: HashMap<String, Option<WebPageInfo>>,
    /// Link whose preview was turned off with ✕.
    dismissed: Option<String>,
    /// Last members response: chat, query, members.
    pub(super) members: Option<(PeerId, String, Vec<Mentionable>)>,
    members_requested: Option<(PeerId, String)>,
    members_changed_at: f64,
    commands: HashMap<PeerId, Vec<BotCmd>>,
    commands_requested: HashSet<PeerId>,
    /// Highlighted popup row.
    selected: usize,
    /// Token the popup was closed for with Esc.
    closed_for: Option<String>,
    /// Bot command menu opened with the button next to the field.
    menu_open: bool,
    /// Bots whose menu button opens a mini app: (button text, URL).
    menu_apps: HashMap<PeerId, (String, String)>,
}

/// What the popup offers.
enum Popup {
    Mentions(String),
    Commands(String),
}

/// One popup row.
enum Item {
    Member(Mentionable),
    Command(BotCmd),
}

impl App {
    pub(super) fn suggest_event(&mut self, ev: Ev) {
        let s = &mut self.suggest;
        match ev {
            Ev::WebPreview { text, page } => {
                s.pages.insert(text, page);
            }
            Ev::Mentions { peer, query, members } => {
                if self.selected == Some(peer) {
                    s.members = Some((peer, query, members));
                }
            }
            Ev::BotCommands { peer, commands } => {
                s.commands.insert(peer, commands);
            }
            _ => {}
        }
    }

    /// Opts for sending the draft: no preview if the user turned it off.
    pub(super) fn link_preview_off(&self) -> bool {
        self.suggest.link.is_some() && self.suggest.link == self.suggest.dismissed
    }

    /// Forgets the composer state of the previous chat.
    pub(super) fn suggest_reset(&mut self) {
        let s = &mut self.suggest;
        s.dismissed = None;
        s.link = None;
        s.members = None;
        s.members_requested = None;
        s.closed_for = None;
        s.menu_open = false;
        s.selected = 0;
    }

    /// Link preview bar above the field; returns the new top of the free space.
    pub(super) fn link_preview_bar(&mut self, ui: &mut Ui, left: f32, right: f32, above: f32, now: f64) -> f32 {
        let link = first_link(&self.draft);
        let s = &mut self.suggest;
        if link != s.link {
            s.link = link.clone();
            s.link_changed_at = now;
        }
        let Some(link) = link else { return above };
        if s.dismissed.as_ref() == Some(&link) {
            return above;
        }
        if s.link_requested.as_ref() != Some(&link) && !s.pages.contains_key(&link) {
            if now - s.link_changed_at > SEARCH_DELAY {
                s.link_requested = Some(link.clone());
                self.send(Cmd::WebPreview(link.clone()));
            } else {
                ui.ctx().request_repaint_after(std::time::Duration::from_secs_f64(SEARCH_DELAY));
            }
        }
        let Some(Some(page)) = self.suggest.pages.get(&link).cloned() else { return above };
        let rect = Rect::from_min_max(pos2(left, above - COMPOSE_BAR_H), pos2(right, above));
        glass::glass(ui.painter(), rect, Glass::tinted(rect.height() / 2.0, glass::chrome(), 130).with_shadow().frosted(0.96));
        let p = ui.painter().clone();
        icons::draw(&p, Rect::from_center_size(pos2(rect.left() + 20.0, rect.center().y), vec2(18.0, 18.0)), Icon::Link, ACCENT);
        p.rect_filled(Rect::from_min_size(pos2(rect.left() + 36.0, rect.top() + 8.0), vec2(3.0, rect.height() - 16.0)), 2.0, ACCENT);
        let mut x = rect.left() + 46.0;
        if let Some(t) = self.textures.get(&ImgKey::WebPage(page.id)) {
            let thumb = Rect::from_min_size(pos2(x, rect.top() + 6.0), vec2(30.0, 30.0));
            glass::image_rounded(&p, thumb, 6.0, t.id(), t.size_vec2());
            x += 38.0;
        }
        let w = rect.right() - 50.0 - x;
        let title = if page.title.is_empty() { page.site.clone() } else { page.title.clone() };
        let sub = if page.description.is_empty() { page.url.clone() } else { page.description.replace('\n', " ") };
        p.galley(pos2(x, rect.top() + 5.0), elided(ui, &title, 12.5, ACCENT, w), ACCENT);
        p.galley(pos2(x, rect.top() + 21.0), elided(ui, &sub, 13.0, ink_dim(), w), ink_dim());
        let close = Rect::from_center_size(pos2(rect.right() - 22.0, rect.center().y), vec2(28.0, 28.0));
        if glass::icon_button(ui, close, "link-preview-close", Icon::Close, false, true).on_hover_text(l!("Не показувати попередній перегляд", "Do not show link preview")).clicked() {
            self.suggest.dismissed = Some(link);
        }
        rect.top() - 8.0
    }

    /// Width taken by the bot command menu button left of the field.
    pub(super) fn bot_menu_width(&mut self, peer: PeerId, kind: ChatKind) -> f32 {
        if matches!(kind, ChatKind::Bot | ChatKind::Group) && self.suggest.commands_requested.insert(peer) {
            self.send(Cmd::LoadBotCommands(peer));
        }
        if kind == ChatKind::Bot
            && let Some((text, _)) = self.suggest.menu_apps.get(&peer)
        {
            return menu_app_width(text) + 8.0;
        }
        let has = kind == ChatKind::Bot && self.suggest.commands.get(&peer).is_some_and(|c| !c.is_empty());
        if has { COMPOSER_H + 8.0 } else { 0.0 }
    }

    pub(super) fn suggest_menu_app(&mut self, peer: PeerId, text: String, url: String) {
        self.suggest.menu_apps.insert(peer, (text, url));
    }

    /// Bot command menu button; `at` is its square.
    pub(super) fn bot_menu_button(&mut self, ui: &mut Ui, at: Rect) {
        // A bot with a mini app shows its own button text instead of the command list, as in Telegram Desktop.
        if let Some(peer) = self.selected
            && let Some((text, url)) = self.suggest.menu_apps.get(&peer).cloned()
        {
            let r = Rect::from_min_size(at.min, vec2(menu_app_width(&text), at.height()));
            if glass::button(ui, r, ("bot-menu-app", peer), &text, true, true).clicked() {
                self.open_web_app(peer, peer, crate::backend::WebAppKind::Menu(url), String::new());
            }
            return;
        }
        let open = self.suggest.menu_open;
        if glass::icon_button(ui, at, "bot-menu", Icon::Commands, open, true).on_hover_text(l!("Меню команд бота", "Bot command menu")).clicked() {
            self.suggest.menu_open = !open;
            self.suggest.selected = 0;
        }
    }

    /// "Start" instead of the composer in a bot chat without messages, or opened from a start link.
    /// Returns `true` when the bar replaced the composer.
    pub(super) fn start_bot_bar(&mut self, ui: &mut Ui, peer: PeerId, kind: ChatKind, rect: Rect) -> bool {
        let empty = self.history.get(&peer).is_some_and(Vec::is_empty);
        let linked = self.pending_bot_start(peer).is_some();
        if kind != ChatKind::Bot || !(empty || linked) || !self.draft.is_empty() || self.topic.is_some() {
            return false;
        }
        let bar = Rect::from_min_max(pos2(rect.left(), rect.bottom() - COMPOSER_H), rect.max);
        if glass::button(ui, bar, ("start-bot", peer), l!("Почати", "Start"), true, true).clicked() {
            self.start_bot(peer);
        }
        true
    }

    /// The @mention / `/command` popup over the field. Runs before the field is drawn so that
    /// ↑/↓/Enter/Tab/Esc reach the popup and not the text editor.
    pub(super) fn suggest_view(&mut self, ui: &mut Ui, peer: PeerId, kind: ChatKind, rect: Rect, now: f64) {
        let popup = match current_token(&self.draft) {
            Some(t) if self.suggest.closed_for.as_ref() == Some(&t) => None,
            Some(t) if t.starts_with('@') && kind == ChatKind::Group => Some(Popup::Mentions(t[1..].to_owned())),
            Some(t) if t.starts_with('/') && t.len() == self.draft.len() => Some(Popup::Commands(t[1..].to_owned())),
            _ if self.suggest.menu_open => Some(Popup::Commands(String::new())),
            _ => None,
        };
        if current_token(&self.draft).is_none() {
            self.suggest.closed_for = None;
        }
        let items = match &popup {
            None => return,
            Some(Popup::Mentions(q)) => {
                self.request_members(peer, q, now);
                self.member_items(peer, q)
            }
            Some(Popup::Commands(q)) => {
                if self.suggest.commands_requested.insert(peer) {
                    self.send(Cmd::LoadBotCommands(peer));
                }
                self.command_items(peer, q)
            }
        };
        if items.is_empty() {
            return;
        }
        let s = &mut self.suggest;
        s.selected = s.selected.min(items.len() - 1);
        let (up, down, tab, enter, esc) = ui.input_mut(|i| {
            let mut k = |key| i.consume_key(egui::Modifiers::NONE, key);
            (k(Key::ArrowUp), k(Key::ArrowDown), k(Key::Tab), k(Key::Enter), k(Key::Escape))
        });
        if up {
            s.selected = s.selected.checked_sub(1).unwrap_or(items.len() - 1);
        }
        if down {
            s.selected = (s.selected + 1) % items.len();
        }
        if esc {
            s.menu_open = false;
            s.closed_for = current_token(&self.draft);
            return;
        }
        let h = items.len().min(ROWS) as f32 * ROW_H + 16.0;
        let panel = Rect::from_min_max(pos2(rect.left(), (rect.bottom() - h).max(rect.top())), rect.max);
        glass::glass(ui.painter(), panel, Glass::tinted(22.0, glass::chrome(), 160).frosted(1.0).with_shadow());
        ui.interact(panel, ui.scope_id().with("suggest-bg"), Sense::click());
        let inner = panel.shrink2(vec2(8.0, 8.0));
        let mut child = ui.new_child(egui::UiBuilder::new().max_rect(inner));
        child.set_clip_rect(inner);
        let selected = s.selected;
        let mut clicked = None;
        egui::ScrollArea::vertical().id_salt("suggest-list").auto_shrink(false).show(&mut child, |ui| {
            for (k, item) in items.iter().enumerate() {
                let w = ui.available_width();
                let (row, resp) = ui.allocate_exact_size(vec2(w, ROW_H), Sense::click());
                if k == selected && (up || down) {
                    ui.scroll_to_rect(row, None);
                }
                let hover = ui.ctx().animate_bool(resp.id, resp.hovered() || k == selected);
                if hover > 0.0 {
                    ui.painter().rect_filled(row, 14.0, glass::veil((hover * 16.0) as u8));
                }
                self_row(ui, row, item, self.textures.get(&item_avatar(item)));
                if resp.clicked() {
                    clicked = Some(k);
                }
            }
        });
        let chosen = clicked.map(|k| (k, true)).or(if enter { Some((selected, true)) } else if tab { Some((selected, false)) } else { None });
        let Some((k, send)) = chosen else { return };
        let Some(item) = items.into_iter().nth(k) else { return };
        let several_bots = self.suggest.commands.get(&peer).is_some_and(|c| c.iter().any(|x| x.bot != c[0].bot));
        self.suggest.menu_open = false;
        self.suggest.selected = 0;
        match item {
            Item::Member(m) => {
                let mention = if m.username.is_empty() { m.name.clone() } else { format!("@{}", m.username) };
                replace_token(&mut self.draft, &format!("{mention} "));
                focus_end(ui, &self.draft);
            }
            Item::Command(c) => {
                let cmd = if several_bots && !c.bot.is_empty() { format!("/{}@{}", c.command, c.bot) } else { format!("/{}", c.command) };
                if send {
                    // Like Telegram: choosing a command sends it right away; Tab only inserts it.
                    self.draft = cmd;
                    self.send_draft();
                } else {
                    self.draft = format!("{cmd} ");
                    focus_end(ui, &self.draft);
                }
            }
        }
    }

    fn request_members(&mut self, peer: PeerId, query: &str, now: f64) {
        let s = &mut self.suggest;
        let key = Some((peer, query.to_owned()));
        if s.members_requested == key {
            return;
        }
        if s.members_changed_at == 0.0 {
            s.members_changed_at = now;
        }
        if now - s.members_changed_at < MEMBERS_DELAY {
            self.ctx.request_repaint_after(std::time::Duration::from_secs_f64(MEMBERS_DELAY));
            return;
        }
        s.members_changed_at = 0.0;
        s.members_requested = key;
        self.send(Cmd::LoadMentions { peer, query: query.to_owned() });
    }

    fn member_items(&self, peer: PeerId, query: &str) -> Vec<Item> {
        let Some((p, _, members)) = &self.suggest.members else { return Vec::new() };
        if *p != peer {
            return Vec::new();
        }
        // The previous answer is filtered locally until the new one arrives, so the list does not blink.
        let q = query.to_lowercase();
        members
            .iter()
            .filter(|m| m.name.to_lowercase().contains(&q) || m.username.to_lowercase().starts_with(&q))
            .cloned()
            .map(Item::Member)
            .collect()
    }

    fn command_items(&self, peer: PeerId, query: &str) -> Vec<Item> {
        let q = query.to_lowercase();
        let q = q.split('@').next().unwrap_or_default();
        self.suggest
            .commands
            .get(&peer)
            .into_iter()
            .flatten()
            .filter(|c| c.command.to_lowercase().starts_with(q))
            .cloned()
            .map(Item::Command)
            .collect()
    }
}

fn item_avatar(item: &Item) -> ImgKey {
    match item {
        Item::Member(m) => ImgKey::Avatar(m.id),
        // Bot avatars are not loaded separately; the key just finds nothing.
        Item::Command(_) => ImgKey::WebPage(0),
    }
}

fn self_row(ui: &Ui, row: Rect, item: &Item, avatar: Option<&TextureHandle>) {
    let p = ui.painter();
    let w = row.width() - 66.0;
    match item {
        Item::Member(m) => {
            glass::avatar(p, pos2(row.left() + 26.0, row.center().y), 17.0, &m.name, m.id.bot_api_dialog_id_unchecked(), m.bot.then_some(Icon::Bot), avatar);
            let name = elided(ui, &m.name, 14.5, ink(), w);
            let name_w = name.size().x;
            p.galley(pos2(row.left() + 54.0, row.center().y - 9.0), name, ink());
            if !m.username.is_empty() {
                let at = pos2(row.left() + 54.0 + name_w + 8.0, row.center().y - 8.0);
                p.galley(at, elided(ui, &format!("@{}", m.username), 13.0, ink_dim(), (w - name_w - 8.0).max(0.0)), ink_dim());
            }
        }
        Item::Command(c) => {
            let badge = Rect::from_center_size(pos2(row.left() + 26.0, row.center().y), vec2(34.0, 34.0));
            glass::glass(p, badge, Glass::tinted(17.0, ACCENT, 150));
            icons::draw(p, Rect::from_center_size(badge.center(), vec2(18.0, 18.0)), Icon::Commands, ink());
            let bold = FontId::new(14.5, egui::FontFamily::Name(format::BOLD.into()));
            let cmd = p.layout_no_wrap(format!("/{}", c.command), bold, ink());
            let cmd_w = cmd.size().x;
            p.galley(pos2(row.left() + 54.0, row.center().y - 9.0), cmd, ink());
            let at = pos2(row.left() + 54.0 + cmd_w + 10.0, row.center().y - 8.0);
            p.galley(at, elided(ui, &c.description, 13.0, ink_dim(), (w - cmd_w - 10.0).max(0.0)), ink_dim());
        }
    }
}

/// Puts the cursor at the end of the composer text and keeps the focus there.
fn focus_end(ui: &Ui, text: &str) {
    let id = ui.scope_id().with(("edit", l!("Повідомлення", "Message")));
    if let Some(mut st) = egui::TextEdit::load_state(ui.ctx(), id) {
        let c = egui::text::CCursor::new(text.chars().count());
        st.cursor.set_char_range(Some(egui::text::CCursorRange::one(c)));
        st.store(ui.ctx(), id);
    }
    ui.memory_mut(|m| m.request_focus(id));
}

/// The word being typed at the end of the draft, if it is `@…` or `/…`.
fn current_token(draft: &str) -> Option<String> {
    let start = draft.rfind(char::is_whitespace).map_or(0, |i| i + draft[i..].chars().next().map_or(1, char::len_utf8));
    let t = &draft[start..];
    let body = t.get(1..)?;
    let ok = (t.starts_with('@') || t.starts_with('/')) && body.chars().all(|c| c.is_alphanumeric() || c == '_' || (t.starts_with('/') && c == '@'));
    ok.then(|| t.to_owned())
}

fn replace_token(draft: &mut String, with: &str) {
    let start = draft.rfind(char::is_whitespace).map_or(0, |i| i + draft[i..].chars().next().map_or(1, char::len_utf8));
    draft.truncate(start);
    draft.push_str(with);
}

/// First link in the text: a bare URL, `t.me/…`, `www.…`, or the target of `[text](url)`.
pub(super) fn first_link(text: &str) -> Option<String> {
    let lower = text.to_lowercase();
    let starts = ["https://", "http://", "www.", "t.me/"];
    let at = starts.iter().filter_map(|s| lower.find(s)).min()?;
    // Only at a word start (or right after `(` of markdown) — not inside another word.
    let before = text[..at].chars().next_back();
    if before.is_some_and(|c| !c.is_whitespace() && c != '(') {
        return None;
    }
    let rest = &text[at..];
    let end = rest.find(|c: char| c.is_whitespace() || c == ')').unwrap_or(rest.len());
    let link = rest[..end].trim_end_matches(['.', ',', '!', '?', ':', ';', '"', '\'']);
    (link.len() > 4 && link.contains('.')).then(|| link.to_owned())
}

/// Laid-out link card for a bubble.
pub(super) struct WebCard {
    site: Option<std::sync::Arc<egui::Galley>>,
    title: Option<std::sync::Arc<egui::Galley>>,
    desc: Option<std::sync::Arc<egui::Galley>>,
    img_h: f32,
    pub(super) width: f32,
}

const CARD_PAD: f32 = 10.0;

impl WebCard {
    pub(super) fn layout(ui: &Ui, w: &WebPageInfo, width: f32, tex: Option<&TextureHandle>, accent: egui::Color32) -> Self {
        let inner = width - CARD_PAD - 6.0;
        let job = |text: &str, font: FontId, color, rows| {
            let mut job = LayoutJob::simple(text.to_owned(), font, color, inner);
            job.wrap = TextWrapping { max_rows: rows, break_anywhere: false, overflow_character: Some('…'), max_width: inner };
            ui.painter().layout_job(job)
        };
        let bold = |size| FontId::new(size, egui::FontFamily::Name(format::BOLD.into()));
        let site = (!w.site.is_empty()).then(|| job(&w.site, bold(13.0), accent, 1));
        let title = (!w.title.is_empty()).then(|| job(&w.title, bold(14.0), ink(), 2));
        let desc = (!w.description.is_empty()).then(|| job(&w.description, FontId::proportional(13.5), ink(), 4));
        let img_h = match tex {
            Some(t) => (inner * t.size_vec2().y / t.size_vec2().x.max(1.0)).clamp(80.0, 220.0),
            None if w.photo => 140.0,
            None => 0.0,
        };
        Self { site, title, desc, img_h, width }
    }

    pub(super) fn height(&self) -> f32 {
        let g = |x: &Option<std::sync::Arc<egui::Galley>>| x.as_ref().map_or(0.0, |g| g.size().y + 2.0);
        8.0 + g(&self.site) + g(&self.title) + g(&self.desc) + if self.img_h > 0.0 { self.img_h + 6.0 } else { 0.0 } + 6.0
    }

    /// Draws the card at `min`; a click opens the link.
    pub(super) fn draw(self, ui: &mut Ui, m: &Msg, min: egui::Pos2, tex: Option<&TextureHandle>, accent: egui::Color32) -> Option<Action> {
        let rect = Rect::from_min_size(min, vec2(self.width, self.height()));
        let p = ui.painter().clone();
        p.rect_filled(rect, 8.0, glass::veil(16));
        p.rect_filled(Rect::from_min_size(rect.min, vec2(3.0, rect.height())), 2.0, accent);
        let mut y = rect.top() + 6.0;
        for g in [self.site, self.title, self.desc].into_iter().flatten() {
            let h = g.size().y;
            p.galley(pos2(rect.left() + CARD_PAD, y), g, ink());
            y += h + 2.0;
        }
        if self.img_h > 0.0 {
            let ir = Rect::from_min_size(pos2(rect.left() + CARD_PAD, y + 4.0), vec2(self.width - CARD_PAD - 6.0, self.img_h));
            match tex {
                Some(t) => glass::image_rounded(&p, ir, 8.0, t.id(), t.size_vec2()),
                None => {
                    p.rect_filled(ir, 8.0, glass::veil(14));
                    icons::draw(&p, Rect::from_center_size(ir.center(), vec2(26.0, 26.0)), Icon::Camera, ink_dim());
                }
            }
        }
        let resp = ui.interact(rect, ui.scope_id().with(("webpage", m.id)), Sense::click());
        let url = m.webpage.as_ref().map(|w| w.url.clone()).unwrap_or_default();
        if resp.hovered() {
            ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
        }
        let resp = resp.on_hover_text(&url);
        resp.clicked().then_some(Action::Open(url))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn finds_links() {
        assert_eq!(first_link("see https://example.com/a, ok").as_deref(), Some("https://example.com/a"));
        assert_eq!(first_link("[docs](https://docs.rs/egui)").as_deref(), Some("https://docs.rs/egui"));
        assert_eq!(first_link("t.me/durov").as_deref(), Some("t.me/durov"));
        assert_eq!(first_link("no links here"), None);
        assert_eq!(first_link("mail@www.example.com"), None);
    }

    #[test]
    fn tokens() {
        assert_eq!(current_token("hi @iri").as_deref(), Some("@iri"));
        assert_eq!(current_token("@").as_deref(), Some("@"));
        assert_eq!(current_token("/wea").as_deref(), Some("/wea"));
        assert_eq!(current_token("hi @iri "), None);
        assert_eq!(current_token("hello"), None);
        let mut d = "Привіт @ір".to_owned();
        replace_token(&mut d, "@iryna ");
        assert_eq!(d, "Привіт @iryna ");
    }
}

/// Width of the mini app menu button: its text plus capsule padding, within limits.
fn menu_app_width(text: &str) -> f32 {
    (text.chars().count() as f32 * 8.0 + 32.0).clamp(COMPOSER_H, 160.0)
}
