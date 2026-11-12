//! Conversion tests on recorded server replies: TL objects shaped like the ones Telegram sends
//! (layer 230), passed through grammers' own update stream so messages carry their peers the way
//! they do in the app. The client never connects: no network, no account.

use super::*;
use crate::format::Style;
use grammers_client::session::storages::MemorySession;
use tokio::sync::mpsc as tmpsc;

const ME: i64 = 1000;
const OLENA: i64 = 2001;
const BOT: i64 = 3001;
const GONE: i64 = 2002;
const FRIENDS: i64 = 400;
const RUST_UA: i64 = 500;
const NEWS: i64 = 600;
const DATE: i32 = 1_767_250_800;

/// The UI string the code under test picks; not `l!`, so the locale scanner keeps the strings with their code.
fn ui(uk: &'static str, en: &'static str) -> &'static str {
    crate::i18n::pick(uk, en)
}

/// A client that is never run: grammers builds peers and messages around it.
fn client() -> Client {
    Client::new(SenderPool::new(Arc::new(MemorySession::default()), 1).handle)
}

fn user(id: i64, first: &str, last: Option<&str>) -> tl::types::User {
    tl::types::User {
        is_self: false,
        contact: false,
        mutual_contact: false,
        deleted: false,
        bot: false,
        bot_chat_history: false,
        bot_nochats: false,
        verified: false,
        restricted: false,
        min: false,
        bot_inline_geo: false,
        support: false,
        scam: false,
        apply_min_photo: false,
        fake: false,
        bot_attach_menu: false,
        premium: false,
        attach_menu_enabled: false,
        bot_can_edit: false,
        close_friend: false,
        stories_hidden: false,
        stories_unavailable: false,
        contact_require_premium: false,
        bot_business: false,
        bot_has_main_app: false,
        bot_forum_view: false,
        bot_forum_can_manage_topics: false,
        bot_can_manage_bots: false,
        bot_guestchat: false,
        bot_guard: false,
        id,
        access_hash: Some(id * 7919),
        first_name: Some(first.to_owned()),
        last_name: last.map(str::to_owned),
        username: None,
        phone: None,
        photo: None,
        status: None,
        bot_info_version: None,
        restriction_reason: None,
        bot_inline_placeholder: None,
        lang_code: None,
        emoji_status: None,
        usernames: None,
        stories_max_id: None,
        color: None,
        profile_color: None,
        bot_active_users: None,
        bot_verification_icon: None,
        send_paid_messages_stars: None,
        linked_community_id: None,
    }
}

fn me() -> tl::types::User {
    tl::types::User { is_self: true, ..user(ME, "Петро", None) }
}

fn olena() -> tl::types::User {
    tl::types::User {
        contact: true,
        mutual_contact: true,
        username: Some("olena".into()),
        status: Some(tl::types::UserStatusOnline { expires: DATE + 300 }.into()),
        ..user(OLENA, "Олена", Some("Коваль"))
    }
}

fn bot() -> tl::types::User {
    tl::types::User {
        bot: true,
        bot_info_version: Some(3),
        username: Some("weatherbot".into()),
        status: Some(tl::types::UserStatusRecently { by_me: false }.into()),
        ..user(BOT, "Weather", Some("Bot"))
    }
}

/// A deleted account: the server keeps the id and drops the name.
fn gone() -> tl::types::User {
    tl::types::User { deleted: true, first_name: None, ..user(GONE, "", None) }
}

fn channel(id: i64, title: &str, broadcast: bool) -> tl::types::Channel {
    tl::types::Channel {
        creator: false,
        left: false,
        broadcast,
        verified: false,
        megagroup: !broadcast,
        restricted: false,
        signatures: false,
        min: false,
        scam: false,
        has_link: false,
        has_geo: false,
        slowmode_enabled: false,
        call_active: false,
        call_not_empty: false,
        fake: false,
        gigagroup: false,
        noforwards: false,
        join_to_send: false,
        join_request: false,
        forum: false,
        stories_hidden: false,
        stories_hidden_min: false,
        stories_unavailable: false,
        signature_profiles: false,
        autotranslation: false,
        broadcast_messages_allowed: false,
        monoforum: false,
        forum_tabs: false,
        id,
        access_hash: Some(id * 104_729),
        title: title.to_owned(),
        username: None,
        photo: tl::enums::ChatPhoto::Empty,
        date: DATE - 86_400,
        restriction_reason: None,
        admin_rights: None,
        banned_rights: None,
        default_banned_rights: None,
        participants_count: None,
        usernames: None,
        stories_max_id: None,
        color: None,
        profile_color: None,
        emoji_status: None,
        level: None,
        subscription_until_date: None,
        bot_verification_icon: None,
        send_paid_messages_stars: None,
        linked_monoforum_id: None,
        linked_community_id: None,
    }
}

/// A forum supergroup where members may not pin.
fn rust_ua() -> tl::types::Channel {
    tl::types::Channel { forum: true, default_banned_rights: Some(banned(true)), ..channel(RUST_UA, "Rust UA", false) }
}

/// A protected news channel with a reason it is hidden on iOS only.
fn news() -> tl::types::Channel {
    tl::types::Channel {
        noforwards: true,
        restricted: true,
        signatures: true,
        restriction_reason: Some(vec![reason("ios", "")]),
        ..channel(NEWS, "Новини", true)
    }
}

/// A basic group (not a supergroup) where everyone may pin.
fn friends() -> tl::types::Chat {
    tl::types::Chat {
        creator: false,
        left: false,
        deactivated: false,
        call_active: false,
        call_not_empty: false,
        noforwards: false,
        id: FRIENDS,
        title: "Друзі".into(),
        photo: tl::enums::ChatPhoto::Empty,
        participants_count: 5,
        date: DATE - 86_400,
        version: 3,
        migrated_to: None,
        admin_rights: None,
        default_banned_rights: Some(banned(false)),
    }
}

