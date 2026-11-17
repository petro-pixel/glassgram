//! The boost box of a channel or supergroup, as in Telegram Desktop: the level with a progress bar,
//! "Boost" with one of our Premium slots (moving a used one here after picking it), the "Premium
//! needed" explanation without Premium, and for admins the boosters and the boost link.
//! Opened from chat info and from `t.me/boost/<name>` / `t.me/<name>?boost` links.

use super::*;
use crate::backend::{BoostSlot, BoostStatus, Booster};

/// What the box shows.
#[derive(Clone, PartialEq)]
enum Page {
    Main,
    /// Every slot is used: pick the ones to move here.
    Reassign(Vec<i32>),
    /// No Premium: Telegram Desktop's "Premium needed!".
    Premium,
}

struct BoostBox {
    peer: PeerId,
    title: String,
    channel: bool,
    /// Admins also see the boosters and the link.
    admin: bool,
    status: Option<BoostStatus>,
    page: Page,
    /// Waiting for the server after "Boost".
    busy: bool,
    error: Option<String>,
    /// Boosters: the total, those loaded, the next page's offset.
    boosters: Option<(i32, Vec<Booster>, String)>,
}

#[derive(Default)]
pub(super) struct Boosts {
    open: Option<BoostBox>,
    /// Our slots as last heard; `None` until loaded.
    slots: Option<Vec<BoostSlot>>,
}

/// The chat name of `t.me/boost/<name>` or `t.me/<name>?boost`.
pub(super) fn tme_boost(url: &str) -> Option<String> {
    let rest = url.split_once("://").map_or(url, |(_, r)| r);
    let rest = rest.strip_prefix("t.me/").or_else(|| rest.strip_prefix("telegram.me/"))?;
    let name = match rest.strip_prefix("boost/") {
        Some(r) => r.split(['/', '?', '#']).next()?,
        None => {
            let (name, query) = rest.split_once('?')?;
            query.split('&').any(|kv| kv == "boost" || kv.starts_with("boost=")).then_some(name.trim_end_matches('/'))?
        }
    };
    let ok = name.len() >= 4 && name.chars().all(|c| c.is_ascii_alphanumeric() || c == '_');
    ok.then(|| name.to_owned())
}

fn date(t: i64) -> String {
    jiff::Timestamp::from_second(t).map(|t| t.to_zoned(jiff::tz::TimeZone::system()).strftime("%d.%m.%Y").to_string()).unwrap_or_default()
}

/// "5 h 12 min" until `t`.
fn time_left(t: i64) -> String {
    let left = (t - jiff::Timestamp::now().as_second()).max(60);
    let (h, m) = (left / 3600, left % 3600 / 60);
    if h > 0 { lf!("{h} год {m} хв", "{h} h {m} min") } else { lf!("{m} хв", "{m} min") }
}

fn more_boosts(n: i32) -> String {
    lp!(n, ["ще {n} підсилення", "ще {n} підсилення", "ще {n} підсилень"], ["{n} more boost", "{n} more boosts"])
}

impl App {
    /// Opens the box; `admin` adds the boosters and the link.
    pub(super) fn open_boosts(&mut self, peer: PeerId, title: String, channel: bool, admin: bool) {
        self.boosts.open = Some(BoostBox { peer, title, channel, admin, status: None, page: Page::Main, busy: false, error: None, boosters: None });
        self.send(Cmd::LoadBoosts(peer));
        if admin {
            self.send(Cmd::LoadBoosters { peer, offset: String::new() });
        }
    }

    /// A boost link: the box opens once the chat is resolved (`Ev::BoostLink`).
    pub(super) fn open_boost_link(&mut self, name: String) {
        self.send(Cmd::OpenBoostLink(name));
    }

