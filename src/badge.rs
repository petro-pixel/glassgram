//! Unread counter on the app's taskbar or dock icon, as in Telegram Desktop. It counts the same
//! chats as the window title: unmuted and not archived.
//! Linux: the Unity LauncherEntry D-Bus signal for our desktop file (Ubuntu Dock, Dash to Dock,
//! KDE Plasma). Windows: an overlay icon on the taskbar button. macOS: the dock tile's badge.

use std::cell::Cell;

thread_local! {
    /// The count the icon shows, and when the last failed attempt was (the taskbar button may not
    /// exist yet right after start, so it is retried, at most once a second).
    static SHOWN: Cell<Option<usize>> = const { Cell::new(None) };
    static FAILED_AT: Cell<Option<std::time::Instant>> = const { Cell::new(None) };
}

/// Called every frame on the UI thread; only a changed count reaches the system.
pub fn set(count: usize) {
    if SHOWN.get() == Some(count) || FAILED_AT.get().is_some_and(|t| t.elapsed().as_secs_f32() < 1.0) {
        return;
    }
    if imp::set(count) {
        SHOWN.set(Some(count));
        FAILED_AT.set(None);
    } else {
        FAILED_AT.set(Some(std::time::Instant::now()));
    }
}

/// What the badge says: up to 99, then "99+".
#[cfg_attr(target_os = "linux", allow(dead_code))]
fn label(count: usize) -> String {
    if count > 99 { "99+".to_owned() } else { count.to_string() }
}

/// The Windows overlay: white digits on a red disc, `size`×`size`, RGBA not premultiplied.
#[cfg(any(target_os = "windows", test))]
fn render(count: usize, size: u16) -> Vec<u8> {
    use vello_cpu::kurbo::{Affine, Circle, Shape as _};
    use vello_cpu::peniko::Color;
    use vello_cpu::{Pixmap, RenderContext, Resources};

    let s = size as f64;
    let mut ctx = RenderContext::new(size, size);
    // The same red as the unread dot on the tray icon (logo.rs).
    ctx.set_paint(Color::from_rgb8(255, 72, 92));
    ctx.fill_path(&Circle::new((s / 2.0, s / 2.0), s / 2.0).to_path(0.05));
    let text = label(count);
    let mut font = s * 0.66;
    if let Some((mut path, mut width)) = crate::draw::text_path(&text, font) {
        // "99+" is wider than the disc at full size.
        if width > s * 0.84 {
            font *= s * 0.84 / width;
            if let Some(p) = crate::draw::text_path(&text, font) {
                (path, width) = p;
            }
        }
        ctx.set_paint(Color::from_rgb8(255, 255, 255));
        ctx.fill_path(&(Affine::translate(((s - width) / 2.0, s / 2.0 + font * 0.36)) * path));
    }
    let mut target = Pixmap::new(size, size);
    ctx.flush();
    ctx.render(&mut target, &mut Resources::new());
    target.take_unpremultiplied().into_iter().flat_map(|p| [p.r, p.g, p.b, p.a]).collect()
}

#[cfg(target_os = "linux")]
mod imp {
    use std::collections::HashMap;
    use std::sync::OnceLock;
    use std::sync::mpsc::{self, Sender};

    use zbus::zvariant::Value;

    /// The desktop file docks match the signal against: the installed one and the Flatpak's.
    const APP_URI: &str = "application://io.github.petro_pixel.Glassgram.desktop";

    /// D-Bus calls run on their own thread, so a slow session bus never holds up a frame.
    static TX: OnceLock<Option<Sender<usize>>> = OnceLock::new();

    pub fn set(count: usize) -> bool {
        let tx = TX.get_or_init(|| {
            let (tx, rx) = mpsc::channel::<usize>();
            let spawned = std::thread::Builder::new().name("badge".into()).spawn(move || {
                let conn = match zbus::blocking::Connection::session() {
                    Ok(c) => c,
                    Err(e) => return crate::errlog::record(&format!("unread badge unavailable: {e}")),
                };
                for count in rx {
                    let props = HashMap::from([("count", Value::from(count as i64)), ("count-visible", Value::from(count > 0))]);
                    let _ = conn.emit_signal(None::<&str>, "/io/github/petro_pixel/Glassgram", "com.canonical.Unity.LauncherEntry", "Update", &(APP_URI, props));
                }
            });
            spawned.ok().map(|_| tx)
        });
        // Without a bus there is nothing to retry: report it as shown either way.
        if let Some(tx) = tx {
            let _ = tx.send(count);
        }
        true
    }
}

#[cfg(target_os = "windows")]
mod imp {
    use std::cell::RefCell;

