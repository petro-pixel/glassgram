//! The emoji tab of the panel (Emoji 17.0, `emoji.rs`) with skin tones as in Telegram Desktop,
//! and messages of one to three emoji drawn large without a bubble.

use super::*;

/// Panel cell side, in points, before it stretches to fill the row.
const CELL: f32 = 40.0;

#[derive(Default)]
pub(super) struct EmojiPanel {
    /// The skin tone picked for each emoji that has them, as Telegram Desktop remembers it.
    tones: HashMap<&'static str, char>,
    /// The emoji whose tone row is open, and the cell it opened from.
    picker: Option<(&'static str, Rect)>,
}

impl App {
    /// Emoji grid by category; a click adds the emoji to the input field, a right click (or a
    /// long press) on one with skin tones opens their row.
    pub(super) fn emoji_grid(&mut self, ui: &mut Ui, rect: Rect) {
        let mut picked = None;
        let mut child = ui.new_child(UiBuilder::new().max_rect(rect));
        child.set_clip_rect(rect);
        egui::ScrollArea::vertical().id_salt("emoji-grid").auto_shrink(false).show(&mut child, |ui| {
            let cols = ((rect.width() / CELL).floor() as usize).max(1);
            let cell = rect.width() / cols as f32 - 1.0;
            for cat in emoji::categories() {
                ui.label(egui::RichText::new((cat.title)()).size(12.5).color(ink_dim()));
                for row in cat.items.chunks(cols) {
                    ui.horizontal(|ui| {
                        ui.spacing_mut().item_spacing.x = 1.0;
                        for item in row {
                            let (r, resp) = ui.allocate_exact_size(vec2(cell, cell), Sense::click());
                            // Only cells on screen are drawn: each colour glyph is rendered once, on first sight.
                            if !ui.is_rect_visible(r) {
                                continue;
                            }
                            if resp.hovered() {
                                ui.painter().rect_filled(r, 10.0, glass::veil(22));
                            }
                            let tone = self.emoji_panel.tones.get(item.emoji).copied();
                            let shown = emoji::with_tone(item.emoji, tone);
                            ui.painter().text(r.center(), Align2::CENTER_CENTER, &shown, FontId::proportional(cell * 0.62), ink());
                            if item.toned && (resp.secondary_clicked() || resp.long_touched()) {
                                self.emoji_panel.picker = Some((item.emoji, r));
                            } else if resp.clicked() {
                                picked = Some(shown);
                            }
                        }
                    });
                }
                ui.add_space(6.0);
            }
        });
        if let Some(e) = self.tone_picker(ui) {
            picked = Some(e);
        }
        if let Some(e) = picked {
            self.draft.push_str(&e);
            self.draft_edited_at = ui.input(|i| i.time);
        }
    }

