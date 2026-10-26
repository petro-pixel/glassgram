//! Birthdays: our own (`account.updateBirthday`), a user's in their profile, and contacts'
//! birthdays today for the reminder above the chat list (`contacts.getBirthdays`).

use grammers_client::peer::{Peer, User};
use grammers_client::tl;

use super::{ChatInfo, Cmd, Ev, Worker, person};
use crate::i18n::lf;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Birthday {
    pub day: u8,
    pub month: u8,
    /// Hidden by its owner, or not set.
    pub year: Option<u16>,
}

impl Birthday {
    pub(super) fn from_raw(b: &tl::enums::Birthday) -> Self {
        let tl::enums::Birthday::Birthday(b) = b;
        Self { day: b.day.clamp(1, 31) as u8, month: b.month.clamp(1, 12) as u8, year: b.year.map(|y| y as u16) }
    }

    fn raw(self) -> tl::enums::Birthday {
        tl::types::Birthday { day: self.day.into(), month: self.month.into(), year: self.year.map(i32::from) }.into()
    }

    /// Falls on the given local day.
    pub fn is_on(self, date: jiff::civil::Date) -> bool {
        date.day() as u8 == self.day && date.month() as u8 == self.month
    }
}

impl Worker {
    pub(super) async fn birthdays_cmd(&mut self, cmd: Cmd) -> Result<(), String> {
        let client = self.client()?;
        match cmd {
            Cmd::SetBirthday(b) => {
                let req = tl::functions::account::UpdateBirthday { birthday: b.map(Birthday::raw) };
                client.invoke(&req).await.map_err(|e| lf!("День народження не збережено: {e}", "Birthday not saved: {e}"))?;
                self.load_profile().await?;
            }
            Cmd::LoadBirthdays => {
                let tl::enums::contacts::ContactBirthdays::Birthdays(res) =
                    client.invoke(&tl::functions::contacts::GetBirthdays {}).await.map_err(|e| lf!("Дні народження: {e}", "Birthdays: {e}"))?;
                // The server sends a window around today; the reminder shows only today's.
                let today = jiff::Zoned::now().date();
                let mut list: Vec<(ChatInfo, Birthday)> = Vec::new();
                for raw in res.users {
                    let peer = Peer::User(User::from_raw(&client, raw));
                    let Some(b) = res.contacts.iter().find_map(|tl::enums::ContactBirthday::Birthday(c)| (c.contact_id == peer.id().bare_id_unchecked()).then(|| Birthday::from_raw(&c.birthday))) else {
                        continue;
                    };
                    if b.is_on(today) && self.remember(&peer).await {
                        list.push((person(&peer), b));
                    }
                }
                self.out.send(Ev::Birthdays(list));
            }
            _ => {}
        }
        Ok(())
    }
}
