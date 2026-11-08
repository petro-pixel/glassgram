//! "Manage" box of a group or channel, as in Telegram Desktop: administrators with their rights and
//! titles, restricted and removed members, and the invite links and join requests from `invites.rs`.
//! Only what our own admin rights allow is offered.

use super::*;
use super::info::{chip, danger_row};
use super::invites::Invites;
use crate::backend::{AdminRights, ChatDetails, ChatRights, Member, MemberRole, PartList, Participant};

/// Telegram's limit on an admin title.
const RANK_MAX: usize = 16;
/// Restriction lengths offered, in seconds (0 — forever), as in Telegram Desktop.
const SPANS: [i64; 4] = [0, 86400, 7 * 86400, 30 * 86400];

#[derive(Clone, Copy, PartialEq, Default)]
pub(super) enum Page {
    #[default]
    Home,
    Admins,
    Restricted,
    Removed,
    Requests,
    Links,
    /// Picking a member to promote, restrict or remove.
    Pick(Pick),
}

#[derive(Clone, Copy, PartialEq)]
pub(super) enum Pick {
    Admin,
    Restrict,
    Ban,
}

impl Page {
    fn title(self, channel: bool) -> &'static str {
        match self {
            Page::Home => if channel { l!("Керування каналом", "Manage channel") } else { l!("Керування групою", "Manage group") },
            Page::Admins => l!("Адміністратори", "Administrators"),
            Page::Restricted => l!("Обмежені користувачі", "Restricted users"),
            Page::Removed => l!("Вилучені користувачі", "Removed users"),
            Page::Requests => if channel { l!("Заявки на підписку", "Subscribe requests") } else { l!("Заявки на вступ", "Join requests") },
            Page::Links => l!("Посилання-запрошення", "Invite links"),
            Page::Pick(Pick::Admin) => l!("Додати адміністратора", "Add administrator"),
            Page::Pick(Pick::Restrict) => l!("Додати виняток", "Add exception"),
            Page::Pick(Pick::Ban) => l!("Вилучити користувача", "Remove user"),
        }
    }

    /// The title with the number of waiting requests, for the section lists.
    pub(super) fn label(self, channel: bool, pending: i32) -> String {
        match self {
            Page::Requests if pending > 0 => format!("{} · {pending}", self.title(channel)),
            _ => self.title(channel).to_owned(),
        }
    }

    fn icon(self) -> Icon {
        match self {
            Page::Links => Icon::Link,
            Page::Requests => Icon::User,
            Page::Restricted => Icon::Lock,
            Page::Removed => Icon::Trash,
            _ => Icon::Users,
        }
    }
}

/// Our place in the chat: what the box may offer.
#[derive(Clone, Copy, Default)]
struct Powers {
    rights: AdminRights,
    owner: bool,
    channel: bool,
    basic: bool,
    forum: bool,
    /// Default member permissions: a restriction starts from them.
    defaults: Option<ChatRights>,
}

/// The editor shown over a list.
enum Editor {
    Admin { user: ChatInfo, rights: AdminRights, rank: String, existing: bool },
    Restrict { user: ChatInfo, rights: ChatRights, until: i64, span: Option<i64>, existing: bool },
    Ban { user: ChatInfo },
}

#[derive(Default)]
pub(super) struct Manage {
    peer: Option<PeerId>,
    me: Powers,
    page: Page,
    pages: Vec<Page>,
    items: Vec<Participant>,
    loading: bool,
    /// Search in the pick pages, sent after a typing pause.
    query: String,
    edited_at: f64,
    asked: Option<String>,
    /// Asked from a member's menu: open this admin's editor once the list arrives.
    pending: Option<PeerId>,
    editor: Option<Editor>,
    pub(super) inv: Invites,
}

/// Sections of the box our rights allow, in Telegram Desktop's order.
pub(super) fn pages_for(d: &ChatDetails, channel: bool) -> Vec<Page> {
    let Some(r) = d.my_rights else { return Vec::new() };
    let mut pages = Vec::new();
    if r.invite_users {
        pages.extend([Page::Links, Page::Requests]);
    }
    pages.push(Page::Admins);
    if r.ban_users && !d.basic_group {
        if !channel {
            pages.push(Page::Restricted);
        }
        pages.push(Page::Removed);
    }
    pages
}

