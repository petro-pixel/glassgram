//! "Translate" from the message menu, as in Telegram Desktop: a window with the original,
//! the translation and a choice of target language. And translating a whole chat: the bar under
//! the header for chats in another language, translations in place of the text (Premium).

use super::*;
use crate::backend::{TRANSLATE_BATCH, TRANSLATE_CHARS};

mod detect;

/// Height of the translate bar under the header.
const BAR_H: f32 = 40.0;
/// Incoming messages, from the newest, the chat's language is guessed from.
const SAMPLE: usize = 40;

/// Target languages offered in the window: code and the name in that language.
const LANGS: [(&str, &str); 8] = [
    ("uk", "Українська"),
    ("en", "English"),
    ("de", "Deutsch"),
    ("pl", "Polski"),
    ("fr", "Français"),
    ("es", "Español"),
    ("it", "Italiano"),
    ("pt", "Português"),
];

pub(super) struct TranslateView {
    peer: PeerId,
    id: i32,
    to: String,
    /// `None` — waiting for the server.
    result: Option<Result<(String, Vec<format::Span>), String>>,
}

impl App {
    pub(super) fn open_translate(&mut self, peer: PeerId, id: i32) {
        // The last chosen language, or the interface language.
        let to = self.translate_to.clone().unwrap_or_else(|| self.settings.lang.shown_code());
        self.translate = Some(TranslateView { peer, id, to: to.clone(), result: None });
        self.send(Cmd::Translate { peer, id, to });
    }

    pub(super) fn translated(&mut self, peer: PeerId, id: i32, to: String, result: Result<(String, Vec<format::Span>), String>) {
        if let Some(t) = &mut self.translate
            && t.peer == peer
            && t.id == id
            && t.to == to
        {
            t.result = Some(result);
        }
    }

    pub(super) fn translate_view(&mut self, ui: &mut Ui, peer: PeerId, rect: Rect) {
        let Some(t) = &self.translate else { return };
        if t.peer != peer || ui.input_mut(|i| i.consume_key(egui::Modifiers::NONE, Key::Escape)) {
            self.translate = None;
            return;
        }
        let (id, to) = (t.id, t.to.clone());
        let original = self.find(peer, id).map(|m| (m.text.clone(), m.spans.clone()));
        let protected = self.protected(peer, &[id]);
        let panel = Rect::from_center_size(rect.center(), vec2(rect.width().min(440.0), (rect.height() - 100.0).min(520.0)));
        glass::glass(ui.painter(), panel, Glass::panel(24.0).with_shadow());
        let p = ui.painter().clone();
        p.text(pos2(panel.left() + 18.0, panel.top() + 26.0), Align2::LEFT_CENTER, l!("Переклад", "Translation"), FontId::proportional(16.0), ink());
        let close = Rect::from_center_size(pos2(panel.right() - 26.0, panel.top() + 26.0), vec2(32.0, 32.0));
        if glass::icon_button(ui, close, "tr-close", Icon::Close, false, true).clicked() {
            self.translate = None;
            return;
        }

        let inner = panel.shrink2(vec2(18.0, 0.0));
        let mut child = ui.new_child(UiBuilder::new().max_rect(Rect::from_min_max(pos2(inner.left(), panel.top() + 52.0), pos2(inner.right(), panel.bottom() - 60.0))));
        let mut pick = None;
        egui::ScrollArea::vertical().id_salt("translate").auto_shrink(false).show(&mut child, |ui| {
            let w = ui.available_width();
            if let Some((text, spans)) = &original {
                ui.label(egui::RichText::new(l!("Оригінал", "Original")).size(12.5).color(ink_dim()));
                let job = format::layout(text, spans, 14.0, ink_dim(), glass::link(false), w, true);
                ui.label(job);
                ui.add_space(12.0);
            }
            // Target language: capsules, the chosen one accented.
            ui.horizontal_wrapped(|ui| {
                ui.spacing_mut().item_spacing = vec2(6.0, 6.0);
                for (code, name) in LANGS {
                    let (r, _) = ui.allocate_exact_size(vec2(96.0, 30.0), Sense::hover());
                    if glass::button(ui, r, ("tr-lang", code), name, code == to, true).clicked() && code != to {
                        pick = Some(code.to_owned());
                    }
                }
            });
            ui.add_space(12.0);
            match self.translate.as_ref().and_then(|t| t.result.as_ref()) {
                None => {
                    ui.add_space(12.0);
                    ui.vertical_centered(|ui| ui.add(egui::Spinner::new().color(ink())));
                }
                Some(Ok((text, spans))) => {
                    let job = format::layout(text, spans, glass::msg_font(), ink(), glass::link(false), w, true);
                    ui.label(job);
                }
                Some(Err(e)) => {
                    ui.label(egui::RichText::new(e).size(14.0).color(glass::danger()));
                }
            }
        });

        let done = self.translate.as_ref().and_then(|t| t.result.as_ref()).and_then(|r| r.as_ref().ok()).map(|(text, _)| text.clone());
        let b = Rect::from_min_max(pos2(inner.left(), panel.bottom() - 52.0), pos2(inner.right(), panel.bottom() - 14.0));
        if !protected && glass::button(ui, b, "tr-copy", l!("Копіювати переклад", "Copy translation"), false, done.is_some()).clicked() {
            if let Some(text) = done {
                self.ctx.copy_text(text);
                self.toast = Some((l!("Скопійовано", "Copied").into(), 0.0, false));
            }
        }
        if let Some(code) = pick {
            self.translate_to = Some(code.clone());
            self.translate = Some(TranslateView { peer, id, to: code.clone(), result: None });
            self.send(Cmd::Translate { peer, id, to: code });
        }
    }
}

