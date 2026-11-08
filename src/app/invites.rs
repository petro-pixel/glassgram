//! Invite links and join requests pages of the "Manage" box, as in Telegram Desktop: our links with
//! their usage, a form for a new or edited link, revoking and deleting; approving or dismissing people.

use super::*;
use super::info::{chip, danger_row};
use crate::backend::{InviteDraft, InviteLink, JoinRequest};

/// Expiry choices of the link form, in seconds (0 — never).
const EXPIRES: [i64; 4] = [0, 3600, 86400, 7 * 86400];
/// Usage limits of the link form (0 — none).
const LIMITS: [i32; 5] = [0, 1, 10, 50, 100];
const TITLE_MAX: usize = 32;

#[derive(Default)]
pub(super) struct Invites {
    /// `None` while loading.
    pub(super) links: Option<Vec<InviteLink>>,
    pub(super) requests: Option<Vec<JoinRequest>>,
    /// Requests waiting, as the chat info counted them.
    pub(super) pending: i32,
    pub(super) form: Option<LinkForm>,
}

/// The "New link" / "Edit link" form.
pub(super) struct LinkForm {
    /// The link being edited; `None` for a new one.
    link: Option<String>,
    title: String,
    expire: i64,
    /// The expiry chip picked; `None` keeps the link's own date.
    span: Option<i64>,
    limit: i32,
    request: bool,
}

impl LinkForm {
    fn new() -> Self {
        Self { link: None, title: String::new(), expire: 0, span: Some(0), limit: 0, request: false }
    }

    fn edit(l: &InviteLink) -> Self {
        let expire = l.expire as i64;
        Self { link: Some(l.link.clone()), title: l.title.clone(), expire, span: (expire == 0).then_some(0), limit: l.usage_limit, request: l.request_needed }
    }
}

impl Invites {
    pub(super) fn form_title(&self) -> &'static str {
        match self.form.as_ref().and_then(|f| f.link.as_ref()) {
            Some(_) => l!("Редагувати посилання", "Edit link"),
            None => l!("Нове посилання", "New link"),
        }
    }
}

/// "t.me/+abc" instead of the full URL, as the official lists show it.
fn short(link: &str) -> &str {
    link.strip_prefix("https://").unwrap_or(link)
}

fn when(t: i64) -> String {
    jiff::Timestamp::from_second(t).map(|t| t.to_zoned(jiff::tz::TimeZone::system()).strftime("%d.%m.%Y %H:%M").to_string()).unwrap_or_default()
}

fn expire_label(s: i64) -> &'static str {
    match s {
        0 => l!("Без терміну", "No limit"),
        3600 => l!("1 година", "1 hour"),
        86400 => l!("1 день", "1 day"),
        _ => l!("1 тиждень", "1 week"),
    }
}

/// "3 joined · 1 request · until 12.11.2026 10:00".
fn link_caption(l: &InviteLink) -> String {
    let mut parts = vec![match l.usage {
        0 => l!("ніхто не приєднався", "no one joined").to_owned(),
        n => lf!("приєдналося: {n}", "{n} joined"),
    }];
    if l.usage_limit > 0 && !l.revoked {
        parts.push(lf!("ліміт {}", "limit {}", l.usage_limit));
    }
    if l.requested > 0 {
        let n = l.requested;
        parts.push(lf!("заявок: {n}", "{n} requests"));
    }
    if l.revoked {
        parts.push(l!("відкликано", "revoked").to_owned());
    } else if l.expired() {
        parts.push(l!("термін минув", "expired").to_owned());
    } else if l.expire > 0 {
        parts.push(lf!("до {}", "until {}", when(l.expire as i64)));
    }
    parts.join(" · ")
}

enum LinkAct {
    New,
    Copy(String),
    Edit(InviteLink),
    Send(Cmd),
}

impl App {
    pub(super) fn on_invite_links(&mut self, peer: PeerId, links: Vec<InviteLink>) {
        if self.manage_peer() == Some(peer) {
            self.manage.inv.links = Some(links);
        }
    }

