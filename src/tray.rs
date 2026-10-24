//! System tray icon: unread counter on the icon, notification toggle and quit.
//! Linux: StatusNotifierItem over D-Bus (`ksni`, its own thread, no system libraries).
//! Windows and macOS: `tray-icon` on the UI thread. Both send events to the UI over a channel.

use std::sync::mpsc::Sender;

use eframe::egui;
use crate::i18n::{l, lf};

pub enum TrayEv {
    /// Click on the icon: show the window.
    Show,
    /// The notifications item was clicked: the UI flips the setting and syncs the tray back.
    ToggleNotifications,
    Quit,
}

fn tooltip(unread: usize) -> String {
    match unread {
        0 => l!("Немає непрочитаних", "No unread messages").to_owned(),
        n => lf!("Непрочитаних повідомлень: {n}", "Unread messages: {n}"),
    }
}

fn open_label() -> &'static str {
    l!("Відкрити Glassgram", "Open Glassgram")
}

fn notifications_label() -> &'static str {
    l!("Сповіщення", "Notifications")
}

fn quit_label() -> &'static str {
    l!("Закрити Glassgram", "Quit Glassgram")
}

/// The tray of the running app; the UI only sees this.
pub struct TrayHandle(imp::Handle);

impl TrayHandle {
    pub fn set_unread(&self, unread: usize) {
        self.0.set_unread(unread);
    }

    pub fn set_notifications(&self, on: bool) {
        self.0.set_notifications(on);
    }

    /// Texts again after a language switch.
    pub fn refresh(&self) {
        self.0.refresh();
    }

    /// Remembers the native window so the tray can bring it back (Windows; elsewhere nothing).
    pub fn attach(&self, frame: &eframe::Frame) {
        #[cfg(target_os = "windows")]
        win::attach(frame);
        #[cfg(not(target_os = "windows"))]
        let _ = frame;
    }

    /// Closing the window hides it to the tray, as in Telegram Desktop on Windows.
    /// `false` where that isn't done: the window then closes as usual.
    pub fn hide_window(&self) -> bool {
        #[cfg(target_os = "windows")]
        return win::hide();
        #[cfg(not(target_os = "windows"))]
        false
    }
}

/// The window is shown and hidden through its HWND: a window hidden by eframe stops getting
/// frames, so it couldn't show itself again on a tray click.
#[cfg(target_os = "windows")]
mod win {
    use raw_window_handle::{HasWindowHandle, RawWindowHandle};
    use std::sync::atomic::{AtomicIsize, Ordering};
    use windows_sys::Win32::UI::WindowsAndMessaging::{SW_HIDE, SW_RESTORE, SetForegroundWindow, ShowWindow};

    static HWND: AtomicIsize = AtomicIsize::new(0);

    pub fn attach(frame: &eframe::Frame) {
        if HWND.load(Ordering::Relaxed) != 0 {
            return;
        }
        if let Ok(h) = frame.window_handle()
            && let RawWindowHandle::Win32(w) = h.as_raw()
        {
            HWND.store(w.hwnd.get(), Ordering::Relaxed);
        }
    }

    pub fn hide() -> bool {
        let h = HWND.load(Ordering::Relaxed);
        if h == 0 {
            return false;
        }
        // SAFETY: a window handle we got from eframe; ShowWindow tolerates a stale one.
        unsafe { ShowWindow(h as _, SW_HIDE) };
        true
    }

    pub fn show() {
        let h = HWND.load(Ordering::Relaxed);
        if h != 0 {
            // SAFETY: as above.
            unsafe {
                ShowWindow(h as _, SW_RESTORE);
                SetForegroundWindow(h as _);
            }
        }
    }
}

/// Starts the tray. `None` when the desktop has no tray (then simply no tray).
pub fn spawn(tx: Sender<TrayEv>, ctx: egui::Context, notifications: bool) -> Option<TrayHandle> {
    match imp::spawn(tx, ctx, notifications) {
        Ok(h) => Some(TrayHandle(h)),
        Err(e) => {
            eprintln!("glassgram: tray unavailable: {e}");
            None
        }
    }
}

#[cfg(target_os = "linux")]
mod imp {
    use super::*;
    use ksni::blocking::TrayMethods;