/// A member's context menu in the info panel: what we may do with them.
pub(super) fn member_menu(ui: &mut Ui, m: &Member, d: &ChatDetails, channel: bool) -> Option<Pick> {
    let r = d.my_rights?;
    let mut pick = None;
    if m.role == MemberRole::Owner {
        return None;
    }
    if r.add_admins || (d.basic_group && d.creator) {
        let label = if m.role == MemberRole::Admin { l!("Змінити права адміністратора", "Edit admin rights") } else { l!("Призначити адміністратором", "Promote to admin") };
        if glass::menu_button(ui, Icon::Users, label).clicked() {
            pick = Some(Pick::Admin);
        }
    }
    if r.ban_users && m.role == MemberRole::Member {
        if !channel && !d.basic_group && glass::menu_button(ui, Icon::Lock, l!("Обмежити", "Restrict")).clicked() {
            pick = Some(Pick::Restrict);
        }
        let label = if channel { l!("Вилучити з каналу", "Remove from channel") } else { l!("Вилучити з групи", "Remove from group") };
        if glass::menu_button(ui, Icon::Trash, label).clicked() {
            pick = Some(Pick::Ban);
        }
    }
    pick
}

fn date_label(t: i64) -> String {
    jiff::Timestamp::from_second(t).map(|t| t.to_zoned(jiff::tz::TimeZone::system()).strftime("%d.%m.%Y %H:%M").to_string()).unwrap_or_default()
}

fn span_label(s: i64) -> &'static str {
    match s {
        0 => l!("Назавжди", "Forever"),
        86400 => l!("1 день", "1 day"),
        604800 => l!("1 тиждень", "1 week"),
        _ => l!("1 місяць", "1 month"),
    }
}

impl App {
    pub(super) fn manage_open(&self) -> bool {
        self.manage.peer.is_some()
    }

    pub(super) fn manage_peer(&self) -> Option<PeerId> {
        self.manage.peer
    }

    /// Opens the box on a page, with what the info panel knows about our rights.
    pub(super) fn open_manage(&mut self, peer: PeerId, page: Page) {
        self.open_manage_for(peer, page, None);
    }

    /// `pending`: an admin whose editor opens when the list comes (the demo answers inside `send`).
    fn open_manage_for(&mut self, peer: PeerId, page: Page, pending: Option<PeerId>) {
        let Some(d) = self.info_details_mut(peer).map(|d| d.clone()) else { return };
        let Some(rights) = d.my_rights else { return };
        let chat = self.chats.iter().find(|c| c.id == peer);
        let channel = chat.is_some_and(|c| c.kind == ChatKind::Channel);
        let forum = chat.is_some_and(|c| c.forum);
        let me = Powers { rights, owner: d.creator, channel, basic: d.basic_group, forum, defaults: d.rights };
        self.manage = Manage { peer: Some(peer), me, pages: pages_for(&d, channel), ..Default::default() };
        self.manage.inv.pending = d.requests;
        self.manage.pending = pending;
        self.manage_go(page);
    }

    /// From a member's menu in the info panel.
    pub(super) fn manage_member(&mut self, peer: PeerId, m: &Member, pick: Pick) {
        let page = match pick {
            Pick::Admin => Page::Admins,
            Pick::Restrict => Page::Restricted,
            Pick::Ban => Page::Removed,
        };
        // An admin's rights come with the admin list.
        let existing = pick == Pick::Admin && m.role == MemberRole::Admin;
        self.open_manage_for(peer, page, existing.then_some(m.chat.id));
        if existing || self.manage.peer.is_none() {
            return;
        }
        let user = m.chat.clone();
        let me = self.manage.me;
        self.manage.editor = match pick {
            Pick::Admin => Some(Editor::Admin { user, rights: grantable(me, AdminRights::new_admin()), rank: String::new(), existing: false }),
            Pick::Restrict => Some(Editor::Restrict { user, rights: me.defaults.unwrap_or(ChatRights::from_banned(None)), until: 0, span: Some(0), existing: false }),
            Pick::Ban => Some(Editor::Ban { user }),
        };
    }