    pub(super) fn on_join_requests(&mut self, peer: PeerId, list: Vec<JoinRequest>) {
        if self.manage_peer() == Some(peer) {
            self.manage.inv.pending = list.len() as i32;
            self.manage.inv.requests = Some(list);
        }
    }

    pub(super) fn links_page(&mut self, ui: &mut Ui, peer: PeerId) {
        let mut act = None;
        let inv = &self.manage.inv;
        egui::ScrollArea::vertical().id_salt("manage-links").auto_shrink(false).show(ui, |ui| {
            ui.spacing_mut().item_spacing.y = 4.0;
            let w = ui.available_width() - 8.0;
            let r = ui.allocate_exact_size(vec2(w, 36.0), Sense::hover()).0;
            if glass::button(ui, r, "invite-new", l!("Створити нове посилання", "Create a new link"), true, true).clicked() {
                act = Some(LinkAct::New);
            }
            let Some(links) = &inv.links else {
                ui.add_space(12.0);
                ui.vertical_centered(|ui| ui.add(egui::Spinner::new().color(ink())));
                return;
            };
            let (active, revoked): (Vec<&InviteLink>, Vec<&InviteLink>) = links.iter().partition(|l| !l.revoked);
            section_title(ui, l!("Ваші посилання", "Your links"));
            if active.is_empty() {
                ui.label(egui::RichText::new(l!("Активних посилань немає.", "No active links.")).color(ink_dim()).size(13.0));
            }
            for l in active {
                if let Some(a) = link_row(ui, peer, l) {
                    act = Some(a);
                }
            }
            if !revoked.is_empty() {
                section_title(ui, l!("Відкликані посилання", "Revoked links"));
                for l in revoked {
                    if let Some(a) = link_row(ui, peer, l) {
                        act = Some(a);
                    }
                }
                if danger_row(ui, l!("Видалити всі відкликані посилання", "Delete all revoked links")).clicked() {
                    act = Some(LinkAct::Send(Cmd::DeleteRevokedInvites(peer)));
                }
            }
            ui.label(egui::RichText::new(l!("Хто має посилання, може приєднатися. Його можна будь-коли відкликати.", "Anyone with a link can join. You can revoke it at any time.")).color(ink_dim()).size(12.5));
        });
        match act {
            Some(LinkAct::New) => self.manage.inv.form = Some(LinkForm::new()),
            Some(LinkAct::Edit(l)) => self.manage.inv.form = Some(LinkForm::edit(&l)),
            Some(LinkAct::Copy(link)) => {
                self.ctx.copy_text(link);
                self.toast = Some((l!("Посилання скопійовано", "Link copied").into(), 0.0, false));
            }
            Some(LinkAct::Send(cmd)) => self.send(cmd),
            None => {}
        }
    }

