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
- ✅ Multiple accounts: menu under the name, adding, switching; the other accounts stay connected in the background — their unread counters in the menu, a dot by the name, notifications with the account's name (dialogs refreshed every 5 minutes)
- ✅ Profile editing in Settings → My Account: first/last name and bio (`account.updateProfile`), username with a live availability check (`account.checkUsername`, `account.updateUsername`), photo from a file, center-cropped (`photos.uploadProfilePhoto`)
- ✅ Birthday: our own in My Account — day, month, optional year (`account.updateBirthday`), who sees it under Privacy; a user's birthday in their info
- ✅ A user's personal channel in their info (opens without joining) and their other usernames ("also @…"); our own collectible usernames in "My account": shown or hidden in the profile (`account.toggleUsername`) and their order (`account.reorderUsernames`); video profile photos play in the info panel (`video_sizes`)
- ✅ 2FA management in Privacy and Security → Two-Step Verification: the current password (an SRP proof, `account.getPasswordSettings`) unlocks changing it, its hint and recovery email, or turning it off; the first password with an optional email; the email code with resend and cancel (`account.updatePasswordSettings`, `confirmPasswordEmail`, `resendPasswordEmail`, `cancelPasswordEmail`). The new password goes as `g^x mod p` with our own 32 bytes of salt, checked in a test against the server side of SRP
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
- ✅ New chat (pencil next to search): contacts with search (`contacts.getContacts`, `contacts.search`), new group (`createChat`), new channel (`channels.createChannel`), a photo for either; after a new channel, its type — public with a checked `t.me/` link (`channels.checkUsername`, `updateUsername`) or private with its invite link — and the first subscribers from contacts (`channels.inviteToChannel`)
- ✅ Contacts: list with search (pencil → Contacts), "New contact" by name and phone (`importContacts`), "Add to contacts" / "Delete contact" in user info (`addContact`, `deleteContacts`), block / unblock and stop / restart bots (`contacts.block`, `unblock`), blocked list at the top of Privacy and Security (`getBlocked`)
- ✅ Saved Messages by source: a strip of "All" and the chats messages were saved from, each with its own history (`getSavedDialogs`, `getSavedHistory`)
- ✅ Saved Messages reaction tags: capsules with the emoji, name and count after the sources (`getSavedReactionTags`), a click shows only messages with that tag (`messages.search` with `saved_reaction`, combined with the source), right click → rename (`updateSavedReactionTag`, Premium), live `updateSavedReactionTags`. Custom emoji tags wait for custom emoji support
- ✅ Folder tags in the chat list: a tag colour in the folder editor (7 colours or none, kept on save), coloured capsules of the other folders a chat is in on its row, the switch in Settings → Folders (`toggleDialogFilterTags`, Premium; `tags_enabled`); recommended folders with "Add" (`getSuggestedDialogFilters`)
- ✅ Contact birthday reminders (`contacts.getBirthdays`): a card above the main list opens the chat, hidden until tomorrow with ×
- ⬜ **P2** Shared folders by link: join, update, leave (`chatlists.*`)
- ⬜ **P2** Communities (2026): linked groups, channels and bots as one structure (`communities.*`)
- ✅ Similar channels in a channel's info (`channels.getChannelRecommendations`): a tab with subscriber counts, a click opens the channel without joining, the Premium-only rest is counted
- ✅ List density: compact, standard and large chat rows (Settings → Chat Settings → Chat list)