    pub(super) fn boosts_event(&mut self, ev: Ev) {
        match ev {
            Ev::BoostLink(chat) => {
                let admin = self.info_details_mut(chat.id).is_some_and(|d| d.my_rights.is_some());
                self.open_boosts(chat.id, chat.title, chat.kind == ChatKind::Channel, admin);
            }
            Ev::MyBoosts(slots) => self.boosts.slots = Some(slots),
            ev => {
                let Some(b) = &mut self.boosts.open else { return };
                match ev {
                    Ev::BoostStatus { peer, status } if peer == b.peer => b.status = Some(status),
                    Ev::Boosted(peer) if peer == b.peer => {
                        b.busy = false;
                        b.error = None;
                        b.page = Page::Main;
                        let title = b.title.clone();
                        self.toast = Some((lf!("Ви підсилили {title}!", "You boosted {title}!"), 0.0, false));
                    }
                    Ev::BoostFailed { peer, error } if peer == b.peer => {
                        b.busy = false;
                        b.error = Some(error);
                    }
                    Ev::Boosters { peer, count, list, next, append } if peer == b.peer => match (&mut b.boosters, append) {
                        (Some((total, have, offset)), true) => {
                            *total = count;
                            have.extend(list);
                            *offset = next;
                        }
                        _ => b.boosters = Some((count, list, next)),
                    },
                    _ => {}
                }
            }
        }
    }

    /// "Boost": a free slot goes straight away; with all of them used, pick which to move here.
    fn boost_click(&mut self) {
        let premium = self.profile.premium;
        let slots = self.boosts.slots.clone();
        let Some(b) = &mut self.boosts.open else { return };
        b.error = None;
        if !premium {
            b.page = Page::Premium;
            return;
        }
        let Some(slots) = slots else { return };
        if let Some(free) = slots.iter().find(|s| s.peer.is_none()) {
            b.busy = true;
            let cmd = Cmd::ApplyBoost { peer: b.peer, slots: vec![free.slot] };
            self.send(cmd);
        } else if slots.iter().any(|s| s.peer.as_ref().is_some_and(|p| p.0 != b.peer)) {
            b.page = Page::Reassign(Vec::new());
        } else if slots.is_empty() {
            b.page = Page::Premium;
        } else {
            b.error = Some(l!("Усі ваші підсилення вже віддані цьому чату.", "All your boosts already go to this chat.").to_owned());
        }
    }

    pub(super) fn boosts_view(&mut self, ui: &mut Ui) {
        let Some(b) = &self.boosts.open else { return };
        let screen = ui.max_rect();
        ui.painter().rect_filled(screen, 0.0, Color32::from_black_alpha(90));
        ui.interact(screen, ui.scope_id().with("boosts-bg"), Sense::click());
        let h = match b.page {
            Page::Premium => 240.0,
            Page::Reassign(_) => 440.0,
            Page::Main if b.admin => 600.0,
            Page::Main => 340.0,
        };
        let rect = Rect::from_center_size(screen.center(), vec2(screen.width().min(440.0) - 20.0, (screen.height() - 60.0).min(h)));
        glass::glass(ui.painter(), rect, Glass::panel(26.0).with_shadow());
        let inner = rect.shrink(18.0);
        let title = match (&b.page, &b.status) {
            (Page::Reassign(_), _) => l!("Перенести підсилення", "Reassign boosts").to_owned(),
            (Page::Premium, _) => l!("Потрібен Premium!", "Premium needed!").to_owned(),
            (_, Some(s)) if s.boosted => if b.channel { l!("Ви підсилили канал", "You boosted this channel") } else { l!("Ви підсилили групу", "You boosted this group") }.to_owned(),
            (_, Some(s)) if s.next_level_boosts.is_none() => l!("Найвищий рівень", "Maximum level reached").to_owned(),
            (_, Some(s)) if s.level == 0 => l!("Увімкнути історії", "Enable stories").to_owned(),
            _ if b.channel => l!("Підсилити канал", "Boost channel").to_owned(),
            _ => l!("Підсилити групу", "Boost group").to_owned(),
        };
        let mut x = inner.left();
        let back = b.page != Page::Main;
        let mut go_back = false;
        if back {
            let r = Rect::from_min_size(pos2(inner.left() - 4.0, inner.top() - 4.0), vec2(32.0, 32.0));
            go_back = glass::icon_button(ui, r, "boost-back", Icon::ChevronLeft, false, true).on_hover_text(l!("Назад", "Back")).clicked();
            x += 36.0;
        }
        ui.painter().galley(pos2(x, inner.top() + 2.0), elided(ui, &title, 17.0, ink(), inner.right() - x - 40.0), ink());
        let close = Rect::from_min_size(pos2(inner.right() - 32.0, inner.top() - 4.0), vec2(32.0, 32.0));
        let esc = ui.input_mut(|i| i.consume_key(egui::Modifiers::NONE, Key::Escape));
        if glass::icon_button(ui, close, "boost-close", Icon::Close, false, true).clicked() || (esc && !back) {
            self.boosts.open = None;
            return;
        }
        if go_back || esc {
            if let Some(b) = &mut self.boosts.open {
                b.page = Page::Main;
            }
            return;
        }
        let body = Rect::from_min_max(pos2(inner.left() - 4.0, inner.top() + 40.0), pos2(inner.right() + 4.0, inner.bottom()));
        let mut child = ui.new_child(UiBuilder::new().max_rect(body));
        child.set_clip_rect(body);
        let page = b.page.clone();
        match page {
            Page::Main => self.boost_main(&mut child),
            Page::Reassign(ticked) => self.boost_reassign(&mut child, ticked),
            Page::Premium => self.boost_premium(&mut child),
        }
    }