    pub struct Tray {
        tx: Sender<TrayEv>,
        ctx: egui::Context,
        unread: usize,
        notifications: bool,
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
            ksni::ToolTip { title: "Glassgram".into(), description: tooltip(self.unread), ..Default::default() }
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
                StandardItem { label: open_label().into(), activate: Box::new(|this: &mut Self| this.emit(TrayEv::Show)), ..Default::default() }.into(),
                CheckmarkItem {
                    label: notifications_label().into(),
                    checked: self.notifications,
                    activate: Box::new(|this: &mut Self| this.emit(TrayEv::ToggleNotifications)),
                    ..Default::default()
                }
                .into(),
                MenuItem::Separator,
                StandardItem {
                    label: quit_label().into(),
                    icon_name: "application-exit".into(),
                    activate: Box::new(|this: &mut Self| this.emit(TrayEv::Quit)),
                    ..Default::default()
                }
                .into(),
            ]
        }
    }

    pub struct Handle(ksni::blocking::Handle<Tray>);

    impl Handle {
        pub fn set_unread(&self, unread: usize) {
            self.0.update(|t| t.unread = unread);
        }

        pub fn set_notifications(&self, on: bool) {
            self.0.update(|t| t.notifications = on);
        }

        pub fn refresh(&self) {
            // The tray rebuilds its menu and tooltip only after an update.
            self.0.update(|_| {});
        }
    }

    pub fn spawn(tx: Sender<TrayEv>, ctx: egui::Context, notifications: bool) -> Result<Handle, String> {
        Tray { tx, ctx, unread: 0, notifications }.spawn().map(Handle).map_err(|e| e.to_string())
    }
}

#[cfg(not(target_os = "linux"))]
mod imp {
    use super::*;
    use tray_icon::menu::{CheckMenuItem, Menu, MenuEvent, MenuItem, PredefinedMenuItem};
    use tray_icon::{MouseButton, MouseButtonState, TrayIcon, TrayIconBuilder, TrayIconEvent};

    pub struct Handle {
        icon: TrayIcon,
        open: MenuItem,
        notifications: CheckMenuItem,
        quit: MenuItem,
        unread: std::cell::Cell<usize>,
    }

    fn icon(unread: bool) -> Option<tray_icon::Icon> {
        let img = crate::logo::render(32, unread);
        tray_icon::Icon::from_rgba(img.into_raw(), 32, 32).ok()
    }

    impl Handle {
        pub fn set_unread(&self, unread: usize) {
            if (unread > 0) != (self.unread.get() > 0) {
                let _ = self.icon.set_icon(icon(unread > 0));
            }
            self.unread.set(unread);
            let _ = self.icon.set_tooltip(Some(tooltip(unread)));
        }

        pub fn set_notifications(&self, on: bool) {
            self.notifications.set_checked(on);
        }

        pub fn refresh(&self) {
            self.open.set_text(open_label());
            self.notifications.set_text(notifications_label());
            self.quit.set_text(quit_label());
            let _ = self.icon.set_tooltip(Some(tooltip(self.unread.get())));
        }
    }

    /// Must run on the UI thread: Windows and macOS deliver tray events to the thread's event loop.
    pub fn spawn(tx: Sender<TrayEv>, ctx: egui::Context, notifications: bool) -> Result<Handle, String> {
        let open = MenuItem::new(open_label(), true, None);
        let check = CheckMenuItem::new(notifications_label(), true, notifications, None);
        let quit = MenuItem::new(quit_label(), true, None);
        let menu = Menu::new();
        menu.append_items(&[&open, &check, &PredefinedMenuItem::separator(), &quit]).map_err(|e| e.to_string())?;
        let icon_tray = TrayIconBuilder::new()
            .with_menu(Box::new(menu))
            // A left click shows the window, as in Telegram Desktop; the menu is on the right click.
            .with_menu_on_left_click(false)
            .with_tooltip(tooltip(0))
            .with_icon(icon(false).ok_or("icon")?)
            .build()
            .map_err(|e| e.to_string())?;
        let (open_id, check_id, quit_id) = (open.id().clone(), check.id().clone(), quit.id().clone());
        let (menu_tx, menu_ctx) = (tx.clone(), ctx.clone());
        MenuEvent::set_event_handler(Some(move |e: MenuEvent| {
            let ev = if e.id == open_id {
                #[cfg(target_os = "windows")]
                super::win::show();
                TrayEv::Show
            } else if e.id == check_id {
                TrayEv::ToggleNotifications
            } else if e.id == quit_id {
                TrayEv::Quit
            } else {
                return;
            };
            let _ = menu_tx.send(ev);
            menu_ctx.request_repaint();
        }));
        TrayIconEvent::set_event_handler(Some(move |e: TrayIconEvent| {
            if let TrayIconEvent::Click { button: MouseButton::Left, button_state: MouseButtonState::Up, .. } = e {
                #[cfg(target_os = "windows")]
                super::win::show();
                let _ = tx.send(TrayEv::Show);
                ctx.request_repaint();
            }
        }));
        Ok(Handle { icon: icon_tray, open, notifications: check, quit, unread: std::cell::Cell::new(0) })
    }
}
