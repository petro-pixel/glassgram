//! Channel and supergroup statistics (`stats.getBroadcastStats`, `stats.getMegagroupStats`), as the
//! "Statistics" page in Telegram Desktop: overview numbers, graphs in Telegram's chart JSON, recent
//! posts and the most active members. The requests go to the channel's statistics DC.

use std::collections::HashMap;

use grammers_client::{Client, InvocationError, tl};
use serde_json::Value;

use super::{Cmd, Ev, Worker, local};
use crate::i18n::{l, lf, lp};

/// Recent posts whose text is looked up for the list (one `channels.getMessages`).
const RECENT_TEXTS: usize = 50;
/// Public shares listed on a post's page; the count covers the rest.
const FORWARDS: i32 = 30;

/// One number of the overview: the current value and the one for the previous period.
#[derive(Debug, Clone, PartialEq)]
pub struct StatTile {
    pub label: String,
    pub value: f64,
    /// `None` for a share (enabled notifications), which has no change to show.
    pub prev: Option<f64>,
    /// `value` is a percentage.
    pub percent: bool,
}

/// How the series of a chart are drawn, from the `types` of its columns.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum ChartKind {
    #[default]
    Line,
    /// Lines that hold their value until the next point.
    Step,
    /// Bars stacked on each other.
    Bars,
    /// Areas stacked to 100%.
    Percent,
}

/// One column of a chart besides `x`.
#[derive(Debug, Clone, PartialEq)]
pub struct Series {
    pub name: String,
    pub color: [u8; 3],
    /// Telegram's name for the colour ("BLUE" in "BLUE#3497ED"), used to pick a dark theme tone.
    pub tone: String,
    pub values: Vec<f64>,
}

/// A parsed `statsGraph`: Telegram's chart format (columns, types, names, colours).
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Chart {
    /// Milliseconds since the epoch, or plain numbers (hours of the day) when small.
    pub x: Vec<i64>,
    pub series: Vec<Series>,
    pub kind: ChartKind,
    /// Each line has its own scale (two y axes).
    pub y_scaled: bool,
}

impl Chart {
    /// `x` holds timestamps, not hours or other small numbers.
    pub fn x_is_time(&self) -> bool {
        self.x.iter().any(|&x| x > 100_000_000_000)
    }
}

#[derive(Debug, Clone, PartialEq)]
pub enum GraphState {
    Ready(Chart),
    /// Loaded on demand with `stats.loadAsyncGraph`.
    Pending(String),
    Failed(String),
}

#[derive(Debug, Clone)]
pub struct StatsGraph {
    pub title: String,
    pub state: GraphState,
}

/// A recent post (or story) with its counters.
#[derive(Debug, Clone, Default)]
pub struct PostStat {
    pub id: i32,
    pub story: bool,
    pub views: i32,
    pub forwards: i32,
    pub reactions: i32,
    /// The first line of the post; empty when it could not be fetched.
    pub text: String,
    pub date: String,
}

/// A member in a "top" list of a supergroup, with what they did worded for the row.
#[derive(Debug, Clone, Default)]
pub struct TopMember {
    pub user: i64,
    pub name: String,
    pub line: String,
}

#[derive(Debug, Clone, Default)]
pub struct ChannelStats {
    /// The DC that answered, for the lazy graphs; `None` — the home DC.
    pub dc: Option<i32>,
    /// "01.11 – 08.11.2026".
    pub period: String,
    pub tiles: Vec<StatTile>,
    pub graphs: Vec<StatsGraph>,
    pub recent: Vec<PostStat>,
    pub posters: Vec<TopMember>,
    pub admins: Vec<TopMember>,
    pub inviters: Vec<TopMember>,
}

/// A public chat that shared a post, or a story that reposted it.
#[derive(Debug, Clone, Default)]
pub struct PublicForward {
    /// Chat or author name; server content, not translated.
    pub title: String,
    pub views: Option<i32>,
    pub story: bool,
}

