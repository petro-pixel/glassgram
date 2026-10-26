//! Reporting messages to Telegram's moderators (`messages.report`): the server leads through
//! its own reasons, sometimes a sub-reason and a comment, as in Telegram Desktop.

use grammers_client::tl;

use super::{Cmd, Ev, Worker};
use crate::i18n::lf;

/// What the server asks next.
#[derive(Debug, Clone)]
pub enum ReportStep {
    /// Pick a reason; the chosen `option` goes into the next request.
    Choose { title: String, options: Vec<(String, Vec<u8>)> },
    /// A comment for the chosen reason, which may be required.
    Comment { option: Vec<u8>, optional: bool },
    Done,
}

impl Worker {
    pub(super) async fn report_cmd(&mut self, cmd: Cmd) -> Result<(), String> {
        let Cmd::Report { peer, ids, option, message } = cmd else { return Ok(()) };
        let client = self.client()?;
        let r = self.peer_ref(peer)?;
        let out = self.out.clone();
        tokio::spawn(async move {
            let req = tl::functions::messages::Report { peer: r.into(), id: ids.clone(), option, message };
            let step = match client.invoke(&req).await {
                Ok(tl::enums::ReportResult::ChooseOption(c)) => ReportStep::Choose {
                    title: c.title,
                    options: c.options.into_iter().map(|tl::enums::MessageReportOption::Option(o)| (o.text, o.option)).collect(),
                },
                Ok(tl::enums::ReportResult::AddComment(c)) => ReportStep::Comment { option: c.option, optional: c.optional },
                Ok(tl::enums::ReportResult::Reported) => ReportStep::Done,
                Err(e) => return out.send(Ev::Error(lf!("Скаргу не надіслано: {e}", "Report not sent: {e}"))),
            };
            out.send(Ev::ReportStep { peer, ids, step });
        });
        Ok(())
    }
}