    fn boost_main(&mut self, ui: &mut Ui) {
        let Some(b) = &self.boosts.open else { return };
        let mut boost = false;
        let mut more: Option<String> = None;
        let mut copy: Option<String> = None;
        let mut open: Option<ChatInfo> = None;
        egui::ScrollArea::vertical().id_salt("boosts").auto_shrink(false).show(ui, |ui| {
            ui.spacing_mut().item_spacing.y = 6.0;
            let w = ui.available_width() - 8.0;
            let Some(s) = &b.status else {
                ui.add_space(24.0);
                ui.vertical_centered(|ui| ui.add(egui::Spinner::new().color(ink())));
                return;
            };
            ui.add_space(30.0);
            level_bar(ui, w, s);
            ui.add_space(8.0);
            let text = match s.next_level_boosts {
                None => lf!("{} досяг найвищого рівня.", "{} reached the maximum level.", b.title),
                Some(next) if s.level == 0 => lf!("{}: потрібно {}, щоб публікувати історії.", "{} needs {} to enable posting stories.", b.title, more_boosts(next - s.boosts)),
                Some(next) => lf!("{}: потрібно {}, щоб відкрити нові можливості.", "{} needs {} to unlock new features.", b.title, more_boosts(next - s.boosts)),
            };
            ui.add(egui::Label::new(egui::RichText::new(text).color(ink()).size(14.0)).wrap());
            if s.boosted {
                let you = if b.channel { l!("Ви підсилюєте цей канал. Дякуємо!", "You are boosting this channel. Thank you!") } else { l!("Ви підсилюєте цю групу. Дякуємо!", "You are boosting this group. Thank you!") };
                ui.label(egui::RichText::new(you).color(ink_dim()).size(12.5));
            }
            ui.add_space(6.0);
            let r = ui.allocate_exact_size(vec2(w, 40.0), Sense::hover()).0;
            let label = match (s.boosted, b.channel) {
                (true, _) => l!("Підсилити ще раз", "Boost again"),
                (false, true) => l!("Підсилити канал", "Boost channel"),
                (false, false) => l!("Підсилити групу", "Boost group"),
            };
            boost = glass::button(ui, r, "boost-apply", label, true, !b.busy).clicked();
            if let Some(e) = &b.error {
                ui.label(egui::RichText::new(e).color(glass::DANGER).size(12.5));
            }
            if !b.admin {
                return;
            }
            // The admin part: Telegram Desktop's "Boosts" page with the boosters and the link.
            ui.add_space(6.0);
            let (count, list, next) = b.boosters.as_ref().map_or((0, &[][..], ""), |(c, l, n)| (*c, &l[..], n.as_str()));
            section_title(ui, &lp!(count, ["{n} підсилення", "{n} підсилення", "{n} підсилень"], ["{n} boost", "{n} boosts"]));
            if b.boosters.is_some() && list.is_empty() {
                let none = if b.channel { l!("Ваш канал поки ніхто не підсилив.", "No one has boosted your channel yet.") } else { l!("Вашу групу поки ніхто не підсилив.", "No one has boosted your group yet.") };
                ui.label(egui::RichText::new(none).color(ink_dim()).size(12.5));
            }
            for (k, x) in list.iter().enumerate() {
                if booster_row(ui, &self.textures, k, x).clicked()
                    && let Some(u) = &x.user
                {
                    open = Some(u.clone());
                }
            }
            if !next.is_empty() {
                let r = ui.allocate_exact_size(vec2(w, 34.0), Sense::hover()).0;
                if glass::button(ui, r, "boosters-more", l!("Показати ще", "Show more"), false, true).clicked() {
                    more = Some(next.to_owned());
                }
            }
            section_title(ui, l!("Посилання для підсилення", "Link for boosting"));
            let (row, _) = ui.allocate_exact_size(vec2(w, 40.0), Sense::hover());
            ui.painter().rect_filled(row, 20.0, glass::veil(12));
            ui.painter().galley(pos2(row.left() + 14.0, row.center().y - 9.0), elided(ui, &s.url, 14.0, ink(), row.width() - 70.0), ink());
            let c = Rect::from_center_size(pos2(row.right() - 22.0, row.center().y), vec2(34.0, 34.0));
            if glass::icon_button(ui, c, "boost-link-copy", Icon::Copy, false, true).on_hover_text(l!("Копіювати посилання", "Copy link")).clicked() {
                copy = Some(s.url.clone());
            }
            let about = if b.channel {
                l!("Поділіться цим посиланням з підписниками, щоб отримати більше підсилень.", "Share this link with your subscribers to get more boosts.")
            } else {
                l!("Поділіться цим посиланням з учасниками, щоб отримати більше підсилень.", "Share this link with your members to get more boosts.")
            };
            ui.add(egui::Label::new(egui::RichText::new(about).color(ink_dim()).size(12.5)).wrap());
        });
        let peer = b.peer;
        if boost {
            self.boost_click();
        }
        if let Some(offset) = more {
            self.send(Cmd::LoadBoosters { peer, offset });
        }
        if let Some(url) = copy {
            self.ctx.copy_text(url);
            self.toast = Some((l!("Посилання скопійовано", "Link copied").into(), 0.0, false));
        }
        if let Some(c) = open {
            self.boosts.open = None;
            if !self.chats.iter().any(|x| x.id == c.id) {
                self.chats.push(c.clone());
            }
            self.open_chat(c.id);
        }
    }

