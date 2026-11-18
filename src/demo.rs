//! Demo mode (`--demo`): made-up chats without network, to look at the interface.

use std::collections::HashMap;

use grammers_client::session::types::PeerId;

use crate::app::App;
use eframe::egui::{Color32, ColorImage};

use crate::backend::{AdminRights, ColorOption, InviteDraft, InviteLink, JoinRequest, PartList, Participant, SavedTag, PostsFlood, PasswordChange, PasswordState, ReportStep, Gift, Story, StoryPeer, StoryAlbum, StoryList, StoryViewer, STORY_PAGE, Birthday, ChatRights, GlobalPrivacy, WebAppKind, WebAppOpened, NotifyException, NotifyScope, Sponsored, SponsoredReport, MUTE_FOREVER, Profile, Queued, UsernameState, BotCmd, Mentionable, WebPageInfo, CardKind, ChatDetails, Member, MemberRole, NotifyState, Ringtone, ReactFrom, ReactionNotify, Sound, SharedKind, Topic, Button, ButtonAction, GeoInfo, InlineResult, MediaInfo, MediaKind, PollInfo, PollVoter, TodoInfo, TodoTask, QueuedMedia, ReplyTo, PollOption, Reaction, ChatInfo, ChatKind, Cmd, Ev, Folder, ImgKey, Msg, Presence, SearchHit};
use crate::format;
use crate::i18n::{l, lf};

mod stickers;
mod own_stickers;
mod stars;
mod custom_emoji;
mod emoji17;
mod effects;
mod music;
mod wallpapers;
mod langpack;
mod sendas;
mod aitext;
mod aicompose;
mod factcheck;
mod instant_view;
mod translate;
mod attach;
mod calls;
mod story_post;
mod boosts;
mod stats;
mod account_actions;
mod login_security;
mod takeout;
mod business;
mod messaging;
mod folders;
mod chat;
mod settings;
mod stories;
mod saved;
mod contacts;
mod manage;
mod giveaways;
mod media;
mod info;
mod search;
mod password;
pub use media::{images, sample_photo};
use info::*;
use manage::*;
use media::*;