fn banned(pin: bool) -> tl::enums::ChatBannedRights {
    tl::types::ChatBannedRights {
        view_messages: false,
        send_messages: false,
        send_media: false,
        send_stickers: false,
        send_gifs: false,
        send_games: false,
        send_inline: false,
        embed_links: false,
        send_polls: false,
        change_info: true,
        invite_users: false,
        pin_messages: pin,
        manage_topics: true,
        send_photos: false,
        send_videos: false,
        send_roundvideos: false,
        send_audios: false,
        send_voices: false,
        send_docs: false,
        send_plain: false,
        edit_rank: false,
        send_reactions: false,
        manage_linked_peers: false,
        until_date: i32::MAX,
    }
    .into()
}

fn admin(post: bool, pin: bool) -> tl::enums::ChatAdminRights {
    tl::types::ChatAdminRights {
        change_info: false,
        post_messages: post,
        edit_messages: post,
        delete_messages: false,
        ban_users: false,
        invite_users: true,
        pin_messages: pin,
        add_admins: false,
        anonymous: false,
        manage_call: false,
        other: true,
        manage_topics: false,
        post_stories: false,
        edit_stories: false,
        delete_stories: false,
        manage_direct_messages: false,
        manage_ranks: false,
        manage_linked_peers: false,
        manage_welcome_messages: false,
    }
    .into()
}

fn reason(platform: &str, text: &str) -> tl::enums::RestrictionReason {
    tl::types::RestrictionReason { platform: platform.into(), reason: "copyright".into(), text: text.into() }.into()
}

fn peer_user(id: i64) -> tl::enums::Peer {
    tl::types::PeerUser { user_id: id }.into()
}

fn peer_chat(id: i64) -> tl::enums::Peer {
    tl::types::PeerChat { chat_id: id }.into()
}

fn peer_channel(id: i64) -> tl::enums::Peer {
    tl::types::PeerChannel { channel_id: id }.into()
}

fn message(id: i32, peer: tl::enums::Peer, from: Option<tl::enums::Peer>, text: &str) -> tl::types::Message {
    tl::types::Message {
        out: false,
        mentioned: false,
        media_unread: false,
        silent: false,
        post: false,
        from_scheduled: false,
        legacy: false,
        edit_hide: false,
        pinned: false,
        noforwards: false,
        invert_media: false,
        offline: false,
        video_processing_pending: false,
        paid_suggested_post_stars: false,
        paid_suggested_post_ton: false,
        id,
        from_id: from,
        from_boosts_applied: None,
        from_rank: None,
        peer_id: peer,
        saved_peer_id: None,
        fwd_from: None,
        via_bot_id: None,
        via_business_bot_id: None,
        guestchat_via_from: None,
        reply_to: None,
        date: DATE,
        message: text.to_owned(),
        media: None,
        reply_markup: None,
        entities: None,
        views: None,
        forwards: None,
        replies: None,
        edit_date: None,
        post_author: None,
        grouped_id: None,
        reactions: None,
        restriction_reason: None,
        ttl_period: None,
        quick_reply_shortcut_id: None,
        effect: None,
        factcheck: None,
        report_delivery_until_date: None,
        paid_message_stars: None,
        suggested_post: None,
        schedule_repeat_period: None,
        summary_from_language: None,
        rich_message: None,
    }
}

fn service(id: i32, peer: tl::enums::Peer, from: Option<tl::enums::Peer>, action: tl::enums::MessageAction) -> tl::enums::Message {
    tl::types::MessageService {
        out: false,
        mentioned: false,
        media_unread: false,
        reactions_are_possible: true,
        silent: false,
        post: false,
        legacy: false,
        id,
        from_id: from,
        peer_id: peer,
        saved_peer_id: None,
        reply_to: None,
        date: DATE,
        action,
        reactions: None,
        ttl_period: None,
    }
    .into()
}

fn document(mime: &str, size: i64, attributes: Vec<tl::enums::DocumentAttribute>) -> tl::enums::MessageMedia {
    let doc = tl::types::Document {
        id: 5_000_000_001,
        access_hash: 42,
        file_reference: vec![1, 2, 3],
        date: DATE,
        mime_type: mime.to_owned(),
        size,
        thumbs: None,
        video_thumbs: None,
        dc_id: 2,
        attributes,
    };
    tl::types::MessageMediaDocument {
        nopremium: false,
        spoiler: false,
        video: false,
        round: false,
        voice: false,
        document: Some(doc.into()),
        alt_documents: None,
        video_cover: None,
        video_timestamp: None,
        ttl_seconds: None,
    }
    .into()
}

fn video_attr(duration: f64, w: i32, h: i32, round: bool) -> tl::enums::DocumentAttribute {
    tl::types::DocumentAttributeVideo {
        round_message: round,
        supports_streaming: true,
        nosound: false,
        duration,
        w,
        h,
        preload_prefix_size: None,
        video_start_ts: None,
        video_codec: None,
    }
    .into()
}

fn audio_attr(voice: bool, duration: i32, title: Option<&str>, performer: Option<&str>, waveform: Option<Vec<u8>>) -> tl::enums::DocumentAttribute {
    tl::types::DocumentAttributeAudio { voice, duration, title: title.map(str::to_owned), performer: performer.map(str::to_owned), waveform }.into()
}

fn file_name(name: &str) -> tl::enums::DocumentAttribute {
    tl::types::DocumentAttributeFilename { file_name: name.to_owned() }.into()
}

fn photo(spoiler: bool) -> tl::enums::MessageMedia {
    let photo = tl::types::Photo {
        has_stickers: false,
        id: 6_000_000_001,
        access_hash: 77,
        file_reference: vec![9],
        date: DATE,
        sizes: Vec::new(),
        video_sizes: None,
        dc_id: 2,
    };
    tl::types::MessageMediaPhoto { spoiler, live_photo: false, photo: Some(photo.into()), ttl_seconds: None, video: None }.into()
}