    /// Our used slots with what they boost; ticked ones move here.
    fn boost_reassign(&mut self, ui: &mut Ui, mut ticked: Vec<i32>) {
        let Some(b) = &self.boosts.open else { return };
        let slots = self.boosts.slots.clone().unwrap_or_default();
        let now = jiff::Timestamp::now().as_second();
        let area = ui.max_rect();
        let buttons = Rect::from_min_max(pos2(area.left() + 4.0, area.bottom() - 36.0), pos2(area.right() - 4.0, area.bottom()));
        let body = Rect::from_min_max(area.min, pos2(area.right(), buttons.top() - 10.0));
        let mut child = ui.new_child(UiBuilder::new().max_rect(body));
        child.set_clip_rect(body);
        egui::ScrollArea::vertical().id_salt("boost-reassign").auto_shrink(false).show(&mut child, |ui| {
            ui.spacing_mut().item_spacing.y = 4.0;
            let text = lf!(
                "Щоб підсилити {}, перенесіть сюди одне з ваших підсилень з іншого чату.",
                "To boost {}, reassign one of your boosts from another chat.",
                b.title
            );
            ui.add(egui::Label::new(egui::RichText::new(text).color(ink()).size(14.0)).wrap());
            ui.add_space(6.0);
            for s in slots.iter().filter(|s| s.peer.as_ref().is_some_and(|p| p.0 != b.peer)) {
                let Some((id, name)) = &s.peer else { continue };
                let wait = s.cooldown_until > now;
                let (row, resp) = ui.allocate_exact_size(vec2(ui.available_width() - 8.0, 50.0), if wait { Sense::hover() } else { Sense::click() });
                let on = ticked.contains(&s.slot);
                if resp.clicked() {
                    if on {
                        ticked.retain(|x| *x != s.slot);
                    } else {
                        ticked.push(s.slot);
                    }
                }
                let p = ui.painter();
                if resp.hovered() && !wait {
                    p.rect_filled(row, 12.0, glass::veil(12));
                    ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
                }
                glass::avatar(p, pos2(row.left() + 26.0, row.center().y), 18.0, name, id.bot_api_dialog_id_unchecked(), None, self.textures.get(&ImgKey::Avatar(*id)));
                let color = if wait { ink_dim() } else { ink() };
                p.galley(pos2(row.left() + 54.0, row.top() + 7.0), elided(ui, name, 14.5, color, row.width() - 100.0), color);
                let sub = if wait { lf!("Можна перенести через {}", "Available in {}", time_left(s.cooldown_until)) } else { lf!("Підсилення діє до {}", "Boost expires on {}", date(s.expires)) };
                p.galley(pos2(row.left() + 54.0, row.top() + 27.0), elided(ui, &sub, 12.5, ink_dim(), row.width() - 100.0), ink_dim());
                let mark = Rect::from_center_size(pos2(row.right() - 20.0, row.center().y), vec2(22.0, 22.0));
                if on {
                    p.circle_filled(mark.center(), 11.0, ACCENT);
                    icons::draw(p, mark.shrink(4.0), Icon::Check, glass::ON_DARK);
                } else if !wait {
                    p.circle_stroke(mark.center(), 10.0, egui::Stroke::new(1.5, ink_dim()));
                }
            }
            ui.add_space(4.0);
            ui.add(egui::Label::new(egui::RichText::new(l!("Перенесене підсилення зникне з того чату. Переносити можна раз на добу.", "A reassigned boost leaves that chat. Each boost can move once a day.")).color(ink_dim()).size(12.5)).wrap());
        });
        let half = (buttons.width() - 8.0) / 2.0;
        let cancel = glass::button(ui, Rect::from_min_size(buttons.min, vec2(half, buttons.height())), "boost-reassign-cancel", l!("Скасувати", "Cancel"), false, true).clicked();
        let ok = glass::button(ui, Rect::from_min_size(pos2(buttons.right() - half, buttons.top()), vec2(half, buttons.height())), "boost-reassign-ok", l!("Перенести", "Reassign"), true, !ticked.is_empty())
            .clicked();
        let peer = b.peer;
        let Some(b) = &mut self.boosts.open else { return };
        if cancel {
            b.page = Page::Main;
        } else if ok && !ticked.is_empty() {
            b.page = Page::Main;
            b.busy = true;
            self.send(Cmd::ApplyBoost { peer, slots: ticked });
        } else {
            b.page = Page::Reassign(ticked);
        }
    }

