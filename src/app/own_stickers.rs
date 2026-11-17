//! Our own sticker sets, in the box opened from the sticker panel's "My sets" tab: "Create a new
//! sticker set" as in @Stickers (title, link name checked while typed, stickers or custom emoji,
//! pictures and videos with an emoji each), the sets we made, and editing one: add a sticker,
//! change its emoji, move, delete, rename or delete the set. A sticker's or photo's menu in a chat
//! has "Add to sticker set", which picks one of these sets.

use std::sync::{Arc, Mutex};

use super::*;
use crate::backend::{NewSticker, OWN_EMOJI_MAX, OWN_STICKERS_MAX, OwnEv, OwnOp, OwnSet, STICKER_FILE_EXTS, short_name_ok};

/// A pause in typing before the link name is checked or suggested.
const NAME_DELAY: f64 = 0.5;
/// Files picked for a new set at once: each is converted and uploaded.
const DRAFT_MAX: usize = 50;
const TITLE_MAX: usize = 64;
/// The emoji a picked file starts with.
const DEFAULT_EMOJI: &str = "🙂";
const CELL: f32 = 84.0;

#[derive(Default, Clone, Copy, PartialEq)]
enum Page {
    #[default]
    List,
    Create,
    Set(i64),
}

/// A file picked for a sticker, with its preview (a video has none).
struct Draft {
    path: PathBuf,
    emoji: String,
    tex: Option<TextureHandle>,
}

/// A picked file and its preview, made on the picker's thread; `bool`: for the open set.
type Picked = (PathBuf, Option<egui::ColorImage>, bool);

/// The own sets box and its forms.
#[derive(Default)]
pub(super) struct OwnStickers {
    open: bool,
    page: Page,
    /// The sets we made; `None` until the server answers.
    sets: Option<Vec<OwnSet>>,
    /// The new set: title, link name, custom emoji instead of stickers, picked files.
    title: String,
    short_name: String,
    /// The link name was typed by hand, so the title no longer fills it in.
    name_typed: bool,
    /// When the title or link last changed.
    typed_at: f64,
    /// The link name asked about and the answer: `Some(None)` free, `Some(Some(why))` not.
    checked: Option<(String, Option<Option<String>>)>,
    /// The title the last suggestion was asked for.
    suggested_for: String,
    emojis: bool,
    drafts: Vec<Draft>,
    picked: Arc<Mutex<Vec<Picked>>>,
    /// Stickers converted and uploaded so far, while a create or add runs.
    busy: Option<(usize, usize)>,
    error: Option<String>,
    /// The open set's new title while renaming.
    rename: Option<String>,
    /// The sticker whose emoji is being changed, and the new emoji.
    emoji_edit: Option<(i64, String)>,
    /// A file picked for the open set, waiting for its emoji.
    adding: Option<Draft>,
    confirm_delete: bool,
    /// "Add to sticker set" from a message: (chat, message, emoji).
    from_msg: Option<(PeerId, i32, String)>,
}

/// A small preview of a picture for the form; videos get an icon instead.
fn preview(path: &std::path::Path) -> Option<egui::ColorImage> {
    let img = image::ImageReader::open(path).ok()?.with_guessed_format().ok()?.decode().ok()?.thumbnail(160, 160).to_rgba8();
    Some(egui::ColorImage::from_rgba_unmultiplied([img.width() as usize, img.height() as usize], img.as_raw()))
}

fn count_label(n: usize, emojis: bool) -> String {
    if emojis { lp!(n, ["{n} емодзі", "{n} емодзі", "{n} емодзі"], ["{n} emoji", "{n} emoji"]) } else { lp!(n, ["{n} стікер", "{n} стікери", "{n} стікерів"], ["{n} sticker", "{n} stickers"]) }
}

impl App {
    /// The buttons in the panel's "My sets" tab that open the box.
    pub(super) fn own_stickers_entry(&mut self, ui: &mut Ui, rect: Rect) {
        let plus = Rect::from_min_size(pos2(rect.right() - 28.0, rect.top()), vec2(28.0, 28.0));
        if glass::icon_button(ui, plus, "own-create", Icon::Plus, false, true).on_hover_text(l!("Створити новий набір стікерів", "Create a new sticker set")).clicked() {
            self.open_own_stickers(Page::Create);
        }
        let mine = Rect::from_min_max(pos2((plus.left() - 130.0).max(rect.left()), rect.top()), pos2(plus.left() - 6.0, rect.bottom()));
        if glass::button(ui, mine, "own-list", l!("Ваші набори", "Your sets"), false, true).clicked() {
            self.open_own_stickers(Page::List);
        }
    }

