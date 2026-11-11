//! Demo message effects and view-once media: messages sent with effects, a view-once photo, and
//! an effect list answered like `messages.getAvailableEffects`, each effect a drawn confetti burst.

use crate::app::App;
use crate::backend::{Effect, Ev, Msg, VIEW_ONCE};

/// The incoming view-once photo (its picture comes from `images()`).
pub(super) const VIEW_ONCE_PHOTO: i32 = 91;

/// (id, emoji, premium, confetti hue).
const EFFECTS: [(i64, &str, bool, f32); 8] = [
    (7001, "👍", false, 0.12),
    (7002, "👎", false, 0.6),
    (7003, "❤", false, 0.97),
    (7004, "🔥", false, 0.06),
    (7005, "🎉", false, 0.8),
    (7006, "💩", false, 0.08),
    (7007, "🤩", true, 0.15),
    (7008, "🥳", true, 0.5),
];

/// A message with an effect that just came (it plays when shown), a view-once photo, and one of
/// ours with an effect (a click on its badge plays it).
pub(super) fn messages(base: Msg) -> Vec<Msg> {
    let now = jiff::Timestamp::now().as_second();
    vec![
        Msg { id: VIEW_ONCE_PHOTO, photo: true, ttl: Some(VIEW_ONCE), ..base.clone() },
        Msg { id: 92, out: true, sender: String::new(), text: "Thank you! ❤".into(), effect: Some(7003), ..base.clone() },
        Msg { id: 90, text: "We did it, the release is out! 🚀".into(), effect: Some(7005), ts: now, ..base },
    ]
}

pub(super) fn load(app: &mut App) {
    app.push_event(Ev::Effects(EFFECTS.iter().map(|(id, emoji, premium, _)| Effect { id: *id, emoji: (*emoji).into(), premium: *premium }).collect()));
    for (id, _, _, hue) in EFFECTS {
        let path = std::env::temp_dir().join(format!("glassgram-demo-effect-{id}.json"));
        if std::fs::write(&path, confetti(hue)).is_ok() {
            app.push_event(Ev::EffectFile { id, path });
        }
    }
}

/// Lottie JSON: pieces of confetti burst leftwards from the right edge and fade.
fn confetti(hue: f32) -> String {
    const N: u32 = 18;
    let rgb = |h: f32| {
        let f = |n: f32| {
            let k = (n + h * 6.0) % 6.0;
            1.0 - (k.min(4.0 - k).clamp(0.0, 1.0))
        };
        [f(5.0), f(3.0), f(1.0)]
    };
    let layers: Vec<String> = (0..N)
        .map(|i| {
            // A stable scatter per piece.
            let r = |k: u32| {
                let mut x = (i * 16 + k).wrapping_mul(0x9e37_79b9) ^ 0x85eb_ca6b;
                x ^= x >> 15;
                x = x.wrapping_mul(0x2c1b_3c6d);
                x ^= x >> 12;
                (x & 0xffff) as f32 / 65535.0
            };
            let (x0, y0) = (430.0, 256.0 + (r(1) - 0.5) * 40.0);
            let (x1, y1) = (60.0 + r(2) * 260.0, 40.0 + r(3) * 430.0);
            let c = rgb((hue + (r(4) - 0.5) * 0.25).rem_euclid(1.0));
            let size = 14.0 + r(5) * 16.0;
            let end = 40 + (r(6) * 20.0) as u32;
            format!(
                r#"{{"ty":4,"ind":{ind},"ip":0,"op":60,"st":0,"ks":{{"o":{{"a":1,"k":[{{"t":0,"s":[100],"i":{{"x":[0.5],"y":[1]}},"o":{{"x":[0.5],"y":[0]}}}},{{"t":{fade},"s":[100],"i":{{"x":[0.5],"y":[1]}},"o":{{"x":[0.5],"y":[0]}}}},{{"t":{end},"s":[0]}}]}},"r":{{"a":0,"k":0}},"p":{{"a":1,"k":[{{"t":0,"s":[{x0},{y0},0],"i":{{"x":[0.2],"y":[1]}},"o":{{"x":[0.3],"y":[0]}}}},{{"t":{end},"s":[{x1},{y1},0]}}]}},"a":{{"a":0,"k":[0,0,0]}},"s":{{"a":0,"k":[100,100,100]}}}},"shapes":[{{"ty":"el","p":{{"a":0,"k":[0,0]}},"s":{{"a":0,"k":[{size},{size}]}}}},{{"ty":"fl","c":{{"a":0,"k":[{c0},{c1},{c2},1]}},"o":{{"a":0,"k":100}}}}]}}"#,
                ind = i + 1,
                fade = end / 2,
                c0 = c[0],
                c1 = c[1],
                c2 = c[2],
            )
        })
        .collect();
    format!(r#"{{"v":"5.5.2","fr":30,"ip":0,"op":60,"w":512,"h":512,"layers":[{}]}}"#, layers.join(","))
}
