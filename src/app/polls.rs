//! Polls: the card in a bubble, the "New poll" box as in Telegram Desktop (question, answers with a
//! new empty row appearing as you type, anonymous voting, multiple answers, quiz mode, time limit and
//! the 2026 options), "View results" with voters per answer, and adding an answer to an open poll.

use super::*;
use crate::backend::{NewPoll, PollInfo, PollVoter};

/// Limits of the official clients.
const MAX_OPTIONS: usize = 10;
const QUESTION_MAX: usize = 255;
const OPTION_MAX: usize = 100;
const SOLUTION_MAX: usize = 200;

/// Voting time limits offered in the box, in seconds.
const PERIODS: [i32; 3] = [3600, 86_400, 7 * 86_400];

#[derive(Default)]
pub(super) struct Polls {
    /// "New poll" box.
    form: Option<PollForm>,
    /// "View results" box.
    results: Option<Results>,
    /// "Add an answer" box: (chat, poll message, text).
    adding: Option<(PeerId, i32, String)>,
    /// "Stop poll?" confirmation: (chat, poll message).
    stopping: Option<(PeerId, i32)>,
}

/// "Retract vote" and "Stop poll" in a poll message's menu, as in Telegram Desktop.
pub(super) fn poll_menu(ui: &mut Ui, m: &Msg, action: &mut Option<Action>) {
    let Some(poll) = m.poll.as_ref().filter(|p| !p.closed && m.id > 0) else { return };
    // A quiz answer and a final vote can't be taken back.
    if poll.voted() && !poll.quiz && !poll.final_vote && glass::menu_button(ui, Icon::Close, l!("Відкликати голос", "Retract vote")).clicked() {
        *action = Some(Action::Vote(m.id, Vec::new()));
    }
    if m.out && glass::menu_button(ui, Icon::Stop, l!("Зупинити опитування", "Stop poll")).clicked() {
        *action = Some(Action::StopPoll(m.id));
    }
}

/// Voters of a public poll, page by page.
struct Results {
    peer: PeerId,
    id: i32,
    voters: Vec<PollVoter>,
    next: Option<String>,
    loading: bool,
}

pub(super) struct PollForm {
    peer: PeerId,
    question: String,
    options: Vec<String>,
    anonymous: bool,
    multiple: bool,
    quiz: bool,
    correct: Option<usize>,
    solution: String,
    /// Index into `PERIODS`; `None`: no time limit.
    period: Option<usize>,
    open_answers: bool,
    final_vote: bool,
    shuffle: bool,
    hide_results: bool,
    subscribers_only: bool,
    /// Channels always vote anonymously.
    channel: bool,
}

impl PollForm {
    fn filled(&self) -> Vec<(usize, &str)> {
        self.options.iter().enumerate().map(|(i, o)| (i, o.trim())).filter(|(_, o)| !o.is_empty()).collect()
    }

    /// What is still missing, for the disabled "Create" button's hint.
    fn problem(&self) -> Option<&'static str> {
        let q = self.question.trim().chars().count();
        if q == 0 || q > QUESTION_MAX {
            return Some(l!("Введіть питання", "Enter a question"));
        }
        if self.filled().len() < 2 {
            return Some(l!("Потрібно щонайменше два варіанти", "At least two answers are needed"));
        }
        if self.options.iter().any(|o| o.chars().count() > OPTION_MAX) {
            return Some(l!("Варіант задовгий", "An answer is too long"));
        }
        if self.quiz && !self.correct.is_some_and(|c| self.options.get(c).is_some_and(|o| !o.trim().is_empty())) {
            return Some(l!("Позначте правильну відповідь", "Mark the correct answer"));
        }
        None
    }
}