## 3. Chat view
- ✅ History, loading more, bubbles with replies, forwards, reactions, albums
- ✅ Photos, videos, GIFs, round videos, voice, audio, files, polls, locations, contacts, bot buttons
- ✅ Multi-select, forwarding, copying, deleting; in-chat search
- ✅ Link previews (`webPage`: site, title, description, picture) in bubbles; click opens the link
- ✅ Pinned messages: glass bar under the header (position segments, click jumps and cycles to the previous one), list of all pinned, pin / unpin from the message menu (`updatePinnedMessage`, notify / only-for-me options), unpin all, live `updatePinnedMessages` / `updatePinnedChannelMessages`; non-admins can hide the bar
- ✅ "Down" button with a counter of messages that arrived while scrolled up; "@" and "❤" buttons walk through unread mentions / reactions (`getUnreadMentions`, `getUnreadReactions`, `readMentions`, `readReactions`, right click — mark all read). A chat with unread messages opens at the first of them under an "Unread messages" divider (by `read_inbox_max_id`, loading back to it if needed). A mention is marked read when jumped to (`readMessageContents`); new mentions and reactions to our messages raise the counters live, or are read at once when the chat is in view
- ✅ Read ticks ✓ / ✓✓ (`read_outbox_max_id` + `updateReadHistoryOutbox` / `updateReadChannelOutbox`), "Seen by" in the group message menu (`getMessageReadParticipants`), channel view counter with live `updateChannelMessageViews`
- ✅ Protected content (`noforwards` of chat and message): no Forward / Copy / Download / Open in player in menus, selection bar and viewer, blocked with a toast. Messages restricted by Telegram (`restriction_reason` for all platforms) show the reason instead of their content; a whole restricted chat, channel or bot shows the reason instead of its history
- ✅ Virtualised long history: only bubbles near the screen are drawn, the heights of the rest are cached (3000 messages — ≈2 ms per frame; in the demo — "Старий проєкт")
- ✅ Right info column: profile, members, shared media, links, files, voice — see sections 5 and 6
- ✅ Sponsored messages in channels — **required** by the API terms of service: an "Ad" / "Recommended" bubble after the newest post with title, text, picture and button, views counted once per session (`viewSponsoredMessage`), clicks (`clickSponsoredMessage`), "About this ad" with the advertiser info, reporting with the server's reasons (`reportSponsoredMessage`), refreshed at most every 5 minutes; further ads every `posts_between` posts above the newest, media ads with their photo or video cover, and "Hide ads" for Premium (`account.toggleSponsoredMessages`, `sponsored_enabled`)
- ✅ Jump to a date: a month calendar from the calendar button in chat search or a click on a date chip; finds the first message of the day (`messages.getHistory` with `offset_date`) and scrolls to it
- ✅ In-chat search filters: Media, Files, Links, Voice, GIF chips under the search field and "From…" in groups (member search, `messages.search` with `from_id`); results without a query. Days with photos and videos are dotted in the jump-to-date calendar (`getSearchResultsCalendar`)
- ✅ Translating a message: "Translate" in the message menu opens a window with the original, the translation and the target language (`messages.translateText`); works without Premium
- ⬜ **P2** Translating a whole chat inline (Premium, `togglePeerTranslations`)
- ✅ Reply quoting part of the text and reply in another chat, as in Telegram Desktop: a click on the reply bar opens the message to select the quote (with its formatting: `quote_text`, `quote_entities`, `quote_offset`) or to pick another chat (`reply_to_peer_id`). Works for text (queued and scheduled), photos, albums, files, voice and round messages, stickers, polls and inline results; such replies go as our own `sendMessage` / `sendMedia` / `sendMultiMedia`, plain ones keep the grammers path. Received quotes and replies to other chats are shown in the reply header; a quote the server rejects (the message was edited) falls back to a plain reply
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
- ✅ @mention autocomplete (`channels.getParticipants` search / basic group members) and bot commands via `/` (`bot_info` from `users.getFullUser` / `messages.getFullChat` / `channels.getFullChannel`): glass popup, ↑/↓/Enter/Tab, click. Users without a username are inserted as `[name](tg://user?id=…)` and sent as a real mention (`inputMessageEntityMentionName`); a click on such a mention opens the chat with that person
- ✅ Link preview while typing (`messages.getWebPagePreview`, debounced) above the field; ✕ sends with `no_webpage`
- ⬜ **P1** Visual rich text editor (2026): formatting bar above the selection, custom shareable styles
- ✅ Emoji suggestions by keyword: typing `:word` offers emoji from Telegram's English and interface-language keyword lists (`getEmojiKeywords`); ↑/↓, Enter, Tab or a click puts the emoji in place of the word
- ⬜ **P1** Custom emoji (showing — P1, sending — Premium P2)
- ✅ Creating polls and quizzes: "Poll" in the 📎 menu (right click) opens the "New poll" box — question, up to 10 answers, anonymous voting, multiple answers, quiz mode with the correct answer and an explanation (`messages.sendMedia` + `inputMediaPoll`)
- ⬜ **P1** Polls with participant limits and stats (2026), checklists (`appendTodoList`)
- ✅ "Send files" box as in Telegram Desktop: previews of photos and file rows (remove one with ✕, more files dropped or pasted join it), the caption, "Compress images", "Hide with spoiler" (`spoiler` on `inputMediaUploadedPhoto` / the video document) and "Caption above media" (`invert_media`); in the chat, media under a spoiler is a blurred copy with drifting dots until clicked, and a caption above the media is drawn above it
- ✅ Editing media messages: the caption (may be left empty), and the photo or file replaced from the "Editing" bar (right click — without compression) or by dropping a file on the chat; keeps the spoiler and caption position (`messages.editMessage` with `media`), the old cached pictures and files are dropped
- ⬜ **P2** Sending as a channel (`saveDefaultSendAs`), "send when online", location and contact
- ⬜ **P2** Spell checking (Hunspell dictionaries, works on every OS)
- ⬜ **P3** AI text editor: translate, rewrite, tones (`composeMessageWithAI`, `aicompose.*`)
- ⬜ **P3** Paid messages (`updatePaidMessagesPrice`, `allow_paid_stars`)