/// The page of one channel post: its graphs and public shares.
#[derive(Debug, Clone, Default)]
pub struct MessageStats {
    pub graphs: Vec<StatsGraph>,
    /// All public shares; `None` when the server did not say.
    pub forwards_count: Option<i32>,
    pub forwards: Vec<PublicForward>,
}

impl Worker {
    pub(super) async fn stats_cmd(&mut self, cmd: Cmd) -> Result<(), String> {
        let client = self.client()?;
        let out = self.out.clone();
        match cmd {
            Cmd::LoadStats { peer, megagroup, dc, dark } => {
                let r = self.peer_ref(peer)?;
                tokio::spawn(async move {
                    let res = if megagroup {
                        let req = tl::functions::stats::GetMegagroupStats { dark, channel: r.into() };
                        invoke_stats(&client, dc, &req).await.map(|(tl::enums::stats::MegagroupStats::Stats(s), dc)| megagroup_stats(s, dc))
                    } else {
                        let req = tl::functions::stats::GetBroadcastStats { dark, channel: r.into() };
                        invoke_stats(&client, dc, &req).await.map(|(tl::enums::stats::BroadcastStats::Stats(s), dc)| broadcast_stats(s, dc))
                    };
                    let stats = match res {
                        Ok(mut s) => {
                            post_texts(&client, r, &mut s.recent).await;
                            Ok(Box::new(s))
                        }
                        Err(e) => Err(lf!("Статистика: {e}", "Statistics: {e}")),
                    };
                    out.send(Ev::Stats { peer, stats });
                });
            }
            Cmd::LoadStatsGraph { peer, dc, msg, index, token } => {
                tokio::spawn(async move {
                    let req = tl::functions::stats::LoadAsyncGraph { token, x: None };
                    let graph = match invoke_stats(&client, dc, &req).await {
                        Ok((g, _)) => graph_state(g),
                        Err(e) => GraphState::Failed(e.to_string()),
                    };
                    out.send(Ev::StatsGraph { peer, msg, index, graph });
                });
            }
            Cmd::LoadMessageStats { peer, msg, dc, dark } => {
                let r = self.peer_ref(peer)?;
                tokio::spawn(async move {
                    let req = tl::functions::stats::GetMessageStats { dark, channel: r.into(), msg_id: msg };
                    let stats = match invoke_stats(&client, dc, &req).await {
                        Ok((tl::enums::stats::MessageStats::Stats(s), dc)) => {
                            let mut m = MessageStats {
                                graphs: graphs(vec![(l!("Перегляди", "Views"), s.views_graph), (l!("Реакції", "Reactions"), s.reactions_by_emotion_graph)]),
                                ..Default::default()
                            };
                            // Shares are a nice-to-have: the page still shows without them.
                            let req = tl::functions::stats::GetMessagePublicForwards { channel: r.into(), msg_id: msg, offset: String::new(), limit: FORWARDS };
                            if let Ok((tl::enums::stats::PublicForwards::Forwards(f), _)) = invoke_stats(&client, dc, &req).await {
                                m.forwards_count = Some(f.count);
                                m.forwards = public_forwards(f);
                            }
                            Ok(Box::new(m))
                        }
                        Err(e) => Err(lf!("Статистика: {e}", "Statistics: {e}")),
                    };
                    out.send(Ev::MessageStats { peer, msg, stats });
                });
            }
            _ => {}
        }
        Ok(())
    }
}