/// Dimmed backdrop and a centred glass panel; returns the panel's inner rect.
pub(super) fn modal(ui: &mut Ui, id: &str, w: f32, h: f32) -> Rect {
    let screen = ui.max_rect();
    ui.painter().rect_filled(screen, 0.0, Color32::from_black_alpha(90));
    ui.interact(screen, ui.scope_id().with((id, "bg")), Sense::click());
    let rect = Rect::from_center_size(screen.center(), vec2(screen.width().min(w) - 20.0, (screen.height() - 60.0).min(h)));
    glass::glass(ui.painter(), rect, Glass::panel(26.0).with_shadow());
    rect.shrink(18.0)
}

/// "2 h left" for a poll that closes by itself.
fn time_left(close: i64) -> Option<String> {
    let left = close - jiff::Timestamp::now().as_second();
    Some(match left {
        ..=0 => return None,
        ..3600 => lf!("ще {} хв", "{} min left", left / 60 + 1),
        ..86_400 => lf!("ще {} год", "{} h left", left / 3600),
        _ => lf!("ще {} дн", "{} d left", left / 86_400),
    })
}

fn when(ts: i64) -> String {
    jiff::Timestamp::from_second(ts).map(|t| t.to_zoned(jiff::tz::TimeZone::system()).strftime("%d.%m %H:%M").to_string()).unwrap_or_default()
}

fn question_galley(ui: &Ui, poll: &PollInfo, w: f32, k: f32) -> std::sync::Arc<egui::Galley> {
    ui.painter().layout(poll.question.clone(), FontId::new(15.0 * k, egui::FontFamily::Name(format::BOLD.into())), ink(), w)
}

/// Results are on view: voted or closed, unless hidden until the end.
fn results_shown(poll: &PollInfo) -> bool {
    poll.closed || (poll.voted() && !poll.hide_results)
}

/// Height of the poll card in a bubble.
pub(super) fn card_height(ui: &Ui, poll: &PollInfo, w: f32, k: f32, chosen: &[Vec<u8>]) -> f32 {
    let q = question_galley(ui, poll, w, k);
    let answering = !poll.voted() && !poll.closed;
    let button = poll.multiple && answering && !chosen.is_empty();
    let add = poll.open_answers && !poll.closed && poll.answers.len() < MAX_OPTIONS;
    q.size().y + (22.0 + poll.answers.len() as f32 * 40.0 + if add { 40.0 } else { 0.0 } + 20.0) * k + if button { 40.0 } else { 0.0 }
}

