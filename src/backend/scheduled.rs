//! Scheduled messages: rescheduling, sending now, deleting.

use super::*;

impl Worker {
    pub(super) async fn scheduled_cmd(&mut self, cmd: Cmd) -> Result<(), String> {
        match cmd {
            Cmd::SendScheduledNow { peer, ids } => {
                let client = self.client()?;
                let r = self.peer_ref(peer)?;
                let n = ids.len();
                client
                    .invoke(&tl::functions::messages::SendScheduledMessages { peer: r.into(), id: ids })
                    .await
                    .map_err(|e| lf!("Не надіслано: {e}", "Not sent: {e}"))?;
                self.emit_latest(peer, n).await;
                self.load_scheduled(peer).await
            }
            Cmd::DeleteScheduled { peer, ids } => {
                let client = self.client()?;
                let r = self.peer_ref(peer)?;
                client
                    .invoke(&tl::functions::messages::DeleteScheduledMessages { peer: r.into(), id: ids })
                    .await
                    .map_err(|e| lf!("Не видалено: {e}", "Not deleted: {e}"))?;
                self.load_scheduled(peer).await
            }
            Cmd::Reschedule { peer, id, at } => {
                let client = self.client()?;
                let r = self.peer_ref(peer)?;
                let req = tl::functions::messages::EditMessage {
                    no_webpage: false,
                    invert_media: false,
                    peer: r.into(),
                    id,
                    message: None,
                    media: None,
                    reply_markup: None,
                    entities: None,
                    schedule_date: Some(at as i32),
                    schedule_repeat_period: None,
                    quick_reply_shortcut_id: None,
                    rich_message: None,
                };
                client.invoke(&req).await.map_err(|e| lf!("Не вдалося змінити час: {e}", "Could not change the time: {e}"))?;
                self.load_scheduled(peer).await
            }
            _ => Ok(()),
        }
    }
}
