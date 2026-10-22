//! Експорт історії чату, як у Telegram Desktop: `messages.html` для читання
//! і `result.json` для програм; за бажанням — фото й файли в теці `files/`.
//! Шляхи й відкриття теки — через `dirs` і `open`, тож працює на будь-якій ОС.

use std::fmt::Write as _;
use std::path::{Path, PathBuf};

use grammers_client::Client;
use grammers_client::media::Media;
use grammers_client::session::types::PeerRef;

use crate::i18n::{l, lf};

/// Одне повідомлення в експорті.
struct Entry {
    id: i32,
    date: String,
    from: String,
    text: String,
    reply_to: Option<i32>,
    /// Шлях до файлу відносно теки експорту.
    file: Option<String>,
    /// Позначка вкладення, якщо файл не збережено.
    media: Option<&'static str>,
}

/// Тека для нового експорту: «Завантаження/Glassgram Export/<назва> <дата>».
pub fn target_dir(title: &str) -> PathBuf {
    let base = dirs::download_dir().or_else(dirs::home_dir).unwrap_or_else(|| PathBuf::from("."));
    let safe: String = title.chars().map(|c| if c.is_alphanumeric() || " -_.".contains(c) { c } else { '_' }).collect();
    let stamp = jiff::Zoned::now().strftime("%Y-%m-%d %H-%M").to_string();
    base.join("Glassgram Export").join(format!("{} {stamp}", safe.trim()))
}

/// Вивантажує всю історію. `progress` викликається кожні 100 повідомлень.
pub async fn run(client: &Client, r: PeerRef, title: &str, with_media: bool, dir: &Path, mut progress: impl FnMut(usize)) -> Result<usize, String> {
    let files = dir.join("files");
    std::fs::create_dir_all(&files).map_err(|e| e.to_string())?;
    let mut entries = Vec::new();
    let mut iter = client.iter_messages(r);
    while let Some(m) = iter.next().await.map_err(|e| lf!("Експорт: {e}", "Export: {e}"))? {
        let date = jiff::Timestamp::from_second(m.date().as_second()).unwrap_or_default().to_zoned(jiff::tz::TimeZone::system());
        let media = m.media();
        let label = media.as_ref().map(|md| match md {
            Media::Photo(_) => l!("Фото", "Photo"),
            Media::Sticker(_) => l!("Стікер", "Sticker"),
            Media::Document(_) => l!("Файл", "File"),
            _ => l!("Вкладення", "Attachment"),
        });
        let mut file = None;
        if with_media && let Some(md) = &media {
            let name = match md {
                Media::Photo(_) => Some(format!("photo_{}.jpg", m.id())),
                Media::Document(d) => Some(format!("{}_{}", m.id(), d.name().filter(|n| !n.is_empty()).unwrap_or("file"))),
                _ => None,
            };
            if let Some(name) = name {
                let path = files.join(&name);
                if client.download_media(md, &path).await.is_ok() {
                    file = Some(format!("files/{name}"));
                }
            }
        }
        entries.push(Entry {
            id: m.id(),
            date: date.strftime("%Y-%m-%d %H:%M:%S").to_string(),
            from: m.sender().and_then(|s| s.name().map(str::to_owned)).unwrap_or_default(),
            text: m.text().to_owned(),
            reply_to: m.reply_to_message_id(),
            media: if file.is_none() { label } else { None },
            file,
        });
        if entries.len() % 100 == 0 {
            progress(entries.len());
        }
    }
    entries.reverse();
    std::fs::write(dir.join("result.json"), json(title, &entries)).map_err(|e| e.to_string())?;
    std::fs::write(dir.join("messages.html"), html(title, &entries)).map_err(|e| e.to_string())?;
    Ok(entries.len())
}

fn json_str(s: &str) -> String {
    let mut out = String::with_capacity(s.len() + 2);
    out.push('"');
    for c in s.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            c if (c as u32) < 0x20 => {
                let _ = write!(out, "\\u{:04x}", c as u32);
            }
            c => out.push(c),
        }
    }
    out.push('"');
    out
}

