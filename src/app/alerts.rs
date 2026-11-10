//! Message notifications with "Reply" and "Mark as read", as in Telegram Desktop: one per chat
//! (on Linux the next message takes the place of the previous one and the title counts them),
//! a click on the notification opens the chat. The buttons come back on a channel, the UI thread
//! acts on them in `poll_alerts`.

use std::collections::HashMap;
use std::sync::{Arc, Mutex, mpsc};

use super::*;

/// What came back from a notification's thread.
enum Back {
    /// The notification is on screen; `fresh` — it didn't replace the chat's previous one.
    Shown { fresh: bool },
    Action(String),
}

pub(super) struct Alerts {
    tx: mpsc::Sender<(PeerId, Back)>,
    rx: mpsc::Receiver<(PeerId, Back)>,
    /// Each chat's notification on screen (Linux ids), to replace it with the next one.
    shown: Arc<Mutex<HashMap<PeerId, u32>>>,
    /// Messages a chat's notification stands for.
    counts: HashMap<PeerId, u32>,
    /// "Reply" was pressed: the composer takes the keyboard on the next frame.
    focus: bool,
}

impl Default for Alerts {
    fn default() -> Self {
        let (tx, rx) = mpsc::channel();
        Self { tx, rx, shown: Arc::default(), counts: HashMap::new(), focus: false }
    }
}

impl App {
    /// Shows (or updates) the chat's notification about a new message.
    pub(super) fn message_alert(&mut self, peer: PeerId, title: &str, body: &str) {
        let n = self.alerts.counts.entry(peer).or_insert(0);
        *n += 1;
        let title = if *n > 1 { format!("{title} ({n})") } else { title.to_owned() };
        let replace = self.alerts.shown.lock().unwrap().get(&peer).copied().unwrap_or(0);
        // Under a local passcode the buttons would act on a locked app.
        let actions = if self.lock.on { Vec::new() } else { vec![("reply", l!("Відповісти", "Reply").to_owned()), ("read", l!("Позначити прочитаним", "Mark as read").to_owned())] };
        let (tx, shown, ctx) = (self.alerts.tx.clone(), self.alerts.shown.clone(), self.ctx.clone());
        let (tx2, ctx2) = (tx.clone(), ctx.clone());
        crate::platform::notify_actions(
            replace,
            &title,
            body,
            actions,
            move |id| {
                if id != 0 {
                    shown.lock().unwrap().insert(peer, id);
                }
                let _ = tx.send((peer, Back::Shown { fresh: id != replace }));
                ctx.request_repaint();
            },
            move |key| {
                if key != "read" {
                    crate::tray::show_window();
                }
                let _ = tx2.send((peer, Back::Action(key.to_owned())));
                ctx2.request_repaint();
            },
        );
    }

    /// Acts on the buttons pressed in notifications.
    pub(super) fn poll_alerts(&mut self) {
        let back: Vec<_> = self.alerts.rx.try_iter().collect();
        for (peer, b) in back {
            match b {
                // The previous one was dismissed: the new notification counts from one again.
                Back::Shown { fresh: true } => {
                    self.alerts.counts.insert(peer, 1);
                }
                Back::Shown { fresh: false } => {}
                Back::Action(key) => {
                    self.alerts.counts.remove(&peer);
                    self.alerts.shown.lock().unwrap().remove(&peer);
                    if key == "read" && !self.lock.on {
                        self.apply_chat(peer, ChatAction::Read);
                        continue;
                    }
                    self.ctx.send_viewport_cmd(egui::ViewportCommand::Minimized(false));
                    self.ctx.send_viewport_cmd(egui::ViewportCommand::Focus);
                    if !self.lock.on && self.chats.iter().any(|c| c.id == peer) {
                        self.open_chat(peer);
                        self.alerts.focus = key == "reply";
                    }
                }
            }
        }
        // A chat looked at in a focused window has nothing left to count.
        if let Some(peer) = self.selected
            && self.ctx.input(|i| i.viewport().focused.unwrap_or(true))
        {
            self.alerts.counts.remove(&peer);
        }
    }

    /// Whether the composer should take the keyboard now (after "Reply").
    pub(super) fn take_composer_focus(&mut self) -> bool {
        std::mem::take(&mut self.alerts.focus)
    }
}
