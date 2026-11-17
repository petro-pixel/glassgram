//! Demo account export: the demo chats of the chosen kinds go through the real HTML/JSON writers
//! into a fresh owner-only temp folder (never the real Downloads), with progress after each chat.

use crate::app::App;
use crate::backend::{ChatKind, Cmd, Ev, TakeoutEv, TakeoutOptions, TakeoutProgress};
use crate::export::{self, Entry};
use crate::i18n::{l, lp};

pub fn handle(app: &mut App, cmd: Cmd) {
    match cmd {
        Cmd::StartTakeout(o) => {
            let ev = match write(app, &o) {
                Ok((dir, progress)) => TakeoutEv::Done { dir, progress },
                Err(e) => TakeoutEv::Failed(e),
            };
            app.push_event(Ev::Takeout(ev));
        }
        Cmd::CancelTakeout => app.push_event(Ev::Takeout(TakeoutEv::Cancelled)),
        _ => {}
    }
}

fn wanted(o: &TakeoutOptions, kind: ChatKind) -> bool {
    match kind {
        ChatKind::User | ChatKind::Saved => o.personal,
        ChatKind::Bot => o.bots,
        ChatKind::Group => o.private_groups || o.public_groups,
        ChatKind::Channel => o.private_channels || o.public_channels,
    }
}

fn write(app: &mut App, o: &TakeoutOptions) -> Result<(std::path::PathBuf, TakeoutProgress), String> {
    let dir = crate::platform::temp_file("Telegram Desktop Export");
    std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    crate::backend::make_private(&dir);
    let mut index = Vec::new();
    if o.info {
        let row = vec![(l!("Ім'я", "First name"), "Demo".to_owned()), (l!("Ім'я користувача", "Username"), "@glassgram_demo".to_owned())];
        export::write_table(&dir, "personal_information", l!("Інформація про акаунт", "Personal information"), &[row], o.format)?;
        index.push((l!("Інформація про акаунт", "Personal information").to_owned(), "personal_information".to_owned(), String::new()));
    }
    let chats: Vec<(String, Vec<Entry>)> = app
        .demo_history()
        .filter(|(_, (_, kind), _)| wanted(o, *kind))
        .map(|(_, (title, _), msgs)| {
            let entries = msgs
                .iter()
                .map(|m| Entry {
                    id: m.id,
                    date: export::date(m.ts),
                    from: if m.out { "Demo".to_owned() } else { m.sender.clone() },
                    text: m.text.clone(),
                    reply_to: m.reply_to,
                    file: None,
                    media: (m.photo || m.media.is_some()).then_some(l!("Вкладення", "Attachment")),
                })
                .collect();
            (title.to_owned(), entries)
        })
        .collect();
    let mut p = TakeoutProgress { chats_total: chats.len(), ..Default::default() };
    for (k, (title, entries)) in chats.iter().enumerate() {
        let rel = format!("chats/chat_{:03}", k + 1);
        std::fs::create_dir_all(dir.join(&rel)).map_err(|e| e.to_string())?;
        export::write_chat(&dir.join(&rel), title, entries, o.format)?;
        p.messages += entries.len();
        p.chats_done = k + 1;
        p.step = title.clone();
        app.push_event(Ev::Takeout(TakeoutEv::Progress(p.clone())));
        let page = if o.format.html() { "messages" } else { "result" };
        index.push((title.clone(), format!("{rel}/{page}"), lp!(entries.len(), ["{n} повідомлення", "{n} повідомлення", "{n} повідомлень"], ["{n} message", "{n} messages"])));
    }
    export::write_index(&dir, l!("Експортовані дані", "Exported Data"), &index, o.format)?;
    Ok((dir, p))
}
