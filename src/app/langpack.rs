//! Settings → Language: Ukrainian and English built in (always offline), localisation files, and every
//! language Telegram Desktop offers. Picking a Telegram language downloads its pack (or refreshes the
//! cached one) and switches to it when it arrives, without a restart.

use super::*;
use super::settings_pages::hint;
use crate::backend::{LangInfo, LangPack};
use crate::i18n::Lang;

#[derive(Default)]
pub(super) struct LangPacks {
    /// The server list; empty until it arrives (or offline).
    list: Vec<LangInfo>,
    /// The list was asked for and hasn't come yet.
    loading: bool,
    /// A language being downloaded; becomes the interface language when its pack arrives.
    pending: Option<String>,
    /// Filter over the server list.
    filter: String,
}

impl App {
    /// Opening the page: the server list, and the active Telegram language brought up to date.
    pub(super) fn open_language_page(&mut self) {
        if self.langpacks.list.is_empty() {
            self.langpacks.loading = true;
            self.send(Cmd::LoadLanguages);
        }
        self.refresh_lang_pack();
    }

    /// After logging in: a cached Telegram language picks up what changed since it was saved.
    pub(super) fn refresh_lang_pack(&mut self) {
        if let Lang::File(code) = &self.settings.lang
            && crate::backend::langpack::load_cached(code).is_some()
        {
            self.send(Cmd::LoadLangPack { code: code.clone(), name: String::new() });
        }
    }

    pub(super) fn langpack_event(&mut self, ev: Ev) {
        match ev {
            Ev::Languages(list) => {
                self.langpacks.list = list;
                self.langpacks.loading = false;
            }
            Ev::LangPack { code, pack } => self.on_lang_pack(code, pack),
            _ => {}
        }
    }

    fn on_lang_pack(&mut self, code: String, pack: Result<LangPack, String>) {
        let chosen = self.langpacks.pending.as_deref() == Some(code.as_str());
        if chosen {
            self.langpacks.pending = None;
        }
        match pack {
            Ok(pack) => {
                crate::i18n::install(&pack);
                if chosen {
                    self.settings.lang = Lang::File(code);
                    self.settings.save();
                }
                // Also when it was a refresh of the language already shown: new strings appear at once.
                if self.settings.lang.code() == pack.code || chosen {
                    self.lang_changed();
                }
            }
            Err(e) if chosen => self.toast = Some((e, 0.0, true)),
            Err(e) => crate::errlog::record(&e),
        }
    }

    /// The interface language changed: strings, the tray menu and the window title follow.
    pub(super) fn lang_changed(&mut self) {
        crate::i18n::set(&self.settings.lang);
        if let Some(t) = &self.tray {
            t.refresh();
        }
        self.title_unread = usize::MAX;
    }

    pub(super) fn language_page(&mut self, ui: &mut Ui) {
        let mut pick: Option<(String, String)> = None;
        let mut local: Option<Lang> = None;
        egui::ScrollArea::vertical().id_salt("settings-lang").auto_shrink(false).show(ui, |ui| {
            // Always offline: Ukrainian and English are in the code, files ship with it or sit in the data folder.
            let server: Vec<&str> = self.langpacks.list.iter().map(|l| l.code.as_str()).collect();
            let builtin = crate::i18n::available().into_iter().filter(|l| !matches!(l, Lang::File(c) if server.contains(&c.as_str())));
            for (k, lang) in builtin.enumerate() {
                let r = ui.allocate_exact_size(vec2(ui.available_width(), 40.0), Sense::hover()).0;
                if glass::button(ui, r, ("lang", k), &lang.name(), self.settings.lang == lang, true).clicked() {
                    local = Some(lang);
                }
            }
            section_title(ui, l!("Мови Telegram", "Telegram languages"));
            if self.langpacks.list.is_empty() {
                if self.langpacks.loading {
                    ui.add_space(8.0);
                    ui.vertical_centered(|ui| ui.add(egui::Spinner::new().color(ink_dim())));
                } else {
                    hint(ui, l!("Список мов з'явиться після підключення.", "The language list appears once connected."));
                }
            } else {
                let r = ui.allocate_exact_size(vec2(ui.available_width(), 36.0), Sense::hover()).0;
                glass::search_field(ui, r, &mut self.langpacks.filter, l!("Пошук", "Search"));
                ui.add_space(6.0);
                let q = self.langpacks.filter.to_lowercase();
                for l in self.langpacks.list.iter().filter(|l| !matches!(l.code.as_str(), "uk" | "en")) {
                    if !q.is_empty() && !l.name.to_lowercase().contains(&q) && !l.native.to_lowercase().contains(&q) && !l.code.contains(&q) {
                        continue;
                    }
                    let selected = self.settings.lang.code() == l.code;
                    let busy = self.langpacks.pending.as_deref() == Some(l.code.as_str());
                    if language_row(ui, l, selected, busy).clicked() && !selected {
                        pick = Some((l.code.clone(), l.native.clone()));
                    }
                }
            }
            ui.add_space(8.0);
            hint(ui, l!(
                "Переклади мов Telegram — з translations.telegram.org; чого бракує, показано англійською. Власні переклади — файли в теці locales поруч із налаштуваннями.",
                "Telegram languages come from translations.telegram.org; what's missing shows in English. Your own translations are files in the locales folder next to the settings."
            ));
        });
        if let Some(lang) = local {
            self.langpacks.pending = None;
            self.settings.lang = lang;
        }
        if let Some((code, name)) = pick {
            // A localisation file for the same language still wins over the pack (see `i18n::build`).
            self.langpacks.pending = Some(code.clone());
            self.send(Cmd::LoadLangPack { code, name });
        }
    }
}

/// A Telegram language: its own name, the English one under it, "beta" and how much is translated.
fn language_row(ui: &mut Ui, l: &LangInfo, selected: bool, busy: bool) -> egui::Response {
    let (r, resp) = ui.allocate_exact_size(vec2(ui.available_width(), 48.0), Sense::click());
    let p = ui.painter();
    if selected {
        p.rect_filled(r, 14.0, glass::veil(22));
    } else if resp.hovered() {
        p.rect_filled(r, 14.0, glass::veil(12));
        ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
    }
    let w = r.width() - 120.0;
    p.galley(pos2(r.left() + 12.0, r.top() + 6.0), elided(ui, &l.native, 14.5, ink(), w), ink());
    p.galley(pos2(r.left() + 12.0, r.top() + 27.0), elided(ui, &l.name, 12.5, ink_dim(), w), ink());
    let right = pos2(r.right() - 12.0, r.center().y);
    if busy {
        ui.put(Rect::from_center_size(pos2(right.x - 9.0, right.y), vec2(18.0, 18.0)), egui::Spinner::new().size(18.0).color(ink_dim()));
    } else if selected {
        icons::draw(p, Rect::from_center_size(pos2(right.x - 9.0, right.y), vec2(18.0, 18.0)), Icon::Check, ACCENT);
    } else {
        let mut tags = Vec::new();
        if l.beta {
            tags.push(l!("бета", "beta").to_owned());
        }
        if l.percent < 100 {
            tags.push(format!("{}%", l.percent));
        }
        p.text(right, Align2::RIGHT_CENTER, tags.join(" · "), FontId::proportional(12.5), ink_dim());
    }
    resp
}