    fn boost_premium(&mut self, ui: &mut Ui) {
        let Some(b) = &self.boosts.open else { return };
        let area = ui.max_rect();
        let text = if b.channel {
            l!("Підсилювати канали можуть лише передплатники Telegram Premium. Хочете оформити Telegram Premium?", "Only Telegram Premium subscribers can boost channels. Do you want to subscribe to Telegram Premium?")
        } else {
            l!("Підсилювати групи можуть лише передплатники Telegram Premium. Хочете оформити Telegram Premium?", "Only Telegram Premium subscribers can boost groups. Do you want to subscribe to Telegram Premium?")
        };
        let mut top = ui.new_child(UiBuilder::new().max_rect(area.shrink2(vec2(4.0, 0.0))));
        top.add(egui::Label::new(egui::RichText::new(text).color(ink()).size(14.0)).wrap());
        let buttons = Rect::from_min_max(pos2(area.left() + 4.0, area.bottom() - 36.0), pos2(area.right() - 4.0, area.bottom()));
        let half = (buttons.width() - 8.0) / 2.0;
        let cancel = glass::button(ui, Rect::from_min_size(buttons.min, vec2(half, buttons.height())), "boost-premium-cancel", l!("Скасувати", "Cancel"), false, true).clicked();
        let more = glass::button(ui, Rect::from_min_size(pos2(buttons.right() - half, buttons.top()), vec2(half, buttons.height())), "boost-premium-ok", "Telegram Premium", true, true).clicked();
        if more {
            self.open_link("https://telegram.org/faq_premium");
        }
        if (cancel || more)
            && let Some(b) = &mut self.boosts.open
        {
            b.page = Page::Main;
        }
    }
}

