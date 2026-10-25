//! Sponsored messages in channels: the API terms require showing them and reporting
//! views and clicks back to the server.

use grammers_client::media::Photo;
use grammers_client::tl;

use super::{Cmd, Ev, ImgKey, Worker};
use crate::format::{self, Span};
use crate::i18n::lf;

/// One sponsored message (`sponsoredMessage`), shown at the end of a channel's history.
#[derive(Debug, Clone)]
pub struct Sponsored {
    /// Identifies the ad in view, click and report requests.
    pub random_id: Vec<u8>,
    pub title: String,
    pub text: String,
    pub spans: Vec<Span>,
    pub url: String,
    pub button: String,
    /// "Recommended" instead of "Ad" (the channel promotes something of its own).
    pub recommended: bool,
    pub can_report: bool,
    /// Who placed the ad and extra legal text, for "About this ad".
    pub sponsor_info: String,
    pub additional_info: String,
    /// Photo id: the picture arrives as `ImgKey::Sponsored(id)`.
    pub photo: Option<i64>,
}

/// Server answer to a report.
#[derive(Debug, Clone)]
pub enum SponsoredReport {
    /// Pick a reason; the chosen `option` goes into the next request.
    Choose { title: String, options: Vec<(String, Vec<u8>)> },
    /// Ads are hidden in this chat (Premium).
    Hidden,
    Reported,
}

fn convert(m: tl::types::SponsoredMessage) -> Sponsored {
    let photo = match &m.photo {
        Some(tl::enums::Photo::Photo(p)) => Some(p.id),
        _ => None,
    };
    Sponsored {
        spans: format::from_entities(&m.message, m.entities.as_deref().unwrap_or_default(), 0),
        random_id: m.random_id,
        title: m.title,
        text: m.message,
        url: m.url,
        button: m.button_text,
        recommended: m.recommended,
        can_report: m.can_report,
        sponsor_info: m.sponsor_info.unwrap_or_default(),
        additional_info: m.additional_info.unwrap_or_default(),
        photo,
    }
}

impl Worker {
    pub(super) async fn sponsored_cmd(&mut self, cmd: Cmd) -> Result<(), String> {
        let client = self.client()?;
        let out = self.out.clone();
        match cmd {
            Cmd::LoadSponsored(peer) => {
                let r = self.peer_ref(peer)?;
                let loader = self.loader.clone();
                tokio::spawn(async move {
                    let req = tl::functions::messages::GetSponsoredMessages { peer: r.into(), msg_id: None };
                    let list = match client.invoke(&req).await {
                        Ok(tl::enums::messages::SponsoredMessages::Messages(l)) => l.messages,
                        Ok(tl::enums::messages::SponsoredMessages::Empty) => Vec::new(),
                        // Not every chat can have ads (private, admin, small channel): no error for the user.
                        Err(e) => return super::log_error(&format!("sponsored: {e}")),
                    };
                    let ads: Vec<Sponsored> = list
                        .into_iter()
                        .map(|tl::enums::SponsoredMessage::Message(m)| {
                            if let (Some(l), Some(tl::enums::Photo::Photo(p))) = (&loader, &m.photo) {
                                let photo = Photo::from_raw(tl::enums::Photo::Photo(p.clone()));
                                l.page_photo(ImgKey::Sponsored(p.id), format!("ad{}.jpg", p.id), photo);
                            }
                            convert(m)
                        })
                        .collect();
                    out.send(Ev::Sponsored { peer, ads });
                });
            }
            Cmd::ViewSponsored(random_id) => {
                tokio::spawn(async move {
                    if let Err(e) = client.invoke(&tl::functions::messages::ViewSponsoredMessage { random_id }).await {
                        super::log_error(&format!("sponsored view: {e}"));
                    }
                });
            }
            Cmd::ClickSponsored(random_id) => {
                tokio::spawn(async move {
                    let req = tl::functions::messages::ClickSponsoredMessage { media: false, fullscreen: false, random_id };
                    if let Err(e) = client.invoke(&req).await {
                        super::log_error(&format!("sponsored click: {e}"));
                    }
                });
            }
            Cmd::ReportSponsored { peer, random_id, option } => {
                tokio::spawn(async move {
                    let req = tl::functions::messages::ReportSponsoredMessage { random_id: random_id.clone(), option };
                    let result = match client.invoke(&req).await {
                        Ok(tl::enums::channels::SponsoredMessageReportResult::ChooseOption(c)) => SponsoredReport::Choose {
                            title: c.title,
                            options: c
                                .options
                                .into_iter()
                                .map(|tl::enums::SponsoredMessageReportOption::Option(o)| (o.text, o.option))
                                .collect(),
                        },
                        Ok(tl::enums::channels::SponsoredMessageReportResult::AdsHidden) => SponsoredReport::Hidden,
                        Ok(tl::enums::channels::SponsoredMessageReportResult::Reported) => SponsoredReport::Reported,
                        Err(e) => return out.send(Ev::Error(lf!("Скарга не надіслана: {e}", "Report not sent: {e}"))),
                    };
                    out.send(Ev::SponsoredReport { peer, random_id, result });
                });
            }
            _ => {}
        }
        Ok(())
    }
}
