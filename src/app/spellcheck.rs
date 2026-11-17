//! Spell checking in the message field, as in Telegram Desktop: misspelled words get a red wavy
//! underline (`glass::rich_area` draws it), a right click on one offers suggestions, "Add to
//! dictionary" and "Ignore"; Settings → Advanced → "Spell checker" turns it on and manages the
//! dictionaries. Dictionaries are read and downloaded on threads (`crate::spell`).

use std::ops::Range;
use std::sync::Arc;

use crate::spell::{self, Checker, Dict};

use super::*;
use super::settings_pages::hint;

/// What the threads send back.
enum Done {
    /// Download progress: bytes so far and the expected total.
    Progress(&'static str, u64, u64),
    /// A dictionary read from disk or downloaded.
    Dict(&'static str, Result<Arc<Dict>, String>),
    Suggestions(String, Vec<String>),
}

/// The right-click menu over a misspelled word.
struct WordMenu {
    /// The word's characters in the draft.
    range: Range<usize>,
    word: String,
    /// `None` while they are being made.
    suggestions: Option<Vec<String>>,
}

pub(super) struct SpellCheck {
    checker: Checker,
    tx: mpsc::Sender<Done>,
    rx: mpsc::Receiver<Done>,
    /// Dictionaries being read (`None`) or downloaded (bytes, total).
    busy: HashMap<&'static str, Option<(u64, u64)>>,
    /// Dictionaries that failed this run, with why; not retried on their own.
    failed: HashMap<&'static str, String>,
    /// The draft last checked and its misspelled words (character ranges).
    last: Option<(String, Vec<Range<usize>>)>,
    /// Every word of `last` was checked (a long paste takes a few frames).
    complete: bool,
    menu: Option<WordMenu>,
    /// The dictionary list under "Manage dictionaries" is open.
    manage: bool,
    /// `--demo`: dictionaries "downloaded" without the network.
    demo_ready: HashSet<&'static str>,
    personal_loaded: bool,
    /// Wanted dictionaries found missing on disk, so the composer doesn't look for them every frame.
    absent: HashSet<&'static str>,
}

impl Default for SpellCheck {
    fn default() -> Self {
        let (tx, rx) = mpsc::channel();
        Self { checker: Checker::default(), tx, rx, busy: HashMap::new(), failed: HashMap::new(), last: None, complete: false, menu: None, manage: false, demo_ready: HashSet::new(), personal_loaded: false, absent: HashSet::new() }
    }
}

impl SpellCheck {
    fn dict_changed(&mut self) {
        self.last = None;
    }
}

/// Up to this many new words are looked up per frame; the rest wait for the next one.
const WORDS_PER_FRAME: usize = 400;

impl App {
    /// Dictionaries the user wants: their own pick, or the defaults for the interface language.
    fn spell_langs(&self) -> Vec<String> {
        self.settings.spell_langs.clone().unwrap_or_else(|| spell::default_langs(&self.settings.lang.shown_code()))
    }

    fn spell_downloaded(&self, code: &str) -> bool {
        if self.demo { self.spell.demo_ready.contains(code) || code == "en_US" || code == "uk_UA" } else { spell::downloaded(code) }
    }

    /// Takes in what the threads finished.
    fn spell_poll(&mut self) {
        while let Ok(done) = self.spell.rx.try_recv() {
            let s = &mut self.spell;
            match done {
                Done::Progress(code, got, total) => {
                    s.busy.insert(code, Some((got, total)));
                }
                Done::Dict(code, Ok(d)) => {
                    s.busy.remove(code);
                    s.failed.remove(code);
                    let mut dicts: Vec<Arc<Dict>> = s.checker.dicts().iter().filter(|x| x.code != code).cloned().collect();
                    dicts.push(d);
                    s.checker.set_dicts(dicts);
                    s.dict_changed();
                }
                Done::Dict(code, Err(e)) => {
                    s.busy.remove(code);
                    crate::errlog::record(&format!("spell checker: {e}"));
                    s.failed.insert(code, e);
                }
                Done::Suggestions(word, list) => {
                    if let Some(m) = s.menu.as_mut().filter(|m| m.word == word) {
                        m.suggestions = Some(list);
                    }
                }
            }
        }
    }

    /// Loads the wanted downloaded dictionaries and drops the ones no longer wanted.
    fn spell_sync(&mut self) {
        if !self.spell.personal_loaded {
            self.spell.personal_loaded = true;
            if self.demo {
                self.spell.checker = Checker::demo();
            } else {
                self.spell.checker.set_personal(spell::load_personal());
            }
        }
        if self.demo {
            return;
        }
        let wanted = self.spell_langs();
        let s = &mut self.spell;
        if s.checker.dicts().iter().any(|d| !wanted.iter().any(|w| w == d.code)) {
            let keep = s.checker.dicts().iter().filter(|d| wanted.iter().any(|w| w == d.code)).cloned().collect();
            s.checker.set_dicts(keep);
            s.dict_changed();
        }
        for code in wanted {
            let Some(lang) = spell::find(&code) else { continue };
            let c = lang.code;
            if s.busy.contains_key(c) || s.failed.contains_key(c) || s.checker.dicts().iter().any(|d| d.code == c) || s.absent.contains(c) {
                continue;
            }
            if !spell::downloaded(c) {
                s.absent.insert(c);
                continue;
            }
            s.busy.insert(c, None);
            let (tx, ctx) = (s.tx.clone(), self.ctx.clone());
            std::thread::spawn(move || {
                let _ = tx.send(Done::Dict(c, spell::load(c).map(Arc::new)));
                ctx.request_repaint();
            });
        }
    }

    /// Downloads a dictionary on a thread (in the demo it is just marked as there).
    fn spell_download(&mut self, code: &'static str) {
        if self.demo {
            self.spell.demo_ready.insert(code);
            return;
        }
        if self.spell.busy.contains_key(code) {
            return;
        }
        self.spell.failed.remove(code);
        self.spell.absent.remove(code);
        self.spell.busy.insert(code, Some((0, 0)));
        let (tx, ctx) = (self.spell.tx.clone(), self.ctx.clone());
        std::thread::spawn(move || {
            let mut shown = 0u64;
            let res = spell::download(code, |got, total| {
                // A repaint per 256 KB is plenty for a progress line.
                if got - shown >= 256 << 10 {
                    shown = got;
                    let _ = tx.send(Done::Progress(code, got, total));
                    ctx.request_repaint();
                }
            });
            let _ = tx.send(Done::Dict(code, res.map(Arc::new)));
            ctx.request_repaint();
        });
    }

    /// The misspelled words of the draft as character ranges, for the field to underline.
    pub(super) fn spell_marks(&mut self) -> Vec<Range<usize>> {
        if !self.settings.spellcheck {
            self.spell.menu = None;
            return Vec::new();
        }
        self.spell_poll();
        self.spell_sync();
        let s = &mut self.spell;
        if !s.checker.active() {
            return Vec::new();
        }
        if let Some((text, marks)) = &s.last
            && s.complete
            && *text == self.draft
        {
            return marks.clone();
        }
        let mut budget = WORDS_PER_FRAME;
        let mut complete = true;
        let mut marks = Vec::new();
        // Byte offsets → character offsets, walking forward once.
        let (mut byte, mut ch) = (0, 0);
        for r in spell::words(&self.draft) {
            let word = &self.draft[r.clone()];
            let ok = match s.checker.cached(word) {
                Some(ok) => ok,
                None if budget > 0 => {
                    budget -= 1;
                    s.checker.check(word)
                }
                None => {
                    complete = false;
                    continue;
                }
            };
            if !ok {
                ch += self.draft[byte..r.start].chars().count();
                let len = word.chars().count();
                marks.push(ch..ch + len);
                ch += len;
                byte = r.end;
            }
        }
        if !complete {
            self.ctx.request_repaint();
        }
        s.complete = complete;
        s.last = Some((self.draft.clone(), marks.clone()));
        marks
    }

    /// The right-click menu of the message field: suggestions for the misspelled word under the
    /// pointer, "Add to dictionary" and "Ignore". `at` is the character the click landed on.
    pub(super) fn spell_menu(&mut self, resp: &egui::Response, at: Option<usize>) {
        if resp.secondary_clicked() {
            let marks = self.spell.last.as_ref().map(|(_, m)| m.clone()).unwrap_or_default();
            self.spell.menu = at.and_then(|c| marks.into_iter().find(|r| r.start <= c && c <= r.end)).map(|range| {
                let word: String = self.draft.chars().skip(range.start).take(range.len()).collect();
                let (tx, ctx, w) = (self.spell.tx.clone(), self.ctx.clone(), word.clone());
                let suggester = self.spell.checker.suggester(&word);
                std::thread::spawn(move || {
                    let list = suggester.suggest(&w);
                    let _ = tx.send(Done::Suggestions(w, list));
                    ctx.request_repaint();
                });
                WordMenu { range, word, suggestions: None }
            });
        }
        let Some(menu) = &self.spell.menu else { return };
        enum Pick {
            Replace(String),
            Add,
            Ignore,
        }
        let mut pick = None;
        let shown = resp.context_menu(|ui| {
            ui.set_min_width(204.0);
            match &menu.suggestions {
                None => {
                    ui.label(egui::RichText::new(l!("Пошук варіантів…", "Looking for suggestions…")).color(ink_dim()).size(13.0));
                }
                Some(list) if list.is_empty() => {
                    ui.label(egui::RichText::new(l!("Немає варіантів", "No suggestions")).color(ink_dim()).size(13.0));
                }
                Some(list) => {
                    for s in list {
                        if ui.add(egui::Button::new(egui::RichText::new(s).strong())).clicked() {
                            pick = Some(Pick::Replace(s.clone()));
                        }
                    }
                }
            }
            ui.separator();
            if glass::menu_button(ui, Icon::Plus, l!("Додати до словника", "Add to dictionary")).clicked() {
                pick = Some(Pick::Add);
            }
            if glass::menu_button(ui, Icon::EyeOff, l!("Ігнорувати", "Ignore")).clicked() {
                pick = Some(Pick::Ignore);
            }
        });
        let (range, word) = (menu.range.clone(), menu.word.clone());
        let done = pick.is_some();
        match pick {
            Some(Pick::Replace(with)) => {
                let (a, b) = (format::char_to_byte(&self.draft, range.start), format::char_to_byte(&self.draft, range.end));
                // Only if the word is still where the menu found it.
                if self.draft.get(a..b) == Some(word.as_str()) {
                    self.draft.replace_range(a..b, &with);
                    self.draft_edited_at = self.ctx.input(|i| i.time);
                }
            }
            Some(Pick::Add) => {
                self.spell.checker.add_personal(&word);
                if !self.demo {
                    spell::save_personal(self.spell.checker.personal());
                }
                self.spell.dict_changed();
            }
            Some(Pick::Ignore) => {
                self.spell.checker.ignore(&word);
                self.spell.dict_changed();
            }
            None => {}
        }
        if done || (shown.is_none() && !resp.secondary_clicked()) {
            self.spell.menu = None;
        }
    }

    /// Settings → Advanced → "Spell checker", as in Telegram Desktop.
    pub(super) fn spell_section(&mut self, ui: &mut Ui) {
        self.spell_poll();
        section_title(ui, l!("Перевірка правопису", "Spell checker"));
        let was = self.settings.spellcheck;
        ui.checkbox(&mut self.settings.spellcheck, l!("Перевіряти правопис", "Check spelling"));
        let wanted = self.spell_langs();
        if self.settings.spellcheck && !was {
            // Turning it on is the request to fetch what it needs.
            for code in &wanted {
                if let Some(l) = spell::find(code).filter(|l| !self.spell_downloaded(l.code)) {
                    self.spell_download(l.code);
                }
            }
        }
        if !self.settings.spellcheck {
            return;
        }
        self.spell_sync();
        let missing: Vec<&'static str> = wanted.iter().filter_map(|c| spell::find(c)).map(|l| l.code).filter(|c| !self.spell_downloaded(c)).collect();
        let loading = !self.spell.busy.is_empty();
        let names = |codes: &[String]| codes.iter().filter_map(|c| spell::find(c)).map(|l| l.name).collect::<Vec<_>>().join(", ");
        if self.demo {
            hint(ui, l!("Демо: вбудований список слів, без завантажень.", "Demo: a built-in word list, nothing is downloaded."));
        } else if loading {
            hint(ui, l!("Словники завантажуються…", "Loading dictionaries…"));
        } else if !missing.is_empty() {
            hint(ui, &lf!("Не завантажено: {}", "Not downloaded: {}", names(&missing.iter().map(|c| (*c).to_owned()).collect::<Vec<_>>())));
            let r = ui.allocate_exact_size(vec2(ui.available_width().min(260.0), 32.0), Sense::hover()).0;
            if glass::button(ui, r, "spell-get", l!("Завантажити словники", "Download dictionaries"), true, true).clicked() {
                for c in missing {
                    self.spell_download(c);
                }
            }
        } else if wanted.is_empty() {
            hint(ui, l!("Не вибрано жодного словника.", "No dictionaries chosen."));
        } else {
            hint(ui, &lf!("Словники: {}", "Dictionaries: {}", names(&wanted)));
        }
        let r = ui.allocate_exact_size(vec2(ui.available_width().min(260.0), 32.0), Sense::hover()).0;
        if glass::button(ui, r, "spell-manage", l!("Керувати словниками", "Manage dictionaries"), self.spell.manage, true).clicked() {
            self.spell.manage = !self.spell.manage;
        }
        if self.spell.manage {
            self.spell_dictionaries(ui, &wanted);
        }
    }

    /// The dictionary list: a tick to use one, its download state and a button to get or delete it.
    fn spell_dictionaries(&mut self, ui: &mut Ui, wanted: &[String]) {
        let mut chosen = wanted.to_vec();
        for lang in spell::LANGS {
            let ready = self.spell_downloaded(lang.code);
            let busy = self.spell.busy.get(lang.code).copied();
            let mut on = chosen.iter().any(|c| c == lang.code);
            ui.horizontal(|ui| {
                if ui.checkbox(&mut on, lang.name).changed() {
                    chosen.retain(|c| c != lang.code);
                    if on {
                        chosen.push(lang.code.to_owned());
                        if !ready {
                            self.spell_download(lang.code);
                        }
                    }
                }
                ui.with_layout(egui::Layout::right_to_left(Align::Center), |ui| {
                    let size = vec2(110.0, 28.0);
                    match busy {
                        Some(Some((got, total))) => {
                            let text = if total > 0 { format!("{}%", got * 100 / total.max(1)) } else { l!("Завантаження…", "Downloading…").to_owned() };
                            ui.label(egui::RichText::new(text).color(ink_dim()).size(12.5));
                        }
                        Some(None) => {
                            ui.label(egui::RichText::new(l!("Читання…", "Reading…")).color(ink_dim()).size(12.5));
                        }
                        None if ready => {
                            let r = ui.allocate_exact_size(size, Sense::hover()).0;
                            if glass::button(ui, r, ("spell-del", lang.code), l!("Видалити", "Delete"), false, !self.demo).clicked() {
                                spell::remove(lang.code);
                                let keep = self.spell.checker.dicts().iter().filter(|d| d.code != lang.code).cloned().collect();
                                self.spell.checker.set_dicts(keep);
                                self.spell.dict_changed();
                            }
                            if !self.demo {
                                let mb = spell::size_on_disk(lang.code) as f64 / 1_048_576.0;
                                ui.label(egui::RichText::new(format!("{mb:.1} MB")).color(ink_dim()).size(12.5));
                            }
                        }
                        None => {
                            let r = ui.allocate_exact_size(size, Sense::hover()).0;
                            if glass::button(ui, r, ("spell-get", lang.code), l!("Завантажити", "Download"), false, true).clicked() {
                                self.spell_download(lang.code);
                            }
                            if self.spell.failed.contains_key(lang.code) {
                                ui.label(egui::RichText::new(l!("Помилка", "Failed")).color(glass::danger()).size(12.5));
                            }
                        }
                    }
                });
            });
        }
        if chosen != wanted {
            self.settings.spell_langs = Some(chosen);
        }
        hint(ui, l!("Словники Hunspell з github.com/wooorm/dictionaries; слова, додані до словника, зберігаються лише на цьому пристрої.", "Hunspell dictionaries from github.com/wooorm/dictionaries; words you add stay on this device only."));
    }
}