pub fn data() -> (String, Vec<ChatInfo>, HashMap<PeerId, Vec<Msg>>) {
    let chats = [
        (1, "Olena Koval", ChatKind::User, 2, true),
        (2, "Rust Team 🦀", ChatKind::Group, 14, true),
        (3, "Saved Messages", ChatKind::Saved, 0, true),
        (4, "Tech News", ChatKind::Channel, 128, false),
        (5, "Andriy", ChatKind::User, 0, false),
        (6, "WeatherBot", ChatKind::Bot, 0, false),
        (7, "Design Studio", ChatKind::Group, 3, false),
        (8, "Maria Shevchenko", ChatKind::User, 0, false),
        (9, "Old Project", ChatKind::Group, 0, false),
    ];
    let scripts: [&[(bool, &str, &str)]; 9] = [
        &[
            (false, "Olena", "Hi! Have you seen the new liquid glass design?"),
            (true, "", "Yes! I'm building a Telegram client in Rust with exactly that look 😄"),
            (false, "Olena", "Wow, how do you do the blur and refraction?"),
            (true, "", "A GPU blur, refraction at the rounded edges and a specular rim. Plus a live background with drifting colours, so the glass feels \"liquid\"."),
            (false, "Olena", "Can you show me a screenshot?"),
            (true, "", "📷"),
        ],
        &[
            (false, "Taras", "Who has updated to the new compiler yet?"),
            (false, "Iryna", "Me, everything built on the first try"),
            (true, "", "egui + grammers, pushing tonight"),
            (false, "Taras", "cargo clippy -- -D warnings 👀"),
            (false, "Iryna", "**Reminder:** the release is on __Friday__, ~~Thursday~~ is off. Details [in the tracker](https://codeberg.org/Lonami/grammers), run it with `cargo run --release`. ||A surprise for the team 🎁||"),
        ],
        &[(true, "", "Ideas: dark theme, reactions, voice messages"), (false, "Tech News", "Rust 1.95: faster builds and new const fn powers")],
        &[
            (false, "Tech News", "📷 Photo\nThe new Wi-Fi standard promises half the latency."),
            (false, "Tech News", "Linux 7.3 is out: a new scheduler and faster file systems."),
            (false, "Tech News", "Europe agrees on one charging port for laptops."),
            (false, "Tech News", "Premium subscribers, boost the channel so we can post stories: [t.me/boost/tech_news_demo](https://t.me/boost/tech_news_demo)"),
        ],
        &[(false, "Andriy", "Call tomorrow at 10:00?"), (true, "", "Yes, deal 👍"), (false, "Andriy", "Here's [the quarterly report](https://example.org/q3-report), have a look before the call")],
        &[(true, "", "/weather Kyiv"), (false, "WeatherBot", "☀ Kyiv: +18°, clear, wind 3 m/s")],
        &[(false, "Sofia", "Dropped the mockups in Figma"), (false, "Sofia", "📄"), (false, "Maksym", "🐱"), (false, "Maksym", "Great, I'll take a look")],
        &[(false, "Maria", "Thanks for the help!"), (false, "Maria", "Here's the bot I told you about: [t.me/GlassHelperBot?start=from_maria](https://t.me/GlassHelperBot?start=from_maria)"), (false, "Maria", "It can also live in the 📎 menu: [t.me/GlassHelperBot?startattach](https://t.me/GlassHelperBot?startattach)")],
        &[(false, "Oleh", "Archived chat: nothing has happened here for ages")],
    ];

    let mut infos = Vec::new();
    let mut history = HashMap::new();
    for ((id, title, kind, unread, pinned), script) in chats.into_iter().zip(scripts) {
        let peer = PeerId::user_unchecked(id);
        let msgs: Vec<Msg> = script
            .iter()
            .enumerate()
            .map(|(i, (out, sender, text))| (i, out, sender, format::parse(text)))
            .map(|(i, out, sender, (text, ents))| Msg {
                id: i as i32 + 1,
                out: *out,
                sender: sender.to_string(),
                sender_color: demo_color(sender),
                spans: format::from_entities(&text, &ents, 0),
                comments: (id == 4 && !*out).then_some(3),
                // Pinned: each of these chats has something for the bar above history.
                pinned: matches!((id, i), (1, 3) | (2, 4) | (4, 0)),
                views: (id == 4).then_some(12_400),
                time: format!("{}:{:02}", 9 + i, (id * 7 + i as i64 * 13) % 60),
                day: l!("Сьогодні", "Today").into(),
                // In "Rust Team" Taras answers Iryna: the quote stripe takes her name colour.
                reply_to: match (id, i) {
                    (1, 3) => Some(3),
                    (2, 3) => Some(2),
                    _ => None,
                },
                photo: text == "📷",
                file: (text == "📄").then(|| ("Mockups_v3.fig".to_owned(), "14.2 MB".to_owned())),
                sticker: (text == "🐱").then(|| (900, "🐱".to_owned())),
                lottie: text == "🐱",
                text,
                ..Msg::placeholder(peer)
            })
            .map(|m| if m.file.is_some() || m.sticker.is_some() { Msg { text: String::new(), ..m } } else { m })
            .map(|m| if id == 3 && !m.out { Msg { forwarded: Some(m.sender.clone()), sender: String::new(), out: true, ..m } } else { m })
            .map(|m| if id == 6 && !m.out { Msg { buttons: weather_buttons(), ..m } } else { m })
            .map(|m| if m.photo { Msg { text: "This is how a photo looks in glass ✨".into(), ..m } } else { m })
            .collect();
        // The reply in the first chat quotes the last words of the message it answers.
        let quoted = msgs.iter().find(|m| m.id == 3).map(|m| {
            let words: Vec<&str> = m.text.split_whitespace().collect();
            words[words.len().saturating_sub(3)..].join(" ")
        });
        let msgs: Vec<Msg> = msgs.into_iter().map(|m| if id == 1 && m.reply_to == Some(3) { Msg { reply_quote: quoted.clone(), ..m } } else { m }).collect();
        let last = msgs.last().unwrap();
        infos.push(ChatInfo {
            id: peer,
            title: title.into(),
            kind,
            last_text: if last.out { lf!("Ви: {}", "You: {}", last.text) } else { last.text.replace('\n', " ") },
            last_time: last.time.clone(),
            last_ts: 100 - id,
            unread,
            pinned,
            contact: matches!(kind, ChatKind::User),
            muted: id == 4,
            archived: id == 9,
            unread_mark: false,
            // The open chat's draft has two slips for the spell checker to underline.
            draft: match id {
                1 => "Sure, I'll send a better screenshot tonihgt. Дякую за терпіня 🙂".into(),
                5 => "Don't forget the **slides**".into(),
                _ => String::new(),
            },
            presence: match id {
                1 => Presence::Online(i64::MAX),
                5 => Presence::Offline(jiff::Timestamp::now().as_second() - 3600),
                8 => Presence::Recently,
                _ => Presence::None,
            },
            forum: false,
            joined: true,
            can_post: kind != ChatKind::Channel,
            // Olena hasn't read our last messages yet: a single check mark is visible.
            read_out: if id == 1 { 4 } else { i32::MAX },
            // "Rust Team" was read up to the second message: it opens at the third, under "Unread messages".
            read_in: if id == 2 { 2 } else { 0 },
            // "Rust Team" has two mentions and a reaction for us — shows the "@" and "❤" buttons.
            unread_mentions: if id == 2 { 2 } else { 0 },
            unread_reactions: if id == 2 { 1 } else { 0 },
            // "Design Studio" has protected content: no forwarding or copying.
            noforwards: id == 7,
            can_pin: !matches!(kind, ChatKind::Channel) && id != 7,
            // The archived group is blocked by Telegram: its reason shows instead of the history.
            restricted: (id == 9).then(|| "This group is blocked for violating Telegram's Terms of Service.".to_owned()),
            // Olena wears an emoji status: a custom emoji after her name.
            emoji_status: (id == 1).then_some(custom_emoji::STATUS),
            // Maria charges non-contacts for messages.
            paid_stars: (id == 8).then_some(25),
        });
        let mut msgs = msgs;
        extras(id, peer, &mut msgs);
        calls::add(id, peer, &mut msgs);
        history.insert(peer, msgs);
    }
    let (forum, msgs) = forum_chat();
    history.insert(forum.id, msgs);
    infos.insert(3, forum);
    // A German group: the translate bar and whole-chat translation.
    let (german, msgs) = translate::chat();
    history.insert(german.id, msgs);
    infos.insert(4, german);
    // A business contact: address and opening hours in its profile.
    let (shop, msgs) = business::chat();
    history.insert(shop.id, msgs);
    infos.insert(5, shop);
    let bot = helper_bot();
    history.insert(bot.id, Vec::new());
    infos.push(bot);
    ("Demo User".into(), infos, history)
}

pub fn folders() -> Vec<Folder> {
    vec![
        // Pinned chats inside a folder can be dragged too.
        Folder {
            id: 1,
            title: "Personal".into(),
            emoticon: "👤".into(),
            contacts: true,
            non_contacts: true,
            include: vec![PeerId::user_unchecked(8), PeerId::user_unchecked(5)],
            pinned: vec![PeerId::user_unchecked(8), PeerId::user_unchecked(5)],
            color: Some(3),
            ..Default::default()
        },
        Folder { id: 2, title: "Groups".into(), emoticon: "👥".into(), groups: true, color: Some(5), ..Default::default() },
        Folder { id: 3, title: "Channels & bots".into(), emoticon: "📢".into(), broadcasts: true, bots: true, color: Some(1), ..Default::default() },
        Folder { id: 4, title: "Unread".into(), emoticon: "✅".into(), contacts: true, non_contacts: true, groups: true, broadcasts: true, bots: true, exclude_read: true, ..Default::default() },
    ]
}

/// A sent reply as the server echoes it: the quote, and the title of the other chat it replies in.
fn replied(app: &App, m: Msg, reply: &Option<ReplyTo>) -> Msg {
    let Some(r) = reply else { return m };
    let chat = |p: PeerId| app.demo_history().find(|(x, _, _)| *x == p).map(|(_, (title, _), _)| title.to_owned()).unwrap_or_default();
    Msg { reply_to: Some(r.id), reply_quote: r.quote.as_ref().map(|q| q.text.clone()), reply_elsewhere: r.peer.map(chat), ..m }
}

fn outgoing(peer: PeerId, markup: String, reply_to: Option<i32>) -> Msg {
    let (text, ents) = format::parse(&markup);
    Msg {
        spans: format::from_entities(&text, &ents, 0),
        id: 10_000 + rand_ish(),
        out: true,
        text,
        time: jiff::Zoned::now().strftime("%H:%M").to_string(),
        ts: jiff::Timestamp::now().as_second(),
        day: l!("Сьогодні", "Today").into(),
        reply_to,
        ..Msg::placeholder(peer)
    }
}