/// "Translate to German": the phrase on the bar for a target language.
fn to_phrase(code: &str) -> String {
    match code {
        "uk" => l!("Перекласти українською", "Translate to Ukrainian").into(),
        "en" => l!("Перекласти англійською", "Translate to English").into(),
        "de" => l!("Перекласти німецькою", "Translate to German").into(),
        "pl" => l!("Перекласти польською", "Translate to Polish").into(),
        "fr" => l!("Перекласти французькою", "Translate to French").into(),
        "es" => l!("Перекласти іспанською", "Translate to Spanish").into(),
        "it" => l!("Перекласти італійською", "Translate to Italian").into(),
        "pt" => l!("Перекласти португальською", "Translate to Portuguese").into(),
        "ru" => l!("Перекласти російською", "Translate to Russian").into(),
        other => lf!("Перекласти: {other}", "Translate to: {other}"),
    }
}

/// "Don't translate German": the menu item for the chat's language.
fn skip_phrase(code: &str) -> String {
    match code {
        "uk" => l!("Не перекладати з української", "Don't translate Ukrainian").into(),
        "en" => l!("Не перекладати з англійської", "Don't translate English").into(),
        "de" => l!("Не перекладати з німецької", "Don't translate German").into(),
        "pl" => l!("Не перекладати з польської", "Don't translate Polish").into(),
        "fr" => l!("Не перекладати з французької", "Don't translate French").into(),
        "es" => l!("Не перекладати з іспанської", "Don't translate Spanish").into(),
        "it" => l!("Не перекладати з італійської", "Don't translate Italian").into(),
        "pt" => l!("Не перекладати з португальської", "Don't translate Portuguese").into(),
        "ru" => l!("Не перекладати з російської", "Don't translate Russian").into(),
        other => lf!("Не перекладати: {other}", "Don't translate: {other}"),
    }
}

/// One chat translated as a whole; kept after "Show original", so switching back is instant.
struct ChatState {
    to: String,
    /// Shown in place of the originals right now.
    on: bool,
    texts: HashMap<i32, (String, Vec<format::Span>)>,
    /// Asked already (answered, waiting or failed): never asked twice.
    asked: HashSet<i32>,
}

