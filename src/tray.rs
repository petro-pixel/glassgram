//! System tray icon (StatusNotifierItem over D-Bus): unread counter
//! on the icon, notification toggle and quit. The tray runs in its own thread and sends events to the UI over a channel.

use std::sync::mpsc::Sender;

use eframe::egui;
use ksni::blocking::{Handle, TrayMethods};
use crate::i18n::{l, lf};

pub enum TrayEv {
    /// Click on the icon: show the window.
    Show,
    Notifications(bool),
    Quit,
}

pub struct Tray {
    tx: Sender<TrayEv>,
    ctx: egui::Context,
    pub unread: usize,
    pub notifications: bool,
}

impl Tray {
    fn emit(&self, ev: TrayEv) {
        let _ = self.tx.send(ev);
        self.ctx.request_repaint();
    }
}

impl ksni::Tray for Tray {
    fn id(&self) -> String {
        "glassgram".into()
    }

    fn title(&self) -> String {
        "Glassgram".into()
    }

    fn tool_tip(&self) -> ksni::ToolTip {
        let description = match self.unread {
            0 => l!("Немає непрочитаних", "No unread messages").to_owned(),
            n => lf!("Непрочитаних повідомлень: {n}", "Unread messages: {n}"),
        };
        ksni::ToolTip { title: "Glassgram".into(), description, ..Default::default() }
    }

    fn icon_pixmap(&self) -> Vec<ksni::Icon> {
        // Several sizes; the panel picks the closest one itself.
        [22, 32, 48, 64]
            .into_iter()
            .map(|size| {
                let mut data = crate::logo::render(size, self.unread > 0).into_raw();
                // RGBA → ARGB, as the spec requires.
                data.chunks_exact_mut(4).for_each(|px| px.rotate_right(1));
                ksni::Icon { width: size as i32, height: size as i32, data }
            })
            .collect()
    }

    fn activate(&mut self, _x: i32, _y: i32) {
        self.emit(TrayEv::Show);
    }

    fn menu(&self) -> Vec<ksni::MenuItem<Self>> {
        use ksni::menu::*;
        vec![
            StandardItem {
                label: l!("Відкрити Glassgram", "Open Glassgram").into(),
                activate: Box::new(|this: &mut Self| this.emit(TrayEv::Show)),
                ..Default::default()
            }
            .into(),
            CheckmarkItem {
                label: l!("Сповіщення", "Notifications").into(),
                checked: self.notifications,
                activate: Box::new(|this: &mut Self| {
                    this.notifications = !this.notifications;
                    this.emit(TrayEv::Notifications(this.notifications));
                }),
                ..Default::default()
            }
            .into(),
            MenuItem::Separator,
            StandardItem {
                label: l!("Вийти", "Log out").into(),
                icon_name: "application-exit".into(),
                activate: Box::new(|this: &mut Self| this.emit(TrayEv::Quit)),
                ..Default::default()
            }
            .into(),
        ]
    }
}

/// Starts the tray. `None` if the session has no StatusNotifier host (then simply no tray).
pub fn spawn(tx: Sender<TrayEv>, ctx: egui::Context, notifications: bool) -> Option<Handle<Tray>> {
    let tray = Tray { tx, ctx, unread: 0, notifications };
    match tray.spawn() {
        Ok(h) => Some(h),
        Err(e) => {
            eprintln!("glassgram: трей недоступний: {e}");
            None
        }
    }
}