fn json(title: &str, entries: &[Entry]) -> String {
    let mut out = format!("{{\n  \"name\": {},\n  \"messages\": [\n", json_str(title));
    for (k, e) in entries.iter().enumerate() {
        let _ = write!(out, "    {{\"id\": {}, \"date\": {}, \"from\": {}, \"text\": {}", e.id, json_str(&e.date), json_str(&e.from), json_str(&e.text));
        if let Some(r) = e.reply_to {
            let _ = write!(out, ", \"reply_to_message_id\": {r}");
        }
        if let Some(f) = &e.file {
            let _ = write!(out, ", \"file\": {}", json_str(f));
        } else if let Some(m) = e.media {
            let _ = write!(out, ", \"media\": {}", json_str(m));
        }
        out.push('}');
        out.push_str(if k + 1 < entries.len() { ",\n" } else { "\n" });
    }
    out.push_str("  ]\n}\n");
    out
}

fn esc(s: &str) -> String {
    s.replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;").replace('"', "&quot;")
}

fn html(title: &str, entries: &[Entry]) -> String {
    let mut out = format!(
        "<!doctype html><html><head><meta charset=\"utf-8\"><meta name=\"viewport\" content=\"width=device-width\"><title>{t}</title><style>\
         body{{font:15px/1.45 system-ui,sans-serif;max-width:760px;margin:24px auto;padding:0 16px;background:#f3f5fb;color:#161a2e}}\
         h1{{font-size:22px}}.m{{background:#fff;border-radius:14px;padding:10px 14px;margin:8px 0;box-shadow:0 1px 3px #0001}}\
         .h{{font-size:13px;color:#5c6380;margin-bottom:4px}}.f{{font-weight:600;color:#3655d8}}.t{{white-space:pre-wrap}}\
         .r{{font-size:13px;color:#5c6380}}img{{max-width:100%;border-radius:10px;margin-top:6px}}\
         @media(prefers-color-scheme:dark){{body{{background:#0d1024;color:#eef0fb}}.m{{background:#1a1f3d}}.h,.r{{color:#a6addb}}.f{{color:#9bb0ff}}}}\
         </style></head><body><h1>{t}</h1>\n",
        t = esc(title)
    );
    for e in entries {
        let _ = write!(out, "<div class=\"m\" id=\"m{}\"><div class=\"h\"><span class=\"f\">{}</span> · {}</div>", e.id, esc(&e.from), esc(&e.date));
        if let Some(r) = e.reply_to {
            let _ = write!(out, "<div class=\"r\"><a href=\"#m{r}\">↩ {}</a></div>", l!("відповідь", "reply"));
        }
        if !e.text.is_empty() {
            let _ = write!(out, "<div class=\"t\">{}</div>", esc(&e.text));
        }
        match (&e.file, e.media) {
            (Some(f), _) if f.ends_with(".jpg") => {
                let _ = write!(out, "<img src=\"{}\" loading=\"lazy\">", esc(f));
            }
            (Some(f), _) => {
                let _ = write!(out, "<div><a href=\"{0}\">📎 {0}</a></div>", esc(f));
            }
            (None, Some(m)) => {
                let _ = write!(out, "<div class=\"r\">[{m}]</div>");
            }
            _ => {}
        }
        out.push_str("</div>\n");
    }
    out.push_str("</body></html>\n");
    out
}

#[cfg(test)]
mod tests {
    #[test]
    fn writes_html_and_json() {
        let e = |id, text: &str, file: Option<&str>| super::Entry {
            id,
            date: "2026-10-22 12:00:00".into(),
            from: "Олена".into(),
            text: text.into(),
            reply_to: (id == 2).then_some(1),
            file: file.map(Into::into),
            media: None,
        };
        let entries = [e(1, "Привіт <b>", None), e(2, "Фото", Some("files/photo_2.jpg"))];
        let html = super::html("Чат", &entries);
        assert!(html.contains("Привіт &lt;b&gt;") && html.contains("<img src=\"files/photo_2.jpg\"") && html.contains("href=\"#m1\""));
        let json = super::json("Чат", &entries);
        assert!(json.contains("\"reply_to_message_id\": 1") && json.contains("\"file\": \"files/photo_2.jpg\""));
    }

    #[test]
    fn json_escapes_control_chars() {
        assert_eq!(super::json_str("a\"b\n\u{1}"), "\"a\\\"b\\n\\u0001\"");
    }
}