impl ChatState {
    fn new(to: String) -> Self {
        Self { to, on: false, texts: HashMap::new(), asked: HashSet::new() }
    }
}

#[derive(Default)]
pub(super) struct ChatTranslate {
    /// Chats whose full info was asked this session.
    asked: HashSet<PeerId>,
    /// Chats whose full info allows the bar (`translations_disabled` unset).
    offered: HashSet<PeerId>,
    /// The guessed language with the (count, newest id) of the history it was guessed from.
    lang: HashMap<PeerId, ((usize, i32), Option<&'static str>)>,
    chats: HashMap<PeerId, ChatState>,
    /// Messages drawn this frame that wait for a translation; drawing can't send.
    wanted: RefCell<Vec<i32>>,
}

impl App {
    /// The language whole chats are translated into: the last chosen one, or the interface language.
    fn chat_translate_to(&self) -> String {
        self.translate_to.clone().unwrap_or_else(|| self.settings.lang.shown_code())
    }

    /// The chat's language, guessed again only when its history changes.
    fn chat_lang(&mut self, peer: PeerId) -> Option<&'static str> {
        let list = self.history.get(&peer)?;
        let key = (list.len(), list.last().map_or(0, |m| m.id));
        if let Some((k, lang)) = self.chat_translate.lang.get(&peer)
            && *k == key
        {
            return *lang;
        }
        let texts = list.iter().rev().filter(|m| !m.out && !m.text.is_empty()).take(SAMPLE).map(|m| m.text.as_str());
        let lang = detect::majority(texts);
        self.chat_translate.lang.insert(peer, (key, lang));
        lang
    }

    /// Room the translate bar takes under the header. The full info is asked the first time a chat opens.
    pub(super) fn translate_bar_h(&mut self, peer: PeerId) -> f32 {
        let Some(chat) = self.chats.iter().find(|c| c.id == peer) else { return 0.0 };
        if chat.kind == ChatKind::Saved {
            return 0.0;
        }
        if self.chat_translate.chats.get(&peer).is_some_and(|c| c.on) {
            return BAR_H + 8.0;
        }
        if self.chat_translate.asked.insert(peer) {
            self.send(Cmd::LoadTranslationOffer(peer));
        }
        if !self.chat_translate.offered.contains(&peer) {
            return 0.0;
        }
        let to = self.chat_translate_to();
        match self.chat_lang(peer) {
            Some(lang) if lang != to && !self.settings.skip_translate.iter().any(|s| s == lang) => BAR_H + 8.0,
            _ => 0.0,
        }
    }

    pub(super) fn chat_translate_event(&mut self, ev: Ev) {
        match ev {
            Ev::TranslationOffer { peer, offered } => {
                if !offered {
                    self.chat_translate.offered.remove(&peer);
                    return;
                }
                self.chat_translate.offered.insert(peer);
                // Premium decides what a click on the bar does; it comes with the own profile.
                if self.profile.id.is_none() {
                    self.send(Cmd::LoadProfile);
                }
            }
            Ev::TranslatedMany { peer, ids, to, result } => {
                let Some(state) = self.chat_translate.chats.get_mut(&peer).filter(|s| s.to == to) else { return };
                match result {
                    Ok(list) => state.texts.extend(list.into_iter().map(|(id, text, spans)| (id, (text, spans)))),
                    // Quota or an unsupported language: asking again now would only repeat it,
                    // so translation stops; turned on again, it retries these.
                    Err(e) => {
                        state.on = false;
                        for id in ids {
                            state.asked.remove(&id);
                        }
                        self.toast = Some((e, 0.0, true));
                    }
                }
            }
            _ => {}
        }
    }

