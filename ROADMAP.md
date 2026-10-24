# Glassgram roadmap

The goal is a full everyday Telegram client: everything the official apps can do, with a
"liquid glass" interface. The list follows the MTProto API schema (layer 230: `messages`,
`account`, `payments`, `stories`, `bots`, `phone`, `wallet`, `communities`…) and Telegram's
updates up to autumn 2026 (rich text, AI editor, communities, gift crafting and so on).

**Interface principle.** The structure of screens, menus and settings is copied from the official
clients: they have been tuned on millions of people for years, and with as many settings as
Telegram has, home-grown schemes lose. Telegram Desktop is GPLv3 — its layouts, texts and logic
can be carried over with attribution. Mobile clients (GPLv2) — only as a reference for behaviour,
no code. What is ours is the "liquid glass" layer on top (rules in [AGENTS.md](AGENTS.md)).

**Marks:** ✅ done · 🟡 partial · ⬜ not yet
**Priorities:** **P0** — without it nobody uses the client daily · **P1** — expected in
everyday use · **P2** — advanced · **P3** — niche, paid or experimental

Time estimate — [at the end of the file](#time-estimate).

---

## 1. Sign-in, account and profile
- ✅ Sign-in by phone and code, two-step verification (2FA), QR-code sign-in (with DC migration)
- ✅ Log out; session and credential files owner-only (0600)
- ✅ Active sessions: list, terminate one or all others
- 🟡 Multiple accounts: menu under the name, adding, switching — **P1**: notifications and counters from inactive accounts (all accounts connected at once)
- ✅ Profile editing in Settings → My Account: first/last name and bio (`account.updateProfile`), username with a live availability check (`account.checkUsername`, `account.updateUsername`), photo from a file, center-cropped (`photos.uploadProfilePhoto`)
- ⬜ **P1** Birthday, personal channel in the profile, multiple usernames (`reorderUsernames`), video avatar
- ⬜ **P1** 2FA management: enable, change the password and recovery email (`account.updatePasswordSettings`)
- ⬜ **P2** Passkeys (`account.registerPasskey`), sign-in with an email code
- ⬜ **P2** Name colour and profile background, emoji status (`account.updateColor`, `updateEmojiStatus`; partly Premium)
- ⬜ **P2** Change number, self-destruct when inactive (`setAccountTTL`), delete account
- ⬜ **P2** Connected websites (Log in with Telegram, `getWebAuthorizations`), Telegram Passport — **P3**

## 2. Chat list and navigation
- ✅ Dialogs: unread, pinned, archive, search by name, cached avatars
- ✅ Folders: showing, creating, editing, deleting
- ✅ Global message search; public chats by name, @mention or t.me link; "Join"
- ✅ Pin, mute, read / unread, archive from the menu
- ✅ Synced drafts, "typing…", "online" / "last seen…"
- ✅ Forums and topics (list, create, rename, close); channel comments
- ✅ Left folder column as in Telegram Desktop (icons, names, unread badges), or tabs on top — switch in Settings → General → Folders
- ✅ Drag to reorder pinned chats (main list, archive, inside a folder) and folders in the column or tabs (`reorderPinnedDialogs`, `updateDialogFiltersOrder`)
- 🟡 New chat (pencil next to search): contacts with search (`contacts.getContacts`, `contacts.search`), new group (`createChat`), new channel (`channels.createChannel`); still missing: group/channel photo, public link and channel type step, adding subscribers to a new channel
- ⬜ **P1** Contacts: list, add, import by number, delete, blocked (`contacts.*`)
- ⬜ **P1** Saved Messages as separate dialogs by source and reaction tags (`getSavedDialogs`, `savedReactionTags`)
- ⬜ **P1** Folder tags in the chat list (`toggleDialogFilterTags`), suggested folders
- ⬜ **P1** Contact birthday reminders (`contacts.getBirthdays`)
- ⬜ **P2** Shared folders by link: join, update, leave (`chatlists.*`)
- ⬜ **P2** Communities (2026): linked groups, channels and bots as one structure (`communities.*`)
- ⬜ **P2** Similar channels (`getChannelRecommendations`), list density modes

## 3. Chat view
- ✅ History, loading more, bubbles with replies, forwards, reactions, albums
- ✅ Photos, videos, GIFs, round videos, voice, audio, files, polls, locations, contacts, bot buttons
- ✅ Multi-select, forwarding, copying, deleting; in-chat search
- ✅ Link previews (`webPage`: site, title, description, picture) in bubbles; click opens the link
- ✅ Pinned messages: glass bar under the header (position segments, click jumps and cycles to the previous one), list of all pinned, pin / unpin from the message menu (`updatePinnedMessage`, notify / only-for-me options), unpin all, live `updatePinnedMessages` / `updatePinnedChannelMessages`; non-admins can hide the bar
- 🟡 "Down" button with a counter of messages that arrived while scrolled up; "@" and "❤" buttons walk through unread mentions / reactions (`getUnreadMentions`, `getUnreadReactions`, `readMentions`, `readReactions`, right click — mark all read). Left: opening a chat at the first unread message, per-message mention read (`readMessageContents`), counters from live updates
- ✅ Read ticks ✓ / ✓✓ (`read_outbox_max_id` + `updateReadHistoryOutbox` / `updateReadChannelOutbox`), "Seen by" in the group message menu (`getMessageReadParticipants`), channel view counter with live `updateChannelMessageViews`
- 🟡 Protected content (`noforwards` of chat and message): no Forward / Copy / Download / Open in player in menus, selection bar and viewer, blocked with a toast. Left: restricted content (`restriction_reason`)
- ✅ Virtualised long history: only bubbles near the screen are drawn, the heights of the rest are cached (3000 messages — ≈2 ms per frame; in the demo — "Старий проєкт")
- ✅ Right info column: profile, members, shared media, links, files, voice — see sections 5 and 6
- ⬜ **P1** Sponsored messages in channels (`getSponsoredMessages`, `viewSponsoredMessage`) — **required** by the API terms of service
- ⬜ **P1** Jump to a date via a calendar (`getSearchResultsCalendar`), search filters by type and sender
- ⬜ **P1** Translating messages and chats (`translateText`, `togglePeerTranslations`)
- ⬜ **P1** Reply quoting part of the text, reply in another chat
- ⬜ **P2** Chat wallpapers and themes, including custom and gift ones (`setChatWallPaper`, `setChatTheme`)
- ⬜ **P2** Instant View for articles; 2026 rich text messages (`getRichMessage`, `translateRichMessage`)
- ⬜ **P2** AI summaries of long posts (`summarizeText`), voice transcription (`transcribeAudio`, Premium)
- ⬜ **P2** Message effects (`availableEffects`), timed and view-once messages
- ⬜ **P2** Member tags (2026), fact checks (`getFactCheck`), greeting messages and buttons in messages (August 2026)
- ⬜ **P3** Emoji games (dice and so on, `getEmojiGameInfo`)

## 4. Composer and editor
- ✅ Multi-line field, markup (`**`, `__`, `~~`, `||`, `` ` ``, links), reply, edit
- ✅ Files, clipboard paste, drag and drop, photo compression, albums, silent and scheduled sending
- ✅ Voice and round video messages with review; stickers, emoji, inline bots
- 🟡 @mention autocomplete (`channels.getParticipants` search / basic group members) and bot commands via `/` (`bot_info` from `users.getFullUser` / `messages.getFullChat` / `channels.getFullChannel`): glass popup, ↑/↓/Enter/Tab, click. Left: users without a username are inserted as plain name, not a `mentionName` entity
- ✅ Link preview while typing (`messages.getWebPagePreview`, debounced) above the field; ✕ sends with `no_webpage`
- ⬜ **P1** Visual rich text editor (2026): formatting bar above the selection, custom shareable styles
- ⬜ **P1** Emoji suggestions by keyword (`getEmojiKeywords`), custom emoji (showing — P1, sending — Premium P2)
- ⬜ **P1** Creating polls and quizzes (including participant limits and stats, 2026), checklists (`appendTodoList`)
- ⬜ **P1** Spoiler on photos and videos, caption above media, replacing the attachment while editing
- ⬜ **P2** Sending as a channel (`saveDefaultSendAs`), "send when online", location and contact
- ⬜ **P2** Spell checking (Hunspell dictionaries, works on every OS)
- ⬜ **P3** AI text editor: translate, rewrite, tones (`composeMessageWithAI`, `aicompose.*`)
- ⬜ **P3** Paid messages (`updatePaidMessagesPrice`, `allow_paid_stars`)

## 5. Media and stickers
- ✅ Photo viewer, video in the bubble, webm stickers and GIFs, voice and audio player
- 🟡 Stickers: panel, sending; **P1** Lottie/TGS animation (our own Lottie renderer or `rlottie` via FFI)
- ✅ **P0** Shared chat media in chat info: photo/video grid, files, links, voice, GIF tabs with counters (`messages.search` + `InputMessagesFilter*`, `getSearchCounters`)
- ⬜ **P1** Full-screen video player: seeking, volume, speed, picture-in-picture
- ⬜ **P1** Streaming large videos (downloading in parts instead of the whole file)
- ⬜ **P1** Animated and custom emoji in text (`getCustomEmojiDocuments`), animated reactions
- ⬜ **P1** Sticker sets: browse, install, order, search (`searchStickerSets`), favourite and saved GIFs
- ⬜ **P1** Media auto-download and cache management (size, clearing by type)
- ⬜ **P2** Music player with a queue and a playback bar; music in the profile (`saveMusic`, 2026)
- ⬜ **P2** Simple photo editor: crop, rotate, draw; Live Photos (2026)
- ⬜ **P3** Creating your own sticker sets (`stickers.createStickerSet`)

## 6. Groups and channels
- ✅ **P0** Chat info column (click the header): description/bio, @username, phone, invite link (`exportChatInvite`), member and admin list with search, click opens a private chat (`getFullUser` / `getFullChat` / `getFullChannel`, `channels.getParticipants`)
- ✅ **P0** Leave a group or channel, clear history, delete chat with confirmation and "also for the other side" (`leaveChannel`, `deleteChatUser`, `deleteHistory`, `channels.deleteHistory`); the owner gets "Delete group" / "Delete channel" instead of "Leave", for everyone (`channels.deleteChannel`, `messages.deleteChat`)
- ⬜ **P1** Editing: title, photo, description; default member permissions; slow mode
- ⬜ **P1** Administration: admin rights, bans and restrictions, join requests, invite link management
- ⬜ **P1** Reactions allowed in the chat; hiding the member list; anti-spam
- ⬜ **P2** Admin action log (`getAdminLog`), linking a discussion group, author signatures in channels
- ⬜ **P2** Channel boosts (`premium.applyBoost`), transferring group ownership (2026)
- ⬜ **P3** Channel statistics (`stats.*`), channel auto-translation, paid messages, managing sponsored messages

## 7. Bots and mini apps
- ✅ Inline mode, callback and URL buttons, regular bot keyboard
- 🟡 Bot command menu button next to the field and the "Start" bar in an empty bot chat (sends `/start`). Left: deep-link start parameters (`messages.startBot` with `start_param`)
- ⬜ **P1** **Mini apps (Web Apps)**: embedded `wry` browser (WebView2 / WebKitGTK / WKWebView — on every OS)
      with a `window.Telegram.WebApp` bridge: theme, MainButton, BackButton, popups, cloud storage, contact and location requests
      (`requestWebView`, `requestAppWebView`, `requestMainWebView`, `invokeWebViewCustomMethod`)
- ⬜ **P1** Bot menu button (`setBotMenuButton`), direct mini app links (`t.me/bot/app`)
- ⬜ **P2** Bots in the attachment menu (`getAttachMenuBots`), logging in to websites via Telegram (`requestUrlAuth`)
- ⬜ **P2** Payments in bots: invoices, payment form, Stars (`payments.getPaymentForm`, `sendStarsForm`)
- ⬜ **P2** Guest AI bots that can be mentioned in any chat (2026); ephemeral bot replies in groups (`ephemeral.*`)
- ⬜ **P3** Games (`setGameScore`), business bots, verification by bots, referral programs

## 8. Stories
- ⬜ **P1** Story strip above the chat list and a viewer: swiping, pause, reply, reaction (`stories.getAllStories`, `readStories`)
- ⬜ **P2** Posting a story from a photo or video, caption, privacy (`stories.sendStory`)
- ⬜ **P2** Pinned stories and albums in the profile, archive, view statistics
- ⬜ **P3** Anonymous viewing (Premium), live streams in stories (`stories.startLive`)

## 9. Gifts, Stars, Premium
- ⬜ **P1** Gifts in profiles: showing, details, sender's note (`getSavedStarGifts`)
- ⬜ **P2** Stars balance, transaction history, top-up via invoice (`getStarsStatus`, `getStarsTransactions`)
- ⬜ **P2** Sending gifts (`getStarGifts`, `checkCanSendGift`), paid ⭐ reactions and paid media
- ⬜ **P3** Unique gifts: upgrading, transferring, reselling, collections, auctions, crafting (2026)
- ⬜ **P3** Giveaways, Premium gift codes, channel subscriptions for Stars
- ⬜ **P3** TON wallet and Fragment collectible usernames (`wallet.*`, `fragment.*`) — only after a separate security review

## 10. Calls
Needs WebRTC and Telegram's protocol: FFI to `tgcalls` (C++) or pure Rust (`webrtc-rs`) + Telegram signalling.
- ⬜ **P1** 1-on-1 voice calls: `phone.requestCall` / `acceptCall` / `confirmCall`, DH key exchange, call window, incoming call as a notification
- ⬜ **P2** Video calls and screen sharing
- ⬜ **P2** Group calls and live streams in channels (`phone.joinGroupCall`); end-to-end encrypted conference calls (`createConferenceCall`)
- ⬜ **P2** Call log, quality rating
- ⬜ **P3** RTMP streams, call recording

## 11. Privacy and security
- ✅ Privacy (phone number, last seen, photo, forwards, calls, voice messages, invites), local passcode
- ⬜ **P1** Exceptions in privacy rules (allow or deny specific people and groups)
- ⬜ **P1** Global settings: archive new non-contacts, hide read time, who can message me (`setGlobalPrivacySettings`)
- ⬜ **P1** Reporting spam and content (`messages.report`), blocking from the chat
- ⬜ **P2** End-to-end encrypted secret chats (`requestEncryption`, MTProto 2.0 E2E)
- ⬜ **P2** Default and per-chat message auto-delete (`setDefaultHistoryTTL`, `setHistoryTTL`)
- ⬜ **P2** Encrypting local data with the local passcode (for now the passcode only hides the interface)
- ⬜ **P3** Sensitive content (`setContentSettings`)

## 12. Notifications
- ✅ System notifications, counter in the title, tray with an unread dot
- 🟡 **P0** Per-chat notifications in chat info: mute for 1 h / 8 h / 2 days / forever, unmute, silent, current state (`updateNotifySettings`, `getNotifySettings`); still missing: mentions-only and the exceptions list
- ⬜ **P1** Reply and "mark as read" right from the notification (where the OS allows), grouping by chat
- ⬜ **P1** Notification sounds and custom ringtones (`saveRingtone`), reaction notification settings
- ⬜ **P2** Unread badge on the taskbar icon (Windows, macOS, Unity)

## 13. Telegram Business
- ⬜ **P3** Opening hours, address, greeting and away messages, quick replies, chat links, connected bots (`account.updateBusiness*`, `quickReplies`)

## 14. Settings (structure as in Telegram Desktop)
- ✅ Language, theme, scale, notifications, autoplay, camera, SOCKS5 proxy, privacy, devices
- ✅ Sections as in Telegram Desktop: a main page with the profile card and My Account · Notifications · Privacy and Security · Chat Settings · Folders · Advanced · Devices · Language, interface scale at the bottom; each section is a page with a back button (Esc goes back)
- ⬜ **P1** Chat settings: text size, Enter or Ctrl+Enter, sticker suggestions, looping animations
- ⬜ **P1** Interface in every language Telegram Desktop offers: language list from the server (`langpack.getLanguages`, incl. beta/community packs from translations.telegram.org), strings from `langpack.getLangPack` / `getDifference`, cached on disk, live switching without restart. Map our `l!`/`lf!` strings to official tdesktop keys so translations come from Telegram; built-in uk/en stay as the offline fallback; plural forms per language (CLDR rules)
- ⬜ **P1** MTProto proxy (obfuscated transport), several proxies with quick switching
- ⬜ **P1** Storage usage and cache limits, auto-download by network type
- ⬜ **P2** Full account data export (`account.initTakeoutSession`), importing history from other messengers (`initHistoryImport`)
- ✅ Exporting a single chat's history (HTML and JSON)

## 15. Interface and "liquid glass"
- ✅ Glass panels, buttons, fields, bubbles, avatars, live background, GPU blur, light and dark themes
- ✅ Vector icons (Lucide), icons in context menus as in Telegram Desktop, anti-aliasing for the screen density, built-in font
- ✅ Hotkeys as in Telegram Desktop: Ctrl+F (search in chat, or chat list), Ctrl+K (chat list search), Ctrl+1…8 folder by position and Ctrl+9 the last one, Alt+↑/↓, Ctrl+Tab / Ctrl+Shift+Tab and Ctrl+PgUp/PgDn (previous/next chat), Esc (close the topmost panel → clear search → close the chat), Ctrl+E or ↑ in an empty composer (edit the last own message)
- ⬜ **P1** Glass context menus, tooltips and popups instead of the stock egui ones
- ⬜ **P1** Blurring chat content under the header and composer (capturing the frame into a wgpu texture)
- ⬜ **P1** "Reduce transparency" and "Increase contrast" (as on Apple), "Reduce motion"
- ⬜ **P1** Animations: spring transitions, bubbles appearing, switching folders (150–350 ms)
- ⬜ **P1** Wide mode with a third column (chat info) and a narrow mode for small windows
- ⬜ **P2** Accessibility: AccessKit (screen readers), full keyboard navigation
- ⬜ **P3** Right-to-left languages

## 16. Reliability and performance
- ✅ Local cache of dialogs and messages (SQLite, `chats.sqlite` next to the session, 0600): chats and history from the cache in ~10 ms before connecting, scrolling without a network; up to 1000 messages per chat
- 🟡 Offline send queue for text messages: the bubble appears at once with a clock, network errors are retried with growing delays (2, 4, 8, 16 s), then a red mark with Retry / Delete in the menu (click retries); the queue lives in `chats.sqlite` and survives a restart (failed ones stay failed). **P1**: queue files and voice messages too; reuse the same `random_id` on retry so a lost reply can't produce a duplicate (needs a raw `messages.sendMessage` instead of grammers' `send_message`)
- ⬜ **P1** LRU limit for textures and memory, unloading hidden images
- ⬜ **P1** Backend tests on recorded server replies, CI (Linux, Windows, macOS)
- ⬜ **P2** Error log to a file and a report for the developer (no personal data)
- ⬜ **P2** Updating the API layer together with grammers

## 17. Platforms and distribution

**Windows**
- ⬜ Notifications: `notify-send` → Windows toast notifications (`notify-rust` or `winrt-notification`)
- ⬜ Opening files and links: `xdg-open` → the `open` crate (new code already does this; file downloads and bot buttons are left)
- ⬜ Data and Downloads folders via the `dirs` crate (export already does this)
- ⬜ Tray: `ksni` → `tray-icon`; on Windows the window can be minimised to the tray
- ⬜ Audio: `-f pulse` → `-f dshow` for voice messages (round videos already support DirectShow)
- ⬜ ffmpeg/ffplay next to the `.exe` or found in `PATH`
- ⬜ Build and CI for `x86_64-pc-windows-msvc`, checking the wgpu blur on DX12/Vulkan
- ⬜ Installer (MSI via `cargo-wix` or Inno Setup), `.ico` icon, code signing if possible

**Flathub**
- ⬜ ID `io.github.<owner>.Glassgram`, a manifest with `rust-stable` and offline dependencies (`flatpak-cargo-generator`)
- ⬜ ffmpeg from `org.freedesktop.Platform.ffmpeg-full`
- ⬜ Permissions: network, Wayland/X11, `dri`, PulseAudio, camera, D-Bus for the tray, `xdg-download`
- ⬜ Portals: notifications, opening files, file chooser, camera
- ⬜ `.desktop`, `hicolor` icons, AppStream metainfo with screenshots, a name that doesn't suggest it is official
- ⬜ Submitting a PR to `flathub/flathub`

**macOS** — ⬜ **P3**: build, `.app`/`.dmg`, signing and notarisation

## Not planned
- SMS jobs (`smsjobs`), managing ad accounts — not needed in a personal client
- Mobile platforms — a different interface and a different stack

---

## Time estimate

**What is done.** Sign-in, chats, folders, forums, comments, every media type, voice and round
video recording, settings, two languages, two themes, multiple accounts — and now the P0 stage:
profile, new chat, chat info, link previews, pinned messages, read ticks, cache and send queue,
virtualisation, shared media, hotkeys, per-chat notifications. Checked in demo mode and on a
real account, including a public group (members, @mention popup, join / leave) and test groups
and channels (create, invite link, pin, "Seen by", clear, delete).

**How we count.** A "day" is 6–8 hours of active work by a person with an agent: setting the task,
checking with screenshots and on a real account, fixing. Judging by what is done so far, that is
about 5–6 thousand lines of working code a day for familiar UI. But the next features are harder
(protocols, encryption, WebRTC), so there is a margin.

| Stage | What it includes | Days with an agent |
|---|---|---|
| **P0** — daily client | profile, new chat, chat info, link previews, pinned, read ticks, cache and queue, virtualisation, shared media, hotkeys, per-chat notifications | **15–20** (mostly done) |
| **P1** — everyday use | mini apps, stories (viewing), Lottie and custom emoji, rich text editor, polls and checklists, translation, administration, language packs, MTProto proxy, glass menus, content blur | **30–40** |
| 1-on-1 voice calls | WebRTC, signalling, encryption, call window | **15–25** (the biggest risk) |
| **P2** — advanced | secret chats, group and video calls, posting stories, Stars and gifts, communities, cache encryption, AI features | **35–50** |
| **P3** — niche | Business, unique gifts and auctions, wallet, statistics, Telegram Passport | **25–35** |
| Platforms | Windows, Flathub, macOS, CI | **10–15** |

**Total:**
- **Daily client (P0 + P1): 45–60 days**, i.e. 2–3 months of working every day.
- **With calls and advanced features (+ P2): another 2–3 months.**
- **Full parity with the official client: 6–9 months** for one person with an agent.

For comparison: Telegram Desktop has been written since 2014 and has over 500 thousand lines of C++.

**What affects the timeline most:**
1. Calls — there is no ready Rust library, FFI to `tgcalls` may be needed.
2. Mini apps — an embedded browser and the full WebApp API bridge.
3. Telegram adds features and changes the API layer every month — part of the time goes to upkeep.
4. The more code, the longer the checking: a selective manual test on a real account is mandatory.
