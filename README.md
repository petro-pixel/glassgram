# Glassgram

**English** · [Українська](README.uk.md)

A desktop Telegram client in Rust with a **Liquid Glass** style interface.

- **MTProto**: [grammers](https://codeberg.org/Lonami/grammers): sign-in by phone number or QR code, 2FA, chats, history, replies, editing and deleting, live updates
- **Media**: avatars, photos, stickers with a disk cache (`~/.local/share/glassgram/cache`); sending files and stickers; voice and audio with a waveform, seeking and speed; videos, GIFs, round videos and webm stickers right in the chat; voice recording (needs ffmpeg with `ffplay`)
- **Chats**: forums with topics, channel comments, folders and archive, pinning, muting, drafts, "online" and "typing…" statuses
- **Search**: global (the field above the chat list) and in-chat (🔍 in the header)
- **Formatting**: Telegram entities are shown, markup is converted when sending
- **Messages**: forwarding, multi-select, silent and scheduled sending, bot buttons, pasting from the clipboard, file progress
- **Notifications**: system ones (via `notify-send`), a counter in the window title, a tray icon
- **Languages**: Ukrainian and English (`src/i18n.rs`)
- **Accounts**: several accounts, switched in the menu under the name in the header (`src/accounts.rs`)
- **Settings** (⚙): language, theme (dark, light, system), interface scale, notifications, autoplay, SOCKS5 proxy, privacy, active sessions, local passcode; stored in `~/.local/share/glassgram/settings`
- **Settings layout** as in Telegram Desktop: My Account (name, bio, username with availability check, profile photo) · Notifications · Privacy and Security · Chat Settings · Folders · Advanced · Devices · Language
- **Send queue**: text messages show up at once (clock), are retried while offline and survive a restart; failed ones get a red mark with Retry / Delete
- **Glass**: real blur via a wgpu shader (`src/blur.rs`, `src/glass.wgsl`); without wgpu — a vector fallback
- **UI**: egui / eframe (wgpu), our own glass renderer — `src/glass.rs`

## Running

```sh
cargo run --release -- --demo   # demo with made-up chats, no network
cargo run --release             # real account
```

A real account needs an `api_id` and `api_hash` from <https://my.telegram.org> → *API development tools*.
Enter them in the sign-in window or pass them via the `TG_API_ID` / `TG_API_HASH` variables.

The session and credentials are stored in `~/.local/share/glassgram/`.

## Structure

| File | What it does |
|---|---|
| `src/backend.rs` | Tokio thread: login, dialogs, history, updates; `Cmd` / `Ev` channels to the UI |
| `src/backend/account.rs`, `src/backend/outbox.rs` | Own profile, username and photo; one send attempt from the queue |
| `src/cache.rs` | Local cache of dialogs, messages and the send queue (SQLite via `libsql`) |
| `src/blur.rs`, `src/glass.wgsl` | GPU blur: background into a texture → Gaussian → glass panels with refraction |
| `src/glass.rs` | Live background, glass panels (gradient, specular rim, refraction halo, shadow), buttons, fields, avatars |
| `src/app.rs` | Sign-in and chat screens |
| `src/app/chatview.rs`, `src/backend/chatview.rs` | Chat view extras: pinned bar and list, jump buttons (down / mentions / reactions), "Seen by", view counters, protected content |
| `src/app/suggest.rs`, `src/backend/suggest.rs` | Link previews (in bubbles and above the composer), @mention and bot command suggestions, "Start" for bots |
| `src/app/info.rs`, `src/backend/info.rs` | Chat info column: profile, members, shared media, per-chat notifications, leave / clear / delete |
| `src/app/settings_pages.rs` | Settings pages (Telegram Desktop layout), My Account editor |
| `src/app/hotkeys.rs` | Keyboard shortcuts |
| `src/app/outbox.rs` | Send queue: pending/failed bubbles, retries with backoff |
| `src/app/chatlist.rs` | Folder column and tabs, dragging pinned chats and folders, "New chat" |
| `src/player.rs` | Voice and audio player via `ffplay`: pause, seeking, speed |
| `src/video.rs` | Video frames via `ffmpeg` into an egui texture: GIFs, round videos, videos, webm stickers |
| `src/camera.rs` | Video message recording: camera + microphone → MP4, round preview |
| `src/recorder.rs` | Voice recording via `ffmpeg` (PulseAudio/PipeWire), level and waveform |
| `src/export.rs` | Chat history export to HTML and JSON |
| `src/settings.rs` | Settings and saving them |
| `src/lock.rs` | Local passcode (salt + SHA-256) |
| `src/tray.rs`, `src/logo.rs` | Tray icon and the app logo |
| `src/icons.rs` | Vector icons: Lucide SVG paths on a 24×24 grid, flattened with `kurbo` |
| `src/format.rs` | Telegram entities ↔ markup, formatted text for egui, fonts |
| `src/demo.rs` | Demo data |

## Controls

- Right-click a message for the menu: reply, copy, edit, delete.
- Double-click — reply.
- Esc cancels a reply or edit and closes the sticker panel.
- 📎 — send files (or drag them into the window); the composer text becomes the caption.
- 🙂 — sticker panel.
- Click a photo or file — download it to Downloads and open it.
- Right-click a chat: pin, mute, mark as read, archive.
- Click the chat title in the header to open chat info (Esc closes it): bio/description, links, notifications, members (click opens a private chat), shared media tabs, leave / clear history / delete chat.
- Composer markup: `**bold**`, `__italic__`, `~~strike~~`, `||spoiler||`, `` `code` ``, ```` ```block``` ````, `[text](https://…)`.
- Clicking a hashtag searches for it in the chat; clicking a bot command sends it.
- `@` in a group and `/` at the start of the composer open suggestions (members, bot commands): ↑/↓ — select, Enter or click — insert (a command is sent), Tab — insert, Esc — close. The "/" button next to the composer in a bot chat opens the command menu.
- A link preview appears above the composer; ✕ sends the message without it.
- Right-click "Send": send silently or schedule.
- "Select" in the message menu turns on selection mode; Esc leaves it.
- Enter sends, Shift+Enter — new line.
- Hotkeys (as in Telegram Desktop): Ctrl+F — search in the chat (or the chat list), Ctrl+K — chat list search, Ctrl+1…9 — folders, Alt+↑/↓ or Ctrl+Tab / Ctrl+Shift+Tab — previous/next chat, Ctrl+E or ↑ in an empty composer — edit your last message, Esc — close the topmost panel, then the chat.
- In `--demo`, a message containing `#offline` pretends there is no network (clock → retries → red mark).
- Folders: "+" to the right of the tabs (or at the bottom of the column) — new; right-click a folder — edit or delete; dragging changes the order. Tabs on top or a column on the left — in Settings.
- Pinned chats can be dragged to change their order (in folders and in the archive too).
- ✎ next to search — "New group", "New channel", "Contacts".
- Ctrl+V with an image or copied files sends them.
- ▶ on a voice or audio message plays it; clicking the waveform seeks, the "1×" chip changes the speed. Clicking a video plays it in the bubble; "Open in player" is in the menu.
- 🎤 (the "Send" button when the composer is empty) starts voice recording; ➤ sends, ■ stops for review, Esc cancels.
- Right-click the microphone — record a video message (camera — in Settings or `GLASSGRAM_CAMERA`).

## What's done and what's next

The full roadmap with priorities and time estimates is in [ROADMAP.md](ROADMAP.md).
Rules for agents and the design code are in [AGENTS.md](AGENTS.md).

## License

GPL-3.0-or-later, like Telegram Desktop. See [LICENSE](LICENSE).
The Ubuntu font is under the Ubuntu Font Licence, see [assets/fonts/UFL.txt](assets/fonts/UFL.txt).
Icons are from [Lucide](https://lucide.dev) under the ISC licence, see [assets/icons/LUCIDE-LICENSE.txt](assets/icons/LUCIDE-LICENSE.txt).
Glassgram is an unofficial client and is not affiliated with Telegram FZ-LLC.
