//! Demo statistics: "Tech News" as a growing channel and "Rust Team 🦀" as a lively supergroup.
//! The graphs go through the real JSON parser, one of them lazily, as the server sends them.

use serde_json::{Value, json};

use crate::app::App;
use crate::backend::{ChannelStats, Cmd, Ev, GraphState, MessageStats, PostStat, PublicForward, StatTile, StatsGraph, TopMember, parse_chart};
use crate::i18n::{l, lp};

/// 16.10.2026 00:00 UTC, the first day of the period, in milliseconds.
const START: i64 = 1_792_108_800_000;
const DAY: i64 = 86_400_000;
const DAYS: usize = 31;
/// Token of the lazy graph.
const ASYNC: &str = "demo-story-interactions";
/// A post's first two days, hour by hour.
const HOURS: usize = 48;

/// A smooth wobble in 0..1 that looks like real traffic, the same on every run.
fn wave(k: usize, seed: f64) -> f64 {
    let t = k as f64;
    0.5 + 0.28 * (t * 0.9 + seed).sin() + 0.14 * (t * 2.3 + seed * 1.7).sin() + 0.08 * (t * 0.21 + seed).cos()
}

/// Weekends are quieter on a tech channel.
fn weekday(k: usize) -> f64 {
    // 16.10.2026 is a Friday.
    if matches!((k + 4) % 7, 5 | 6) { 0.7 } else { 1.0 }
}

fn days() -> Vec<Value> {
    (0..DAYS).map(|k| json!(START + k as i64 * DAY)).collect()
}

/// Builds the server's chart JSON: `series` are (key, type, name, colour, values).
fn chart_json(x: Vec<Value>, series: &[(&str, &str, &str, &str, Vec<f64>)], extra: Value) -> String {
    let mut columns = vec![Value::Array([vec![json!("x")], x].concat())];
    let (mut types, mut names, mut colors) = (json!({"x": "x"}), json!({}), json!({}));
    for (key, kind, name, color, values) in series {
        columns.push(Value::Array([vec![json!(key)], values.iter().map(|v| json!(v.round())).collect()].concat()));
        types[key] = json!(kind);
        names[key] = json!(name);
        colors[key] = json!(color);
    }
    let mut v = json!({"columns": columns, "types": types, "names": names, "colors": colors});
    if let (Some(v), Some(extra)) = (v.as_object_mut(), extra.as_object()) {
        v.extend(extra.clone());
    }
    v.to_string()
}

fn graph(title: &str, json: String) -> StatsGraph {
    let state = match parse_chart(&json) {
        Ok(c) => GraphState::Ready(c),
        Err(e) => GraphState::Failed(e),
    };
    StatsGraph { title: title.to_owned(), state }
}

fn series(f: impl Fn(usize) -> f64) -> Vec<f64> {
    (0..DAYS).map(f).collect()
}

fn tile(label: &str, value: f64, prev: f64) -> StatTile {
    StatTile { label: label.to_owned(), value, prev: Some(prev), percent: false }
}