    use windows::Win32::Foundation::HWND;
    use windows::Win32::Graphics::Gdi::{CreateBitmap, DeleteObject};
    use windows::Win32::System::Com::{CLSCTX_INPROC_SERVER, COINIT_APARTMENTTHREADED, CoCreateInstance, CoInitializeEx};
    use windows::Win32::UI::Shell::{ITaskbarList3, TaskbarList};
    use windows::Win32::UI::WindowsAndMessaging::{CreateIconIndirect, DestroyIcon, HICON, ICONINFO};
    use windows::core::{HSTRING, PCWSTR};

    /// 16 pt at 200 %: Windows scales it down for lower DPI.
    const SIZE: u16 = 32;

    thread_local! {
        static TASKBAR: RefCell<Option<ITaskbarList3>> = const { RefCell::new(None) };
    }

    pub fn set(count: usize) -> bool {
        let Some(hwnd) = crate::tray::window() else { return false };
        let hwnd = HWND(hwnd as _);
        TASKBAR.with_borrow_mut(|slot| {
            if slot.is_none() {
                // SAFETY: COM on the UI thread; winit has usually initialised it already, which is fine.
                *slot = unsafe {
                    let _ = CoInitializeEx(None, COINIT_APARTMENTTHREADED);
                    CoCreateInstance::<_, ITaskbarList3>(&TaskbarList, None, CLSCTX_INPROC_SERVER).ok().filter(|t| t.HrInit().is_ok())
                };
            }
            let Some(taskbar) = slot.as_ref() else { return false };
            if count == 0 {
                // SAFETY: a window handle from eframe and a null icon, which removes the overlay.
                return unsafe { taskbar.SetOverlayIcon(hwnd, HICON::default(), PCWSTR::null()) }.is_ok();
            }
            let Some(icon) = icon(count) else { return false };
            let desc = HSTRING::from(super::label(count));
            // SAFETY: the taskbar copies the icon, so it is destroyed right after.
            unsafe {
                let ok = taskbar.SetOverlayIcon(hwnd, icon, &desc).is_ok();
                let _ = DestroyIcon(icon);
                ok
            }
        })
    }

    /// A 32-bit icon with alpha from the rendered badge.
    fn icon(count: usize) -> Option<HICON> {
        let rgba = super::render(count, SIZE);
        let bgra: Vec<u8> = rgba.chunks_exact(4).flat_map(|p| [p[2], p[1], p[0], p[3]]).collect();
        let mask = vec![0u8; (SIZE as usize).div_ceil(16) * 2 * SIZE as usize];
        // SAFETY: both buffers outlive the calls and match the sizes given; the bitmaps are freed
        // once the icon holds its own copy.
        unsafe {
            let color = CreateBitmap(SIZE as i32, SIZE as i32, 1, 32, Some(bgra.as_ptr().cast()));
            let alpha_mask = CreateBitmap(SIZE as i32, SIZE as i32, 1, 1, Some(mask.as_ptr().cast()));
            let info = ICONINFO { fIcon: true.into(), xHotspot: 0, yHotspot: 0, hbmMask: alpha_mask, hbmColor: color };
            let icon = CreateIconIndirect(&info).ok();
            let _ = DeleteObject(color.into());
            let _ = DeleteObject(alpha_mask.into());
            icon
        }
    }
}

#[cfg(target_os = "macos")]
mod imp {
    use objc2::MainThreadMarker;
    use objc2_app_kit::NSApplication;
    use objc2_foundation::NSString;

    pub fn set(count: usize) -> bool {
        let Some(mtm) = MainThreadMarker::new() else { return false };
        let label = (count > 0).then(|| NSString::from_str(&super::label(count)));
        NSApplication::sharedApplication(mtm).dockTile().setBadgeLabel(label.as_deref());
        true
    }
}

#[cfg(not(any(target_os = "linux", target_os = "windows", target_os = "macos")))]
mod imp {
    pub fn set(_count: usize) -> bool {
        true
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn badge_has_a_red_disc_and_white_digits() {
        for count in [7, 42, 1234] {
            let px = super::render(count, 32);
            assert_eq!(px.len(), 32 * 32 * 4);
            let at = |x: usize, y: usize| &px[(y * 32 + x) * 4..(y * 32 + x) * 4 + 4];
            // The corner is outside the disc, the edge of the disc is red.
            assert_eq!(at(0, 0)[3], 0);
            assert!(at(3, 16)[0] > 200 && at(3, 16)[1] < 120, "{count}: {:?}", at(3, 16));
            let white = px.chunks_exact(4).filter(|p| p[0] > 230 && p[1] > 230 && p[2] > 230 && p[3] > 230).count();
            assert!(white > 20, "{count}: {white} white pixels");
        }
        assert_eq!(super::label(1234), "99+");
    }
}
