//! Channel and supergroup boosts, as Telegram Desktop's boost box: the level and progress
//! (`premium.getBoostsStatus`), our Premium boost slots (`premium.getMyBoosts`), giving a boost
//! or moving one here (`premium.applyBoost`), and the boosters an admin sees (`premium.getBoostsList`).

use super::*;

/// Boosters per page of the admin list.
const BOOSTERS_PAGE: i32 = 20;

/// Where a chat stands: its level and the boosts counted towards the next one.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct BoostStatus {
    pub level: i32,
    /// Boosts the current level needed.
    pub level_boosts: i32,
    pub boosts: i32,
    /// Boosts the next level needs; `None` at the top level.
    pub next_level_boosts: Option<i32>,
    /// One of our slots boosts this chat.
    pub boosted: bool,
    /// The `t.me/boost/…` link of the chat.
    pub url: String,
    /// Boosts that came from gifted Premium and giveaways.
    pub gift_boosts: i32,
}

/// One of our boost slots (Premium gives a few).
#[derive(Debug, Clone, PartialEq)]
pub struct BoostSlot {
    pub slot: i32,
    /// The chat this slot boosts now, with its title.
    pub peer: Option<(PeerId, String)>,
    /// Unix time the boost runs out.
    pub expires: i64,
    /// Unix time the slot may move to another chat; 0 — any time.
    pub cooldown_until: i64,
}

/// A booster in the admin list.
#[derive(Debug, Clone)]
pub struct Booster {
    /// `None` for an unclaimed giveaway prize.
    pub user: Option<ChatInfo>,
    /// One Premium subscriber can count several times (`multiplier`).
    pub multiplier: i32,
    pub expires: i64,
    pub gift: bool,
    pub giveaway: bool,
}

/// What a failed `applyBoost` means to the user, as in Telegram Desktop's error boxes.
fn boost_error(e: &grammers_client::InvocationError) -> String {
    if e.is("PREMIUM_ACCOUNT_REQUIRED") {
        l!("Підсилювати можуть лише передплатники Telegram Premium.", "Only Telegram Premium subscribers can boost.").to_owned()
    } else if e.is("BOOST_NOT_MODIFIED") {
        l!("Ви вже підсилюєте цей чат.", "You are already boosting this chat.").to_owned()
    } else if e.is("PREMIUM_GIFTED_NOT_ALLOWED") {
        l!("Подарований Telegram Premium не дає підсилювати цей чат.", "A gifted Telegram Premium subscription can't boost this chat.").to_owned()
    } else if e.is("FLOOD_WAIT_*") || e.is("BOOSTS_*") {
        l!("Змінювати чат, який ви підсилюєте, можна раз на день.", "You can change the chat you boost only once a day.").to_owned()
    } else {
        lf!("Не вдалося підсилити: {e}", "Could not boost: {e}")
    }
}

impl Worker {
    pub(super) async fn boosts_cmd(&mut self, cmd: Cmd) -> Result<(), String> {
        let client = self.client()?;
        match cmd {
            Cmd::LoadBoosts(peer) => {
                self.send_boost_status(&client, peer).await?;
                self.send_my_boosts(&client).await?;
            }
            Cmd::ApplyBoost { peer, slots } => {
                let r = self.peer_ref(peer)?;
                let req = tl::functions::premium::ApplyBoost { slots: Some(slots), peer: r.into() };
                match client.invoke(&req).await {
                    Ok(res) => {
                        self.on_my_boosts(res).await;
                        self.out.send(Ev::Boosted(peer));
                    }
                    Err(e) => self.out.send(Ev::BoostFailed { peer, error: boost_error(&e) }),
                }
                self.send_boost_status(&client, peer).await?;
            }
            Cmd::LoadBoosters { peer, offset } => {
                let r = self.peer_ref(peer)?;
                let req = tl::functions::premium::GetBoostsList { gifts: false, peer: r.into(), offset: offset.clone(), limit: BOOSTERS_PAGE };
                let tl::enums::premium::BoostsList::List(res) = client.invoke(&req).await.map_err(|e| lf!("Підсилення: {e}", "Boosts: {e}"))?;
                let users = self.boost_users(&client, res.users).await;
                let list = res
                    .boosts
                    .into_iter()
                    .map(|tl::enums::Boost::Boost(b)| Booster {
                        user: b.user_id.and_then(|id| users.get(&id).cloned()),
                        multiplier: b.multiplier.unwrap_or(1).max(1),
                        expires: b.expires as i64,
                        gift: b.gift,
                        giveaway: b.giveaway,
                    })
                    .collect();
                let next = res.next_offset.unwrap_or_default();
                self.out.send(Ev::Boosters { peer, count: res.count, list, next, append: !offset.is_empty() });
            }
            Cmd::OpenBoostLink(name) => {
                let p = self.resolve_peer(&name).await?;
                if !matches!(p, Peer::Group(_) | Peer::Channel(_)) {
                    return Err(lf!("@{name} — не канал і не група", "@{name} is not a channel or a group"));
                }
                self.out.send(Ev::BoostLink(found_chat(&p, String::new())));
            }
            _ => {}
        }
        Ok(())
    }

    async fn send_boost_status(&self, client: &Client, peer: PeerId) -> Result<(), String> {
        let r = self.peer_ref(peer)?;
        let tl::enums::premium::BoostsStatus::Status(s) =
            client.invoke(&tl::functions::premium::GetBoostsStatus { peer: r.into() }).await.map_err(|e| lf!("Підсилення: {e}", "Boosts: {e}"))?;
        let status = BoostStatus {
            level: s.level,
            level_boosts: s.current_level_boosts,
            boosts: s.boosts,
            next_level_boosts: s.next_level_boosts,
            boosted: s.my_boost,
            url: s.boost_url,
            gift_boosts: s.gift_boosts.unwrap_or(0),
        };
        self.out.send(Ev::BoostStatus { peer, status });
        Ok(())
    }

    async fn send_my_boosts(&self, client: &Client) -> Result<(), String> {
        let res = client.invoke(&tl::functions::premium::GetMyBoosts {}).await.map_err(|e| lf!("Підсилення: {e}", "Boosts: {e}"))?;
        self.on_my_boosts(res).await;
        Ok(())
    }

    async fn on_my_boosts(&self, res: tl::enums::premium::MyBoosts) {
        let tl::enums::premium::MyBoosts::Boosts(res) = res;
        let Ok(client) = self.client() else { return };
        let mut titles: HashMap<PeerId, String> = HashMap::new();
        for raw in res.chats {
            let p = Peer::from_raw(&client, raw);
            if self.remember(&p).await {
                titles.insert(p.id(), peer_title(&p));
            }
        }
        let slots = res
            .my_boosts
            .into_iter()
            .map(|tl::enums::MyBoost::Boost(b)| BoostSlot {
                slot: b.slot,
                peer: b.peer.as_ref().map(super::chatlists::raw_peer_id).map(|id| (id, titles.get(&id).cloned().unwrap_or_default())),
                expires: b.expires as i64,
                cooldown_until: b.cooldown_until_date.unwrap_or(0) as i64,
            })
            .collect();
        self.out.send(Ev::MyBoosts(slots));
    }

    /// Boosters as chat rows, remembered so a click can open them.
    async fn boost_users(&self, client: &Client, raw: Vec<tl::enums::User>) -> HashMap<i64, ChatInfo> {
        let mut users = HashMap::new();
        for u in raw {
            let p = Peer::User(User::from_raw(client, u));
            if self.remember(&p).await {
                users.insert(p.id().bare_id_unchecked(), found_chat(&p, String::new()));
            }
        }
        users
    }
}