fn channel() -> ChannelStats {
    let joined = series(|k| (38.0 + 30.0 * wave(k, 0.4)) * weekday(k));
    let left = series(|k| 6.0 + 9.0 * wave(k, 2.1));
    let mut total = 12_100.0;
    let growth: Vec<f64> = (0..DAYS)
        .map(|k| {
            total += joined[k] - left[k];
            total
        })
        .collect();
    let views = series(|k| (9_800.0 + 5_200.0 * wave(k, 1.2)) * weekday(k));
    let shares = series(|k| (120.0 + 140.0 * wave(k, 3.3)) * weekday(k));
    let hours: Vec<Value> = (0..24).map(|h| json!(h)).collect();
    let by_hour = |seed: f64| (0..24).map(|h| {
        let h = h as f64;
        // Morning coffee and evening peaks.
        900.0 + 2_600.0 * (-(h - 9.0).powi(2) / 8.0).exp() + 3_400.0 * (-(h - 20.0).powi(2) / 10.0).exp() + 200.0 * (h * 1.3 + seed).sin()
    }).collect::<Vec<f64>>();
    let source = |share: f64, seed: f64| series(|k| views[k] * share * (0.8 + 0.4 * wave(k, seed)));
    let reactions = |base: f64, seed: f64| series(move |k| (base + base * wave(k, seed)) * weekday(k));
    let graphs = vec![
        graph(l!("Зростання", "Growth"), chart_json(days(), &[("y0", "line", l!("Підписники", "Followers"), "BLUE#3497ED", growth)], json!({}))),
        graph(
            l!("Підписники", "Followers"),
            chart_json(days(), &[("y0", "line", l!("Приєдналися", "Joined"), "GREEN#3CC23F", joined.clone()), ("y1", "line", l!("Вийшли", "Left"), "RED#F34C44", left.clone())], json!({})),
        ),
        graph(
            l!("Сповіщення", "Notifications"),
            chart_json(days(), &[("y0", "line", l!("Вимкнули", "Muted"), "RED#F34C44", series(|k| 4.0 + 6.0 * wave(k, 0.9))), ("y1", "line", l!("Увімкнули", "Unmuted"), "GREEN#3CC23F", series(|k| 2.0 + 4.0 * wave(k, 4.0)))], json!({})),
        ),
        graph(
            l!("Перегляди за годинами (UTC)", "Views by hours (UTC)"),
            chart_json(hours, &[("y0", "line", l!("Цей тиждень", "This week"), "BLUE#3497ED", by_hour(0.0)), ("y1", "line", l!("Минулий тиждень", "Last week"), "LIGHTBLUE#58A8ED", by_hour(1.5).iter().map(|v| v * 0.86).collect())], json!({})),
        ),
        graph(
            l!("Перегляди за джерелами", "Views by source"),
            chart_json(
                days(),
                &[
                    ("y0", "bar", l!("Підписники", "Followers"), "BLUE#3497ED", source(0.62, 0.3)),
                    ("y1", "bar", l!("Інші канали", "Other channels"), "GREEN#3CC23F", source(0.18, 1.3)),
                    ("y2", "bar", l!("Пошук", "Search"), "GOLDEN#F5BD25", source(0.08, 2.3)),
                    ("y3", "bar", l!("Посилання", "URL"), "VIOLET#9E66E0", source(0.07, 3.3)),
                    ("y4", "bar", l!("Інше", "Other"), "LIGHTBLUE#58A8ED", source(0.05, 4.3)),
                ],
                json!({"stacked": true}),
            ),
        ),
        graph(
            l!("Мови", "Languages"),
            chart_json(
                days(),
                &[
                    ("y0", "area", l!("Українська", "Ukrainian"), "BLUE#3497ED", series(|k| 48.0 + 4.0 * wave(k, 0.2))),
                    ("y1", "area", l!("Англійська", "English"), "GREEN#3CC23F", series(|k| 31.0 + 3.0 * wave(k, 1.2))),
                    ("y2", "area", l!("Польська", "Polish"), "GOLDEN#F5BD25", series(|k| 11.0 + 2.0 * wave(k, 2.2))),
                    ("y3", "area", l!("Інші", "Others"), "VIOLET#9E66E0", series(|k| 10.0 + 2.0 * wave(k, 3.2))),
                ],
                json!({"percentage": true, "stacked": true}),
            ),
        ),
        graph(
            l!("Взаємодії", "Interactions"),
            chart_json(days(), &[("y0", "line", l!("Перегляди", "Views"), "BLUE#3497ED", views.clone()), ("y1", "line", l!("Поширення", "Shares"), "GOLDEN#F5BD25", shares)], json!({"y_scaled": true})),
        ),
        graph(
            l!("Реакції", "Reactions"),
            chart_json(
                days(),
                &[
                    ("y0", "bar", "👍", "BLUE#3497ED", reactions(70.0, 0.5)),
                    ("y1", "bar", "❤", "RED#F34C44", reactions(45.0, 1.5)),
                    ("y2", "bar", "🔥", "GOLDEN#F5BD25", reactions(30.0, 2.5)),
                    ("y3", "bar", "🤔", "VIOLET#9E66E0", reactions(8.0, 3.5)),
                ],
                json!({"stacked": true}),
            ),
        ),
        StatsGraph { title: l!("Взаємодії з історіями", "Story interactions").into(), state: GraphState::Pending(ASYNC.into()) },
    ];
    let post = |id: i32, views: i32, forwards: i32, reactions: i32, text: &str, date: &str| PostStat { id, story: false, views, forwards, reactions, text: text.into(), date: date.into() };
    ChannelStats {
        dc: Some(2),
        period: "16.10 – 15.11.2026".into(),
        tiles: vec![
            tile(l!("Підписники", "Followers"), 12_840.0, 12_116.0),
            StatTile { label: l!("Увімкнені сповіщення", "Enabled notifications").into(), value: 37.4, prev: None, percent: true },
            tile(l!("Переглядів на допис", "Views per post"), 8_412.0, 7_905.0),
            tile(l!("Поширень на допис", "Shares per post"), 164.0, 181.0),
            tile(l!("Реакцій на допис", "Reactions per post"), 212.0, 188.0),
            tile(l!("Переглядів на історію", "Views per story"), 2_930.0, 2_930.0),
        ],
        graphs,
        recent: vec![
            post(3, 9_870, 212, 340, "Europe agrees on one charging port for laptops.", "15.11 09:30"),
            post(2, 11_420, 388, 512, "Linux 7.3 is out: a new scheduler and faster file systems.", "14.11 18:05"),
            post(1, 7_960, 141, 233, "📷 The new Wi-Fi standard promises half the latency.", "13.11 12:00"),
            PostStat { id: 7, story: true, views: 3_120, forwards: 18, reactions: 96, ..Default::default() },
            post(0, 6_410, 97, 158, "Rust 1.95: faster builds and new const fn powers", "11.11 10:15"),
        ],
        ..Default::default()
    }
}

