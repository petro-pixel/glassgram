//! Vector icons on a 24×24 grid, anti-aliased by egui's tessellator (unlike emoji from the raster atlas).
//! Outline icons are Lucide (https://lucide.dev, ISC licence, see `assets/icons/LUCIDE-LICENSE.txt`):
//! the same grid, stroke and round caps as our design code. Filled media controls are our own.

use std::collections::HashMap;
use std::sync::{Mutex, OnceLock};

use eframe::egui::{Color32, Painter, Pos2, Rect, Shape, Stroke, Vec2, pos2, vec2};
use kurbo::PathEl;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Icon {
    Send,
    Check,
    Close,
    Search,
    Reload,
    Power,
    Attach,
    Smile,
    Reply,
    Pencil,
    Pin,
    Muted,
    Channel,
    Bot,
    File,
    Camera,
    Bookmark,
    Forward,
    Trash,
    Copy,
    Download,
    Clock,
    Location,
    ChevronLeft,
    ChevronRight,
    Bell,
    Play,
    Pause,
    Mic,
    Stop,
    Gear,
    Lock,
    ChevronDown,
    /// Mention "@".
    At,
    /// Reaction.
    Heart,
    /// Views.
    Eye,
    /// List (all pinned).
    List,
    Link,
    /// Bot command menu: a slash in a square.
    Commands,
    Info,
    Phone,
    User,
    Chat,
    Folder,
    Globe,
    Laptop,
    /// Speech bubble for "All chats".
    Chats,
    Users,
    Archive,
    PinOff,
    BellOff,
    CheckCheck,
    /// Mark as unread: a bubble with a dot.
    MarkUnread,
    Unarchive,
    /// Export chat history.
    Export,
    Video,
    LockOpen,
    Plus,
    FolderPlus,
    /// Selection mode.
    Select,
    /// Open in the external player.
    PlayCircle,
}