/// Sends a statistics request where the server keeps them: `full.stats_dc` when known, else the
/// home DC, following `STATS_MIGRATE_X`. Another DC needs our authorization copied there first.
/// Returns the answer and the DC that gave it.
async fn invoke_stats<R: tl::RemoteCall>(client: &Client, mut dc: Option<i32>, req: &R) -> Result<(R::Return, Option<i32>), InvocationError> {
    let mut imported = false;
    for _ in 0..4 {
        let res = match dc {
            Some(d) => client.invoke_in_dc(d, req).await,
            None => client.invoke(req).await,
        };
        match res {
            Ok(r) => return Ok((r, dc)),
            Err(InvocationError::Rpc(e)) if e.is("STATS_MIGRATE") && e.value.is_some() => dc = e.value.map(|v| v as i32),
            Err(InvocationError::Rpc(e)) if e.is("AUTH_KEY_UNREGISTERED") && !imported => {
                let Some(d) = dc else { return Err(InvocationError::Rpc(e)) };
                imported = true;
                let tl::enums::auth::ExportedAuthorization::Authorization(a) = client.invoke(&tl::functions::auth::ExportAuthorization { dc_id: d }).await?;
                client.invoke_in_dc(d, &tl::functions::auth::ImportAuthorization { id: a.id, bytes: a.bytes }).await?;
            }
            Err(e) => return Err(e),
        }
    }
    Err(InvocationError::Dropped("statistics: too many DC redirects"))
}

fn graph_state(g: tl::enums::StatsGraph) -> GraphState {
    match g {
        tl::enums::StatsGraph::Graph(g) => {
            let tl::enums::DataJson::Json(j) = g.json;
            match parse_chart(&j.data) {
                Ok(c) => GraphState::Ready(c),
                Err(e) => GraphState::Failed(e),
            }
        }
        tl::enums::StatsGraph::Async(a) => GraphState::Pending(a.token),
        tl::enums::StatsGraph::Error(e) => GraphState::Failed(e.error),
    }
}

/// Graphs in Telegram Desktop's order; ones the server has no data for are left out.
fn graphs(list: Vec<(&str, tl::enums::StatsGraph)>) -> Vec<StatsGraph> {
    list.into_iter()
        .map(|(title, g)| StatsGraph { title: title.to_owned(), state: graph_state(g) })
        .filter(|g| !matches!(g.state, GraphState::Failed(_)))
        .collect()
}

fn period(p: &tl::enums::StatsDateRangeDays) -> String {
    let tl::enums::StatsDateRangeDays::Days(p) = p;
    let day = |t: i32| local(jiff::Timestamp::from_second(t as i64).unwrap_or_default());
    format!("{} – {}", day(p.min_date).strftime("%d.%m"), day(p.max_date).strftime("%d.%m.%Y"))
}

fn abs(label: &str, v: &tl::enums::StatsAbsValueAndPrev) -> StatTile {
    let tl::enums::StatsAbsValueAndPrev::Prev(v) = v;
    StatTile { label: label.to_owned(), value: v.current, prev: Some(v.previous), percent: false }
}