/// The users and chats every reply carries alongside its messages.
fn everyone() -> (Vec<tl::enums::User>, Vec<tl::enums::Chat>) {
    (
        vec![me().into(), olena().into(), bot().into(), gone().into()],
        vec![friends().into(), rust_ua().into(), news().into()],
    )
}

/// Delivers one message as an `updates` push and returns it as the app's worker sees it.
fn deliver(raw: impl Into<tl::enums::Message>) -> Message {
    let raw = raw.into();
    let rt = tokio::runtime::Builder::new_current_thread().enable_all().build().unwrap();
    rt.block_on(async {
        let client = client();
        let (tx, rx) = tmpsc::channel(4);
        let mut stream = client.stream_updates(rx, UpdatesConfiguration::default()).await.unwrap();
        let peer = match &raw {
            tl::enums::Message::Message(m) => &m.peer_id,
            tl::enums::Message::Service(m) => &m.peer_id,
            tl::enums::Message::Empty(_) => panic!("empty message"),
        };
        let in_channel = matches!(peer, tl::enums::Peer::Channel(_));
        let update: tl::enums::Update = if in_channel {
            tl::types::UpdateNewChannelMessage { message: raw, pts: 10, pts_count: 1 }.into()
        } else {
            tl::types::UpdateNewMessage { message: raw, pts: 10, pts_count: 1 }.into()
        };
        let (users, chats) = everyone();
        let updates = tl::types::Updates { updates: vec![update], users, chats, date: DATE, seq: 0 };
        tx.send(UpdatesLike::Updates(updates.into())).await.unwrap();
        match stream.next().await.unwrap() {
            Update::NewMessage(m) => m.into_inner(),
            _ => panic!("not a new message"),
        }
    })
}

/// The names map as the worker fills it from the dialog list.
fn names() -> Names {
    let names: HashMap<PeerId, String> = [(PeerId::from(peer_user(OLENA)), "Олена Коваль".to_owned()), (PeerId::from(peer_channel(NEWS)), "Новини".to_owned())].into();
    Arc::new(Mutex::new(names))
}

fn spanned<'a>(m: &'a Msg, style: Style) -> Vec<&'a str> {
    m.spans.iter().filter(|s| s.style == style).map(|s| &m.text[s.start..s.end]).collect()
}

#[test]
fn private_message_takes_sender_from_peer() {
    let m = convert(&deliver(message(1, peer_user(OLENA), None, "Привіт!")), &names());
    assert_eq!(m.sender, "Олена Коваль");
    assert_eq!(m.peer, PeerId::from(peer_user(OLENA)));
    assert!(!m.out);
    assert_eq!(m.text, "Привіт!");
    assert_eq!(m.ts, DATE as i64);
    assert!(m.spans.is_empty() && !m.edited && !m.photo && m.forwarded.is_none());
}

#[test]
fn outgoing_message_in_saved_messages() {
    let raw = tl::types::Message { out: true, ..message(2, peer_user(ME), None, "нотатка") };
    let wire = deliver(raw);
    let m = convert(&wire, &names());
    assert!(m.out);
    assert_eq!(m.text, "нотатка");
    assert_eq!(chat_kind(m.peer, &wire), ChatKind::Saved);
}

#[test]
fn entities_map_utf16_offsets_to_bytes() {
    // Offsets count UTF-16 units: 👋 is two of them, Cyrillic letters are one each but two bytes.
    let text = "Привіт 👋 світ! Деталі: https://glassgram.app @olena #rust";
    let raw = tl::types::Message {
        entities: Some(vec![
            tl::types::MessageEntityBold { offset: 10, length: 4 }.into(),
            tl::types::MessageEntityUrl { offset: 24, length: 21 }.into(),
            tl::types::MessageEntityMention { offset: 46, length: 6 }.into(),
            tl::types::MessageEntityHashtag { offset: 53, length: 5 }.into(),
        ]),
        ..message(3, peer_user(OLENA), None, text)
    };
    let m = convert(&deliver(raw), &names());
    assert_eq!(spanned(&m, Style::Bold), ["світ"]);
    assert_eq!(m.spans[0].start, "Привіт 👋 ".len());
    assert_eq!(spanned(&m, Style::Link), ["https://glassgram.app", "@olena", "#rust"]);
    // Auto-detected links point at their own text.
    let targets: Vec<_> = m.spans.iter().filter_map(|s| s.target.as_deref()).collect();
    assert_eq!(targets, ["https://glassgram.app", "@olena", "#rust"]);
}

#[test]
fn entities_after_zwj_emoji_and_custom_emoji() {
    // 👨‍👩‍👧 is eight UTF-16 units (three people and two joiners).
    let text = "Сім'я 👨‍👩‍👧 вдома, кличте Олену ⭐";
    let raw = tl::types::Message {
        entities: Some(vec![
            tl::types::MessageEntityItalic { offset: 15, length: 5 }.into(),
            tl::types::MessageEntityMentionName { offset: 29, length: 5, user_id: OLENA }.into(),
            tl::types::MessageEntityCustomEmoji { offset: 35, length: 1, document_id: 5_368_324_170_671_202_286 }.into(),
            tl::types::MessageEntitySpoiler { offset: 6, length: 8 }.into(),
        ]),
        ..message(4, peer_chat(FRIENDS), Some(peer_user(OLENA)), text)
    };
    let m = convert(&deliver(raw), &names());
    assert_eq!(spanned(&m, Style::Italic), ["вдома"]);
    assert_eq!(spanned(&m, Style::Spoiler), ["👨‍👩‍👧"]);
    let mention = m.spans.iter().find(|s| s.style == Style::Link).unwrap();
    assert_eq!(&m.text[mention.start..mention.end], "Олену");
    assert_eq!(mention.target.as_deref(), Some(format!("tg://user?id={OLENA}").as_str()));
    let emoji = m.spans.iter().find(|s| s.style == Style::CustomEmoji).unwrap();
    assert_eq!(&m.text[emoji.start..emoji.end], "⭐");
    assert_eq!(emoji.target.as_deref(), Some("5368324170671202286"));
}

