# Rules for agents

Agents (Claude Code, Codex, Cursor and others) read this file before working on the repository.
What to do next is in [ROADMAP.md](ROADMAP.md); how the code is organised is in [README.md](README.md).

## The project in brief

Glassgram is a desktop Telegram client in Rust: `eframe`/`egui` (`wgpu` renderer), MTProto via
`grammers` (API layer 230). License: GPL-3.0-or-later. The interface copies the structure of
Telegram Desktop with a "liquid glass" layer on top.

```sh
cargo build --release
cargo test --release
cargo run --release -- --demo   # made-up chats, no network
cargo run --release             # real account (session in ~/.local/share/glassgram)
```

Before you finish, go through [the checklist](#before-you-finish).

## Architecture

Two threads talk through two enums. The UI sends `Cmd`, the backend answers with `Ev`; the UI
never waits for the network.

- **`backend.rs`** — the protocol (`Cmd`, `Ev`, `ImgKey`) and the `Worker` on a tokio thread:
  connection, dialogs, history. `Worker::handle` only dispatches: a group of commands goes to its
  feature's handler in one line, `cmd @ (Cmd::A { .. } | Cmd::B(..)) => self.feature_cmd(cmd).await`.
  Shared pieces: `types.rs` (data for the UI), `loader.rs` (images), `convert.rs` (Telegram → our
  types), `updates.rs`, `upload.rs`.
- **`app.rs`** — the `App` struct, `push_event` (events), `apply` (user actions) and the frame loop.
  Everything else is in `src/app/<area>.rs` as `impl App` blocks: `history` and `bubbles` (the open
  chat), `composer`, `recording`, `media`, `chatlist`, `info`, `settings_pages`, `login` and so on.
- **A feature is a pair of files**: `src/app/<feature>.rs` and `src/backend/<feature>.rs`, listed in
  the structure table in README. It never grows `app.rs` or `backend.rs` beyond a field, an event
  arm and a dispatch line.
- **A feature owns its state.** Instead of loose `App` fields, the module defines a struct
  (`stories::Stories`, `saved::Saved`, `forum::Forum`, `info::InfoPanel`…) and `App` holds one field
  of it. Methods that change that state live next to it.
- **Size limits**: a file over ~1000 lines or a function over ~150 lines is a sign to split it.
  Remaining debt is listed in ROADMAP §18.
- **`demo.rs`** — answers the same `Cmd`s synchronously, without the network. Code must cope with
  the answer (`Ev`) arriving while still inside `send()`.
- **`cache.rs`** — local cache of dialogs, messages and the send queue (SQLite, `chats.sqlite`).
- Media: `player.rs` (audio via `ffplay`), `video.rs` (frames from `ffmpeg` into a texture),
  `recorder.rs` and `camera.rs` (recording), `export.rs`, `accounts.rs`, `settings.rs`, `lock.rs`, `i18n.rs`.
- Glass: `glass.rs` (colours, panels, buttons, fields), `blur.rs` + `glass.wgsl` (GPU blur), `icons.rs`.

### How to add a feature

1. `Cmd` variants (with doc-comments) and `src/backend/<feature>.rs` with
   `impl Worker { pub(super) async fn <feature>_cmd(&mut self, cmd: Cmd) -> Result<(), String> }`,
   plus its dispatch line in `Worker::handle`.
2. `Ev` variants for the results. Long or slow requests go into `tokio::spawn` with clones of
   `client` and `out`, so they don't hold up other commands.
3. `src/app/<feature>.rs`: the state struct (`#[derive(Default)]`, documented fields), event
   handlers (`on_<event>`), and the UI as `*_view` / `*_bar` methods. `App` gets one field and
   `push_event` one arm that calls into the module.
4. Colours and glass only through `glass::*`; strings only through `l!` / `lf!`.
5. **Mandatory** — an answer in `demo.rs` and demo data, so the feature is visible in `--demo`.
6. Update `ROADMAP.md` (mark ✅/🟡) and `README.md` (plus `README.uk.md`) if controls or structure changed.
   The readiness bar at the top of both READMEs is counted from the marks: refresh it with
   `UPDATE_PROGRESS=1 cargo test progress` (`src/progress.rs`).

## Code style

- Comments and doc-comments are in **English**, short: **why**, not what.
- Docs: `AGENTS.md` and `ROADMAP.md` are in English. `README.md` is in English with a Ukrainian
  translation in `README.uk.md`; keep the two in sync in the same change.
- Commit messages are in English.
- Read the neighbouring code and write the same way: same comment density, names, idioms.
- No `unwrap()` on data from the network or disk; errors go as a string into `Ev::Error`.
- New dependencies — pure Rust only, building on Linux, Windows and macOS without system
  dev libraries. Before adding a crate, check whether a suitable one is already in `Cargo.lock`.
  The one agreed exception is the mini app WebView (`wry` + `tao`, `webview.rs`): Linux builds need
  `libwebkit2gtk-4.1-dev` (and `pkg-config`), the Flatpak uses the GNOME runtime for WebKitGTK.

### UI strings

Every UI string is bilingual (Ukrainian + English) side by side, via the macros from `i18n.rs`:

```rust
l!("Скасувати", "Cancel")                    // &'static str
lf!("Голосів: {n}", "Votes: {n}")             // like format!, the compiler checks the arguments
```

A string without a translation is a bug. Content from the server (chat names, texts) is not translated.

Other languages are localisation files `locales/<code>.json` keyed by the English string
(`{"@name": "Deutsch", "Cancel": "Abbrechen"}`); missing entries fall back to English. After adding or
changing UI strings run `UPDATE_LOCALES=1 cargo test locales` to refresh `locales/template.json`, and
keep the placeholders of a translation the same as in English. In `lf!` put text between placeholders:
`{n}{}` can't be translated. A new built-in language goes into `BUILTIN` in `i18n.rs`.

### Cross-platform

New code must work on Linux, Windows and macOS. Everything that differs between them lives in
`platform.rs`; the rest of the code calls it instead of OS tools:
- links with `platform::open_url` (in the UI: `App::open_link`), received files with `platform::open_file`,
  our own files and folders with `platform::open` — never `xdg-open` (see [Security](#security));
- folders from `platform::data_dir` / `platform::download_dir` (the `dirs` crate), not XDG variables or `HOME`;
- `ffmpeg` / `ffplay` only via `platform::tool` (finds them next to the exe and in Homebrew, hides
  the console window on Windows), never `Command::new("ffmpeg")`; the microphone via `camera::Mic`
  (PipeWire through `pw-record` or PulseAudio on Linux, DirectShow, AVFoundation; stop with `Mic::stop`);
- notifications via `platform::notify`; the tray only through `tray::TrayHandle`;
- the system language from `sys_locale`; file permissions via `backend::make_private`;
- if the OS can't be avoided — `#[cfg(target_os = …)]` for **every** platform, not just Linux;
- dependencies that need system libraries on one OS go under `[target.'cfg(…)'.dependencies]`.

There is no Windows or macOS machine here, so check that the code at least compiles for them
(`rustup target add x86_64-pc-windows-gnu aarch64-apple-darwin`). C dependencies (SQLite) need a C
compiler for the target; for a type check a stub that creates empty object files is enough:
`CC_x86_64_pc_windows_gnu=<stub> AR_x86_64_pc_windows_gnu=ar cargo check --target x86_64-pc-windows-gnu`
(the stub answers `-?` with an error and prints `gcc` to stderr for `-E`).

## Design code

### Where the interface comes from

1. **Structure and behaviour — from Telegram Desktop.** Screen layout, menu item order,
   settings sections, hotkeys, texts — as in the official client. Don't invent your own
   schemes when Telegram already has an answer.
2. **Materials and motion — following Apple's principles** (Human Interface Guidelines, Liquid Glass 2025).
   We take the principles, not the assets: SF Symbols and Apple fonts are licensed only for Apple
   platforms, so the outline icons in `icons.rs` come from [Lucide](https://lucide.dev) (ISC): the same
   24×24 grid, stroke and round caps. A new icon: drop its SVG from lucide.dev into `assets/icons/`, add an `Icon`
   variant and an `svg()` arm.

### Where glass goes and where it doesn't

Glass is **a control layer floating above the content**. Not the content.

| Glass fits | No glass |
|---|---|
| chat header, composer, folder tabs, side panels | message text and long lists |
| popup menus, tooltips, dialogs, player, toasts | photos, videos, maps — they are the content |
| bubbles — a light tint (`Glass::tinted`) | glass on glass (no more than one layer) |

- Density and tint — through the ready presets: `Glass::panel`, `Glass::tinted(..).frosted(..)`.
  The accent tone (`ACCENT`) is only for the main action on the screen.
- Text on glass must be readable on any background: if the background is busy (media), put a dark
  chip under the text and use the `glass::ON_DARK` colour.
- Respect "Reduce transparency" and "Increase contrast" — with them, glass becomes opaque.

### Tokens

- **Colours — only through functions**: `ink()`, `ink_dim()`, `chrome()`, `veil(a)`, `ACCENT`,
  `ON_DARK`, `bubble_in()`, `link(out)`. No `Color32::from_rgb` for text or backgrounds in new
  places — otherwise one of the themes breaks.
- **Spacing** — a 4 pt grid: 4, 8, 12, 16, 24. Panel padding — 14–18 pt.
- **Radii** are concentric: inner = outer − padding. Capsules — radius = height / 2.
  Panels — 24–28, bubbles — 18, round buttons — half the side.
- **Font** — Ubuntu (built in). Scale: 11 (timestamps), 12.5 (captions), 13.5–14.5 (lists),
  15 (message text), 17 (window titles), 24 (section title).
- **Click targets** — at least 32 pt, icon buttons — 36–50 pt.
- **Motion** — 150–350 ms, spring curves; smooth, but with no delay in reacting to a click.
- **Icons** — Lucide SVG files in `assets/icons/` on a 24×24 grid, stroke ≈1.9 pt, round caps (`icons.rs`); menu rows with an icon via `glass::menu_button`.

### Checking the look

Check every UI change **in both themes and both languages**. There are no system screenshot
tools (Wayland), so use a temporary hook at the end of `App::ui`:
`egui::ViewportCommand::Screenshot` → `image::save_buffer`. Mark it `// TMP-SHOT`
and **remove** it before finishing (check: `grep -c TMP src/app.rs` gives 0).

## Testing on a real account

- The demo only proves the look. Check the protocol on a real account with a temporary windowless
  run (`src/probe.rs` + the `--probe` flag) that sends `Cmd`s and checks the `Ev`s. Delete it afterwards.
- Output **only counts and types** — no chat names, names or texts.
- Without the owner's explicit permission — **read only**. Send only to chats the owner named
  themselves; mark every test message `[тест Glassgram]`.
- Never without direct permission: terminate sessions, change privacy or the profile, join or
  leave chats, start bots, mark stories seen (the owner sees you), delete other people's content,
  pay Stars.
- The test account has few chats. To test groups, channels or stories, find public ones with
  `Cmd::SearchPublic` / `Cmd::ResolveUsername` and read them **without joining**.
- The probe skeleton: `backend::spawn(egui::Context::default(), accounts::session_path(0))`, send
  `Cmd::Connect` with the saved credentials, then a `wait(secs, |ev| …)` helper over `rx.recv_timeout`
  that prints `Ev::Error`. Keep each wait short and write output to a file: a run must finish in
  under two minutes. Remove `src/probe.rs` and the `// TMP-PROBE` lines in `main.rs` afterwards.

## Releases

A pushed tag `v*` runs `.github/workflows/release.yml`: the Windows installer (`packaging/windows`) and the
Flatpak bundle (`packaging/flatpak`), attached to the GitHub release. Bump `version` in `Cargo.toml` and add a
`<release>` to the metainfo before tagging. The Telegram API keys come from the repository secrets
`TG_API_ID` / `TG_API_HASH` via `build.rs`, built with `--features no-key-screen`, so a release never shows the key screen — never put them
in the code, a manifest, logs or a cache.

## Security

Everything that comes from the network is untrusted: message texts and links, file names, bot
answers and buttons, and above all mini app pages (any website can be one).

- **Links** only through `platform::open_url` / `App::open_link`: https, http, mailto and tg.
  `file:`, `smb:`, `search-ms:`, `ms-…:` and other protocol handlers can run programs or leak the
  Windows password hash. Never pass a string from the network to `platform::open` or `Command`.
- **Received files**: the name through `platform::safe_file_name` before it touches a path (no
  `../`, no absolute paths, no device names); opening through `platform::open_file`, which shows
  programs and scripts in their folder instead of running them.
- **Mini apps** (`webview.rs`, `app/webapp.rs`): every bridge method is an API for a stranger's page.
  Nothing that reads local data (clipboard, files, contacts) without the user's action; the window
  only navigates to web schemes. A new bridge method needs the same review.
- **Temporary files** (recordings, cropped photos) via `platform::temp_file` — a fresh owner-only
  folder, not a predictable name in the shared temp dir; delete them after sending.
- Session, credential and settings files — owner-only (`make_private`). The passcode (`lock.rs`) is
  a salted hash and only locks the UI; it does not encrypt the session.
- Don't log phone numbers, codes, passwords, session keys, message texts.
- Wallet, payments and secret chats — only with a separate security review before merging.

## Before you finish

1. `cargo build --release` with **no warnings**, `cargo test --release` green.
2. UI strings changed → `UPDATE_LOCALES=1 cargo test locales`; ROADMAP marks changed →
   `UPDATE_PROGRESS=1 cargo test progress`.
3. Platform code changed → `cargo check` for `x86_64-pc-windows-gnu` and `aarch64-apple-darwin`.
4. UI changed → screenshots in both themes and both languages; `grep -c TMP src/app.rs src/main.rs` gives 0.
5. Protocol changed → a probe on the real account (read only), counts only in the output.
6. ROADMAP, README and README.uk updated in the same change; one commit per feature.
