//! Mini apps from the main window's side: asks the server for the URL, starts the WebView
//! process (`webview.rs`) and answers the page's bridge calls that need the network.

use std::io::{BufRead, BufReader, Write};
use std::process::{Child, ChildStdin, Command, Stdio};

use super::*;
use crate::backend::{WebAppKind, WebAppOpened};
use crate::webview::{FromView, Open, Strings, ToView};

/// `prolongWebView` interval for inline-button sessions, as in Telegram Desktop.
const PROLONG: f64 = 60.0;

/// An open mini app window.
pub(super) struct WebAppWin {
    opened: WebAppOpened,
    /// Keyboard button text, for `sendWebViewData`.
    button_text: String,
    child: Child,
    stdin: ChildStdin,
    rx: mpsc::Receiver<FromView>,
    prolonged_at: f64,
}

/// "#rrggbb" for the page's `theme_params`.
fn hex(c: Color32) -> String {
    format!("#{:02x}{:02x}{:02x}", c.r(), c.g(), c.b())
}

/// `a` moved towards `b` by `t` (0…1), for a second background tone from the theme ones.
fn mix(a: Color32, b: Color32, t: f32) -> Color32 {
    let m = |x: u8, y: u8| (x as f32 + (y as f32 - x as f32) * t).round() as u8;
    Color32::from_rgb(m(a.r(), b.r()), m(a.g(), b.g()), m(a.b(), b.b()))
}

/// Our theme in Telegram's `theme_params` keys, so the page matches the client.
fn theme_json() -> String {
    let bg = glass::chrome();
    serde_json::json!({
        "bg_color": hex(bg),
        "secondary_bg_color": hex(mix(bg, ink(), 0.06)),
        "section_bg_color": hex(bg),
        "header_bg_color": hex(bg),
        "bottom_bar_bg_color": hex(bg),
        "text_color": hex(ink()),
        "hint_color": hex(ink_dim()),
        "subtitle_text_color": hex(ink_dim()),
        "section_header_text_color": hex(ACCENT),
        "link_color": hex(glass::link(false)),
        "accent_text_color": hex(ACCENT),
        "button_color": hex(ACCENT),
        "button_text_color": hex(glass::ON_DARK),
        "destructive_text_color": hex(glass::danger()),
    })
    .to_string()
}

impl WebAppWin {
    fn send(&mut self, msg: &ToView) {
        if let Ok(mut line) = serde_json::to_string(msg) {
            line.push('\n');
            let _ = self.stdin.write_all(line.as_bytes());
            let _ = self.stdin.flush();
        }
    }

