//! Mini apps (Web Apps): the server gives a signed URL for the bot's page; the page itself
//! runs in a separate WebView process (`webview.rs`), and its bridge calls come back here.

use grammers_client::session::types::PeerId;
use grammers_client::tl;

use super::{Cmd, Ev, Worker};
use crate::i18n::lf;

/// Platform name the server expects; it decides how the page treats the client.
const PLATFORM: &str = "tdesktop";

/// How a mini app is opened; each maps to its own `messages.request*WebView` call.
#[derive(Debug, Clone, PartialEq)]
pub enum WebAppKind {
    /// Inline button under a message (`requestWebView` with the button URL; the result can be a message).
    Inline(String),
    /// Reply keyboard button: the page may send data back with `sendData`.
    Simple(String),
    /// The bot's menu button (the URL from `botMenuButton`).
    Menu(String),
    /// The bot's main mini app (`t.me/bot?startapp`).
    Main { start_param: String },
    /// A named app (`t.me/bot/app`).
    Named { short_name: String, start_param: String },
}

/// An opened mini app, as the server returned it.
#[derive(Debug, Clone)]
pub struct WebAppOpened {
    pub peer: PeerId,
    pub bot: PeerId,
    pub kind: WebAppKind,
    pub url: String,
    /// Set for inline buttons: the session has to be prolonged while the page is open.
    pub query_id: Option<i64>,
}

fn json(data: &str) -> tl::enums::DataJson {
    tl::types::DataJson { data: data.to_owned() }.into()
}

impl Worker {
    pub(super) async fn webapp_cmd(&mut self, cmd: Cmd) -> Result<(), String> {
        let client = self.client()?;
        let out = self.out.clone();
        match cmd {
            Cmd::OpenWebApp { peer, bot, kind, theme } => self.open_web_app(peer, bot, kind, theme).await?,
            Cmd::OpenWebAppLink { username, app, start_param, theme } => {
                // A link opens the app in the bot's own chat.
                let bot = self.resolve(&username).await?;
                let kind = match app {
                    Some(short_name) => WebAppKind::Named { short_name, start_param },
                    None => WebAppKind::Main { start_param },
                };
                self.open_web_app(bot, bot, kind, theme).await?;
            }
            Cmd::ProlongWebApp { peer, bot, query_id } => {
                let (p, b) = (self.peer_ref(peer)?, self.peer_ref(bot)?);
                let req = tl::functions::messages::ProlongWebView { silent: false, peer: p.into(), bot: b.into(), query_id, reply_to: None, send_as: None };
                if let Err(e) = client.invoke(&req).await {
                    super::log_error(&format!("prolong web view: {e}"));
                }
            }
            Cmd::WebAppData { bot, button_text, data } => {
                let b = self.peer_ref(bot)?;
                let req = tl::functions::messages::SendWebViewData { bot: b.into(), random_id: jiff::Timestamp::now().as_nanosecond() as i64, button_text, data };
                client.invoke(&req).await.map_err(|e| lf!("Дані не надіслано: {e}", "Data not sent: {e}"))?;
            }
            Cmd::WebAppMethod { bot, req_id, method, params } => {
                let b = self.peer_ref(bot)?;
                tokio::spawn(async move {
                    let req = tl::functions::bots::InvokeWebViewCustomMethod { bot: b.into(), custom_method: method, params: json(&params) };
                    let result = match client.invoke(&req).await {
                        Ok(tl::enums::DataJson::Json(d)) => Ok(d.data),
                        Err(e) => Err(e.to_string()),
                    };
                    out.send(Ev::WebAppMethodResult { bot, req_id, result });
                });
            }
            _ => {}
        }
        Ok(())
    }

    /// Ask the server for the signed URL of a mini app and hand it to the UI.
    async fn open_web_app(&self, peer: PeerId, bot: PeerId, kind: WebAppKind, theme: String) -> Result<(), String> {
        let client = self.client()?;
        let p = self.peer_ref(peer)?;
        let b = self.peer_ref(bot)?;
        let theme = Some(json(&theme));
        let res = match &kind {
            WebAppKind::Inline(url) | WebAppKind::Menu(url) => {
                let req = tl::functions::messages::RequestWebView {
                    from_bot_menu: matches!(kind, WebAppKind::Menu(_)),
                    silent: false,
                    compact: false,
                    fullscreen: false,
                    peer: p.into(),
                    bot: b.into(),
                    url: Some(url.clone()),
                    start_param: None,
                    theme_params: theme,
                    platform: PLATFORM.into(),
                    reply_to: None,
                    send_as: None,
                };
                client.invoke(&req).await
            }
            WebAppKind::Simple(url) => {
                let req = tl::functions::messages::RequestSimpleWebView {
                    from_switch_webview: false,
                    from_side_menu: false,
                    compact: false,
                    fullscreen: false,
                    bot: b.into(),
                    url: Some(url.clone()),
                    start_param: None,
                    theme_params: theme,
                    platform: PLATFORM.into(),
                };
                client.invoke(&req).await
            }
            WebAppKind::Main { start_param } => {
                let req = tl::functions::messages::RequestMainWebView {
                    compact: false,
                    fullscreen: false,
                    peer: p.into(),
                    bot: b.into(),
                    start_param: (!start_param.is_empty()).then(|| start_param.clone()),
                    theme_params: theme,
                    platform: PLATFORM.into(),
                };
                client.invoke(&req).await
            }
            WebAppKind::Named { short_name, start_param } => {
                let app = tl::types::InputBotAppShortName { bot_id: b.into(), short_name: short_name.clone() }.into();
                let req = tl::functions::messages::RequestAppWebView {
                    write_allowed: false,
                    compact: false,
                    fullscreen: false,
                    peer: p.into(),
                    app,
                    start_param: (!start_param.is_empty()).then(|| start_param.clone()),
                    theme_params: theme,
                    platform: PLATFORM.into(),
                };
                client.invoke(&req).await
            }
        };
        let tl::enums::WebViewResult::Url(r) = res.map_err(|e| lf!("Міні-застосунок не відкрито: {e}", "Mini app not opened: {e}"))?;
        self.out.send(Ev::WebAppOpened(WebAppOpened { peer, bot, kind, url: r.url, query_id: r.query_id }));
        Ok(())
    }
}