    fn manage_go(&mut self, page: Page) {
        let Some(peer) = self.manage.peer else { return };
        let m = &mut self.manage;
        m.page = page;
        m.items.clear();
        m.query.clear();
        m.asked = None;
        m.loading = true;
        let list = match page {
            Page::Home => {
                m.loading = false;
                return;
            }
            Page::Links => {
                m.inv.links = None;
                return self.send(Cmd::LoadInviteLinks(peer));
            }
            Page::Requests => {
                m.inv.requests = None;
                return self.send(Cmd::LoadJoinRequests(peer));
            }
            Page::Admins => PartList::Admins,
            Page::Restricted => PartList::Restricted,
            Page::Removed => PartList::Removed,
            Page::Pick(_) => {
                m.asked = Some(String::new());
                PartList::Search(String::new())
            }
        };
        self.send(Cmd::LoadParticipants { peer, list });
    }

    pub(super) fn manage_event(&mut self, ev: Ev) {
        match ev {
            Ev::Participants { peer, list, items } => {
                let m = &mut self.manage;
                let wanted = match m.page {
                    Page::Admins => list == PartList::Admins,
                    Page::Restricted => list == PartList::Restricted,
                    Page::Removed => list == PartList::Removed,
                    Page::Pick(_) => matches!(&list, PartList::Search(q) if Some(q) == m.asked.as_ref()),
                    _ => false,
                };
                if m.peer != Some(peer) || !wanted {
                    return;
                }
                m.loading = false;
                if let Some(user) = m.pending.take()
                    && let Some(p) = items.iter().find(|p| p.chat.id == user)
                {
                    m.editor = Some(admin_editor(m.me, p));
                }
                m.items = items;
            }
            Ev::ParticipantsChanged(peer) => {
                if self.manage.peer == Some(peer) && !matches!(self.manage.page, Page::Pick(_)) {
                    let page = self.manage.page;
                    self.manage_go(page);
                }
                // The members tab shows roles: refresh it.
                self.reload_info_members(peer);
            }
            Ev::InviteLinks { peer, links } => self.on_invite_links(peer, links),
            Ev::JoinRequests { peer, list } => self.on_join_requests(peer, list),
            _ => {}
        }
    }

    pub(super) fn manage_view(&mut self, ui: &mut Ui) {
        let Some(peer) = self.manage.peer else { return };
        let screen = ui.max_rect();
        ui.painter().rect_filled(screen, 0.0, Color32::from_black_alpha(90));
        ui.interact(screen, ui.scope_id().with("manage-bg"), Sense::click());
        let rect = Rect::from_center_size(screen.center(), vec2(screen.width().min(460.0) - 20.0, (screen.height() - 60.0).min(640.0)));
        glass::glass(ui.painter(), rect, Glass::panel(26.0).with_shadow());
        let inner = rect.shrink(18.0);
        let m = &self.manage;
        let editing = m.editor.is_some() || self.manage.inv.form.is_some();
        let title = match &m.editor {
            Some(Editor::Admin { existing: true, .. }) => l!("Права адміністратора", "Admin rights"),
            Some(Editor::Admin { .. }) => l!("Новий адміністратор", "New administrator"),
            Some(Editor::Restrict { .. }) => l!("Обмеження користувача", "User restrictions"),
            Some(Editor::Ban { .. }) => l!("Вилучити користувача", "Remove user"),
            None if m.inv.form.is_some() => self.manage.inv.form_title(),
            None => m.page.title(m.me.channel),
        };
        let back = editing || m.page != Page::Home;
        let mut x = inner.left();
        let mut go_back = false;
        if back {
            let r = Rect::from_min_size(pos2(inner.left() - 4.0, inner.top() - 4.0), vec2(32.0, 32.0));
            go_back = glass::icon_button(ui, r, "manage-back", Icon::ChevronLeft, false, true).on_hover_text(l!("Назад", "Back")).clicked();
            x += 36.0;
        }
        ui.painter().text(pos2(x, inner.top() + 12.0), Align2::LEFT_CENTER, title, FontId::proportional(17.0), ink());
        let close = Rect::from_min_size(pos2(inner.right() - 32.0, inner.top() - 4.0), vec2(32.0, 32.0));
        let esc = ui.input_mut(|i| i.consume_key(egui::Modifiers::NONE, Key::Escape));
        if glass::icon_button(ui, close, "manage-close", Icon::Close, false, true).clicked() || (esc && !editing) {
            self.manage = Manage::default();
            return;
        }
        if go_back || esc {
            let m = &mut self.manage;
            if m.editor.take().is_none() && m.inv.form.take().is_none() {
                // A pick page returns to the list it was opened from.
                let page = match m.page {
                    Page::Pick(Pick::Admin) => Page::Admins,
                    Page::Pick(Pick::Restrict) => Page::Restricted,
                    Page::Pick(Pick::Ban) => Page::Removed,
                    _ => Page::Home,
                };
                self.manage_go(page);
            }
            return;
        }
        let body = Rect::from_min_max(pos2(inner.left() - 4.0, inner.top() + 40.0), pos2(inner.right() + 4.0, inner.bottom()));
        let mut child = ui.new_child(UiBuilder::new().max_rect(body));
        child.set_clip_rect(body);
        if self.manage.editor.is_some() {
            self.manage_editor(&mut child, peer);
        } else if self.manage.inv.form.is_some() {
            self.link_form_view(&mut child, peer);
        } else {
            let now = ui.input(|i| i.time);
            self.manage_page(&mut child, peer, now);
        }
    }

