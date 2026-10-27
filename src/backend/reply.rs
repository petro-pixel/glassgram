//! Replies grammers can't express: quoting a part of the message and replying to a message in
//! another chat. Such messages go as our own `sendMessage` / `sendMedia` / `sendMultiMedia`;
//! plain replies keep the grammers path.

use grammers_client::InvocationError;

use super::*;

impl Worker {
    /// The reply as the API wants it; the other chat must be one we know.
    pub(super) fn input_reply(&self, reply: &ReplyTo) -> Result<tl::enums::InputReplyTo, String> {
        let reply_to_peer_id = match reply.peer {
            Some(p) => Some(self.peer_ref(p)?.into()),
            None => None,
        };
        let quote = reply.quote.as_ref().map(|q| {
            // The quote's own formatting, offsets counted from the quote start.
            let (_, ents) = format::parse(&format::to_markup(&q.text, &q.spans));
            (q.text.clone(), (!ents.is_empty()).then_some(ents), q.offset)
        });
        let (quote_text, quote_entities, quote_offset) = match quote {
            Some((t, e, o)) => (Some(t), e, Some(o)),
            None => (None, None, None),
        };
        Ok(tl::types::InputReplyToMessage {
            reply_to_msg_id: reply.id,
            top_msg_id: None,
            reply_to_peer_id,
            quote_text,
            quote_entities,
            quote_offset,
            monoforum_peer_id: None,
            todo_item_id: None,
            poll_option: None,
        }
        .into())
    }

    /// `input_reply` for the requests that need it, `None` when grammers can send the reply itself.
    pub(super) fn extended_reply(&self, reply: &Option<ReplyTo>) -> Result<Option<tl::enums::InputReplyTo>, String> {
        reply.as_ref().filter(|r| r.extended()).map(|r| self.input_reply(r)).transpose()
    }
}

fn random_id() -> i64 {
    jiff::Timestamp::now().as_nanosecond() as i64
}

/// Why our own send request failed: the server's answer (some are worth retrying) or something else.
pub(super) enum SendError {
    Rpc(InvocationError),
    Other(String),
}

impl SendError {
    /// Worth another attempt from the send queue.
    pub(super) fn retry(&self) -> bool {
        matches!(self, SendError::Rpc(e) if outbox::retryable(e))
    }

    /// An earlier attempt with the same `random_id` got through.
    pub(super) fn duplicate(&self) -> bool {
        matches!(self, SendError::Rpc(InvocationError::Rpc(e)) if e.name == "RANDOM_ID_DUPLICATE")
    }
}

impl From<InvocationError> for SendError {
    fn from(e: InvocationError) -> Self {
        SendError::Rpc(e)
    }
}

impl std::fmt::Display for SendError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            SendError::Rpc(e) => e.fmt(f),
            SendError::Other(s) => f.write_str(s),
        }
    }
}

/// The messages a request created, in the order of `random_ids`; scheduled ones are not fetched.
async fn created(client: &Client, r: PeerRef, updates: &tl::enums::Updates, random_ids: &[i64], opts: SendOpts) -> Result<Vec<Message>, SendError> {
    if opts.schedule.is_some() {
        return Ok(Vec::new());
    }
    let ids: Vec<i32> = random_ids.iter().filter_map(|rid| outbox::sent_id(updates, *rid)).collect();
    let msgs = client.get_messages_by_id(r, &ids).await?;
    Ok(msgs.into_iter().flatten().collect())
}

/// One message with an extended reply: text (markup) and maybe media.
pub(super) async fn send_one(client: &Client, r: PeerRef, markup: &str, media: Option<tl::enums::InputMedia>, reply: tl::enums::InputReplyTo, opts: SendOpts) -> Result<Option<Message>, String> {
    send_raw(client, r, markup, media, Some(reply), opts, random_id()).await.map_err(|e| e.to_string())
}