#[test]
fn entity_past_the_end_is_clamped_or_dropped() {
    // Servers have sent lengths that run past the text; nothing must panic or slice mid-char.
    let raw = tl::types::Message {
        entities: Some(vec![
            tl::types::MessageEntityBold { offset: 3, length: 100 }.into(),
            tl::types::MessageEntityCode { offset: 50, length: 2 }.into(),
        ]),
        ..message(5, peer_user(OLENA), None, "Ще 🙂")
    };
    let m = convert(&deliver(raw), &names());
    assert_eq!(spanned(&m, Style::Bold), ["🙂"]);
    assert!(spanned(&m, Style::Code).is_empty());
}

#[test]
fn group_message_names_the_member_and_detects_group() {
    let wire = deliver(message(6, peer_chat(FRIENDS), Some(peer_user(BOT)), "Сьогодні +3°"));
    let m = convert(&wire, &names());
    assert_eq!(m.sender, "Weather Bot");
    assert_eq!(chat_kind(m.peer, &wire), ChatKind::Group);
    assert_eq!(short_name(wire.sender().unwrap()), "Weather");
}

#[test]
fn supergroup_is_a_group_and_broadcast_is_a_channel() {
    let group = deliver(message(7, peer_channel(RUST_UA), Some(peer_user(OLENA)), "cargo 1.95"));
    assert_eq!(chat_kind(group.peer_id(), &group), ChatKind::Group);
    let post = deliver(tl::types::Message { post: true, ..message(8, peer_channel(NEWS), None, "Анонс") });
    assert_eq!(chat_kind(post.peer_id(), &post), ChatKind::Channel);
    // A channel post without a signature has no sender of its own.
    assert_eq!(convert(&post, &names()).sender, "");
}

#[test]
fn chat_kind_falls_back_to_the_id_when_the_peer_is_not_cached() {
    // Neither chat came with the reply.
    let wire = deliver(message(9, peer_channel(77), Some(peer_user(OLENA)), "x"));
    assert!(wire.peer().is_none());
    assert_eq!(chat_kind(wire.peer_id(), &wire), ChatKind::Channel);
    let wire = deliver(message(9, peer_chat(78), Some(peer_user(OLENA)), "x"));
    assert_eq!(chat_kind(wire.peer_id(), &wire), ChatKind::Group);
}

#[test]
fn channel_post_counters_flags_and_comments() {
    let raw = tl::types::Message {
        post: true,
        pinned: true,
        noforwards: true,
        views: Some(12_345),
        forwards: Some(17),
        edit_date: Some(DATE + 60),
        post_author: Some("Редакція".into()),
        grouped_id: Some(13_579),
        replies: Some(
            tl::types::MessageReplies { comments: true, replies: 42, replies_pts: 100, recent_repliers: None, channel_id: Some(RUST_UA), max_id: Some(900), read_max_id: None }.into(),
        ),
        ..message(10, peer_channel(NEWS), None, "Новина дня")
    };
    let m = convert(&deliver(raw), &names());
    assert_eq!(m.views, Some(12_345));
    assert_eq!(m.comments, Some(42));
    assert_eq!(m.group, Some(13_579));
    assert!(m.pinned && m.noforwards && m.edited);
}

#[test]
fn thread_replies_are_not_comments() {
    let raw = tl::types::Message {
        replies: Some(tl::types::MessageReplies { comments: false, replies: 5, replies_pts: 1, recent_repliers: None, channel_id: None, max_id: None, read_max_id: None }.into()),
        ..message(11, peer_channel(RUST_UA), Some(peer_user(OLENA)), "питання")
    };
    assert_eq!(convert(&deliver(raw), &names()).comments, None);
}

fn fwd(from_id: Option<tl::enums::Peer>, from_name: Option<&str>, post_author: Option<&str>) -> tl::enums::MessageFwdHeader {
    tl::types::MessageFwdHeader {
        imported: false,
        saved_out: false,
        from_id,
        from_name: from_name.map(str::to_owned),
        date: DATE - 3600,
        channel_post: post_author.map(|_| 321),
        post_author: post_author.map(str::to_owned),
        saved_from_peer: None,
        saved_from_msg_id: None,
        saved_from_id: None,
        saved_from_name: None,
        saved_date: None,
        psa_type: None,
    }
    .into()
}

#[test]
fn forwarded_headers() {
    let names = names();
    let from_channel = tl::types::Message { fwd_from: Some(fwd(Some(peer_channel(NEWS)), None, Some("Редакція"))), ..message(12, peer_user(OLENA), None, "текст") };
    assert_eq!(convert(&deliver(from_channel), &names).forwarded.as_deref(), Some("Новини (Редакція)"));
    // Someone who hides their account in forwards: only a name comes.
    let hidden = tl::types::Message { fwd_from: Some(fwd(None, Some("Таємний Автор"), None)), ..message(13, peer_user(OLENA), None, "текст") };
    assert_eq!(convert(&deliver(hidden), &names).forwarded.as_deref(), Some("Таємний Автор"));
    let unknown = tl::types::Message { fwd_from: Some(fwd(Some(peer_user(9999)), None, None)), ..message(14, peer_user(OLENA), None, "текст") };
    assert_eq!(convert(&deliver(unknown), &names).forwarded.as_deref(), Some(ui("невідомого", "unknown")));
}

fn reply_header(msg_id: i32, top: Option<i32>, forum: bool) -> tl::types::MessageReplyHeader {
    tl::types::MessageReplyHeader {
        reply_to_scheduled: false,
        forum_topic: forum,
        quote: false,
        reply_to_ephemeral: false,
        reply_to_msg_id: Some(msg_id),
        reply_to_peer_id: None,
        reply_from: None,
        reply_media: None,
        reply_to_top_id: top,
        quote_text: None,
        quote_entities: None,
        quote_offset: None,
        todo_item_id: None,
        poll_option: None,
    }
}

