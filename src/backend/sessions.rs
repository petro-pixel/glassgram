//! Active sessions (Devices): the list and ending sessions.

use super::*;

impl Worker {
    pub(super) async fn sessions_cmd(&mut self, cmd: Cmd) -> Result<(), String> {
        match cmd {
            Cmd::LoadSessions => {
                let client = self.client()?;
                let tl::enums::account::Authorizations::Authorizations(res) =
                    client.invoke(&tl::functions::account::GetAuthorizations {}).await.map_err(|e| lf!("Сеанси: {e}", "Sessions: {e}"))?;
                let mut list: Vec<Session> = res
                    .authorizations
                    .into_iter()
                    .map(|tl::enums::Authorization::Authorization(a)| {
                        let active = jiff::Timestamp::from_second(a.date_active as i64).unwrap_or_default();
                        Session {
                            hash: a.hash,
                            app: format!("{} {}", a.app_name, a.app_version).trim().to_owned(),
                            device: [a.device_model, a.platform, a.system_version].into_iter().filter(|s| !s.is_empty()).collect::<Vec<_>>().join(" · "),
                            place: [a.country, a.ip].into_iter().filter(|s| !s.is_empty()).collect::<Vec<_>>().join(" · "),
                            active: if a.current { l!("зараз", "now").to_owned() } else { format!("{} {}", day(active), clock(active)) },
                            current: a.current,
                        }
                    })
                    .collect();
                list.sort_by_key(|s| !s.current);
                self.out.send(Ev::Sessions(list));
                Ok(())
            }
            Cmd::TerminateSession(hash) => {
                let client = self.client()?;
                client.invoke(&tl::functions::account::ResetAuthorization { hash }).await.map_err(|e| lf!("Сеанс не завершено: {e}", "Session not terminated: {e}"))?;
                let _ = self.back.send(Cmd::LoadSessions);
                Ok(())
            }
            Cmd::TerminateOtherSessions => {
                let client = self.client()?;
                client.invoke(&tl::functions::auth::ResetAuthorizations {}).await.map_err(|e| lf!("Сеанси не завершено: {e}", "Sessions not terminated: {e}"))?;
                let _ = self.back.send(Cmd::LoadSessions);
                Ok(())
            }
            _ => Ok(()),
        }
    }
}
