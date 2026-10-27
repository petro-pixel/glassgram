//! Mini app window: a separate process (`glassgram --webview`) with a system WebView
//! (WebView2 on Windows, WebKitGTK on Linux, WKWebView on macOS) — egui can't host one,
//! and Wayland doesn't allow embedding it into our window.
//!
//! Talks to the main process in JSON lines: stdin gets `{"open": …}` once, then `{"event": …}`
//! to pass into the page (`Telegram.WebView.receiveEvent`); stdout gets the page's bridge calls
//! that need the network or the main window. Everything visual (main / secondary / back buttons,
//! popups, viewport, theme) is handled inside the page by `BRIDGE_JS`.

use std::io::{BufRead, Write};

use serde::{Deserialize, Serialize};
use tao::event::{Event, WindowEvent};
use tao::event_loop::{ControlFlow, EventLoopBuilder};
use tao::window::WindowBuilder;
use wry::WebViewBuilder;

/// What the main process sends first.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Open {
    pub url: String,
    pub title: String,
    /// `theme_params` JSON for the page.
    pub theme: String,
    /// Strings for the window's own buttons and popups, in the interface language.
    pub strings: Strings,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Strings {
    pub back: String,
    pub ok: String,
    pub cancel: String,
    pub close: String,
}

/// A line from the main process after `Open`.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ToView {
    /// Pass an event into the page.
    Event { r#type: String, data: serde_json::Value },
    Close,
}

/// A bridge call from the page that the main process answers.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FromView {
    pub r#type: String,
    #[serde(default)]
    pub data: serde_json::Value,
}

enum User {
    Line(ToView),
    Eof,
    /// The page asked for the window to fill the screen, or to leave it.
    Fullscreen(bool),
}

