//! Websites we logged in to with Telegram (`account.getWebAuthorizations`) and logging out of them.

use super::*;

/// A website session: the site, its login bot, and where it was last used.
#[derive(Debug, Clone)]
pub struct WebSession {
    pub hash: i64,
    pub domain: String,
    /// The bot the site logs in through.
    pub bot: String,
    /// Browser and platform.
    pub device: String,
    /// Region and IP.
    pub place: String,
    pub active: String,
}

impl Worker {
    pub(super) async fn websites_cmd(&mut self, cmd: Cmd) -> Result<(), String> {
        let client = self.client()?;
        match cmd {
            Cmd::LoadWebsites => {}
            Cmd::LogoutWebsite(hash) => {
                client.invoke(&tl::functions::account::ResetWebAuthorization { hash }).await.map_err(|e| lf!("Не вдалося відключити: {e}", "Could not disconnect: {e}"))?;
            }
            Cmd::LogoutAllWebsites => {
                client.invoke(&tl::functions::account::ResetWebAuthorizations {}).await.map_err(|e| lf!("Не вдалося відключити: {e}", "Could not disconnect: {e}"))?;
            }
            _ => return Ok(()),
        }
        let tl::enums::account::WebAuthorizations::Authorizations(res) =
            client.invoke(&tl::functions::account::GetWebAuthorizations {}).await.map_err(|e| lf!("Сайти: {e}", "Websites: {e}"))?;
        let bots: HashMap<i64, String> = res
            .users
            .iter()
            .filter_map(|u| match u {
                tl::enums::User::User(u) => Some((u.id, [u.first_name.clone(), u.last_name.clone()].into_iter().flatten().collect::<Vec<_>>().join(" "))),
                _ => None,
            })
            .collect();
        let list = res
            .authorizations
            .into_iter()
            .map(|tl::enums::WebAuthorization::Authorization(a)| {
                let active = jiff::Timestamp::from_second(a.date_active as i64).unwrap_or_default();
                let join = |parts: [String; 2]| parts.into_iter().filter(|s| !s.is_empty()).collect::<Vec<_>>().join(" · ");
                WebSession {
                    hash: a.hash,
                    domain: a.domain,
                    bot: bots.get(&a.bot_id).cloned().unwrap_or_default(),
                    device: join([a.browser, a.platform]),
                    place: join([a.region, a.ip]),
                    active: format!("{} {}", day(active), clock(active)),
                }
            })
            .collect();
        self.out.send(Ev::Websites(list));
        Ok(())
    }
}