/// The poll card: answers to pick, or results with bars; "View results" for public polls.
pub(super) fn draw_card(ui: &mut Ui, m: &Msg, poll: &PollInfo, rect: Rect, k: f32, chosen: &[Vec<u8>]) -> Option<Action> {
    let p = ui.painter().clone();
    let mut action = None;
    let q = question_galley(ui, poll, rect.width(), k);
    let mut y = rect.top();
    let qh = q.size().y;
    p.galley(pos2(rect.left(), y), q, ink());
    y += qh + 2.0;
    let kind = match (poll.quiz, poll.public) {
        (true, true) => l!("Вікторина", "Quiz"),
        (true, false) => l!("Анонімна вікторина", "Anonymous quiz"),
        (false, true) => l!("Публічне опитування", "Public poll"),
        (false, false) => l!("Анонімне опитування", "Anonymous poll"),
    };
    let mut sub = kind.to_owned();
    if poll.multiple {
        sub = format!("{sub} · {}", l!("кілька відповідей", "multiple answers"));
    }
    if let Some(left) = poll.close_date.filter(|_| !poll.closed).and_then(time_left) {
        sub = format!("{sub} · {left}");
    }
    p.galley(pos2(rect.left(), y), elided(ui, &sub, 12.0 * k, rgba(ink(), 160), rect.width()), ink());
    y += 20.0 * k;
    let show = results_shown(poll);
    let answering = !poll.voted() && !poll.closed;
    let stroke = egui::Stroke::new(1.5, rgba(ink(), 180));
    for a in &poll.answers {
        let r = Rect::from_min_size(pos2(rect.left(), y), vec2(rect.width(), 36.0 * k));
        if show {
            let pct = if poll.total > 0 { a.voters as f32 / poll.total as f32 } else { 0.0 };
            p.text(pos2(r.left(), r.top() + 2.0), Align2::LEFT_TOP, format!("{:.0}%", pct * 100.0), FontId::proportional(13.0 * k), ink());
            let label = format!("{}{}", a.text, if a.chosen { "  ✓" } else { "" });
            let x = 44.0 * k;
            p.galley(pos2(r.left() + x, r.top() + 2.0), elided(ui, &label, 14.0 * k, ink(), r.width() - x), ink());
            let bar = Rect::from_min_size(pos2(r.left() + x, r.bottom() - 9.0), vec2((r.width() - x) * pct.max(0.02), 4.0));
            let col = if poll.quiz && a.correct { Color32::from_rgb(70, 220, 120) } else if a.chosen { ink() } else { rgba(ink(), 140) };
            p.rect_filled(bar, 2.0, col);
        } else {
            // Not answered yet, or voted with the results hidden until the end.
            let resp = ui.interact(r, ui.scope_id().with(("poll", m.id, &a.option)), Sense::click());
            if answering && resp.hovered() {
                p.rect_filled(r, 10.0, glass::veil(18));
                ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
            }
            let c = pos2(r.left() + 12.0 * k, r.center().y);
            if chosen.contains(&a.option) || a.chosen {
                p.circle_filled(c, 8.0 * k, ink());
            } else {
                p.circle_stroke(c, 8.0 * k, stroke);
            }
            let label = elided(ui, &a.text, 14.5 * k, ink(), r.width() - 34.0 * k);
            p.galley(pos2(r.left() + 30.0 * k, r.center().y - label.size().y / 2.0), label, ink());
            if answering && resp.clicked() {
                action = Some(if poll.multiple { Action::TogglePoll(m.id, a.option.clone()) } else { Action::Vote(m.id, vec![a.option.clone()]) });
            }
        }
        y += 40.0 * k;
    }
    if poll.open_answers && !poll.closed && poll.answers.len() < MAX_OPTIONS {
        let r = Rect::from_min_size(pos2(rect.left(), y), vec2(rect.width(), 36.0 * k));
        let resp = ui.interact(r, ui.scope_id().with(("poll-add", m.id)), Sense::click());
        if resp.hovered() {
            p.rect_filled(r, 10.0, glass::veil(18));
            ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
        }
        icons::draw(&p, Rect::from_center_size(pos2(r.left() + 12.0 * k, r.center().y), vec2(16.0, 16.0) * k), Icon::Plus, glass::link(m.out));
        p.text(pos2(r.left() + 30.0 * k, r.center().y), Align2::LEFT_CENTER, l!("Додати варіант", "Add an answer"), FontId::proportional(14.0 * k), glass::link(m.out));
        if resp.clicked() {
            action = Some(Action::AddPollAnswer(m.id));
        }
        y += 40.0 * k;
    }
    if poll.multiple && answering && !chosen.is_empty() {
        let b = Rect::from_min_size(pos2(rect.left(), y), vec2(rect.width(), 34.0));
        if glass::button(ui, b, ("vote", m.id), l!("Голосувати", "Vote"), true, true).clicked() {
            action = Some(Action::Vote(m.id, chosen.to_vec()));
        }
        y += 40.0;
    }
    let votes = match poll.total {
        0 => l!("Ще немає голосів", "No votes yet").to_owned(),
        n => format!("{}{}", lp!(n, ["{n} голос", "{n} голоси", "{n} голосів"], ["{n} vote", "{n} votes"]), if poll.closed { l!(" · завершено", " · closed") } else { "" }),
    };
    let votes = if poll.voted() && !show { format!("{votes} · {}", l!("результати після завершення", "results when it ends")) } else { votes };
    let small = FontId::proportional(12.0 * k);
    // Right side: the quiz explanation and "View results".
    let mut right = rect.right();
    if poll.public && show && poll.total > 0 {
        let label = l!("Результати", "View results");
        let g = p.layout_no_wrap(label.to_owned(), small.clone(), glass::link(m.out));
        let r = Rect::from_min_size(pos2(right - g.size().x, y), g.size());
        let resp = ui.interact(r.expand(4.0), ui.scope_id().with(("poll-results", m.id)), Sense::click());
        if resp.hovered() {
            ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
        }
        p.galley(r.min, g, glass::link(m.out));
        if resp.clicked() {
            action = Some(Action::PollResults(m.id));
        }
        right = r.left() - 10.0;
    }
    if let Some(s) = poll.solution.as_ref().filter(|_| poll.quiz && show) {
        let r = Rect::from_min_size(pos2(right - 16.0 * k, y - 1.0), vec2(16.0, 16.0) * k);
        icons::draw(&p, r, Icon::Info, rgba(ink(), 200));
        ui.interact(r.expand(4.0), ui.scope_id().with(("poll-solution", m.id)), Sense::hover()).on_hover_text(s);
        right = r.left() - 8.0;
    }
    p.galley(pos2(rect.left(), y), elided(ui, &votes, 12.0 * k, rgba(ink(), 160), right - rect.left()), ink());
    action
}