    fn event(&mut self, kind: &str, data: serde_json::Value) {
        self.send(&ToView::Event { r#type: kind.to_owned(), data });
    }
}

impl App {
    /// A mini app button, menu button or link was used.
    pub(super) fn open_web_app(&mut self, peer: PeerId, bot: PeerId, kind: WebAppKind, button_text: String) {
        self.web_app_button = button_text;
        self.send(Cmd::OpenWebApp { peer, bot, kind, theme: theme_json() });
    }

    pub(super) fn open_web_app_link(&mut self, username: String, app: Option<String>, start_param: String) {
        self.web_app_button.clear();
        self.send(Cmd::OpenWebAppLink { username, app, start_param, theme: theme_json() });
    }

    pub(super) fn web_app_event(&mut self, ev: Ev) {
        match ev {
            Ev::WebAppOpened(opened) => {
                let button_text = std::mem::take(&mut self.web_app_button);
                if self.demo {
                    // No network in the demo: show where the page would open.
                    self.toast = Some((lf!("Міні-застосунок: {}", "Mini app: {}", opened.url), 0.0, false));
                    return;
                }
                match self.spawn_web_app(opened, button_text) {
                    Ok(w) => self.web_apps.push(w),
                    Err(e) => self.toast = Some((lf!("Не вдалося відкрити вікно міні-застосунку: {e}", "Could not open the mini app window: {e}"), 0.0, true)),
                }
            }
            Ev::BotMenuApp { peer, text, url } => {
                self.suggest_menu_app(peer, text, url);
            }
            Ev::WebAppMethodResult { bot, req_id, result } => {
                let data = match result {
                    Ok(json) => serde_json::json!({ "req_id": req_id, "result": serde_json::from_str::<serde_json::Value>(&json).unwrap_or(serde_json::Value::Null) }),
                    Err(e) => serde_json::json!({ "req_id": req_id, "error": e }),
                };
                if let Some(w) = self.web_apps.iter_mut().find(|w| w.opened.bot == bot) {
                    w.event("custom_method_invoked", data);
                }
            }
            _ => {}
        }
    }

    fn spawn_web_app(&self, opened: WebAppOpened, button_text: String) -> Result<WebAppWin, String> {
        let exe = std::env::current_exe().map_err(|e| e.to_string())?;
        let mut child = Command::new(exe).arg("--webview").stdin(Stdio::piped()).stdout(Stdio::piped()).spawn().map_err(|e| e.to_string())?;
        let (Some(mut stdin), Some(stdout)) = (child.stdin.take(), child.stdout.take()) else { return Err("no pipes".into()) };
        let title = self.chats.iter().find(|c| c.id == opened.bot).map_or_else(|| l!("Міні-застосунок", "Mini app").to_owned(), |c| c.title.clone());
        let strings = Strings { back: l!("Назад", "Back").into(), ok: "OK".into(), cancel: l!("Скасувати", "Cancel").into(), close: l!("Закрити", "Close").into() };
        let open = Open { url: opened.url.clone(), title, theme: theme_json(), strings };
        let line = serde_json::to_string(&open).map_err(|e| e.to_string())? + "\n";
        stdin.write_all(line.as_bytes()).map_err(|e| e.to_string())?;
        let (tx, rx) = mpsc::channel();
        let ctx = self.ctx.clone();
        std::thread::spawn(move || {
            for line in BufReader::new(stdout).lines() {
                let Ok(line) = line else { break };
                if let Ok(call) = serde_json::from_str::<FromView>(&line) {
                    let _ = tx.send(call);
                    ctx.request_repaint();
                }
            }
        });
        let now = self.ctx.input(|i| i.time);
        Ok(WebAppWin { opened, button_text, child, stdin, rx, prolonged_at: now })
    }

    /// Bridge calls from the windows, session upkeep and closed windows; every frame.
    pub(super) fn poll_web_apps(&mut self) {
        if self.web_apps.is_empty() {
            return;
        }
        let now = self.ctx.input(|i| i.time);
        let mut calls = Vec::new();
        let mut prolong = Vec::new();
        self.web_apps.retain_mut(|w| {
            while let Ok(c) = w.rx.try_recv() {
                calls.push((w.opened.clone(), w.button_text.clone(), c));
            }
            if let Some(query_id) = w.opened.query_id
                && now - w.prolonged_at > PROLONG
            {
                w.prolonged_at = now;
                prolong.push(Cmd::ProlongWebApp { peer: w.opened.peer, bot: w.opened.bot, query_id });
            }
            // The window was closed by the user.
            !matches!(w.child.try_wait(), Ok(Some(_)))
        });
        for cmd in prolong {
            self.send(cmd);
        }
        for (opened, button_text, call) in calls {
            self.web_app_call(opened, button_text, call);
        }
        self.ctx.request_repaint_after(Duration::from_secs(1));
    }

    fn web_app_call(&mut self, opened: WebAppOpened, button_text: String, call: FromView) {
        let bot = opened.bot;
        let str_of = |k: &str| call.data.get(k).and_then(|v| v.as_str()).unwrap_or_default().to_owned();
        let reply = |app: &mut App, kind: &str, data: serde_json::Value| {
            if let Some(w) = app.web_apps.iter_mut().find(|w| w.opened.bot == bot) {
                w.event(kind, data);
            }
        };
        match call.r#type.as_str() {
            "web_app_close" => self.close_web_app(bot),
            // Keyboard-button apps hand data to the bot and close, as in Telegram Desktop.
            "web_app_data_send" if matches!(opened.kind, WebAppKind::Simple(_)) => {
                self.send(Cmd::WebAppData { bot, button_text, data: str_of("data") });
                self.close_web_app(bot);
            }
            "web_app_invoke_custom_method" => {
                let params = call.data.get("params").map_or_else(|| "{}".to_owned(), |p| p.to_string());
                self.send(Cmd::WebAppMethod { bot, req_id: str_of("req_id"), method: str_of("method"), params });
            }
            "web_app_open_tg_link" => {
                // Without the leading slash `https://t.me@host/…` would point to another site.
                let path = str_of("path_full");
                if path.starts_with('/') {
                    self.apply(opened.peer, Action::Open(format!("https://t.me{path}")));
                }
            }
            // The clipboard may hold passwords and codes: a page doesn't get it without asking,
            // and `null` is the answer the API defines for "no access".
            "web_app_read_text_from_clipboard" => reply(self, "clipboard_text_received", serde_json::json!({ "req_id": str_of("req_id"), "data": null })),
            // Payments are not supported yet: tell the page right away.
            "web_app_open_invoice" => reply(self, "invoice_closed", serde_json::json!({ "slug": str_of("slug"), "status": "failed" })),
            _ => {}
        }
    }

    fn close_web_app(&mut self, bot: PeerId) {
        self.web_apps.retain_mut(|w| {
            if w.opened.bot != bot {
                return true;
            }
            w.send(&ToView::Close);
            false
        });
    }

    /// Close every mini app window (logout, account switch, quit).
    pub(super) fn close_web_apps(&mut self) {
        for w in &mut self.web_apps {
            w.send(&ToView::Close);
            let _ = w.child.kill();
        }
        self.web_apps.clear();
    }
}