#[test]
fn replies_quotes_and_replies_to_other_chats() {
    let names = names();
    let quoted = tl::types::Message {
        reply_to: Some(tl::types::MessageReplyHeader { quote: true, quote_text: Some("саме це".into()), quote_offset: Some(4), ..reply_header(40, None, false) }.into()),
        ..message(15, peer_user(OLENA), None, "Так")
    };
    let m = convert(&deliver(quoted), &names);
    assert_eq!(m.reply_to, Some(40));
    assert_eq!(m.reply_quote.as_deref(), Some("саме це"));
    assert_eq!(m.reply_elsewhere, None);

    let elsewhere = tl::types::Message {
        reply_to: Some(tl::types::MessageReplyHeader { reply_to_peer_id: Some(peer_channel(NEWS)), ..reply_header(77, None, false) }.into()),
        ..message(16, peer_user(OLENA), None, "Бачив?")
    };
    assert_eq!(convert(&deliver(elsewhere), &names).reply_elsewhere.as_deref(), Some("Новини"));
}

#[test]
fn forum_topics_from_reply_headers_and_topic_creation() {
    let in_topic = tl::types::Message { reply_to: Some(reply_header(120, Some(100), true).into()), ..message(17, peer_channel(RUST_UA), Some(peer_user(OLENA)), "у темі") };
    assert_eq!(convert(&deliver(in_topic), &names()).topic, Some(100));
    // The first reply in a topic has no top id: the replied message is the topic itself.
    let first = tl::types::Message { reply_to: Some(reply_header(100, None, true).into()), ..message(18, peer_channel(RUST_UA), Some(peer_user(OLENA)), "перша") };
    assert_eq!(convert(&deliver(first), &names()).topic, Some(100));
    let plain = tl::types::Message { reply_to: Some(reply_header(100, None, false).into()), ..message(19, peer_channel(RUST_UA), Some(peer_user(OLENA)), "звичайна") };
    assert_eq!(convert(&deliver(plain), &names()).topic, None);

    let created = tl::types::MessageActionTopicCreate { title_missing: false, title: "Питання".into(), icon_color: 0x6FB9F0, icon_emoji_id: None };
    let wire = deliver(service(100, peer_channel(RUST_UA), Some(peer_user(OLENA)), created.into()));
    assert_eq!(message_topic(&wire), Some(100));
}

#[test]
fn service_messages_get_a_placeholder_text() {
    let created = tl::types::MessageActionChatCreate { title: "Друзі".into(), users: vec![ME, OLENA] };
    let wire = deliver(service(20, peer_chat(FRIENDS), Some(peer_user(OLENA)), created.into()));
    let m = convert(&wire, &names());
    assert_eq!(m.text, ui("Сервісне повідомлення", "Service message"));
    assert_eq!(m.sender, "Олена Коваль");
    assert!(m.spans.is_empty());
    assert_eq!(preview_text(&wire), ui("Сервісне повідомлення", "Service message"));
    let pinned = deliver(service(21, peer_chat(FRIENDS), Some(peer_user(OLENA)), tl::enums::MessageAction::PinMessage));
    assert!(pinned.action().is_some());
    assert_eq!(convert(&pinned, &names()).text, ui("Сервісне повідомлення", "Service message"));
}

#[test]
fn photo_with_caption_keeps_caption_spans() {
    let raw = tl::types::Message {
        media: Some(photo(true)),
        entities: Some(vec![tl::types::MessageEntityBold { offset: 8, length: 5 }.into(), tl::types::MessageEntityItalic { offset: 19, length: 8 }.into()]),
        ..message(22, peer_user(OLENA), None, "Захід о 19:00 🎉 у кав'ярні")
    };
    let wire = deliver(raw);
    let m = convert(&wire, &names());
    assert!(m.photo && m.spoiler);
    assert_eq!(m.text, "Захід о 19:00 🎉 у кав'ярні");
    assert_eq!(spanned(&m, Style::Bold), ["19:00"]);
    assert_eq!(spanned(&m, Style::Italic), ["кав'ярні"]);
    assert_eq!(preview_text(&wire), format!("{} · Захід о 19:00 🎉 у кав'ярні", ui("📷 Фото", "📷 Photo")));
    assert_eq!(preview_text(&deliver(tl::types::Message { media: Some(photo(false)), ..message(23, peer_user(OLENA), None, "") })), ui("📷 Фото", "📷 Photo"));
}

#[test]
fn voice_message_with_waveform() {
    let levels: Vec<u8> = (0..20).map(|i| (i * 3 % 32) as u8).collect();
    let raw = tl::types::Message {
        media_unread: true,
        media: Some(document("audio/ogg", 9_812, vec![audio_attr(true, 7, None, None, Some(pack_waveform(&levels)))])),
        ..message(24, peer_user(OLENA), None, "")
    };
    let wire = deliver(raw);
    let m = convert(&wire, &names());
    let info = m.media.as_ref().unwrap();
    assert_eq!(info.kind, MediaKind::Voice);
    assert_eq!(info.duration, 7.0);
    assert_eq!(info.bytes, 9_812);
    assert_eq!(&info.waveform[..20], &levels[..]);
    assert!(m.media_unread && !m.mentioned && m.file.is_none());
    assert_eq!(m.text, "");
    assert_eq!(preview_text(&wire), ui("🎤 Голосове повідомлення", "🎤 Voice message"));
}

#[test]
fn round_video_with_sound_is_round_not_audio() {
    // The server adds an audio attribute to videos with sound.
    let media = document("video/mp4", 1_200_000, vec![video_attr(9.5, 384, 384, true), audio_attr(false, 9, None, None, None)]);
    let wire = deliver(tl::types::Message { media: Some(media), ..message(25, peer_chat(FRIENDS), Some(peer_user(OLENA)), "") });
    let info = media_info(&wire).unwrap();
    assert_eq!(info.kind, MediaKind::Round);
    assert_eq!(info.size, (384, 384));
    assert_eq!(info.duration, 9.5);
    assert_eq!(preview_text(&wire), ui("📹 Відеоповідомлення", "📹 Video message"));
}

