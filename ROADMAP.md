# Glassgram roadmap

The goal is a full everyday Telegram client: everything the official apps can do, with a
"liquid glass" interface. The list follows the MTProto API schema (layer 230: `messages`,
`account`, `payments`, `stories`, `bots`, `phone`, `wallet`, `communities`…) and Telegram's
updates up to November 2026 (rich text, AI editor, communities, gift crafting, search tabs,
HTML widgets in bots — Telegram Desktop 7.4).

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
- ✅ Account self-destruct when inactive: 1, 3, 6, 12, 18 or 24 months in Privacy and Security (`account.getAccountTTL` / `setAccountTTL`)
- ⬜ **P2** Change number, delete account
- ✅ QR code of a public username (Telegram Desktop 7.3.2): from the profile card in Settings, "My account" and the username row in any chat's info; `https://t.me/<name>` at error correction H with rounded modules, Telegram's logo in the middle and the username under it, six gradient designs; copy the link, copy the image, or save it as a PNG in Downloads (drawn on the CPU at 960×1200, not a screen capture). Without a username the box explains and offers to set one
- ✅ Connected websites in Privacy and Security → "Logged in with Telegram": site, its bot, browser, region and IP, last use; disconnect one or all (`account.getWebAuthorizations`, `resetWebAuthorization(s)`)
- ⬜ **P3** Telegram Passport

