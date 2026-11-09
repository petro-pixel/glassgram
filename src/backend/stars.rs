//! "My Stars" in Settings: the balance (`payments.getStarsStatus`) and the transaction history
//! (`payments.getStarsTransactions`). Read only: top-up and every payment wait for a separate
//! security review (AGENTS.md), so no request here spends or buys Stars.

use super::*;

/// Transactions per page.
const PAGE: i32 = 30;

/// Which side of the history a tab shows.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum StarsDir {
    #[default]
    All,
    Incoming,
    Outgoing,
}

/// One entry of the Stars history.
#[derive(Debug, Clone, Default)]
pub struct StarsTx {
    pub id: String,
    /// Signed amount in whole Stars; `nanos` is the fraction (paid reactions and commissions have one).
    pub stars: i64,
    pub nanos: i32,
    /// Formatted date and time, ready for the list.
    pub when: String,
    /// Who the Stars came from or went to: a chat or bot, or a store (Fragment, App Store…).
    pub source: String,
    /// The bot's product title or what the transaction was for.
    pub title: String,
    pub description: String,
    pub refund: bool,
    pub pending: bool,
    pub failed: bool,
    /// A Fragment withdrawal's page in the blockchain explorer.
    pub url: String,
}

impl StarsTx {
    /// "+12", "−3.5": what the list shows next to the star.
    pub fn amount(&self) -> String {
        let neg = self.stars < 0 || self.nanos < 0;
        let whole = self.stars.unsigned_abs();
        let frac = self.nanos.unsigned_abs();
        let mut s = format!("{}{whole}", if neg { "−" } else { "+" });
        if frac > 0 {
            let f = format!("{:09}", frac);
            s.push('.');
            s.push_str(f.trim_end_matches('0'));
        }
        s
    }
}

/// Whole Stars and the fraction of a `StarsAmount`; TON amounts are not asked for, so they count as zero.
fn amount(a: tl::enums::StarsAmount) -> (i64, i32) {
    match a {
        tl::enums::StarsAmount::Amount(a) => (a.amount, a.nanos),
        tl::enums::StarsAmount::StarsTonAmount(_) => (0, 0),
    }
}

/// Names of the chats and users that came with an answer.
fn names(client: &Client, chats: Vec<tl::enums::Chat>, users: Vec<tl::enums::User>) -> HashMap<PeerId, String> {
    let users = users.into_iter().map(|u| Peer::User(User::from_raw(client, u)));
    let chats = chats.into_iter().map(|c| Peer::from_raw(client, c));
    users.chain(chats).map(|p| (p.id(), peer_title(&p))).collect()
}

fn source(peer: &tl::enums::StarsTransactionPeer, names: &HashMap<PeerId, String>) -> String {
    use tl::enums::StarsTransactionPeer as P;
    match peer {
        P::Peer(p) => names.get(&PeerId::from(&p.peer)).cloned().unwrap_or_default(),
        P::AppStore => "App Store".into(),
        P::PlayMarket => "Google Play".into(),
        P::PremiumBot => "Premium Bot".into(),
        P::Fragment => "Fragment".into(),
        P::Ads => l!("Реклама Telegram", "Telegram Ads").into(),
        P::Api => "Telegram API".into(),
        P::Unsupported => l!("Невідоме джерело", "Unknown source").into(),
    }
}

/// The title Telegram Desktop shows when the bot gave none.
fn fallback_title(t: &tl::types::StarsTransaction) -> &'static str {
    if t.refund {
        l!("Повернення", "Refund")
    } else if t.reaction {
        l!("Зіркова реакція", "Star reaction")
    } else if t.stargift_upgrade || t.stargift_prepaid_upgrade {
        l!("Покращення подарунка", "Gift upgrade")
    } else if t.stargift_resale {
        l!("Продаж подарунка", "Gift sale")
    } else if t.stargift.is_some() || t.gift {
        l!("Подарунок", "Gift")
    } else if t.subscription_period.is_some() {
        l!("Підписка", "Subscription")
    } else if t.premium_gift_months.is_some() {
        l!("Подарунок Telegram Premium", "Telegram Premium gift")
    } else if t.paid_messages.is_some() {
        l!("Платні повідомлення", "Paid messages")
    } else if t.giveaway_post_id.is_some() {
        l!("Розіграш", "Giveaway")
    } else if t.posts_search {
        l!("Пошук дописів", "Post search")
    } else if t.ads_proceeds_from_date.is_some() {
        l!("Дохід від реклами", "Ads revenue")
    } else if t.extended_media.is_some() {
        l!("Платний медіафайл", "Paid media")
    } else if matches!(t.peer, tl::enums::StarsTransactionPeer::Fragment) {
        l!("Виведення", "Withdrawal")
    } else if matches!(t.peer, tl::enums::StarsTransactionPeer::AppStore | tl::enums::StarsTransactionPeer::PlayMarket | tl::enums::StarsTransactionPeer::PremiumBot) {
        l!("Купівля зірок", "Stars purchase")
    } else {
        ""
    }
}

fn convert(t: tl::enums::StarsTransaction, names: &HashMap<PeerId, String>) -> StarsTx {
    let tl::enums::StarsTransaction::Transaction(t) = t;
    let (stars, nanos) = amount(t.amount.clone());
    let ts = jiff::Timestamp::from_second(t.date as i64).unwrap_or_default();
    let title = t.title.clone().filter(|s| !s.is_empty()).unwrap_or_else(|| fallback_title(&t).to_owned());
    StarsTx {
        source: source(&t.peer, names),
        title,
        description: t.description.clone().unwrap_or_default(),
        url: t.transaction_url.clone().unwrap_or_default(),
        refund: t.refund,
        pending: t.pending,
        failed: t.failed,
        id: t.id,
        stars,
        nanos,
        when: format!("{} {}", day(ts), clock(ts)),
    }
}

impl Worker {
    pub(super) async fn stars_cmd(&mut self, cmd: Cmd) -> Result<(), String> {
        let Cmd::LoadStars { dir, offset } = cmd else { return Ok(()) };
        let client = self.client()?;
        let out = self.out.clone();
        tokio::spawn(async move {
            // The balance is refreshed with every first page, so reopening the page shows a fresh one.
            if offset.is_empty() {
                let req = tl::functions::payments::GetStarsStatus { ton: false, peer: tl::enums::InputPeer::PeerSelf };
                match client.invoke(&req).await {
                    Ok(tl::enums::payments::StarsStatus::Status(s)) => {
                        let (stars, nanos) = amount(s.balance);
                        out.send(Ev::StarsBalance { stars, nanos });
                    }
                    Err(e) => return out.send(Ev::Error(lf!("Зірки: {e}", "Stars: {e}"))),
                }
            }
            let req = tl::functions::payments::GetStarsTransactions {
                inbound: dir == StarsDir::Incoming,
                outbound: dir == StarsDir::Outgoing,
                ascending: false,
                ton: false,
                subscription_id: None,
                peer: tl::enums::InputPeer::PeerSelf,
                offset: offset.clone(),
                limit: PAGE,
            };
            let tl::enums::payments::StarsStatus::Status(s) = match client.invoke(&req).await {
                Ok(s) => s,
                Err(e) => return out.send(Ev::Error(lf!("Історія зірок: {e}", "Stars history: {e}"))),
            };
            let names = names(&client, s.chats, s.users);
            let list = s.history.unwrap_or_default().into_iter().map(|t| convert(t, &names)).collect();
            out.send(Ev::StarsHistory { dir, list, next: s.next_offset.unwrap_or_default(), append: !offset.is_empty() });
        });
        Ok(())
    }
}
