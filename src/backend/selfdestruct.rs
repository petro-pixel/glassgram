//! "Delete my account if away for…" (`account.getAccountTTL` / `setAccountTTL`).

use super::*;

impl Worker {
    pub(super) async fn self_destruct_cmd(&mut self, cmd: Cmd) -> Result<(), String> {
        let client = self.client()?;
        let failed = |e: &dyn std::fmt::Display| lf!("Самознищення акаунта: {e}", "Account self-destruct: {e}");
        match cmd {
            Cmd::LoadAccountTtl => {}
            Cmd::SetAccountTtl(days) => {
                let ttl = tl::types::AccountDaysTtl { days }.into();
                client.invoke(&tl::functions::account::SetAccountTtl { ttl }).await.map_err(|e| failed(&e))?;
            }
            _ => return Ok(()),
        }
        let tl::enums::AccountDaysTtl::Ttl(t) = client.invoke(&tl::functions::account::GetAccountTtl {}).await.map_err(|e| failed(&e))?;
        self.out.send(Ev::AccountTtl(t.days));
        Ok(())
    }
}
