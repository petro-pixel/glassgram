//! Turning Telegram objects into our types: messages, previews, buttons, media, polls, places, times.

use super::*;

/// A person for the contact list, with @username under the name when there is one.
pub(super) fn person(peer: &Peer) -> ChatInfo {
    let mut c = found_chat(peer, peer.username().map(|u| format!("@{u}")).unwrap_or_default());
    c.joined = true;
    c
}

pub(super) fn inline_result(r: tl::enums::BotInlineResult) -> InlineResult {
    match r {
        tl::enums::BotInlineResult::Result(r) => InlineResult {
            id: r.id,
            title: r.title.unwrap_or_default(),
            description: r.description.or(r.url).unwrap_or_default(),
            kind: r.r#type,
        },
        tl::enums::BotInlineResult::BotInlineMediaResult(r) => InlineResult {
            id: r.id,
            title: r.title.unwrap_or_default(),
            description: r.description.unwrap_or_default(),
            kind: r.r#type,
        },
    }
}

/// Raw message (scheduled ones arrive without the members map) → `Msg`.
pub(super) fn convert_raw(peer: PeerId, m: &tl::types::Message) -> Msg {
    let ts = jiff::Timestamp::from_second(m.date as i64).unwrap_or_default();
    let text = if m.message.is_empty() && m.media.is_some() { l!("📎 Вкладення", "📎 Attachment").to_owned() } else { m.message.clone() };
    let spans = if m.message.is_empty() { Vec::new() } else { format::from_entities(&m.message, m.entities.as_deref().unwrap_or_default(), 0) };
    Msg { id: m.id, out: true, text, time: clock(ts), ts: m.date as i64, day: day(ts), spans, ..Msg::placeholder(peer) }
}

/// Input field text with markup → a message with entities.
pub(super) fn rich(markup: &str) -> InputMessage {
    let (text, entities) = format::parse(markup);
    InputMessage::new().text(text).fmt_entities(entities)
}

pub(super) fn dialog_peer(r: PeerRef) -> tl::enums::InputDialogPeer {
    tl::types::InputDialogPeer { peer: r.into() }.into()
}

pub(super) fn remember_name(names: &Names, m: &Message) {
    if let Some(s) = m.sender() {
        names.lock().unwrap().entry(s.id()).or_insert_with(|| peer_title(s));
    }
}

pub(super) fn draft_text(d: &Option<tl::enums::DraftMessage>) -> String {
    match d {
        Some(tl::enums::DraftMessage::Message(d)) => {
            let spans = format::from_entities(&d.message, d.entities.as_deref().unwrap_or_default(), 0);
            format::to_markup(&d.message, &spans)
        }
        _ => String::new(),
    }
}

pub(super) fn peer_title(peer: &Peer) -> String {
    match peer {
        Peer::User(u) if u.is_self() => l!("Збережене", "Saved Messages").to_owned(),
        Peer::User(u) if u.deleted() => l!("Видалений акаунт", "Deleted account").to_owned(),
        Peer::User(u) => u.full_name(),
        other => other.name().unwrap_or(l!("Без назви", "Untitled")).to_owned(),
    }
}

pub(super) fn short_name(peer: &Peer) -> String {
    match peer {
        Peer::User(u) => u.first_name().unwrap_or("?").to_owned(),
        other => other.name().unwrap_or("?").to_owned(),
    }
}

pub(super) fn peer_kind(peer: &Peer) -> ChatKind {
    match peer {
        Peer::User(u) if u.is_self() => ChatKind::Saved,
        Peer::User(u) if u.is_bot() => ChatKind::Bot,
        Peer::User(_) => ChatKind::User,
        Peer::Channel(_) => ChatKind::Channel,
        _ => ChatKind::Group,
    }
}