    fn manage_page(&mut self, ui: &mut Ui, peer: PeerId, now: f64) {
        let page = self.manage.page;
        match page {
            Page::Links => return self.links_page(ui, peer),
            Page::Requests => return self.requests_page(ui, peer),
            _ => {}
        }
        let mut todo: Option<Act> = None;
        let m = &mut self.manage;
        let me = m.me;
        egui::ScrollArea::vertical().id_salt(("manage", page_key(page))).auto_shrink(false).show(ui, |ui| {
            ui.spacing_mut().item_spacing.y = 4.0;
            let w = ui.available_width() - 8.0;
            match page {
                Page::Home => {
                    for p in &m.pages {
                        if super::info::info_row(ui, p.icon(), &p.label(me.channel, m.inv.pending), "").clicked() {
                            todo = Some(Act::Go(*p));
                        }
                    }
                    return;
                }
                Page::Pick(_) => {
                    let r = ui.allocate_exact_size(vec2(w, 38.0), Sense::hover()).0;
                    if glass::search_field(ui, r, &mut m.query, l!("Пошук учасників", "Search members")).changed() {
                        m.edited_at = now;
                    }
                }
                Page::Admins if me.rights.add_admins || (me.basic && me.owner) => {
                    let r = ui.allocate_exact_size(vec2(w, 36.0), Sense::hover()).0;
                    if glass::button(ui, r, "manage-add-admin", l!("Додати адміністратора", "Add administrator"), false, true).clicked() {
                        todo = Some(Act::Go(Page::Pick(Pick::Admin)));
                    }
                }
                Page::Restricted | Page::Removed => {
                    let (label, pick) = if page == Page::Restricted { (l!("Додати виняток", "Add exception"), Pick::Restrict) } else { (l!("Вилучити користувача", "Remove user"), Pick::Ban) };
                    let r = ui.allocate_exact_size(vec2(w, 36.0), Sense::hover()).0;
                    if glass::button(ui, r, "manage-add", label, false, true).clicked() {
                        todo = Some(Act::Go(Page::Pick(pick)));
                    }
                }
                _ => {}
            }
            ui.add_space(4.0);
            if m.items.is_empty() {
                let text = if m.loading {
                    l!("Завантаження…", "Loading…")
                } else {
                    match page {
                        Page::Restricted => l!("Немає користувачів з особистими обмеженнями.", "No users with personal restrictions."),
                        Page::Removed => l!("Вилучені користувачі не зможуть повернутися за посиланням.", "Removed users can't come back by invite link."),
                        _ => l!("Нікого не знайдено", "Nobody found"),
                    }
                };
                ui.label(egui::RichText::new(text).color(ink_dim()).size(13.0));
            }
            for p in &m.items {
                if let Some(a) = participant_row(ui, &self.textures, peer, p, page, me) {
                    todo = Some(a);
                }
            }
            if page == Page::Admins && !m.items.is_empty() {
                ui.label(egui::RichText::new(l!("Адміністратори допомагають керувати чатом; права кожного можна налаштувати.", "Admins help you manage the chat; you can set the rights of each.")).color(ink_dim()).size(12.5));
            }
        });
        // The member search goes to the server after a typing pause.
        let m = &mut self.manage;
        if matches!(page, Page::Pick(_)) && m.asked.as_deref() != Some(m.query.trim()) && now - m.edited_at > SEARCH_DELAY {
            let q = m.query.trim().to_owned();
            m.asked = Some(q.clone());
            m.loading = true;
            self.send(Cmd::LoadParticipants { peer, list: PartList::Search(q) });
        }
        match todo {
            Some(Act::Go(p)) => self.manage_go(p),
            Some(Act::Open(e)) => self.manage.editor = Some(e),
            Some(Act::Send(cmd)) => self.send(cmd),
            None => {}
        }
    }

