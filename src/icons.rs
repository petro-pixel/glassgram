//! Vector icons on a 24×24 grid, anti-aliased by egui's tessellator (unlike emoji from the raster atlas).
//! Outline icons are Lucide SVG files in `assets/icons/` (https://lucide.dev, ISC licence, see
//! `LUCIDE-LICENSE.txt` there): the same grid, stroke and round caps as our design code.
//! A new icon: drop its SVG into `assets/icons/` and add a variant with an `svg()` arm.
//! Filled media controls are our own.

use std::collections::HashMap;
use std::sync::{Mutex, OnceLock};

use eframe::egui::{Color32, Painter, Pos2, Rect, Shape, Stroke, Vec2, pos2, vec2};
use kurbo::{PathEl, Shape as _};

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
    Flag,
}

/// The Lucide SVG file of an icon; `None` for our own filled icons.
fn svg(icon: Icon) -> Option<&'static str> {
    Some(match icon {
        Icon::Check => include_str!("../assets/icons/check.svg"),
        Icon::Close => include_str!("../assets/icons/x.svg"),
        Icon::Search => include_str!("../assets/icons/search.svg"),
        Icon::Reload => include_str!("../assets/icons/refresh-cw.svg"),
        Icon::Power => include_str!("../assets/icons/power.svg"),
        Icon::Attach => include_str!("../assets/icons/paperclip.svg"),
        Icon::Smile => include_str!("../assets/icons/smile.svg"),
        Icon::Reply => include_str!("../assets/icons/reply.svg"),
        Icon::Forward => include_str!("../assets/icons/forward.svg"),
        Icon::Pencil => include_str!("../assets/icons/pencil.svg"),
        Icon::Pin => include_str!("../assets/icons/pin.svg"),
        Icon::Muted => include_str!("../assets/icons/volume-x.svg"),
        Icon::Channel => include_str!("../assets/icons/megaphone.svg"),
        Icon::Bot => include_str!("../assets/icons/bot.svg"),
        Icon::File => include_str!("../assets/icons/file.svg"),
        Icon::Camera => include_str!("../assets/icons/camera.svg"),
        Icon::Bookmark => include_str!("../assets/icons/bookmark.svg"),
        Icon::Trash => include_str!("../assets/icons/trash-2.svg"),
        Icon::Copy => include_str!("../assets/icons/copy.svg"),
        Icon::Download => include_str!("../assets/icons/download.svg"),
        Icon::Clock => include_str!("../assets/icons/clock.svg"),
        Icon::Location => include_str!("../assets/icons/map-pin.svg"),
        Icon::ChevronLeft => include_str!("../assets/icons/chevron-left.svg"),
        Icon::ChevronRight => include_str!("../assets/icons/chevron-right.svg"),
        Icon::ChevronDown => include_str!("../assets/icons/chevron-down.svg"),
        Icon::Bell => include_str!("../assets/icons/bell.svg"),
        Icon::Mic => include_str!("../assets/icons/mic.svg"),
        Icon::Gear => include_str!("../assets/icons/settings.svg"),
        Icon::Lock => include_str!("../assets/icons/lock.svg"),
        Icon::At => include_str!("../assets/icons/at-sign.svg"),
        Icon::Heart => include_str!("../assets/icons/heart.svg"),
        Icon::Eye => include_str!("../assets/icons/eye.svg"),
        Icon::List => include_str!("../assets/icons/list.svg"),
        Icon::Link => include_str!("../assets/icons/link.svg"),
        Icon::Commands => include_str!("../assets/icons/square-slash.svg"),
        Icon::Info => include_str!("../assets/icons/info.svg"),
        Icon::Phone => include_str!("../assets/icons/phone.svg"),
        Icon::User => include_str!("../assets/icons/user.svg"),
        Icon::Chat => include_str!("../assets/icons/message-circle.svg"),
        Icon::Folder => include_str!("../assets/icons/folder.svg"),
        Icon::Globe => include_str!("../assets/icons/globe.svg"),
        Icon::Laptop => include_str!("../assets/icons/laptop.svg"),
        Icon::Chats => include_str!("../assets/icons/messages-square.svg"),
        Icon::Users => include_str!("../assets/icons/users.svg"),
        Icon::Archive => include_str!("../assets/icons/archive.svg"),
        Icon::PinOff => include_str!("../assets/icons/pin-off.svg"),
        Icon::BellOff => include_str!("../assets/icons/bell-off.svg"),
        Icon::CheckCheck => include_str!("../assets/icons/check-check.svg"),
        Icon::MarkUnread => include_str!("../assets/icons/message-square-dot.svg"),
        Icon::Unarchive => include_str!("../assets/icons/archive-restore.svg"),
        Icon::Export => include_str!("../assets/icons/upload.svg"),
        Icon::Video => include_str!("../assets/icons/video.svg"),
        Icon::LockOpen => include_str!("../assets/icons/lock-open.svg"),
        Icon::Plus => include_str!("../assets/icons/plus.svg"),
        Icon::FolderPlus => include_str!("../assets/icons/folder-plus.svg"),
        Icon::Select => include_str!("../assets/icons/square-check-big.svg"),
        Icon::PlayCircle => include_str!("../assets/icons/circle-play.svg"),
        Icon::Flag => include_str!("../assets/icons/flag.svg"),
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
        let subs = svg(icon).map(parse_svg).unwrap_or_default();
        // A few dozen icons for the whole run: leaking keeps the cache free of lifetimes.
        Vec::leak(subs)
    })
}