fn broadcast_stats(s: tl::types::stats::BroadcastStats, dc: Option<i32>) -> ChannelStats {
    let tl::enums::StatsPercentValue::Value(n) = &s.enabled_notifications;
    let share = if n.total > 0.0 { n.part / n.total * 100.0 } else { 0.0 };
    let mut tiles = vec![
        abs(l!("Підписники", "Followers"), &s.followers),
        StatTile { label: l!("Увімкнені сповіщення", "Enabled notifications").to_owned(), value: share, prev: None, percent: true },
        abs(l!("Переглядів на допис", "Views per post"), &s.views_per_post),
        abs(l!("Поширень на допис", "Shares per post"), &s.shares_per_post),
        abs(l!("Реакцій на допис", "Reactions per post"), &s.reactions_per_post),
        abs(l!("Переглядів на історію", "Views per story"), &s.views_per_story),
        abs(l!("Поширень на історію", "Shares per story"), &s.shares_per_story),
        abs(l!("Реакцій на історію", "Reactions per story"), &s.reactions_per_story),
    ];
    // Telegram Desktop hides what never happened (no stories, no reactions).
    tiles.retain(|t| t.percent || t.value != 0.0 || t.prev.is_some_and(|p| p != 0.0));
    let recent = s
        .recent_posts_interactions
        .iter()
        .map(|p| match p {
            tl::enums::PostInteractionCounters::Message(m) => PostStat { id: m.msg_id, views: m.views, forwards: m.forwards, reactions: m.reactions, ..Default::default() },
            tl::enums::PostInteractionCounters::Story(m) => PostStat { id: m.story_id, story: true, views: m.views, forwards: m.forwards, reactions: m.reactions, ..Default::default() },
        })
        .collect();
    ChannelStats {
        dc,
        period: period(&s.period),
        tiles,
        graphs: graphs(vec![
            (l!("Зростання", "Growth"), s.growth_graph),
            (l!("Підписники", "Followers"), s.followers_graph),
            (l!("Сповіщення", "Notifications"), s.mute_graph),
            (l!("Перегляди за годинами (UTC)", "Views by hours (UTC)"), s.top_hours_graph),
            (l!("Перегляди за джерелами", "Views by source"), s.views_by_source_graph),
            (l!("Нові підписники за джерелами", "New followers by source"), s.new_followers_by_source_graph),
            (l!("Мови", "Languages"), s.languages_graph),
            (l!("Взаємодії", "Interactions"), s.interactions_graph),
            (l!("Взаємодії з миттєвим переглядом", "Instant View interactions"), s.iv_interactions_graph),
            (l!("Реакції", "Reactions"), s.reactions_by_emotion_graph),
            (l!("Взаємодії з історіями", "Story interactions"), s.story_interactions_graph),
            (l!("Реакції на історії", "Story reactions"), s.story_reactions_by_emotion_graph),
        ]),
        recent,
        ..Default::default()
    }
}

fn megagroup_stats(s: tl::types::stats::MegagroupStats, dc: Option<i32>) -> ChannelStats {
    let names: HashMap<i64, String> = s
        .users
        .iter()
        .filter_map(|u| match u {
            tl::enums::User::User(u) => Some((u.id, [u.first_name.clone(), u.last_name.clone()].into_iter().flatten().collect::<Vec<_>>().join(" "))),
            _ => None,
        })
        .collect();
    let member = |user: i64, line: String| TopMember { user, name: names.get(&user).cloned().unwrap_or_else(|| l!("Видалений акаунт", "Deleted Account").to_owned()), line };
    let posters = s
        .top_posters
        .iter()
        .map(|tl::enums::StatsGroupTopPoster::Poster(p)| {
            let (n, avg) = (p.messages, p.avg_chars);
            let msgs = lp!(n, ["{n} повідомлення", "{n} повідомлення", "{n} повідомлень"], ["{n} message", "{n} messages"]);
            let chars = lp!(avg, ["{n} символ у середньому", "{n} символи у середньому", "{n} символів у середньому"], ["{n} symbol on average", "{n} symbols on average"]);
            member(p.user_id, format!("{msgs}, {chars}"))
        })
        .collect();
    let admins = s
        .top_admins
        .iter()
        .map(|tl::enums::StatsGroupTopAdmin::Admin(a)| {
            let mut parts = Vec::new();
            let (del, kick, ban) = (a.deleted, a.kicked, a.banned);
            if del > 0 {
                parts.push(lp!(del, ["{n} видалення", "{n} видалення", "{n} видалень"], ["{n} deletion", "{n} deletions"]));
            }
            if kick > 0 {
                parts.push(lp!(kick, ["{n} вилучення", "{n} вилучення", "{n} вилучень"], ["{n} kick", "{n} kicks"]));
            }
            if ban > 0 {
                parts.push(lp!(ban, ["{n} обмеження", "{n} обмеження", "{n} обмежень"], ["{n} restriction", "{n} restrictions"]));
            }
            member(a.user_id, parts.join(", "))
        })
        .collect();
    let inviters = s
        .top_inviters
        .iter()
        .map(|tl::enums::StatsGroupTopInviter::Inviter(i)| {
            let n = i.invitations;
            member(i.user_id, lp!(n, ["{n} запрошення", "{n} запрошення", "{n} запрошень"], ["{n} invitation", "{n} invitations"]))
        })
        .collect();
    ChannelStats {
        dc,
        period: period(&s.period),
        tiles: vec![
            abs(l!("Учасники", "Members"), &s.members),
            abs(l!("Повідомлення", "Messages"), &s.messages),
            abs(l!("Учасники, що переглядали", "Viewing members"), &s.viewers),
            abs(l!("Учасники, що писали", "Posting members"), &s.posters),
        ],
        graphs: graphs(vec![
            (l!("Зростання", "Growth"), s.growth_graph),
            (l!("Учасники групи", "Group members"), s.members_graph),
            (l!("Нові учасники за джерелами", "New members by source"), s.new_members_by_source_graph),
            (l!("Основна мова учасників", "Members' primary language"), s.languages_graph),
            (l!("Повідомлення", "Messages"), s.messages_graph),
            (l!("Дії", "Actions"), s.actions_graph),
            (l!("Активність за годинами (UTC)", "Activity by hours (UTC)"), s.top_hours_graph),
            (l!("Активність за днями тижня", "Activity by days of week"), s.weekdays_graph),
        ]),
        posters,
        admins,
        inviters,
        ..Default::default()
    }
}

