//! Bots in the attachment menu (`messages.getAttachMenuBots`): their mini apps open for the
//! current chat from the 📎 menu. `t.me/<bot>?startattach` adds a bot after the user agrees.

use grammers_client::peer::{Peer, User};
use grammers_client::session::types::PeerId;
use grammers_client::tl;

use super::convert::peer_title;
use super::{Cmd, Ev, Worker};
use crate::i18n::lf;

/// Chat types an attachment menu bot works in (`AttachMenuPeerType`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AttachPeer {
    /// The chat with the bot itself.
    SameBot,
    /// Chats with other bots.
    Bot,
    /// Private chats with people.
    User,
    Group,
    Channel,
}

/// A bot of the attachment menu.
#[derive(Debug, Clone, PartialEq)]
pub struct AttachBot {
    pub bot: PeerId,
    pub name: String,
    pub peer_types: Vec<AttachPeer>,
    /// Not added to the menu by the user yet.
    pub inactive: bool,
    /// Adding it asks to let the bot message us.
    pub write_access: bool,
    /// Shown in the 📎 menu (some bots live only in the side menu).
    pub in_menu: bool,
}

impl Worker {
    pub(super) async fn attach_bots_cmd(&mut self, cmd: Cmd) -> Result<(), String> {
        let client = self.client()?;
        match cmd {
            Cmd::LoadAttachBots { hash } => self.load_attach_bots(hash).await?,
            Cmd::OpenAttachLink { peer, username, start_param } => {
                let bot = self.resolve(&username).await?;
                let b = self.peer_ref(bot)?;
                let tl::enums::AttachMenuBotsBot::Bot(res) = client
                    .invoke(&tl::functions::messages::GetAttachMenuBot { bot: b.into() })
                    .await
                    .map_err(|e| lf!("@{username} не працює в меню вкладень: {e}", "@{username} doesn't work in the attachment menu: {e}"))?;
                let users = self.remember_users(res.users).await;
                if let Some(bot) = attach_bot(res.bot, &users) {
                    self.out.send(Ev::AttachBotFound { peer, bot, start_param });
                }
            }
            Cmd::ToggleAttachBot { bot, enabled, write_allowed } => {
                let b = self.peer_ref(bot)?;
                let req = tl::functions::messages::ToggleBotInAttachMenu { write_allowed, bot: b.into(), enabled };
                client.invoke(&req).await.map_err(|e| lf!("Меню вкладень не змінено: {e}", "Attachment menu not changed: {e}"))?;
                self.out.send(Ev::AttachBotToggled { bot, enabled });
                // The changed list comes with a new hash.
                self.load_attach_bots(0).await?;
            }
            _ => {}
        }
        Ok(())
    }

    /// The list, unless the server says the one with `hash` is still current.
    async fn load_attach_bots(&self, hash: i64) -> Result<(), String> {
        let client = self.client()?;
        let res = client
            .invoke(&tl::functions::messages::GetAttachMenuBots { hash })
            .await
            .map_err(|e| lf!("Меню вкладень: {e}", "Attachment menu: {e}"))?;
        let tl::enums::AttachMenuBots::Bots(res) = res else { return Ok(()) };
        let users = self.remember_users(res.users).await;
        let bots = res.bots.into_iter().filter_map(|b| attach_bot(b, &users)).collect();
        self.out.send(Ev::AttachBots { hash: res.hash, bots });
        Ok(())
    }

    /// Access to the bots (for `requestWebView`) and their avatars, which stand for their icons.
    async fn remember_users(&self, raw: Vec<tl::enums::User>) -> Vec<Peer> {
        let Ok(client) = self.client() else { return Vec::new() };
        let mut users = Vec::new();
        for u in raw {
            let peer = Peer::User(User::from_raw(&client, u));
            if self.remember(&peer).await {
                users.push(peer);
            }
        }
        users
    }
}

/// Our view of a bot; `None` when its user didn't come with it.
fn attach_bot(raw: tl::enums::AttachMenuBot, users: &[Peer]) -> Option<AttachBot> {
    let tl::enums::AttachMenuBot::Bot(b) = raw;
    let user = users.iter().find(|p| p.id().bare_id_unchecked() == b.bot_id)?;
    let peer_types = b
        .peer_types
        .unwrap_or_default()
        .into_iter()
        .map(|t| match t {
            tl::enums::AttachMenuPeerType::SameBotPm => AttachPeer::SameBot,
            tl::enums::AttachMenuPeerType::BotPm => AttachPeer::Bot,
            tl::enums::AttachMenuPeerType::Pm => AttachPeer::User,
            tl::enums::AttachMenuPeerType::Chat => AttachPeer::Group,
            tl::enums::AttachMenuPeerType::Broadcast => AttachPeer::Channel,
        })
        .collect();
    Some(AttachBot {
        bot: user.id(),
        name: peer_title(user),
        peer_types,
        inactive: b.inactive,
        write_access: b.request_write_access,
        in_menu: b.show_in_attach_menu,
    })
}