    fn open_own_stickers(&mut self, page: Page) {
        let o = &mut self.own_stickers;
        o.open = true;
        o.error = None;
        o.from_msg = None;
        Self::go(o, page);
        if self.own_stickers.sets.is_none() {
            self.send(Cmd::OwnStickers(OwnOp::Load));
        }
    }

    fn go(o: &mut OwnStickers, page: Page) {
        o.page = page;
        o.rename = None;
        o.emoji_edit = None;
        o.adding = None;
        o.confirm_delete = false;
    }

    /// "Add to sticker set" in a message's menu: the list of our sets to pick from.
    pub(super) fn add_to_own_set(&mut self, peer: PeerId, id: i32) {
        let emoji = self.history.get(&peer).and_then(|h| h.iter().find(|m| m.id == id)).and_then(|m| m.sticker.as_ref().map(|s| s.1.clone())).filter(|e| !e.is_empty());
        self.open_own_stickers(Page::List);
        self.own_stickers.from_msg = Some((peer, id, emoji.unwrap_or_else(|| DEFAULT_EMOJI.into())));
    }

    fn pick_sticker_files(&self, for_set: bool) {
        let (picked, ctx) = (self.own_stickers.picked.clone(), self.ctx.clone());
        std::thread::spawn(move || {
            let dialog = rfd::FileDialog::new().set_title(l!("Додати стікери", "Add stickers")).add_filter(l!("Зображення та відео", "Pictures and videos"), &STICKER_FILE_EXTS);
            let paths = if for_set { dialog.pick_file().into_iter().collect() } else { dialog.pick_files().unwrap_or_default() };
            let mut got: Vec<Picked> = paths.into_iter().map(|p| (p.clone(), preview(&p), for_set)).collect();
            if let Ok(mut v) = picked.lock() {
                v.append(&mut got);
            }
            ctx.request_repaint();
        });
    }

    fn take_picked(&mut self) {
        let got: Vec<Picked> = self.own_stickers.picked.lock().map(|mut v| std::mem::take(&mut *v)).unwrap_or_default();
        for (path, img, for_set) in got {
            let tex = img.map(|i| self.ctx.load_texture(format!("own-sticker-{}", path.display()), i, TextureOptions::LINEAR));
            let draft = Draft { path, emoji: DEFAULT_EMOJI.into(), tex };
            let o = &mut self.own_stickers;
            if for_set {
                o.adding = Some(draft);
            } else if o.drafts.len() < DRAFT_MAX {
                o.drafts.push(draft);
            }
        }
    }

    pub(super) fn own_stickers_event(&mut self, ev: OwnEv) {
        let o = &mut self.own_stickers;
        match ev {
            OwnEv::Sets(sets) => o.sets = Some(sets),
            OwnEv::Set { set, created } => self.on_own_set(set, created),
            OwnEv::Deleted(id) => {
                if let Some(sets) = &mut o.sets {
                    sets.retain(|s| s.set.id != id);
                }
                if o.page == Page::Set(id) {
                    Self::go(o, Page::List);
                }
                if self.stickers.sets.iter().any(|s| s.id == id) {
                    self.on_set_changed(StickerSetInfo { id, title: String::new(), short_name: String::new(), count: 0, installed: false, stickers: Vec::new() });
                }
                self.toast = Some((l!("Набір видалено", "Set deleted").into(), 0.0, false));
            }
            OwnEv::NameChecked { name, problem } => {
                if o.short_name.trim() == name {
                    o.checked = Some((name, Some(problem)));
                }
            }
            OwnEv::NameSuggested { title, name } => {
                if !o.name_typed && o.title.trim() == title {
                    o.short_name = name.clone();
                    o.checked = Some((name, Some(None)));
                }
            }
            OwnEv::Progress { done, total } => o.busy = Some((done, total)),
            OwnEv::Failed(e) => {
                o.busy = None;
                o.error = Some(e);
            }
        }
    }

    fn on_own_set(&mut self, set: OwnSet, created: bool) {
        let o = &mut self.own_stickers;
        let (id, title) = (set.set.id, set.set.title.clone());
        let added = o.busy.is_some() && !created;
        o.busy = None;
        o.error = None;
        let sets = o.sets.get_or_insert_with(Vec::new);
        match sets.iter_mut().find(|s| s.set.id == id) {
            Some(s) => *s = set.clone(),
            None => sets.insert(0, set.clone()),
        }
        if created {
            o.title.clear();
            o.short_name.clear();
            o.name_typed = false;
            o.checked = None;
            o.suggested_for.clear();
            o.drafts.clear();
        }
        if created || added {
            o.from_msg = None;
            Self::go(o, Page::Set(id));
        }
        // The panel shows our installed sets too: keep its copy current, and add a new one.
        match self.stickers.sets.iter_mut().find(|s| s.id == id) {
            Some(s) => *s = StickerSetInfo { installed: s.installed, ..set.set.clone() },
            None if created && !set.emojis && set.set.installed => self.on_set_changed(set.set.clone()),
            None => {}
        }
        if created {
            self.toast = Some((lf!("Набір «{title}» створено", "Set “{title}” created"), 0.0, false));
        } else if added {
            self.toast = Some((lf!("Додано до «{title}»", "Added to “{title}”"), 0.0, false));
        }
    }

