# Glassgram

**English** · [Українська](README.uk.md)

A desktop Telegram client in Rust with a **Liquid Glass** style interface.

<!-- progress -->
**Roadmap progress: 50%** `██████████░░░░░░░░░░` — 85 done, 1 partial, 84 to go ([ROADMAP.md](ROADMAP.md))
<!-- /progress -->

<p>
  <img src="assets/screenshots/dark-en.png" width="49%" alt="Glassgram, dark theme">
  <img src="assets/screenshots/light-en.png" width="49%" alt="Glassgram, light theme">
</p>

[![ko-fi](https://ko-fi.com/img/githubbutton_sm.svg)](https://ko-fi.com/K8Z3299QVD)

- **MTProto**: [grammers](https://codeberg.org/Lonami/grammers): sign-in by phone number or QR code, 2FA, chats, history, replies, editing and deleting, live updates
- **Media**: avatars, photos, stickers with a disk cache (`~/.local/share/glassgram/cache`); sending files and stickers; voice and audio with a waveform, seeking and speed; videos, GIFs, round videos and webm stickers right in the chat; voice recording (needs ffmpeg with `ffplay`)
- **Chats**: forums with topics, channel comments, folders and archive, pinning, muting, drafts, "online" and "typing…" statuses
- **Search**: global (the field above the chat list) and in-chat (🔍 in the header)
- **Formatting**: Telegram entities are shown, markup is converted when sending
- **Messages**: forwarding, multi-select, silent and scheduled sending, bot buttons, pasting from the clipboard, file progress
- **Notifications**: system ones on Linux, Windows and macOS, a counter in the window title, a tray icon
- **Languages**: Ukrainian and English built into the code, German and any other language from localisation files (`locales/*.json`, see below)
- **Accounts**: several accounts, switched in the menu under the name in the header (`src/accounts.rs`)
- **Settings** (⚙): language, theme (dark, light, system), interface scale, notifications, autoplay, SOCKS5 proxy, privacy, active sessions, local passcode; stored in `~/.local/share/glassgram/settings`
- **Settings layout** as in Telegram Desktop: My Account (name, bio, username with availability check, profile photo) · Notifications · Privacy and Security · Chat Settings · Folders · Advanced · Devices · Language
- **Send queue**: text messages, files, voice and round video messages show up at once (clock), are retried while offline and survive a restart; failed ones get a red mark with Retry / Delete
- **Glass**: real blur via a wgpu shader (`src/blur.rs`, `src/glass.wgsl`); without wgpu — a vector fallback
- **UI**: egui / eframe (wgpu), our own glass renderer — `src/glass.rs`

## Installing

Ready builds are on the [Releases](https://github.com/petro-pixel/glassgram/releases) page; they already
contain the Telegram API keys and ffmpeg, so you only sign in with your phone number:
- **Windows**: `Glassgram-<version>-setup.exe`;
- **Linux**: `Glassgram.flatpak` — `flatpak install --user Glassgram.flatpak`, then `flatpak run io.github.petro_pixel.Glassgram`.

### Code signing policy

Free code signing provided by [SignPath.io](https://about.signpath.io/), certificate by [SignPath Foundation](https://signpath.org/).
The Windows installer and `glassgram.exe` are built and signed by GitHub Actions (`.github/workflows/release.yml`)
from this repository; the bundled `ffmpeg.exe` is the upstream build and is not signed by us.

- Committers and reviewers: [petro-pixel](https://github.com/petro-pixel)
- Approvers: [petro-pixel](https://github.com/petro-pixel)

Privacy: this program will not transfer any information to other networked systems unless specifically
requested by the user or the person installing or operating it. It connects only to the Telegram servers to run
your account, and opens bot mini apps and links only when you do.

## Running from source

```sh
cargo run --release -- --demo   # demo with made-up chats, no network
cargo run --release             # real account
```

On Linux the build needs WebKitGTK for mini app windows: `sudo apt install libwebkit2gtk-4.1-dev pkg-config`
(`sudo dnf install webkit2gtk4.1-devel` on Fedora, `sudo pacman -S webkit2gtk-4.1` on Arch); the version from the distribution's repositories is used. Windows (WebView2) and macOS (WKWebView) need nothing extra.

A build from source needs an `api_id` and `api_hash` from <https://my.telegram.org> → *API development tools*.
Enter them in the sign-in window or pass them via the `TG_API_ID` / `TG_API_HASH` variables. To build them
into the binary as the releases do, set `GLASSGRAM_API_ID` / `GLASSGRAM_API_HASH` while building (`build.rs`); add `--features no-key-screen`
to drop the key screen as well (without a connection the app offers "Retry" instead).

The session and credentials are stored in the data folder: `~/.local/share/glassgram/` on Linux,
`%APPDATA%\glassgram\` on Windows, `~/Library/Application Support/glassgram/` on macOS.

Voice, audio, video and recording need ffmpeg (`ffmpeg` and `ffplay`):
- **Linux**: from the distribution (`sudo apt install ffmpeg`, `sudo dnf install ffmpeg`…);
- **Windows**: put `ffmpeg.exe` and `ffplay.exe` next to `glassgram.exe` or anywhere in `PATH`;
- **macOS**: `brew install ffmpeg`.

On Linux the microphone is taken from PipeWire (`pw-record`) when it runs, otherwise from PulseAudio;
`GLASSGRAM_AUDIO=pulse` or `GLASSGRAM_AUDIO=pipewire` forces one. On Windows closing the window hides
Glassgram to the tray; quit from the tray icon's menu.

Windows and macOS builds compile but haven't been tried on real machines yet.

## Translations

Ukrainian and English live in the code. Every other language is a JSON file keyed by the English string:

```json
{ "@name": "Deutsch", "Cancel": "Abbrechen", "Votes: {n}": "Stimmen: {n}" }
```

1. Copy [locales/template.json](locales/template.json) (all strings, empty values) to `<code>.json`, e.g. `pl.json`.
2. Set `@name` and translate the values; keep `{placeholders}` as they are. Empty values fall back to English.
3. Put the file into the `locales` folder next to the session and settings (`~/.local/share/glassgram/locales/`)
   and pick the language in Settings → Language. No rebuild needed.
4. To ship it with the app, add it to `locales/` and to `BUILTIN` in `src/i18n.rs`.

## Structure

| File | What it does |
|---|---|
| `src/backend.rs` | Tokio thread: the `Cmd` / `Ev` protocol with the UI, the `Worker` (connection, dialogs, history) dispatching commands to feature handlers |
| `src/backend/types.rs` | Data handed to the UI: chats, messages, media, folders, privacy, presence |
| `src/backend/loader.rs`, `src/backend/convert.rs` | Background image downloads with a disk cache; Telegram objects → our types |
| `src/backend/updates.rs`, `src/backend/upload.rs` | Live updates and QR login loops; sending files, downloads, media cache |
| `src/backend/login.rs`, `src/backend/sessions.rs` | Signing in (code, 2FA, QR) and out; active sessions |
| `src/backend/sending.rs`, `src/backend/scheduled.rs`, `src/backend/inline.rs` | Sending, editing, deleting, reactions, votes; scheduled messages; inline bots and buttons |
| `src/backend/dialogs.rs`, `src/backend/folders.rs`, `src/backend/topics.rs`, `src/backend/comments.rs` | Chat list actions, folders, forum topics, channel comments |
| `src/backend/account.rs`, `src/backend/outbox.rs` | Own profile, username and photo; one send attempt from the queue (text, or files uploaded in the background) |
| `src/cache.rs` | Local cache of dialogs, messages and the send queue (SQLite via `libsql`) |
| `src/blur.rs`, `src/glass.wgsl` | GPU blur: background into a texture → Gaussian → glass panels with refraction |
| `src/glass.rs` | Live background, glass panels (gradient, specular rim, refraction halo, shadow), buttons, fields, avatars |
| `src/app.rs` | `App`: state, events (`push_event`), user actions (`apply`), the frame loop |
| `src/app/history.rs`, `src/app/bubbles.rs` | The open chat: history, header, overlays; drawing bubbles, albums, media cards, reactions |
| `src/app/composer.rs`, `src/app/recording.rs` | The message field: text, files, stickers, emoji, inline bots; voice and round video recording |
| `src/app/media.rs`, `src/app/dialogs.rs` | Audio player, video clips, photo viewer; export, forward, schedule boxes |
| `src/app/login.rs`, `src/app/accounts_ui.rs` | Sign-in screens and the passcode lock; the account menu and the other accounts in the background |
| `src/app/forum.rs`, `src/app/comments.rs`, `src/app/folders.rs` | Forum topics, channel comments, folder editor |
| `src/app/chatview.rs`, `src/backend/chatview.rs` | Chat view extras: pinned bar and list, jump buttons (down / mentions / reactions), "Seen by", view counters, protected content |
| `src/app/chatedit.rs`, `src/backend/chatedit.rs` | "Edit group / channel": photo, title, description, member permissions, slow mode |
| `src/app/birthdays.rs`, `src/backend/birthdays.rs` | Birthdays: reminder above the chat list, birthday in user info and in My Account |
| `src/app/botstart.rs`, `src/backend/botstart.rs` | Bot deep links `t.me/<bot>?start=…`: Start bar with the parameter |
| `src/app/similar.rs`, `src/backend/similar.rs` | "Similar channels" tab in a channel's info |
| `src/app/saved.rs`, `src/backend/saved.rs` | Saved Messages split by source: "All" and one tab per chat |
| `src/app/chatsearch.rs` | In-chat search panel: type and sender filters, results |
| `src/app/stories.rs`, `src/backend/stories.rs` | Stories: strip above the chat list and the viewer (video with sound, replies, reactions) |
| `src/app/confirm.rs` | "Open this link?" for hidden links and "Run this file?" for received programs |
| `src/app/gifts.rs`, `src/backend/gifts.rs` | "Gifts" tab in a profile |
| `src/app/textures.rs` | Pictures on the GPU with a memory budget; dropped ones come back from the disk cache |
| `src/app/emojikw.rs`, `src/backend/emojikw.rs` | Emoji suggestions while typing `:word` |
| `src/app/report.rs`, `src/backend/report.rs` | "Report" box for messages |
| `src/app/quote.rs`, `src/backend/reply.rs` | Replies with a quote and in another chat |
| `src/app/newchat.rs`, `src/backend/newchat.rs` | "New chat": contacts, new group, new channel with its type, link and subscribers |
| `src/app/privacy.rs`, `src/backend/privacy.rs` | Privacy rule exceptions (always / never allow) and global switches: new non-contact chats, read time, who can message me |
| `src/app/polls.rs`, `src/backend/polls.rs` | "New poll" box: polls and quizzes |
| `src/app/webapp.rs`, `src/backend/webapp.rs`, `src/webview.rs` | Mini apps: server requests, the WebView window process (`glassgram --webview`) and its bridge |
| `src/app/contacts.rs`, `src/backend/contacts.rs` | "New contact" box, add / delete contact and block from user info, blocked list in Privacy and Security |
| `src/app/translate.rs`, `src/backend/translate.rs` | "Translate" window from the message menu |
| `src/app/calendar.rs` | "Jump to date" calendar over the chat (the request is `Cmd::JumpToDate` in `src/backend/chatview.rs`) |
| `src/app/notify.rs`, `src/backend/notify.rs` | Notifications for chats: private / groups / channels defaults, exceptions, resolving a chat's mute state from its category |
| `src/app/sponsored.rs`, `src/backend/sponsored.rs` | Sponsored messages in channel history (after the newest post and between posts, media ads): "Ad" bubble, view and click reports, "About this ad", reporting, hiding for Premium |
| `src/app/suggest.rs`, `src/backend/suggest.rs` | Link previews (in bubbles and above the composer), @mention and bot command suggestions, "Start" for bots |
| `src/app/info.rs`, `src/backend/info.rs` | Chat info column: profile, members, shared media, per-chat notifications, leave / clear / delete |
| `src/app/settings_pages.rs` | Settings pages (Telegram Desktop layout), My Account editor |
| `src/app/hotkeys.rs` | Keyboard shortcuts |
| `src/app/outbox.rs` | Send queue: pending/failed bubbles, retries with backoff |
| `src/app/chatlist.rs` | Folder column and tabs, dragging pinned chats and folders |
| `src/player.rs` | Voice and audio player via `ffplay`: pause, seeking, speed |
| `src/video.rs` | Video frames via `ffmpeg` into an egui texture: GIFs, round videos, videos, webm stickers; `.tgs` stickers from `src/lottie.rs` |
| `src/lottie.rs` | Animated (Lottie) stickers drawn on the CPU: `velato` + `vello_cpu`, with limits for untrusted files |
| `src/camera.rs` | Video message recording: camera + microphone → MP4, round preview; the microphone for all recordings (`Mic`) |
| `src/recorder.rs` | Voice recording via `ffmpeg`, level and waveform |
| `src/export.rs` | Chat history export to HTML and JSON |
| `src/platform.rs` | Everything that differs between Linux, Windows and macOS: folders, ffmpeg/ffplay, notifications; the safe openers for links (`open_url`) and received files (`open_file`, `safe_file_name`), private temp files |
| `src/settings.rs` | Settings and saving them |
| `src/lock.rs` | Local passcode (salt + SHA-256) |
| `src/tray.rs`, `src/logo.rs` | Tray icon (`ksni` on Linux, `tray-icon` on Windows and macOS) and the app logo |
| `src/icons.rs`, `assets/icons/` | Vector icons: Lucide SVG files (24×24), parsed with `roxmltree` and flattened with `kurbo` |
| `src/i18n.rs`, `locales/` | UI language: `l!`/`lf!` for Ukrainian and English, JSON localisation files for the rest |
| `src/format.rs` | Telegram entities ↔ markup, formatted text for egui, fonts |
| `src/demo.rs` | Demo data |
| `build.rs` | Telegram API keys from the build environment (masked), the Windows exe icon |
| `packaging/`, `.github/workflows/release.yml` | Windows installer (Inno Setup), Flatpak manifest, `.desktop`, metainfo, icons; release builds |

## Controls

- Right-click a message for the menu: reply, copy, edit, delete.
- Double-click — reply. Click the reply bar above the field to quote a part of the message or to reply in another chat.
- Esc cancels a reply or edit and closes the sticker panel.
- 📎 — send files (or drag them into the window); the composer text becomes the caption. Right click on 📎: send without compression, create a poll or quiz.
- 🙂 — sticker panel. An emoji typed alone offers matching stickers above the field.
- Click a photo or file — download it to Downloads and open it.
- Right-click a chat: pin, mute, mark as read, archive.
- Click the chat title in the header to open chat info (Esc closes it): bio/description, links, notifications, members (click opens a private chat), shared media tabs, leave / clear history / delete chat.
- Composer markup: `**bold**`, `__italic__`, `~~strike~~`, `||spoiler||`, `` `code` ``, ```` ```block``` ````, `[text](https://…)`.
- Clicking a hashtag searches for it in the chat; clicking a bot command sends it.
- `@` in a group and `/` at the start of the composer open suggestions (members, bot commands): ↑/↓ — select, Enter or click — insert (a command is sent), Tab — insert, Esc — close. The "/" button next to the composer in a bot chat opens the command menu. `:word` (two letters or more, e.g. `:heart`, `:серце`) suggests emoji the same way.
- A link preview appears above the composer; ✕ sends the message without it.
- Right-click "Send": send silently or schedule.
- "Select" in the message menu turns on selection mode; Esc leaves it.
- Enter sends, Shift+Enter — new line.
- Jump to a date: the calendar button in chat search, or click a date chip between messages.
- Enter sends, Shift+Enter starts a new line; Settings → Chat Settings switches sending to Ctrl+Enter (Cmd+Enter on macOS) and sets the message text size.
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
