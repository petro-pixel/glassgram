//! Logging in to websites with Telegram: a "Log in" button or a link to one of the server's
//! `url_auth_domains` asks "Log in to <domain> as <name>?", as in Telegram Desktop. The domain
//! comes from the server's answer, never from the button, and nothing is accepted by itself.

use super::*;
use crate::backend::{UrlAuthAsk, UrlAuthFrom};

#[derive(Default)]
pub(super) struct UrlAuth {
    /// Hosts whose links ask the server first (lower case).
    domains: Vec<String>,
    requested: bool,
    /// The question on screen and its "allow messages" checkbox.
    ask: Option<(UrlAuthAsk, bool)>,
}

/// The lower-case host of an http(s) link.
fn host(url: &str) -> Option<String> {
    let rest = url.strip_prefix("https://").or_else(|| url.strip_prefix("http://"))?;
    let host = rest.split(['/', '?', '#']).next()?;
    // `user@host` must not pass for `user`.
    let host = host.rsplit('@').next()?.split(':').next()?;
    (!host.is_empty()).then(|| host.to_lowercase())
}

impl UrlAuth {
    /// A link that goes through `requestUrlAuth`.
    fn covers(&self, url: &str) -> bool {
        host(url).is_some_and(|h| self.domains.contains(&h))
    }
}

impl App {
    pub(super) fn url_auth_load(&mut self) {
        if !self.url_auth.requested {
            self.url_auth.requested = true;
            self.send(Cmd::LoadUrlAuthDomains);
        }
    }

    /// A link from a message: `false` when it isn't a login link and should open as usual.
    pub(super) fn url_auth_link(&mut self, url: &str) -> bool {
        if !self.url_auth.covers(url) {
            return false;
        }
        self.send(Cmd::RequestUrlAuth { from: UrlAuthFrom::Link, url: url.to_owned() });
        true
    }

    pub(super) fn url_auth_button(&mut self, peer: PeerId, msg: i32, button_id: i32, url: String) {
        self.send(Cmd::RequestUrlAuth { from: UrlAuthFrom::Button { peer, msg, button_id }, url });
    }

    pub(super) fn url_auth_event(&mut self, ev: Ev) {
        match ev {
            Ev::UrlAuthDomains(domains) => self.url_auth.domains = domains,
            Ev::UrlAuthAsk(ask) => {
                // The name in the question comes with the own profile.
                if self.profile.first.is_empty() {
                    self.send(Cmd::LoadProfile);
                }
                self.url_auth.ask = Some((ask, true));
            }
            Ev::UrlAuthOpen(url) => self.url_auth_open(&url),
            _ => {}
        }
    }

    fn url_auth_open(&mut self, url: &str) {
        if self.demo {
            // No browser from the demo: show where it would go.
            self.toast = Some((lf!("Відкрилося б: {url}", "Would open: {url}"), 0.0, false));
        } else {
            self.open_link(url);
        }
    }

    pub(super) fn url_auth_view(&mut self, ui: &mut Ui) {
        let Some((ask, write)) = &mut self.url_auth.ask else { return };
        let me = format!("{} {}", self.profile.first, self.profile.last).trim().to_owned();
        let domain = &ask.domain;
        let title = if me.is_empty() { lf!("Увійти на {domain}?", "Log in to {domain}?") } else { lf!("Увійти на {domain} як {me}?", "Log in to {domain} as {me}?") };
        let bot = &ask.bot_name;
        let body = lf!(
            "{bot} передасть сайту {domain} ваше ім'я, ім'я користувача й фото профілю. Номер телефону сайт не отримає.",
            "{bot} will give {domain} your name, username and profile photo. The site won't get your phone number."
        );
        let label = lf!("Дозволити {bot} надсилати мені повідомлення", "Allow {bot} to send me messages");
        let check = ask.write_access.then_some((label.as_str(), write));
        let buttons = [l!("Скасувати", "Cancel"), l!("Не входити", "Don't log in"), l!("Увійти", "Log in")];
        let Some(i) = confirm::ask_box(ui, "url-auth", &title, &body, check, &buttons) else { return };
        let Some((ask, write)) = self.url_auth.ask.take() else { return };
        match i {
            // Declined: the plain address, without logging in.
            1 => self.url_auth_open(&ask.url),
            2 => self.send(Cmd::AcceptUrlAuth { write_allowed: ask.write_access && write, from: ask.from, url: ask.url }),
            _ => {}
        }
    }
}

#[cfg(test)]
mod tests {
    use super::host;

    #[test]
    fn hosts() {
        assert_eq!(host("https://Example.org/login?x=1"), Some("example.org".into()));
        assert_eq!(host("http://example.org:8080"), Some("example.org".into()));
        assert_eq!(host("https://example.org@evil.com/"), Some("evil.com".into()));
        assert_eq!(host("mailto:a@example.org"), None);
        assert_eq!(host("https:///path"), None);
    }
}