pub(super) fn media_label(m: &Message) -> Option<&'static str> {
    Some(match m.media()? {
        Media::Photo(_) => l!("📷 Фото", "📷 Photo"),
        Media::Document(_) => match media_info(m).map(|i| i.kind) {
            Some(MediaKind::Voice) => l!("🎤 Голосове повідомлення", "🎤 Voice message"),
            Some(MediaKind::Round) => l!("📹 Відеоповідомлення", "📹 Video message"),
            Some(MediaKind::Video) => l!("🎬 Відео", "🎬 Video"),
            Some(MediaKind::Gif) => "GIF",
            Some(MediaKind::Audio) => l!("🎵 Аудіо", "🎵 Audio"),
            None => l!("📎 Файл", "📎 File"),
        },
        Media::Sticker(_) => l!("Стікер", "Sticker"),
        Media::Contact(_) => l!("👤 Контакт", "👤 Contact"),
        Media::Poll(_) => l!("📊 Опитування", "📊 Poll"),
        Media::Geo(_) | Media::GeoLive(_) | Media::Venue(_) => l!("📍 Локація", "📍 Location"),
        Media::Dice(_) => "🎲",
        Media::WebPage(_) => return None,
        #[allow(unreachable_patterns)]
        _ => l!("Вкладення", "Attachment"),
    })
}

pub(super) fn preview_text(m: &Message) -> String {
    let text = m.text().replace('\n', " ");
    match (media_label(m), text.is_empty()) {
        (Some(label), true) => label.to_owned(),
        (Some(label), false) => format!("{label} · {text}"),
        (None, true) if m.action().is_some() => l!("Сервісне повідомлення", "Service message").to_owned(),
        _ => text,
    }
}

/// Buttons under a message (inline keyboard or a regular bot keyboard).
pub(super) fn buttons(m: &Message) -> Vec<Vec<Button>> {
    use tl::enums::{InlineButtonType as T, KeyboardButton as K, KeyboardInlineButton as I, ReplyMarkup as R};
    let bot = m.via_bot_id().or_else(|| m.sender_id().filter(|p| p.kind() == grammers_client::session::types::PeerKind::User).map(|p| p.bare_id_unchecked())).unwrap_or(0);
    match m.reply_markup() {
        Some(R::ReplyInlineMarkup(k)) => k
            .rows
            .into_iter()
            .map(|tl::enums::KeyboardInlineButtonRow::Row(row)| {
                row.buttons
                    .into_iter()
                    .map(|I::Button(b)| Button {
                        text: b.text,
                        action: match b.r#type {
                            T::Callback(c) => ButtonAction::Callback(c.data),
                            T::Url(u) => ButtonAction::Url(u.url),
                            T::UrlAuth(u) => ButtonAction::Url(u.url),
                            T::WebView(u) => ButtonAction::WebApp { url: u.url, simple: false, bot },
                            T::Copy(c) => ButtonAction::Copy(c.copy_text),
                            _ => ButtonAction::Other,
                        },
                    })
                    .collect()
            })
            .collect(),
        Some(R::ReplyKeyboardMarkup(k)) => k
            .rows
            .into_iter()
            .map(|tl::enums::KeyboardButtonRow::Row(row)| {
                row.buttons
                    .into_iter()
                    .map(|K::Button(b)| Button {
                        action: match b.r#type {
                            tl::enums::ButtonType::Default => ButtonAction::Text,
                            tl::enums::ButtonType::SimpleWebView(w) => ButtonAction::WebApp { url: w.url, simple: true, bot },
                            _ => ButtonAction::Other,
                        },
                        text: b.text,
                    })
                    .collect()
            })
            .collect(),
        _ => Vec::new(),
    }
}

pub(super) fn forwarded(m: &Message, names: &Names) -> Option<String> {
    let tl::enums::MessageFwdHeader::Header(h) = m.forward_header()?;
    let name = h
        .from_name
        .clone()
        .or_else(|| h.from_id.as_ref().and_then(|p| names.lock().unwrap().get(&PeerId::from(p)).cloned()))
        .unwrap_or_else(|| l!("невідомого", "unknown").to_owned());
    Some(match h.post_author {
        Some(author) => format!("{name} ({author})"),
        None => name,
    })
}