/// The shapes of a Lucide SVG: `path`, `circle`, `ellipse`, `rect`, `line`, `polyline`, `polygon`.
fn parse_svg(text: &str) -> Vec<Sub> {
    let Ok(doc) = roxmltree::Document::parse(text) else { return Vec::new() };
    let mut subs = Vec::new();
    for n in doc.root_element().children().filter(|n| n.is_element()) {
        let num = |k: &str| n.attribute(k).and_then(|v| v.parse::<f64>().ok()).unwrap_or(0.0);
        let path = match n.tag_name().name() {
            "path" => n.attribute("d").and_then(|d| kurbo::BezPath::from_svg(d).ok()),
            "circle" => Some(kurbo::Circle::new((num("cx"), num("cy")), num("r")).to_path(0.01)),
            "ellipse" => Some(kurbo::Ellipse::new((num("cx"), num("cy")), (num("rx"), num("ry")), 0.0).to_path(0.01)),
            "rect" => {
                let r = kurbo::Rect::new(num("x"), num("y"), num("x") + num("width"), num("y") + num("height"));
                Some(r.to_rounded_rect(num("rx").max(num("ry"))).to_path(0.01))
            }
            "line" => Some(kurbo::Line::new((num("x1"), num("y1")), (num("x2"), num("y2"))).to_path(0.01)),
            tag @ ("polyline" | "polygon") => {
                let v: Vec<f64> = n.attribute("points").unwrap_or_default().split([' ', ',']).filter_map(|x| x.parse().ok()).collect();
                let mut p = kurbo::BezPath::new();
                for (i, xy) in v.chunks_exact(2).enumerate() {
                    if i == 0 { p.move_to((xy[0], xy[1])) } else { p.line_to((xy[0], xy[1])) }
                }
                if tag == "polygon" {
                    p.close_path();
                }
                Some(p)
            }
            _ => None,
        };
        subs.extend(path.map(|p| flatten(&p)).unwrap_or_default());
    }
    subs
}

fn flatten(path: &kurbo::BezPath) -> Vec<Sub> {
    let mut subs: Vec<Sub> = Vec::new();
    let p = |pt: kurbo::Point| pos2(pt.x as f32, pt.y as f32);
    kurbo::flatten(path.iter(), 0.02, |el| match el {
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
    fn lucide_svgs_parse() {
        let all = [
            Icon::Check, Icon::Close, Icon::Search, Icon::Reload, Icon::Power, Icon::Attach, Icon::Smile, Icon::Reply, Icon::Pencil,
            Icon::Pin, Icon::Muted, Icon::Channel, Icon::Bot, Icon::File, Icon::Camera, Icon::Bookmark, Icon::Forward, Icon::Trash,
            Icon::Copy, Icon::Download, Icon::Clock, Icon::Location, Icon::ChevronLeft, Icon::ChevronRight, Icon::Bell, Icon::Mic,
            Icon::Gear, Icon::Lock, Icon::ChevronDown, Icon::At, Icon::Heart, Icon::Eye, Icon::List, Icon::Link, Icon::Commands,
            Icon::Info, Icon::Phone, Icon::User, Icon::Chat, Icon::Folder, Icon::Globe, Icon::Laptop, Icon::Chats, Icon::Users,
            Icon::Archive, Icon::PinOff, Icon::BellOff, Icon::CheckCheck, Icon::MarkUnread, Icon::Unarchive, Icon::Export, Icon::Video, Icon::LockOpen, Icon::Plus, Icon::FolderPlus, Icon::Select, Icon::PlayCircle, Icon::Flag,
        ];
        for icon in all {
            let text = svg(icon).unwrap_or_else(|| panic!("{icon:?} has no SVG"));
            let elements = roxmltree::Document::parse(text).unwrap().root_element().children().filter(|n| n.is_element()).count();
            let subs = shapes(icon);
            assert!(subs.len() >= elements, "{icon:?}: every SVG element gives at least one subpath");
            // Everything stays on the 24×24 grid.
            assert!(subs.iter().flat_map(|s| &s.pts).all(|p| (-0.5..=24.5).contains(&p.x) && (-0.5..=24.5).contains(&p.y)), "{icon:?}");
        }
    }
}
