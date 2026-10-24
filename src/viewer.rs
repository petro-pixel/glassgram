//! Built-in photo viewer: fullscreen dimming, zoom with the wheel and double
//! click, dragging, arrow keys to move between the chat's photos.

use eframe::egui::{self, Align2, Color32, FontId, Key, Rect, Sense, TextureHandle, Ui, Vec2, pos2, vec2};
use grammers_client::session::types::PeerId;

use crate::glass::{self, Glass, ON_DARK as TEXT};
use crate::icons::Icon;
use crate::i18n::{l, lf};

const MAX_ZOOM: f32 = 8.0;

pub struct Viewer {
    pub peer: PeerId,
    pub id: i32,
    zoom: f32,
    pan: Vec2,
}

pub enum ViewerAction {
    Close,
    Prev,
    Next,
    Download,
}

impl Viewer {
    pub fn new(peer: PeerId, id: i32) -> Self {
        Self { peer, id, zoom: 1.0, pan: Vec2::ZERO }
    }

    /// Go to another photo, resetting zoom.
    pub fn show_id(&mut self, id: i32) {
        *self = Self::new(self.peer, id);
    }

    /// `index` is (photo number, total) for the caption; `loading` is whether the original is still loading.
    pub fn ui(&mut self, ui: &mut Ui, tex: Option<&TextureHandle>, caption: &str, index: (usize, usize), loading: bool) -> Option<ViewerAction> {
        let screen = ui.max_rect();
        let p = ui.painter().clone();
        p.rect_filled(screen, 0.0, Color32::from_black_alpha(225));
        let bg = ui.interact(screen, ui.scope_id().with("viewer-bg"), Sense::click_and_drag());
        let mut action = None;

        // Buttons at the top right and arrows on the sides.
        let btn = |i: f32| Rect::from_min_size(pos2(screen.right() - 58.0 - i * 52.0, screen.top() + 16.0), vec2(42.0, 42.0));
        if glass::icon_button(ui, btn(0.0), "viewer-close", Icon::Close, false, true).on_hover_text(l!("Закрити (Esc)", "Close (Esc)")).clicked() {
            action = Some(ViewerAction::Close);
        }
        if glass::icon_button(ui, btn(1.0), "viewer-save", Icon::Download, false, true).on_hover_text(l!("Зберегти", "Save")).clicked() {
            action = Some(ViewerAction::Download);
        }
        let side = |left: bool| {
            let x = if left { screen.left() + 20.0 } else { screen.right() - 66.0 };
            Rect::from_min_size(pos2(x, screen.center().y - 23.0), vec2(46.0, 46.0))
        };
        let (n, total) = index;
        if n > 1 && glass::icon_button(ui, side(true), "viewer-prev", Icon::ChevronLeft, false, true).clicked() {
            action = Some(ViewerAction::Prev);
        }
        if n < total && glass::icon_button(ui, side(false), "viewer-next", Icon::ChevronRight, false, true).clicked() {
            action = Some(ViewerAction::Next);
        }

        let area = screen.shrink2(vec2(90.0, 80.0));
        match tex {
            Some(t) => {
                let size = t.size_vec2();
                let fit = (area.width() / size.x).min(area.height() / size.y);
                // Wheel zoom relative to the cursor.
                let scroll = ui.input(|i| i.smooth_scroll_delta.y);
                if scroll != 0.0 && bg.hovered() {
                    let old = self.zoom;
                    self.zoom = (self.zoom * (1.0 + scroll * 0.002)).clamp(1.0, MAX_ZOOM);
                    if let Some(ptr) = bg.hover_pos() {
                        let from_center = ptr - area.center() - self.pan;
                        self.pan -= from_center * (self.zoom / old - 1.0);
                    }
                }
                if bg.double_clicked() {
                    self.zoom = if self.zoom > 1.01 { 1.0 } else { 2.5 };
                    self.pan = Vec2::ZERO;
                }
                if bg.dragged() && self.zoom > 1.0 {
                    self.pan += bg.drag_delta();
                }
                if self.zoom <= 1.0 {
                    self.pan = Vec2::ZERO;
                }
                let shown = size * fit * self.zoom;
                // Don't let the photo be dragged off screen.
                let slack = ((shown - area.size()) / 2.0).max(Vec2::ZERO);
                self.pan = self.pan.clamp(-slack, slack);
                let rect = Rect::from_center_size(area.center() + self.pan, shown);
                let clip = ui.painter().with_clip_rect(screen);
                glass::image_rounded(&clip, rect, 10.0 / self.zoom, t.id(), size);
                if bg.clicked() && !rect.contains(bg.interact_pointer_pos().unwrap_or_default()) {
                    action = Some(ViewerAction::Close);
                }
            }
            None => {
                ui.put(Rect::from_center_size(area.center(), vec2(40.0, 40.0)), egui::Spinner::new().size(40.0).color(TEXT));
            }
        }
        if loading && tex.is_some() {
            ui.put(Rect::from_min_size(pos2(screen.left() + 24.0, screen.top() + 26.0), vec2(22.0, 22.0)), egui::Spinner::new().size(22.0).color(TEXT));
        }

        // Caption and counter at the bottom.
        let label = if total > 1 { lf!("{n} з {total}", "{n} of {total}") } else { String::new() };
        let text = [label.as_str(), caption].iter().filter(|s| !s.is_empty()).copied().collect::<Vec<_>>().join("  ·  ");
        if !text.is_empty() {
            let g = p.layout(text, FontId::proportional(14.5), TEXT, (screen.width() - 200.0).max(100.0));
            let chip = Rect::from_center_size(pos2(screen.center().x, screen.bottom() - 40.0), g.size() + vec2(28.0, 14.0));
            glass::glass(&p, chip, Glass::tinted(16.0, Color32::from_rgb(28, 32, 70), 170).frosted(1.0));
            p.galley(chip.center() - g.size() / 2.0, g, TEXT);
        }
        if self.zoom > 1.01 {
            p.text(pos2(screen.left() + 24.0, screen.bottom() - 30.0), Align2::LEFT_CENTER, format!("{:.0}%", self.zoom * 100.0), FontId::proportional(13.0), TEXT);
        }

        ui.input(|i| {
            if i.key_pressed(Key::Escape) {
                action = Some(ViewerAction::Close);
            } else if i.key_pressed(Key::ArrowLeft) && n > 1 {
                action = Some(ViewerAction::Prev);
            } else if i.key_pressed(Key::ArrowRight) && n < total {
                action = Some(ViewerAction::Next);
            }
        });
        action
    }
}