    /// The row of the emoji without a tone and in the five tones, above its cell.
    fn tone_picker(&mut self, ui: &mut Ui) -> Option<String> {
        let (base, at) = self.emoji_panel.picker?;
        let side = 40.0;
        let size = vec2(side * 6.0 + 12.0, side + 12.0);
        let screen = ui.ctx().content_rect();
        let x = (at.center().x - size.x / 2.0).clamp(screen.left() + 8.0, screen.right() - size.x - 8.0);
        let pos = pos2(x, at.top() - size.y - 4.0);
        let mut chosen = None;
        let area = egui::Area::new(egui::Id::unique("emoji-tones")).order(egui::Order::Foreground).fixed_pos(pos).show(ui.ctx(), |ui| {
            let (frame, _) = ui.allocate_exact_size(size, Sense::hover());
            // Dense enough to read over the glass panel under it.
            glass::glass(ui.painter(), frame, Glass::tinted(frame.height() / 2.0, glass::chrome(), 235).with_shadow());
            for (i, tone) in std::iter::once(None).chain(emoji::TONES.map(Some)).enumerate() {
                let r = Rect::from_min_size(frame.min + vec2(6.0 + side * i as f32, 6.0), vec2(side, side));
                let resp = ui.interact(r, ui.scope_id().with(("tone", i)), Sense::click());
                if resp.hovered() || self.emoji_panel.tones.get(base).copied() == tone {
                    ui.painter().rect_filled(r, side / 2.0, glass::veil(if resp.hovered() { 30 } else { 18 }));
                }
                ui.painter().text(r.center(), Align2::CENTER_CENTER, emoji::with_tone(base, tone), FontId::proportional(25.0), ink());
                if resp.clicked() {
                    chosen = Some(tone);
                }
            }
        });
        let away = ui.input(|i| i.pointer.any_pressed() || i.key_pressed(egui::Key::Escape)) && !area.response.contains_pointer();
        if let Some(tone) = chosen {
            match tone {
                Some(t) => self.emoji_panel.tones.insert(base, t),
                None => self.emoji_panel.tones.remove(base),
            };
            self.emoji_panel.picker = None;
            return Some(emoji::with_tone(base, tone));
        }
        // The press that opened the row is the right click itself, so only a later one closes it.
        if away && !ui.input(|i| i.pointer.secondary_pressed()) {
            self.emoji_panel.picker = None;
        }
        None
    }
}

/// How many emoji a plain text message is, when Telegram Desktop shows it large: nothing but
/// one to three emoji, without media, formatting, a reply, a forward or buttons.
pub(super) fn big_emoji(m: &Msg) -> Option<usize> {
    let plain = m.spans.is_empty()
        && m.media.is_none()
        && !m.photo
        && m.file.is_none()
        && m.webpage.is_none()
        && m.poll.is_none()
        && m.todo.is_none()
        && m.geo.is_none()
        && m.contact.is_none()
        && m.forwarded.is_none()
        && m.reply_to.is_none()
        && m.buttons.is_empty();
    plain.then(|| emoji::big_count(&m.text)).flatten()
}

/// Large emoji: no bubble, like a sticker, with the time in a small glass chip.
pub(super) fn big_emoji_bubble(ui: &mut Ui, m: &Msg, count: usize, look: Look) -> Option<Action> {
    // Telegram Desktop shrinks them as there are more.
    let font = FontId::proportional(match count {
        1 => 64.0,
        2 => 52.0,
        _ => 44.0,
    });
    let g = ui.painter().layout_no_wrap(m.text.trim().to_owned(), font, ink());
    let avail = ui.available_width();
    let (row, resp) = ui.allocate_exact_size(vec2(avail, g.size().y + 30.0), Sense::click());
    pick_mark(ui, row, look);
    if look.picking && resp.clicked() {
        return Some(Action::Pick(m.id));
    }
    let w = g.size().x.max(60.0);
    let x = if m.out { row.right() - 10.0 - w } else { row.left() + 10.0 + if look.picking { 30.0 } else { 0.0 } };
    let r = Rect::from_min_size(pos2(x, row.top() + 2.0), vec2(w, g.size().y));
    let p = ui.painter();
    p.galley(pos2(if m.out { r.right() - g.size().x } else { r.left() }, r.top()), g, ink());
    let meta_text = format!("{}{}", if m.edited { l!("ред. ", "edited ") } else { "" }, m.time);
    let meta = p.layout_no_wrap(meta_text, FontId::proportional(11.0), ink());
    let cx = if m.out { r.right() - meta.size().x - 12.0 } else { r.left() };
    let chip = Rect::from_min_size(pos2(cx, r.bottom() + 2.0), meta.size() + vec2(12.0, 6.0));
    glass::glass(p, chip, Glass::tinted(chip.height() / 2.0, glass::chrome(), 120).frosted(1.0));
    p.galley(chip.min + vec2(6.0, 3.0), meta, ink());
    let mut action = None;
    if !m.reactions.is_empty() {
        action = reaction_chips(ui, m.id, &m.reactions, r.left(), m.out, r.right());
    }
    resp.context_menu(|ui| message_menu(ui, m, look.chat, &mut action));
    if resp.double_clicked() {
        action = Some(Action::Reply(m.id));
    }
    action
}