/// Bridge calls handled here or in the page; everything else goes to the main process.
fn local(call: &FromView) -> bool {
    matches!(call.r#type.as_str(), "web_app_open_link" | "web_app_request_fullscreen" | "web_app_exit_fullscreen")
}

pub fn run() {
    let mut first = String::new();
    if std::io::stdin().read_line(&mut first).is_err() {
        return;
    }
    let Ok(open) = serde_json::from_str::<Open>(first.trim()) else { return };

    let event_loop = EventLoopBuilder::<User>::with_user_event().build();
    let proxy = event_loop.create_proxy();
    let window_proxy = event_loop.create_proxy();
    std::thread::spawn(move || {
        for line in std::io::stdin().lock().lines() {
            let Ok(line) = line else { break };
            if let Ok(msg) = serde_json::from_str::<ToView>(&line) {
                let _ = proxy.send_event(User::Line(msg));
            }
        }
        // The main process is gone: close with it.
        let _ = proxy.send_event(User::Eof);
    });

    let Ok(window) = WindowBuilder::new().with_title(&open.title).with_inner_size(tao::dpi::LogicalSize::new(420.0, 720.0)).build(&event_loop) else { return };
    let init = BRIDGE_JS.replace("__THEME__", &open.theme).replace("__STRINGS__", &serde_json::to_string(&open.strings).unwrap_or_else(|_| "{}".into()));
    let builder = WebViewBuilder::new()
        .with_url(&open.url)
        .with_initialization_script(&init)
        // Mini apps are web pages: never let one navigate the window to local files or app handlers.
        .with_navigation_handler(|url| ["https:", "http:", "about:", "blob:", "data:"].iter().any(|s| url.starts_with(s)))
        .with_ipc_handler(move |req| {
            let Ok(call) = serde_json::from_str::<FromView>(req.body()) else { return };
            if local(&call) {
                match call.r#type.as_str() {
                    "web_app_request_fullscreen" => drop(window_proxy.send_event(User::Fullscreen(true))),
                    "web_app_exit_fullscreen" => drop(window_proxy.send_event(User::Fullscreen(false))),
                    // The page chooses the link: web links only, as Telegram's `openLink` allows.
                    _ => {
                        if let Some(url) = call.data.get("url").and_then(|u| u.as_str()).filter(|u| u.starts_with("https://") || u.starts_with("http://")) {
                            crate::platform::open_url(url);
                        }
                    }
                }
                return;
            }
            let mut out = std::io::stdout().lock();
            let _ = serde_json::to_writer(&mut out, &call);
            let _ = out.write_all(b"\n");
            let _ = out.flush();
        });
    // On Linux the WebView lives in the window's GTK box: that works on Wayland as well as X11.
    #[cfg(target_os = "linux")]
    let webview = {
        use tao::platform::unix::WindowExtUnix;
        use wry::WebViewBuilderExtUnix;
        match window.default_vbox() {
            Some(vbox) => builder.build_gtk(vbox),
            None => return,
        }
    };
    #[cfg(not(target_os = "linux"))]
    let webview = builder.build(&window);
    let Ok(webview) = webview else { return };

    event_loop.run(move |event, _, flow| {
        *flow = ControlFlow::Wait;
        match event {
            Event::WindowEvent { event: WindowEvent::CloseRequested, .. } | Event::UserEvent(User::Line(ToView::Close) | User::Eof) => *flow = ControlFlow::Exit,
            Event::UserEvent(User::Line(ToView::Event { r#type, data })) => {
                let js = format!("window.__glassgram && window.__glassgram.receive({}, {})", serde_json::Value::String(r#type), data);
                let _ = webview.evaluate_script(&js);
            }
            Event::UserEvent(User::Fullscreen(on)) => {
                window.set_fullscreen(on.then_some(tao::window::Fullscreen::Borderless(None)));
                let js = format!("window.__glassgram && window.__glassgram.receive('fullscreen_changed', {{ is_fullscreen: {on} }})");
                let _ = webview.evaluate_script(&js);
            }
            _ => {}
        }
    });
}

/// Runs before the page's own scripts. Provides `TelegramWebviewProxy` (what telegram-web-app.js
/// calls on desktop) and draws the window's own controls in a closed shadow root, so the page's
/// styles can't touch them.
const BRIDGE_JS: &str = r#"
(() => {
  const theme = __THEME__;
  const t = __STRINGS__;
  const toHost = (type, data) => window.ipc.postMessage(JSON.stringify({ type, data: data ?? null }));
  const receive = (type, data) => {
    const wv = window.Telegram && window.Telegram.WebView;
    if (wv && wv.receiveEvent) wv.receiveEvent(type, data ?? null);
  };
  let ui = null, main = null, second = null, back = null, bar = null;
  const state = { main: {}, second: {} };
  const barHeight = () => (bar && bar.style.display !== 'none') ? bar.offsetHeight : 0;
  const viewport = () => receive('viewport_changed', { height: window.innerHeight - barHeight(), width: window.innerWidth, is_expanded: true, is_state_stable: true });

  const build = () => {
    if (ui) return;
    const host = document.createElement('div');
    host.style.cssText = 'position:fixed;inset:0;pointer-events:none;z-index:2147483647';
    document.documentElement.appendChild(host);
    ui = host.attachShadow({ mode: 'closed' });
    ui.innerHTML = `<style>
      :host{all:initial}
      *{box-sizing:border-box;font:500 15px system-ui,-apple-system,"Segoe UI",Ubuntu,sans-serif}
      .bar{position:fixed;left:0;right:0;bottom:0;display:none;gap:8px;padding:8px 12px;pointer-events:auto;background:${theme.bg_color||'#fff'}}
      .bar button{flex:1;height:44px;border:0;border-radius:12px;cursor:pointer}
      .bar button[disabled]{opacity:.5;cursor:default}
      .back{position:fixed;left:10px;top:10px;width:36px;height:36px;border-radius:18px;border:0;display:none;pointer-events:auto;cursor:pointer;
        background:rgba(0,0,0,.45);color:#fff;font-size:20px;line-height:36px}
      .veil{position:fixed;inset:0;background:rgba(0,0,0,.4);display:flex;align-items:center;justify-content:center;pointer-events:auto}
      .pop{width:min(320px,90vw);border-radius:16px;padding:18px;background:${theme.secondary_bg_color||theme.bg_color||'#fff'};color:${theme.text_color||'#000'}}
      .pop h3{margin:0 0 8px;font-size:17px}.pop p{margin:0 0 14px;white-space:pre-wrap;color:${theme.hint_color||'#777'}}
      .pop .row{display:flex;flex-direction:column;gap:6px}
      .pop button{height:40px;border:0;border-radius:10px;cursor:pointer;background:${theme.button_color||'#3390ec'};color:${theme.button_text_color||'#fff'}}
      .pop button.d{background:transparent;color:${theme.destructive_text_color||'#e53935'}}
      .pop button.c{background:transparent;color:${theme.link_color||'#3390ec'}}
    </style>
    <div class="bar"><button class="s"></button><button class="m"></button></div>
    <button class="back" title="${t.back}">‹</button>`;
    bar = ui.querySelector('.bar'); main = ui.querySelector('.m'); second = ui.querySelector('.s'); back = ui.querySelector('.back');
    main.onclick = () => receive('main_button_pressed');
    second.onclick = () => receive('secondary_button_pressed');
    back.onclick = () => receive('back_button_pressed');
    window.addEventListener('resize', viewport);
  };
  const paint = (btn, p, mainBtn) => {
    btn.style.display = p.is_visible ? '' : 'none';
    btn.textContent = p.is_progress_visible ? '…' : (p.text || '');
    btn.disabled = p.is_active === false;
    btn.style.background = p.color || (mainBtn ? (theme.button_color || '#3390ec') : (theme.secondary_bg_color || '#eee'));
    btn.style.color = p.text_color || (mainBtn ? (theme.button_text_color || '#fff') : (theme.button_color || '#3390ec'));
    btn.style.order = (!mainBtn && p.position === 'right') ? 2 : 0;
  };
  const layout = () => {
    bar.style.display = (state.main.is_visible || state.second.is_visible) ? 'flex' : 'none';
    viewport();
  };
  const popup = (p) => {
    const veil = document.createElement('div');
    veil.className = 'veil';
    const box = document.createElement('div');
    box.className = 'pop';
    if (p.title) { const h = document.createElement('h3'); h.textContent = p.title; box.appendChild(h); }
    const m = document.createElement('p'); m.textContent = p.message || ''; box.appendChild(m);
    const row = document.createElement('div'); row.className = 'row';
    for (const b of (p.buttons && p.buttons.length ? p.buttons : [{ id: '', type: 'close' }])) {
      const e = document.createElement('button');
      e.textContent = b.text || ({ ok: t.ok, close: t.close, cancel: t.cancel }[b.type] || t.ok);
      if (b.type === 'destructive') e.className = 'd';
      if (b.type === 'cancel' || b.type === 'close') e.className = 'c';
      e.onclick = () => { veil.remove(); receive('popup_closed', { button_id: b.id }); };
      row.appendChild(e);
    }
    box.appendChild(row); veil.appendChild(box); ui.appendChild(veil);
  };

  // Calls the window answers by itself; the rest go to the host.
  const local = {
    web_app_ready: () => {},
    web_app_expand: () => viewport(),
    web_app_request_viewport: () => viewport(),
    web_app_request_theme: () => receive('theme_changed', { theme_params: theme }),
    web_app_request_safe_area: () => receive('safe_area_changed', { top: 0, bottom: 0, left: 0, right: 0 }),
    web_app_request_content_safe_area: () => receive('content_safe_area_changed', { top: 0, bottom: 0, left: 0, right: 0 }),
    web_app_setup_main_button: (p) => { build(); state.main = p || {}; paint(main, state.main, true); layout(); },
    web_app_setup_secondary_button: (p) => { build(); state.second = p || {}; paint(second, state.second, false); layout(); },
    web_app_setup_back_button: (p) => { build(); back.style.display = p && p.is_visible ? 'block' : 'none'; },
    web_app_open_popup: (p) => { build(); popup(p || {}); },
    web_app_set_header_color: () => {},
    web_app_set_background_color: () => {},
    web_app_set_bottom_bar_color: (p) => { build(); if (p && p.color) bar.style.background = p.color; },
    web_app_setup_closing_behavior: () => {},
    web_app_setup_swipe_behavior: () => {},
    web_app_trigger_haptic_feedback: () => {},
    web_app_check_home_screen: () => receive('home_screen_checked', { status: 'unsupported' }),
    // A desktop has no location, biometry or camera scanner for pages, as in Telegram Desktop.
    web_app_check_location: () => receive('location_checked', { available: false }),
    web_app_request_location: () => receive('location_requested', { available: false }),
    web_app_biometry_get_info: () => receive('biometry_info_received', { available: false }),
    web_app_biometry_request_access: () => receive('biometry_info_received', { available: false }),
    web_app_open_scan_qr_popup: () => receive('scan_qr_popup_closed', {}),
  };
  window.TelegramWebviewProxy = {
    postEvent(type, json) {
      let data = null;
      try { data = json ? JSON.parse(json) : null; } catch (e) {}
      if (local[type]) local[type](data); else toHost(type, data);
    },
  };
  window.__glassgram = { receive };
  if (document.readyState === 'loading') document.addEventListener('DOMContentLoaded', build); else build();
})();
"#;