impl App {
    pub(super) fn open_poll_form(&mut self, peer: PeerId) {
        let channel = self.chats.iter().any(|c| c.id == peer && c.kind == ChatKind::Channel);
        self.polls.form = Some(PollForm {
            peer,
            question: String::new(),
            options: vec![String::new(), String::new()],
            anonymous: true,
            multiple: false,
            quiz: false,
            correct: None,
            solution: String::new(),
            period: None,
            open_answers: false,
            final_vote: false,
            shuffle: false,
            hide_results: false,
            subscribers_only: false,
            channel,
        });
    }

    /// "View results" and "Add an answer" from a poll card.
    pub(super) fn poll_action(&mut self, peer: PeerId, action: Action) {
        match action {
            Action::PollResults(id) => {
                self.polls.results = Some(Results { peer, id, voters: Vec::new(), next: None, loading: true });
                self.send(Cmd::PollVotes { peer, id, offset: None });
            }
            Action::AddPollAnswer(id) => self.polls.adding = Some((peer, id, String::new())),
            Action::StopPoll(id) => self.polls.stopping = Some((peer, id)),
            _ => {}
        }
    }

    pub(super) fn on_poll_votes(&mut self, peer: PeerId, id: i32, more: bool, voters: Vec<PollVoter>, next: Option<String>) {
        let Some(r) = self.polls.results.as_mut().filter(|r| r.peer == peer && r.id == id) else { return };
        if !more {
            r.voters.clear();
        }
        r.voters.extend(voters);
        r.next = next;
        r.loading = false;
    }

    /// The poll boxes: "New poll", "View results", "Add an answer".
    pub(super) fn polls_view(&mut self, ui: &mut Ui) {
        if self.polls.form.is_some() {
            self.poll_form_view(ui);
        } else if self.polls.results.is_some() {
            self.poll_results_view(ui);
        } else if self.polls.adding.is_some() {
            self.poll_add_view(ui);
        } else if self.polls.stopping.is_some() {
            self.poll_stop_view(ui);
        }
    }

