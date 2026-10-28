//! Emoji games, as in Telegram: a message of just 🎲 🎯 🏀 ⚽ 🎰 or 🎳 is sent as a game whose
//! result the server picks; the bubble shows the emoji big with the outcome under it.

use super::*;

/// The game emoji the server accepts (`appConfig.emojies_send_dice`).
pub(super) const GAMES: [&str; 6] = ["🎲", "🎯", "🏀", "⚽", "🎰", "🎳"];

/// The game a typed text starts, if it is just one of the emoji.
pub(super) fn game_for(text: &str) -> Option<&'static str> {
    let plain: String = text.trim().chars().filter(|c| *c != '\u{fe0f}').collect();
    GAMES.into_iter().find(|g| g.trim_end_matches('\u{fe0f}') == plain)
}

/// What the value means for each game (the official apps' animations end on these).
pub(super) fn outcome(emoji: &str, v: i32) -> String {
    match emoji {
        "🎲" => lf!("Випало {v}", "Rolled {v}"),
        "🎯" if v == 6 => l!("Прямо в ціль!", "Bullseye!").to_owned(),
        "🎯" if v == 1 => l!("Повз мішень", "Missed the board").to_owned(),
        "🏀" if v >= 4 => l!("Влучив!", "Scored!").to_owned(),
        "⚽" if v >= 3 => l!("Гол!", "Goal!").to_owned(),
        "🏀" | "⚽" => l!("Мимо", "Missed").to_owned(),
        "🎳" if v == 6 => l!("Страйк!", "Strike!").to_owned(),
        "🎳" => {
            // Values 1..5 knock down 0, 1, 3, 4 and 5 pins.
            let pins = [0, 1, 3, 4, 5].get((v - 1).max(0) as usize).copied().unwrap_or(0);
            lf!("Збито кеглів: {pins}", "Pins down: {pins}")
        }
        "🎰" => {
            // 1..64 encodes three reels, four symbols each.
            let reel = ["BAR", "🍇", "🍋", "7"];
            let k = (v - 1).clamp(0, 63) as usize;
            let line = [reel[k % 4], reel[k / 4 % 4], reel[k / 16]].join(" ");
            if v == 64 { lf!("Джекпот! {line}", "Jackpot! {line}") } else { line }
        }
        _ => lf!("Очок: {v}", "Score: {v}"),
    }
}

/// A game message: no bubble, like a sticker, with its outcome and the time in a glass chip.
pub(super) fn dice_bubble(ui: &mut Ui, m: &Msg, emoji: &str, value: i32, look: Look) -> Option<Action> {
    const SIZE: f32 = 120.0;
    let avail = ui.available_width();
    let (row, resp) = ui.allocate_exact_size(vec2(avail, SIZE + 40.0), Sense::click());
    pick_mark(ui, row, look);
    if look.picking && resp.clicked() {
        return Some(Action::Pick(m.id));
    }
    let x = if m.out { row.right() - 10.0 - SIZE } else { row.left() + 10.0 + if look.picking { 30.0 } else { 0.0 } };
    let r = Rect::from_min_size(pos2(x, row.top() + 4.0), vec2(SIZE, SIZE));
    let p = ui.painter();
    p.text(r.center(), Align2::CENTER_CENTER, emoji, FontId::proportional(78.0), ink());
    // Until the server answers, a just-sent game has no value yet.
    let label = if value > 0 { format!("{}  ·  {}", outcome(emoji, value), m.time) } else { m.time.clone() };
    let g = p.layout_no_wrap(label, FontId::proportional(12.0), ink());
    let w = g.size().x + 16.0;
    let cx = if m.out { r.right() - w } else { r.left() };
    let chip = Rect::from_min_size(pos2(cx, r.bottom() + 6.0), vec2(w, g.size().y + 8.0));
    glass::glass(p, chip, Glass::tinted(chip.height() / 2.0, glass::chrome(), 120).frosted(1.0));
    p.galley(chip.min + vec2(8.0, 4.0), g, ink());
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn games_and_outcomes() {
        assert_eq!(game_for(" 🎲 "), Some("🎲"));
        assert_eq!(game_for("🎲🎲"), None);
        assert_eq!(game_for("hi"), None);
        assert!(outcome("🎰", 64).contains("7 7 7"));
        assert_eq!(outcome("🎰", 1), "BAR BAR BAR");
    }
}
