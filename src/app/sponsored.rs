//! Sponsored messages at the end of channel history, as in Telegram Desktop:
//! an incoming bubble marked "Ad", "About this ad" and reporting.

use std::collections::{HashMap, HashSet};

use eframe::egui::{self, Align2, FontId, Key, Rect, Sense, TextureHandle, Ui, UiBuilder, pos2, vec2};
use grammers_client::session::types::PeerId;

use super::{Action, App, elided};
use crate::backend::{ChatKind, Cmd, Ev, Sponsored, SponsoredReport};
use crate::format;
use crate::glass::{self, ACCENT, Glass, ink, ink_dim};
use crate::i18n::l;
use crate::icons::Icon;

/// Telegram asks clients not to request ads for the same chat more often than this.
const REFRESH: f64 = 300.0;
const BUTTON_H: f32 = 36.0;
const THUMB: f32 = 56.0;
const ADS_INFO: &str = "https://ads.telegram.org";

/// Sponsored message state; kept in one field of `App`.
#[derive(Default)]
pub(super) struct Ads {
    /// Channel → when the ads were fetched (UI time) and the ads.
    by_peer: HashMap<PeerId, (f64, Vec<Sponsored>)>,
    /// Views already reported in this session.
    viewed: HashSet<Vec<u8>>,
    popup: Option<Popup>,
}

enum Popup {
    About(PeerId, Sponsored),
    /// `options: None` — waiting for the server.
    Report { peer: PeerId, random_id: Vec<u8>, title: String, options: Option<Vec<(String, Vec<u8>)>> },
}

/// What happened to the card in this frame.
pub(super) enum AdAction {
    Open,
    About,
    Report,
}

impl Ads {
    /// The ad shown at the end of the channel's history.
    pub(super) fn first(&self, peer: PeerId) -> Option<&Sponsored> {
        self.by_peer.get(&peer)?.1.first()
    }
}

/// Incoming bubble with the "Ad" label, title, text and the action button.
/// Returns whether it is on screen (for the view report) and what was clicked.
pub(super) fn card(ui: &mut Ui, ad: &Sponsored, tex: Option<&TextureHandle>) -> (bool, Option<AdAction>) {
    let avail = ui.available_width();
    let max_w = (avail * 0.72).min(460.0);
    let pad = vec2(14.0, 9.0);
    let thumb = if tex.is_some() { THUMB + 10.0 } else { 0.0 };
    let text_w = (max_w - pad.x * 2.0 - thumb).max(80.0);
    let p = ui.painter().clone();

    let label = if ad.recommended { l!("Рекомендовано", "Recommended") } else { l!("Реклама", "Ad") };
    let label = elided(ui, label, 12.5, ACCENT, text_w);
    let what = p.layout_no_wrap(l!("що це?", "what's this?").to_owned(), FontId::proportional(12.5), ink_dim());
    let title = (!ad.title.is_empty()).then(|| elided(ui, &ad.title, 14.5, ink(), text_w));
    let body = (!ad.text.is_empty()).then(|| p.layout_job(format::layout(&ad.text, &ad.spans, glass::msg_font(), ink(), glass::link(false), text_w, true)));

    let mut block = vec2(label.size().x + 8.0 + what.size().x, label.size().y + 4.0);
    if let Some(t) = &title {
        block.x = block.x.max(t.size().x);
        block.y += t.size().y + 2.0;
    }
    if let Some(b) = &body {
        block.x = block.x.max(b.size().x);
        block.y += b.size().y;
    }
    block.x += thumb;
    block.y = block.y.max(if tex.is_some() { THUMB } else { 0.0 });
    let has_button = !ad.button.is_empty();
    if has_button {
        block.x = block.x.max(200.0);
        block.y += 8.0 + BUTTON_H;
    }
    let size = block + pad * 2.0;

    let (row, resp) = ui.allocate_exact_size(vec2(avail, size.y + 6.0), Sense::click());
    let rect = Rect::from_min_size(pos2(row.left() + 10.0, row.top() + 3.0), size);
    glass::glass(&p, rect, Glass::tinted(18.0, glass::bubble_in().0, glass::bubble_in().1).frosted(1.0));

    let mut cursor = rect.min + pad;
    let label_h = label.size().y;
    let what_rect = Rect::from_min_size(pos2(cursor.x + label.size().x + 8.0, cursor.y), what.size());
    p.galley(cursor, label, ACCENT);
    p.galley(what_rect.min, what, ink_dim());
    cursor.y += label_h + 4.0;
    if let Some(t) = title {
        let h = t.size().y;
        p.galley(cursor, t, ink());
        cursor.y += h + 2.0;
    }
    if let Some(b) = body {
        p.galley(cursor, b, ink());
    }
    if let Some(t) = tex {
        let r = Rect::from_min_size(pos2(rect.right() - pad.x - THUMB, rect.top() + pad.y), vec2(THUMB, THUMB));
        glass::image_rounded(&p, r, 10.0, t.id(), t.size_vec2());
    }
    // A flat strip, not a glass button: no glass on glass.
    let button = has_button.then(|| Rect::from_min_max(pos2(rect.left() + pad.x, rect.bottom() - pad.y - BUTTON_H), rect.max - pad));
    if let Some(b) = button {
        let hovered = resp.hover_pos().is_some_and(|pos| b.contains(pos));
        p.rect_filled(b, 10.0, glass::veil(if hovered { 30 } else { 16 }));
        p.text(b.center(), Align2::CENTER_CENTER, &ad.button, FontId::proportional(14.0), ACCENT);
    }

    let visible = ui.is_rect_visible(rect);
    let mut action = None;
    if resp.hover_pos().is_some_and(|pos| rect.contains(pos)) {
        ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
    }
    if resp.clicked() {
        action = Some(if resp.interact_pointer_pos().is_some_and(|pos| what_rect.expand(4.0).contains(pos)) { AdAction::About } else { AdAction::Open });
    }
    resp.context_menu(|ui| {
        if glass::menu_button(ui, Icon::Info, l!("Про цю рекламу", "About this ad")).clicked() {
            action = Some(AdAction::About);
        }
        if ad.can_report && glass::menu_button(ui, Icon::Flag, l!("Поскаржитися", "Report")).clicked() {
            action = Some(AdAction::Report);
        }
    });
    (visible, action)
}