pub fn handle(app: &mut App, cmd: Cmd) {
    match cmd {
        cmd @ (Cmd::Send { .. } | Cmd::DeleteScheduled { .. } | Cmd::SendScheduledNow { .. } | Cmd::InlineQuery { .. } | Cmd::SendInline { .. } | Cmd::WebPreview { .. } | Cmd::LoadMentions { .. } | Cmd::LoadBotCommands { .. } | Cmd::Forward { .. } | Cmd::PlayGame { .. } | Cmd::PressButton { .. } | Cmd::SendDice { .. } | Cmd::SendQueued { .. } | Cmd::SendPoll { .. } | Cmd::PollVotes { .. } | Cmd::AddPollAnswer { .. } | Cmd::SendTodo { .. } | Cmd::ToggleTodo { .. } | Cmd::AppendTodo { .. } | Cmd::EditMedia { .. }) => messaging::handle(app, cmd),
        cmd @ (Cmd::LoadStickers
        | Cmd::OpenStickerSet(_)
        | Cmd::InstallStickerSet { .. }
        | Cmd::SearchStickerSets(_)
        | Cmd::ReorderStickerSets(_)
        | Cmd::FaveSticker { .. }
        | Cmd::LoadSavedGifs
        | Cmd::SaveGif { .. }
        | Cmd::LoadStickerThumbs(_)
        | Cmd::SendSticker { .. }) => stickers::handle(app, cmd),
        cmd @ Cmd::OwnStickers(_) => own_stickers::handle(app, cmd),
        cmd @ (Cmd::LoadWallPapers | Cmd::LoadChatThemes | Cmd::LoadChatWallPaper(_) | Cmd::DownloadWallPaper(_) | Cmd::SetChatWallPaper { .. } | Cmd::SetChatTheme { .. }) => wallpapers::handle(app, cmd),
        cmd @ (Cmd::SaveFolder { .. } | Cmd::SetFolderTags { .. } | Cmd::LoadSuggestedFolders { .. } | Cmd::DeleteFolder { .. } | Cmd::ReorderFolders { .. } | Cmd::CheckChatlist { .. } | Cmd::CheckChatlistUpdates { .. } | Cmd::AskLeaveChatlist { .. } | Cmd::ApplyChatlist { .. }) => folders::handle(app, cmd),
        cmd @ (Cmd::OpenComments { .. } | Cmd::LoadTopics { .. } | Cmd::CreateTopic { .. } | Cmd::EditTopic { .. } | Cmd::OpenTopic { .. } | Cmd::ExportChat { .. } | Cmd::Download { .. } | Cmd::SetAutoDownload { .. } | Cmd::LoadPhoto { .. } | Cmd::StreamMedia { .. } | Cmd::LoadMedia { .. } | Cmd::Delete { .. } | Cmd::LoadOlder { .. } | Cmd::Logout { .. } | Cmd::Search { .. } | Cmd::LoadPinned { .. } | Cmd::PinMessage { .. } | Cmd::UnpinAll { .. } | Cmd::LoadUnreadMarks { .. } | Cmd::LoadSponsored { .. } | Cmd::ViewSponsored { .. } | Cmd::ClickSponsored { .. } | Cmd::BotWriteAccess { .. } | Cmd::SharePhone { .. } | Cmd::SetAdsEnabled { .. } | Cmd::ReportSponsored { .. } | Cmd::Translate { .. } | Cmd::JumpToDate { .. } | Cmd::LoadReadBy { .. } | Cmd::LoadChatInfo { .. } | Cmd::LoadMembers { .. } | Cmd::ReloadImages { .. } | Cmd::LoadMediaDays { .. } | Cmd::Report { .. } | Cmd::LoadSimilarChannels { .. } | Cmd::ExportInvite { .. } | Cmd::LoadShared { .. } | Cmd::SetNotify { .. } | Cmd::LeaveChat { .. } | Cmd::DeleteChat { .. } | Cmd::ClearHistory { .. } | Cmd::OpenChat { .. } | Cmd::Typing { .. }) => chat::handle(app, cmd),
        cmd @ (Cmd::LoadSessions { .. } | Cmd::TerminateSession { .. } | Cmd::TerminateOtherSessions { .. } | Cmd::LoadPrivacy { .. } | Cmd::SetPrivacy { .. } | Cmd::LoadNotifyExceptions { .. } | Cmd::SetNotifyDefault { .. } | Cmd::ResetNotify { .. } | Cmd::SetProxy { .. } | Cmd::CheckProxy { .. } | Cmd::LoadRingtones { .. } | Cmd::UploadRingtone { .. } | Cmd::DeleteRingtone { .. } | Cmd::LoadReactionNotify { .. } | Cmd::SetReactionNotify { .. } | Cmd::LoadProfile { .. } | Cmd::SaveProfile { .. } | Cmd::CheckUsername { .. } | Cmd::SetUsername { .. } | Cmd::SetProfilePhoto { .. } | Cmd::LoadGlobalPrivacy { .. } | Cmd::SetGlobalPrivacy { .. } | Cmd::LoadCacheUsage { .. } | Cmd::ClearCache { .. } | Cmd::TrimCache { .. } | Cmd::LoadCountries { .. } | Cmd::SendChangePhoneCode { .. } | Cmd::ChangePhone { .. } | Cmd::DeleteAccount { .. } | Cmd::LoadAccountTtl { .. } | Cmd::SetAccountTtl { .. } | Cmd::LoadDefaultTtl { .. } | Cmd::SetDefaultTtl { .. } | Cmd::SetHistoryTtl { .. } | Cmd::LoadWebsites { .. } | Cmd::LogoutWebsite { .. } | Cmd::LogoutAllWebsites { .. } | Cmd::LoadContentSettings { .. } | Cmd::SetSensitive { .. } | Cmd::SetBirthday { .. } | Cmd::ToggleUsername { .. } | Cmd::ReorderUsernames { .. }) => settings::handle(app, cmd),
        Cmd::Transcribe { peer, id } => aitext::transcribe(app, peer, id),
        Cmd::Summarize { peer, id } => aitext::summarize(app, peer, id),
        Cmd::AiCompose { draft, mode } => aicompose::compose(app, draft, mode),
        Cmd::LoadAiTones => aicompose::tones(app),
        Cmd::GiveawayInfo { peer, id } => giveaways::info(app, peer, id),
        Cmd::LoadFactChecks { peer, ids } => factcheck::answer(app, peer, ids),
        Cmd::InstantView { url } => instant_view::open(app, url),
        cmd @ (Cmd::LoadStories { .. } | Cmd::LoadStoryTargets { .. } | Cmd::CanPostStory { .. } | Cmd::PostStory { .. } | Cmd::LoadPeerStories { .. } | Cmd::LoadStoryVideo { .. } | Cmd::ReadStories { .. } | Cmd::ReactStory { .. } | Cmd::StoryStealth { .. } | Cmd::LoadProfileStories { .. } | Cmd::LoadStoryAlbums { .. } | Cmd::LoadStoryViewers { .. } | Cmd::ReplyStory { .. }) => stories::handle(app, cmd),
        cmd @ (Cmd::LoadSavedDialogs { .. } | Cmd::LoadSavedTags { .. } | Cmd::RenameSavedTag { .. } | Cmd::LoadSavedTagged { .. } | Cmd::LoadSavedHistory { .. }) => saved::handle(app, cmd),
        Cmd::LoadCustomEmoji(ids) => custom_emoji::load(app, ids),
        Cmd::LoadEffects => effects::load(app),
        Cmd::LoadPeerColors => app.push_event(Ev::PeerColors { names: peer_colors(), profiles: Vec::new() }),
        Cmd::LoadEmojiStatuses => app.push_event(Ev::EmojiStatuses(custom_emoji::statuses())),
        Cmd::SetNameColor { profile, color, emoji } => {
            let p = app.demo_profile();
            let p = if profile { Profile { profile_color: (color, emoji), ..p } } else { Profile { color: (color, emoji), ..p } };
            app.push_event(Ev::Profile(p));
            app.push_event(Ev::Notice(l!("Колір збережено", "Color saved").into()));
        }
        Cmd::SetEmojiStatus(emoji_status) => app.push_event(Ev::Profile(Profile { emoji_status, ..app.demo_profile() })),
        Cmd::LoadEmojiKeywords(lang) => {
            let w = |k: &str, e: &[&str]| (k.to_owned(), e.iter().map(|x| x.to_string()).collect());
            let words = if lang == "uk" {
                vec![w("серце", &["❤", "💖", "💕"]), w("сміх", &["😂", "🤣", "😆"]), w("сонце", &["☀", "🌞"]), w("кіт", &["🐱", "😺"]), w("ракета", &["🚀"])]
            } else {
                vec![w("heart", &["❤", "💖", "💕"]), w("laugh", &["😂", "🤣"]), w("sun", &["☀", "🌞"]), w("cat", &["🐱", "😺"]), w("rocket", &["🚀"])]
            };
            app.push_event(Ev::EmojiKeywords { words });
        }
        Cmd::LoadGifts(peer) => {
            let gift = |title: &str, stars: i64, from: &str, note: &str| Gift { title: title.into(), stars, sticker: None, from: from.into(), note: note.into(), date: 0 };
            let gifts = vec![
                gift("", 50, "Andriy", "Happy birthday! 🎂"),
                gift("Plush Pepe #1842", 0, "", ""),
                gift("", 25, "Maria Shevchenko", ""),
                gift("", 100, "Rust Team", "Thanks for the release"),
                gift("", 15, "", ""),
            ];
            app.push_event(Ev::Gifts { peer, gifts, total: 5 });
        }
        cmd @ (Cmd::SearchChannels(_) | Cmd::SearchApps(_) | Cmd::CheckPostsFlood(_) | Cmd::SearchPosts(_)) => search::handle(app, cmd),
        cmd @ (Cmd::LoadSendAs(_) | Cmd::SaveSendAs { .. }) => sendas::handle(app, cmd),
        cmd @ (Cmd::LoadMusic(_) | Cmd::LoadSavedMusic(_) | Cmd::LoadSavedTrack { .. }) => music::handle(app, cmd),
        cmd @ (Cmd::LoadTranslationOffer(_) | Cmd::TranslateMany { .. } | Cmd::TogglePeerTranslations { .. }) => translate::handle(app, cmd),
        cmd @ (Cmd::LoadAttachBots { .. } | Cmd::OpenAttachLink { .. } | Cmd::ToggleAttachBot { .. } | Cmd::LoadUrlAuthDomains | Cmd::RequestUrlAuth { .. } | Cmd::AcceptUrlAuth { .. } | Cmd::DeclineUrlAuth { .. }) => attach::handle(app, cmd),
        Cmd::ReorderPinned { .. } => {}
        Cmd::OpenWebApp { peer, bot, kind, .. } => {
            let url = match &kind {
                WebAppKind::Inline(u) | WebAppKind::Simple(u) | WebAppKind::Menu(u) => u.clone(),
                WebAppKind::Main { .. } | WebAppKind::Named { .. } => "https://example.org/app".into(),
                WebAppKind::Attach { .. } => "https://example.org/attach".into(),
            };
            app.push_event(Ev::WebAppOpened(WebAppOpened { peer, bot, kind, url, query_id: None }));
        }
        Cmd::OpenBotStart { param, .. } => {
            app.push_event(Ev::ChatFound(helper_bot()));
            app.push_event(Ev::BotStart { peer: PeerId::user_unchecked(HELPER_BOT), param });
        }
        Cmd::StartBot { peer, param } => {
            app.push_event(Ev::NewMsg(outgoing(peer, "/start".into(), None)));
            let answer = format!("Hi! You came from a link with the parameter \"{param}\".");
            app.push_event(Ev::NewMsg(Msg { out: false, sender: "GlassHelperBot".into(), id: 20_000 + rand_ish(), ..outgoing(peer, answer, None) }));
        }
        Cmd::OpenWebAppLink { username, .. } => app.push_event(Ev::Notice(format!("t.me/{username}"))),
        Cmd::ProlongWebApp { .. } | Cmd::WebAppData { .. } | Cmd::WebAppMethod { .. } => {}
        cmd @ (Cmd::LoadAdminLog { .. } | Cmd::SetSignatures { .. } | Cmd::SetDiscussionGroup { .. } | Cmd::LoadDiscussionGroups { .. } | Cmd::EditChat { .. } | Cmd::SetChatPhoto { .. } | Cmd::SetChatRights { .. } | Cmd::SetSlowMode { .. } | Cmd::SetChatReactions { .. } | Cmd::SetMembersHidden { .. } | Cmd::SetAntiSpam { .. }) => manage::handle(app, cmd),
        cmd @ Cmd::LoadStars { .. } => stars::handle(app, cmd),
        Cmd::Business(cmd) => business::handle(app, cmd),
        cmd @ (Cmd::LoadBoosts(_) | Cmd::ApplyBoost { .. } | Cmd::LoadBoosters { .. } | Cmd::OpenBoostLink(_) | Cmd::TransferOwnership { .. }) => boosts::handle(app, cmd),
        cmd @ (Cmd::LoadStats { .. } | Cmd::LoadStatsGraph { .. } | Cmd::LoadMessageStats { .. }) => stats::handle(app, cmd),
        cmd @ (Cmd::LoadLanguages | Cmd::LoadLangPack { .. }) => langpack::handle(app, cmd),
        cmd @ (Cmd::LoadLoginSecurity | Cmd::ChangeLoginEmail(_) | Cmd::VerifyLoginEmail(_) | Cmd::DeletePasskey(_)) => login_security::handle(app, cmd),
        cmd @ (Cmd::LoadCalls { .. } | Cmd::DeleteCalls { .. }) => calls::handle(app, cmd),
        cmd @ (Cmd::StartTakeout(_) | Cmd::CancelTakeout) => takeout::handle(app, cmd),
        cmd @ (Cmd::LoadParticipants { .. } | Cmd::EditAdmin { .. } | Cmd::RestrictMember { .. } | Cmd::BanMember { .. } | Cmd::UnbanMember { .. }) => manage::participants(app, cmd),
        cmd @ (Cmd::LoadInviteLinks(_) | Cmd::SaveInviteLink { .. } | Cmd::RevokeInviteLink { .. } | Cmd::DeleteInviteLink { .. } | Cmd::DeleteRevokedInvites(_) | Cmd::LoadJoinRequests(_) | Cmd::AnswerJoinRequest { .. }) => manage::invites(app, cmd),
        cmd @ (Cmd::LoadPassword | Cmd::CheckPassword(_) | Cmd::UpdatePassword { .. } | Cmd::ConfirmPasswordEmail(_) | Cmd::ResendPasswordEmail | Cmd::CancelPasswordEmail) => password::handle(app, cmd),
        cmd @ (Cmd::LoadBirthdays { .. } | Cmd::LoadContacts { .. } | Cmd::AddContact { .. } | Cmd::DeleteContact { .. } | Cmd::Block { .. } | Cmd::LoadBlocked { .. } | Cmd::SearchContacts { .. } | Cmd::CreateGroup { .. } | Cmd::CreateChannel { .. } | Cmd::CheckChannelUsername { .. } | Cmd::SetChannelUsername { .. } | Cmd::InviteToChannel { .. }) => contacts::handle(app, cmd),
        _ => {}
    }
}

