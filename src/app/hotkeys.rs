//! Keyboard shortcuts as in Telegram Desktop. They run at the start of the frame and consume their
//! keys, so the composer (multiline text edit) doesn't get Ctrl+Tab or Alt+arrows.

use eframe::egui::{self, Key, Modifiers};

use super::{ARCHIVE, App, Compose, Screen, Search};
use crate::backend::Cmd;

const DIGITS: [Key; 9] = [Key::Num1, Key::Num2, Key::Num3, Key::Num4, Key::Num5, Key::Num6, Key::Num7, Key::Num8, Key::Num9];

#[derive(Default)]
struct Pressed {
    search_chat: bool,
    search_all: bool,
    edit_last: bool,
    prev: bool,
    next: bool,
    folder: Option<usize>,
    esc: bool,
}

impl App {
    /// Windows over the main screen own the keyboard: Esc and shortcuts must not reach the chat under them.
    pub(super) fn modal_open(&self) -> bool {
        self.settings_open || self.folder_edit.is_some() || self.accounts_open || self.export_ask.is_some() || self.viewer.is_some() || self.topic_edit.is_some()
    }

    pub(super) fn hotkeys(&mut self, ctx: &egui::Context) {
        if !matches!(self.screen, Screen::Main) || self.modal_open() {
            return;
        }
        let k = ctx.input_mut(|i| {
            // Shift variants first: `consume_key` ignores an extra Shift.
            let prev = i.consume_key(Modifiers::CTRL | Modifiers::SHIFT, Key::Tab)
                || i.consume_key(Modifiers::ALT, Key::ArrowUp)
                || i.consume_key(Modifiers::CTRL, Key::PageUp);
            let next = i.consume_key(Modifiers::CTRL, Key::Tab) || i.consume_key(Modifiers::ALT, Key::ArrowDown) || i.consume_key(Modifiers::CTRL, Key::PageDown);
            Pressed {
                search_chat: i.consume_key(Modifiers::COMMAND, Key::F),
                search_all: i.consume_key(Modifiers::COMMAND, Key::K),
                edit_last: i.consume_key(Modifiers::COMMAND, Key::E),
                prev,
                next,
                folder: DIGITS.iter().position(|d| i.consume_key(Modifiers::COMMAND, *d)),
                esc: i.key_pressed(Key::Escape),
            }
        });
        let forum_list = self.selected.and_then(|p| self.chats.iter().find(|c| c.id == p)).is_some_and(|c| c.forum && self.topic.is_none());
        let in_chat = self.selected.is_some() && !forum_list;
        if k.search_chat && in_chat {
            // An empty search panel focuses its own field.
            self.chat_search = Some(Search::default());
        } else if k.search_all || k.search_chat {
            self.focus_search = true;
        }
        if k.edit_last {
            self.edit_last();
        }
        if k.prev || k.next {
            self.step_chat(if k.next { 1 } else { -1 });
        }
        if let Some(n) = k.folder {
            self.pick_folder(n);
        }
        // Inside an open chat Esc goes through the overlay chain (`chat_overlays`).
        if k.esc && !in_chat {
            if !self.search.query.is_empty() {
                self.search = Search::default();
            } else if self.selected.is_some() {
                self.close_chat();
            }
        }
    }

    /// The last of the chain: nothing else to close — clear the search, then close the chat.
    pub(super) fn esc_fallback(&mut self) {
        if !self.search.query.is_empty() {
            self.search = Search::default();
        } else {
            self.close_chat();
        }
    }

    fn close_chat(&mut self) {
        self.leave_chat();
        self.selected = None;
        self.topic = None;
        self.chat_search = None;
        self.picked.clear();
        self.sticker_panel = false;
    }

    /// Ctrl+E or ↑ in an empty composer: edit the last own text message.
    pub(super) fn edit_last(&mut self) {
        let Some(peer) = self.selected else { return };
        let topic = self.topic;
        let last = self.history.get(&peer).and_then(|list| {
            list.iter()
                .rev()
                .filter(|m| topic.is_none_or(|t| m.topic_id() == t))
                .find(|m| m.out && m.id > 0 && !m.text.is_empty() && m.sticker.is_none() && m.poll.is_none() && m.forwarded.is_none())
                .map(|m| m.id)
        });
        if let Some(id) = last {
            self.apply(peer, super::Action::Edit(id));
        }
    }

    /// Next or previous chat in the visible list (current folder or search results).
    fn step_chat(&mut self, dir: i32) {
        let query = self.search.query.trim().to_lowercase();
        let list: Vec<_> = self
            .chats
            .iter()
            .filter(|c| if query.is_empty() { self.in_folder(c, self.folder) } else { c.title.to_lowercase().contains(&query) })
            .map(|c| c.id)
            .collect();
        if list.is_empty() {
            return;
        }
        let at = self.selected.and_then(|s| list.iter().position(|c| *c == s));
        let to = match at {
            Some(i) => (i as i32 + dir).clamp(0, list.len() as i32 - 1) as usize,
            None if dir > 0 => 0,
            None => list.len() - 1,
        };
        if Some(list[to]) != self.selected {
            self.open_chat(list[to]);
        }
    }

    /// Ctrl+1…8 — folder by position ("All" is the first), Ctrl+9 — the last one, as in Telegram Desktop.
    fn pick_folder(&mut self, n: usize) {
        let mut tabs: Vec<Option<i32>> = vec![None];
        tabs.extend(self.folders.iter().map(|f| Some(f.id)));
        if self.chats.iter().any(|c| c.archived) {
            tabs.push(Some(ARCHIVE));
        }
        let pick = if n == 8 { tabs.last() } else { tabs.get(n) };
        if let Some(&f) = pick {
            self.folder = f;
        }
    }

    /// For the composer: whether ↑ should start editing (empty field, no other mode).
    pub(super) fn can_edit_by_arrow(&self) -> bool {
        self.draft.is_empty() && self.compose == Compose::New && self.picked.is_empty()
    }

    /// Opening the settings also refreshes the own profile for the header.
    pub(super) fn open_settings(&mut self) {
        self.settings_open = true;
        self.settings_tab = super::SettingsTab::Main;
        self.send(Cmd::LoadProfile);
    }
}
