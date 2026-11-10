//! Demo "My Stars": a balance and a short history split into two pages, so the tabs,
//! "Show more" and the details box can be tried without the network.

use crate::app::App;
use crate::backend::{Cmd, Ev, StarsDir, StarsTx};
use crate::i18n::l;

/// Transactions per demo page.
const PAGE: usize = 6;

fn history() -> Vec<StarsTx> {
    let tx = |id: &str, stars: i64, nanos: i32, source: &str, title: &str, when: &str| StarsTx {
        id: id.into(),
        stars,
        nanos,
        when: when.into(),
        source: source.into(),
        title: title.into(),
        ..Default::default()
    };
    let mut list = vec![
        tx("demo-tx-01", 250, 0, "App Store", l!("Купівля зірок", "Stars purchase"), "09.11.2026 10:42"),
        tx("demo-tx-02", -50, 0, "Weather Bot", l!("Прогноз на тиждень", "Weekly forecast"), "09.11.2026 09:15"),
        tx("demo-tx-03", -1, 0, "Rust Community", l!("Зіркова реакція", "Star reaction"), "08.11.2026 21:03"),
        tx("demo-tx-04", 25, 0, "Olia", l!("Подарунок", "Gift"), "07.11.2026 18:30"),
        tx("demo-tx-05", -100, 0, "Glass Shop", l!("Набір тем", "Theme pack"), "05.11.2026 12:00"),
        tx("demo-tx-06", 2, 500_000_000, "Glass Shop", l!("Комісія партнерської програми", "Affiliate commission"), "03.11.2026 08:47"),
        tx("demo-tx-07", -15, 0, "Channel News", l!("Платний медіафайл", "Paid media"), "01.11.2026 19:20"),
        tx("demo-tx-08", 500, 0, "Fragment", l!("Купівля зірок", "Stars purchase"), "28.10.2026 14:05"),
        tx("demo-tx-09", -30, 0, "Pixel Games", l!("Підписка", "Subscription"), "20.10.2026 11:11"),
        tx("demo-tx-10", 30, 0, "Pixel Games", l!("Повернення", "Refund"), "20.10.2026 11:40"),
    ];
    list[1].description = l!("Щоденні прогнози погоди на 7 днів", "Daily weather forecasts for 7 days").into();
    list[4].pending = true;
    list[9].refund = true;
    list[7].url = "https://fragment.com".into();
    list
}

pub fn handle(app: &mut App, cmd: Cmd) {
    let Cmd::LoadStars { dir, offset } = cmd else { return };
    if offset.is_empty() {
        app.push_event(Ev::StarsBalance { stars: 611, nanos: 500_000_000 });
    }
    let all: Vec<StarsTx> = history()
        .into_iter()
        .filter(|t| match dir {
            StarsDir::All => true,
            StarsDir::Incoming => t.stars > 0,
            StarsDir::Outgoing => t.stars < 0,
        })
        .collect();
    let from = offset.parse::<usize>().unwrap_or(0).min(all.len());
    let to = (from + PAGE).min(all.len());
    let next = if to < all.len() { to.to_string() } else { String::new() };
    app.push_event(Ev::StarsHistory { dir, list: all[from..to].to_vec(), next, append: from > 0 });
}