/// Names the chats (or story authors) that shared a post publicly.
fn public_forwards(f: tl::types::stats::PublicForwards) -> Vec<PublicForward> {
    let mut names: HashMap<i64, String> = HashMap::new();
    for c in &f.chats {
        match c {
            tl::enums::Chat::Channel(c) => names.insert(c.id, c.title.clone()),
            tl::enums::Chat::Chat(c) => names.insert(c.id, c.title.clone()),
            _ => None,
        };
    }
    for u in &f.users {
        if let tl::enums::User::User(u) = u {
            names.insert(u.id, [u.first_name.clone(), u.last_name.clone()].into_iter().flatten().collect::<Vec<_>>().join(" "));
        }
    }
    let name_of = |p: &tl::enums::Peer| {
        let id = match p {
            tl::enums::Peer::Channel(c) => c.channel_id,
            tl::enums::Peer::Chat(c) => c.chat_id,
            tl::enums::Peer::User(u) => u.user_id,
        };
        names.get(&id).cloned().unwrap_or_else(|| l!("Приватний чат", "Private chat").to_owned())
    };
    f.forwards
        .iter()
        .filter_map(|p| match p {
            tl::enums::PublicForward::Message(m) => match &m.message {
                tl::enums::Message::Message(m) => Some(PublicForward { title: name_of(&m.peer_id), views: m.views, story: false }),
                _ => None,
            },
            tl::enums::PublicForward::Story(s) => {
                let views = match &s.story {
                    tl::enums::StoryItem::Item(i) => i.views.as_ref().map(|tl::enums::StoryViews::Views(v)| v.views_count),
                    _ => None,
                };
                Some(PublicForward { title: name_of(&s.peer), views, story: true })
            }
        })
        .collect()
}

/// Fills in the first line and date of recent posts; without them the rows show only numbers.
async fn post_texts(client: &Client, r: grammers_client::session::types::PeerRef, recent: &mut [PostStat]) {
    let id: Vec<tl::enums::InputMessage> =
        recent.iter().filter(|p| !p.story).take(RECENT_TEXTS).map(|p| tl::enums::InputMessage::Id(tl::types::InputMessageId { id: p.id })).collect();
    if id.is_empty() {
        return;
    }
    let Ok(raw) = client.invoke(&tl::functions::channels::GetMessages { channel: r.into(), id }).await else { return };
    let list = match raw {
        tl::enums::messages::Messages::Messages(m) => m.messages,
        tl::enums::messages::Messages::Slice(m) => m.messages,
        tl::enums::messages::Messages::ChannelMessages(m) => m.messages,
        tl::enums::messages::Messages::NotModified(_) => Vec::new(),
    };
    for m in list {
        let tl::enums::Message::Message(m) = m else { continue };
        let Some(p) = recent.iter_mut().find(|p| !p.story && p.id == m.id) else { continue };
        let line = m.message.lines().find(|l| !l.trim().is_empty()).unwrap_or_default();
        p.text = if line.is_empty() && m.media.is_some() { l!("Медіа", "Media").to_owned() } else { line.chars().take(120).collect() };
        p.date = local(jiff::Timestamp::from_second(m.date as i64).unwrap_or_default()).strftime("%d.%m %H:%M").to_string();
    }
}