impl App {
    /// Ask for ads when a channel opens, unless the last answer is still fresh.
    pub(super) fn open_sponsored(&mut self, peer: PeerId) {
        let channel = self.chats.iter().any(|c| c.id == peer && c.kind == ChatKind::Channel);
        let now = self.ctx.input(|i| i.time);
        let fresh = self.ads.by_peer.get(&peer).is_some_and(|(t, _)| now - t < REFRESH);
        if channel && !fresh {
            // Mark the request at once so reopening the chat doesn't repeat it.
            self.ads.by_peer.entry(peer).or_default().0 = now;
            self.send(Cmd::LoadSponsored(peer));
        }
    }

    pub(super) fn sponsored_event(&mut self, ev: Ev) {
        match ev {
            Ev::Sponsored { peer, ads } => {
                let now = self.ctx.input(|i| i.time);
                self.ads.by_peer.insert(peer, (now, ads));
            }
            Ev::SponsoredReport { peer, random_id, result } => {
                let waiting = matches!(&self.ads.popup, Some(Popup::Report { random_id: r, .. }) if *r == random_id);
                match result {
                    SponsoredReport::Choose { title, options } => {
                        if waiting {
                            self.ads.popup = Some(Popup::Report { peer, random_id, title, options: Some(options) });
                        }
                    }
                    SponsoredReport::Reported => {
                        self.drop_ads(peer, Some(&random_id));
                        self.toast = Some((l!("Дякуємо! Модератори Telegram розглянуть скаргу.", "Thank you! Telegram moderators will review your report.").into(), 0.0, false));
                    }
                    SponsoredReport::Hidden => {
                        self.drop_ads(peer, None);
                        self.toast = Some((l!("Рекламу в цьому каналі приховано", "Ads are hidden in this channel").into(), 0.0, false));
                    }
                }
            }
            _ => {}
        }
    }

    /// Remove one ad (or all of the chat's) after a report.
    fn drop_ads(&mut self, peer: PeerId, random_id: Option<&[u8]>) {
        if let Some((_, list)) = self.ads.by_peer.get_mut(&peer) {
            list.retain(|a| random_id.is_some_and(|r| a.random_id != r));
        }
        if matches!(&self.ads.popup, Some(Popup::Report { peer: p, .. } | Popup::About(p, _)) if *p == peer) {
            self.ads.popup = None;
        }
    }

    /// The card reached the screen: the server counts each ad once per session.
    pub(super) fn ad_seen(&mut self, random_id: &[u8]) {
        if self.ads.viewed.insert(random_id.to_vec()) {
            self.send(Cmd::ViewSponsored(random_id.to_vec()));
        }
    }

    pub(super) fn ad_action(&mut self, peer: PeerId, action: AdAction) {
        let Some(ad) = self.ads.first(peer).cloned() else { return };
        match action {
            AdAction::Open => {
                self.send(Cmd::ClickSponsored(ad.random_id.clone()));
                if !ad.url.is_empty() {
                    self.apply(peer, Action::Open(ad.url));
                }
            }
            AdAction::About => self.ads.popup = Some(Popup::About(peer, ad)),
            AdAction::Report => self.report_ad(peer, ad.random_id, Vec::new()),
        }
    }

    fn report_ad(&mut self, peer: PeerId, random_id: Vec<u8>, option: Vec<u8>) {
        let title = match &self.ads.popup {
            Some(Popup::Report { title, .. }) => title.clone(),
            _ => l!("Поскаржитися на рекламу", "Report ad").to_owned(),
        };
        self.ads.popup = Some(Popup::Report { peer, random_id: random_id.clone(), title, options: None });
        self.send(Cmd::ReportSponsored { peer, random_id, option });
    }

