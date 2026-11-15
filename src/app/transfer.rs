//! "Transfer ownership" from the admin rights editor, as in Telegram Desktop: shown to the owner
//! once every right is ticked, then a confirmation, a security check (two-step verification older
//! than 7 days, this session older than 24 hours) and the cloud password.

use super::*;
use crate::backend::{AdminRights, TransferStep};

#[derive(Clone, Copy, PartialEq)]
enum Step {
    Confirm,
    Password,
    /// The account may not transfer yet; `no_password`: it has no cloud password at all.
    Check { no_password: bool },
}

struct Ask {
    peer: PeerId,
    chat: String,
    channel: bool,
    user: ChatInfo,
    step: Step,
    password: String,
    error: Option<String>,
    /// Waiting for the server.
    busy: bool,
}

#[derive(Default)]
pub(super) struct Transfer {
    ask: Option<Ask>,
}

/// Telegram Desktop offers the transfer only to an admin who has every right the chat has.
pub(super) fn offered(r: &AdminRights, channel: bool, basic: bool, forum: bool) -> bool {
    if basic {
        // Basic group admins have no separate rights.
        return true;
    }
    let common = r.change_info && r.delete_messages && r.invite_users && r.manage_call && r.add_admins && r.stories;
    if channel {
        common && r.post_messages && r.edit_messages
    } else {
        common && r.ban_users && r.pin_messages && (r.manage_topics || !forum)
    }
}

impl App {
    pub(super) fn ask_transfer(&mut self, peer: PeerId, user: ChatInfo, channel: bool) {
        let chat = self.chats.iter().find(|c| c.id == peer).map(|c| c.title.clone()).unwrap_or_default();
        self.transfer.ask = Some(Ask { peer, chat, channel, user, step: Step::Confirm, password: String::new(), error: None, busy: false });
    }

    pub(super) fn on_transfer(&mut self, peer: PeerId, user: PeerId, step: TransferStep) {
        let Some(a) = self.transfer.ask.as_mut().filter(|a| a.peer == peer && a.user.id == user) else { return };
        a.busy = false;
        match step {
            TransferStep::AskPassword => {
                a.step = Step::Password;
                a.error = None;
            }
            TransferStep::SecurityCheck { no_password } => {
                a.step = Step::Check { no_password };
                a.error = None;
            }
            TransferStep::Failed(e) => a.error = Some(e),
            TransferStep::Done => {
                let name = a.user.title.clone();
                let text = if a.channel { lf!("{name} тепер власник каналу.", "{name} is now the owner of the channel.") } else { lf!("{name} тепер власник групи.", "{name} is now the owner of the group.") };
                self.transfer.ask = None;
                self.toast = Some((text, 0.0, false));
                // Our powers there changed: the box closes and the info reloads.
                self.close_manage();
                if self.info_details_mut(peer).is_some() {
                    self.send(Cmd::LoadChatInfo(peer));
                }
            }
        }
    }