    /// `m` with its translation in place of the text, when its chat is being translated and it has one.
    pub(super) fn translated_msg(&self, peer: PeerId, m: &Msg) -> Option<Msg> {
        let state = self.chat_translate.chats.get(&peer).filter(|s| s.on)?;
        let (text, spans) = state.texts.get(&m.id)?;
        Some(Msg { text: text.clone(), spans: spans.clone(), ..m.clone() })
    }

    /// Called for a drawn message: asks for its translation if it is on screen and still has none.
    pub(super) fn want_translation(&self, ui: &Ui, peer: PeerId, m: &Msg, top: f32) {
        let Some(state) = self.chat_translate.chats.get(&peer).filter(|s| s.on) else { return };
        // Our own messages are in our language; text with no letters needs no translation.
        if m.out || m.id <= 0 || !m.text.chars().any(char::is_alphabetic) || state.asked.contains(&m.id) {
            return;
        }
        let rect = Rect::from_x_y_ranges(ui.max_rect().x_range(), top..=ui.cursor().top().max(top + 1.0));
        if ui.is_rect_visible(rect) && detect::detect(&m.text) != Some(state.to.as_str()) {
            self.chat_translate.wanted.borrow_mut().push(m.id);
        }
    }

    /// Sends what this frame's messages wanted, in batches the server takes.
    pub(super) fn translate_wanted(&mut self, peer: PeerId) {
        let mut wanted = std::mem::take(&mut *self.chat_translate.wanted.borrow_mut());
        let Some(state) = self.chat_translate.chats.get_mut(&peer).filter(|s| s.on) else { return };
        wanted.sort_unstable();
        wanted.dedup();
        wanted.retain(|id| !state.asked.contains(id));
        if wanted.is_empty() {
            return;
        }
        let list = self.history.get(&peer);
        let len = |id: i32| list.and_then(|l| l.iter().find(|m| m.id == id)).map_or(0, |m| m.text.len());
        let mut batches = vec![Vec::new()];
        let mut chars = 0;
        for id in wanted {
            let n = len(id);
            let full = batches.last().is_some_and(|b: &Vec<i32>| !b.is_empty() && (b.len() == TRANSLATE_BATCH || chars + n > TRANSLATE_CHARS));
            if full {
                batches.push(Vec::new());
                chars = 0;
            }
            state.asked.insert(id);
            if let Some(b) = batches.last_mut() {
                b.push(id);
            }
            chars += n;
        }
        let to = state.to.clone();
        for ids in batches {
            self.send(Cmd::TranslateMany { peer, ids, to: to.clone() });
        }
    }

    /// Starts translating the chat into `to`, keeping what is already translated into it.
    /// "Translate chat" in the chat menu, as in Telegram Desktop: the only way back once the bar was
    /// hidden (on the server, for all our devices) or its language put on the "don't translate" list.
    pub(super) fn translate_chat(&mut self, peer: PeerId) {
        self.open_chat(peer);
        if self.chat_translate.offered.insert(peer) {
            self.send(Cmd::TogglePeerTranslations { peer, disabled: false });
        }
        if let Some(lang) = self.chat_lang(peer)
            && self.settings.skip_translate.iter().any(|s| s == lang)
        {
            self.settings.skip_translate.retain(|s| s != lang);
            self.settings.save();
        }
        let to = self.chat_translate_to();
        self.chat_translate_on(peer, to);
    }

    fn chat_translate_on(&mut self, peer: PeerId, to: String) {
        if !self.profile.premium {
            self.toast = Some((l!("Переклад цілих чатів доступний з Telegram Premium.", "Translating entire chats is available with Telegram Premium.").into(), 0.0, false));
            return;
        }
        let state = self.chat_translate.chats.entry(peer).or_insert_with(|| ChatState::new(to.clone()));
        if state.to != to {
            *state = ChatState::new(to);
        }
        state.on = true;
    }