/// "Level N" → "Level N+1" with the boosts so far in a bubble over the bar, as in Telegram Desktop.
fn level_bar(ui: &mut Ui, w: f32, s: &BoostStatus) {
    let (r, _) = ui.allocate_exact_size(vec2(w, 30.0), Sense::hover());
    let frac = match s.next_level_boosts {
        Some(next) if next > s.level_boosts => ((s.boosts - s.level_boosts) as f32 / (next - s.level_boosts) as f32).clamp(0.0, 1.0),
        _ => 1.0,
    };
    let p = ui.painter();
    p.rect_filled(r, 15.0, glass::veil(22));
    let fill = Rect::from_min_size(r.min, vec2((r.width() * frac).max(30.0), r.height()));
    glass::glass(p, fill, glass::accent_glass(fill, true, 0.0));
    let font = FontId::proportional(13.0);
    p.text(pos2(r.left() + 12.0, r.center().y), Align2::LEFT_CENTER, lf!("Рівень {}", "Level {}", s.level), font.clone(), glass::ON_DARK);
    if s.next_level_boosts.is_some() {
        let color = if frac > 0.85 { glass::ON_DARK } else { ink() };
        p.text(pos2(r.right() - 12.0, r.center().y), Align2::RIGHT_CENTER, lf!("Рівень {}", "Level {}", s.level + 1), font, color);
    }
    // The bubble with the count sits over the end of the filled part.
    let label = s.boosts.to_string();
    let g = p.layout_no_wrap(label.clone(), FontId::proportional(14.0), glass::ON_DARK);
    let bw = g.size().x + 36.0;
    let cx = (r.left() + r.width() * frac).clamp(r.left() + bw / 2.0, r.right() - bw / 2.0);
    let bubble = Rect::from_center_size(pos2(cx, r.top() - 18.0), vec2(bw, 26.0));
    glass::glass(p, bubble, glass::accent_glass(bubble, true, 0.0));
    icons::draw(p, Rect::from_center_size(pos2(bubble.left() + 15.0, bubble.center().y), vec2(14.0, 14.0)), Icon::Zap, glass::ON_DARK);
    p.galley(pos2(bubble.left() + 26.0, bubble.center().y - g.size().y / 2.0), g, glass::ON_DARK);
}

fn booster_row(ui: &mut Ui, textures: &super::textures::Textures, k: usize, x: &Booster) -> egui::Response {
    let w = ui.available_width() - 8.0;
    let (rect, resp) = ui.allocate_exact_size(vec2(w, 48.0), if x.user.is_some() { Sense::click() } else { Sense::hover() });
    let p = ui.painter();
    if resp.hovered() && x.user.is_some() {
        p.rect_filled(rect, 14.0, glass::veil(12));
        ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
    }
    let (name, seed, tex) = match &x.user {
        Some(u) => (u.title.clone(), u.id.bot_api_dialog_id_unchecked(), textures.get(&ImgKey::Avatar(u.id))),
        None => (l!("Не отримано", "Unclaimed").to_owned(), k as i64, None),
    };
    glass::avatar(p, pos2(rect.left() + 24.0, rect.center().y), 18.0, &name, seed, None, tex);
    let badge = (x.multiplier > 1).then(|| format!("×{}", x.multiplier));
    let text_w = rect.width() - 70.0 - if badge.is_some() { 44.0 } else { 0.0 };
    p.galley(pos2(rect.left() + 52.0, rect.top() + 6.0), elided(ui, &name, 14.0, ink(), text_w), ink());
    let sub = match (x.gift, x.giveaway) {
        (_, true) => lf!("Розіграш · діє до {}", "Giveaway · expires on {}", date(x.expires)),
        (true, false) => lf!("Подарунок · діє до {}", "Gift · expires on {}", date(x.expires)),
        _ => lf!("Підсилення діє до {}", "Boost expires on {}", date(x.expires)),
    };
    p.galley(pos2(rect.left() + 52.0, rect.top() + 26.0), elided(ui, &sub, 12.5, ink_dim(), text_w), ink_dim());
    if let Some(b) = badge {
        let chip = Rect::from_center_size(pos2(rect.right() - 26.0, rect.center().y), vec2(40.0, 22.0));
        p.rect_filled(chip, 11.0, glass::veil(24));
        p.text(chip.center(), Align2::CENTER_CENTER, b, FontId::proportional(12.5), ink());
    }
    resp
}

#[cfg(test)]
mod tests {
    use super::tme_boost;

    #[test]
    fn boost_links() {
        assert_eq!(tme_boost("https://t.me/boost/tech_news"), Some("tech_news".into()));
        assert_eq!(tme_boost("t.me/boost/tech_news?x=1"), Some("tech_news".into()));
        assert_eq!(tme_boost("https://t.me/tech_news?boost"), Some("tech_news".into()));
        assert_eq!(tme_boost("https://t.me/tech_news?start=1&boost"), Some("tech_news".into()));
        assert_eq!(tme_boost("https://t.me/tech_news"), None);
        assert_eq!(tme_boost("https://t.me/tech_news?booster"), None);
        assert_eq!(tme_boost("https://example.com/boost/tech_news"), None);
    }
}
