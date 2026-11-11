//! Checklists (`messageMediaToDo`): creating them (`inputMediaTodo`), ticking items
//! (`messages.toggleTodoCompleted`) and adding items (`messages.appendTodoList`).

use super::*;

/// A checklist from the "New checklist" box.
#[derive(Debug, Clone, Default)]
pub struct NewTodo {
    pub title: String,
    pub items: Vec<String>,
    pub others_can_append: bool,
    pub others_can_complete: bool,
}

fn text(s: &str) -> tl::enums::TextWithEntities {
    tl::types::TextWithEntities { text: s.trim().to_owned(), entities: Vec::new() }.into()
}

fn item(id: i32, title: &str) -> tl::enums::TodoItem {
    tl::types::TodoItem { id, title: text(title) }.into()
}

/// Checklists are a Premium feature: the bare server code says nothing to the user.
fn explain(e: &dyn std::fmt::Display) -> String {
    let e = e.to_string();
    if e.contains("PREMIUM_ACCOUNT_REQUIRED") {
        l!("Чеклисти доступні лише з Telegram Premium", "Checklists need Telegram Premium").to_owned()
    } else {
        lf!("Чеклист: {e}", "Checklist: {e}")
    }
}

/// The checklist of a message, with who ticked what.
pub(super) fn todo_info(raw: &tl::enums::Message, names: &Names) -> Option<TodoInfo> {
    let tl::enums::Message::Message(m) = raw else { return None };
    let Some(tl::enums::MessageMedia::ToDo(media)) = &m.media else { return None };
    let tl::enums::TodoList::List(list) = &media.todo;
    let plain = |t: &tl::enums::TextWithEntities| {
        let tl::enums::TextWithEntities::Entities(t) = t;
        t.text.clone()
    };
    let done: Vec<&tl::types::TodoCompletion> = media.completions.iter().flatten().map(|tl::enums::TodoCompletion::Completion(c)| c).collect();
    let names = names.lock().unwrap();
    let items = list
        .list
        .iter()
        .map(|tl::enums::TodoItem::Item(i)| TodoTask {
            id: i.id,
            title: plain(&i.title),
            done: done.iter().find(|c| c.id == i.id).map(|c| {
                let who = names.get(&PeerId::from(&c.completed_by)).cloned().unwrap_or_else(|| l!("Учасник", "Member").to_owned());
                (who, c.date as i64)
            }),
        })
        .collect();
    Some(TodoInfo { title: plain(&list.title), items, others_can_append: list.others_can_append, others_can_complete: list.others_can_complete })
}

impl Worker {
    pub(super) async fn todo_cmd(&mut self, cmd: Cmd) -> Result<(), String> {
        let client = self.client()?;
        match cmd {
            Cmd::SendTodo { peer, todo, reply_to, send_as } => {
                let r = self.peer_ref(peer)?;
                let send_as = self.send_as_ref(send_as)?;
                let list = tl::types::TodoList {
                    others_can_append: todo.others_can_append,
                    others_can_complete: todo.others_can_complete,
                    title: text(&todo.title),
                    list: todo.items.iter().enumerate().map(|(i, t)| item(i as i32 + 1, t)).collect(),
                };
                let media = tl::types::InputMediaTodo { todo: list.into() };
                let local = ReplyTo::local(&reply_to);
                let reply = reply_to.as_ref().map(|x| self.input_reply(x)).transpose()?;
                let rid = jiff::Timestamp::now().as_nanosecond() as i64;
                let sent = reply::send_raw(&client, r, "", Some(media.into()), reply, SendOpts::default(), send_as, rid).await.map_err(|e| explain(&e))?;
                if let Some(m) = sent {
                    self.sent().emit(peer, &m, local, SendOpts::default());
                }
            }
            Cmd::ToggleTodo { peer, id, completed, incompleted } => {
                let r = self.peer_ref(peer)?;
                let req = tl::functions::messages::ToggleTodoCompleted { peer: r.into(), msg_id: id, completed, incompleted };
                let res = client.invoke(&req).await.map_err(|e| explain(&e));
                // Reload either way: on failure this undoes the optimistic tick.
                self.refresh_message(&client, peer, r, id).await;
                res?;
            }
            Cmd::AppendTodo { peer, id, items } => {
                let r = self.peer_ref(peer)?;
                let list = items.iter().map(|(i, t)| item(*i, t)).collect();
                let res = client.invoke(&tl::functions::messages::AppendTodoList { peer: r.into(), msg_id: id, list }).await.map_err(|e| explain(&e));
                self.refresh_message(&client, peer, r, id).await;
                res?;
            }
            _ => {}
        }
        Ok(())
    }

    /// Fetch one message again and replace it in the UI.
    pub(super) async fn refresh_message(&self, client: &Client, peer: PeerId, r: PeerRef, id: i32) {
        if let Ok(msgs) = client.get_messages_by_id(r, &[id]).await
            && let Some(Some(m)) = msgs.into_iter().next()
        {
            let mut msg = convert(&m, &self.names);
            msg.peer = peer;
            self.out.send(Ev::Edited(msg));
        }
    }
}