thread_local! {
    /// Demo auto-delete timers: chats changed in this run, and the default for new chats.
    static DEMO_TTL: std::cell::RefCell<HashMap<PeerId, i32>> = std::cell::RefCell::new(HashMap::new());
    static DEMO_DEFAULT_TTL: std::cell::Cell<i32> = const { std::cell::Cell::new(0) };
    static DEMO_ACCOUNT_TTL: std::cell::Cell<i32> = const { std::cell::Cell::new(180) };
    /// Demo cache types the user cleared (the real cache is never touched in the demo).
    static DEMO_CLEARED: std::cell::RefCell<Vec<crate::backend::CacheKind>> = const { std::cell::RefCell::new(Vec::new()) };
    /// Demo websites the user disconnected.
    static DEMO_WEB_GONE: std::cell::RefCell<Vec<i64>> = const { std::cell::RefCell::new(Vec::new()) };
    /// Demo reaction tags of Saved Messages; renaming changes them.
    static SAVED_TAGS: std::cell::RefCell<Vec<SavedTag>> = std::cell::RefCell::new(vec![
        SavedTag { emoji: "👍".into(), title: "Ideas".into(), count: 2 },
        SavedTag { emoji: "❤".into(), title: String::new(), count: 1 },
    ]);
    /// Demo cloud password: "demo", with a hint and no recovery email at first.
    static PASSWORD: std::cell::RefCell<(String, PasswordState)> = std::cell::RefCell::new((
        "demo".into(),
        PasswordState { has_password: true, hint: "demo".into(), has_recovery: false, email_pending: None },
    ));
    /// Demo administration, the same for every chat we run.
    static MANAGE: std::cell::RefCell<Option<DemoManage>> = const { std::cell::RefCell::new(None) };
    /// Demo blocked list: starts with one spammer, follows Block / Unblock.
    static BLOCKED: std::cell::RefCell<Vec<ChatInfo>> = std::cell::RefCell::new(vec![person(50, "Spam Blast", "", Presence::None)]);
}

