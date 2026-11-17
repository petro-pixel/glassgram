//! Telegram Business: our location and opening hours (`account.updateBusinessLocation` /
//! `updateBusinessWorkHours`), greeting and away messages, quick replies (`messages.*QuickReply*`),
//! chat links (`account.*BusinessChatLink*`) and connected bots (`account.getConnectedBots`);
//! and another user's address and hours for their profile.

use super::*;

/// One week in minutes: intervals start in `0..WEEK` (Monday 00:00 is 0) and may end in the next week.
pub const WEEK: i32 = 7 * 24 * 60;

/// Opening hours: intervals in the business's own time zone.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct WorkHours {
    /// Time zone id from `help.getTimezonesList` ("Europe/Kyiv").
    pub tz: String,
    /// The server's word on it right now; used when the zone is unknown.
    pub open_now: bool,
    /// (start, end) minutes from Monday 00:00, sorted.
    pub weekly: Vec<(i32, i32)>,
}

impl WorkHours {
    fn from_raw(raw: &tl::enums::BusinessWorkHours) -> Self {
        let tl::enums::BusinessWorkHours::Hours(h) = raw;
        let mut weekly: Vec<(i32, i32)> = h.weekly_open.iter().map(|tl::enums::BusinessWeeklyOpen::Open(o)| (o.start_minute, o.end_minute)).collect();
        weekly.sort();
        Self { tz: h.timezone_id.clone(), open_now: h.open_now, weekly }
    }

    fn raw(&self) -> tl::enums::BusinessWorkHours {
        let weekly_open = self.weekly.iter().map(|&(start_minute, end_minute)| tl::types::BusinessWeeklyOpen { start_minute, end_minute }.into()).collect();
        tl::types::BusinessWorkHours { open_now: false, timezone_id: self.tz.clone(), weekly_open }.into()
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct Timezone {
    pub id: String,
    /// "Kyiv (UTC+2)", as the server names it.
    pub name: String,
    /// Seconds east of UTC.
    pub offset: i32,
}

/// Another user's business card for their profile.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct BusinessInfo {
    pub address: String,
    pub hours: Option<WorkHours>,
    /// UTC offset of the hours' time zone in seconds, when the list knows it.
    pub offset: Option<i32>,
}

/// Who an automatic message goes to.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Recipients {
    pub existing: bool,
    pub new: bool,
    pub contacts: bool,
    pub non_contacts: bool,
    /// The types and users above are exceptions ("all chats except…") rather than the only ones.
    pub exclude_selected: bool,
    /// Chosen users and their names; kept as they are when saving.
    pub users: Vec<(PeerId, String)>,
}