pub(super) fn convert(m: &Message, names: &Names) -> Msg {
    let photo = matches!(m.media(), Some(Media::Photo(_)));
    // Polls, places and contacts are drawn as cards: no label needed in the text.
    let card = matches!(m.media(), Some(Media::Poll(_) | Media::Geo(_) | Media::GeoLive(_) | Media::Venue(_) | Media::Contact(_)));
    let text = match (media_label(m), m.text().is_empty()) {
        _ if photo || card => m.text().to_owned(),
        (Some(label), true) => label.to_owned(),
        (Some(label), false) => format!("{label}\n{}", m.text()),
        (None, true) if m.action().is_some() => l!("Сервісне повідомлення", "Service message").to_owned(),
        _ => m.text().to_owned(),
    };
    let sticker = match m.media() {
        Some(Media::Sticker(s)) => Some((s.document.id(), s.emoji().to_owned())),
        _ => None,
    };
    let media = media_info(m);
    let file = match m.media() {
        Some(Media::Document(d)) if media.is_none() => Some((
            d.name().filter(|n| !n.is_empty()).unwrap_or(l!("Файл", "File")).to_owned(),
            d.size().map(human_size).unwrap_or_default(),
        )),
        _ => None,
    };
    let text = if sticker.is_some() || file.is_some() || media.is_some() { m.text().to_owned() } else { text };
    let spans = match (m.fmt_entities(), text.strip_suffix(m.text())) {
        (Some(ents), Some(prefix)) if !m.text().is_empty() => format::from_entities(&text, ents, prefix.len()),
        _ => Vec::new(),
    };
    let msg = Msg {
        id: m.id(),
        peer: m.peer_id(),
        out: m.outgoing(),
        sender: m.sender().map(peer_title).unwrap_or_default(),
        text,
        time: clock(m.date()),
        ts: m.date().as_second(),
        day: day(m.date()),
        edited: m.edit_date().is_some(),
        reply_to: m.reply_to_message_id(),
        reply_quote: reply_header(m).filter(|h| h.quote).and_then(|h| h.quote_text.clone()).filter(|t| !t.is_empty()),
        // `reply_to_peer_id` is set only for replies to another chat.
        reply_elsewhere: reply_header(m).and_then(|h| h.reply_to_peer_id.as_ref()).map(|p| names.lock().unwrap().get(&PeerId::from(p)).cloned().unwrap_or_default()),
        photo,
        file,
        sticker,
        spans,
        forwarded: forwarded(m, names),
        buttons: buttons(m),
        group: m.grouped_id(),
        reactions: match &m.raw {
            tl::enums::Message::Message(x) => x.reactions.as_ref().map(reactions).unwrap_or_default(),
            _ => Vec::new(),
        },
        poll: match m.media() {
            Some(Media::Poll(p)) => Some(poll_info(&p)),
            _ => None,
        },
        geo: geo_info(m),
        contact: match m.media() {
            Some(Media::Contact(c)) => Some((format!("{} {}", c.first_name(), c.last_name()).trim().to_owned(), c.phone_number().to_owned())),
            _ => None,
        },
        media,
        video_sticker: matches!(m.media(), Some(Media::Sticker(_))) && raw_document(m).is_some_and(|d| d.mime_type == "video/webm"),
        lottie: matches!(m.media(), Some(Media::Sticker(_))) && raw_document(m).is_some_and(|d| d.mime_type == "application/x-tgsticker"),
        media_unread: matches!(&m.raw, tl::enums::Message::Message(x) if x.media_unread),
        // An unread mention is a mention whose contents are not read yet.
        mentioned: matches!(&m.raw, tl::enums::Message::Message(x) if x.mentioned && x.media_unread),
        reaction_unread: match &m.raw {
            tl::enums::Message::Message(x) => x.reactions.as_ref().is_some_and(reactions_unread),
            _ => false,
        },
        topic: message_topic(m),
        comments: match &m.raw {
            tl::enums::Message::Message(x) => match &x.replies {
                Some(tl::enums::MessageReplies::Replies(r)) if r.comments => Some(r.replies),
                _ => None,
            },
            _ => None,
        },
        pinned: m.pinned(),
        views: m.view_count(),
        noforwards: matches!(&m.raw, tl::enums::Message::Message(x) if x.noforwards),
        webpage: suggest::message_web_page(m),
        saved_from: saved::saved_from(&m.raw),
    };
    // Hidden by Telegram for everyone (or for desktop apps): show why instead of the content.
    match restriction(&m.raw) {
        Some(reason) => Msg { text: reason, spans: Vec::new(), photo: false, file: None, sticker: None, media: None, poll: None, geo: None, contact: None, webpage: None, buttons: Vec::new(), video_sticker: false, lottie: false, ..msg },
        None => msg,
    }
}