/// People found only by global search in the demo: (id, name, username).
const GLOBAL_PEOPLE: [(i64, &str, &str); 3] = [(30, "Oleksandr Melnyk", "melnyk"), (31, "Olena Petrenko", "olena_p"), (32, "Rust Weekly", "rustweekly")];

fn person(id: i64, name: &str, status: &str, presence: Presence) -> ChatInfo {
    ChatInfo {
        id: PeerId::user_unchecked(id),
        title: name.into(),
        kind: ChatKind::User,
        last_text: status.into(),
        last_time: String::new(),
        last_ts: 0,
        unread: 0,
        pinned: false,
        contact: true,
        muted: false,
        archived: false,
        unread_mark: false,
        draft: String::new(),
        presence,
        forum: false,
        joined: true,
        can_post: true,
        read_out: 0,
        read_in: 0,
        unread_mentions: 0,
        unread_reactions: 0,
        noforwards: false,
        can_pin: false,
        restricted: None,
        emoji_status: None,
        paid_stars: None,
    }
}

/// Demo contacts: people from the chat list plus a few without any chat yet.
fn demo_contacts() -> Vec<ChatInfo> {
    let now = jiff::Timestamp::now().as_second();
    let mut list = vec![
        person(1, "Olena Koval", "@olena", Presence::Online(i64::MAX)),
        person(5, "Andriy", "@andriy", Presence::Offline(now - 3600)),
        person(8, "Maria Shevchenko", "", Presence::Recently),
        person(20, "Ihor Bondar", "@bondar", Presence::LastWeek),
        person(21, "Nataliia Lysenko", "", Presence::Offline(now - 86_400 * 3)),
        person(22, "Tetiana", "@tanya", Presence::Online(i64::MAX)),
    ];
    list.sort_by_key(|c| c.title.to_lowercase());
    list
}