/// Lucide SVG path data (`circle`, `rect` and `line` elements are converted to paths);
/// `None` for our own filled icons.
fn lucide(icon: Icon) -> Option<&'static [&'static str]> {
    Some(match icon {
        // check
        Icon::Check => &["M20 6 9 17l-5-5"],
        // x
        Icon::Close => &["M18 6 6 18", "m6 6 12 12"],
        // search
        Icon::Search => &["M3 11A8 8 0 1 0 19 11A8 8 0 1 0 3 11Z", "m21 21-4.3-4.3"],
        // refresh-cw
        Icon::Reload => &["M3 12a9 9 0 0 1 9-9 9.75 9.75 0 0 1 6.74 2.74L21 8", "M21 3v5h-5", "M21 12a9 9 0 0 1-9 9 9.75 9.75 0 0 1-6.74-2.74L3 16", "M8 16H3v5"],
        // power
        Icon::Power => &["M12 2v10", "M18.4 6.6a9 9 0 1 1-12.77.04"],
        // paperclip
        Icon::Attach => &["m21.44 11.05-9.19 9.19a6 6 0 0 1-8.49-8.49l8.57-8.57A4 4 0 1 1 18 8.84l-8.59 8.57a2 2 0 0 1-2.83-2.83l8.49-8.48"],
        // smile
        Icon::Smile => &["M2 12A10 10 0 1 0 22 12A10 10 0 1 0 2 12Z", "M8 14s1.5 2 4 2 4-2 4-2", "M9 9L9.01 9", "M15 9L15.01 9"],
        // reply
        Icon::Reply => &["M9 17L4 12L9 7", "M20 18v-2a4 4 0 0 0-4-4H4"],
        // forward
        Icon::Forward => &["M15 17L20 12L15 7", "M4 18v-2a4 4 0 0 1 4-4h12"],
        // pencil
        Icon::Pencil => &["M21.174 6.812a1 1 0 0 0-3.986-3.987L3.842 16.174a2 2 0 0 0-.5.83l-1.321 4.352a.5.5 0 0 0 .623.622l4.353-1.32a2 2 0 0 0 .83-.497z", "m15 5 4 4"],
        // pin
        Icon::Pin => &["M12 17v5", "M9 10.76a2 2 0 0 1-1.11 1.79l-1.78.9A2 2 0 0 0 5 15.24V16a1 1 0 0 0 1 1h12a1 1 0 0 0 1-1v-.76a2 2 0 0 0-1.11-1.79l-1.78-.9A2 2 0 0 1 15 10.76V7a1 1 0 0 1 1-1 2 2 0 0 0 0-4H8a2 2 0 0 0 0 4 1 1 0 0 1 1 1z"],
        // volume-x
        Icon::Muted => &["M11 4.702a.705.705 0 0 0-1.203-.498L6.413 7.587A1.4 1.4 0 0 1 5.416 8H3a1 1 0 0 0-1 1v6a1 1 0 0 0 1 1h2.416a1.4 1.4 0 0 1 .997.413l3.383 3.384A.705.705 0 0 0 11 19.298z", "M22 9L16 15", "M16 9L22 15"],
        // megaphone
        Icon::Channel => &["m3 11 18-5v12L3 14v-3z", "M11.6 16.8a3 3 0 1 1-5.8-1.6"],
        // bot
        Icon::Bot => &["M12 8V4H8", "M6 8H18A2 2 0 0 1 20 10V18A2 2 0 0 1 18 20H6A2 2 0 0 1 4 18V10A2 2 0 0 1 6 8Z", "M2 14h2", "M20 14h2", "M15 13v2", "M9 13v2"],
        // file
        Icon::File => &["M15 2H6a2 2 0 0 0-2 2v16a2 2 0 0 0 2 2h12a2 2 0 0 0 2-2V7Z", "M14 2v4a2 2 0 0 0 2 2h4"],
        // camera
        Icon::Camera => &["M14.5 4h-5L7 7H4a2 2 0 0 0-2 2v9a2 2 0 0 0 2 2h16a2 2 0 0 0 2-2V9a2 2 0 0 0-2-2h-3l-2.5-3z", "M9 13A3 3 0 1 0 15 13A3 3 0 1 0 9 13Z"],
        // bookmark
        Icon::Bookmark => &["m19 21-7-4-7 4V5a2 2 0 0 1 2-2h10a2 2 0 0 1 2 2v16z"],
        // trash-2
        Icon::Trash => &["M3 6h18", "M19 6v14c0 1-1 2-2 2H7c-1 0-2-1-2-2V6", "M8 6V4c0-1 1-2 2-2h4c1 0 2 1 2 2v2", "M10 11L10 17", "M14 11L14 17"],
        // copy
        Icon::Copy => &["M10 8H20A2 2 0 0 1 22 10V20A2 2 0 0 1 20 22H10A2 2 0 0 1 8 20V10A2 2 0 0 1 10 8Z", "M4 16c-1.1 0-2-.9-2-2V4c0-1.1.9-2 2-2h10c1.1 0 2 .9 2 2"],
        // download
        Icon::Download => &["M21 15v4a2 2 0 0 1-2 2H5a2 2 0 0 1-2-2v-4", "M7 10L12 15L17 10", "M12 15L12 3"],
        // clock
        Icon::Clock => &["M2 12A10 10 0 1 0 22 12A10 10 0 1 0 2 12Z", "M12 6L12 12L16 14"],
        // map-pin
        Icon::Location => &["M20 10c0 4.993-5.539 10.193-7.399 11.799a1 1 0 0 1-1.202 0C9.539 20.193 4 14.993 4 10a8 8 0 0 1 16 0", "M9 10A3 3 0 1 0 15 10A3 3 0 1 0 9 10Z"],
        // chevron-left
        Icon::ChevronLeft => &["m15 18-6-6 6-6"],
        // chevron-right
        Icon::ChevronRight => &["m9 18 6-6-6-6"],
        // chevron-down
        Icon::ChevronDown => &["m6 9 6 6 6-6"],
        // bell
        Icon::Bell => &["M6 8a6 6 0 0 1 12 0c0 7 3 9 3 9H3s3-2 3-9", "M10.3 21a1.94 1.94 0 0 0 3.4 0"],
        // mic
        Icon::Mic => &["M12 2a3 3 0 0 0-3 3v7a3 3 0 0 0 6 0V5a3 3 0 0 0-3-3Z", "M19 10v2a7 7 0 0 1-14 0v-2", "M12 19L12 22"],
        // settings
        Icon::Gear => &["M12.22 2h-.44a2 2 0 0 0-2 2v.18a2 2 0 0 1-1 1.73l-.43.25a2 2 0 0 1-2 0l-.15-.08a2 2 0 0 0-2.73.73l-.22.38a2 2 0 0 0 .73 2.73l.15.1a2 2 0 0 1 1 1.72v.51a2 2 0 0 1-1 1.74l-.15.09a2 2 0 0 0-.73 2.73l.22.38a2 2 0 0 0 2.73.73l.15-.08a2 2 0 0 1 2 0l.43.25a2 2 0 0 1 1 1.73V20a2 2 0 0 0 2 2h.44a2 2 0 0 0 2-2v-.18a2 2 0 0 1 1-1.73l.43-.25a2 2 0 0 1 2 0l.15.08a2 2 0 0 0 2.73-.73l.22-.39a2 2 0 0 0-.73-2.73l-.15-.08a2 2 0 0 1-1-1.74v-.5a2 2 0 0 1 1-1.74l.15-.09a2 2 0 0 0 .73-2.73l-.22-.38a2 2 0 0 0-2.73-.73l-.15.08a2 2 0 0 1-2 0l-.43-.25a2 2 0 0 1-1-1.73V4a2 2 0 0 0-2-2z", "M9 12A3 3 0 1 0 15 12A3 3 0 1 0 9 12Z"],
        // lock
        Icon::Lock => &["M5 11H19A2 2 0 0 1 21 13V20A2 2 0 0 1 19 22H5A2 2 0 0 1 3 20V13A2 2 0 0 1 5 11Z", "M7 11V7a5 5 0 0 1 10 0v4"],
        // at-sign
        Icon::At => &["M8 12A4 4 0 1 0 16 12A4 4 0 1 0 8 12Z", "M16 8v5a3 3 0 0 0 6 0v-1a10 10 0 1 0-4 8"],
        // heart
        Icon::Heart => &["M19 14c1.49-1.46 3-3.21 3-5.5A5.5 5.5 0 0 0 16.5 3c-1.76 0-3 .5-4.5 2-1.5-1.5-2.74-2-4.5-2A5.5 5.5 0 0 0 2 8.5c0 2.3 1.5 4.05 3 5.5l7 7Z"],
        // eye
        Icon::Eye => &["M2.062 12.348a1 1 0 0 1 0-.696 10.75 10.75 0 0 1 19.876 0 1 1 0 0 1 0 .696 10.75 10.75 0 0 1-19.876 0", "M9 12A3 3 0 1 0 15 12A3 3 0 1 0 9 12Z"],
        // list
        Icon::List => &["M3 12h.01", "M3 18h.01", "M3 6h.01", "M8 12h13", "M8 18h13", "M8 6h13"],
        // link
        Icon::Link => &["M10 13a5 5 0 0 0 7.54.54l3-3a5 5 0 0 0-7.07-7.07l-1.72 1.71", "M14 11a5 5 0 0 0-7.54-.54l-3 3a5 5 0 0 0 7.07 7.07l1.71-1.71"],
        // square-slash
        Icon::Commands => &["M5 3H19A2 2 0 0 1 21 5V19A2 2 0 0 1 19 21H5A2 2 0 0 1 3 19V5A2 2 0 0 1 5 3Z", "M9 15L15 9"],
        // info
        Icon::Info => &["M2 12A10 10 0 1 0 22 12A10 10 0 1 0 2 12Z", "M12 16v-4", "M12 8h.01"],
        // phone
        Icon::Phone => &["M22 16.92v3a2 2 0 0 1-2.18 2 19.79 19.79 0 0 1-8.63-3.07 19.5 19.5 0 0 1-6-6 19.79 19.79 0 0 1-3.07-8.67A2 2 0 0 1 4.11 2h3a2 2 0 0 1 2 1.72 12.84 12.84 0 0 0 .7 2.81 2 2 0 0 1-.45 2.11L8.09 9.91a16 16 0 0 0 6 6l1.27-1.27a2 2 0 0 1 2.11-.45 12.84 12.84 0 0 0 2.81.7A2 2 0 0 1 22 16.92z"],
        // user
        Icon::User => &["M19 21v-2a4 4 0 0 0-4-4H9a4 4 0 0 0-4 4v2", "M8 7A4 4 0 1 0 16 7A4 4 0 1 0 8 7Z"],
        // message-circle
        Icon::Chat => &["M7.9 20A9 9 0 1 0 4 16.1L2 22Z"],
        // folder
        Icon::Folder => &["M20 20a2 2 0 0 0 2-2V8a2 2 0 0 0-2-2h-7.9a2 2 0 0 1-1.69-.9L9.6 3.9A2 2 0 0 0 7.93 3H4a2 2 0 0 0-2 2v13a2 2 0 0 0 2 2Z"],
        // globe
        Icon::Globe => &["M2 12A10 10 0 1 0 22 12A10 10 0 1 0 2 12Z", "M12 2a14.5 14.5 0 0 0 0 20 14.5 14.5 0 0 0 0-20", "M2 12h20"],
        // laptop
        Icon::Laptop => &["M20 16V7a2 2 0 0 0-2-2H6a2 2 0 0 0-2 2v9m16 0H4m16 0 1.28 2.55a1 1 0 0 1-.9 1.45H3.62a1 1 0 0 1-.9-1.45L4 16"],
        // messages-square
        Icon::Chats => &["M14 9a2 2 0 0 1-2 2H6l-4 4V4a2 2 0 0 1 2-2h8a2 2 0 0 1 2 2z", "M18 9h2a2 2 0 0 1 2 2v11l-4-4h-6a2 2 0 0 1-2-2v-1"],
        // users
        Icon::Users => &["M16 21v-2a4 4 0 0 0-4-4H6a4 4 0 0 0-4 4v2", "M5 7A4 4 0 1 0 13 7A4 4 0 1 0 5 7Z", "M22 21v-2a4 4 0 0 0-3-3.87", "M16 3.13a4 4 0 0 1 0 7.75"],
        // archive
        Icon::Archive => &["M3 3H21A1 1 0 0 1 22 4V7A1 1 0 0 1 21 8H3A1 1 0 0 1 2 7V4A1 1 0 0 1 3 3Z", "M4 8v11a2 2 0 0 0 2 2h12a2 2 0 0 0 2-2V8", "M10 12h4"],
        // pin-off
        Icon::PinOff => &["M12 17v5", "M15 9.34V7a1 1 0 0 1 1-1 2 2 0 0 0 0-4H7.89", "m2 2 20 20", "M9 9v1.76a2 2 0 0 1-1.11 1.79l-1.78.9A2 2 0 0 0 5 15.24V16a1 1 0 0 0 1 1h11"],
        // bell-off
        Icon::BellOff => &["M8.7 3A6 6 0 0 1 18 8a21.3 21.3 0 0 0 .6 5", "M17 17H3s3-2 3-9a4.67 4.67 0 0 1 .3-1.7", "M10.3 21a1.94 1.94 0 0 0 3.4 0", "m2 2 20 20"],
        // check-check
        Icon::CheckCheck => &["M18 6 7 17l-5-5", "m22 10-7.5 7.5L13 16"],
        // message-square-dot
        Icon::MarkUnread => &["M11.7 3H5a2 2 0 0 0-2 2v16l4-4h12a2 2 0 0 0 2-2v-2.7", "M15 6A3 3 0 1 0 21 6A3 3 0 1 0 15 6Z"],
        // archive-restore
        Icon::Unarchive => &["M3 3H21A1 1 0 0 1 22 4V7A1 1 0 0 1 21 8H3A1 1 0 0 1 2 7V4A1 1 0 0 1 3 3Z", "M4 8v11a2 2 0 0 0 2 2h2", "M20 8v11a2 2 0 0 1-2 2h-2", "m9 15 3-3 3 3", "M12 12v9"],
        // upload
        Icon::Export => &["M21 15v4a2 2 0 0 1-2 2H5a2 2 0 0 1-2-2v-4", "M17 8L12 3L7 8", "M12 3L12 15"],
        // video
        Icon::Video => &["m16 13 5.223 3.482a.5.5 0 0 0 .777-.416V7.87a.5.5 0 0 0-.752-.432L16 10.5", "M4 6H14A2 2 0 0 1 16 8V16A2 2 0 0 1 14 18H4A2 2 0 0 1 2 16V8A2 2 0 0 1 4 6Z"],
        // lock-open
        Icon::LockOpen => &["M5 11H19A2 2 0 0 1 21 13V20A2 2 0 0 1 19 22H5A2 2 0 0 1 3 20V13A2 2 0 0 1 5 11Z", "M7 11V7a5 5 0 0 1 9.9-1"],
        // plus
        Icon::Plus => &["M5 12h14", "M12 5v14"],
        // folder-plus
        Icon::FolderPlus => &["M12 10v6", "M9 13h6", "M20 20a2 2 0 0 0 2-2V8a2 2 0 0 0-2-2h-7.9a2 2 0 0 1-1.69-.9L9.6 3.9A2 2 0 0 0 7.93 3H4a2 2 0 0 0-2 2v13a2 2 0 0 0 2 2Z"],
        // square-check-big
        Icon::Select => &["M21 10.5V19a2 2 0 0 1-2 2H5a2 2 0 0 1-2-2V5a2 2 0 0 1 2-2h12.5", "m9 11 3 3L22 4"],
        // circle-play
        Icon::PlayCircle => &["M2 12A10 10 0 1 0 22 12A10 10 0 1 0 2 12Z", "M10 8L16 12L10 16L10 8Z"],
        Icon::Send | Icon::Play | Icon::Pause | Icon::Stop => return None,
    })
}

