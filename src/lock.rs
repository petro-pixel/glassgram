//! Local passcode: locks the UI at startup and after inactivity.
//! Only a salt and a hash (SHA-256, many iterations) are stored, not the passcode itself.
//! It does not encrypt the Telegram session on disk: it guards against prying eyes, not file access.

use sha2::{Digest, Sha256};

/// Hashing iterations, so brute-forcing short passcodes is slower.
const ROUNDS: u32 = 100_000;

fn digest(salt: &str, passcode: &str) -> String {
    let mut h: [u8; 32] = Sha256::digest(format!("{salt}:{passcode}").as_bytes()).into();
    for _ in 0..ROUNDS {
        h = Sha256::digest(h).into();
    }
    h.iter().map(|b| format!("{b:02x}")).collect()
}

/// Settings entry: `salt$hash`.
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