    fn poll_stop_view(&mut self, ui: &mut Ui) {
        let Some((peer, id)) = self.polls.stopping else { return };
        let inner = modal(ui, "poll-stop", 380.0, 160.0);
        let title = match self.find(peer, id).and_then(|m| m.poll.as_ref()) {
            Some(p) if p.quiz => l!("Зупинити вікторину?", "Stop quiz?"),
            _ => l!("Зупинити опитування?", "Stop poll?"),
        };
        ui.painter().text(inner.left_top(), Align2::LEFT_TOP, title, FontId::proportional(17.0), ink());
        let body = l!("Після цього ніхто не зможе голосувати. Цю дію не можна скасувати.", "Nobody will be able to vote after this. This cannot be undone.");
        let g = ui.painter().layout_job(LayoutJob::simple(body.to_owned(), FontId::proportional(14.0), ink_dim(), inner.width()));
        ui.painter().galley(pos2(inner.left(), inner.top() + 32.0), g, ink_dim());
        let buttons = Rect::from_min_max(pos2(inner.left(), inner.bottom() - 36.0), inner.max);
        let half = (buttons.width() - 8.0) / 2.0;
        let esc = ui.input_mut(|i| i.consume_key(egui::Modifiers::NONE, Key::Escape));
        let cancel = glass::button(ui, Rect::from_min_size(buttons.min, vec2(half, buttons.height())), "poll-stop-cancel", l!("Скасувати", "Cancel"), false, true).clicked();
        let stop = glass::button(ui, Rect::from_min_size(pos2(buttons.right() - half, buttons.top()), vec2(half, buttons.height())), "poll-stop-ok", l!("Зупинити", "Stop"), true, true).clicked();
        if stop {
            self.polls.stopping = None;
            // Closed right away; the server's copy replaces it when the edit comes back.
            if let Some(p) = self.find_mut(peer, id).and_then(|m| m.poll.as_mut()) {
                p.closed = true;
            }
            self.send(Cmd::StopPoll { peer, id });
        } else if cancel || esc {
            self.polls.stopping = None;
        }
    }