#[test]
fn gifs_videos_and_music() {
    let gif = document("video/mp4", 300_000, vec![video_attr(2.0, 480, 270, false), tl::enums::DocumentAttribute::Animated, file_name("cat.mp4")]);
    assert_eq!(media_info(&deliver(tl::types::Message { media: Some(gif), ..message(26, peer_user(OLENA), None, "") })).unwrap().kind, MediaKind::Gif);
    // Old GIF uploads have no video attribute, only the MIME type.
    let old_gif = document("image/gif", 50_000, vec![file_name("old.gif")]);
    assert_eq!(media_info(&deliver(tl::types::Message { media: Some(old_gif), ..message(27, peer_user(OLENA), None, "") })).unwrap().kind, MediaKind::Gif);

    let video = document("video/mp4", 8_000_000, vec![video_attr(61.0, 1280, 720, false), file_name("trip.mp4")]);
    let m = convert(&deliver(tl::types::Message { media: Some(video), ..message(28, peer_user(OLENA), None, "Поїздка") }), &names());
    assert_eq!(m.media.as_ref().map(|i| (i.kind, i.size, i.title.as_str())), Some((MediaKind::Video, (1280, 720), "trip.mp4")));
    assert_eq!(m.text, "Поїздка");

    let song = document("audio/mpeg", 4_000_000, vec![audio_attr(false, 215, Some("Стефанія"), Some("Kalush Orchestra"), None), file_name("track.mp3")]);
    let info = media_info(&deliver(tl::types::Message { media: Some(song), ..message(29, peer_user(OLENA), None, "") })).unwrap();
    assert_eq!((info.kind, info.title.as_str(), info.performer.as_str()), (MediaKind::Audio, "Стефанія", "Kalush Orchestra"));
    // A track without tags shows its file name.
    let untagged = document("audio/mpeg", 4_000, vec![audio_attr(false, 3, Some(""), None, None), file_name("demo.mp3")]);
    assert_eq!(media_info(&deliver(tl::types::Message { media: Some(untagged), ..message(30, peer_user(OLENA), None, "") })).unwrap().title, "demo.mp3");
}

#[test]
fn plain_document_becomes_a_file() {
    let doc = document("application/pdf", 2_621_440, vec![file_name("Звіт за квартал.pdf")]);
    let wire = deliver(tl::types::Message { media: Some(doc), ..message(31, peer_user(OLENA), None, "") });
    let m = convert(&wire, &names());
    assert_eq!(m.file, Some(("Звіт за квартал.pdf".to_owned(), human_size(2_621_440))));
    assert!(m.media.is_none());
    assert_eq!(preview_text(&wire), ui("📎 Файл", "📎 File"));
}

#[test]
fn stickers_static_video_and_animated() {
    let sticker = |mime: &str| {
        let attr = tl::types::DocumentAttributeSticker { mask: false, alt: "😺".into(), stickerset: tl::enums::InputStickerSet::Empty, mask_coords: None };
        document(mime, 20_000, vec![attr.into()])
    };
    let webp = convert(&deliver(tl::types::Message { media: Some(sticker("image/webp")), ..message(32, peer_user(OLENA), None, "") }), &names());
    assert_eq!(webp.sticker, Some((5_000_000_001, "😺".to_owned())));
    assert!(!webp.video_sticker && !webp.lottie && webp.file.is_none());
    let webm = convert(&deliver(tl::types::Message { media: Some(sticker("video/webm")), ..message(33, peer_user(OLENA), None, "") }), &names());
    assert!(webm.video_sticker && !webm.lottie);
    let tgs = convert(&deliver(tl::types::Message { media: Some(sticker("application/x-tgsticker")), ..message(34, peer_user(OLENA), None, "") }), &names());
    assert!(tgs.lottie && !tgs.video_sticker);
}

#[test]
fn contact_dice_and_places() {
    let contact = tl::types::MessageMediaContact { phone_number: "+380501234567".into(), first_name: "Олена".into(), last_name: "".into(), vcard: "".into(), user_id: OLENA };
    let m = convert(&deliver(tl::types::Message { media: Some(contact.into()), ..message(35, peer_user(OLENA), None, "") }), &names());
    assert_eq!(m.contact, Some(("Олена".to_owned(), "+380501234567".to_owned())));
    assert_eq!(m.text, "");

    let dice = tl::types::MessageMediaDice { value: 6, emoticon: "🎯".into(), game_outcome: None };
    let wire = deliver(tl::types::Message { media: Some(dice.into()), ..message(36, peer_chat(FRIENDS), Some(peer_user(OLENA)), "") });
    assert_eq!(convert(&wire, &names()).dice, Some(("🎯".to_owned(), 6)));
    assert_eq!(preview_text(&wire), "🎯");

    let point = || tl::enums::GeoPoint::from(tl::types::GeoPoint { long: 30.5234, lat: 50.4501, access_hash: 1, accuracy_radius: None });
    let geo = tl::types::MessageMediaGeo { geo: point() };
    let m = convert(&deliver(tl::types::Message { media: Some(geo.into()), ..message(37, peer_user(OLENA), None, "") }), &names());
    let g = m.geo.unwrap();
    assert_eq!((g.lat, g.lon, g.live), (50.4501, 30.5234, false));
    let venue = tl::types::MessageMediaVenue { geo: point(), title: "Майдан".into(), address: "Київ".into(), provider: "foursquare".into(), venue_id: "1".into(), venue_type: "".into() };
    let wire = deliver(tl::types::Message { media: Some(venue.into()), ..message(38, peer_user(OLENA), None, "") });
    let g = geo_info(&wire).unwrap();
    assert_eq!((g.title.as_str(), g.address.as_str()), ("Майдан", "Київ"));
    assert_eq!(preview_text(&wire), ui("📍 Локація", "📍 Location"));
}