    fn manage_editor(&mut self, ui: &mut Ui, peer: PeerId) {
        let me = self.manage.me;
        let Some(editor) = &mut self.manage.editor else { return };
        let buttons = Rect::from_min_max(pos2(ui.max_rect().left() + 4.0, ui.max_rect().bottom() - 36.0), pos2(ui.max_rect().right() - 4.0, ui.max_rect().bottom()));
        let body = Rect::from_min_max(ui.max_rect().min, pos2(ui.max_rect().right(), buttons.top() - 10.0));
        let mut child = ui.new_child(UiBuilder::new().max_rect(body));
        child.set_clip_rect(body);
        let mut extra: Option<Cmd> = None;
        egui::ScrollArea::vertical().id_salt("manage-editor").auto_shrink(false).show(&mut child, |ui| {
            ui.spacing_mut().item_spacing.y = 6.0;
            let user = match editor {
                Editor::Admin { user, .. } | Editor::Restrict { user, .. } | Editor::Ban { user } => user.clone(),
            };
            let (row, _) = ui.allocate_exact_size(vec2(ui.available_width() - 8.0, 56.0), Sense::hover());
            glass::avatar(ui.painter(), pos2(row.left() + 28.0, row.center().y), 22.0, &user.title, user.id.bot_api_dialog_id_unchecked(), None, self.textures.get(&ImgKey::Avatar(user.id)));
            ui.painter().galley(pos2(row.left() + 62.0, row.center().y - 9.0), elided(ui, &user.title, 15.0, ink(), row.width() - 70.0), ink());
            match editor {
                Editor::Admin { user, rights, rank, existing } => {
                    if me.basic {
                        ui.label(egui::RichText::new(l!("У звичайних групах адміністратори можуть усе, крім призначення інших адміністраторів.", "In basic groups admins can do everything except add new admins.")).color(ink_dim()).size(12.5));
                    } else {
                        admin_switches(ui, rights, me);
                        if !me.channel {
                            section_title(ui, l!("Звання", "Custom title"));
                            let r = ui.allocate_exact_size(vec2(ui.available_width() - 8.0, 38.0), Sense::hover()).0;
                            glass::text_field(ui, r, rank, l!("Адміністратор", "Admin"), false);
                            if rank.chars().count() > RANK_MAX {
                                *rank = rank.chars().take(RANK_MAX).collect();
                            }
                            ui.label(egui::RichText::new(l!("Звання видно учасникам замість «адмін».", "Members will see this title instead of \"admin\".")).color(ink_dim()).size(12.5));
                        }
                    }
                    if *existing {
                        ui.add_space(6.0);
                        if danger_row(ui, l!("Звільнити адміністратора", "Dismiss admin")).clicked() {
                            extra = Some(Cmd::EditAdmin { peer, user: user.id, rights: None, rank: String::new() });
                        }
                    }
                }
                Editor::Restrict { user, rights, until, span, existing } => {
                    section_title(ui, l!("Що може цей користувач", "What this user can do"));
                    let d = me.defaults.unwrap_or(ChatRights::from_banned(None));
                    member_switches(ui, rights, d);
                    section_title(ui, l!("Тривалість обмеження", "Restriction duration"));
                    ui.horizontal_wrapped(|ui| {
                        ui.spacing_mut().item_spacing = vec2(6.0, 6.0);
                        for s in SPANS {
                            if chip(ui, span_label(s), *span == Some(s)).clicked() {
                                *span = Some(s);
                                *until = if s == 0 { 0 } else { jiff::Timestamp::now().as_second() + s };
                            }
                        }
                    });
                    let text = if *until == 0 { l!("Без терміну", "No end date").to_owned() } else { lf!("До {}", "Until {}", date_label(*until)) };
                    ui.label(egui::RichText::new(text).color(ink_dim()).size(12.5));
                    ui.add_space(6.0);
                    if *existing && danger_row(ui, l!("Зняти обмеження", "Lift restrictions")).clicked() {
                        extra = Some(Cmd::UnbanMember { peer, user: user.id });
                    }
                    let label = if me.channel { l!("Вилучити з каналу", "Remove from channel") } else { l!("Вилучити з групи", "Remove from group") };
                    if danger_row(ui, label).clicked() {
                        extra = Some(Cmd::BanMember { peer, user: user.id });
                    }
                }
                Editor::Ban { user } => {
                    let text = if me.basic {
                        lf!("Вилучити {} з групи?", "Remove {} from the group?", user.title)
                    } else if me.channel {
                        lf!("Вилучити {} з каналу? Повернутися за посиланням не вийде.", "Remove {} from the channel? They won't be able to come back by invite link.", user.title)
                    } else {
                        lf!("Вилучити {} з групи? Повернутися за посиланням не вийде.", "Remove {} from the group? They won't be able to come back by invite link.", user.title)
                    };
                    ui.label(egui::RichText::new(text).color(ink()).size(14.0));
                }
            }
        });
        let half = (buttons.width() - 8.0) / 2.0;
        let cancel = glass::button(ui, Rect::from_min_size(buttons.min, vec2(half, buttons.height())), "manage-cancel", l!("Скасувати", "Cancel"), false, true).clicked();
        let ok_label = if matches!(editor, Editor::Ban { .. }) { l!("Вилучити", "Remove") } else { l!("Зберегти", "Save") };
        let save = glass::button(ui, Rect::from_min_size(pos2(buttons.right() - half, buttons.top()), vec2(half, buttons.height())), "manage-save", ok_label, true, true).clicked();
        if let Some(cmd) = extra {
            self.manage.editor = None;
            self.send(cmd);
        } else if cancel {
            self.manage.editor = None;
        } else if save && let Some(e) = self.manage.editor.take() {
            self.send(match e {
                Editor::Admin { user, rights, rank, .. } => Cmd::EditAdmin { peer, user: user.id, rights: Some(rights), rank: rank.trim().to_owned() },
                Editor::Restrict { user, rights, until, .. } => Cmd::RestrictMember { peer, user: user.id, rights, until: until as i32 },
                Editor::Ban { user } => Cmd::BanMember { peer, user: user.id },
            });
            // A pick page has done its job: back to the list.
            if let Page::Pick(p) = self.manage.page {
                self.manage_go(match p {
                    Pick::Admin => Page::Admins,
                    Pick::Restrict => Page::Restricted,
                    Pick::Ban => Page::Removed,
                });
            }
        }
    }
}