fn group() -> ChannelStats {
    let joined = series(|k| 3.0 + 6.0 * wave(k, 0.7));
    let left = series(|k| 0.5 + 2.0 * wave(k, 2.7));
    let mut total = 1_180.0;
    let members: Vec<f64> = (0..DAYS)
        .map(|k| {
            total += joined[k] - left[k];
            total
        })
        .collect();
    let graphs = vec![
        graph(l!("Зростання", "Growth"), chart_json(days(), &[("y0", "line", l!("Учасники", "Members"), "BLUE#3497ED", members)], json!({}))),
        graph(
            l!("Учасники групи", "Group members"),
            chart_json(days(), &[("y0", "line", l!("Приєдналися", "Joined"), "GREEN#3CC23F", joined), ("y1", "line", l!("Вийшли", "Left"), "RED#F34C44", left)], json!({})),
        ),
        graph(
            l!("Повідомлення", "Messages"),
            chart_json(
                days(),
                &[
                    ("y0", "bar", l!("Текст", "Text"), "BLUE#3497ED", series(|k| (260.0 + 220.0 * wave(k, 0.1)) * weekday(k))),
                    ("y1", "bar", l!("Медіа", "Media"), "GREEN#3CC23F", series(|k| (60.0 + 50.0 * wave(k, 1.1)) * weekday(k))),
                    ("y2", "bar", l!("Стікери", "Stickers"), "GOLDEN#F5BD25", series(|k| (40.0 + 40.0 * wave(k, 2.1)) * weekday(k))),
                ],
                json!({"stacked": true}),
            ),
        ),
        graph(
            l!("Дії", "Actions"),
            chart_json(days(), &[("y0", "step", l!("Повідомлення", "Messages"), "BLUE#3497ED", series(|k| 300.0 + 240.0 * wave(k, 0.6))), ("y1", "step", l!("Переглядали", "Viewers"), "VIOLET#9E66E0", series(|k| 520.0 + 160.0 * wave(k, 1.9)))], json!({})),
        ),
        graph(
            l!("Активність за годинами (UTC)", "Activity by hours (UTC)"),
            chart_json((0..24).map(|h| json!(h)).collect(), &[("y0", "bar", l!("Повідомлення", "Messages"), "BLUE#3497ED", (0..24).map(|h| {
                let h = h as f64;
                40.0 + 380.0 * (-(h - 13.0).powi(2) / 18.0).exp() + 240.0 * (-(h - 21.0).powi(2) / 6.0).exp()
            }).collect())], json!({})),
        ),
    ];
    let member = |user: i64, name: &str, line: String| TopMember { user, name: name.into(), line };
    let posts = |n: i32, avg: i32| {
        format!(
            "{}, {}",
            lp!(n, ["{n} повідомлення", "{n} повідомлення", "{n} повідомлень"], ["{n} message", "{n} messages"]),
            lp!(avg, ["{n} символ у середньому", "{n} символи у середньому", "{n} символів у середньому"], ["{n} symbol on average", "{n} symbols on average"])
        )
    };
    ChannelStats {
        dc: Some(2),
        period: "16.10 – 15.11.2026".into(),
        tiles: vec![
            tile(l!("Учасники", "Members"), 1_284.0, 1_180.0),
            tile(l!("Повідомлення", "Messages"), 12_930.0, 13_802.0),
            tile(l!("Учасники, що переглядали", "Viewing members"), 1_021.0, 964.0),
            tile(l!("Учасники, що писали", "Posting members"), 214.0, 199.0),
        ],
        graphs,
        posters: vec![
            member(101, "Taras Bondar", posts(1_284, 64)),
            member(102, "Iryna Moroz", posts(962, 88)),
            member(1, "Olena Koval", posts(611, 120)),
            member(103, "Sofia Lysenko", posts(254, 41)),
        ],
        admins: vec![
            member(1, "Olena Koval", format!("{}, {}", lp!(41, ["{n} видалення", "{n} видалення", "{n} видалень"], ["{n} deletion", "{n} deletions"]), lp!(3, ["{n} обмеження", "{n} обмеження", "{n} обмежень"], ["{n} restriction", "{n} restrictions"]))),
            member(5, "Andriy", lp!(17, ["{n} видалення", "{n} видалення", "{n} видалень"], ["{n} deletion", "{n} deletions"])),
        ],
        inviters: vec![member(8, "Maria Shevchenko", lp!(22, ["{n} запрошення", "{n} запрошення", "{n} запрошень"], ["{n} invitation", "{n} invitations"]))],
        ..Default::default()
    }
}

