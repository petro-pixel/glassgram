//! Chat history export, as in Telegram Desktop: `messages.html` for reading
//! and `result.json` for programs; optionally photos and files in the `files/` folder.
//! The writers are shared with the full account export (`backend/takeout.rs`).
//! Paths and opening the folder go through `dirs` and `open`, so it works on any OS.

use std::fmt::Write as _;
use std::path::{Path, PathBuf};

use grammers_client::Client;
use grammers_client::media::Media;
use grammers_client::session::types::PeerRef;

use crate::i18n::{l, lf};

/// One message in the export.
pub struct Entry {
    pub id: i32,
    pub date: String,
    pub from: String,
    pub text: String,
    pub reply_to: Option<i32>,
    /// File path relative to the export folder.
    pub file: Option<String>,
    /// Attachment label, if the file was not saved.
    pub media: Option<&'static str>,
}

/// Which files an export writes, as Telegram Desktop's "Format" choice.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Format {
    /// `messages.html` for reading.
    #[default]
    Html,
    /// `result.json` for programs.
    Json,
    Both,
}

impl Format {
    pub fn html(self) -> bool {
        self != Format::Json
    }

    pub fn json(self) -> bool {
        self != Format::Html
    }
}

/// "2026-10-22 12:00:00" in the local zone, as dates are written in the export.
pub fn date(unix: i64) -> String {
    jiff::Timestamp::from_second(unix).unwrap_or_default().to_zoned(jiff::tz::TimeZone::system()).strftime("%Y-%m-%d %H:%M:%S").to_string()
}

/// Writes one chat's messages (oldest first) into `dir`.
pub fn write_chat(dir: &Path, title: &str, entries: &[Entry], format: Format) -> Result<(), String> {
    if format.json() {
        std::fs::write(dir.join("result.json"), json(title, entries)).map_err(|e| e.to_string())?;
    }
    if format.html() {
        std::fs::write(dir.join("messages.html"), html(title, entries)).map_err(|e| e.to_string())?;
    }
    Ok(())
}

/// A list of records (contacts, sessions, account info) as `<stem>.json` / `<stem>.html`:
/// every row is (field, value) pairs, the same fields in every row.
pub fn write_table(dir: &Path, stem: &str, title: &str, rows: &[Vec<(&str, String)>], format: Format) -> Result<(), String> {
    if format.json() {
        std::fs::write(dir.join(format!("{stem}.json")), table_json(title, rows)).map_err(|e| e.to_string())?;
    }
    if format.html() {
        let mut out = head(title);
        out.push_str("<table>");
        if let Some(first) = rows.first() {
            out.push_str("<tr>");
            for (f, _) in first {
                let _ = write!(out, "<th>{}</th>", esc(f));
            }
            out.push_str("</tr>\n");
        }
        for row in rows {
            out.push_str("<tr>");
            for (_, v) in row {
                let _ = write!(out, "<td>{}</td>", esc(v));
            }
            out.push_str("</tr>\n");
        }
        out.push_str("</table></body></html>\n");
        std::fs::write(dir.join(format!("{stem}.html")), out).map_err(|e| e.to_string())?;
    }
    Ok(())
}

fn table_json(title: &str, rows: &[Vec<(&str, String)>]) -> String {
    let mut out = format!("{{\n  \"name\": {},\n  \"list\": [\n", json_str(title));
    for (k, row) in rows.iter().enumerate() {
        let fields: Vec<String> = row.iter().map(|(f, v)| format!("{}: {}", json_str(f), json_str(v))).collect();
        let _ = write!(out, "    {{{}}}{}", fields.join(", "), if k + 1 < rows.len() { ",\n" } else { "\n" });
    }
    out.push_str("  ]\n}\n");
    out
}