    /// The box over the window: the list of our sets, the new set form or one set.
    pub(super) fn own_stickers_box(&mut self, ui: &mut Ui) {
        if !self.own_stickers.open {
            return;
        }
        self.take_picked();
        let screen = ui.max_rect();
        ui.painter().rect_filled(screen, 0.0, Color32::from_black_alpha(90));
        ui.interact(screen, ui.scope_id().with("own-box-bg"), Sense::CLICK);
        let rect = Rect::from_center_size(screen.center(), vec2(screen.width().min(480.0) - 20.0, (screen.height() - 60.0).min(620.0)));
        glass::glass(ui.painter(), rect, Glass::panel(26.0).with_shadow());
        let inner = rect.shrink(18.0);
        let page = self.own_stickers.page;
        let close = Rect::from_center_size(pos2(inner.right() - 14.0, inner.top() + 14.0), vec2(32.0, 32.0));
        let back = Rect::from_center_size(pos2(inner.left() + 14.0, inner.top() + 14.0), vec2(32.0, 32.0));
        let has_back = page != Page::List && self.own_stickers.busy.is_none();
        if has_back && glass::icon_button(ui, back, "own-back", Icon::ChevronLeft, false, true).on_hover_text(l!("Назад", "Back")).clicked() {
            Self::go(&mut self.own_stickers, Page::List);
            return;
        }
        let title = match page {
            Page::List if self.own_stickers.from_msg.is_some() => l!("Додати до набору стікерів", "Add to sticker set").to_owned(),
            Page::List => l!("Ваші набори стікерів", "Your sticker sets").to_owned(),
            Page::Create => l!("Новий набір", "New set").to_owned(),
            Page::Set(id) => self.own_set(id).map(|s| s.set.title.clone()).unwrap_or_default(),
        };
        let tx = if has_back { back.right() + 8.0 } else { inner.left() };
        let title_rect = Rect::from_min_max(pos2(tx, inner.top()), pos2(close.left() - 8.0, inner.top() + 28.0));
        a11y::title(ui, title_rect, "own-box", &title);
        ui.painter().galley(pos2(tx, inner.top() + 4.0), elided(ui, &title, 17.0, ink(), title_rect.width()), ink());
        // Esc closes only when no question is open over the box.
        let esc = !self.own_stickers.confirm_delete && ui.input_mut(|i| i.consume_key(egui::Modifiers::NONE, Key::Escape));
        let closed = glass::icon_button(ui, close, "own-close", Icon::Close, false, true).clicked();
        if (esc || closed) && self.own_stickers.busy.is_none() {
            self.own_stickers.open = false;
            return;
        }
        let body = Rect::from_min_max(pos2(inner.left(), inner.top() + 44.0), inner.max);
        match page {
            Page::List => self.own_list_page(ui, body),
            Page::Create => self.own_create_page(ui, body),
            Page::Set(id) => self.own_set_page(ui, body, id),
        }
    }

    fn own_set(&self, id: i64) -> Option<&OwnSet> {
        self.own_stickers.sets.as_ref()?.iter().find(|s| s.set.id == id)
    }