    fn poll_results_view(&mut self, ui: &mut Ui) {
        let Some((peer, id)) = self.polls.results.as_ref().map(|r| (r.peer, r.id)) else { return };
        let Some(poll) = self.find(peer, id).and_then(|m| m.poll.clone()) else {
            self.polls.results = None;
            return;
        };
        let inner = modal(ui, "poll-results", 420.0, 560.0);
        let p = ui.painter().clone();
        p.text(inner.left_top(), Align2::LEFT_TOP, if poll.quiz { l!("Результати вікторини", "Quiz results") } else { l!("Результати опитування", "Poll results") }, FontId::proportional(17.0), ink());
        let close = Rect::from_center_size(pos2(inner.right() - 10.0, inner.top() + 10.0), vec2(32.0, 32.0));
        let esc = ui.input_mut(|i| i.consume_key(egui::Modifiers::NONE, Key::Escape));
        if glass::icon_button(ui, close, "poll-results-close", Icon::Close, false, true).clicked() || esc {
            self.polls.results = None;
            return;
        }
        let Some(r) = &self.polls.results else { return };
        let body = Rect::from_min_max(pos2(inner.left() - 4.0, inner.top() + 34.0), pos2(inner.right() + 4.0, inner.bottom()));
        let mut child = ui.new_child(UiBuilder::new().max_rect(body));
        child.set_clip_rect(body);
        let mut more = false;
        egui::ScrollArea::vertical().id_salt("poll-results").auto_shrink(false).show(&mut child, |ui| {
            let w = ui.available_width() - 8.0;
            ui.label(egui::RichText::new(&poll.question).size(14.5).color(ink()));
            if r.loading && r.voters.is_empty() {
                ui.add_space(24.0);
                ui.vertical_centered(|ui| ui.add(egui::Spinner::new().color(ink())));
                return;
            }
            for a in &poll.answers {
                ui.add_space(8.0);
                let pct = if poll.total > 0 { a.voters * 100 / poll.total } else { 0 };
                let head = format!("{} — {pct}%", a.text);
                ui.horizontal(|ui| {
                    ui.label(egui::RichText::new(head).size(13.5).color(if poll.quiz && a.correct { ACCENT } else { ink() }).strong());
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        ui.label(egui::RichText::new(lf!("Голосів: {}", "Votes: {}", a.voters)).size(12.5).color(ink_dim()));
                    });
                });
                for (i, v) in r.voters.iter().filter(|v| v.options.contains(&a.option)).enumerate() {
                    let (row, _) = ui.allocate_exact_size(vec2(w, 40.0), Sense::hover());
                    let tex = self.textures.get(&ImgKey::Avatar(v.peer));
                    glass::avatar(ui.painter(), pos2(row.left() + 18.0, row.center().y), 15.0, &v.name, v.peer.bare_id_unchecked() + i as i64, None, tex);
                    let time = ui.painter().layout_no_wrap(when(v.ts), FontId::proportional(12.0), ink_dim());
                    let name_w = row.width() - 50.0 - time.size().x - 8.0;
                    ui.painter().galley(pos2(row.left() + 42.0, row.center().y - 9.0), elided(ui, &v.name, 14.0, ink(), name_w), ink());
                    ui.painter().galley(pos2(row.right() - time.size().x, row.center().y - time.size().y / 2.0), time, ink_dim());
                }
            }
            if r.next.is_some() {
                ui.add_space(8.0);
                let (b, _) = ui.allocate_exact_size(vec2(w, 34.0), Sense::hover());
                more = glass::button(ui, b, "poll-votes-more", l!("Показати ще", "Show more"), false, !r.loading).clicked();
            }
        });
        if more && let Some(r) = &mut self.polls.results {
            r.loading = true;
            let (peer, id, offset) = (r.peer, r.id, r.next.clone());
            self.send(Cmd::PollVotes { peer, id, offset });
        }
    }

    fn poll_add_view(&mut self, ui: &mut Ui) {
        let inner = modal(ui, "poll-add", 400.0, 170.0);
        let Some((peer, id, text)) = &mut self.polls.adding else { return };
        ui.painter().text(inner.left_top(), Align2::LEFT_TOP, l!("Новий варіант", "New answer"), FontId::proportional(17.0), ink());
        let field = Rect::from_min_size(pos2(inner.left(), inner.top() + 36.0), vec2(inner.width(), 38.0));
        let resp = glass::text_field(ui, field, text, l!("Варіант відповіді", "Answer"), false);
        if !resp.has_focus() && text.is_empty() {
            resp.request_focus();
        }
        if text.chars().count() > OPTION_MAX {
            *text = text.chars().take(OPTION_MAX).collect();
        }
        let buttons = Rect::from_min_max(pos2(inner.left(), inner.bottom() - 36.0), inner.max);
        let half = (buttons.width() - 8.0) / 2.0;
        let ok = !text.trim().is_empty();
        let esc = ui.input_mut(|i| i.consume_key(egui::Modifiers::NONE, Key::Escape));
        let enter = resp.lost_focus() && ui.input(|i| i.key_pressed(Key::Enter));
        let cancel = glass::button(ui, Rect::from_min_size(buttons.min, vec2(half, buttons.height())), "poll-add-cancel", l!("Скасувати", "Cancel"), false, true).clicked();
        let add = glass::button(ui, Rect::from_min_size(pos2(buttons.right() - half, buttons.top()), vec2(half, buttons.height())), "poll-add-ok", l!("Додати", "Add"), true, ok).clicked();
        if (add || enter) && ok {
            let cmd = Cmd::AddPollAnswer { peer: *peer, id: *id, text: text.trim().to_owned() };
            self.polls.adding = None;
            self.send(cmd);
        } else if cancel || esc {
            self.polls.adding = None;
        }
    }

    fn poll_form_view(&mut self, ui: &mut Ui) {
        let inner = modal(ui, "poll-form", 440.0, 680.0);
        let Some(f) = &mut self.polls.form else { return };
        let title = if f.quiz { l!("Нова вікторина", "New quiz") } else { l!("Нове опитування", "New poll") };
        ui.painter().text(inner.left_top(), Align2::LEFT_TOP, title, FontId::proportional(17.0), ink());

        let buttons = Rect::from_min_max(pos2(inner.left(), inner.bottom() - 36.0), inner.max);
        let body = Rect::from_min_max(pos2(inner.left() - 4.0, inner.top() + 36.0), pos2(inner.right() + 4.0, buttons.top() - 10.0));
        let mut child = ui.new_child(UiBuilder::new().max_rect(body));
        child.set_clip_rect(body);
        let mut remove = None;
        egui::ScrollArea::vertical().id_salt("poll-form").auto_shrink(false).show(&mut child, |ui| {
            ui.spacing_mut().item_spacing.y = 6.0;
            let w = ui.available_width() - 8.0;
            section_title(ui, l!("Питання", "Question"));
            let r = ui.allocate_exact_size(vec2(w, 38.0), Sense::hover()).0;
            let q = glass::text_field(ui, r, &mut f.question, l!("Поставте питання", "Ask a question"), false);
            if f.question.is_empty() && !q.has_focus() && !ui.memory(|m| m.focused().is_some()) {
                q.request_focus();
            }
            section_title(ui, if f.quiz { l!("Варіанти (позначте правильний)", "Answers (mark the correct one)") } else { l!("Варіанти відповіді", "Answers") });
            let n = f.options.len();
            for i in 0..n {
                let (row, _) = ui.allocate_exact_size(vec2(w, 38.0), Sense::hover());
                let mut field = row;
                if f.quiz {
                    // Radio mark of the correct answer, left of the field.
                    let mark = Rect::from_center_size(pos2(row.left() + 14.0, row.center().y), vec2(28.0, 28.0));
                    let resp = ui.interact(mark, ui.scope_id().with(("poll-correct", i)), Sense::click());
                    let p = ui.painter();
                    if f.correct == Some(i) {
                        p.circle_filled(mark.center(), 10.0, ACCENT);
                        icons::draw(p, Rect::from_center_size(mark.center(), vec2(14.0, 14.0)), Icon::Check, glass::ON_DARK);
                    } else {
                        p.circle_stroke(mark.center(), 9.0, egui::Stroke::new(1.5, ink_dim()));
                    }
                    if resp.clicked() {
                        f.correct = Some(i);
                    }
                    field.min.x += 32.0;
                }
                // The trailing empty row is the "add an answer" row; the others can be removed.
                let last = i + 1 == n;
                if !last {
                    field.max.x -= 40.0;
                    let x = Rect::from_center_size(pos2(row.right() - 18.0, row.center().y), vec2(32.0, 32.0));
                    if glass::icon_button(ui, x, ("poll-remove", i), Icon::Close, false, n > 2).on_hover_text(l!("Прибрати", "Remove")).clicked() {
                        remove = Some(i);
                    }
                }
                let hint = if last { l!("Додати варіант", "Add an answer") } else { l!("Варіант", "Answer") };
                glass::text_field(ui, field, &mut f.options[i], hint, false);
            }
            let left = MAX_OPTIONS.saturating_sub(f.filled().len());
            ui.label(egui::RichText::new(lf!("Можна додати ще {left}", "You can add {left} more")).color(ink_dim()).size(12.5));

            section_title(ui, l!("Налаштування", "Settings"));
            if !f.channel {
                ui.checkbox(&mut f.anonymous, l!("Анонімне голосування", "Anonymous voting"));
            }
            ui.add_enabled(!f.quiz, egui::Checkbox::new(&mut f.multiple, l!("Кілька відповідей", "Multiple answers")));
            ui.add_enabled(!f.quiz, egui::Checkbox::new(&mut f.open_answers, l!("Учасники можуть додавати варіанти", "Voters can add answers")));
            ui.checkbox(&mut f.final_vote, l!("Голос не можна змінити", "Votes can't be changed"));
            ui.checkbox(&mut f.shuffle, l!("Перемішувати варіанти", "Shuffle answers"));
            if f.channel {
                ui.checkbox(&mut f.subscribers_only, l!("Голосують лише підписники", "Only subscribers can vote"));
            }
            if ui.checkbox(&mut f.quiz, l!("Режим вікторини", "Quiz mode")).changed() && f.quiz {
                f.multiple = false;
                f.open_answers = false;
            }

            section_title(ui, l!("Тривалість", "Duration"));
            ui.horizontal_wrapped(|ui| {
                ui.spacing_mut().item_spacing = vec2(6.0, 6.0);
                let labels = [l!("Без обмежень", "No limit"), l!("1 година", "1 hour"), l!("1 день", "1 day"), l!("1 тиждень", "1 week")];
                for (k, label) in labels.into_iter().enumerate() {
                    let pick = k.checked_sub(1);
                    // "No limit" is the longest label: it gets a wider capsule.
                    let unit = (w - 18.0) / 4.6;
                    let (r, _) = ui.allocate_exact_size(vec2(if k == 0 { unit * 1.6 } else { unit }, 30.0), Sense::hover());
                    if glass::button(ui, r, ("poll-period", k), label, f.period == pick, true).clicked() {
                        f.period = pick;
                    }
                }
            });
            if f.period.is_none() {
                f.hide_results = false;
            }
            ui.add_enabled(f.period.is_some(), egui::Checkbox::new(&mut f.hide_results, l!("Показати результати лише після завершення", "Show results only when it ends")));

            if f.quiz {
                ui.label(egui::RichText::new(l!("У вікторині одна правильна відповідь; змінити її після створення не можна.", "A quiz has one correct answer; it can't be changed later.")).color(ink_dim()).size(12.5));
                section_title(ui, l!("Пояснення", "Explanation"));
                let r = ui.allocate_exact_size(vec2(w, 38.0), Sense::hover()).0;
                glass::text_field(ui, r, &mut f.solution, l!("Показується після відповіді (необов'язково)", "Shown after answering (optional)"), false);
                if f.solution.chars().count() > SOLUTION_MAX {
                    f.solution = f.solution.chars().take(SOLUTION_MAX).collect();
                }
            }
        });
        if let Some(i) = remove {
            f.options.remove(i);
            f.correct = match f.correct {
                Some(c) if c == i => None,
                Some(c) if c > i => Some(c - 1),
                c => c,
            };
        }
        // Keep exactly one empty row at the end while there is room for more answers.
        while f.options.len() > 2 && f.options[f.options.len() - 1].trim().is_empty() && f.options[f.options.len() - 2].trim().is_empty() {
            f.options.pop();
        }
        if f.options.last().is_some_and(|o| !o.trim().is_empty()) && f.options.len() < MAX_OPTIONS {
            f.options.push(String::new());
        }

        let problem = f.problem();
        let half = (buttons.width() - 8.0) / 2.0;
        let esc = ui.input_mut(|i| i.consume_key(egui::Modifiers::NONE, Key::Escape));
        let cancel = glass::button(ui, Rect::from_min_size(buttons.min, vec2(half, buttons.height())), "poll-cancel", l!("Скасувати", "Cancel"), false, true).clicked();
        let create = glass::button(ui, Rect::from_min_size(pos2(buttons.right() - half, buttons.top()), vec2(half, buttons.height())), "poll-create", l!("Створити", "Create"), true, problem.is_none());
        let create = if let Some(p) = problem { create.on_disabled_hover_text(p) } else { create };
        if cancel || esc {
            self.polls.form = None;
        } else if create.clicked() && problem.is_none() {
            // Indices of the correct answer count only non-empty rows, as they are sent.
            let quiz = f.quiz.then(|| f.filled().iter().position(|(i, _)| Some(*i) == f.correct)).flatten();
            let poll = NewPoll {
                question: f.question.trim().to_owned(),
                options: f.filled().iter().map(|(_, o)| (*o).to_owned()).collect(),
                anonymous: f.anonymous || f.channel,
                multiple: f.multiple,
                quiz,
                solution: f.solution.trim().to_owned(),
                close_period: f.period.and_then(|k| PERIODS.get(k).copied()),
                open_answers: f.open_answers && !f.quiz,
                final_vote: f.final_vote,
                shuffle: f.shuffle,
                hide_results: f.hide_results,
                subscribers_only: f.subscribers_only && f.channel,
            };
            let peer = f.peer;
            self.polls.form = None;
            let reply_to = self.take_reply();
            self.send(Cmd::SendPoll { peer, poll, reply_to });
        }
    }
}