    /// The bar: "Translate to …" or "Show original" in the middle, its menu on the right.
    pub(super) fn translate_bar(&mut self, ui: &mut Ui, peer: PeerId, rect: Rect) {
        let to = self.chat_translate_to();
        let on = self.chat_translate.chats.get(&peer).is_some_and(|c| c.on);
        let lang = self.chat_lang(peer);
        let p = ui.painter().clone();
        glass::glass(&p, rect, Glass::tinted(rect.height() / 2.0, glass::chrome(), 110).with_shadow().frosted(0.96));
        let menu_btn = Rect::from_center_size(pos2(rect.right() - 24.0, rect.center().y), vec2(34.0, 34.0));
        let label = if on { l!("Показати оригінал", "Show original").to_owned() } else { to_phrase(&to) };
        let body = Rect::from_min_max(pos2(rect.left() + 48.0, rect.top()), pos2(menu_btn.left() - 4.0, rect.bottom()));
        let g = elided(ui, &label, 14.0, ACCENT, body.width() - 26.0);
        let start = pos2(body.center().x - (g.size().x + 26.0) / 2.0, rect.center().y);
        icons::draw(&p, Rect::from_center_size(pos2(start.x + 9.0, start.y), vec2(18.0, 18.0)), Icon::Translate, ACCENT);
        p.galley(pos2(start.x + 26.0, start.y - g.size().y / 2.0), g, ACCENT);
        let resp = ui.interact(body, ui.scope_id().with(("translate-bar", peer)), Sense::click());
        if resp.hovered() {
            ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
        }
        if resp.clicked() {
            match self.chat_translate.chats.get_mut(&peer).filter(|_| on) {
                Some(s) => s.on = false,
                None => self.chat_translate_on(peer, to.clone()),
            }
        }

        let menu = glass::icon_button(ui, menu_btn, ("translate-menu", peer), Icon::Gear, false, true).on_hover_text(l!("Налаштування перекладу", "Translation settings"));
        // The usual targets plus the interface language, if it isn't one of them.
        let ui_lang = self.settings.lang.shown_code();
        let mut langs: Vec<(String, String)> = LANGS.iter().map(|(c, n)| (c.to_string(), n.to_string())).collect();
        if !langs.iter().any(|(c, _)| *c == ui_lang) {
            langs.insert(0, (ui_lang.clone(), crate::i18n::Lang::from_code(&ui_lang).name()));
        }
        let mut picked = None;
        let (mut skip, mut hide) = (false, false);
        egui::Popup::menu(&menu).align(egui::RectAlign::BOTTOM_END).gap(8.0).width(250.0).show(|ui| {
            ui.label(egui::RichText::new(l!("Перекладати на", "Translate to")).size(12.5).color(ink_dim()));
            for (code, name) in &langs {
                if glass::menu_button(ui, if *code == to { Icon::Check } else { Icon::Globe }, name).clicked() {
                    picked = Some(code.clone());
                }
            }
            ui.separator();
            if let Some(lang) = lang
                && glass::menu_button(ui, Icon::EyeOff, &skip_phrase(lang)).clicked()
            {
                skip = true;
            }
            if glass::menu_button(ui, Icon::Close, l!("Сховати панель", "Hide the bar")).clicked() {
                hide = true;
            }
        });
        if let Some(code) = picked {
            self.translate_to = Some(code.clone());
            self.chat_translate_on(peer, code);
        }
        if let Some(lang) = lang.filter(|_| skip) {
            self.settings.skip_translate.push(lang.to_owned());
            self.settings.save();
            self.chat_translate.chats.remove(&peer);
            self.toast = Some((l!("Цю мову більше не пропонуватимуть перекладати.", "This language will no longer be offered for translation.").into(), 0.0, false));
        }
        if hide {
            // The server keeps it for our other devices too, as in Telegram Desktop.
            self.chat_translate.offered.remove(&peer);
            self.chat_translate.chats.remove(&peer);
            self.send(Cmd::TogglePeerTranslations { peer, disabled: true });
            self.toast = Some((l!("Панель перекладу для цього чату сховано.", "The translation bar is now hidden for this chat.").into(), 0.0, false));
        }
    }
}
