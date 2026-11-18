//! Giveaways in channels, as in Telegram Desktop: the giveaway and its results drawn as cards
//! (`messageMediaGiveaway`, `messageMediaGiveawayResults`), and "Learn more" with where it
//! stands for us (`payments.getGiveawayInfo`). Taking part or buying one is not here.

use grammers_client::message::Message;
use grammers_client::tl;

use super::{CardKind, Cmd, Ev, WebPageInfo, Worker};
use crate::i18n::{l, lf, lp};

/// The prize line: Stars, Premium months, or the channel's own words.
fn prize(stars: Option<i64>, months: Option<i32>) -> String {
    match (stars, months) {
        (Some(n), _) => lf!("⭐ {} зірок Telegram", "⭐ {} Telegram Stars", crate::i18n::group(n as u64)),
        (None, Some(m)) => lp!(m, ["Telegram Premium на {n} місяць", "Telegram Premium на {n} місяці", "Telegram Premium на {n} місяців"], ["Telegram Premium for {n} month", "Telegram Premium for {n} months"]),
        (None, None) => l!("Приз", "Prize").to_owned(),
    }
}

fn day(ts: i32) -> String {
    jiff::Timestamp::from_second(ts as i64).unwrap_or_default().to_zoned(jiff::tz::TimeZone::system()).strftime("%d.%m.%Y").to_string()
}

/// The giveaway or its results as a card under the message.
pub(super) fn card(m: &Message) -> Option<WebPageInfo> {
    let tl::enums::Message::Message(x) = &m.raw else { return None };
    let (site, title, lines) = match x.media.as_ref()? {
        tl::enums::MessageMedia::Giveaway(g) => {
            let mut lines = vec![lp!(g.quantity, ["{n} переможець", "{n} переможці", "{n} переможців"], ["{n} winner", "{n} winners"])];
            if let Some(extra) = g.prize_description.as_ref().filter(|d| !d.is_empty()) {
                lines.push(lf!("Також: {extra}", "Also: {extra}"));
            }
            if g.channels.len() > 1 {
                lines.push(lp!(g.channels.len(), ["Серед підписників {n} каналу", "Серед підписників {n} каналів", "Серед підписників {n} каналів"], ["Among subscribers of {n} channel", "Among subscribers of {n} channels"]));
            }
            if g.only_new_subscribers {
                lines.push(l!("Лише нові підписники", "Only new subscribers").to_owned());
            }
            if let Some(c) = g.countries_iso2.as_ref().filter(|c| !c.is_empty()) {
                lines.push(lf!("Країни: {}", "Countries: {}", c.join(", ")));
            }
            lines.push(lf!("Переможців оберуть {}", "Winners will be chosen on {}", day(g.until_date)));
            (l!("Розіграш", "Giveaway"), prize(g.stars, g.months), lines)
        }
        tl::enums::MessageMedia::GiveawayResults(r) => {
            let mut lines = vec![lp!(r.winners_count, ["Обрано {n} переможця", "Обрано {n} переможців", "Обрано {n} переможців"], ["{n} winner selected", "{n} winners selected"])];
            if r.unclaimed_count > 0 {
                lines.push(lp!(r.unclaimed_count, ["{n} приз не отримано", "{n} призи не отримано", "{n} призів не отримано"], ["{n} prize unclaimed", "{n} prizes unclaimed"]));
            }
            if r.refunded {
                lines.push(l!("Розіграш скасовано, кошти повернуто", "The giveaway was cancelled and refunded").to_owned());
            }
            (l!("Результати розіграшу", "Giveaway results"), prize(r.stars, r.months), lines)
        }
        _ => return None,
    };
    Some(WebPageInfo { id: x.id as i64, url: String::new(), site: site.to_owned(), title, description: lines.join("\n"), photo: false, iv: false, card: CardKind::Giveaway })
}

/// `payments.giveawayInfo` in words, as Telegram Desktop's "About the giveaway" box says it.
fn info_text(i: tl::enums::payments::GiveawayInfo) -> String {
    match i {
        tl::enums::payments::GiveawayInfo::Info(i) => {
            let mut s = vec![lf!("Розіграш почався {}.", "The giveaway started on {}.", day(i.start_date))];
            s.push(if i.preparing_results {
                l!("Переможців уже обирають.", "The winners are being selected.").to_owned()
            } else if i.participating {
                l!("Ви берете участь у цьому розіграші.", "You are participating in this giveaway.").to_owned()
            } else if let Some(c) = &i.disallowed_country {
                lf!("Ваша країна ({c}) не бере участі в розіграші.", "Your country ({c}) is not part of this giveaway.")
            } else if i.admin_disallowed_chat_id.is_some() {
                l!("Адміністратори каналу не можуть брати участь.", "Channel admins can't take part.").to_owned()
            } else if let Some(t) = i.joined_too_early_date {
                lf!("Ви не можете брати участь: ви підписалися до {}, а розіграш лише для нових підписників.", "You can't take part: you joined before {}, and the giveaway is only for new subscribers.", day(t))
            } else {
                l!("Ви не берете участі. Щоб узяти участь, підпишіться на канал до кінця розіграшу.", "You are not participating. To take part, subscribe to the channel before the giveaway ends.").to_owned()
            });
            s.join(" ")
        }
        tl::enums::payments::GiveawayInfo::Results(r) => {
            let mut s = vec![lf!("Розіграш завершився {}.", "The giveaway ended on {}.", day(r.finish_date))];
            s.push(lp!(r.winners_count, ["Обрано {n} переможця.", "Обрано {n} переможців.", "Обрано {n} переможців."], ["{n} winner was selected.", "{n} winners were selected."]));
            if r.refunded {
                s.push(l!("Розіграш скасовано, кошти повернуто.", "The giveaway was cancelled and refunded.").to_owned());
            } else if r.winner {
                s.push(match r.stars_prize {
                    Some(n) => lf!("Ви виграли ⭐ {} — їх уже додано на ваш баланс.", "You won ⭐ {} — they are already on your balance.", crate::i18n::group(n as u64)),
                    None => l!("Ви виграли! Подарунковий код є в чаті з каналом-організатором.", "You won! The gift code is in your chat with the organising channel.").to_owned(),
                });
            } else {
                s.push(l!("Цього разу ви не виграли.", "You didn't win this time.").to_owned());
            }
            s.join(" ")
        }
    }
}

impl Worker {
    pub(super) async fn giveaways_cmd(&mut self, cmd: Cmd) -> Result<(), String> {
        let Cmd::GiveawayInfo { peer, id } = cmd else { return Ok(()) };
        let client = self.client()?;
        let req = tl::functions::payments::GetGiveawayInfo { peer: self.peer_ref(peer)?.into(), msg_id: id };
        let out = self.out.clone();
        tokio::spawn(async move {
            let result = client.invoke(&req).await.map(info_text).map_err(|e| lf!("Розіграш: {e}", "Giveaway: {e}"));
            out.send(Ev::GiveawayInfo { peer, id, result });
        });
        Ok(())
    }
}
