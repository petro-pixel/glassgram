//! Кілька акаунтів: у кожного своя сесія, спільні — кеш, налаштування й облікові дані API.
//! Акаунт 0 — сесія в корені теки даних (так було до появи кількох акаунтів),
//! решта — `accounts/<id>/session.sqlite`. Список і активний акаунт — у файлі `accounts`.

use std::path::PathBuf;

use crate::backend::data_dir;

#[derive(Debug, Clone, PartialEq)]
pub struct Account {
    pub id: u32,
    /// Ім'я з Telegram (порожнє, поки не ввійшли).
    pub name: String,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Accounts {
    pub active: u32,
    pub list: Vec<Account>,
}

fn file() -> PathBuf {
    data_dir().join("accounts")
}

impl Accounts {
    /// Формат: `active=<id>`, далі рядки `<id>|<ім'я>`.
    pub fn load() -> Self {
        let mut a = Self { active: 0, list: Vec::new() };
        if let Ok(text) = std::fs::read_to_string(file()) {
            for line in text.lines() {
                if let Some(v) = line.strip_prefix("active=") {
                    a.active = v.trim().parse().unwrap_or(0);
                } else if let Some((id, name)) = line.split_once('|')
                    && let Ok(id) = id.trim().parse()
                {
                    a.list.push(Account { id, name: name.to_owned() });
                }
            }
        }
        if a.list.is_empty() {
            a.list.push(Account { id: 0, name: String::new() });
        }
        if !a.list.iter().any(|x| x.id == a.active) {
            a.active = a.list[0].id;
        }
        a
    }

    pub fn save(&self) {
        let mut text = format!("active={}\n", self.active);
        for x in &self.list {
            // Ім'я — в одному рядку, без розділювача.
            text.push_str(&format!("{}|{}\n", x.id, x.name.replace(['\n', '|'], " ")));
        }
        let _ = std::fs::create_dir_all(data_dir());
        let _ = std::fs::write(file(), text);
        crate::backend::make_private(&file());
    }

    /// Новий порожній акаунт; повертає його id.
    pub fn add(&mut self) -> u32 {
        let id = self.list.iter().map(|x| x.id).max().unwrap_or(0) + 1;
        self.list.push(Account { id, name: String::new() });
        id
    }

    pub fn remove(&mut self, id: u32) {
        self.list.retain(|x| x.id != id);
        if self.list.is_empty() {
            self.list.push(Account { id: 0, name: String::new() });
        }
        if self.active == id {
            self.active = self.list[0].id;
        }
    }

    pub fn set_name(&mut self, id: u32, name: &str) {
        if let Some(x) = self.list.iter_mut().find(|x| x.id == id) {
            x.name = name.to_owned();
        }
    }
}

/// Файл сесії акаунта.
pub fn session_path(id: u32) -> PathBuf {
    match id {
        0 => data_dir().join("session.sqlite"),
        n => data_dir().join("accounts").join(n.to_string()).join("session.sqlite"),
    }
}

#[cfg(test)]
mod tests {
    use super::{Account, Accounts};

    #[test]
    fn add_and_remove_keep_one_active() {
        let mut a = Accounts { active: 0, list: vec![Account { id: 0, name: "Я".into() }] };
        let id = a.add();
        assert_eq!(id, 1);
        a.active = id;
        a.remove(id);
        assert_eq!(a.active, 0);
        a.remove(0);
        assert_eq!(a.list.len(), 1);
    }
}