/// A freshly "created" group or channel with a service-like first message.
fn created(app: &mut App, title: &str, kind: ChatKind, text: String) {
    let peer = PeerId::user_unchecked(2000 + rand_ish() as i64);
    let msg = outgoing(peer, text, None);
    let mut info = person(0, title, &msg.text, Presence::None);
    info.id = peer;
    info.kind = kind;
    info.contact = false;
    info.last_time = msg.time.clone();
    info.last_ts = msg.ts;
    app.push_event(Ev::History { peer, msgs: vec![msg], older: false });
    app.push_event(Ev::ChatCreated(info));
}

const FORUM: i64 = 10;
/// Our message in "Rust Team" with an unread reaction.
const DEMO_REACTED: i32 = 3;
/// The channel that shows a sponsored message.
const DEMO_CHANNEL: i64 = 4;
const DEMO_AD_PHOTO: i64 = 77;
const DEMO_AD_MEDIA: i64 = 78;

fn demo_ad() -> Sponsored {
    let text = "A weekly digest of new crates, releases and articles about **Rust**. No fluff.";
    let (text, entities) = format::parse(text);
    Sponsored {
        random_id: vec![7; 16],
        title: "Rust Weekly".into(),
        spans: format::from_entities(&text, &entities, 0),
        text,
        url: "https://t.me/rustweekly".into(),
        button: "View channel".into(),
        recommended: false,
        can_report: true,
        sponsor_info: "Demo Media LLC, reg. no. 00000000".into(),
        additional_info: "This ad was placed via Telegram Ads.".into(),
        photo: Some(DEMO_AD_PHOTO),
        media: None,
        video: false,
    }
}

/// The second ad, a few posts higher: a media ad with a wide picture.
fn demo_media_ad() -> Sponsored {
    Sponsored {
        random_id: vec![8; 16],
        title: "Glass Wallpapers".into(),
        text: "A pack of \"liquid glass\" desktop wallpapers.".into(),
        spans: Vec::new(),
        url: "https://example.org/wallpapers".into(),
        button: "Open".into(),
        photo: None,
        media: Some(DEMO_AD_MEDIA),
        ..demo_ad()
    }
}
/// Group members for @mentions and the search sender filter: (id, name, username, bot).
const MENTIONABLE: [(i64, &str, &str, bool); 5] =
    [(31, "Taras Boyko", "taras_b", false), (32, "Iryna Tkachenko", "iryna", false), (33, "Sofia", "", false), (34, "Maksym Lysenko", "max_l", false), (35, "RustDocsBot", "rustdocsbot", true)];

/// Bot chat with no messages yet: shows the "Start" bar.
const HELPER_BOT: i64 = 11;

fn helper_bot() -> ChatInfo {
    ChatInfo {
        id: PeerId::user_unchecked(HELPER_BOT),
        title: "GlassHelperBot".into(),
        kind: ChatKind::Bot,
        last_text: String::new(),
        last_time: String::new(),
        last_ts: 0,
        unread: 0,
        pinned: false,
        contact: false,
        muted: false,
        archived: false,
        unread_mark: false,
        draft: String::new(),
        presence: Presence::None,
        forum: false,
        joined: true,
        can_post: true,
        read_out: i32::MAX,
        read_in: 0,
        unread_mentions: 0,
        unread_reactions: 0,
        noforwards: false,
        can_pin: false,
        restricted: None,
        emoji_status: None,
        paid_stars: None,
    }
}
/// Name colours of demo people: built-in ones and the server's two-tone 9.
fn demo_color(sender: &str) -> Option<i32> {
    match sender {
        "" => None,
        "Taras" => Some(0),
        "Iryna" => Some(3),
        "Sofia" => Some(9),
        "Maksym" => Some(1),
        "Olena" => Some(5),
        "Oleh" => Some(4),
        _ => Some(2),
    }
}

/// Extra name colours as `help.getPeerColors` sends them: two tones for each theme.
fn peer_colors() -> Vec<ColorOption> {
    [
        (7, [0xe15052, 0xf9ae63], [0xff9380, 0x992f37]),
        (8, [0xe0802b, 0xfac534], [0xecb04e, 0xc35714]),
        (9, [0xa05ff3, 0xf48fff], [0xc697ff, 0x5e31c8]),
        (10, [0x27a910, 0xa7dc57], [0xa7eb6e, 0x167e2d]),
        (11, [0x27acce, 0x82e8d6], [0x40d8d0, 0x045c7f]),
        (12, [0x3391d4, 0x7dd3f0], [0x52bfff, 0x0b5494]),
        (13, [0xdd4371, 0xffbe9f], [0xff86a6, 0x8e366e]),
    ]
    .into_iter()
    .map(|(id, light, dark)| ColorOption { id, light: light.to_vec(), dark: dark.to_vec() })
    .collect()
}

/// The demo user's own id (for the avatar in settings).
const ME: i64 = 1000;

fn demo_profile() -> Profile {
    Profile {
        id: Some(PeerId::user_unchecked(ME)),
        first: "Demo".into(),
        last: "User".into(),
        bio: "Writing a Telegram client in Rust 🦀".into(),
        username: "glassgram_demo".into(),
        phone: "+380 00 000 00 00".into(),
        birthday: Some(Birthday { day: 12, month: 5, year: None }),
        // Collectible usernames bought on Fragment: one shown in the profile, one hidden.
        usernames: vec![("glass".into(), true), ("liquidgram".into(), false)],
        // Premium: "About this ad" offers to hide ads.
        premium: true,
        ads_off: false,
        color: (Some(5), None),
        profile_color: (Some(4), None),
        emoji_status: Some(9003),
    }
}

/// Send queue "from the previous run": one message that failed, to show the red mark and its menu.
pub fn outbox() -> Vec<Queued> {
    let ts = jiff::Timestamp::now().as_second() - 120;
    vec![Queued { key: 1, peer: PeerId::user_unchecked(5), text: "Sent the report, have a look when you're online".into(), reply_to: None, silent: false, ts, failed: true, no_webpage: false, quote: None, reply_peer: None, media: None, effect: None, send_as: None }]
}

/// Demo forum topics: (id, title, color, unread, pinned, closed).
const TOPICS: [(i32, &str, u32, i32, bool, bool); 4] = [
    (1, "General", 0, 0, false, false),
    (100, "Help for newcomers", 0x6FB9F0, 3, true, false),
    (200, "Release 1.0", 0xFFD67E, 1, false, false),
    (300, "Discussion archive", 0xCB86DB, 0, false, true),
];

