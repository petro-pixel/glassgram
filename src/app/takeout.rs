//! Settings → Advanced → "Export Telegram data", as Telegram Desktop's export box: what to export
//! (account, contacts, sessions, chats by type, media by type up to a size), the format and the
//! folder; then progress with a stop button, Telegram's 24-hour wait, or the finished folder.

use super::*;
use crate::backend::{TakeoutEv, TakeoutOptions, TakeoutProgress, human_size};
use crate::export::Format;

/// Size limits offered for media other than photos, in MB.
const SIZE_LIMITS: [u32; 5] = [1, 8, 50, 200, 4000];

enum Stage {
    /// Choosing what to export.
    Setup,
    Running(TakeoutProgress),
    /// Telegram allows the export from `until` (unix seconds).
    Delay { until: i64 },
    Done { dir: PathBuf, progress: TakeoutProgress },
    Failed(String),
}

#[derive(Default)]
pub(super) struct Takeout {
    /// The box is shown (the export may go on with it hidden).
    open: bool,
    opts: TakeoutOptions,
    /// `None` until the box is first opened.
    stage: Option<Stage>,
}

impl Takeout {
    fn running(&self) -> bool {
        matches!(self.stage, Some(Stage::Running(_)))
    }
}

impl App {
    pub(super) fn takeout_event(&mut self, ev: TakeoutEv) {
        let t = &mut self.takeout;
        // A late progress report after a stop must not bring the box back to "running".
        if !t.running() && matches!(ev, TakeoutEv::Progress(_)) {
            return;
        }
        t.stage = Some(match ev {
            TakeoutEv::Progress(p) => Stage::Running(p),
            TakeoutEv::Delay { until } => Stage::Delay { until },
            TakeoutEv::Done { dir, progress } => {
                if !t.open {
                    self.toast = Some((l!("Дані експортовано", "Your data is exported").into(), 0.0, false));
                }
                Stage::Done { dir, progress }
            }
            TakeoutEv::Cancelled => Stage::Setup,
            TakeoutEv::Failed(e) => Stage::Failed(e),
        });
    }

    /// The entry in Settings → Advanced.
    pub(super) fn takeout_section(&mut self, ui: &mut Ui) {
        section_title(ui, l!("Експорт даних", "Data export"));
        let label = if let Some(Stage::Running(p)) = &self.takeout.stage {
            lf!("Експорт триває… {} з {}", "Exporting… {} of {}", p.chats_done, p.chats_total)
        } else {
            l!("Експортувати дані Telegram", "Export Telegram data").to_owned()
        };
        let (r, _) = ui.allocate_exact_size(vec2(ui.available_width().min(280.0), 32.0), Sense::hover());
        if glass::button(ui, r, "takeout-open", &label, false, true).clicked() {
            let t = &mut self.takeout;
            t.open = true;
            if !matches!(t.stage, Some(Stage::Running(_) | Stage::Delay { .. })) {
                t.stage = Some(Stage::Setup);
            }
        }
    }