    fn own_list_page(&mut self, ui: &mut Ui, body: Rect) {
        let mut top = body.top();
        let from_msg = self.own_stickers.from_msg.is_some();
        if let Some((_, _, emoji)) = &mut self.own_stickers.from_msg {
            let hint = l!("Виберіть набір і емодзі для стікера", "Choose a set and an emoji for the sticker");
            ui.painter().text(pos2(body.left() + 4.0, top + 9.0), Align2::LEFT_CENTER, hint, FontId::proportional(13.0), ink_dim());
            glass::text_field(ui, Rect::from_min_size(pos2(body.right() - 70.0, top), vec2(70.0, 30.0)), emoji, DEFAULT_EMOJI, false);
            top += 40.0;
        } else {
            let row = Rect::from_min_size(pos2(body.left(), top), vec2(body.width(), 44.0));
            let resp = ui.interact(row, ui.scope_id().with("own-new"), Sense::click());
            if resp.hovered() {
                ui.painter().rect_filled(row, 12.0, glass::veil(12));
            }
            icons::draw(ui.painter(), Rect::from_center_size(pos2(row.left() + 26.0, row.center().y), vec2(22.0, 22.0)), Icon::Plus, ACCENT);
            ui.painter().text(pos2(row.left() + 56.0, row.center().y), Align2::LEFT_CENTER, l!("Створити новий набір стікерів", "Create a new sticker set"), FontId::proportional(14.5), ink());
            if resp.on_hover_cursor(egui::CursorIcon::PointingHand).clicked() {
                Self::go(&mut self.own_stickers, Page::Create);
            }
            top += 52.0;
        }
        let list = Rect::from_min_max(pos2(body.left(), top), body.max);
        let Some(sets) = self.own_stickers.sets.clone() else {
            ui.put(Rect::from_center_size(list.center(), vec2(32.0, 32.0)), egui::Spinner::new().color(ink_dim()));
            return;
        };
        if let Some((done, total)) = self.own_stickers.busy {
            let text = lf!("Додавання… {done}/{total}", "Adding… {done}/{total}");
            ui.painter().text(list.center(), Align2::CENTER_CENTER, text, FontId::proportional(14.0), ink_dim());
            return;
        }
        if let Some(e) = &self.own_stickers.error {
            ui.painter().text(pos2(list.left() + 4.0, list.top()), Align2::LEFT_TOP, e, FontId::proportional(12.5), glass::danger());
        }
        if sets.is_empty() {
            ui.painter().text(list.center(), Align2::CENTER_CENTER, l!("Ви ще не створили жодного набору", "You haven't made any sets yet"), FontId::proportional(14.0), ink_dim());
            return;
        }
        self.want_sticker_thumbs(sets.iter().filter_map(|s| s.set.stickers.first().map(|x| x.0)));
        let mut chosen = None;
        let mut child = ui.new_child(UiBuilder::new().max_rect(list));
        child.set_clip_rect(list);
        egui::ScrollArea::vertical().id_salt("own-sets").auto_shrink(false).show(&mut child, |ui| {
            for s in &sets {
                let (row, resp) = ui.allocate_exact_size(vec2(list.width(), 56.0), Sense::click());
                if resp.hovered() {
                    ui.painter().rect_filled(row, 12.0, glass::veil(12));
                }
                let thumb = Rect::from_min_size(pos2(row.left() + 6.0, row.top() + 8.0), vec2(40.0, 40.0));
                match s.set.stickers.first() {
                    Some((id, emoji)) => self.own_sticker(ui.painter(), thumb, *id, emoji),
                    None => {
                        ui.painter().text(thumb.center(), Align2::CENTER_CENTER, glass::initials(&s.set.title), FontId::proportional(15.0), ink());
                    }
                }
                ui.painter().galley(pos2(thumb.right() + 10.0, row.top() + 10.0), elided(ui, &s.set.title, 14.5, ink(), row.right() - thumb.right() - 20.0), ink());
                ui.painter().text(pos2(thumb.right() + 10.0, row.top() + 31.0), Align2::LEFT_TOP, count_label(s.set.count, s.emojis), FontId::proportional(12.5), ink_dim());
                if resp.on_hover_cursor(egui::CursorIcon::PointingHand).clicked() {
                    chosen = Some((s.set.id, s.emojis));
                }
            }
        });
        let Some((id, emojis)) = chosen else { return };
        match self.own_stickers.from_msg.clone() {
            Some((peer, msg, emoji)) if from_msg => {
                let emoji = if emoji.trim().is_empty() { DEFAULT_EMOJI.to_owned() } else { emoji.trim().to_owned() };
                self.own_stickers.busy = Some((0, 1));
                self.own_stickers.error = None;
                self.send(Cmd::OwnStickers(OwnOp::AddFromMsg { set: id, emojis, peer, msg, emoji }));
            }
            _ => {
                Self::go(&mut self.own_stickers, Page::Set(id));
                self.send(Cmd::OwnStickers(OwnOp::Open(id)));
            }
        }
    }

    /// A sticker's picture, or its emoji while the picture loads.
    fn own_sticker(&self, p: &egui::Painter, r: Rect, id: i64, emoji: &str) {
        match self.textures.get(&ImgKey::Sticker(id)) {
            Some(t) => glass::image_contain(p, r, t),
            None => {
                p.text(r.center(), Align2::CENTER_CENTER, emoji, FontId::proportional(r.height() * 0.55), ink());
            }
        }
    }