fn reply_header(m: &Message) -> Option<&tl::types::MessageReplyHeader> {
    match &m.raw {
        tl::enums::Message::Message(x) => match &x.reply_to {
            Some(tl::enums::MessageReplyHeader::Header(h)) => Some(h),
            _ => None,
        },
        _ => None,
    }
}

/// Why Telegram hides a message here.
fn restriction(raw: &tl::enums::Message) -> Option<String> {
    let tl::enums::Message::Message(x) = raw else { return None };
    reason_text(x.restriction_reason.as_ref()?, l!("Це повідомлення недоступне", "This message is unavailable"))
}

/// Reasons for "all" platforms (and desktop ones) apply, mobile-only ones don't.
fn reason_text(reasons: &[tl::enums::RestrictionReason], fallback: &str) -> Option<String> {
    reasons.iter().find_map(|tl::enums::RestrictionReason::Reason(r)| {
        (!matches!(r.platform.as_str(), "ios" | "android")).then(|| if r.text.is_empty() { fallback.to_owned() } else { r.text.clone() })
    })
}

/// Forum topic from the reply header; the message that created a topic belongs to that topic.
pub(super) fn message_topic(m: &Message) -> Option<i32> {
    match &m.raw {
        tl::enums::Message::Message(x) => match &x.reply_to {
            Some(tl::enums::MessageReplyHeader::Header(h)) if h.forum_topic => h.reply_to_top_id.or(h.reply_to_msg_id),
            _ => None,
        },
        tl::enums::Message::Service(x) if matches!(x.action, tl::enums::MessageAction::TopicCreate(_)) => Some(x.id),
        _ => None,
    }
}

/// A chat found outside the dialog list (by username or in search): not a member yet.
pub(super) fn found_chat(peer: &Peer, last_text: String) -> ChatInfo {
    ChatInfo {
        id: peer.id(),
        title: peer_title(peer),
        kind: peer_kind(peer),
        last_text,
        last_time: String::new(),
        last_ts: 0,
        unread: 0,
        pinned: false,
        contact: matches!(peer, Peer::User(u) if u.contact()),
        muted: false,
        archived: false,
        unread_mark: false,
        draft: String::new(),
        presence: match peer {
            Peer::User(u) if !u.is_bot() => Presence::from_status(u.status()),
            _ => Presence::None,
        },
        forum: matches!(peer, Peer::Group(g) if matches!(&g.raw, tl::enums::Chat::Channel(c) if c.forum)),
        // Users and bots don't need joining.
        joined: matches!(peer, Peer::User(_)),
        can_post: can_post(peer),
        read_out: 0,
        read_in: 0,
        unread_mentions: 0,
        unread_reactions: 0,
        noforwards: noforwards(peer),
        can_pin: can_pin(peer),
        restricted: chat_restriction(peer),
    }
}

/// Why Telegram hides a whole chat, bot or channel on desktop apps.
pub(super) fn chat_restriction(peer: &Peer) -> Option<String> {
    let reasons = match peer {
        Peer::User(u) => match &u.raw {
            tl::enums::User::User(x) if x.restricted => x.restriction_reason.as_deref(),
            _ => None,
        },
        Peer::Channel(c) if c.raw.restricted => c.raw.restriction_reason.as_deref(),
        Peer::Group(g) => match &g.raw {
            tl::enums::Chat::Channel(x) if x.restricted => x.restriction_reason.as_deref(),
            _ => None,
        },
        _ => None,
    };
    reason_text(reasons?, l!("Цей чат недоступний", "This chat is unavailable"))
}

/// Protected content of a group or channel.
pub(super) fn noforwards(peer: &Peer) -> bool {
    match peer {
        Peer::Channel(c) => c.raw.noforwards,
        Peer::Group(g) => match &g.raw {
            tl::enums::Chat::Chat(c) => c.noforwards,
            tl::enums::Chat::Channel(c) => c.noforwards,
            _ => false,
        },
        _ => false,
    }
}