    pub(super) fn takeout_view(&mut self, ui: &mut Ui) {
        if !self.takeout.open {
            return;
        }
        let screen = ui.max_rect();
        ui.painter().rect_filled(screen, 0.0, Color32::from_black_alpha(90));
        ui.interact(screen, ui.scope_id().with("takeout-bg"), Sense::click());
        let setup = matches!(self.takeout.stage, Some(Stage::Setup) | None);
        let h = if setup { (screen.height() - 60.0).min(640.0) } else { 300.0 };
        let rect = Rect::from_center_size(screen.center(), vec2(screen.width().min(440.0) - 20.0, h));
        glass::glass(ui.painter(), rect, Glass::panel(26.0).with_shadow());
        let inner = rect.shrink(18.0);
        ui.painter().galley(inner.min, elided(ui, l!("Експорт ваших даних", "Export your data"), 17.0, ink(), inner.width()), ink());
        let body = Rect::from_min_max(inner.min + vec2(0.0, 34.0), pos2(inner.right(), inner.bottom() - 46.0));
        let mut child = ui.new_child(UiBuilder::new().max_rect(body));
        child.set_clip_rect(body);
        if setup {
            egui::ScrollArea::vertical().id_salt("takeout-setup").auto_shrink(false).show(&mut child, |ui| takeout_setup(ui, &mut self.takeout.opts));
        } else {
            self.takeout_status(&mut child);
        }
        let buttons = Rect::from_min_max(pos2(inner.left(), inner.bottom() - 36.0), inner.max);
        let half = (buttons.width() - 8.0) / 2.0;
        let left = Rect::from_min_size(buttons.min, vec2(half, buttons.height()));
        let right = Rect::from_min_size(pos2(buttons.right() - half, buttons.top()), vec2(half, buttons.height()));
        let esc = ui.input_mut(|i| i.consume_key(egui::Modifiers::NONE, Key::Escape));
        let t = &mut self.takeout;
        match &t.stage {
            Some(Stage::Setup) | None => {
                let ready = t.opts.info || t.opts.contacts || t.opts.sessions || t.opts.any_chats();
                if glass::button(ui, left, "takeout-cancel", l!("Скасувати", "Cancel"), false, true).clicked() || esc {
                    t.open = false;
                }
                if glass::button(ui, right, "takeout-go", l!("Експортувати", "Export"), true, ready).clicked() && ready {
                    t.stage = Some(Stage::Running(TakeoutProgress { step: l!("Підготовка…", "Preparing…").into(), ..Default::default() }));
                    let opts = t.opts.clone();
                    self.send(Cmd::StartTakeout(opts));
                }
            }
            Some(Stage::Running(_)) => {
                // Hidden, the export goes on; Settings shows how far it got.
                if glass::button(ui, left, "takeout-hide", l!("Сховати", "Hide"), false, true).clicked() || esc {
                    t.open = false;
                }
                if glass::button(ui, right, "takeout-stop", l!("Зупинити", "Stop"), false, true).clicked() {
                    t.stage = Some(Stage::Setup);
                    self.send(Cmd::CancelTakeout);
                }
            }
            Some(Stage::Done { dir, .. }) => {
                let dir = dir.clone();
                if glass::button(ui, left, "takeout-close", l!("Закрити", "Close"), false, true).clicked() || esc {
                    t.open = false;
                }
                if glass::button(ui, right, "takeout-show", l!("Показати мої дані", "Show my data"), true, true).clicked() {
                    crate::platform::open(&dir);
                }
            }
            Some(Stage::Delay { .. } | Stage::Failed(_)) => {
                if glass::button(ui, left, "takeout-close", l!("Закрити", "Close"), false, true).clicked() || esc {
                    t.open = false;
                }
                if glass::button(ui, right, "takeout-back", l!("Налаштування", "Settings"), false, true).clicked() {
                    t.stage = Some(Stage::Setup);
                }
            }
        }
    }

    /// Progress, the wait or the result.
    fn takeout_status(&self, ui: &mut Ui) {
        let text = |ui: &mut Ui, s: &str, color| {
            ui.add(egui::Label::new(egui::RichText::new(s).color(color).size(14.0)).wrap());
        };
        let counts = |ui: &mut Ui, p: &TakeoutProgress| {
            if p.chats_total > 0 {
                text(ui, &lf!("Чати: {} з {}", "Chats: {} of {}", p.chats_done, p.chats_total), ink_dim());
            }
            text(ui, &lp!(p.messages, ["{n} повідомлення", "{n} повідомлення", "{n} повідомлень"], ["{n} message", "{n} messages"]), ink_dim());
            text(ui, &lf!("Файлів: {} · {}", "Files: {} · {}", p.files, human_size(p.bytes as usize)), ink_dim());
        };
        match &self.takeout.stage {
            Some(Stage::Running(p)) => {
                text(ui, &p.step, ink());
                ui.add_space(8.0);
                let frac = if p.chats_total > 0 { p.chats_done as f32 / p.chats_total as f32 } else { 0.0 };
                ui.add(egui::ProgressBar::new(frac).fill(ACCENT).desired_height(6.0).animate(p.chats_total == 0));
                ui.add_space(8.0);
                counts(ui, p);
            }
            Some(Stage::Delay { until }) => {
                let at = jiff::Timestamp::from_second(*until).unwrap_or_default().to_zoned(jiff::tz::TimeZone::system());
                text(ui, l!(
                    "З міркувань безпеки ви зможете почати завантаження даних за 24 години. Ми повідомили всі ваші пристрої про запит на експорт, щоб переконатися, що його дозволено, і дати вам час відреагувати, якщо ні.",
                    "For security reasons, you will be able to begin downloading your data in 24 hours. We have notified all your devices about the export request to make sure it's authorized and give you time to react if it's not."
                ), ink());
                ui.add_space(8.0);
                let (date, time) = (at.strftime("%d.%m.%Y").to_string(), at.strftime("%H:%M").to_string());
                text(ui, &lf!("Поверніться {date} о {time} і почніть експорт знову.", "Please come back on {date} at {time} and start the export again."), ink());
            }
            Some(Stage::Done { dir, progress }) => {
                text(ui, l!("Ваші дані успішно експортовано в теку:", "Your data is successfully exported to the following folder:"), ink());
                text(ui, &dir.display().to_string(), ink_dim());
                ui.add_space(8.0);
                counts(ui, progress);
            }
            Some(Stage::Failed(e)) => text(ui, e, glass::DANGER),
            Some(Stage::Setup) | None => {}
        }
    }
}