/// A post's page: views by the hour (a fresh post peaks, then fades), reactions sent lazily.
fn message(msg: i32) -> MessageStats {
    let seed = msg as f64;
    let posted = START + (DAYS as i64 - 2) * DAY + 9 * 3_600_000;
    let x: Vec<Value> = (0..HOURS).map(|h| json!(posted + h as i64 * 3_600_000)).collect();
    let views: Vec<f64> = (0..HOURS).map(|h| 2_400.0 * (-(h as f64) / 7.0).exp() + 60.0 + 40.0 * wave(h, seed)).collect();
    let shares: Vec<f64> = views.iter().enumerate().map(|(h, v)| v * 0.02 + 2.0 * wave(h, seed + 1.0)).collect();
    let views = chart_json(x, &[("y0", "line", l!("Перегляди", "Views"), "BLUE#3497ED", views), ("y1", "line", l!("Поширення", "Shares"), "GOLDEN#F5BD25", shares)], json!({"y_scaled": true}));
    let fwd = |title: &str, views: i32, story: bool| PublicForward { title: title.into(), views: Some(views), story };
    MessageStats {
        graphs: vec![graph(l!("Перегляди", "Views"), views), StatsGraph { title: l!("Реакції", "Reactions").into(), state: GraphState::Pending(format!("demo-reactions-{msg}")) }],
        forwards_count: Some(4),
        forwards: vec![fwd("Rust Team 🦀", 1_204, false), fwd("Dev Digest", 3_870, false), fwd("Open Source UA", 912, false), fwd("Taras Bondar", 240, true)],
    }
}

/// The lazy reactions graph of a post, by the hour.
fn message_reactions(msg: i32) -> String {
    let seed = msg as f64;
    let posted = START + (DAYS as i64 - 2) * DAY + 9 * 3_600_000;
    let x: Vec<Value> = (0..HOURS).map(|h| json!(posted + h as i64 * 3_600_000)).collect();
    let r = |base: f64, s: f64| (0..HOURS).map(|h| base * (-(h as f64) / 6.0).exp() * (0.7 + 0.6 * wave(h, seed + s))).collect::<Vec<f64>>();
    chart_json(x, &[("y0", "bar", "👍", "BLUE#3497ED", r(60.0, 0.0)), ("y1", "bar", "❤", "RED#F34C44", r(35.0, 1.0)), ("y2", "bar", "🔥", "GOLDEN#F5BD25", r(22.0, 2.0))], json!({"stacked": true}))
}

pub fn handle(app: &mut App, cmd: Cmd) {
    match cmd {
        Cmd::LoadStats { peer, megagroup, .. } => {
            let stats = if megagroup { group() } else { channel() };
            app.push_event(Ev::Stats { peer, stats: Ok(Box::new(stats)) });
        }
        Cmd::LoadMessageStats { peer, msg, .. } => {
            app.push_event(Ev::MessageStats { peer, msg, stats: Ok(Box::new(message(msg))) });
        }
        Cmd::LoadStatsGraph { peer, msg: Some(msg), index, .. } => {
            let graph = graph("", message_reactions(msg)).state;
            app.push_event(Ev::StatsGraph { peer, msg: Some(msg), index, graph });
        }
        Cmd::LoadStatsGraph { peer, index, .. } => {
            let json = chart_json(
                days(),
                &[
                    ("y0", "line", l!("Перегляди", "Views"), "BLUE#3497ED", series(|k| 2_400.0 + 1_300.0 * wave(k, 2.4))),
                    ("y1", "line", l!("Поширення", "Shares"), "GOLDEN#F5BD25", series(|k| 20.0 + 30.0 * wave(k, 0.8))),
                ],
                serde_json::json!({"y_scaled": true}),
            );
            let graph = graph("", json).state;
            app.push_event(Ev::StatsGraph { peer, msg: None, index, graph });
        }
        _ => {}
    }
}