## 2. Chat list and navigation
- ✅ Dialogs: unread, pinned, archive, search by name, cached avatars
- ✅ Folders: showing, creating, editing, deleting
- ✅ Global message search; public chats by name, @mention or t.me link; "Join"
- ✅ **P1** Search tabs in global search (Telegram Desktop 7.3): glass capsules under the search field — Chats, Channels (ours plus public ones, `contacts.search` with `broadcasts`), Apps (popular mini app bots, `bots.getPopularAppBots`, plus bots by name), Posts (public channel posts, `channels.searchPosts`, `#hashtag` too; started with Enter or the button since searches are counted, the free daily quota from `checkSearchPostsFlood`, never paid with Stars), Media (`messages.searchGlobal` with the photo/video filter as a grid). Opening an app's main mini app directly and loading more pages are not done yet
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
- ✅ Shared folders by link: `t.me/addlist/…` opens a box to add the folder with the ticked chats, "Check for new chats" in a shared folder's menu, deleting it offers the chats to leave (`chatlists.*`). Sharing our own folders by link is not done yet
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
- 🟡 **P2** Chat wallpapers and themes, as in Telegram Desktop: Settings → Chat Settings → "Chat background" with the server gallery (`account.getWallPapers`: pictures, pattern wallpapers over their gradient fill at their intensity, solid colours and gradients, all drawn locally, files kept in `wallpapers/`), a picture from a file, or our live background; a chat's own wallpaper and theme from its full info (`userFull.wallpaper` / `theme`, `channelFull.wallpaper`, `theme_emoticon`) shown under its messages and fed to the GPU blur, so the glass header and field refract it; "Chat wallpaper and theme" in a private chat's info sets them (`messages.setChatWallPaper`, `messages.setChatTheme`) from emoji themes (`account.getChatThemes`) and owned gift themes (`account.getUniqueGiftChatThemes`). Left: a chat wallpaper from a file (`account.uploadWallPaper`), "for both" (Premium), themes' bubble and accent colours, `updatePeerWallpaper` live updates, channel wallpapers set by boosts
- ⬜ **P2** Instant View for articles; 2026 rich text messages (`getRichMessage`, `translateRichMessage`)
- ⬜ **P2** Rich message content (autumn 2026): inline documents, files and music in the text (music plays as a playlist), buttons inside messages with the link in a tooltip, quote replies to rich messages
- ✅ Messages we can't show yet (`messageMediaUnsupported`, games, invoices, giveaways, paid media, forwarded stories, streams): a card "Glassgram can't show this message yet" with "Check for updates" (the releases page) instead of an empty bubble, as the official apps do
- ⬜ **P2** AI summaries of long posts (`summarizeText`), voice transcription (`transcribeAudio`, Premium)
- 🟡 **P2** Message effects (`availableEffects`), timed and view-once messages, as in Telegram Desktop: a message with an `effect` shows the effect's emoji by its time and plays the effect's `.tgs` over the chat once when it first appears (new or unread; `messages.getAvailableEffects`, the effect's own animation or its sticker's premium "f" animation, drawn by `lottie.rs`); a click on the emoji replays it. Right-clicking Send in a private chat shows the effects row (Premium ones starred) and sends the text with `effect`. View-once and timed photos and videos (`ttl_seconds`) show a blurred cover with the "1" mark or the timer; a click opens the received one in the viewer or the player (`readMessageContents`) and once closed it reads "Photo/Video has expired", as does media the server already expired; ours stay covered. The "Send files" box sends one photo or video "view once" (`ttl_seconds` = `0x7FFFFFFF`) in private chats. Left: effects on media, stickers and round messages and in the send box; the effect preview before sending; the countdown of timed media while open; view-once voice and round messages (received ones play normally, sending them isn't offered)
- ⬜ **P2** Member tags (2026), fact checks (`getFactCheck`), greeting messages and buttons in messages (August 2026)
- ✅ Emoji games: a message of just 🎲 🎯 🏀 ⚽ 🎰 or 🎳 goes as `inputMediaDice`; the bubble shows the outcome (score, goal, strike, the slot machine's reels)
- ⬜ **P3** Dice games with TON stakes (`getEmojiGameInfo`) — only after the payments security review

## 4. Composer and editor
- ✅ Multi-line field, markup (`**`, `__`, `~~`, `||`, `` ` ``, links), reply, edit
- ✅ Files, clipboard paste, drag and drop, photo compression, albums, silent and scheduled sending
- ✅ Voice and round video messages with review; stickers, emoji, inline bots
- ✅ @mention autocomplete (`channels.getParticipants` search / basic group members) and bot commands via `/` (`bot_info` from `users.getFullUser` / `messages.getFullChat` / `channels.getFullChannel`): glass popup, ↑/↓/Enter/Tab, click. Users without a username are inserted as `[name](tg://user?id=…)` and sent as a real mention (`inputMessageEntityMentionName`); a click on such a mention opens the chat with that person
- ✅ Link preview while typing (`messages.getWebPagePreview`, debounced) above the field; ✕ sends with `no_webpage`
- 🟡 **P1** Visual rich text editor (2026): the message field shows its markup styled as it will be sent (bold, italic, strikethrough, monospace, spoiler, link labels; the markers stay, faded); selecting text shows a glass formatting bar above it — bold, italic, strikethrough, monospace, spoiler, link, clear formatting — and Telegram Desktop's shortcuts do the same (Ctrl+B / I, Ctrl+Shift+X / M / P / N, Ctrl+K with a selection); pressing a button again takes the formatting off. Left: custom shareable styles, underline and quotes (no markup for them yet)
- ✅ Emoji suggestions by keyword: typing `:word` offers emoji from Telegram's English and interface-language keyword lists (`getEmojiKeywords`); ↑/↓, Enter, Tab or a click puts the emoji in place of the word
- 🟡 **P1** Custom emoji (showing — P1, sending — Premium P2): custom emoji entities in message text are drawn as pictures in place of their fallback emoji (`messages.getCustomEmojiDocuments` in batches of 200, static webp whole, `.tgs` played in the text (see §5), a video emoji's thumbnail), cached by document id; the fallback emoji shows until the picture arrives. Left: sending (Premium), custom emoji in the panel, reactions and Saved Messages tags, the chat list preview
- ✅ Creating polls and quizzes: "Poll" in the 📎 menu (right click) opens the "New poll" box — question, up to 10 answers, anonymous voting, multiple answers, quiz mode with the correct answer and an explanation (`messages.sendMedia` + `inputMediaPoll`)
- 🟡 Checklists and 2026 polls: checklist cards with ticked items and who ticked them, ticking (`messages.toggleTodoCompleted`) and adding items (`messages.appendTodoList`) when the list allows it, "Checklist" in the 📎 menu (`inputMediaTodo`; the Premium error is explained). Polls: "View results" with voters per answer and the time of each vote (`messages.getPollVotes`), adding an answer to open polls (`messages.addPollAnswer`), time left and the quiz explanation on the card; the "New poll" box adds a time limit, voters adding answers, final votes, shuffling, results hidden until the end and subscribers-only voting in channels. The message menu retracts our vote (`messages.sendVote` with no options; not for quizzes or final votes) and stops our own poll after asking (`messages.editMessage` with the poll closed, a quiz naming its correct answer again). Left: media in polls and answers, country limits (`countries_iso2`)
- ✅ "Send files" box as in Telegram Desktop: previews of photos and file rows (remove one with ✕, more files dropped or pasted join it), the caption, "Compress images", "Hide with spoiler" (`spoiler` on `inputMediaUploadedPhoto` / the video document) and "Caption above media" (`invert_media`); in the chat, media under a spoiler is a blurred copy with drifting dots until clicked, and a caption above the media is drawn above it
- ✅ Editing media messages: the caption (may be left empty), and the photo or file replaced from the "Editing" bar (right click — without compression) or by dropping a file on the chat; keeps the spoiler and caption position (`messages.editMessage` with `media`), the old cached pictures and files are dropped
- 🟡 **P2** Sending as a channel (`saveDefaultSendAs`), "send when online", location and contact: "Send when online" in the send button's menu for private chats whose last seen time is visible, as in Telegram Desktop (scheduled at `0x7FFFFFFE`; the scheduled list says "When online"). Left: sending as a channel, location and contact
- ⬜ **P2** Spell checking (Hunspell dictionaries, works on every OS)
- ✅ **P2** Dropping a folder or several files sends them as one archive (Telegram Desktop 7.2.6): a dropped folder shows in the send box with its file count and size and goes as `<folder>.zip`; with two or more items "Send as one archive" packs everything into one. Our own zip writer (`zip.rs`, deflate from `flate2`, stored for already compressed formats, UTF-8 names, symlinks skipped, up to 4 GB) packs on a thread while the box shows "Packing…" and can be cancelled
- ⬜ **P2** Emoji up to Unicode 17 in the panel, suggestions and big-emoji messages
- ⬜ **P3** AI text editor: translate, rewrite, tones (`composeMessageWithAI`, `aicompose.*`)
- ⬜ **P3** Paid messages (`updatePaidMessagesPrice`, `allow_paid_stars`)

## 5. Media and stickers
- ✅ Photo viewer, video in the bubble, webm stickers and GIFs, voice and audio player
- ✅ Stickers: panel, sending; animated `.tgs` stickers play in the chat (pure Rust: `velato` reads Lottie, `vello_cpu` draws it on a thread of its own; file sizes and repeaters are capped, a file we can't draw stays a still picture)
- ✅ **P0** Shared chat media in chat info: photo/video grid, files, links, voice, GIF tabs with counters (`messages.search` + `InputMessagesFilter*`, `getSearchCounters`)
- ✅ **P1** Full-screen video player: clicking a video opens it over the window with play/pause, a seek bar (click or drag, elapsed/total; frames restart from `ffmpeg -ss`), volume and mute (`ffplay -volume`), speed 0.5/1/1.5/2× (`atempo`), full screen (`ViewportCommand::Fullscreen`) and picture-in-picture: a glass window in a corner of the main window that keeps playing while chats are browsed, dragged to another corner. Telegram Desktop keys: Space, ←/→ 5 s, F, Esc. Controls on a dark glass chip that hides while playing. Videos play once downloaded; streaming is a separate item
- ✅ **P1** Streaming large videos: a video from 10 MB plays in the player at once from a local HTTP server (127.0.0.1, an unguessable token per video, `Range` requests) that fetches the 512 KB parts `ffmpeg` / `ffplay` ask for on demand, so seeking jumps ahead; fetched parts fill a sparse file that becomes the ordinary cached file once complete. Seeking far into a not yet fetched part takes a few seconds (parts come one at a time)
- 🟡 **P1** Animated and custom emoji in text (`getCustomEmojiDocuments`), animated reactions: custom emoji in text are drawn as pictures (see §4); animated (`.tgs`) ones play in the text: the loop is drawn once at text size on a thread (30 fps, up to 3 s), kept per document id, only for emoji on screen and at most 24 at a time (frames dropped 10 s after leaving the screen); with autoplay or sticker looping off, or "reduce motion" on, they keep the still first frame. Left: playing video (webm) custom emoji (they show their thumbnail), custom emoji and animated reactions
- ✅ Sticker sets as in Telegram Desktop: the panel's row of tabs (emoji, saved GIFs, set search, favourite and recent stickers, then the installed sets), the set box from a sticker in a chat or a `t.me/addstickers/…` link with Add / Remove and its link (`getStickerSet`, `installStickerSet`, `uninstallStickerSet`), search and trending sets (`searchStickerSets`, `getFeaturedStickers`), the order of installed sets (`reorderStickerSets`), favourite stickers from a sticker's menu (`getFavedStickers`, `faveSticker`), saved GIFs as still previews: sending, saving from a chat, removing (`getSavedGifs`, `saveGif`)
- ✅ Cache management in Settings → Advanced → "Data and storage": what photos, videos and GIFs, voice and music, stickers, profile photos and stories take, clearing each type or all, a cache size limit (1, 5, 20 GB or none) kept by removing the oldest files on every connection
- ✅ Media auto-download (Settings → Advanced): photos, videos and audio separately for private chats, groups and channels, videos and GIFs up to 10 / 50 / 200 MB or any size; held-back media show a download arrow and load on a click. Not by network type (a desktop has no metered-network signal on every OS)
- ⬜ **P2** Music player with a queue and a playback bar; music in the profile (`saveMusic`, 2026)
- ⬜ **P2** Simple photo editor: crop, rotate, draw, text with fonts and styles; editing GIFs before sending; Live Photos (2026)
- ⬜ **P3** Creating your own sticker sets (`stickers.createStickerSet`), custom emoji made from video stickers

## 6. Groups and channels
- ✅ **P0** Chat info column (click the header): description/bio, @username, phone, invite link (`exportChatInvite`), member and admin list with search, click opens a private chat (`getFullUser` / `getFullChat` / `getFullChannel`, `channels.getParticipants`)
- ✅ **P0** Leave a group or channel, clear history, delete chat with confirmation and "also for the other side" (`leaveChannel`, `deleteChatUser`, `deleteHistory`, `channels.deleteHistory`); the owner gets "Delete group" / "Delete channel" instead of "Leave", for everyone (`channels.deleteChannel`, `messages.deleteChat`)
- ✅ Editing a group or channel (pencil in chat info, for admins who may change info): title, photo, description; default member permissions; slow mode in supergroups (`editTitle` / `editChatTitle`, `editChatAbout`, `editPhoto`, `editChatDefaultBannedRights`, `toggleSlowMode`)
- ✅ **P1** Administration ("Manage" box, from the "Edit" box or the info header): administrators with rights and a custom title, add / edit / dismiss (`channels.editAdmin`, `messages.editChatAdmin`); member menu → promote, restrict with an end date, remove; "Restricted" and "Removed users" lists with unban (`channels.getParticipants`, `channels.editBanned`); join requests with approve / dismiss and all at once (`getChatInviteImporters`, `hideChatJoinRequest`, `hideAllChatJoinRequests`); our invite links with usage, new / edit with name, expiry, limit and admin approval, revoke, delete revoked (`exportChatInvite`, `getExportedChatInvites`, `editExportedChatInvite`, `deleteExportedChatInvite`, `deleteRevokedExportedChatInvites`). Only what our admin rights allow is offered. Not yet: other admins' links, who joined by a link, paging past 200 people
- ✅ Reactions allowed in the chat (all, some from the standard set, none — `setChatAvailableReactions`); hiding the member list and aggressive anti-spam in supergroups (`toggleParticipantsHidden`, `toggleAntiSpam`), in the "Edit" box
- ✅ "Recent actions" admin log (`getAdminLog`, paged), linking and unlinking a channel's discussion group (`getGroupsForDiscussion`, `setDiscussionGroup`), author signatures in channels (`toggleSignatures`) — in the "Edit" box
- ⬜ **P2** Channel boosts (`premium.applyBoost`), transferring group ownership (2026)
- ⬜ **P3** Channel statistics (`stats.*`), channel auto-translation, paid messages, managing sponsored messages
- ⬜ **P3** AI guardians that screen new members, chat automation rules (2026)

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
- ⬜ **P2** HTML widgets in bot messages (Telegram Desktop 7.4, November 2026)
- ⬜ **P3** Games (`setGameScore`), business bots, verification by bots, referral programs

## 8. Stories
- ✅ Story strip above the chat list (ring while unseen) and a viewer: progress segments, auto-advance, click/arrows to move between stories and people, Space pauses, seen marks (`stories.getAllStories`, `getPeerStories`, `readStories`); video stories play with sound (downloaded via `getStoriesByID`, the progress follows their length), a private reply to people (`inputReplyToStory`; typing holds the story), a reaction with ❤ or the quick row on right click (`stories.sendReaction`)
- ⬜ **P2** Posting a story from a photo or video, caption, privacy (`stories.sendStory`)
- ⬜ **P2** Pinned stories and albums in the profile, archive, view statistics
- ⬜ **P3** Anonymous viewing (Premium), live streams in stories (`stories.startLive`)

## 9. Gifts, Stars, Premium
- ✅ Gifts in profiles: a "Gifts" tab with the gift stickers, price or collectible number, sender and note on hover (`getSavedStarGifts`)
- 🟡 **P2** Stars balance, transaction history, top-up via invoice (`getStarsStatus`, `getStarsTransactions`): Settings → My Stars shows the balance and the history with All / Incoming / Outgoing tabs, paged with "Show more"; each row has the source (bot, chat, Fragment, App Store…), title, date and the signed amount, a click opens the details (amount, source, date, status incl. refunds, description, ID with Copy, the blockchain link of a withdrawal). Left: top-up via invoice — after the payments security review
- ⬜ **P2** Sending gifts (`getStarGifts`, `checkCanSendGift`), paid ⭐ reactions and paid media
- ⬜ **P3** Unique gifts: upgrading, transferring, reselling, collections, auctions, crafting, signed marketplace gifts (2026); moving them to a TON Money card — with the wallet review
- ⬜ **P3** Giveaways, Premium gift codes, channel subscriptions for Stars
- ⬜ **P3** TON wallet and Fragment collectible usernames (`wallet.*`, `fragment.*`) — only after a separate security review

## 10. Calls
Needs WebRTC and Telegram's protocol: FFI to `tgcalls` (C++) or pure Rust (`webrtc-rs`) + Telegram signalling.
- ⬜ **P1** 1-on-1 voice calls: `phone.requestCall` / `acceptCall` / `confirmCall`, DH key exchange, call window, incoming call as a notification
- ⬜ **P2** Video calls and screen sharing
- ⬜ **P2** Group calls and live streams in channels (`phone.joinGroupCall`); end-to-end encrypted conference calls (`createConferenceCall`)
- ⬜ **P2** Call log, rating a finished call with stars in the call panel
- ⬜ **P3** RTMP streams, call recording

## 11. Privacy and security
- ✅ Privacy (phone number, last seen, photo, forwards, calls, voice messages, invites), local passcode
- ✅ Exceptions in privacy rules: "Always allow" / "Never allow" lists under each rule with people and groups, added through a search picker, removed with ✕; changing the preset keeps them (`inputPrivacyValueAllow/DisallowUsers`, `…ChatParticipants`)
- ✅ Global settings on the Privacy and Security page: archive and mute new non-contacts (keep unmuted / folder chats archived), hide read time, who can message me (`get/setGlobalPrivacySettings`; other server fields are kept)
- ✅ Blocking from user info; reporting messages in groups and channels from the message menu: the server's reasons, sub-reasons and a comment step (`messages.report`)
- ⬜ **P2** End-to-end encrypted secret chats (`requestEncryption`, MTProto 2.0 E2E)
- ✅ Message auto-delete: the default timer for new chats in Privacy and Security (`setDefaultHistoryTTL`) and a chat's own timer in its info panel (`setHistoryTTL`, read from the full chat's `ttl_period`): off, 1 day, 1 week, 1 month
- ⬜ **P2** Encrypting local data with the local passcode (for now the passcode only hides the interface)
- ✅ Sensitive content: "Disable filtering" in Privacy and Security (`account.getContentSettings` / `setContentSettings`), greyed out for accounts that can't change it
- ✅ Hardening against untrusted content (audit 2026-10): links open only https/http/mailto/tg; sender file names can't leave Downloads; received programs and scripts are shown in their folder, not run; mini apps can't open local paths, read the clipboard or navigate to `file:`; recordings live in an owner-only temp folder
- ✅ Confirmation for hidden links whose text differs from the target (`[text](url)`): the real address in a tooltip and in the "Open this link?" box, as in Telegram Desktop
- ✅ Warning before opening a received program or script: "Run this file?" with "Show in folder" as the safe default
- ⬜ **P2** Mini app permissions asked from the user: clipboard, location, contact, biometry, camera and microphone (Telegram Desktop 7.3.5) (`requestWriteAccess`, `requestContact`)

## 12. Notifications
- ✅ System notifications, counter in the title, tray with an unread dot
- ✅ **P0** Per-chat notifications in chat info: mute for 1 h / 8 h / 2 days / forever, unmute, silent, current state (`updateNotifySettings`, `getNotifySettings`)
- ✅ Notifications for chats in Settings → Notifications, as in Telegram Desktop: private chats, groups and channels on / off (`inputNotifyUsers` / `Chats` / `Broadcasts`), the exceptions of each with "Remove exception" (`getNotifyExceptions`); chats without their own setting follow their category, live `updateNotifySettings` from other devices. (MTProto has no per-chat "mentions only" mode.)
- 🟡 **P1** Reply and "mark as read" right from the notification (where the OS allows), grouping by chat: message notifications have "Reply" (opens the chat with the cursor in the composer) and "Mark as read" buttons, a click opens the chat and brings the window back from the tray (Linux over D-Bus, Windows toasts); on Linux a chat's next message replaces its notification and the title counts them. Under a local passcode the buttons are left out. Left: typing the reply inside the notification (no portable API: only macOS has one), buttons on macOS
- ✅ **P1** Notification sounds and custom ringtones: "Play sound" in Settings → Notifications, a built-in chime synthesised in code (no third-party audio), one ring for messages arriving together; the account's saved ringtones with listening, adding an MP3/OGG up to 300 KB (`account.uploadRingtone`) and deleting (`saveRingtone`), files cached; a chat's own sound (Default / No sound / a ringtone, `other_sound`) in its notification menu, silent chats stay quiet. Reaction notification settings shared with the account: whose reactions to messages and stories and poll votes notify, previews, sound (`account.setReactionsNotifySettings`)
- ✅ **P2** Unread badge on the taskbar icon (Windows, macOS, Unity): the same count as the window title (unmuted, not archived chats), up to 99, then "99+". Linux: the Unity LauncherEntry signal for our desktop file, shown by Ubuntu Dock, Dash to Dock and KDE Plasma (the Flatpak may talk to `com.canonical.Unity`); Windows: a red overlay icon on the taskbar button (`ITaskbarList3::SetOverlayIcon`, drawn with `vello_cpu`); macOS: the dock tile badge. Windows and macOS are compile-checked only until the real runs in §17

## 13. Telegram Business
- ⬜ **P3** Opening hours, address, greeting and away messages, quick replies, chat links, connected bots (`account.updateBusiness*`, `quickReplies`)

## 14. Settings (structure as in Telegram Desktop)
- ✅ Language, theme, scale, notifications, autoplay, camera, SOCKS5 proxy, privacy, devices
- ✅ Sections as in Telegram Desktop: a main page with the profile card and My Account · Notifications · Privacy and Security · Chat Settings · Folders · Advanced · Devices · Language, interface scale at the bottom; each section is a page with a back button (Esc goes back)
- ✅ Chat settings: message text size (small / standard / large / huge; poll, location and contact cards grow with it), sending with Enter or Ctrl+Enter (Cmd+Enter on macOS), "Suggest stickers by emoji" (installed stickers for an emoji typed alone, above the field), "Loop animated stickers"
- ✅ Localisation files: `locales/<code>.json` keyed by the English string, built-in or dropped into the data folder, picked in Settings → Language, English fallback, generated template; German built in
- 🟡 **P1** Interface in every language Telegram Desktop offers: Settings → Language lists the server's languages (`langpack.getLanguages`, lang pack `tdesktop`, incl. beta/community packs from translations.telegram.org) with native names, "beta" and the translated share; a click downloads the pack (`langpack.getLangPack`), caches it owner-only in `langpacks/<code>.json`, and switches live without a restart; a cached pack is refreshed with `langpack.getDifference` from its version (on login and when the page opens). `locales/tdesktop-keys.json` maps 343 of our English strings to tdesktop keys (exact meaning matches, placeholders renamed); lookup: built-in uk/en → `locales/*.json` → the Telegram pack → English. Plural forms: `lp!` for counted strings, CLDR cardinal rules for ~50 languages (`i18n/plural.rs`), `#zero`…`#other` keys from the pack kept and picked by the count, plural objects in `locales/*.json`. Left: mapping the rest of our strings (most have no exact tdesktop counterpart or carry `**markup**`), `updateLangPack` pushes
- 🟡 **P1** MTProto proxy (obfuscated transport), several proxies with quick switching: Settings → Advanced → Proxy keeps a list of SOCKS5 and MTProto proxies (plain and `dd` secrets, hex or base64) with each one's ping (`req_pq_multi` to DC 2 through it); a click switches to another or to none without a restart; a `t.me/proxy` / `tg://proxy` / `t.me/socks` link in a message asks and connects. grammers only speaks SOCKS5, so it connects through a local bridge (127.0.0.1) that forwards straight, through SOCKS5 (with login), or in MTProxy's obfuscated transport, repacking grammers' full-transport packets and naming the DC from the session. Left: fake-TLS proxies (`ee` secrets), the proxy-sponsored channel (`help.getPromoData`)
- ⬜ **P2** Full account data export (`account.initTakeoutSession`), importing history from other messengers (`initHistoryImport`)
- ✅ Exporting a single chat's history (HTML and JSON)

## 15. Interface and "liquid glass"
- ✅ Glass panels, buttons, fields, bubbles, avatars, live background, GPU blur, light and dark themes
- ✅ Vector icons (Lucide), icons in context menus as in Telegram Desktop, anti-aliasing for the screen density, built-in font
- ✅ Hotkeys as in Telegram Desktop: Ctrl+F (search in chat, or chat list), Ctrl+K (chat list search), Ctrl+1…8 folder by position and Ctrl+9 the last one, Alt+↑/↓, Ctrl+Tab / Ctrl+Shift+Tab and Ctrl+PgUp/PgDn (previous/next chat), Esc (close the topmost panel → clear search → close the chat), Ctrl+E or ↑ in an empty composer (edit the last own message)
- ✅ Glass context menus, submenus, drop-down lists and tooltips: at the end of each frame the flat egui popup frames are swapped for glass (`glass::glass_popups`)
- ✅ **P1** Blurring chat content under the header and composer: egui_wgpu paints the frame in one pass that can't sample itself, so the history shapes lying under any glass painted over the chat (header, pinned bar, composer, overlays) are tessellated again into a plain mesh (`blur/content.rs`: text as soft word pills, pictures as a colour grid from an 8×8 thumbnail), painted over a half-size copy of the background and blurred by a second small chain that only those panels sample; nothing runs when nothing is under a bar, and "Reduce transparency" skips it
- ✅ "Reduce transparency", "Increase contrast" and "Reduce motion" (Settings → Chat Settings → Accessibility): solid glass without blur, denser surfaces with clear rims and darker captions, a still background and no egui animations
- ✅ **P1** Animations: spring transitions, bubbles appearing, switching folders (150–350 ms): Apple-style springs (`app/motion.rs`, damped springs driven by frame time, snapping with "Reduce motion"). A message that arrives or is sent while its chat is open rises and grows from its tail corner with a fade (~310 ms; history loaded on open or scrolled back doesn't animate, and the server copy of a sent message doesn't replay it); the folder tab capsule (and the folder column's selection) glides to the chosen folder with a slight bounce while the list slides in from the side of travel and fades in (~340 ms); the info column and overlay and the narrow list ↔ chat switch run on a spring without overshoot, and going back in a narrow window slides the chat out instead of dropping it
- ✅ Wide mode with a third column (chat info) and a narrow mode for small windows (`app/layout.rs`): the info panel squeezes the chat while it keeps 420 pt and slides in/out, otherwise it overlays the chat; below list 250 + chat 560 pt one column at a time, the open chat pushing the list out, with a back arrow in the header (Esc too)
- ⬜ **P2** Accessibility: AccessKit (screen readers), full keyboard navigation
- ⬜ **P3** Right-to-left languages

## 16. Reliability and performance
- ✅ Local cache of dialogs and messages (SQLite, `chats.sqlite` next to the session, 0600): chats and history from the cache in ~10 ms before connecting, scrolling without a network; up to 1000 messages per chat
- ✅ Offline send queue for text messages, files, voice and round video messages: the bubble appears at once with a clock, network errors are retried with growing delays (2, 4, 8, 16 s), then a red mark with Retry / Delete in the menu (click retries); the queue lives in `chats.sqlite` and survives a restart (failed ones stay failed). Retries reuse the queue key as `random_id` (raw `messages.sendMessage`), so a lost reply can't post a message twice; the server's `RANDOM_ID_DUPLICATE` counts as sent. Files are copied into the account's `outbox/<key>/` folder first (recordings moved), uploaded with progress at every attempt and sent as our own requests whose `random_id`s come from the key; folders of messages no longer queued are pruned at start
- ✅ Memory budget for pictures (192 MB): the least recently drawn ones that were off screen for 30 s are dropped and decoded again from the disk cache when they come back into view (`app/textures.rs`, `Cmd::ReloadImages`)
- ✅ Backend tests on recorded server replies and CI: `backend/convert/tests.rs` feeds TL objects shaped like Telegram's replies (users, groups, supergroups, channels; entities with emoji and Cyrillic offsets, media kinds, forwards, replies, topics, service messages, reactions, buttons, restrictions) through grammers' update stream with a client that never connects; `.github/workflows/ci.yml` builds with `-D warnings` and runs the tests on Linux, Windows and macOS for every push and pull request, without the API key secrets
- ✅ Error log to a file (`logs/errors.log`, owner-only, rotated at 512 KB): errors shown to the user, background failures and panics, the home folder written as `~`; Settings → Advanced → "Error log" copies a report (version, system, last 60 lines), opens the folder, clears it
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