/// A flattened subpath in grid coordinates.
struct Sub {
    pts: Vec<Pos2>,
    closed: bool,
}

/// Curves are flattened once per icon: icons are drawn every frame.
fn shapes(icon: Icon) -> &'static [Sub] {
    static CACHE: OnceLock<Mutex<HashMap<Icon, &'static [Sub]>>> = OnceLock::new();
    let mut cache = CACHE.get_or_init(Default::default).lock().unwrap_or_else(|e| e.into_inner());
    cache.entry(icon).or_insert_with(|| {
        let subs: Vec<Sub> = lucide(icon).unwrap_or_default().iter().flat_map(|d| flatten(d)).collect();
        // A few dozen icons for the whole run: leaking keeps the cache free of lifetimes.
        Vec::leak(subs)
    })
}

fn flatten(d: &str) -> Vec<Sub> {
    let Ok(path) = kurbo::BezPath::from_svg(d) else { return Vec::new() };
    let mut subs: Vec<Sub> = Vec::new();
    let p = |pt: kurbo::Point| pos2(pt.x as f32, pt.y as f32);
    kurbo::flatten(path, 0.02, |el| match el {
        PathEl::MoveTo(pt) => subs.push(Sub { pts: vec![p(pt)], closed: false }),
        PathEl::LineTo(pt) => {
            if let Some(s) = subs.last_mut() {
                s.pts.push(p(pt));
            }
        }
        PathEl::ClosePath => {
            if let Some(s) = subs.last_mut() {
                s.closed = true;
                // The closing point repeats the first one; a closed line joins them itself.
                if s.pts.len() > 2 && s.pts.first().zip(s.pts.last()).is_some_and(|(a, b)| a.distance(*b) < 0.01) {
                    s.pts.pop();
                }
            }
        }
        _ => {}
    });
    subs
}