impl Recipients {
    fn from_raw(raw: &tl::enums::BusinessRecipients, names: &HashMap<i64, String>) -> Self {
        let tl::enums::BusinessRecipients::Recipients(r) = raw;
        let users = r.users.iter().flatten().map(|id| (PeerId::user_unchecked(*id), names.get(id).cloned().unwrap_or_default())).collect();
        Self { existing: r.existing_chats, new: r.new_chats, contacts: r.contacts, non_contacts: r.non_contacts, exclude_selected: r.exclude_selected, users }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct Greeting {
    pub shortcut: i32,
    pub recipients: Recipients,
    /// Sent again after this many days without messages (7, 14, 21 or 28).
    pub days: i32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AwaySchedule {
    Always,
    OutsideHours,
    /// From and to, Unix seconds.
    Custom(i32, i32),
}

#[derive(Debug, Clone, PartialEq)]
pub struct Away {
    pub shortcut: i32,
    /// Only when we are offline.
    pub offline_only: bool,
    pub schedule: AwaySchedule,
    pub recipients: Recipients,
}

/// Our own business settings, with the time zones to pick from.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct MyBusiness {
    pub address: String,
    pub hours: Option<WorkHours>,
    pub greeting: Option<Greeting>,
    pub away: Option<Away>,
    pub timezones: Vec<Timezone>,
}

/// A quick reply shortcut ("/hello").
#[derive(Debug, Clone, PartialEq)]
pub struct QuickReply {
    pub id: i32,
    pub name: String,
    /// Text of its first message, for the list.
    pub top: String,
    pub count: i32,
}

/// One message of a shortcut: id and text with markup.
#[derive(Debug, Clone, PartialEq)]
pub struct QuickMsg {
    pub id: i32,
    pub text: String,
    pub media: bool,
}

#[derive(Debug, Clone, PartialEq)]
pub struct ChatLink {
    /// https://t.me/m/…; the slug is its last part.
    pub link: String,
    pub title: String,
    /// The suggested first message, with markup.
    pub message: String,
    pub views: i32,
}

impl ChatLink {
    pub fn slug(&self) -> &str {
        self.link.rsplit('/').next().unwrap_or_default()
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct ConnectedBot {
    pub id: PeerId,
    pub name: String,
    pub username: String,
    /// May answer for us, not only read.
    pub can_reply: bool,
}

#[derive(Debug)]
pub enum BusinessCmd {
    /// Our business settings and the time zones → `BusinessEv::Loaded`.
    Load,
    /// Empty removes the location.
    SetLocation(String),
    SetHours(Option<WorkHours>),
    SetGreeting(Option<Greeting>),
    SetAway(Option<Away>),
    LoadQuickReplies,
    /// Messages of one shortcut → `BusinessEv::QuickMessages`.
    LoadQuickMessages(i32),
    /// Adds a text message to a shortcut by its name, creating the shortcut if there is none.
    AddQuickMessage { shortcut: String, text: String },
    EditQuickMessage { shortcut: i32, id: i32, text: String },
    DeleteQuickMessage { shortcut: i32, id: i32 },
    RenameQuickReply { shortcut: i32, name: String },
    DeleteQuickReply(i32),
    /// Sends all messages of a shortcut to a chat.
    SendQuickReply { peer: PeerId, shortcut: i32 },
    LoadLinks,
    /// Creates a link, or edits the one with this slug.
    SaveLink { slug: Option<String>, title: String, message: String },
    DeleteLink(String),
    LoadBots,
    /// Connects a bot by username: it may reply in all 1-to-1 chats except contacts.
    ConnectBot(String),
    DisconnectBot(PeerId),
}

#[derive(Debug)]
pub enum BusinessEv {
    Loaded(MyBusiness),
    QuickReplies(Vec<QuickReply>),
    QuickMessages { shortcut: i32, msgs: Vec<QuickMsg> },
    Links(Vec<ChatLink>),
    Bots(Vec<ConnectedBot>),
}

/// The server's time zone list, asked once per run.
static TIMEZONES: Mutex<Vec<Timezone>> = Mutex::new(Vec::new());

async fn timezones(client: &Client) -> Vec<Timezone> {
    let known = TIMEZONES.lock().unwrap().clone();
    if !known.is_empty() {
        return known;
    }
    let Ok(tl::enums::help::TimezonesList::List(l)) = client.invoke(&tl::functions::help::GetTimezonesList { hash: 0 }).await else { return Vec::new() };
    let list: Vec<Timezone> = l.timezones.into_iter().map(|tl::enums::Timezone::Timezone(t)| Timezone { id: t.id, name: t.name, offset: t.utc_offset }).collect();
    *TIMEZONES.lock().unwrap() = list.clone();
    list
}

/// A user's address and hours from their full info, for the profile.
pub(super) async fn info_of(client: &Client, u: &tl::types::UserFull) -> Option<BusinessInfo> {
    let address = match &u.business_location {
        Some(tl::enums::BusinessLocation::Location(l)) => l.address.clone(),
        None => String::new(),
    };
    let hours = u.business_work_hours.as_ref().map(WorkHours::from_raw);
    if address.is_empty() && hours.is_none() {
        return None;
    }
    let offset = match &hours {
        Some(h) => timezones(client).await.into_iter().find(|t| t.id == h.tz).map(|t| t.offset),
        None => None,
    };
    Some(BusinessInfo { address, hours, offset })
}

fn input_recipients(r: &Recipients, refs: &HashMap<PeerId, PeerRef>) -> tl::enums::InputBusinessRecipients {
    let users: Vec<tl::enums::InputUser> = r.users.iter().filter_map(|(id, _)| refs.get(id)).map(|x| (*x).into()).collect();
    tl::types::InputBusinessRecipients {
        existing_chats: r.existing,
        new_chats: r.new,
        contacts: r.contacts,
        non_contacts: r.non_contacts,
        exclude_selected: r.exclude_selected,
        users: (!users.is_empty()).then_some(users),
    }
    .into()
}

fn away_schedule(s: AwaySchedule) -> tl::enums::BusinessAwayMessageSchedule {
    match s {
        AwaySchedule::Always => tl::enums::BusinessAwayMessageSchedule::Always,
        AwaySchedule::OutsideHours => tl::enums::BusinessAwayMessageSchedule::OutsideWorkHours,
        AwaySchedule::Custom(start_date, end_date) => tl::types::BusinessAwayMessageScheduleCustom { start_date, end_date }.into(),
    }
}

/// Text with markup of a raw message, and whether it has media.
fn quick_text(m: &tl::enums::Message) -> Option<QuickMsg> {
    let tl::enums::Message::Message(m) = m else { return None };
    let spans = format::from_entities(&m.message, m.entities.as_deref().unwrap_or_default(), 0);
    Some(QuickMsg { id: m.id, text: format::to_markup(&m.message, &spans), media: m.media.is_some() })
}

impl Worker {
    pub(super) async fn business_cmd(&mut self, cmd: BusinessCmd) -> Result<(), String> {
        let client = self.client()?;
        let saved = |e: &dyn std::fmt::Display| lf!("Не збережено: {e}", "Not saved: {e}");
        match cmd {
            BusinessCmd::Load => self.load_business(&client).await?,
            BusinessCmd::SetLocation(address) => {
                let address = address.trim().to_owned();
                let req = tl::functions::account::UpdateBusinessLocation { geo_point: None, address: (!address.is_empty()).then_some(address) };
                client.invoke(&req).await.map_err(|e| saved(&e))?;
                self.load_business(&client).await?;
            }
            BusinessCmd::SetHours(h) => {
                client.invoke(&tl::functions::account::UpdateBusinessWorkHours { business_work_hours: h.as_ref().map(WorkHours::raw) }).await.map_err(|e| saved(&e))?;
                self.load_business(&client).await?;
            }
            BusinessCmd::SetGreeting(g) => {
                let refs = self.refs.lock().unwrap().clone();
                let message = g.map(|g| tl::types::InputBusinessGreetingMessage { shortcut_id: g.shortcut, recipients: input_recipients(&g.recipients, &refs), no_activity_days: g.days }.into());
                client.invoke(&tl::functions::account::UpdateBusinessGreetingMessage { message }).await.map_err(|e| saved(&e))?;
                self.load_business(&client).await?;
            }
            BusinessCmd::SetAway(a) => {
                let refs = self.refs.lock().unwrap().clone();
                let message = a.map(|a| {
                    tl::types::InputBusinessAwayMessage { offline_only: a.offline_only, shortcut_id: a.shortcut, schedule: away_schedule(a.schedule), recipients: input_recipients(&a.recipients, &refs) }.into()
                });
                client.invoke(&tl::functions::account::UpdateBusinessAwayMessage { message }).await.map_err(|e| saved(&e))?;
                self.load_business(&client).await?;
            }
            BusinessCmd::LoadQuickReplies => {
                self.load_quick_replies(&client).await?;
            }
            BusinessCmd::LoadQuickMessages(shortcut) => self.load_quick_messages(&client, shortcut).await?,
            BusinessCmd::AddQuickMessage { shortcut, text } => {
                let (message, entities) = format::parse(&text);
                let req = tl::functions::messages::SendMessage {
                    no_webpage: false,
                    silent: false,
                    background: false,
                    clear_draft: false,
                    noforwards: false,
                    update_stickersets_order: false,
                    invert_media: false,
                    allow_paid_floodskip: false,
                    peer: tl::enums::InputPeer::PeerSelf,
                    reply_to: None,
                    message,
                    random_id: jiff::Timestamp::now().as_nanosecond() as i64,
                    reply_markup: None,
                    entities: (!entities.is_empty()).then_some(entities),
                    schedule_date: None,
                    schedule_repeat_period: None,
                    send_as: None,
                    quick_reply_shortcut: Some(tl::types::InputQuickReplyShortcut { shortcut: shortcut.clone() }.into()),
                    effect: None,
                    allow_paid_stars: None,
                    suggested_post: None,
                    rich_message: None,
                };
                client.invoke(&req).await.map_err(|e| saved(&e))?;
                let list = self.load_quick_replies(&client).await?;
                if let Some(q) = list.iter().find(|q| q.name == shortcut) {
                    self.load_quick_messages(&client, q.id).await?;
                }
            }
            BusinessCmd::EditQuickMessage { shortcut, id, text } => {
                let (message, entities) = format::parse(&text);
                let req = tl::functions::messages::EditMessage {
                    no_webpage: false,
                    invert_media: false,
                    peer: tl::enums::InputPeer::PeerSelf,
                    id,
                    message: Some(message),
                    media: None,
                    reply_markup: None,
                    entities: Some(entities),
                    schedule_date: None,
                    schedule_repeat_period: None,
                    quick_reply_shortcut_id: Some(shortcut),
                    rich_message: None,
                };
                client.invoke(&req).await.map_err(|e| saved(&e))?;
                self.load_quick_replies(&client).await?;
                self.load_quick_messages(&client, shortcut).await?;
            }
            BusinessCmd::DeleteQuickMessage { shortcut, id } => {
                client.invoke(&tl::functions::messages::DeleteQuickReplyMessages { shortcut_id: shortcut, id: vec![id] }).await.map_err(|e| lf!("Не видалено: {e}", "Not deleted: {e}"))?;
                self.load_quick_replies(&client).await?;
                self.load_quick_messages(&client, shortcut).await?;
            }
            BusinessCmd::RenameQuickReply { shortcut, name } => {
                client.invoke(&tl::functions::messages::EditQuickReplyShortcut { shortcut_id: shortcut, shortcut: name }).await.map_err(|e| saved(&e))?;
                self.load_quick_replies(&client).await?;
            }
            BusinessCmd::DeleteQuickReply(shortcut) => {
                client.invoke(&tl::functions::messages::DeleteQuickReplyShortcut { shortcut_id: shortcut }).await.map_err(|e| lf!("Не видалено: {e}", "Not deleted: {e}"))?;
                self.load_quick_replies(&client).await?;
            }
            BusinessCmd::SendQuickReply { peer, shortcut } => {
                let r = self.peer_ref(peer)?;
                let ids: Vec<i32> = self.quick_messages(&client, shortcut).await?.into_iter().map(|m| m.id).collect();
                if ids.is_empty() {
                    return Ok(());
                }
                let base = jiff::Timestamp::now().as_nanosecond() as i64;
                let random_id = (0..ids.len() as i64).map(|k| base + k).collect();
                let n = ids.len();
                client
                    .invoke(&tl::functions::messages::SendQuickReplyMessages { peer: r.into(), shortcut_id: shortcut, id: ids, random_id })
                    .await
                    .map_err(|e| lf!("Не надіслано: {e}", "Not sent: {e}"))?;
                self.emit_latest(peer, n).await;
            }
            BusinessCmd::LoadLinks => self.load_links(&client).await?,
            BusinessCmd::SaveLink { slug, title, message } => {
                let (message, entities) = format::parse(&message);
                let title = title.trim().to_owned();
                let link: tl::enums::InputBusinessChatLink =
                    tl::types::InputBusinessChatLink { message, entities: (!entities.is_empty()).then_some(entities), title: (!title.is_empty()).then_some(title) }.into();
                match slug {
                    Some(slug) => client.invoke(&tl::functions::account::EditBusinessChatLink { slug, link }).await.map(|_| ()),
                    None => client.invoke(&tl::functions::account::CreateBusinessChatLink { link }).await.map(|_| ()),
                }
                .map_err(|e| saved(&e))?;
                self.load_links(&client).await?;
            }
            BusinessCmd::DeleteLink(slug) => {
                client.invoke(&tl::functions::account::DeleteBusinessChatLink { slug }).await.map_err(|e| lf!("Не видалено: {e}", "Not deleted: {e}"))?;
                self.load_links(&client).await?;
            }
            BusinessCmd::LoadBots => self.load_bots(&client).await?,
            BusinessCmd::ConnectBot(name) => {
                let bot = self.resolve_peer(&name).await?;
                let Peer::User(u) = &bot else { return Err(l!("Це не бот", "This is not a bot").into()) };
                if !u.is_bot() {
                    return Err(l!("Це не бот", "This is not a bot").into());
                }
                let r = self.peer_ref(bot.id())?;
                let rights = tl::types::BusinessBotRights {
                    reply: true,
                    read_messages: true,
                    delete_sent_messages: false,
                    delete_received_messages: false,
                    edit_name: false,
                    edit_bio: false,
                    edit_profile_photo: false,
                    edit_username: false,
                    view_gifts: false,
                    sell_gifts: false,
                    change_gift_settings: false,
                    transfer_and_upgrade_gifts: false,
                    transfer_stars: false,
                    manage_stories: false,
                };
                let recipients = tl::types::InputBusinessBotRecipients {
                    existing_chats: true,
                    new_chats: true,
                    contacts: false,
                    non_contacts: true,
                    exclude_selected: false,
                    users: None,
                    exclude_users: None,
                };
                let req = tl::functions::account::UpdateConnectedBot { deleted: false, rights: Some(rights.into()), bot: r.into(), recipients: recipients.into() };
                client.invoke(&req).await.map_err(|e| lf!("Бота не підключено: {e}", "Bot not connected: {e}"))?;
                self.load_bots(&client).await?;
            }
            BusinessCmd::DisconnectBot(id) => {
                let r = self.peer_ref(id)?;
                let recipients = tl::types::InputBusinessBotRecipients {
                    existing_chats: false,
                    new_chats: false,
                    contacts: false,
                    non_contacts: false,
                    exclude_selected: false,
                    users: None,
                    exclude_users: None,
                };
                let req = tl::functions::account::UpdateConnectedBot { deleted: true, rights: None, bot: r.into(), recipients: recipients.into() };
                client.invoke(&req).await.map_err(|e| lf!("Бота не відключено: {e}", "Bot not disconnected: {e}"))?;
                self.load_bots(&client).await?;
            }
        }
        Ok(())
    }

    async fn load_business(&self, client: &Client) -> Result<(), String> {
        let req = tl::functions::users::GetFullUser { id: tl::enums::InputUser::UserSelf };
        let tl::enums::users::UserFull::Full(f) = client.invoke(&req).await.map_err(|e| lf!("Telegram Business: {e}", "Telegram Business: {e}"))?;
        let tl::enums::UserFull::Full(u) = f.full_user;
        // The chosen users of greeting and away messages: names for the page, refs for saving.
        let mut names = HashMap::new();
        for raw in f.users {
            let peer = Peer::User(User::from_raw(client, raw));
            if self.remember(&peer).await {
                names.insert(peer.id().bare_id_unchecked(), peer_title(&peer));
            }
        }
        let address = match &u.business_location {
            Some(tl::enums::BusinessLocation::Location(l)) => l.address.clone(),
            None => String::new(),
        };
        let greeting = u.business_greeting_message.as_ref().map(|tl::enums::BusinessGreetingMessage::Message(g)| Greeting {
            shortcut: g.shortcut_id,
            recipients: Recipients::from_raw(&g.recipients, &names),
            days: g.no_activity_days,
        });
        let away = u.business_away_message.as_ref().map(|tl::enums::BusinessAwayMessage::Message(a)| Away {
            shortcut: a.shortcut_id,
            offline_only: a.offline_only,
            schedule: match &a.schedule {
                tl::enums::BusinessAwayMessageSchedule::Always => AwaySchedule::Always,
                tl::enums::BusinessAwayMessageSchedule::OutsideWorkHours => AwaySchedule::OutsideHours,
                tl::enums::BusinessAwayMessageSchedule::Custom(c) => AwaySchedule::Custom(c.start_date, c.end_date),
            },
            recipients: Recipients::from_raw(&a.recipients, &names),
        });
        let hours = u.business_work_hours.as_ref().map(WorkHours::from_raw);
        let timezones = timezones(client).await;
        self.out.send(Ev::Business(BusinessEv::Loaded(MyBusiness { address, hours, greeting, away, timezones })));
        Ok(())
    }

    async fn load_quick_replies(&self, client: &Client) -> Result<Vec<QuickReply>, String> {
        let res = client.invoke(&tl::functions::messages::GetQuickReplies { hash: 0 }).await.map_err(|e| lf!("Швидкі відповіді: {e}", "Quick replies: {e}"))?;
        let list = match res {
            tl::enums::messages::QuickReplies::Replies(r) => {
                let tops: Vec<QuickMsg> = r.messages.iter().filter_map(quick_text).collect();
                r.quick_replies
                    .into_iter()
                    .map(|tl::enums::QuickReply::Reply(q)| {
                        let top = tops.iter().find(|m| m.id == q.top_message).map(|m| if m.text.is_empty() && m.media { l!("📎 Вкладення", "📎 Attachment").to_owned() } else { m.text.clone() });
                        QuickReply { id: q.shortcut_id, name: q.shortcut, top: top.unwrap_or_default(), count: q.count }
                    })
                    .collect()
            }
            tl::enums::messages::QuickReplies::NotModified => Vec::new(),
        };
        self.out.send(Ev::Business(BusinessEv::QuickReplies(list.clone())));
        Ok(list)
    }

    async fn quick_messages(&self, client: &Client, shortcut: i32) -> Result<Vec<QuickMsg>, String> {
        use tl::enums::messages::Messages as M;
        let res = client
            .invoke(&tl::functions::messages::GetQuickReplyMessages { shortcut_id: shortcut, id: None, hash: 0 })
            .await
            .map_err(|e| lf!("Швидкі відповіді: {e}", "Quick replies: {e}"))?;
        let raw = match res {
            M::Messages(m) => m.messages,
            M::Slice(m) => m.messages,
            M::ChannelMessages(m) => m.messages,
            M::NotModified(_) => Vec::new(),
        };
        let mut msgs: Vec<QuickMsg> = raw.iter().filter_map(quick_text).collect();
        msgs.sort_by_key(|m| m.id);
        Ok(msgs)
    }

    async fn load_quick_messages(&self, client: &Client, shortcut: i32) -> Result<(), String> {
        let msgs = self.quick_messages(client, shortcut).await?;
        self.out.send(Ev::Business(BusinessEv::QuickMessages { shortcut, msgs }));
        Ok(())
    }

    async fn load_links(&self, client: &Client) -> Result<(), String> {
        let tl::enums::account::BusinessChatLinks::Links(res) =
            client.invoke(&tl::functions::account::GetBusinessChatLinks {}).await.map_err(|e| lf!("Посилання на чат: {e}", "Chat links: {e}"))?;
        let list = res
            .links
            .into_iter()
            .map(|tl::enums::BusinessChatLink::Link(l)| {
                let spans = format::from_entities(&l.message, l.entities.as_deref().unwrap_or_default(), 0);
                ChatLink { message: format::to_markup(&l.message, &spans), link: l.link, title: l.title.unwrap_or_default(), views: l.views }
            })
            .collect();
        self.out.send(Ev::Business(BusinessEv::Links(list)));
        Ok(())
    }

    async fn load_bots(&self, client: &Client) -> Result<(), String> {
        let tl::enums::account::ConnectedBots::Bots(res) =
            client.invoke(&tl::functions::account::GetConnectedBots {}).await.map_err(|e| lf!("Чат-боти: {e}", "Chatbots: {e}"))?;
        let mut users = HashMap::new();
        for raw in res.users {
            let peer = Peer::User(User::from_raw(client, raw));
            if self.remember(&peer).await {
                users.insert(peer.id().bare_id_unchecked(), peer);
            }
        }
        let list = res
            .connected_bots
            .into_iter()
            .map(|tl::enums::ConnectedBot::Bot(b)| {
                let peer = users.get(&b.bot_id);
                let tl::enums::BusinessBotRights::Rights(rights) = &b.rights;
                ConnectedBot {
                    id: PeerId::user_unchecked(b.bot_id),
                    name: peer.map(peer_title).unwrap_or_default(),
                    username: peer.and_then(|p| p.username()).unwrap_or_default().to_owned(),
                    can_reply: rights.reply,
                }
            })
            .collect();
        self.out.send(Ev::Business(BusinessEv::Bots(list)));
        Ok(())
    }
}