#[test]
fn unsupported_media_is_flagged() {
    let wire = deliver(tl::types::Message { media: Some(tl::enums::MessageMedia::Unsupported), ..message(39, peer_user(OLENA), None, "") });
    let m = convert(&wire, &names());
    assert!(m.unsupported && m.media.is_none() && m.file.is_none());
}

#[test]
fn inline_keyboard_buttons() {
    let button = |text: &str, r#type: tl::enums::InlineButtonType| tl::enums::KeyboardInlineButton::from(tl::types::KeyboardInlineButton { style: None, text: text.into(), r#type });
    let markup = tl::types::ReplyInlineMarkup {
        force_reply: false,
        rows: vec![
            tl::types::KeyboardInlineButtonRow {
                buttons: vec![
                    button("Оновити", tl::types::InlineButtonTypeCallback { requires_password: false, data: b"refresh".to_vec() }.into()),
                    button("Сайт", tl::types::InlineButtonTypeUrl { url: "https://example.org".into() }.into()),
                ],
            }
            .into(),
            tl::types::KeyboardInlineButtonRow { buttons: vec![button("Код", tl::types::InlineButtonTypeCopy { copy_text: "GG-2026".into() }.into()), button("Гра", tl::enums::InlineButtonType::Game)] }.into(),
            tl::types::KeyboardInlineButtonRow { buttons: vec![button("Увійти", tl::types::InlineButtonTypeUrlAuth { fwd_text: None, url: "https://example.org/login".into(), button_id: 7 }.into())] }.into(),
        ],
    };
    let raw = tl::types::Message { reply_markup: Some(markup.into()), ..message(40, peer_user(BOT), None, "Погода на сьогодні") };
    let m = convert(&deliver(raw), &names());
    assert_eq!(m.sender, "Weather Bot");
    let rows: Vec<Vec<(&str, &ButtonAction)>> = m.buttons.iter().map(|r| r.iter().map(|b| (b.text.as_str(), &b.action)).collect()).collect();
    assert_eq!(rows.len(), 3);
    assert!(matches!(rows[0][0], ("Оновити", ButtonAction::Callback(d)) if d == b"refresh"));
    assert!(matches!(rows[0][1], ("Сайт", ButtonAction::Url(u)) if u == "https://example.org"));
    assert!(matches!(rows[1][0], ("Код", ButtonAction::Copy(c)) if c == "GG-2026"));
    assert!(matches!(rows[1][1], ("Гра", ButtonAction::Other)));
    // The login button keeps its id: `requestUrlAuth` names the button, not the address.
    assert!(matches!(rows[2][0], ("Увійти", ButtonAction::UrlAuth { url, button_id: 7 }) if url == "https://example.org/login"));
}

#[test]
fn reactions_counts_and_unread() {
    let count = |emoji: &str, count: i32, mine: bool| tl::enums::ReactionCount::from(tl::types::ReactionCount {
        chosen_order: mine.then_some(0),
        reaction: tl::types::ReactionEmoji { emoticon: emoji.into() }.into(),
        count,
    });
    let recent = tl::types::MessagePeerReaction { big: false, unread: true, my: false, peer_id: peer_user(OLENA), date: DATE + 5, reaction: tl::types::ReactionEmoji { emoticon: "❤".into() }.into() };
    let reactions = tl::types::MessageReactions {
        min: false,
        can_see_list: true,
        reactions_as_tags: false,
        results: vec![count("👍", 3, true), count("❤", 1, false), tl::types::ReactionCount { chosen_order: None, reaction: tl::types::ReactionCustomEmoji { document_id: 1 }.into(), count: 2 }.into()],
        recent_reactions: Some(vec![recent.into()]),
        top_reactors: None,
    };
    let raw = tl::types::Message { out: true, reactions: Some(reactions.into()), ..message(41, peer_chat(FRIENDS), Some(peer_user(ME)), "Зустрічаємось о 7") };
    let m = convert(&deliver(raw), &names());
    let got: Vec<_> = m.reactions.iter().map(|r| (r.emoji.as_str(), r.count, r.mine)).collect();
    // Custom emoji reactions are not drawn yet.
    assert_eq!(got, [("👍", 3, true), ("❤", 1, false)]);
    assert!(m.reaction_unread);
}

#[test]
fn unread_mention_needs_unread_contents() {
    let fresh = tl::types::Message { mentioned: true, media_unread: true, ..message(42, peer_chat(FRIENDS), Some(peer_user(OLENA)), "@petro глянь") };
    assert!(convert(&deliver(fresh), &names()).mentioned);
    let seen = tl::types::Message { mentioned: true, media_unread: false, ..message(43, peer_chat(FRIENDS), Some(peer_user(OLENA)), "@petro глянь") };
    assert!(!convert(&deliver(seen), &names()).mentioned);
}

#[test]
fn restricted_messages_hide_content_on_desktop_only() {
    let hidden = tl::types::Message {
        media: Some(photo(false)),
        restriction_reason: Some(vec![reason("ios", "Not on iOS"), reason("all", "Недоступно у вашій країні")]),
        ..message(44, peer_channel(NEWS), None, "секрет")
    };
    let m = convert(&deliver(hidden), &names());
    assert_eq!(m.text, "Недоступно у вашій країні");
    assert!(!m.photo && m.spans.is_empty());
    let mobile_only = tl::types::Message { restriction_reason: Some(vec![reason("android", "x")]), ..message(45, peer_channel(NEWS), None, "видно") };
    assert_eq!(convert(&deliver(mobile_only), &names()).text, "видно");
    let no_text = tl::types::Message { restriction_reason: Some(vec![reason("all", "")]), ..message(46, peer_channel(NEWS), None, "x") };
    assert_eq!(convert(&deliver(no_text), &names()).text, ui("Це повідомлення недоступне", "This message is unavailable"));
}