pub fn draw(p: &Painter, rect: Rect, icon: Icon, color: Color32) {
    let side = rect.width().min(rect.height());
    let scale = side / 24.0;
    let origin = rect.center() - Vec2::splat(side / 2.0);
    let at = |x: f32, y: f32| origin + vec2(x, y) * scale;
    let fill = |xy: &[(f32, f32)]| {
        p.add(Shape::convex_polygon(xy.iter().map(|&(x, y)| at(x, y)).collect(), color, Stroke::NONE));
    };
    match icon {
        Icon::Send => {
            // Paper plane made of two convex halves.
            fill(&[(3.0, 3.5), (21.0, 12.0), (14.5, 12.0), (3.0, 10.2)]);
            fill(&[(3.0, 13.8), (14.5, 12.0), (21.0, 12.0), (3.0, 20.5)]);
        }
        Icon::Play => fill(&[(7.0, 4.5), (19.5, 12.0), (7.0, 19.5)]),
        Icon::Pause => {
            p.rect_filled(Rect::from_two_pos(at(6.5, 5.0), at(10.0, 19.0)), scale, color);
            p.rect_filled(Rect::from_two_pos(at(14.0, 5.0), at(17.5, 19.0)), scale, color);
        }
        Icon::Stop => {
            p.rect_filled(Rect::from_two_pos(at(6.0, 6.0), at(18.0, 18.0)), 2.5 * scale, color);
        }
        _ => {
            let w = (1.9 * scale).max(1.1);
            let stroke = Stroke::new(w, color);
            for s in shapes(icon) {
                let pts: Vec<Pos2> = s.pts.iter().map(|q| at(q.x, q.y)).collect();
                if s.closed {
                    p.add(Shape::closed_line(pts, stroke));
                    continue;
                }
                // egui has no round caps: a circle at each open end (and a lone dot for "h.01" segments).
                if let (Some(a), Some(b)) = (pts.first().copied(), pts.last().copied()) {
                    p.circle_filled(a, w / 2.0, color);
                    p.circle_filled(b, w / 2.0, color);
                }
                if pts.len() > 1 {
                    p.add(Shape::line(pts, stroke));
                }
            }
        }
    }
}