/// The front page of a full export: links to every section as (title, path without the
/// extension, note). The JSON variant lists the same for programs.
pub fn write_index(dir: &Path, title: &str, items: &[(String, String, String)], format: Format) -> Result<(), String> {
    if format.json() {
        let mut out = format!("{{\n  \"name\": {},\n  \"sections\": [\n", json_str(title));
        for (k, (t, path, note)) in items.iter().enumerate() {
            let _ = write!(out, "    {{\"title\": {}, \"file\": {}, \"about\": {}}}", json_str(t), json_str(&format!("{path}.json")), json_str(note));
            out.push_str(if k + 1 < items.len() { ",\n" } else { "\n" });
        }
        out.push_str("  ]\n}\n");
        std::fs::write(dir.join("result.json"), out).map_err(|e| e.to_string())?;
    }
    if format.html() {
        let mut out = head(title);
        for (t, path, note) in items {
            let _ = write!(out, "<div class=\"m\"><a class=\"f\" href=\"{}.html\">{}</a><div class=\"r\">{}</div></div>\n", esc(path), esc(t), esc(note));
        }
        out.push_str("</body></html>\n");
        std::fs::write(dir.join("export_results.html"), out).map_err(|e| e.to_string())?;
    }
    Ok(())
}

/// Folder for a new export: “Downloads/Glassgram Export/<name> <date>”.
pub fn target_dir(title: &str) -> PathBuf {
    let base = dirs::download_dir().or_else(dirs::home_dir).unwrap_or_else(|| PathBuf::from("."));
    let stamp = jiff::Zoned::now().strftime("%Y-%m-%d %H-%M").to_string();
    base.join("Glassgram Export").join(crate::platform::safe_file_name(&format!("{} {stamp}", title.trim())))
}

/// Dumps the whole history. `progress` is called every 100 messages.
pub async fn run(client: &Client, r: PeerRef, title: &str, with_media: bool, dir: &Path, mut progress: impl FnMut(usize)) -> Result<usize, String> {
    let files = dir.join("files");
    std::fs::create_dir_all(&files).map_err(|e| e.to_string())?;
    let mut entries = Vec::new();
    let mut iter = client.iter_messages(r);
    while let Some(m) = iter.next().await.map_err(|e| lf!("Експорт: {e}", "Export: {e}"))? {
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
                Media::Document(d) => Some(format!("{}_{}", m.id(), crate::platform::safe_file_name(d.name().unwrap_or_default()))),
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
            date: date(m.date().as_second()),
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
    write_chat(dir, title, &entries, Format::Both)?;
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

/// The page head with the shared style; `<body>` is left open.
fn head(title: &str) -> String {
    format!(
        "<!doctype html><html><head><meta charset=\"utf-8\"><meta name=\"viewport\" content=\"width=device-width\"><title>{t}</title><style>\
         body{{font:15px/1.45 system-ui,sans-serif;max-width:760px;margin:24px auto;padding:0 16px;background:#f3f5fb;color:#161a2e}}\
         h1{{font-size:22px}}.m{{background:#fff;border-radius:14px;padding:10px 14px;margin:8px 0;box-shadow:0 1px 3px #0001}}\
         .h{{font-size:13px;color:#5c6380;margin-bottom:4px}}.f{{font-weight:600;color:#3655d8}}.t{{white-space:pre-wrap}}\
         .r{{font-size:13px;color:#5c6380}}table{{border-collapse:collapse;width:100%}}td,th{{text-align:left;padding:6px 8px;border-bottom:1px solid #8884}}img{{max-width:100%;border-radius:10px;margin-top:6px}}\
         @media(prefers-color-scheme:dark){{body{{background:#0d1024;color:#eef0fb}}.m{{background:#1a1f3d}}.h,.r{{color:#a6addb}}.f{{color:#9bb0ff}}}}\
         </style></head><body><h1>{t}</h1>\n",
        t = esc(title)
    )
}

fn html(title: &str, entries: &[Entry]) -> String {
    let mut out = head(title);
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
    fn table_lists_rows() {
        let rows = vec![vec![("Name", "Олена".to_owned()), ("Phone", "+380".to_owned())], vec![("Name", "\"Q\"".to_owned()), ("Phone", String::new())]];
        let json = super::table_json("Contacts", &rows);
        assert!(json.contains("{\"Name\": \"Олена\", \"Phone\": \"+380\"},\n") && json.contains("\"Name\": \"\\\"Q\\\"\""));
    }

    #[test]
    fn json_escapes_control_chars() {
        assert_eq!(super::json_str("a\"b\n\u{1}"), "\"a\\\"b\\n\\u0001\"");
    }
}