/// Forum group "Rust Ukraine" with several topics.
fn forum_chat() -> (ChatInfo, Vec<Msg>) {
    let peer = PeerId::channel_unchecked(FORUM);
    let script: [(i32, i32, &str, &str); 7] = [
        (1, 1, "Moderator", "Welcome to the forum! Questions go to \"Help for newcomers\"."),
        (100, 101, "Ostap", "How do I get rid of `borrow of moved value`?"),
        (100, 102, "Lesia", "Clone it or pass a reference: `&value`."),
        (100, 103, "", "Also worth reading the chapter on **ownership** in the book."),
        (200, 201, "Taras", "Building release 1.0 on Friday 🦀"),
        (200, 202, "Iryna", "The changelog is ready, release notes are left."),
        (300, 301, "Moderator", "Topic closed."),
    ];
    let msgs = script
        .iter()
        .map(|&(topic, id, sender, text)| {
            let (text, ents) = format::parse(text);
            Msg {
                id,
                sender: sender.into(),
                out: sender.is_empty(),
                spans: format::from_entities(&text, &ents, 0),
                text,
                time: format!("1{}:{:02}", id % 10, id % 60),
                day: l!("Сьогодні", "Today").into(),
                topic: (topic != 1).then_some(topic),
                ..Msg::placeholder(peer)
            }
        })
        .collect();
    let chat = ChatInfo {
        id: peer,
        title: "Rust Ukraine".into(),
        kind: ChatKind::Group,
        last_text: "Iryna: The changelog is ready, release notes are left.".into(),
        last_time: "12:02".into(),
        last_ts: 97,
        unread: 4,
        pinned: false,
        contact: false,
        muted: false,
        archived: false,
        unread_mark: false,
        draft: String::new(),
        presence: Presence::None,
        forum: true,
        joined: true,
        can_post: true,
        read_out: i32::MAX,
        read_in: 0,
        unread_mentions: 0,
        unread_reactions: 0,
        noforwards: false,
        can_pin: true,
        restricted: None,
        emoji_status: None,
        paid_stars: None,
    };
    (chat, msgs)
}

fn demo_topics() -> Vec<Topic> {
    TOPICS
        .iter()
        .map(|&(id, title, color, unread, pinned, closed)| {
            let last = match id {
                1 => "Moderator: Welcome to the forum!",
                100 => "You: Also worth reading the chapter on ownership in the book.",
                200 => "Iryna: The changelog is ready, release notes are left.",
                _ => "Moderator: Topic closed.",
            };
            Topic { id, title: title.into(), color, unread, pinned, closed, last_text: last.into(), last_time: "12:02".into(), last_ts: 0 }
        })
        .collect()
}

/// Extra demo messages: poll, place, contact, album, reactions.
fn extras(id: i64, peer: PeerId, msgs: &mut Vec<Msg>) {
    let base = |n: i32, sender: &str| Msg { sender: sender.into(), sender_color: demo_color(sender), out: sender.is_empty(), time: format!("12:{n:02}"), ..Msg::placeholder(peer) };
    match id {
        2 => {
            // A link with a preview card (its picture comes from `images()`).
            if let Some(m) = msgs.iter_mut().find(|m| m.id == 5) {
                m.webpage = Some(WebPageInfo {
                    id: 1,
                    url: "https://codeberg.org/Lonami/grammers".into(),
                    site: "Codeberg.org".into(),
                    title: "Lonami/grammers".into(),
                    description: "(tele)gramme.rs — a set of Rust crates for the Telegram MTProto API".into(),
                    photo: true,
                    iv: false,
                    card: CardKind::Link,
                });
            }
            if let Some(m) = msgs.first_mut() {
                m.reactions = vec![Reaction { emoji: "👍".into(), count: 3, mine: false }, Reaction { emoji: "🔥".into(), count: 1, mine: true }];
            }
            // Unread reaction to our message.
            if let Some(m) = msgs.iter_mut().find(|m| m.id == DEMO_REACTED) {
                m.reactions = vec![Reaction { emoji: "❤".into(), count: 1, mine: false }];
            }
            let opt = |t: &str, v: i32, o: u8| PollOption { text: t.into(), option: vec![o], voters: v, chosen: false, correct: false };
            msgs.push(Msg {
                id: 50,
                poll: Some(PollInfo {
                    question: "When do we release?".into(),
                    answers: vec![opt("On Friday", 4, 0), opt("Next week", 2, 1), opt("When it's ready 🦀", 5, 2)],
                    total: 11,
                    public: true,
                    open_answers: true,
                    close_date: Some(jiff::Timestamp::now().as_second() + 5 * 3600),
                    ..PollInfo::default()
                }),
                pinned: true,
                ..base(10, "Taras")
            });
            // A shared checklist, partly done.
            let task = |id: i32, title: &str, done: Option<(&str, i64)>| TodoTask { id, title: title.into(), done: done.map(|(w, t)| (w.to_owned(), t)) };
            let ts = jiff::Timestamp::now().as_second();
            msgs.push(Msg {
                id: 52,
                todo: Some(TodoInfo {
                    title: "Before the release".into(),
                    items: vec![
                        task(1, "Update the README", Some(("Iryna", ts - 7200))),
                        task(2, "Run the tests on Windows", Some(("Taras", ts - 3600))),
                        task(3, "Build the Flatpak", None),
                        task(4, "Write the release notes", None),
                    ],
                    others_can_append: true,
                    others_can_complete: true,
                }),
                ..base(11, "Iryna")
            });
            msgs.push(Msg {
                id: 51,
                geo: Some(GeoInfo { lat: 50.4501, lon: 30.5234, title: "Office".into(), address: "1 Khreshchatyk St, Kyiv".into(), live: false }),
                text: "Meeting here at 10:00".into(),
                ..base(12, "Iryna")
            });
        }
        1 => {
            msgs.push(Msg { id: 60, contact: Some(("Andriy Melnyk".into(), "+380 67 123 45 67".into())), ..base(20, "Olena") });
            for (k, t) in ["Sunset from the balcony 🌅", "", ""].iter().enumerate() {
                msgs.push(Msg { id: 70 + k as i32, photo: true, group: Some(7), text: t.to_string(), ..base(21, "") });
            }
            let media = |kind, duration, title: &str, performer: &str| MediaInfo {
                kind,
                duration,
                size: (0, 0),
                title: title.into(),
                performer: performer.into(),
                waveform: (0..63u32).map(|i| ((i * 7 % 13) as f32 * 1.6 + (i as f32 * 0.4).sin().abs() * 12.0) as u8).collect(),
                bytes: 0,
            };
            msgs.push(Msg { id: 80, media: Some(media(MediaKind::Voice, 4.0, "", "")), ..base(22, "Olena") });
            msgs.push(Msg { id: 81, media: Some(media(MediaKind::Voice, 3.0, "", "")), media_unread: true, ..base(22, "Olena") });
            msgs.push(Msg { id: 82, media: Some(media(MediaKind::Audio, 5.0, "Glass Morning", "Glassgram")), ..base(23, "") });
            msgs.extend(music::messages(base(23, "Olena")));
            msgs.push(Msg {
                id: 83,
                media: Some(MediaInfo { size: (640, 360), ..media(MediaKind::Video, 6.0, "", "") }),
                text: "Video from the concert".into(),
                ..base(24, "Olena")
            });
            msgs.push(Msg { id: 84, media: Some(MediaInfo { size: (480, 270), ..media(MediaKind::Gif, 3.0, "", "") }), ..base(25, "") });
            msgs.push(Msg { id: 85, media: Some(MediaInfo { size: (240, 240), ..media(MediaKind::Round, 5.0, "", "") }), media_unread: true, ..base(26, "Olena") });
            msgs.push(Msg { id: 73, dice: Some(("🎰".into(), 64)), ..base(27, "Olena") });
            msgs.push(Msg { id: 74, photo: true, spoiler: true, text: "Don't open it if you haven't seen the finale 🙈".into(), ..base(27, "Olena") });
            msgs.push(Msg { id: 75, photo: true, invert_media: true, text: "Caption above the photo, as in Telegram".into(), ..base(27, "") });
            msgs.push(Msg { id: 76, unsupported: true, ..base(27, "Olena") });
            msgs.push(custom_emoji::message(base(28, "Olena")));
            msgs.extend(effects::messages(base(29, "Olena")));
            msgs.extend(emoji17::messages(base(29, "Olena")));
        }
        4 => msgs.push(aitext::post(base(30, "Tech News"))),
        9 => {
            // A long history: shows virtualization (only bubbles near the screen are drawn).
            const LONG: i32 = 3000;
            let lines = ["Who's on duty today?", "Me, after lunch", "The build broke on CI, looking", "Fixed, it was an outdated dependency", "👍", "Reminder: retro on Friday"];
            let now = jiff::Zoned::now();
            let old = (1..=LONG).map(|n| {
                let date = now.checked_sub(jiff::Span::new().days(i64::from((LONG - n) / 40 + 1))).ok();
                let day = date.as_ref().map_or_else(String::new, |d| d.strftime("%d.%m.%Y").to_string());
                // Real times so the calendar can jump through this history.
                let ts = date.map_or(0, |d| d.timestamp().as_second());
                let sender = ["Oleh", "Natalia", ""][(n % 3) as usize];
                let text = lines[(n as usize * 7) % lines.len()];
                Msg { id: n, text: text.into(), spans: format::from_entities(text, &[], 0), day, ts, reply_to: (n % 17 == 0).then_some(n - 2), ..base(n % 60, sender) }
            });
            let tail = std::mem::take(msgs);
            msgs.extend(old);
            msgs.extend(tail.into_iter().map(|m| Msg { id: m.id + LONG, ..m }));
        }
        _ => {}
    }
    factcheck::extras(id, msgs, base);
    if id == 4 {
        // After the fact-checked post, so ids stay ascending.
        msgs.push(instant_view::post(base(32, "Tech News")));
        msgs.extend(giveaways::posts(base));
    }
    if id == 6 {
        msgs.push(game_message(base(50, "Weather Bot")));
    }
}