/// Read ticks next to the time: ✓ — sent, ✓✓ — read; `right` is the right edge at the text's mid-line.
/// Own small layout instead of the 24×24 grid: ticks must stay crisp next to an 11 pt font.
pub fn ticks(p: &Painter, right: Pos2, read: bool, color: Color32) {
    let stroke = Stroke::new(1.4, color);
    let check = |dx: f32| {
        let o = right + vec2(dx - 4.5, -1.0);
        p.add(Shape::line(vec![o + vec2(-4.0, 0.5), o + vec2(-1.2, 3.3), o + vec2(4.2, -3.2)], stroke));
    };
    check(0.0);
    if read {
        check(-4.0);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lucide_paths_parse() {
        let all = [
            Icon::Check, Icon::Close, Icon::Search, Icon::Reload, Icon::Power, Icon::Attach, Icon::Smile, Icon::Reply, Icon::Pencil,
            Icon::Pin, Icon::Muted, Icon::Channel, Icon::Bot, Icon::File, Icon::Camera, Icon::Bookmark, Icon::Forward, Icon::Trash,
            Icon::Copy, Icon::Download, Icon::Clock, Icon::Location, Icon::ChevronLeft, Icon::ChevronRight, Icon::Bell, Icon::Mic,
            Icon::Gear, Icon::Lock, Icon::ChevronDown, Icon::At, Icon::Heart, Icon::Eye, Icon::List, Icon::Link, Icon::Commands,
            Icon::Info, Icon::Phone, Icon::User, Icon::Chat, Icon::Folder, Icon::Globe, Icon::Laptop, Icon::Chats, Icon::Users,
            Icon::Archive, Icon::PinOff, Icon::BellOff, Icon::CheckCheck, Icon::MarkUnread, Icon::Unarchive, Icon::Export, Icon::Video, Icon::LockOpen, Icon::Plus, Icon::FolderPlus, Icon::Select, Icon::PlayCircle,
        ];
        for icon in all {
            let paths = lucide(icon).unwrap_or_else(|| panic!("{icon:?} has no paths"));
            for d in paths {
                assert!(kurbo::BezPath::from_svg(d).is_ok(), "{icon:?}: {d}");
            }
            let subs = shapes(icon);
            assert!(subs.len() >= paths.len(), "{icon:?}: every path gives at least one subpath");
            // Everything stays on the 24×24 grid.
            assert!(subs.iter().flat_map(|s| &s.pts).all(|p| (-0.5..=24.5).contains(&p.x) && (-0.5..=24.5).contains(&p.y)), "{icon:?}");
        }
    }
}