    /// The new set form: title, link, type, files, Create.
    fn own_create_page(&mut self, ui: &mut Ui, body: Rect) {
        let now = ui.input(|i| i.time);
        let busy = self.own_stickers.busy;
        let o = &mut self.own_stickers;
        let label = |ui: &Ui, y: f32, text: &str| {
            ui.painter().text(pos2(body.left() + 4.0, y), Align2::LEFT_TOP, text, FontId::proportional(12.5), ink_dim());
        };
        let mut y = body.top();
        label(ui, y, l!("Назва", "Name"));
        let title_r = Rect::from_min_size(pos2(body.left(), y + 18.0), vec2(body.width(), 36.0));
        if glass::text_field(ui, title_r, &mut o.title, l!("Назва набору", "Set name"), false).changed() {
            o.title = o.title.chars().take(TITLE_MAX).collect();
            o.typed_at = now;
        }
        y = title_r.bottom() + 10.0;
        label(ui, y, l!("Посилання", "Link"));
        let prefix = "t.me/addstickers/";
        let pg = ui.painter().layout_no_wrap(prefix.into(), FontId::proportional(14.0), ink_dim());
        let link_r = Rect::from_min_size(pos2(body.left(), y + 18.0), vec2(body.width(), 36.0));
        ui.painter().galley(pos2(link_r.left() + 12.0, link_r.center().y - pg.size().y / 2.0), pg.clone(), ink_dim());
        let field = Rect::from_min_max(pos2(link_r.left() + pg.size().x + 4.0, link_r.top()), link_r.max);
        if glass::text_field(ui, field, &mut o.short_name, l!("назва", "name"), false).changed() {
            o.name_typed = !o.short_name.is_empty();
            o.typed_at = now;
        }
        y = link_r.bottom() + 4.0;
        let (status, color) = self.own_name_status(ui, now);
        ui.painter().text(pos2(body.left() + 4.0, y), Align2::LEFT_TOP, status, FontId::proportional(12.0), color);
        y += 24.0;
        let o = &mut self.own_stickers;
        let half = (body.width() - 8.0) / 2.0;
        for (k, (emojis, text)) in [(false, l!("Стікери", "Stickers")), (true, l!("Власні емодзі", "Custom emoji"))].into_iter().enumerate() {
            let r = Rect::from_min_size(pos2(body.left() + k as f32 * (half + 8.0), y), vec2(half, 32.0));
            if glass::button(ui, r, ("own-kind", k), text, o.emojis == emojis, busy.is_none()).clicked() {
                o.emojis = emojis;
            }
        }
        y += 42.0;
        let n = o.drafts.len();
        label(ui, y, &lf!("Файли: {n}/{DRAFT_MAX}", "Files: {n}/{DRAFT_MAX}"));
        let footer = Rect::from_min_max(pos2(body.left(), body.bottom() - 38.0), body.max);
        let grid = Rect::from_min_max(pos2(body.left(), y + 20.0), pos2(body.right(), footer.top() - 26.0));
        self.own_drafts_grid(ui, grid, busy.is_none());
        let o = &self.own_stickers;
        if let Some(e) = &o.error {
            ui.painter().galley(pos2(body.left() + 4.0, grid.bottom() + 4.0), elided(ui, e, 12.5, glass::danger(), body.width() - 8.0), glass::danger());
        }
        let name_free = o.checked.as_ref().is_some_and(|(name, ans)| *name == o.short_name.trim() && matches!(ans, Some(None)));
        let ready = !o.title.trim().is_empty() && name_free && !o.drafts.is_empty() && o.drafts.iter().all(|d| !d.emoji.trim().is_empty());
        let text = match busy {
            Some((done, total)) => lf!("Завантаження… {done}/{total}", "Uploading… {done}/{total}"),
            None => l!("Створити", "Create").to_owned(),
        };
        if glass::button(ui, footer, "own-create-go", &text, true, ready && busy.is_none()).clicked() {
            let o = &mut self.own_stickers;
            let stickers = o.drafts.iter().map(|d| NewSticker { path: d.path.clone(), emoji: d.emoji.trim().to_owned() }).collect();
            let op = OwnOp::Create { title: o.title.trim().to_owned(), short_name: o.short_name.trim().to_owned(), emojis: o.emojis, stickers };
            o.busy = Some((0, o.drafts.len()));
            o.error = None;
            self.send(Cmd::OwnStickers(op));
        }
    }