/// The choices, in the order of Telegram Desktop's export box.
fn takeout_setup(ui: &mut Ui, o: &mut TakeoutOptions) {
    let check = |ui: &mut Ui, on: &mut bool, label: &str| {
        ui.checkbox(on, egui::RichText::new(label).color(ink()).size(14.0));
    };
    section_title(ui, l!("Що експортувати", "Choose what to export"));
    check(ui, &mut o.info, l!("Інформація про акаунт", "Account information"));
    check(ui, &mut o.contacts, l!("Список контактів", "Contacts list"));
    check(ui, &mut o.sessions, l!("Активні сеанси", "Active sessions"));
    ui.add_space(8.0);
    section_title(ui, l!("Чати", "Chat export settings"));
    check(ui, &mut o.personal, l!("Особисті чати", "Personal chats"));
    check(ui, &mut o.bots, l!("Чати з ботами", "Bot chats"));
    check(ui, &mut o.private_groups, l!("Приватні групи", "Private groups"));
    check(ui, &mut o.private_channels, l!("Приватні канали", "Private channels"));
    check(ui, &mut o.public_groups, l!("Публічні групи", "Public groups"));
    if o.public_groups {
        ui.horizontal(|ui| {
            ui.add_space(24.0);
            check(ui, &mut o.only_my, l!("Лише мої повідомлення", "Only my messages"));
        });
    }
    check(ui, &mut o.public_channels, l!("Публічні канали", "Public channels"));
    ui.add_space(8.0);
    section_title(ui, l!("Медіа", "Media export settings"));
    check(ui, &mut o.photos, l!("Фото", "Photos"));
    check(ui, &mut o.videos, l!("Відеофайли", "Video files"));
    check(ui, &mut o.voice, l!("Голосові й відеоповідомлення", "Voice and video messages"));
    check(ui, &mut o.stickers, l!("Стікери", "Stickers"));
    check(ui, &mut o.files, l!("Файли", "Files"));
    if o.any_media() {
        ui.label(egui::RichText::new(l!("Обмеження розміру (крім фото)", "Size limit (photos excluded)")).color(ink()).size(14.0));
        ui.horizontal_wrapped(|ui| {
            ui.spacing_mut().item_spacing = vec2(6.0, 6.0);
            for mb in SIZE_LIMITS {
                let (r, _) = ui.allocate_exact_size(vec2(72.0, 30.0), Sense::hover());
                if glass::button(ui, r, ("takeout-mb", mb), &format!("{mb} MB"), o.max_mb == mb, true).clicked() {
                    o.max_mb = mb;
                }
            }
        });
    }
    ui.add_space(8.0);
    section_title(ui, l!("Розташування й формат", "Location and format"));
    ui.label(egui::RichText::new(o.dir.display().to_string()).color(ink_dim()).size(12.5));
    let (r, _) = ui.allocate_exact_size(vec2(140.0, 30.0), Sense::hover());
    if glass::button(ui, r, "takeout-dir", l!("Змінити теку", "Change folder"), false, true).clicked()
        && let Some(dir) = rfd::FileDialog::new().set_title(l!("Тека для експорту", "Export folder")).set_directory(&o.dir).pick_folder()
    {
        o.dir = dir;
    }
    ui.add_space(4.0);
    ui.horizontal_wrapped(|ui| {
        ui.spacing_mut().item_spacing = vec2(6.0, 6.0);
        for (f, label) in [(Format::Html, "HTML"), (Format::Json, "JSON"), (Format::Both, l!("Обидва", "Both"))] {
            let (r, _) = ui.allocate_exact_size(vec2(90.0, 30.0), Sense::hover());
            if glass::button(ui, r, ("takeout-format", label), label, o.format == f, true).clicked() {
                o.format = f;
            }
        }
    });
    ui.label(egui::RichText::new(l!("HTML — для читання, JSON — для програм.", "HTML is human-readable, JSON is machine-readable.")).color(ink_dim()).size(12.5));
}