    pub(super) fn link_form_view(&mut self, ui: &mut Ui, peer: PeerId) {
        let Some(f) = &mut self.manage.inv.form else { return };
        let area = ui.max_rect();
        let buttons = Rect::from_min_max(pos2(area.left() + 4.0, area.bottom() - 36.0), pos2(area.right() - 4.0, area.bottom()));
        let body = Rect::from_min_max(area.min, pos2(area.right(), buttons.top() - 10.0));
        let mut child = ui.new_child(UiBuilder::new().max_rect(body));
        child.set_clip_rect(body);
        egui::ScrollArea::vertical().id_salt("invite-form").auto_shrink(false).show(&mut child, |ui| {
            ui.spacing_mut().item_spacing.y = 6.0;
            let w = ui.available_width() - 8.0;
            section_title(ui, l!("Назва посилання", "Link name"));
            let r = ui.allocate_exact_size(vec2(w, 38.0), Sense::hover()).0;
            glass::text_field(ui, r, &mut f.title, l!("Назва (необов'язково)", "Name (optional)"), false);
            if f.title.chars().count() > TITLE_MAX {
                f.title = f.title.chars().take(TITLE_MAX).collect();
            }
            ui.label(egui::RichText::new(l!("Назву бачите лише ви та інші адміністратори.", "Only you and other admins will see this name.")).color(ink_dim()).size(12.5));
            section_title(ui, l!("Заявки на вступ", "Request admin approval"));
            ui.checkbox(&mut f.request, l!("Схвалювати нових учасників", "Approve new members"));
            ui.label(egui::RichText::new(l!("Люди зможуть приєднатися лише після схвалення адміністратором.", "People will join only after an admin approves them.")).color(ink_dim()).size(12.5));
            section_title(ui, l!("Обмеження в часі", "Limit by time period"));
            ui.horizontal_wrapped(|ui| {
                ui.spacing_mut().item_spacing = vec2(6.0, 6.0);
                for s in EXPIRES {
                    if chip(ui, expire_label(s), f.span == Some(s)).clicked() {
                        f.span = Some(s);
                        f.expire = if s == 0 { 0 } else { jiff::Timestamp::now().as_second() + s };
                    }
                }
            });
            if f.expire > 0 {
                ui.label(egui::RichText::new(lf!("Діє до {}", "Works until {}", when(f.expire))).color(ink_dim()).size(12.5));
            }
            if !f.request {
                section_title(ui, l!("Обмеження кількості", "Limit number of uses"));
                ui.horizontal_wrapped(|ui| {
                    ui.spacing_mut().item_spacing = vec2(6.0, 6.0);
                    for n in LIMITS {
                        let label = if n == 0 { l!("Без ліміту", "No limit").to_owned() } else { n.to_string() };
                        if chip(ui, &label, f.limit == n).clicked() {
                            f.limit = n;
                        }
                    }
                });
            }
        });
        let half = (buttons.width() - 8.0) / 2.0;
        let cancel = glass::button(ui, Rect::from_min_size(buttons.min, vec2(half, buttons.height())), "invite-cancel", l!("Скасувати", "Cancel"), false, true).clicked();
        let save = glass::button(ui, Rect::from_min_size(pos2(buttons.right() - half, buttons.top()), vec2(half, buttons.height())), "invite-save", l!("Зберегти", "Save"), true, true).clicked();
        if cancel {
            self.manage.inv.form = None;
        } else if save && let Some(f) = self.manage.inv.form.take() {
            let draft = InviteDraft { title: f.title, expire: f.expire as i32, usage_limit: if f.request { 0 } else { f.limit }, request_needed: f.request };
            self.send(Cmd::SaveInviteLink { peer, link: f.link, draft });
        }
    }

    pub(super) fn requests_page(&mut self, ui: &mut Ui, peer: PeerId) {
        let mut cmd = None;
        let inv = &self.manage.inv;
        let textures = &self.textures;
        egui::ScrollArea::vertical().id_salt("manage-requests").auto_shrink(false).show(ui, |ui| {
            ui.spacing_mut().item_spacing.y = 4.0;
            let Some(list) = &inv.requests else {
                ui.add_space(12.0);
                ui.vertical_centered(|ui| ui.add(egui::Spinner::new().color(ink())));
                return;
            };
            if list.is_empty() {
                ui.label(egui::RichText::new(l!("Нових заявок немає.", "No new requests.")).color(ink_dim()).size(13.0));
                return;
            }
            let w = ui.available_width() - 8.0;
            if list.len() > 1 {
                ui.horizontal(|ui| {
                    ui.spacing_mut().item_spacing.x = 8.0;
                    let half = (w - 8.0) / 2.0;
                    let (r, _) = ui.allocate_exact_size(vec2(half, 34.0), Sense::hover());
                    if glass::button(ui, r, "requests-all-ok", l!("Прийняти всіх", "Approve all"), true, true).clicked() {
                        cmd = Some(Cmd::AnswerJoinRequest { peer, user: None, approve: true });
                    }
                    let (r, _) = ui.allocate_exact_size(vec2(half, 34.0), Sense::hover());
                    if glass::button(ui, r, "requests-all-no", l!("Відхилити всіх", "Dismiss all"), false, true).clicked() {
                        cmd = Some(Cmd::AnswerJoinRequest { peer, user: None, approve: false });
                    }
                });
                ui.add_space(4.0);
            }
            for q in list {
                let h = if q.about.is_empty() { 92.0 } else { 112.0 };
                let (row, _) = ui.allocate_exact_size(vec2(w, h), Sense::hover());
                let p = ui.painter();
                p.rect_filled(row, 14.0, glass::veil(10));
                let c = &q.chat;
                glass::avatar(p, pos2(row.left() + 28.0, row.top() + 26.0), 18.0, &c.title, c.id.bot_api_dialog_id_unchecked(), None, textures.get(&ImgKey::Avatar(c.id)));
                let x = row.left() + 56.0;
                p.galley(pos2(x, row.top() + 8.0), elided(ui, &c.title, 14.0, ink(), row.width() - 70.0), ink());
                p.galley(pos2(x, row.top() + 28.0), elided(ui, &lf!("заявка {}", "requested {}", q.date), 12.5, ink_dim(), row.width() - 70.0), ink_dim());
                if !q.about.is_empty() {
                    p.galley(pos2(x, row.top() + 48.0), elided(ui, &q.about, 12.5, ink(), row.width() - 70.0), ink());
                }
                let id = c.id.bot_api_dialog_id_unchecked();
                let b = Rect::from_min_size(pos2(x, row.bottom() - 38.0), vec2(110.0, 30.0));
                if glass::button(ui, b, ("request-ok", id), l!("Прийняти", "Approve"), true, true).clicked() {
                    cmd = Some(Cmd::AnswerJoinRequest { peer, user: Some(c.id), approve: true });
                }
                if glass::button(ui, b.translate(vec2(118.0, 0.0)), ("request-no", id), l!("Відхилити", "Dismiss"), false, true).clicked() {
                    cmd = Some(Cmd::AnswerJoinRequest { peer, user: Some(c.id), approve: false });
                }
            }
        });
        if let Some(cmd) = cmd {
            self.send(cmd);
        }
    }
}