/// What a click in a list asks for; done after drawing.
enum Act {
    Go(Page),
    Open(Editor),
    Send(Cmd),
}

fn page_key(p: Page) -> u8 {
    match p {
        Page::Home => 0,
        Page::Admins => 1,
        Page::Restricted => 2,
        Page::Removed => 3,
        Page::Requests => 4,
        Page::Links => 5,
        Page::Pick(_) => 6,
    }
}

/// Rights a new admin may get: only those we have ourselves, unless we own the chat.
fn grantable(me: Powers, mut r: AdminRights) -> AdminRights {
    if !me.owner {
        let m = me.rights;
        r.change_info &= m.change_info;
        r.post_messages &= m.post_messages;
        r.edit_messages &= m.edit_messages;
        r.delete_messages &= m.delete_messages;
        r.ban_users &= m.ban_users;
        r.invite_users &= m.invite_users;
        r.pin_messages &= m.pin_messages;
        r.manage_topics &= m.manage_topics;
        r.stories &= m.stories;
        r.manage_call &= m.manage_call;
        r.anonymous &= m.anonymous;
        r.add_admins &= m.add_admins;
    }
    r
}

fn admin_editor(me: Powers, p: &Participant) -> Editor {
    let rights = p.admin.unwrap_or_else(|| grantable(me, AdminRights::new_admin()));
    Editor::Admin { user: p.chat.clone(), rights, rank: p.rank.clone(), existing: p.admin.is_some() }
}