#[test]
fn saved_messages_remember_their_source() {
    let raw = tl::types::Message { out: true, saved_peer_id: Some(peer_channel(NEWS)), fwd_from: Some(fwd(Some(peer_channel(NEWS)), None, None)), ..message(47, peer_user(ME), None, "збережено") };
    let m = convert(&deliver(raw), &names());
    assert_eq!(m.saved_from, Some(PeerId::from(peer_channel(NEWS))));
    assert_eq!(m.forwarded.as_deref(), Some("Новини"));
}

#[test]
fn scheduled_message_without_peers() {
    let raw = tl::types::Message {
        out: true,
        from_scheduled: true,
        entities: Some(vec![tl::types::MessageEntityUnderline { offset: 9, length: 6 }.into()]),
        ..message(48, peer_user(OLENA), None, "Нагадую: завтра")
    };
    let m = convert_raw(PeerId::from(peer_user(OLENA)), &raw);
    assert!(m.out);
    assert_eq!(spanned(&m, Style::Underline), ["завтра"]);
    let attachment = tl::types::Message { media: Some(photo(false)), ..message(49, peer_user(OLENA), None, "") };
    assert_eq!(convert_raw(PeerId::from(peer_user(OLENA)), &attachment).text, ui("📎 Вкладення", "📎 Attachment"));
}

#[test]
fn draft_keeps_markup() {
    let draft = tl::types::DraftMessage {
        no_webpage: false,
        invert_media: false,
        reply_to: None,
        message: "Купити хліб".into(),
        entities: Some(vec![tl::types::MessageEntityBold { offset: 7, length: 4 }.into()]),
        media: None,
        date: DATE,
        effect: None,
        suggested_post: None,
        rich_message: None,
    };
    let text = draft_text(&Some(draft.into()));
    assert!(text.contains("хліб") && text != "Купити хліб", "markup lost: {text}");
    assert_eq!(format::parse(&text).0, "Купити хліб");
    assert_eq!(draft_text(&None), "");
}

#[test]
fn people_and_chats_found_outside_the_dialog_list() {
    let client = client();
    let olena = Peer::User(User::from_raw(&client, olena().into()));
    let c = person(&olena);
    assert_eq!((c.title.as_str(), c.last_text.as_str(), c.kind), ("Олена Коваль", "@olena", ChatKind::User));
    assert!(c.contact && c.joined && c.can_pin);
    assert_eq!(c.presence, Presence::Online((DATE + 300) as i64));

    let bot = found_chat(&Peer::User(User::from_raw(&client, bot().into())), String::new());
    assert_eq!((bot.kind, bot.presence), (ChatKind::Bot, Presence::None));
    let gone = Peer::User(User::from_raw(&client, gone().into()));
    assert_eq!(peer_title(&gone), ui("Видалений акаунт", "Deleted account"));
    assert_eq!(short_name(&gone), "?");

    let forum = found_chat(&Peer::from_raw(&client, rust_ua().into()), String::new());
    assert_eq!((forum.kind, forum.forum, forum.joined), (ChatKind::Group, true, false));
    assert!(forum.can_post && !forum.can_pin);

    let news = found_chat(&Peer::from_raw(&client, news().into()), String::new());
    assert_eq!((news.kind, news.title.as_str()), (ChatKind::Channel, "Новини"));
    assert!(news.noforwards && !news.can_post && !news.can_pin);
    // Hidden only on iOS: still visible here.
    assert_eq!(news.restricted, None);

    let friends = found_chat(&Peer::from_raw(&client, friends().into()), String::new());
    assert_eq!(friends.kind, ChatKind::Group);
    assert!(friends.can_pin && !friends.forum);
}

#[test]
fn admin_rights_decide_posting_and_pinning() {
    let client = client();
    let editor = Peer::from_raw(&client, tl::types::Channel { admin_rights: Some(admin(true, false)), ..news() }.into());
    assert!(can_post(&editor) && !can_pin(&editor));
    let owner = Peer::from_raw(&client, tl::types::Channel { creator: true, ..news() }.into());
    assert!(can_post(&owner) && can_pin(&owner));
    let moderator = Peer::from_raw(&client, tl::types::Channel { admin_rights: Some(admin(false, true)), ..rust_ua() }.into());
    assert!(can_pin(&moderator));
}

#[test]
fn chat_restriction_for_desktop() {
    let client = client();
    let blocked = Peer::from_raw(&client, tl::types::Channel { restriction_reason: Some(vec![reason("all", "Заблоковано")]), ..news() }.into());
    assert_eq!(chat_restriction(&blocked).as_deref(), Some("Заблоковано"));
    let bot = tl::types::User { restricted: true, restriction_reason: Some(vec![reason("desktop", "")]), ..bot() };
    assert_eq!(chat_restriction(&Peer::User(User::from_raw(&client, bot.into()))).as_deref(), Some(ui("Цей чат недоступний", "This chat is unavailable")));
    // The flag must be set too; reasons alone don't hide a chat.
    let stale = Peer::from_raw(&client, tl::types::Channel { restricted: false, restriction_reason: Some(vec![reason("all", "x")]), ..news() }.into());
    assert_eq!(chat_restriction(&stale), None);
}

#[test]
fn call_messages_become_call_cards() {
    let action = tl::types::MessageActionPhoneCall { video: true, call_id: 9, reason: Some(tl::enums::PhoneCallDiscardReason::Missed), duration: None };
    let wire = deliver(service(30, peer_user(OLENA), Some(peer_user(OLENA)), action.into()));
    let m = convert(&wire, &names());
    let call = m.call.expect("a call card");
    assert!(call.video && call.end == crate::backend::CallEnd::Missed);
    // The card shows the label itself: no placeholder text under it.
    assert!(m.text.is_empty());
    assert_eq!(preview_text(&wire), ui("Пропущений відеодзвінок", "Missed video call"));
}
