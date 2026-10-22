//! Локальний пароль (passcode): блокує інтерфейс на старті й після бездіяльності.
//! Зберігається лише сіль і хеш (SHA-256, багато ітерацій), а не сам пароль.
//! Сесію Telegram на диску він не шифрує — це захист від чужих очей, а не від доступу до файлів.

use sha2::{Digest, Sha256};

/// Ітерацій хешування — щоб перебір коротких паролів був повільнішим.
const ROUNDS: u32 = 100_000;

fn digest(salt: &str, passcode: &str) -> String {
    let mut h: [u8; 32] = Sha256::digest(format!("{salt}:{passcode}").as_bytes()).into();
    for _ in 0..ROUNDS {
        h = Sha256::digest(h).into();
    }
    h.iter().map(|b| format!("{b:02x}")).collect()
}

/// Запис для налаштувань: `сіль$хеш`.
pub fn make(passcode: &str) -> String {
    let now = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap_or_default();
    let salt = format!("{:x}{:x}", now.as_nanos(), std::process::id());
    format!("{salt}${}", digest(&salt, passcode))
}

pub fn check(stored: &str, passcode: &str) -> bool {
    match stored.split_once('$') {
        Some((salt, hash)) => digest(salt, passcode) == hash,
        None => false,
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn passcode_roundtrip() {
        let stored = super::make("1234");
        assert!(super::check(&stored, "1234"));
        assert!(!super::check(&stored, "12345"));
        assert!(!super::check("", "1234"));
    }
}
