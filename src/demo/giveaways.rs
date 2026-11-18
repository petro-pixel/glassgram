//! A demo giveaway in "Tech News" and its results, and what "Learn more" says about each.

use super::*;

const GIVEAWAY: i32 = 43;
const RESULTS: i32 = 44;

fn card(id: i32, site: &str, title: &str, description: &str) -> WebPageInfo {
    WebPageInfo { id: id as i64, url: String::new(), site: site.into(), title: title.into(), description: description.into(), photo: false, iv: false, card: CardKind::Giveaway }
}

/// After the Instant View post, so ids stay ascending.
pub(super) fn posts(base: impl Fn(i32, &str) -> Msg) -> [Msg; 2] {
    let giveaway = card(GIVEAWAY, l!("Розіграш", "Giveaway"), "Telegram Premium for 6 months", "5 winners\nOnly new subscribers\nCountries: UA, PL\nWinners will be chosen on 01.12.2026");
    let results = card(RESULTS, l!("Результати розіграшу", "Giveaway results"), "⭐ 500 Telegram Stars", "3 winners selected");
    [
        Msg { id: GIVEAWAY, text: String::new(), webpage: Some(giveaway), views: Some(9_400), ..base(33, "Tech News") },
        Msg { id: RESULTS, text: String::new(), webpage: Some(results), views: Some(7_100), ..base(34, "Tech News") },
    ]
}

pub(super) fn info(app: &mut App, peer: PeerId, id: i32) {
    let text = if id == RESULTS {
        l!("Розіграш завершився 15.11.2026. Обрано 3 переможців. Цього разу ви не виграли.", "The giveaway ended on 15.11.2026. 3 winners were selected. You didn't win this time.")
    } else {
        l!("Розіграш почався 18.11.2026. Ви берете участь у цьому розіграші.", "The giveaway started on 18.11.2026. You are participating in this giveaway.")
    };
    app.push_event(Ev::GiveawayInfo { peer, id, result: Ok(text.into()) });
}
