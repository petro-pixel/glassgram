//! "New poll" box, as in Telegram Desktop: question, answers (a new empty row appears as you type),
//! anonymous voting, multiple answers, quiz mode with the correct answer and an explanation.

use super::*;
use crate::backend::NewPoll;

/// Limits of the official clients.
const MAX_OPTIONS: usize = 10;
const QUESTION_MAX: usize = 255;
const OPTION_MAX: usize = 100;
const SOLUTION_MAX: usize = 200;

pub(super) struct PollForm {
    peer: PeerId,
    question: String,
    options: Vec<String>,
    anonymous: bool,
    multiple: bool,
    quiz: bool,
    correct: Option<usize>,
    solution: String,
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


impl App {
    pub(super) fn open_poll_form(&mut self, peer: PeerId) {
        let channel = self.chats.iter().any(|c| c.id == peer && c.kind == ChatKind::Channel);
        self.poll_form = Some(PollForm {
            peer,
            question: String::new(),
            options: vec![String::new(), String::new()],
            anonymous: true,
            multiple: false,
            quiz: false,
            correct: None,
            solution: String::new(),
            channel,
        });
    }

    pub(super) fn poll_form_view(&mut self, ui: &mut Ui) {
        let Some(f) = &mut self.poll_form else { return };
        let screen = ui.max_rect();
        ui.painter().rect_filled(screen, 0.0, Color32::from_black_alpha(90));
        ui.interact(screen, ui.scope_id().with("poll-form-bg"), Sense::click());
        let rect = Rect::from_center_size(screen.center(), vec2(screen.width().min(440.0) - 20.0, (screen.height() - 60.0).min(640.0)));
        glass::glass(ui.painter(), rect, Glass::panel(26.0).with_shadow());
        let inner = rect.shrink(18.0);
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
            if ui.checkbox(&mut f.quiz, l!("Режим вікторини", "Quiz mode")).changed() && f.quiz {
                f.multiple = false;
            }
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
            self.poll_form = None;
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
            };
            let peer = f.peer;
            self.poll_form = None;
            let reply_to = self.reply_target();
            self.send(Cmd::SendPoll { peer, poll, reply_to });
        }
    }
}