    pub(super) fn transfer_view(&mut self, ui: &mut Ui) {
        let Some(a) = &mut self.transfer.ask else { return };
        let screen = ui.max_rect();
        ui.painter().rect_filled(screen, 0.0, Color32::from_black_alpha(90));
        ui.interact(screen, ui.scope_id().with("transfer-bg"), Sense::click());
        let rect = Rect::from_center_size(screen.center(), vec2(screen.width().min(400.0) - 20.0, 270.0));
        glass::glass(ui.painter(), rect, Glass::panel(26.0).with_shadow());
        let inner = rect.shrink(18.0);
        let title = match a.step {
            Step::Confirm if a.channel => l!("Передати права на канал", "Transfer channel ownership"),
            Step::Confirm => l!("Передати права на групу", "Transfer group ownership"),
            Step::Password => l!("Двоетапна перевірка", "Two-step verification"),
            Step::Check { .. } => l!("Перевірка безпеки", "Security check"),
        };
        ui.painter().galley(inner.min, elided(ui, title, 17.0, ink(), inner.width()), ink());
        let body = Rect::from_min_max(inner.min + vec2(0.0, 34.0), pos2(inner.right(), inner.bottom() - 46.0));
        let mut child = ui.new_child(UiBuilder::new().max_rect(body));
        child.set_clip_rect(body);
        let text = |ui: &mut Ui, s: &str, color| {
            ui.add(egui::Label::new(egui::RichText::new(s).color(color).size(14.0)).wrap());
        };
        let mut submit = false;
        match a.step {
            Step::Confirm => {
                let about = if a.channel {
                    lf!(
                        "Усі права власника каналу «{}» перейдуть до {}. Ви більше не будете його творцем, а новий власник зможе забрати ваші права адміністратора.",
                        "This will transfer the full owner rights for «{}» to {}. You will no longer be considered the creator of the channel, and the new owner will be free to remove any of your admin privileges.",
                        a.chat, a.user.title
                    )
                } else {
                    lf!(
                        "Усі права власника групи «{}» перейдуть до {}. Ви більше не будете її творцем, а новий власник зможе забрати ваші права адміністратора або навіть заблокувати вас.",
                        "This will transfer the full owner rights for «{}» to {}. You will no longer be considered the creator of the group, and the new owner will be free to remove any of your admin privileges or even ban you.",
                        a.chat, a.user.title
                    )
                };
                text(&mut child, &about, ink());
            }
            Step::Password => {
                text(&mut child, l!("Введіть ваш пароль, щоб завершити передачу.", "Please enter your password to complete the transfer."), ink());
                child.add_space(8.0);
                let r = child.allocate_exact_size(vec2(child.available_width(), 38.0), Sense::hover()).0;
                let field = glass::text_field(&mut child, r, &mut a.password, l!("Пароль", "Password"), true);
                if !field.has_focus() && !a.busy && a.password.is_empty() {
                    field.request_focus();
                }
                submit = field.lost_focus() && child.input(|i| i.key_pressed(Key::Enter));
            }
            Step::Check { no_password } => {
                let kind = if a.channel { l!("канал", "channel") } else { l!("групу", "group") };
                text(&mut child, &lf!("Передати {kind} користувачу {} можна, лише якщо:", "You can transfer this {kind} to {} only if you have:", a.user.title), ink());
                text(&mut child, l!("• двоетапну перевірку увімкнено понад 7 днів тому;", "• Enabled Two-Step Verification more than 7 days ago."), ink_dim());
                text(&mut child, l!("• вхід на цьому пристрої виконано понад 24 години тому.", "• Logged in on this device more than 24 hours ago."), ink_dim());
                if !no_password {
                    child.add_space(4.0);
                    text(&mut child, l!("Будь ласка, поверніться пізніше.", "Please come back later."), ink());
                }
            }
        }
        if let Some(e) = &a.error {
            child.label(egui::RichText::new(e).color(glass::DANGER).size(12.5));
        }
        let buttons = Rect::from_min_max(pos2(inner.left(), inner.bottom() - 36.0), inner.max);
        let half = (buttons.width() - 8.0) / 2.0;
        let esc = ui.input_mut(|i| i.consume_key(egui::Modifiers::NONE, Key::Escape));
        let cancel_label = if matches!(a.step, Step::Check { no_password: false }) { l!("Закрити", "Close") } else { l!("Скасувати", "Cancel") };
        let cancel = glass::button(ui, Rect::from_min_size(buttons.min, vec2(half, buttons.height())), "transfer-cancel", cancel_label, false, true).clicked();
        let main = match a.step {
            Step::Confirm | Step::Password => Some(l!("Змінити власника", "Change owner")),
            Step::Check { no_password: true } => Some(l!("Задати пароль", "Set password")),
            Step::Check { no_password: false } => None,
        };
        let ready = !a.busy && (a.step != Step::Password || !a.password.is_empty());
        let ok = main.is_some_and(|label| glass::button(ui, Rect::from_min_size(pos2(buttons.right() - half, buttons.top()), vec2(half, buttons.height())), "transfer-ok", label, true, ready).clicked());
        if cancel || esc {
            self.transfer.ask = None;
            return;
        }
        if !(ok || submit) || !ready {
            return;
        }
        let (peer, user) = (a.peer, a.user.id);
        match a.step {
            // The first call only asks whether the account may transfer.
            Step::Confirm => {
                a.busy = true;
                a.error = None;
                self.send(Cmd::TransferOwnership { peer, user, password: None });
            }
            Step::Password => {
                a.busy = true;
                a.error = None;
                let password = std::mem::take(&mut a.password);
                self.send(Cmd::TransferOwnership { peer, user, password: Some(password) });
            }
            Step::Check { .. } => {
                self.transfer.ask = None;
                self.close_manage();
                self.settings_open = true;
                self.open_settings_page(SettingsTab::Privacy);
            }
        }
    }
}