    /// Asks for a link name made from the title, or checks a typed one, after a pause in typing;
    /// returns the line shown under the field.
    fn own_name_status(&mut self, ui: &Ui, now: f64) -> (String, Color32) {
        let o = &mut self.own_stickers;
        let (title, name) = (o.title.trim().to_owned(), o.short_name.trim().to_owned());
        let settled = now - o.typed_at >= NAME_DELAY;
        if !settled {
            ui.ctx().request_repaint_after(std::time::Duration::from_secs_f64(NAME_DELAY));
        }
        if settled && !o.name_typed && !title.is_empty() && o.suggested_for != title {
            o.suggested_for = title.clone();
            self.send(Cmd::OwnStickers(OwnOp::SuggestName(title)));
        } else if settled && !name.is_empty() && o.checked.as_ref().is_none_or(|c| c.0 != name) {
            if short_name_ok(&name) {
                o.checked = Some((name.clone(), None));
                self.send(Cmd::OwnStickers(OwnOp::CheckName(name.clone())));
            } else {
                let why = l!("Лише латинські літери, цифри й підкреслення, першою — літера", "Only Latin letters, digits and underscores, starting with a letter");
                o.checked = Some((name.clone(), Some(Some(why.into()))));
            }
        }
        let o = &self.own_stickers;
        match o.checked.as_ref().filter(|c| c.0 == name) {
            _ if name.is_empty() => (l!("Посилання, за яким можна поділитися набором", "The link to share the set").into(), ink_dim()),
            Some((_, Some(None))) => (lf!("t.me/addstickers/{name} вільне", "t.me/addstickers/{name} is available"), glass::positive()),
            Some((_, Some(Some(why)))) => (why.clone(), glass::danger()),
            _ => (l!("Перевірка…", "Checking…").into(), ink_dim()),
        }
    }

    /// Picked files with their emoji, a cross to drop one, and the tile that picks more.
    fn own_drafts_grid(&mut self, ui: &mut Ui, grid: Rect, enabled: bool) {
        let mut child = ui.new_child(UiBuilder::new().max_rect(grid));
        child.set_clip_rect(grid);
        let mut removed = None;
        let mut pick = false;
        let full = self.own_stickers.drafts.len() >= DRAFT_MAX;
        egui::ScrollArea::vertical().id_salt("own-drafts").auto_shrink(false).show(&mut child, |ui| {
            let cols = ((grid.width() / (CELL + 6.0)).floor() as usize).max(1);
            let drafts = &mut self.own_stickers.drafts;
            let n = drafts.len() + usize::from(!full);
            for row in 0..n.div_ceil(cols) {
                ui.horizontal(|ui| {
                    ui.spacing_mut().item_spacing.x = 6.0;
                    for i in row * cols..((row + 1) * cols).min(n) {
                        let (cell, resp) = ui.allocate_exact_size(vec2(CELL, CELL + 34.0), Sense::click());
                        let pic = Rect::from_min_size(cell.min, vec2(CELL, CELL));
                        ui.painter().rect_filled(pic, 14.0, glass::veil(if resp.hovered() { 22 } else { 12 }));
                        let Some(d) = drafts.get_mut(i) else {
                            icons::draw(ui.painter(), Rect::from_center_size(pic.center(), vec2(26.0, 26.0)), Icon::Plus, ink_dim());
                            if enabled && resp.on_hover_text(l!("Додати файли", "Add files")).clicked() {
                                pick = true;
                            }
                            continue;
                        };
                        draft_picture(ui.painter(), pic.shrink(6.0), d);
                        let x = Rect::from_center_size(pic.right_top() + vec2(-12.0, 12.0), vec2(24.0, 24.0));
                        if enabled && glass::icon_button(ui, x, ("own-draft-x", i), Icon::Close, false, true).on_hover_text(l!("Прибрати", "Remove")).clicked() {
                            removed = Some(i);
                        }
                        let field = Rect::from_min_size(pos2(cell.left(), pic.bottom() + 4.0), vec2(CELL, 28.0));
                        ui.push_id(("own-draft-emoji", i), |ui| glass::text_field(ui, field, &mut d.emoji, DEFAULT_EMOJI, false));
                    }
                });
                ui.add_space(6.0);
            }
        });
        if let Some(i) = removed {
            self.own_stickers.drafts.remove(i);
        }
        if pick {
            self.pick_sticker_files(false);
        }
    }