/// "#3497ED" or Telegram's "BLUE#3497ED" → (tone, rgb).
fn parse_color(s: &str) -> Option<(String, [u8; 3])> {
    let (tone, hex) = s.rsplit_once('#')?;
    if hex.len() != 6 || !hex.is_ascii() {
        return None;
    }
    let byte = |i: usize| u8::from_str_radix(&hex[i..i + 2], 16).ok();
    Some((tone.to_owned(), [byte(0)?, byte(2)?, byte(4)?]))
}

/// Colours for series that come without one, Telegram's blue, green, red, yellow, violet.
const FALLBACK: [[u8; 3]; 5] = [[0x34, 0x97, 0xED], [0x3C, 0xC2, 0x3F], [0xF3, 0x4C, 0x44], [0xF5, 0xBD, 0x25], [0x9E, 0x66, 0xE0]];

/// Parses the JSON of a `statsGraph`: `columns` (each a label followed by numbers, `x` first or
/// anywhere), `types` (`x`, `line`, `step`, `bar`, `area`), `names`, `colors`, and the `stacked`,
/// `percentage` and `y_scaled` flags. Unknown columns are skipped; a chart needs `x` and one series.
pub fn parse_chart(json: &str) -> Result<Chart, String> {
    let bad = || l!("Некоректні дані графіка", "Invalid chart data").to_owned();
    let v: Value = serde_json::from_str(json).map_err(|_| bad())?;
    let columns = v.get("columns").and_then(Value::as_array).ok_or_else(bad)?;
    let str_of = |map: &str, key: &str| v.get(map).and_then(|m| m.get(key)).and_then(Value::as_str).map(str::to_owned);
    let flag = |key: &str| v.get(key).and_then(Value::as_bool).unwrap_or(false);
    let mut x = None;
    let mut series = Vec::new();
    let mut kinds = Vec::new();
    for col in columns {
        let Some(col) = col.as_array() else { continue };
        let Some((key, values)) = col.split_first() else { continue };
        let Some(key) = key.as_str() else { continue };
        let values: Vec<f64> = values.iter().map(|n| n.as_f64().unwrap_or(0.0)).collect();
        let kind = str_of("types", key).unwrap_or_else(|| if key == "x" { "x".into() } else { "line".into() });
        if kind == "x" {
            x = Some(values.iter().map(|&n| n as i64).collect::<Vec<i64>>());
            continue;
        }
        if !matches!(kind.as_str(), "line" | "step" | "bar" | "area") {
            continue;
        }
        let (tone, color) = str_of("colors", key).as_deref().and_then(parse_color).unwrap_or_else(|| (String::new(), FALLBACK[series.len() % FALLBACK.len()]));
        series.push(Series { name: str_of("names", key).unwrap_or_else(|| key.to_owned()), color, tone, values });
        kinds.push(kind);
    }
    let mut x = x.ok_or_else(bad)?;
    if x.is_empty() || series.is_empty() {
        return Err(l!("Недостатньо даних", "Not enough data").to_owned());
    }
    // A short column cuts the chart rather than inventing points.
    let n = series.iter().map(|s| s.values.len()).min().unwrap_or(0).min(x.len());
    x.truncate(n);
    for s in &mut series {
        s.values.truncate(n);
    }
    let has = |k: &str| kinds.iter().any(|t| t == k);
    let kind = if has("area") || flag("percentage") {
        ChartKind::Percent
    } else if has("bar") {
        ChartKind::Bars
    } else if has("step") {
        ChartKind::Step
    } else {
        ChartKind::Line
    };
    Ok(Chart { x, series, kind, y_scaled: flag("y_scaled") && kind == ChartKind::Line })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lines_with_names_and_colours() {
        let json = r##"{"columns":[["x",1730419200000,1730505600000,1730592000000],["y0",120,135,150],["y1",4,2,7]],
            "types":{"x":"x","y0":"line","y1":"line"},"names":{"y0":"Joined","y1":"Left"},
            "colors":{"y0":"GREEN#3CC23F","y1":"#F34C44"},"hidden":[]}"##;
        let c = parse_chart(json).expect("chart");
        assert_eq!(c.kind, ChartKind::Line);
        assert_eq!(c.x.len(), 3);
        assert!(c.x_is_time());
        assert_eq!(c.series.len(), 2);
        assert_eq!(c.series[0].name, "Joined");
        assert_eq!(c.series[0].tone, "GREEN");
        assert_eq!(c.series[0].color, [0x3C, 0xC2, 0x3F]);
        assert_eq!(c.series[1].tone, "");
        assert_eq!(c.series[1].values, vec![4.0, 2.0, 7.0]);
    }

    #[test]
    fn stacked_bars_and_percent_areas() {
        let bars = r##"{"columns":[["x",1,2],["y0",1,2],["y1",3,4]],"types":{"x":"x","y0":"bar","y1":"bar"},"stacked":true}"##;
        assert_eq!(parse_chart(bars).expect("bars").kind, ChartKind::Bars);
        let area = r##"{"columns":[["x",1,2],["y0",60,55],["y1",40,45]],"types":{"x":"x","y0":"area","y1":"area"},"percentage":true}"##;
        let c = parse_chart(area).expect("area");
        assert_eq!(c.kind, ChartKind::Percent);
        assert!(!c.x_is_time());
        let step = r##"{"columns":[["x",1,2],["y0",1,1]],"types":{"x":"x","y0":"step"}}"##;
        assert_eq!(parse_chart(step).expect("step").kind, ChartKind::Step);
    }

    #[test]
    fn y_scaled_only_for_lines() {
        let json = r##"{"columns":[["x",1,2],["y0",1,2],["y1",300,400]],"types":{"x":"x","y0":"line","y1":"line"},"y_scaled":true}"##;
        assert!(parse_chart(json).expect("chart").y_scaled);
    }

    #[test]
    fn short_columns_cut_and_defaults() {
        // No types or colours: lines with fallback colours, keys as names; floats and nulls accepted.
        let json = r##"{"columns":[["x",1,2,3,4],["y0",1.5,null,3],["y1",1,2,3,4]]}"##;
        let c = parse_chart(json).expect("chart");
        assert_eq!(c.x, vec![1, 2, 3]);
        assert_eq!(c.series[0].values, vec![1.5, 0.0, 3.0]);
        assert_eq!(c.series[0].name, "y0");
        assert_eq!(c.series[0].color, FALLBACK[0]);
        assert_eq!(c.series[1].color, FALLBACK[1]);
    }

    #[test]
    fn broken_input_is_an_error() {
        for json in ["", "[]", "{}", r#"{"columns":5}"#, r#"{"columns":[["y0",1,2]]}"#, r#"{"columns":[["x"]]}"#, r#"{"columns":[["x",1],["y0","a"]],"types":{"y0":"pie"}}"#] {
            assert!(parse_chart(json).is_err(), "{json}");
        }
    }

    #[test]
    fn colours() {
        assert_eq!(parse_color("BLUE#3497ED"), Some(("BLUE".into(), [0x34, 0x97, 0xED])));
        assert_eq!(parse_color("#fff"), None);
        assert_eq!(parse_color("3497ED"), None);
        assert_eq!(parse_color("#ZZ97ED"), None);
    }
}