/// One row of a list, with what a click on it does.
fn participant_row(ui: &mut Ui, textures: &super::textures::Textures, peer: PeerId, p: &Participant, page: Page, me: Powers) -> Option<Act> {
    let w = ui.available_width() - 8.0;
    let (rect, resp) = ui.allocate_exact_size(vec2(w, 50.0), Sense::click());
    let hover = ui.ctx().animate_bool(resp.id, resp.hovered());
    if hover > 0.0 {
        ui.painter().rect_filled(rect, 14.0, glass::veil((hover * 14.0) as u8));
    }
    let c = &p.chat;
    glass::avatar(ui.painter(), pos2(rect.left() + 26.0, rect.center().y), 18.0, &c.title, c.id.bot_api_dialog_id_unchecked(), None, textures.get(&ImgKey::Avatar(c.id)));
    let sub = match page {
        _ if p.owner => l!("власник", "owner").to_owned(),
        Page::Admins if !p.rank.is_empty() => p.rank.clone(),
        Page::Admins if !p.by.is_empty() => lf!("призначив(ла) {}", "promoted by {}", p.by),
        Page::Admins => l!("адмін", "admin").to_owned(),
        Page::Restricted => match p.restricted {
            Some((_, until)) if until > 0 => lf!("до {}", "until {}", date_label(until as i64)),
            _ => l!("назавжди", "forever").to_owned(),
        },
        Page::Removed if !p.by.is_empty() => lf!("вилучив(ла) {}", "removed by {}", p.by),
        _ if p.admin.is_some() => l!("адмін", "admin").to_owned(),
        _ => String::new(),
    };
    // A removed user gets an "Unban" button at the end of the row.
    let unban = page == Page::Removed && me.rights.ban_users;
    let text_w = rect.width() - 64.0 - if unban { 116.0 } else { 0.0 };
    let x = rect.left() + 54.0;
    ui.painter().galley(pos2(x, rect.top() + 7.0), elided(ui, &c.title, 14.0, ink(), text_w), ink());
    ui.painter().galley(pos2(x, rect.top() + 27.0), elided(ui, &sub, 12.5, ink_dim(), text_w), ink_dim());
    if unban {
        let b = Rect::from_min_size(pos2(rect.right() - 116.0, rect.center().y - 15.0), vec2(108.0, 30.0));
        if glass::button(ui, b, ("unban", c.id.bot_api_dialog_id_unchecked()), l!("Повернути", "Unban"), false, true).clicked() {
            return Some(Act::Send(Cmd::UnbanMember { peer, user: c.id }));
        }
    }
    let clickable = match page {
        Page::Admins => !p.owner && (p.can_edit || me.owner) && (me.rights.add_admins || (me.basic && me.owner)),
        Page::Restricted | Page::Pick(_) => !p.owner,
        _ => false,
    };
    if !clickable {
        return None;
    }
    if resp.hovered() {
        ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
    }
    if !resp.clicked() {
        return None;
    }
    let defaults = me.defaults.unwrap_or(ChatRights::from_banned(None));
    Some(Act::Open(match page {
        Page::Admins | Page::Pick(Pick::Admin) => admin_editor(me, p),
        Page::Pick(Pick::Ban) => Editor::Ban { user: c.clone() },
        _ => match p.restricted {
            Some((rights, until)) => Editor::Restrict { user: c.clone(), rights, until: until as i64, span: (until == 0).then_some(0), existing: true },
            None => Editor::Restrict { user: c.clone(), rights: defaults, until: 0, span: Some(0), existing: false },
        },
    }))
}