    /// One of our sets: its stickers to edit, adding, renaming, the link, deleting.
    fn own_set_page(&mut self, ui: &mut Ui, body: Rect, id: i64) {
        let Some(set) = self.own_set(id).cloned() else {
            ui.put(Rect::from_center_size(body.center(), vec2(32.0, 32.0)), egui::Spinner::new().color(ink_dim()));
            return;
        };
        let busy = self.own_stickers.busy.is_some();
        let side = 32.0;
        let btn = |n: f32| Rect::from_min_size(pos2(body.right() - (side + 4.0) * n + 4.0, body.top()), vec2(side, side));
        ui.painter().text(pos2(body.left() + 4.0, body.top() + side / 2.0), Align2::LEFT_CENTER, count_label(set.set.count, set.emojis), FontId::proportional(13.0), ink_dim());
        if glass::icon_button(ui, btn(3.0), "own-rename", Icon::Pencil, false, !busy).on_hover_text(l!("Перейменувати", "Rename")).clicked() {
            self.own_stickers.rename = Some(set.set.title.clone());
        }
        if glass::icon_button(ui, btn(2.0), "own-link", Icon::Link, false, true).on_hover_text(l!("Копіювати посилання", "Copy link")).clicked() {
            self.ctx.copy_text(format!("https://t.me/addstickers/{}", set.set.short_name));
            self.toast = Some((l!("Посилання скопійовано", "Link copied").into(), 0.0, false));
        }
        if glass::icon_button(ui, btn(1.0), "own-delete", Icon::Trash, false, !busy).on_hover_text(l!("Видалити набір", "Delete set")).clicked() {
            self.own_stickers.confirm_delete = true;
        }
        let footer_h = if self.own_stickers.rename.is_some() || self.own_stickers.emoji_edit.is_some() || self.own_stickers.adding.is_some() || busy { 84.0 } else { 22.0 };
        let grid = Rect::from_min_max(pos2(body.left(), body.top() + side + 10.0), pos2(body.right(), body.bottom() - footer_h));
        self.want_sticker_thumbs(set.set.stickers.iter().map(|x| x.0));
        let max = if set.emojis { OWN_EMOJI_MAX } else { OWN_STICKERS_MAX };
        let edit = self.own_set_grid(ui, grid, &set, !busy && set.set.stickers.len() < max);
        let footer = Rect::from_min_max(pos2(body.left(), grid.bottom() + 8.0), body.max);
        self.own_set_footer(ui, footer, &set);
        match edit {
            Some(SetEdit::Pick) => self.pick_sticker_files(true),
            Some(SetEdit::Emoji(doc, emoji)) => self.own_stickers.emoji_edit = Some((doc, emoji)),
            Some(SetEdit::Move(doc, position)) => self.send(Cmd::OwnStickers(OwnOp::Move { set: id, doc, position })),
            Some(SetEdit::Remove(doc)) => self.send(Cmd::OwnStickers(OwnOp::Remove { set: id, doc })),
            None => {}
        }
        if self.own_stickers.confirm_delete {
            let title = l!("Видалити набір?", "Delete set?");
            let body_text = l!("Набір буде видалено для всіх, хто його додав. Цю дію не можна скасувати.", "The set will be deleted for everyone who added it. This can't be undone.");
            match confirm::ask_box(ui, "own-delete-ask", title, body_text, None, &[l!("Скасувати", "Cancel"), l!("Видалити", "Delete")]) {
                Some(1) => {
                    self.own_stickers.confirm_delete = false;
                    self.send(Cmd::OwnStickers(OwnOp::Delete(id)));
                }
                Some(_) => self.own_stickers.confirm_delete = false,
                None => {}
            }
        }
    }

    /// The set's stickers: a right click changes the emoji, moves or deletes; the last tile adds one.
    fn own_set_grid(&self, ui: &mut Ui, grid: Rect, set: &OwnSet, can_add: bool) -> Option<SetEdit> {
        let mut edit = None;
        let mut child = ui.new_child(UiBuilder::new().max_rect(grid));
        child.set_clip_rect(grid);
        let stickers = &set.set.stickers;
        egui::ScrollArea::vertical().id_salt(("own-set-grid", set.set.id)).auto_shrink(false).show(&mut child, |ui| {
            let cols = ((grid.width() / 76.0).floor() as usize).max(1);
            let cell = grid.width() / cols as f32 - 4.0;
            let n = stickers.len() + usize::from(can_add);
            for row in 0..n.div_ceil(cols) {
                ui.horizontal(|ui| {
                    ui.spacing_mut().item_spacing.x = 4.0;
                    for i in row * cols..((row + 1) * cols).min(n) {
                        let (r, resp) = ui.allocate_exact_size(vec2(cell, cell), Sense::click());
                        if resp.hovered() {
                            ui.painter().rect_filled(r, 14.0, glass::veil(22));
                        }
                        let Some((doc, emoji)) = stickers.get(i) else {
                            ui.painter().rect_filled(r.shrink(6.0), 12.0, glass::veil(12));
                            icons::draw(ui.painter(), Rect::from_center_size(r.center(), vec2(24.0, 24.0)), Icon::Plus, ink_dim());
                            if resp.on_hover_text(l!("Додати стікер", "Add sticker")).clicked() {
                                edit = Some(SetEdit::Pick);
                            }
                            continue;
                        };
                        self.own_sticker(ui.painter(), r.shrink(6.0), *doc, emoji);
                        let resp = resp.on_hover_text(emoji);
                        let last = i + 1 == stickers.len();
                        resp.context_menu(|ui| {
                            if glass::menu_button(ui, Icon::Smile, l!("Змінити емодзі", "Change emoji")).clicked() {
                                edit = Some(SetEdit::Emoji(*doc, emoji.clone()));
                            }
                            if i > 0 && glass::menu_button(ui, Icon::ChevronLeft, l!("Перемістити ліворуч", "Move left")).clicked() {
                                edit = Some(SetEdit::Move(*doc, i as i32 - 1));
                            }
                            if !last && glass::menu_button(ui, Icon::ChevronRight, l!("Перемістити праворуч", "Move right")).clicked() {
                                edit = Some(SetEdit::Move(*doc, i as i32 + 1));
                            }
                            if glass::menu_button(ui, Icon::Trash, l!("Видалити з набору", "Remove from set")).clicked() {
                                edit = Some(SetEdit::Remove(*doc));
                            }
                        });
                    }
                });
                ui.add_space(4.0);
            }
        });
        edit
    }

