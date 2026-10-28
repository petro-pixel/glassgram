//! Auto-deleting messages: the default timer for new chats (`account.setDefaultHistoryTTL`)
//! and a chat's own timer (`messages.setHistoryTTL`). The timer of an open chat comes with
//! `ChatDetails::ttl`.

use super::*;

impl Worker {
    pub(super) async fn autodelete_cmd(&mut self, cmd: Cmd) -> Result<(), String> {
        let client = self.client()?;
        let failed = |e: &dyn std::fmt::Display| lf!("Автовидалення: {e}", "Auto-delete: {e}");
        match cmd {
            Cmd::LoadDefaultTtl => {}
            Cmd::SetDefaultTtl(period) => {
                client.invoke(&tl::functions::messages::SetDefaultHistoryTtl { period }).await.map_err(|e| failed(&e))?;
            }
            Cmd::SetHistoryTtl { peer, period } => {
                let r = self.peer_ref(peer)?;
                client.invoke(&tl::functions::messages::SetHistoryTtl { peer: r.into(), period }).await.map_err(|e| failed(&e))?;
                self.out.send(Ev::HistoryTtl { peer, period });
                return Ok(());
            }
            _ => return Ok(()),
        }
        let tl::enums::DefaultHistoryTtl::Ttl(t) = client.invoke(&tl::functions::messages::GetDefaultHistoryTtl {}).await.map_err(|e| failed(&e))?;
        self.out.send(Ev::DefaultTtl(t.period));
        Ok(())
    }
}