    /// "About this ad" and the report reasons, centred over the chat; Esc closes them.
    pub(super) fn sponsored_overlay(&mut self, ui: &mut Ui, peer: PeerId, rect: Rect) {
        if matches!(&self.ads.popup, Some(Popup::Report { peer: p, .. } | Popup::About(p, _)) if *p != peer) {
            self.ads.popup = None;
        }
        let Some(popup) = &self.ads.popup else { return };
        if ui.input_mut(|i| i.consume_key(egui::Modifiers::NONE, Key::Escape)) {
            self.ads.popup = None;
            return;
        }
        let panel = Rect::from_center_size(rect.center(), vec2(rect.width().min(380.0), (rect.height() - 120.0).min(420.0)));
        glass::glass(ui.painter(), panel, Glass::panel(24.0).with_shadow());
        let title = match popup {
            Popup::About(..) => l!("Про цю рекламу", "About this ad").to_owned(),
            Popup::Report { title, .. } => title.clone(),
        };
        let head_w = panel.width() - 70.0;
        ui.painter().galley(pos2(panel.left() + 18.0, panel.top() + 16.0), elided(ui, &title, 15.0, ink(), head_w), ink());
        let close = Rect::from_center_size(pos2(panel.right() - 26.0, panel.top() + 26.0), vec2(32.0, 32.0));
        if glass::icon_button(ui, close, "ad-close", Icon::Close, false, true).clicked() {
            self.ads.popup = None;
            return;
        }
        let footer = if matches!(popup, Popup::About(..)) { 52.0 } else { 0.0 };
        let body = Rect::from_min_max(pos2(panel.left() + 18.0, panel.top() + 52.0), pos2(panel.right() - 18.0, panel.bottom() - 8.0 - footer));
        let mut child = ui.new_child(UiBuilder::new().max_rect(body));
        let mut picked = None;
        match popup {
            Popup::About(_, ad) => {
                egui::ScrollArea::vertical().id_salt("ad-about").auto_shrink(false).show(&mut child, |ui| {
                    let text = |ui: &mut Ui, s: &str, size: f32, color| {
                        ui.label(egui::RichText::new(s).size(size).color(color));
                    };
                    text(
                        ui,
                        l!(
                            "На відміну від інших застосунків, Telegram не використовує ваші особисті дані для показу реклами. Спонсоровані повідомлення залежать лише від тематики публічного каналу, в якому їх показують.",
                            "Unlike other apps, Telegram never uses your private data to target ads. Sponsored messages are based solely on the topic of the public channel in which they are shown."
                        ),
                        14.0,
                        ink(),
                    );
                    for (head, value) in [(l!("Рекламодавець", "Advertiser"), &ad.sponsor_info), (l!("Додатково", "Additional info"), &ad.additional_info)] {
                        if !value.is_empty() {
                            ui.add_space(12.0);
                            text(ui, head, 12.5, ACCENT);
                            text(ui, value, 14.0, ink());
                        }
                    }
                });
            }
            Popup::Report { options: None, .. } => {
                child.add_space(24.0);
                child.vertical_centered(|ui| ui.add(egui::Spinner::new().color(ink())));
            }
            Popup::Report { options: Some(list), .. } => {
                egui::ScrollArea::vertical().id_salt("ad-report").auto_shrink(false).show(&mut child, |ui| {
                    for (text, option) in list {
                        let w = ui.available_width();
                        let (r, resp) = ui.allocate_exact_size(vec2(w, 44.0), Sense::click());
                        if resp.hovered() {
                            ui.painter().rect_filled(r, 12.0, glass::veil(18));
                            ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
                        }
                        ui.painter().galley(pos2(r.left() + 10.0, r.center().y - 9.0), elided(ui, text, 14.0, ink(), w - 20.0), ink());
                        if resp.clicked() {
                            picked = Some(option.clone());
                        }
                    }
                });
            }
        }
        if let Popup::About(_, ad) = popup {
            let can_report = ad.can_report;
            let half = (panel.width() - 36.0 - 8.0) / 2.0;
            let left = Rect::from_min_size(pos2(panel.left() + 18.0, panel.bottom() - 50.0), vec2(if can_report { half } else { panel.width() - 36.0 }, 38.0));
            if glass::button(ui, left, "ad-more", l!("Докладніше", "Learn more"), false, true).clicked() {
                crate::platform::open(ADS_INFO);
            }
            let right = Rect::from_min_size(pos2(left.right() + 8.0, left.top()), vec2(half, 38.0));
            if can_report && glass::button(ui, right, "ad-report", l!("Поскаржитися", "Report"), false, true).clicked() {
                let random_id = ad.random_id.clone();
                self.ads.popup = None;
                self.report_ad(peer, random_id, Vec::new());
                return;
            }
        }
        if let (Some(option), Some(Popup::Report { random_id, .. })) = (picked, &self.ads.popup) {
            let random_id = random_id.clone();
            self.report_ad(peer, random_id, option);
        }
    }
}