/// One link: a click copies it, the context menu edits, revokes or deletes it.
fn link_row(ui: &mut Ui, peer: PeerId, l: &InviteLink) -> Option<LinkAct> {
    let w = ui.available_width() - 8.0;
    let (rect, resp) = ui.allocate_exact_size(vec2(w, 50.0), Sense::click());
    let hover = ui.ctx().animate_bool(resp.id, resp.hovered());
    if hover > 0.0 {
        ui.painter().rect_filled(rect, 14.0, glass::veil((hover * 14.0) as u8));
    }
    let dead = l.revoked || l.expired();
    let icon = Rect::from_center_size(pos2(rect.left() + 24.0, rect.center().y), vec2(20.0, 20.0));
    icons::draw(ui.painter(), icon, Icon::Link, if dead { ink_dim() } else { ACCENT });
    let x = rect.left() + 50.0;
    let name = if l.title.is_empty() { short(&l.link) } else { &l.title };
    ui.painter().galley(pos2(x, rect.top() + 7.0), elided(ui, name, 14.0, if dead { ink_dim() } else { ink() }, rect.width() - 60.0), ink());
    ui.painter().galley(pos2(x, rect.top() + 27.0), elided(ui, &link_caption(l), 12.5, ink_dim(), rect.width() - 60.0), ink_dim());
    if resp.hovered() {
        ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
    }
    let mut act = (resp.clicked() && !l.revoked).then(|| LinkAct::Copy(l.link.clone()));
    resp.context_menu(|ui| {
        if !l.revoked {
            if glass::menu_button(ui, Icon::Copy, l!("Копіювати посилання", "Copy link")).clicked() {
                act = Some(LinkAct::Copy(l.link.clone()));
            }
            // The primary link only gets replaced, as in Telegram Desktop.
            if !l.permanent && glass::menu_button(ui, Icon::Pencil, l!("Редагувати", "Edit")).clicked() {
                act = Some(LinkAct::Edit(l.clone()));
            }
            if glass::menu_button(ui, Icon::Close, l!("Відкликати", "Revoke")).clicked() {
                act = Some(LinkAct::Send(Cmd::RevokeInviteLink { peer, link: l.link.clone() }));
            }
        } else if glass::menu_button(ui, Icon::Trash, l!("Видалити", "Delete")).clicked() {
            act = Some(LinkAct::Send(Cmd::DeleteInviteLink { peer, link: l.link.clone() }));
        }
    });
    act
}