/// Admin rights switches, worded as in Telegram Desktop; those we lack are greyed out.
fn admin_switches(ui: &mut Ui, r: &mut AdminRights, me: Powers) {
    section_title(ui, l!("Що може цей адміністратор", "What can this admin do"));
    let m = if me.owner { AdminRights::all() } else { me.rights };
    let item = |ui: &mut Ui, on: &mut bool, allowed: bool, label: &str| {
        ui.add_enabled(allowed, egui::Checkbox::new(on, label));
    };
    if me.channel {
        item(ui, &mut r.change_info, m.change_info, l!("Змінювати інформацію про канал", "Change channel info"));
        item(ui, &mut r.post_messages, m.post_messages, l!("Публікувати повідомлення", "Post messages"));
        item(ui, &mut r.edit_messages, m.edit_messages, l!("Редагувати чужі повідомлення", "Edit messages of others"));
        item(ui, &mut r.delete_messages, m.delete_messages, l!("Видаляти чужі повідомлення", "Delete messages of others"));
        item(ui, &mut r.stories, m.stories, l!("Керувати історіями", "Manage stories"));
        item(ui, &mut r.invite_users, m.invite_users, l!("Додавати підписників", "Add subscribers"));
        item(ui, &mut r.manage_call, m.manage_call, l!("Керувати трансляціями", "Manage live streams"));
    } else {
        item(ui, &mut r.change_info, m.change_info, l!("Змінювати інформацію про групу", "Change group info"));
        item(ui, &mut r.delete_messages, m.delete_messages, l!("Видаляти повідомлення", "Delete messages"));
        item(ui, &mut r.ban_users, m.ban_users, l!("Блокувати користувачів", "Ban users"));
        item(ui, &mut r.invite_users, m.invite_users, l!("Запрошувати за посиланням", "Invite users via link"));
        item(ui, &mut r.pin_messages, m.pin_messages, l!("Закріплювати повідомлення", "Pin messages"));
        if me.forum {
            item(ui, &mut r.manage_topics, m.manage_topics, l!("Керувати темами", "Manage topics"));
        }
        item(ui, &mut r.stories, m.stories, l!("Керувати історіями", "Manage stories"));
        item(ui, &mut r.manage_call, m.manage_call, l!("Керувати відеочатами", "Manage video chats"));
        item(ui, &mut r.anonymous, m.anonymous, l!("Залишатися анонімним", "Remain anonymous"));
    }
    item(ui, &mut r.add_admins, m.add_admins, l!("Призначати адміністраторів", "Add new admins"));
}

/// A member's permissions; what the whole chat forbids stays off, as in Telegram Desktop.
fn member_switches(ui: &mut Ui, r: &mut ChatRights, d: ChatRights) {
    let item = |ui: &mut Ui, on: &mut bool, allowed: bool, label: &str| {
        if !allowed {
            *on = false;
        }
        ui.add_enabled(allowed, egui::Checkbox::new(on, label));
    };
    item(ui, &mut r.send_messages, d.send_messages, l!("Надсилати повідомлення", "Send messages"));
    item(ui, &mut r.send_media, d.send_media, l!("Надсилати медіа", "Send media"));
    item(ui, &mut r.send_stickers, d.send_stickers, l!("Надсилати стікери й GIF", "Send stickers and GIFs"));
    item(ui, &mut r.send_polls, d.send_polls, l!("Надсилати опитування", "Send polls"));
    item(ui, &mut r.embed_links, d.embed_links, l!("Попередній перегляд посилань", "Embed links"));
    item(ui, &mut r.invite_users, d.invite_users, l!("Додавати учасників", "Add members"));
    item(ui, &mut r.pin_messages, d.pin_messages, l!("Закріплювати повідомлення", "Pin messages"));
    item(ui, &mut r.change_info, d.change_info, l!("Змінювати інформацію про групу", "Change group info"));
}