## 5. Media and stickers
- ✅ Photo viewer, video in the bubble, webm stickers and GIFs, voice and audio player
- ✅ Stickers: panel, sending; animated `.tgs` stickers play in the chat (pure Rust: `velato` reads Lottie, `vello_cpu` draws it on a thread of its own; file sizes and repeaters are capped, a file we can't draw stays a still picture)
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
- ✅ Editing a group or channel (pencil in chat info, for admins who may change info): title, photo, description; default member permissions; slow mode in supergroups (`editTitle` / `editChatTitle`, `editChatAbout`, `editPhoto`, `editChatDefaultBannedRights`, `toggleSlowMode`)
- ⬜ **P1** Administration: admin rights, bans and restrictions, join requests, invite link management
- ⬜ **P1** Reactions allowed in the chat; hiding the member list; anti-spam
- ⬜ **P2** Admin action log (`getAdminLog`), linking a discussion group, author signatures in channels
- ⬜ **P2** Channel boosts (`premium.applyBoost`), transferring group ownership (2026)
- ⬜ **P3** Channel statistics (`stats.*`), channel auto-translation, paid messages, managing sponsored messages

## 7. Bots and mini apps
- ✅ Inline mode, callback and URL buttons, regular bot keyboard
- ✅ Bot command menu button next to the field and the "Start" bar in an empty bot chat (sends `/start`); `t.me/<bot>?start=<param>` links open the bot with a Start bar that passes the parameter (`messages.startBot`)
- 🟡 **Mini apps (Web Apps)**: a separate window process (`glassgram --webview`) with the system WebView through `wry`
      (WebView2 / WebKitGTK / WKWebView); opened from inline and keyboard buttons, the bot's menu button and `t.me/bot/app`,
      `t.me/bot?startapp` links (`requestWebView`, `requestSimpleWebView`, `requestMainWebView`, `requestAppWebView`, `prolongWebView`);
      bridge: theme, viewport, main / secondary / back buttons, popups, `sendData`, cloud storage and other custom methods
      (`invokeWebViewCustomMethod`), links (web only), fullscreen; write access (`bots.canSendMessage`, `allowSendMessage`) and the phone number (sent as a contact) only after a question in our own window, never inside the page; reading the clipboard is refused, and location, biometry and the QR scanner answer "unavailable", as on desktop. Left: payments (`openInvoice`) — only after a separate security review
- ⬜ **P2** Bots in the attachment menu (`getAttachMenuBots`), logging in to websites via Telegram (`requestUrlAuth`)
- ⬜ **P2** Payments in bots: invoices, payment form, Stars (`payments.getPaymentForm`, `sendStarsForm`)
- ⬜ **P2** Guest AI bots that can be mentioned in any chat (2026); ephemeral bot replies in groups (`ephemeral.*`)
- ⬜ **P3** Games (`setGameScore`), business bots, verification by bots, referral programs

## 8. Stories
- ✅ Story strip above the chat list (ring while unseen) and a viewer: progress segments, auto-advance, click/arrows to move between stories and people, Space pauses, seen marks (`stories.getAllStories`, `getPeerStories`, `readStories`); video stories play with sound (downloaded via `getStoriesByID`, the progress follows their length), a private reply to people (`inputReplyToStory`; typing holds the story), a reaction with ❤ or the quick row on right click (`stories.sendReaction`)
- ⬜ **P2** Posting a story from a photo or video, caption, privacy (`stories.sendStory`)
- ⬜ **P2** Pinned stories and albums in the profile, archive, view statistics
- ⬜ **P3** Anonymous viewing (Premium), live streams in stories (`stories.startLive`)

## 9. Gifts, Stars, Premium
- ✅ Gifts in profiles: a "Gifts" tab with the gift stickers, price or collectible number, sender and note on hover (`getSavedStarGifts`)
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
- ✅ Exceptions in privacy rules: "Always allow" / "Never allow" lists under each rule with people and groups, added through a search picker, removed with ✕; changing the preset keeps them (`inputPrivacyValueAllow/DisallowUsers`, `…ChatParticipants`)
- ✅ Global settings on the Privacy and Security page: archive and mute new non-contacts (keep unmuted / folder chats archived), hide read time, who can message me (`get/setGlobalPrivacySettings`; other server fields are kept)
- ✅ Blocking from user info; reporting messages in groups and channels from the message menu: the server's reasons, sub-reasons and a comment step (`messages.report`)
- ⬜ **P2** End-to-end encrypted secret chats (`requestEncryption`, MTProto 2.0 E2E)
- ⬜ **P2** Default and per-chat message auto-delete (`setDefaultHistoryTTL`, `setHistoryTTL`)
- ⬜ **P2** Encrypting local data with the local passcode (for now the passcode only hides the interface)
- ✅ Sensitive content: "Disable filtering" in Privacy and Security (`account.getContentSettings` / `setContentSettings`), greyed out for accounts that can't change it
- ✅ Hardening against untrusted content (audit 2026-10): links open only https/http/mailto/tg; sender file names can't leave Downloads; received programs and scripts are shown in their folder, not run; mini apps can't open local paths, read the clipboard or navigate to `file:`; recordings live in an owner-only temp folder
- ✅ Confirmation for hidden links whose text differs from the target (`[text](url)`): the real address in a tooltip and in the "Open this link?" box, as in Telegram Desktop
- ✅ Warning before opening a received program or script: "Run this file?" with "Show in folder" as the safe default
- ⬜ **P2** Mini app permissions asked from the user: clipboard, location, contact, biometry (`requestWriteAccess`, `requestContact`)

## 12. Notifications
- ✅ System notifications, counter in the title, tray with an unread dot
- ✅ **P0** Per-chat notifications in chat info: mute for 1 h / 8 h / 2 days / forever, unmute, silent, current state (`updateNotifySettings`, `getNotifySettings`)
- ✅ Notifications for chats in Settings → Notifications, as in Telegram Desktop: private chats, groups and channels on / off (`inputNotifyUsers` / `Chats` / `Broadcasts`), the exceptions of each with "Remove exception" (`getNotifyExceptions`); chats without their own setting follow their category, live `updateNotifySettings` from other devices. (MTProto has no per-chat "mentions only" mode.)
- ⬜ **P1** Reply and "mark as read" right from the notification (where the OS allows), grouping by chat
- ⬜ **P1** Notification sounds and custom ringtones (`saveRingtone`), reaction notification settings
- ⬜ **P2** Unread badge on the taskbar icon (Windows, macOS, Unity)

## 13. Telegram Business
- ⬜ **P3** Opening hours, address, greeting and away messages, quick replies, chat links, connected bots (`account.updateBusiness*`, `quickReplies`)

## 14. Settings (structure as in Telegram Desktop)
- ✅ Language, theme, scale, notifications, autoplay, camera, SOCKS5 proxy, privacy, devices
- ✅ Sections as in Telegram Desktop: a main page with the profile card and My Account · Notifications · Privacy and Security · Chat Settings · Folders · Advanced · Devices · Language, interface scale at the bottom; each section is a page with a back button (Esc goes back)
- ✅ Chat settings: message text size (small / standard / large / huge; poll, location and contact cards grow with it), sending with Enter or Ctrl+Enter (Cmd+Enter on macOS), "Suggest stickers by emoji" (installed stickers for an emoji typed alone, above the field), "Loop animated stickers"
- ✅ Localisation files: `locales/<code>.json` keyed by the English string, built-in or dropped into the data folder, picked in Settings → Language, English fallback, generated template; German built in
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
- ✅ Offline send queue for text messages, files, voice and round video messages: the bubble appears at once with a clock, network errors are retried with growing delays (2, 4, 8, 16 s), then a red mark with Retry / Delete in the menu (click retries); the queue lives in `chats.sqlite` and survives a restart (failed ones stay failed). Retries reuse the queue key as `random_id` (raw `messages.sendMessage`), so a lost reply can't post a message twice; the server's `RANDOM_ID_DUPLICATE` counts as sent. Files are copied into the account's `outbox/<key>/` folder first (recordings moved), uploaded with progress at every attempt and sent as our own requests whose `random_id`s come from the key; folders of messages no longer queued are pruned at start
- ✅ Memory budget for pictures (192 MB): the least recently drawn ones that were off screen for 30 s are dropped and decoded again from the disk cache when they come back into view (`app/textures.rs`, `Cmd::ReloadImages`)
- ⬜ **P1** Backend tests on recorded server replies, CI (Linux, Windows, macOS)
- ⬜ **P2** Error log to a file and a report for the developer (no personal data)
- ⬜ **P2** Updating the API layer together with grammers

## 17. Platforms and distribution

**Shared code (`platform.rs`)** — compiles for Windows and macOS (type-checked), not yet run there
- ✅ Notifications: `notify-rust` (D-Bus via zbus on Linux, toasts on Windows, Notification Center on macOS)
- ✅ Opening files and links via the `open` crate everywhere (downloads, bot buttons, files)
- ✅ Data and Downloads folders via `dirs` (`%APPDATA%\glassgram`, `~/Library/Application Support/glassgram`)
- ✅ Tray: `ksni` on Linux, `tray-icon` on Windows and macOS behind one `TrayHandle`
- ✅ Microphone: PipeWire (via `pw-record`) or PulseAudio on Linux, DirectShow on Windows, AVFoundation on macOS (voice and video messages)
- ✅ ffmpeg/ffplay next to the executable, in `PATH` or Homebrew; no console windows on Windows, no console for the app itself
- ⬜ **P1** Run on real Windows and macOS: tray, notifications, recording, wgpu blur (DX12/Metal)
- ✅ Closing the window hides it to the tray on Windows, the tray icon brings it back, "Quit" exits (not run on Windows yet)

**Windows**
- ⬜ Build and CI for `x86_64-pc-windows-msvc`, checking the wgpu blur on DX12/Vulkan
- ✅ Installer (Inno Setup, English and Ukrainian) with ffmpeg, exe icon, built in GitHub Actions with the API keys from secrets
- ⬜ Code signing (SmartScreen warns about unsigned installers)

**Flathub**
- ✅ Our own `.flatpak` bundle from GitHub Actions: `io.github.petro_pixel.Glassgram`, ffmpeg + x264 built in, `.desktop`, metainfo, icons
- ⬜ Flathub: offline crates (`flatpak-cargo-generator`) and a decision about the API keys (a Flathub manifest is public)
- ✅ ffmpeg (with x264, opus, vpx, ffplay) as a module of the manifest
- ✅ Permissions: network, Wayland/X11, GPU and camera (`--device=all`), PulseAudio + PipeWire, D-Bus for the tray and notifications, `xdg-download`
- ⬜ Portals: notifications, opening files, file chooser, camera
- ✅ `.desktop`, `hicolor` icons, AppStream metainfo (validated) with screenshots of both themes
- ⬜ Submitting a PR to `flathub/flathub`

**macOS** — ⬜ **P3**: `.app`/`.dmg` bundle (with ffmpeg inside or via Homebrew), signing and notarisation

## 18. Code health

Structure rules are in [AGENTS.md](AGENTS.md#architecture). Status of the refactor started in October 2026:

- ✅ `app.rs` split by area (5.4k → 2.1k lines): history, bubbles, composer, recording, media, dialogs, login, accounts, forum, comments, folders
- ✅ `backend.rs` split (3.9k → 1.6k lines): types, loader, convert, updates, upload; `Worker::handle` dispatches command groups to `<feature>_cmd` handlers
- ✅ Feature state owned by its module: stories, saved, birthdays, forum topics, recording, sticker panel, pinned messages, playback, privacy page, history scroll, sign-in form, passcode lock (`App`: 148 → 106 fields)
- ✅ `history::chat_area` (~500 lines) split into `chat_history`, `chat_header`, `chat_bottom` and `message_row`, none over 150 lines
- ✅ `App::handle_event` (302 → 123 lines) and `Worker::handle` (973 → 70 lines) only dispatch to feature modules
- ⬜ **P2** Remaining loose `App` fields: drafts and editing, uploads, forward / schedule boxes, accounts menu, folder editing and dragging, tray
- ⬜ **P2** `App::apply` (~170 lines) and `App::fresh` (the constructor) by feature, like the event handlers
- ⬜ **P2** Tests for `convert.rs` and `format.rs` on recorded server objects; a CI job that builds and tests on Linux, Windows and macOS
- ⬜ **P2** `demo.rs` (1.3k lines) split by feature next to the code it answers for

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
