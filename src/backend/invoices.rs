//! Invoices from bots drawn as cards (`messageMediaInvoice`): what is sold and for how much.
//! Paying is not here: it waits for the payments security review.

use grammers_client::message::Message;
use grammers_client::tl;

use super::{CardKind, WebPageInfo};
use crate::i18n::{l, lf};

/// Currencies whose amounts have no minor unit, or three digits of it; the rest have two.
const WHOLE: [&str; 17] = ["BIF", "CLP", "DJF", "GNF", "ISK", "JPY", "KMF", "KRW", "MGA", "PYG", "RWF", "UGX", "VND", "VUV", "XAF", "XOF", "XPF"];
const THOUSANDTHS: [&str; 5] = ["BHD", "JOD", "KWD", "OMR", "TND"];

/// `total_amount` is in the currency's smallest unit; `XTR` is Telegram Stars.
fn price(currency: &str, amount: i64) -> String {
    if currency == "XTR" {
        return format!("⭐ {amount}");
    }
    let digits = if WHOLE.contains(&currency) { 0 } else if THOUSANDTHS.contains(&currency) { 3 } else { 2 };
    let unit = 10_i64.pow(digits);
    let (sign, abs) = (if amount < 0 { "-" } else { "" }, amount.unsigned_abs());
    match digits {
        0 => format!("{sign}{abs} {currency}"),
        _ => format!("{sign}{}.{:0w$} {currency}", abs / unit as u64, abs % unit as u64, w = digits as usize),
    }
}

fn invoice(m: &Message) -> Option<&tl::types::MessageMediaInvoice> {
    match &m.raw {
        tl::enums::Message::Message(tl::types::Message { media: Some(tl::enums::MessageMedia::Invoice(i)), .. }) => Some(i),
        _ => None,
    }
}

pub(super) fn is_invoice(m: &Message) -> bool {
    invoice(m).is_some()
}

/// The invoice as a card under the message.
pub(super) fn card(m: &Message) -> Option<WebPageInfo> {
    let i = invoice(m)?;
    let mut lines = vec![i.description.clone(), lf!("До сплати: {}", "Total: {}", price(&i.currency, i.total_amount))];
    if i.receipt_msg_id.is_some() {
        lines.push(l!("Оплачено", "Paid").to_owned());
    }
    if i.test {
        lines.push(l!("Тестовий рахунок", "Test invoice").to_owned());
    }
    Some(WebPageInfo { id: m.id() as i64, url: String::new(), site: l!("Рахунок", "Invoice").to_owned(), title: i.title.clone(), description: lines.join("\n"), photo: false, iv: false, card: CardKind::Invoice })
}

#[cfg(test)]
mod tests {
    use super::price;

    #[test]
    fn prices_by_currency() {
        assert_eq!(price("USD", 1250), "12.50 USD");
        assert_eq!(price("UAH", 5), "0.05 UAH");
        assert_eq!(price("JPY", 1250), "1250 JPY");
        assert_eq!(price("KWD", 1250), "1.250 KWD");
        assert_eq!(price("XTR", 50), "⭐ 50");
    }
}