/// Anyone can pin in private chats; in groups — admins with the right, or everyone unless banned.
pub(super) fn can_pin(peer: &Peer) -> bool {
    let admin = |r: &Option<tl::enums::ChatAdminRights>| matches!(r, Some(tl::enums::ChatAdminRights::Rights(r)) if r.pin_messages);
    let everyone = |r: &Option<tl::enums::ChatBannedRights>| matches!(r, Some(tl::enums::ChatBannedRights::Rights(r)) if !r.pin_messages);
    match peer {
        Peer::User(_) => true,
        Peer::Channel(c) => c.raw.creator || admin(&c.raw.admin_rights),
        Peer::Group(g) => match &g.raw {
            tl::enums::Chat::Chat(c) => c.creator || admin(&c.admin_rights) || everyone(&c.default_banned_rights),
            tl::enums::Chat::Channel(c) => c.creator || admin(&c.admin_rights) || everyone(&c.default_banned_rights),
            _ => false,
        },
        _ => false,
    }
}

pub(super) fn can_post(peer: &Peer) -> bool {
    match peer {
        Peer::Channel(c) => c.raw.creator || matches!(&c.raw.admin_rights, Some(tl::enums::ChatAdminRights::Rights(r)) if r.post_messages),
        _ => true,
    }
}

/// Media type of a document from its attributes.
pub(super) fn media_info(m: &Message) -> Option<MediaInfo> {
    let Some(Media::Document(d)) = m.media() else { return None };
    let doc = raw_document(m)?;
    let mut info = MediaInfo {
        kind: MediaKind::Video,
        duration: 0.0,
        size: (0, 0),
        title: d.name().unwrap_or_default().to_owned(),
        performer: String::new(),
        waveform: Vec::new(),
    };
    let mut kind = None;
    let animated = doc.attributes.iter().any(|a| matches!(a, tl::enums::DocumentAttribute::Animated));
    let has_video = doc.attributes.iter().any(|a| matches!(a, tl::enums::DocumentAttribute::Video(_)));
    for a in &doc.attributes {
        match a {
            tl::enums::DocumentAttribute::Video(v) => {
                info.duration = v.duration;
                info.size = (v.w, v.h);
                kind = Some(if animated { MediaKind::Gif } else if v.round_message { MediaKind::Round } else { MediaKind::Video });
            }
            // The server adds an audio attribute to videos with sound (round ones included): video wins.
            tl::enums::DocumentAttribute::Audio(a) if !has_video => {
                info.duration = a.duration as f64;
                info.title = a.title.clone().filter(|t| !t.is_empty()).unwrap_or(info.title);
                info.performer = a.performer.clone().unwrap_or_default();
                info.waveform = a.waveform.as_deref().map(unpack_waveform).unwrap_or_default();
                kind = Some(if a.voice { MediaKind::Voice } else { MediaKind::Audio });
            }
            _ => {}
        }
    }
    if kind.is_none() && doc.mime_type == "image/gif" {
        kind = Some(MediaKind::Gif);
    }
    info.kind = kind?;
    Some(info)
}

/// Telegram waveform: 5-bit values packed back to back.
pub(super) fn unpack_waveform(bytes: &[u8]) -> Vec<u8> {
    let n = bytes.len() * 8 / 5;
    (0..n)
        .map(|i| {
            let bit = i * 5;
            let (byte, shift) = (bit / 8, bit % 8);
            let lo = bytes[byte] as u16;
            let hi = bytes.get(byte + 1).copied().unwrap_or(0) as u16;
            (((hi << 8 | lo) >> shift) & 31) as u8
        })
        .collect()
}

/// The inverse of `unpack_waveform`: levels 0..31 into a 5-bit stream.
pub(super) fn pack_waveform(levels: &[u8]) -> Vec<u8> {
    let mut out = vec![0u8; (levels.len() * 5).div_ceil(8)];
    for (i, &v) in levels.iter().enumerate() {
        let (bit, v) = (i * 5, (v.min(31)) as u16);
        let (byte, shift) = (bit / 8, bit % 8);
        out[byte] |= (v << shift) as u8;
        if shift > 3 {
            out[byte + 1] |= (v >> (8 - shift)) as u8;
        }
    }
    out
}

/// Someone else's reaction we haven't seen (only our own messages get these).
pub(super) fn reactions_unread(r: &tl::enums::MessageReactions) -> bool {
    let tl::enums::MessageReactions::Reactions(r) = r;
    r.recent_reactions.iter().flatten().any(|tl::enums::MessagePeerReaction::Reaction(p)| p.unread && !p.my)
}

