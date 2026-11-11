//! Logging in to websites with Telegram (`messages.requestUrlAuth`): login buttons under bot
//! messages and links to the domains the server lists in `url_auth_domains`.

use grammers_client::session::types::PeerId;
use grammers_client::tl;

use super::{Cmd, Ev, Worker};
use crate::i18n::lf;

/// Where a login request comes from.
#[derive(Debug, Clone, PartialEq)]
pub enum UrlAuthFrom {
    /// A `keyboardButtonUrlAuth` under a message.
    Button { peer: PeerId, msg: i32, button_id: i32 },
    /// A link to one of the `url_auth_domains`.
    Link,
}

/// The server asks whether to log the user in; everything shown comes from its answer.
#[derive(Debug, Clone)]
pub struct UrlAuthAsk {
    pub from: UrlAuthFrom,
    /// The plain address, opened when the user declines.
    pub url: String,
    pub domain: String,
    pub bot_name: String,
    /// The bot also asks to message the user.
    pub write_access: bool,
}

/// Request fields shared by `requestUrlAuth` and `acceptUrlAuth`: a button or a URL.
fn target(w: &Worker, from: &UrlAuthFrom, url: &str) -> Result<(Option<tl::enums::InputPeer>, Option<i32>, Option<i32>, Option<String>), String> {
    Ok(match from {
        UrlAuthFrom::Button { peer, msg, button_id } => (Some(w.peer_ref(*peer)?.into()), Some(*msg), Some(*button_id), None),
        UrlAuthFrom::Link => (None, None, None, Some(url.to_owned())),
    })
}

/// The string list under `key` in the app config.
fn config_strings(config: &tl::enums::Jsonvalue, key: &str) -> Vec<String> {
    let tl::enums::Jsonvalue::JsonObject(obj) = config else { return Vec::new() };
    let found = obj.value.iter().find_map(|tl::enums::JsonobjectValue::JsonObjectValue(kv)| (kv.key == key).then_some(&kv.value));
    let Some(tl::enums::Jsonvalue::JsonArray(arr)) = found else { return Vec::new() };
    arr.value
        .iter()
        .filter_map(|v| match v {
            tl::enums::Jsonvalue::JsonString(s) => Some(s.value.to_lowercase()),
            _ => None,
        })
        .collect()
}

impl Worker {
    pub(super) async fn url_auth_cmd(&mut self, cmd: Cmd) -> Result<(), String> {
        let client = self.client()?;
        match cmd {
            Cmd::LoadUrlAuthDomains => {
                let res = client.invoke(&tl::functions::help::GetAppConfig { hash: 0 }).await.map_err(|e| lf!("Налаштування сервера: {e}", "Server config: {e}"))?;
                if let tl::enums::help::AppConfig::Config(c) = res {
                    self.out.send(Ev::UrlAuthDomains(config_strings(&c.config, "url_auth_domains")));
                }
            }
            Cmd::RequestUrlAuth { from, url } => {
                let (peer, msg_id, button_id, link) = target(self, &from, &url)?;
                let req = tl::functions::messages::RequestUrlAuth { peer, msg_id, button_id, url: link, in_app_origin: None };
                match client.invoke(&req).await {
                    Ok(tl::enums::UrlAuthResult::Request(r)) => {
                        let bot_name = match &r.bot {
                            tl::enums::User::User(u) => [u.first_name.as_deref(), u.last_name.as_deref()].into_iter().flatten().collect::<Vec<_>>().join(" "),
                            tl::enums::User::Empty(_) => String::new(),
                        };
                        self.out.send(Ev::UrlAuthAsk(UrlAuthAsk { from, url, domain: r.domain, bot_name, write_access: r.request_write_access }));
                    }
                    Ok(tl::enums::UrlAuthResult::Accepted(a)) => self.out.send(Ev::UrlAuthOpen(a.url.unwrap_or(url))),
                    // No login offered: the button or link is an ordinary address.
                    Ok(tl::enums::UrlAuthResult::Default) => self.out.send(Ev::UrlAuthOpen(url)),
                    Err(e) => {
                        super::log_error(&format!("request url auth: {e}"));
                        self.out.send(Ev::UrlAuthOpen(url));
                    }
                }
            }
            Cmd::AcceptUrlAuth { from, url, write_allowed } => {
                let (peer, msg_id, button_id, link) = target(self, &from, &url)?;
                let req = tl::functions::messages::AcceptUrlAuth { write_allowed, share_phone_number: false, peer, msg_id, button_id, url: link, match_code: None };
                let res = client.invoke(&req).await.map_err(|e| lf!("Не вдалося увійти: {e}", "Could not log in: {e}"))?;
                // The signed address from the server; without one, the plain one.
                let open = match res {
                    tl::enums::UrlAuthResult::Accepted(a) => a.url.unwrap_or(url),
                    _ => url,
                };
                self.out.send(Ev::UrlAuthOpen(open));
            }
            _ => {}
        }
        Ok(())
    }
}
