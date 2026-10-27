// Build script: Telegram API keys for release builds and the Windows exe icon.
//
// Release builds (GitHub Actions) get `GLASSGRAM_API_ID` / `GLASSGRAM_API_HASH` from repository secrets,
// so users don't have to enter keys. The keys are never in the source; in the binary they are XORed with
// a per-build random mask, so a plain search for strings doesn't find them. That is all a client can do:
// whatever the program can read, a determined person can read too. Without the variables (local builds)
// the app asks for the keys at sign-in, as before.

use std::env;
use std::fs;
use std::path::PathBuf;

fn main() {
    println!("cargo:rerun-if-env-changed=GLASSGRAM_API_ID");
    println!("cargo:rerun-if-env-changed=GLASSGRAM_API_HASH");
    let out = PathBuf::from(env::var("OUT_DIR").expect("OUT_DIR")).join("api_keys.rs");
    let id = env::var("GLASSGRAM_API_ID").ok().and_then(|v| v.trim().parse::<i32>().ok());
    let hash = env::var("GLASSGRAM_API_HASH").ok().map(|h| h.trim().to_owned()).filter(|h| !h.is_empty());
    let code = match (id, hash) {
        (Some(id), Some(hash)) => {
            let mask = random_bytes(4 + hash.len());
            let id_x = (id as u32) ^ u32::from_le_bytes([mask[0], mask[1], mask[2], mask[3]]);
            let hash_x: Vec<u8> = hash.bytes().zip(&mask[4..]).map(|(b, m)| b ^ m).collect();
            format!("pub const API: Option<(u32, &[u8], &[u8])> = Some(({id_x}, &{hash_x:?}, &{mask:?}));\n")
        }
        _ => "pub const API: Option<(u32, &[u8], &[u8])> = None;\n".to_owned(),
    };
    fs::write(out, code).expect("write api_keys.rs");

    // The exe icon (Explorer, taskbar, shortcuts). Only with MSVC, which has the resource compiler.
    if env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("windows") && env::var("CARGO_CFG_TARGET_ENV").as_deref() == Ok("msvc") {
        println!("cargo:rerun-if-changed=packaging/icons/glassgram.ico");
        println!("cargo:rerun-if-changed=packaging/windows/glassgram.manifest");
        let mut res = winresource::WindowsResource::new();
        res.set_icon("packaging/icons/glassgram.ico");
        res.set_manifest_file("packaging/windows/glassgram.manifest");
        // Full version info: antivirus heuristics distrust unsigned exes that say nothing about themselves.
        res.set("ProductName", "Glassgram");
        res.set("FileDescription", "Glassgram");
        res.set("CompanyName", "petro-pixel");
        res.set("LegalCopyright", "GPL-3.0-or-later, https://github.com/petro-pixel/glassgram");
        res.set("OriginalFilename", "glassgram.exe");
        res.set("InternalName", "glassgram");
        res.compile().expect("Windows resources");
    }
}

/// A mask that differs on every build; not cryptographic, only so the bytes don't repeat between builds.
fn random_bytes(n: usize) -> Vec<u8> {
    use std::collections::hash_map::RandomState;
    use std::hash::{BuildHasher, Hasher};
    (0..n.div_ceil(8))
        .flat_map(|i| {
            let mut h = RandomState::new().build_hasher();
            h.write_usize(i);
            h.finish().to_le_bytes()
        })
        .take(n)
        .collect()
}
