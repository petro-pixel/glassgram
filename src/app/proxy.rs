//! Settings → Advanced → "Proxy", as in Telegram Desktop: saved SOCKS5 and MTProto proxies with
//! their ping, one click switches to another (or to none) without a restart, a pasted link or a
//! `t.me/proxy` link from a message adds one. The connection itself is `backend/proxy.rs`.

use super::*;
use crate::backend::proxy::Proxy;

#[derive(Default)]
pub(super) struct Proxies {
    /// The link being typed or pasted.
    add: String,
    /// Each proxy's last check: `None` while it runs.
    status: HashMap<String, Option<Result<u32, String>>>,
}

impl App {
    pub(super) fn proxy_section(&mut self, ui: &mut Ui) {
        section_title(ui, l!("Проксі", "Proxy"));
        // Checked once per settings visit, as Telegram Desktop pings its list when opened.
        for link in self.settings.proxies.clone() {
            if !self.proxies.status.contains_key(&link) {
                self.proxies.status.insert(link.clone(), None);
                self.send(Cmd::CheckProxy(link));
            }
        }
        let none = self.settings.proxy.is_empty();
        let r = ui.allocate_exact_size(vec2(ui.available_width(), 32.0), Sense::hover()).0;
        if glass::button(ui, r, "proxy-none", l!("Без проксі", "No proxy"), none, true).clicked() && !none {
            self.use_proxy(String::new());
        }
        let mut remove = None;
        for (k, link) in self.settings.proxies.clone().into_iter().enumerate() {
            let on = self.settings.proxy == link;
            let row = ui.allocate_exact_size(vec2(ui.available_width(), 44.0), Sense::hover()).0;
            let pick = Rect::from_min_max(row.min, pos2(row.right() - 40.0, row.bottom()));
            if glass::button(ui, pick, ("proxy", k), "", on, true).clicked() && !on {
                self.use_proxy(link.clone());
            }
            let (title, kind) = match Proxy::parse(&link) {
                Ok(p) => (p.address(), p.kind()),
                Err(_) => (link.clone(), "?"),
            };
            let (state, color) = match self.proxies.status.get(&link) {
                Some(Some(Ok(ms))) => (lf!("{kind} · доступний, пінг {ms} мс", "{kind} · available, ping {ms} ms"), ACCENT),
                Some(Some(Err(_))) => (lf!("{kind} · недоступний", "{kind} · unavailable"), glass::DANGER),
                _ => (lf!("{kind} · перевірка…", "{kind} · checking…"), ink_dim()),
            };
            let fg = if on { Color32::WHITE } else { ink() };
            let p = ui.painter();
            p.text(pos2(pick.left() + 14.0, pick.top() + 13.0), Align2::LEFT_CENTER, title, FontId::proportional(14.0), fg);
            p.text(pos2(pick.left() + 14.0, pick.top() + 31.0), Align2::LEFT_CENTER, state, FontId::proportional(12.0), if on { Color32::WHITE } else { color });
            let del = Rect::from_center_size(pos2(row.right() - 18.0, row.center().y), egui::Vec2::splat(32.0));
            if glass::icon_button(ui, del, ("proxy-del", k), Icon::Trash, false, true).on_hover_text(l!("Видалити", "Delete")).clicked() {
                remove = Some(link);
            }
        }
        if let Some(link) = remove {
            self.settings.proxies.retain(|l| *l != link);
            self.proxies.status.remove(&link);
            if self.settings.proxy == link {
                self.use_proxy(String::new());
            }
        }
        ui.add_space(4.0);
        let w = ui.available_width();
        let (field, add) = {
            let row = ui.allocate_exact_size(vec2(w, 36.0), Sense::hover()).0;
            (Rect::from_min_max(row.min, pos2(row.right() - 112.0, row.bottom())), Rect::from_min_max(pos2(row.right() - 104.0, row.top()), row.max))
        };
        glass::text_field(ui, field, &mut self.proxies.add, "t.me/proxy?… · socks5://host:port", false);
        let link = self.proxies.add.trim().to_owned();
        let error = (!link.is_empty()).then(|| Proxy::parse(&link).err()).flatten();
        if glass::button(ui, add, "proxy-add", l!("Додати", "Add"), true, !link.is_empty() && error.is_none()).clicked() {
            self.add_proxy(link);
            self.proxies.add.clear();
        }
        let note = match error {
            Some(e) => egui::RichText::new(e).color(glass::DANGER),
            None => egui::RichText::new(l!("Через проксі йдуть усі з'єднання з Telegram. Власник проксі бачить, що ви користуєтеся Telegram, але не бачить повідомлень.", "Every connection to Telegram goes through the proxy. Its owner can see that you use Telegram, but not your messages.")).color(ink_dim()),
        };
        ui.label(note.size(12.5));
    }

    /// Saves a proxy (if new) and switches to it.
    pub(super) fn add_proxy(&mut self, link: String) {
        if !self.settings.proxies.contains(&link) {
            self.settings.proxies.push(link.clone());
        }
        self.use_proxy(link);
        self.settings.save();
    }

    fn use_proxy(&mut self, link: String) {
        self.settings.proxy = link.clone();
        self.send(Cmd::SetProxy(link));
    }

    pub(super) fn on_proxy_checked(&mut self, link: String, result: Result<u32, String>) {
        if self.settings.proxies.contains(&link) {
            self.proxies.status.insert(link, Some(result));
        }
    }

    /// Settings closed: the next visit checks the list again.
    pub(super) fn forget_proxy_checks(&mut self) {
        self.proxies.status.clear();
    }
}