/// One message as our own request, with the `random_id` given (the send queue reuses its key).
pub(super) async fn send_raw(client: &Client, r: PeerRef, markup: &str, media: Option<tl::enums::InputMedia>, reply: Option<tl::enums::InputReplyTo>, opts: SendOpts, rid: i64) -> Result<Option<Message>, SendError> {
    let (message, ents) = format::parse(markup);
    let entities = (!ents.is_empty()).then_some(ents);
    let schedule_date = opts.schedule.map(|t| t as i32);
    let updates = match media {
        Some(media) => {
            client
                .invoke(&tl::functions::messages::SendMedia {
                    silent: opts.silent,
                    background: false,
                    clear_draft: false,
                    noforwards: false,
                    update_stickersets_order: false,
                    invert_media: false,
                    allow_paid_floodskip: false,
                    peer: r.into(),
                    reply_to: reply,
                    media,
                    message,
                    random_id: rid,
                    reply_markup: None,
                    entities,
                    schedule_date,
                    schedule_repeat_period: None,
                    send_as: None,
                    quick_reply_shortcut: None,
                    effect: None,
                    allow_paid_stars: None,
                    suggested_post: None,
                })
                .await
        }
        None => {
            client
                .invoke(&tl::functions::messages::SendMessage {
                    no_webpage: opts.no_webpage,
                    silent: opts.silent,
                    background: false,
                    clear_draft: true,
                    noforwards: false,
                    update_stickersets_order: false,
                    invert_media: false,
                    allow_paid_floodskip: false,
                    peer: r.into(),
                    reply_to: reply,
                    message,
                    random_id: rid,
                    reply_markup: None,
                    entities,
                    schedule_date,
                    schedule_repeat_period: None,
                    send_as: None,
                    quick_reply_shortcut: None,
                    effect: None,
                    allow_paid_stars: None,
                    suggested_post: None,
                    rich_message: None,
                })
                .await
        }
    }?;
    Ok(created(client, r, &updates, &[rid], opts).await?.into_iter().next())
}

/// An album as our own request; photo `i` gets the `random_id` `rid + i`.
pub(super) async fn album_raw(client: &Client, r: PeerRef, ups: Vec<Uploaded>, caption: &str, reply: Option<tl::enums::InputReplyTo>, opts: SendOpts, rid: i64) -> Result<Vec<Message>, SendError> {
    let mut multi_media = Vec::new();
    let mut rids = Vec::new();
    for (i, up) in ups.into_iter().enumerate() {
        // An album takes only media already on the server.
        let stored = client.invoke(&tl::functions::messages::UploadMedia { business_connection_id: None, peer: r.into(), media: photo(up) }).await?;
        let tl::enums::MessageMedia::Photo(tl::types::MessageMediaPhoto { photo: Some(tl::enums::Photo::Photo(p)), .. }) = stored else {
            return Err(SendError::Other(l!("Сервер не прийняв фото", "The server did not accept the photo").into()));
        };
        let media = tl::types::InputMediaPhoto {
            spoiler: false,
            live_photo: false,
            id: tl::types::InputPhoto { id: p.id, access_hash: p.access_hash, file_reference: p.file_reference }.into(),
            ttl_seconds: None,
            video: None,
        };
        let (message, ents) = if i == 0 { format::parse(caption) } else { (String::new(), Vec::new()) };
        let rid = rid + i as i64;
        rids.push(rid);
        multi_media.push(tl::types::InputSingleMedia { media: media.into(), random_id: rid, message, entities: (!ents.is_empty()).then_some(ents) }.into());
    }
    let updates = client
        .invoke(&tl::functions::messages::SendMultiMedia {
            silent: opts.silent,
            background: false,
            clear_draft: false,
            noforwards: false,
            update_stickersets_order: false,
            invert_media: false,
            allow_paid_floodskip: false,
            peer: r.into(),
            reply_to: reply,
            multi_media,
            schedule_date: opts.schedule.map(|t| t as i32),
            send_as: None,
            quick_reply_shortcut: None,
            effect: None,
            allow_paid_stars: None,
        })
        .await?;
    created(client, r, &updates, &rids, opts).await
}

pub(super) fn photo(up: Uploaded) -> tl::enums::InputMedia {
    tl::types::InputMediaUploadedPhoto { spoiler: false, live_photo: false, file: up.raw, stickers: None, ttl_seconds: None, video: None }.into()
}

/// A document under `name`, its type guessed from the extension like grammers does.
pub(super) fn document(up: Uploaded, name: &str, mime: Option<&str>, mut attributes: Vec<tl::enums::DocumentAttribute>) -> tl::enums::InputMedia {
    let mime_type = mime.map(str::to_owned).unwrap_or_else(|| mime_guess::from_path(name).first_or_octet_stream().to_string());
    attributes.push(tl::types::DocumentAttributeFilename { file_name: name.to_owned() }.into());
    tl::types::InputMediaUploadedDocument {
        nosound_video: false,
        force_file: false,
        spoiler: false,
        file: up.raw,
        thumb: None,
        mime_type,
        attributes,
        stickers: None,
        video_cover: None,
        video_timestamp: None,
        ttl_seconds: None,
    }
    .into()
}
