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

Before you finish, the build must have **no warnings** and the tests must be green.

## Architecture

- **`backend.rs`** — a separate thread with tokio. The UI sends `Cmd`, the backend answers with `Ev`
  over channels. The UI never waits for the network.
- **`app.rs`** — all state and drawing. Events are handled in `push_event`, user actions in `apply`.
- **Features live in submodules**: UI in `src/app/<feature>.rs` (`impl App` blocks), network in
  `src/backend/<feature>.rs` (handlers called from `Worker::handle`). Current ones: `chatlist`,
  `chatview`, `info`, `suggest`, `outbox`, `hotkeys`, `settings_pages` (Settings pages in the
  Telegram Desktop layout), `account`. A new feature of any size gets its own pair of files instead
  of growing `app.rs` / `backend.rs`; see the structure table in README.
- **`demo.rs`** — answers the same `Cmd`s synchronously, without the network. Code must cope with
  the answer (`Ev`) arriving while still inside `send()`.
- **`cache.rs`** — local cache of dialogs, messages and the send queue (SQLite, `chats.sqlite`).
- Media: `player.rs` (audio via `ffplay`), `video.rs` (frames from `ffmpeg` into a texture),
  `recorder.rs` and `camera.rs` (recording), `export.rs`, `accounts.rs`, `settings.rs`, `lock.rs`, `i18n.rs`.
- Glass: `glass.rs` (colours, panels, buttons, fields), `blur.rs` + `glass.wgsl` (GPU blur), `icons.rs`.

### How to add a feature

1. A `Cmd` variant (with a doc-comment) and its handler in `Worker::handle`.
2. An `Ev` variant for the result. Long operations go into `tokio::spawn` so they don't block other commands.
3. Handle the `Ev` in `App::push_event`; state lives in `App` fields with a comment.
4. UI is a separate `*_view` method or function; colours and glass only through `glass::*`.
5. **Mandatory** — an answer in `demo.rs` and demo data, so the feature is visible in `--demo`.
6. Update `ROADMAP.md` (mark ✅/🟡) and `README.md` (plus `README.uk.md`) if controls or structure changed.

## Code style

- Comments and doc-comments are in **English**, short: **why**, not what.
- Docs: `AGENTS.md` and `ROADMAP.md` are in English. `README.md` is in English with a Ukrainian
  translation in `README.uk.md`; keep the two in sync in the same change.
- Commit messages are in English.
- Read the neighbouring code and write the same way: same comment density, names, idioms.
- No `unwrap()` on data from the network or disk; errors go as a string into `Ev::Error`.
- New dependencies — pure Rust only, building on Linux, Windows and macOS without system
  dev libraries. Before adding a crate, check whether a suitable one is already in `Cargo.lock`.

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

New code must work on Linux, Windows and macOS:
- open files and links with `open::that_detached`, not `xdg-open`;
- take folders from `dirs` (`download_dir`, `data_dir`), not from XDG variables;
- the system language from `sys_locale`; file permissions via `backend::make_private`;
- if the OS can't be avoided — `#[cfg(target_os = …)]` for **every** platform, not just Linux.

## Design code

### Where the interface comes from

1. **Structure and behaviour — from Telegram Desktop.** Screen layout, menu item order,
   settings sections, hotkeys, texts — as in the official client. Don't invent your own
   schemes when Telegram already has an answer.
2. **Materials and motion — following Apple's principles** (Human Interface Guidelines, Liquid Glass 2025).
   We take the principles, not the assets: SF Symbols and Apple fonts are licensed only for Apple
   platforms, so the outline icons in `icons.rs` come from [Lucide](https://lucide.dev) (ISC): the same
   24×24 grid, stroke and round caps. A new icon: convert its SVG elements to path data and add a `lucide()` arm.

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
- **Icons** — Lucide paths on a 24×24 grid, stroke ≈1.9 pt, round caps (`icons.rs`); menu rows with an icon via `glass::menu_button`.

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
- Never without direct permission: terminate sessions, change privacy, join or leave
  chats, delete other people's content, pay Stars.

## Security

- Session, credential and settings files — owner-only (`make_private`).
- Don't log phone numbers, codes, passwords, session keys, message texts.
- Temporary files (recordings, exports) go in `std::env::temp_dir()`, deleted after sending.
- Wallet, payments and secret chats — only with a separate security review before merging.