/// A bot game: a card with its picture and the bot's "Play" button.
fn game_message(base: Msg) -> Msg {
    let page = WebPageInfo {
        id: 50,
        url: String::new(),
        site: l!("Гра", "Game").into(),
        title: "Cloud Catcher".into(),
        description: "Catch the raindrops before they reach the city. Beat your friends' scores!".into(),
        photo: true,
        iv: false,
        card: CardKind::Game,
    };
    let play = Button { text: l!("Грати в Cloud Catcher", "Play Cloud Catcher").into(), action: ButtonAction::Game, style: crate::backend::ButtonStyle::Plain };
    Msg { id: 50, text: String::new(), webpage: Some(page), buttons: vec![vec![play]], ..base }
}

fn weather_buttons() -> Vec<Vec<Button>> {
    use crate::backend::ButtonStyle;
    let b = |text: &str, action| Button { text: text.into(), action, style: ButtonStyle::Plain };
    // Bots may colour their buttons (`keyboardButtonStyle`).
    let styled = |style, b: Button| Button { style, ..b };
    vec![
        vec![styled(ButtonStyle::Success, b("Refresh", ButtonAction::Callback(b"now".to_vec()))), b("Tomorrow", ButtonAction::Callback(b"tomorrow".to_vec()))],
        vec![b("Weather map",ButtonAction::Url("https://www.windy.com".into())), b("Radar", ButtonAction::WebApp { url: "https://example.org/radar".into(), simple: false, bot: 6 })],
        vec![styled(ButtonStyle::Primary, b("Log in to the site", ButtonAction::UrlAuth { url: "https://example.org/weather/login".into(), button_id: 3 }))],
        vec![styled(ButtonStyle::Danger, b("Unsubscribe", ButtonAction::Callback(b"stop".to_vec())))],
    ]
}

fn rand_ish() -> i32 {
    jiff::Timestamp::now().subsec_nanosecond() % 1_000_000
}

/// A tall gradient for a demo story, a different hue per story.
/// Story ids of a demo profile list, newest first: Olena, the channel and our own profile have some.
fn profile_story_ids(peer: PeerId, list: StoryList) -> Vec<i32> {
    let ids: Vec<i32> = match (peer.bot_api_dialog_id_unchecked(), list) {
        (1, StoryList::Pinned) => (101..=107).collect(),
        (1, StoryList::Album(1)) => vec![101, 103, 105],
        (1, StoryList::Album(2)) => vec![102, 106],
        (4, StoryList::Pinned) => (201..=205).collect(),
        (4, StoryList::Album(3)) => vec![201, 203],
        (ME, StoryList::Pinned) => (301..=304).collect(),
        (ME, StoryList::Album(4)) => vec![301, 302],
        // Over one page, so "Show more" appears.
        (ME, StoryList::Archive) => (401..=440).collect(),
        _ => Vec::new(),
    };
    ids.into_iter().rev().collect()
}