    /// Under the grid: renaming, a new emoji, a picked file waiting for its emoji, or progress.
    fn own_set_footer(&mut self, ui: &mut Ui, footer: Rect, set: &OwnSet) {
        let id = set.set.id;
        let o = &mut self.own_stickers;
        let row = Rect::from_min_size(pos2(footer.left(), footer.top() + 18.0), vec2(footer.width(), 36.0));
        let ok = Rect::from_min_size(pos2(row.right() - 100.0, row.top()), vec2(100.0, 36.0));
        let field = Rect::from_min_max(row.min, pos2(ok.left() - 8.0, row.bottom()));
        let caption = |ui: &Ui, text: &str, color: Color32| {
            ui.painter().galley(footer.min + vec2(4.0, 0.0), elided(ui, text, 12.5, color, footer.width() - 8.0), color);
        };
        if let Some((done, total)) = o.busy {
            caption(ui, &lf!("Завантаження… {done}/{total}", "Uploading… {done}/{total}"), ink_dim());
        } else if let Some(e) = o.error.clone() {
            caption(ui, &e, glass::danger());
        }
        if o.busy.is_some() {
            return;
        }
        if let Some(title) = &mut o.rename {
            if o.error.is_none() {
                caption(ui, l!("Нова назва набору", "New set name"), ink_dim());
            }
            glass::text_field(ui, field, title, l!("Назва набору", "Set name"), false);
            let title = title.trim().chars().take(TITLE_MAX).collect::<String>();
            if glass::button(ui, ok, "own-rename-ok", l!("Зберегти", "Save"), true, !title.is_empty()).clicked() {
                o.rename = None;
                self.send(Cmd::OwnStickers(OwnOp::Rename { set: id, title }));
            }
        } else if let Some((doc, emoji)) = &mut o.emoji_edit {
            if o.error.is_none() {
                caption(ui, l!("Новий емодзі для стікера", "New emoji for the sticker"), ink_dim());
            }
            glass::text_field(ui, field, emoji, DEFAULT_EMOJI, false);
            let (doc, emoji) = (*doc, emoji.trim().to_owned());
            if glass::button(ui, ok, "own-emoji-ok", l!("Зберегти", "Save"), true, !emoji.is_empty()).clicked() {
                o.emoji_edit = None;
                self.send(Cmd::OwnStickers(OwnOp::ChangeEmoji { set: id, doc, emoji }));
            }
        } else if let Some(d) = &mut o.adding {
            let pic = Rect::from_min_size(row.min, vec2(36.0, 36.0));
            draft_picture(ui.painter(), pic, d);
            if o.error.is_none() {
                caption(ui, l!("Емодзі для нового стікера", "An emoji for the new sticker"), ink_dim());
            }
            glass::text_field(ui, Rect::from_min_max(pos2(pic.right() + 8.0, row.top()), field.max), &mut d.emoji, DEFAULT_EMOJI, false);
            let emoji = d.emoji.trim().to_owned();
            if glass::button(ui, ok, "own-add-ok", l!("Додати", "Add"), true, !emoji.is_empty()).clicked() {
                let sticker = NewSticker { path: d.path.clone(), emoji };
                o.adding = None;
                o.busy = Some((0, 1));
                o.error = None;
                self.send(Cmd::OwnStickers(OwnOp::Add { set: id, emojis: set.emojis, sticker }));
            }
        } else if o.error.is_none() {
            caption(ui, l!("Клацніть правою кнопкою на стікері, щоб змінити його", "Right-click a sticker to change it"), ink_dim());
        }
    }
}

/// A picked file's preview, or a video icon and its name.
fn draft_picture(p: &egui::Painter, r: Rect, d: &Draft) {
    match &d.tex {
        Some(t) => glass::image_contain(p, r, t),
        None => icons::draw(p, Rect::from_center_size(r.center(), vec2(24.0, 24.0)), Icon::Video, ink_dim()),
    }
}

/// What the set's grid asked for.
enum SetEdit {
    Pick,
    Emoji(i64, String),
    /// (sticker, new position)
    Move(i64, i32),
    Remove(i64),
}