pub(super) fn reactions(r: &tl::enums::MessageReactions) -> Vec<Reaction> {
    let tl::enums::MessageReactions::Reactions(r) = r;
    r.results
        .iter()
        .filter_map(|tl::enums::ReactionCount::Count(c)| match &c.reaction {
            tl::enums::Reaction::Emoji(e) => Some(Reaction { emoji: e.emoticon.clone(), count: c.count, mine: c.chosen_order.is_some() }),
            _ => None,
        })
        .collect()
}

pub(super) fn poll_info(p: &grammers_client::media::Poll) -> PollInfo {
    let text = |t: &tl::enums::TextWithEntities| {
        let tl::enums::TextWithEntities::Entities(t) = t;
        t.text.clone()
    };
    let voters: Vec<&tl::types::PollAnswerVoters> = p.iter_voters_summary().map(|v| v.collect()).unwrap_or_default();
    let answers = p
        .iter_answers()
        .filter_map(|a| match a {
            tl::enums::PollAnswer::Answer(a) => Some(a),
            _ => None,
        })
        .map(|a| {
            let v = voters.iter().find(|v| v.option == a.option);
            PollOption {
                text: text(&a.text),
                option: a.option.clone(),
                voters: v.and_then(|v| v.voters).unwrap_or(0),
                chosen: v.is_some_and(|v| v.chosen),
                correct: v.is_some_and(|v| v.correct),
            }
        })
        .collect();
    PollInfo {
        question: text(p.question()),
        answers,
        total: p.total_voters().unwrap_or(0),
        closed: p.closed(),
        quiz: p.is_quiz(),
        multiple: p.raw.multiple_choice,
    }
}

pub(super) fn geo_info(m: &Message) -> Option<GeoInfo> {
    let point = |g: &grammers_client::media::Geo| (g.latitue(), g.longitude());
    let (lat, lon, title, address, live) = match m.media()? {
        Media::Geo(g) => {
            let (lat, lon) = point(&g);
            (lat, lon, String::new(), String::new(), false)
        }
        Media::GeoLive(g) => {
            let (lat, lon) = point(g.geo.as_ref()?);
            (lat, lon, String::new(), String::new(), true)
        }
        Media::Venue(v) => {
            let (lat, lon) = point(v.geo.as_ref()?);
            (lat, lon, v.title().to_owned(), v.address().to_owned(), false)
        }
        _ => return None,
    };
    Some(GeoInfo { lat, lon, title, address, live })
}

pub(super) fn local(ts: jiff::Timestamp) -> jiff::Zoned {
    ts.to_zoned(jiff::tz::TimeZone::system())
}

pub(super) fn clock(ts: jiff::Timestamp) -> String {
    local(ts).strftime("%H:%M").to_string()
}

pub(super) fn short_time(ts: jiff::Timestamp) -> String {
    let z = local(ts);
    let now = jiff::Zoned::now();
    if z.date() == now.date() {
        z.strftime("%H:%M").to_string()
    } else if z.year() == now.year() {
        z.strftime("%d.%m").to_string()
    } else {
        z.strftime("%d.%m.%y").to_string()
    }
}

/// Month in a date ("5 березня" / "5 Mar").
pub(super) fn month(m: i8) -> &'static str {
    match m {
        1 => l!("січня", "Jan"),
        2 => l!("лютого", "Feb"),
        3 => l!("березня", "Mar"),
        4 => l!("квітня", "Apr"),
        5 => l!("травня", "May"),
        6 => l!("червня", "Jun"),
        7 => l!("липня", "Jul"),
        8 => l!("серпня", "Aug"),
        9 => l!("вересня", "Sep"),
        10 => l!("жовтня", "Oct"),
        11 => l!("листопада", "Nov"),
        12 => l!("грудня", "Dec"),
        _ => l!("грудня", "Dec"),
    }
}

pub(super) fn day(ts: jiff::Timestamp) -> String {
    let z = local(ts);
    let now = jiff::Zoned::now();
    let base = format!("{} {}", z.day(), month(z.month()));
    if z.date() == now.date() {
        l!("Сьогодні", "Today").to_owned()
    } else if z.year() == now.year() {
        base
    } else {
        format!("{base} {}", z.year())
    }
}
