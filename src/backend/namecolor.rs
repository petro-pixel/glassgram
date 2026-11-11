//! Name colour, profile colour and emoji status: reading them off users and channels, the
//! server's extra colours (`help.getPeerColors`), and setting our own (`account.updateColor`,
//! `account.updateEmojiStatus`).

use grammers_client::peer::Peer;
use grammers_client::tl;

use super::account::rpc_name;
use super::{Cmd, Ev, PeerId, Worker};
use crate::i18n::{l, lf};

/// A colour the server offers: its stripe (1–3 RGB tones) for the light and the dark theme.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct ColorOption {
    pub id: i32,
    pub light: Vec<u32>,
    pub dark: Vec<u32>,
}

/// Telegram picks a colour from the id when the peer hasn't chosen one (7 built-in colours).
pub fn default_color(id: PeerId) -> i32 {
    (id.bare_id_unchecked().unsigned_abs() % 7) as i32
}

fn raw_colors(peer: &Peer) -> (Option<&tl::enums::PeerColor>, Option<&tl::enums::EmojiStatus>) {
    match peer {
        Peer::User(u) => match &u.raw {
            tl::enums::User::User(x) => (x.color.as_ref(), x.emoji_status.as_ref()),
            _ => (None, None),
        },
        Peer::Channel(c) => (c.raw.color.as_ref(), c.raw.emoji_status.as_ref()),
        Peer::Group(g) => match &g.raw {
            tl::enums::Chat::Channel(c) => (c.color.as_ref(), c.emoji_status.as_ref()),
            _ => (None, None),
        },
        _ => (None, None),
    }
}

/// (colour index, background emoji) of a `PeerColor`. Collectible colours carry their own RGB
/// instead of an index: they fall back to the default one.
pub(super) fn color_parts(c: Option<&tl::enums::PeerColor>) -> (Option<i32>, Option<i64>) {
    match c {
        Some(tl::enums::PeerColor::Color(x)) => (x.color, x.background_emoji_id),
        Some(tl::enums::PeerColor::Collectible(x)) => (None, Some(x.background_emoji_id)),
        _ => (None, None),
    }
}

/// The colour index a peer's name is drawn with.
pub(super) fn name_color(peer: &Peer) -> i32 {
    color_parts(raw_colors(peer).0).0.unwrap_or_else(|| default_color(peer.id()))
}

/// The custom emoji shown after a peer's name, unless it has expired.
pub(super) fn emoji_status(peer: &Peer) -> Option<i64> {
    status_doc(raw_colors(peer).1?)
}

pub(super) fn status_doc(s: &tl::enums::EmojiStatus) -> Option<i64> {
    let now = jiff::Timestamp::now().as_second();
    let alive = |until: Option<i32>| until.is_none_or(|u| u as i64 > now);
    match s {
        tl::enums::EmojiStatus::Status(x) if alive(x.until) => Some(x.document_id),
        tl::enums::EmojiStatus::Collectible(x) if alive(x.until) => Some(x.document_id),
        _ => None,
    }
}

/// Document ids of a status list, without repeats.
fn status_ids(list: Result<tl::enums::account::EmojiStatuses, grammers_client::InvocationError>, ids: &mut Vec<i64>) {
    if let Ok(tl::enums::account::EmojiStatuses::Statuses(s)) = list {
        for id in s.statuses.iter().filter_map(status_doc) {
            if !ids.contains(&id) {
                ids.push(id);
            }
        }
    }
}

fn options(list: Result<tl::enums::help::PeerColors, grammers_client::InvocationError>, profile: bool) -> Vec<ColorOption> {
    let tones = |set: Option<tl::enums::help::PeerColorSet>| match set {
        Some(tl::enums::help::PeerColorSet::Set(s)) if !profile => s.colors,
        Some(tl::enums::help::PeerColorSet::PeerColorProfileSet(s)) if profile => s.bg_colors,
        _ => Vec::new(),
    }
    .into_iter()
    .map(|c| c as u32)
    .collect::<Vec<u32>>();
    match list {
        Ok(tl::enums::help::PeerColors::Colors(c)) => c
            .colors
            .into_iter()
            .filter_map(|tl::enums::help::PeerColorOption::Option(o)| {
                (!o.hidden).then(|| {
                    let light = tones(o.colors);
                    let dark = tones(o.dark_colors);
                    ColorOption { id: o.color_id, dark: if dark.is_empty() { light.clone() } else { dark }, light }
                })
            })
            .collect(),
        Ok(_) => Vec::new(),
        Err(e) => {
            super::log_error(&lf!("Кольори імен: {e}", "Name colours: {e}"));
            Vec::new()
        }
    }
}

/// Name colours and statuses are Premium: say so instead of the bare error code.
fn premium_error(e: &grammers_client::InvocationError, what: &str) -> String {
    match rpc_name(e) {
        Some("PREMIUM_ACCOUNT_REQUIRED") => lf!("{what}: потрібен Telegram Premium", "{what}: this needs Telegram Premium"),
        Some("COLOR_INVALID") => lf!("{what}: цей колір недоступний", "{what}: this colour is not available"),
        _ => format!("{what}: {e}"),
    }
}

impl Worker {
    pub(super) async fn namecolor_cmd(&mut self, cmd: Cmd) -> Result<(), String> {
        let client = self.client()?;
        match cmd {
            Cmd::LoadPeerColors => {
                let out = self.out.clone();
                tokio::spawn(async move {
                    let names = options(client.invoke(&tl::functions::help::GetPeerColors { hash: 0 }).await, false);
                    let profiles = options(client.invoke(&tl::functions::help::GetPeerProfileColors { hash: 0 }).await, true);
                    out.send(Ev::PeerColors { names, profiles });
                });
            }
            Cmd::LoadEmojiStatuses => {
                let out = self.out.clone();
                tokio::spawn(async move {
                    let mut ids = Vec::new();
                    status_ids(client.invoke(&tl::functions::account::GetRecentEmojiStatuses { hash: 0 }).await, &mut ids);
                    status_ids(client.invoke(&tl::functions::account::GetDefaultEmojiStatuses { hash: 0 }).await, &mut ids);
                    out.send(Ev::EmojiStatuses(ids));
                });
            }
            Cmd::SetNameColor { profile, color, emoji } => {
                let what = if profile { l!("Колір профілю", "Profile colour") } else { l!("Колір імені", "Name colour") };
                let color = (color.is_some() || emoji.is_some()).then(|| tl::types::PeerColor { color, background_emoji_id: emoji }.into());
                client.invoke(&tl::functions::account::UpdateColor { for_profile: profile, color }).await.map_err(|e| premium_error(&e, what))?;
                self.load_profile().await?;
                self.out.send(Ev::Notice(lf!("{what} збережено", "{what} saved")));
            }
            Cmd::SetEmojiStatus(id) => {
                let emoji_status = match id {
                    Some(document_id) => tl::types::EmojiStatus { document_id, until: None }.into(),
                    None => tl::enums::EmojiStatus::Empty,
                };
                let what = l!("Емодзі-статус", "Emoji status");
                client.invoke(&tl::functions::account::UpdateEmojiStatus { emoji_status }).await.map_err(|e| premium_error(&e, what))?;
                self.load_profile().await?;
            }
            _ => {}
        }
        Ok(())
    }
}
