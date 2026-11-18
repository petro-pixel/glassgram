//! Demo settings: sessions, privacy, notifications, proxy, ringtones, profile, cache, auto-delete, websites and sensitive content.

use super::*;

pub(super) fn handle(app: &mut App, cmd: Cmd) {
    match cmd {
        Cmd::LoadSessions => {
            let s = |hash, app: &str, device: &str, place: &str, active: &str, current| crate::backend::Session {
                hash,
                app: app.into(),
                device: device.into(),
                place: place.into(),
                active: active.into(),
                current,
            };
            app.push_event(Ev::Sessions(vec![
                s(1, "Glassgram 0.1", "Linux · Ubuntu", "Ukraine · 93.170.0.1", "now", true),
                s(2, "Telegram Android 11.2", "Pixel 7 · Android 15", "Ukraine · 46.211.0.7", "Today 09:41", false),
                s(3, "Telegram Desktop 5.6", "Windows 11", "Poland · 31.0.12.4", "12.10 18:02", false),
            ]));
        }
        Cmd::TerminateSession(_) | Cmd::TerminateOtherSessions => app.push_event(Ev::Notice(l!("У демо-режимі сеанси не завершуються", "Sessions aren't terminated in demo mode").into())),
        Cmd::LoadPrivacy => {
            use crate::backend::{Audience, PrivacyKey, PrivacyState};
            for key in PrivacyKey::ALL {
                let audience = match key {
                    PrivacyKey::Phone => Audience::Nobody,
                    PrivacyKey::LastSeen | PrivacyKey::Invites => Audience::Contacts,
                    _ => Audience::Everybody,
                };
                app.push_event(Ev::Privacy(key, PrivacyState { audience, exceptions: key == PrivacyKey::LastSeen }));
                // "Last seen": hidden from one colleague, shown to a whole group.
                let ex = if key == PrivacyKey::LastSeen {
                    let pick = |id: i64| app.demo_chats().iter().find(|c| c.id == PeerId::user_unchecked(id)).cloned();
                    crate::backend::PrivacyExceptions { allow: pick(7).into_iter().collect(), deny: pick(5).into_iter().collect() }
                } else {
                    Default::default()
                };
                app.push_event(Ev::PrivacyExceptions(key, ex));
            }
        }
        Cmd::SetPrivacy { key, audience, allow, deny } => {
            let exceptions = !allow.is_empty() || !deny.is_empty();
            app.push_event(Ev::Privacy(key, crate::backend::PrivacyState { audience, exceptions }));
            // Names come from the chat list and contacts the app already has.
            let find = |ids: &[PeerId]| ids.iter().filter_map(|p| app.demo_chats().iter().chain(demo_contacts().iter()).find(|c| c.id == *p).cloned()).collect::<Vec<_>>();
            let ex = crate::backend::PrivacyExceptions { allow: find(&allow), deny: find(&deny) };
            app.push_event(Ev::PrivacyExceptions(key, ex));
        }
        Cmd::LoadNotifyExceptions(scope) => {
            let default = app.demo_notify_default(scope);
            app.push_event(Ev::NotifyDefault { scope, state: default });
            // Chats that differ from their category are its exceptions.
            let list = app
                .demo_chats()
                .iter()
                .filter(|c| NotifyScope::of(c.kind) == scope && c.muted != default.muted())
                .map(|c| NotifyException { peer: c.id, title: c.title.clone(), state: NotifyState { mute_until: if c.muted { MUTE_FOREVER } else { 0 }, silent: false, inherit: false, sound: None } })
                .collect();
            app.push_event(Ev::NotifyExceptions { scope, list });
        }
        Cmd::SetNotifyDefault { scope, muted } => {
            app.push_event(Ev::NotifyDefault { scope, state: NotifyState { mute_until: if muted { MUTE_FOREVER } else { 0 }, ..Default::default() } });
            handle(app, Cmd::LoadNotifyExceptions(scope));
        }
        Cmd::ResetNotify(peer) => {
            let Some(kind) = app.demo_chat(peer).map(|c| c.kind) else { return };
            let state = NotifyState { inherit: true, ..app.demo_notify_default(NotifyScope::of(kind)) };
            app.push_event(Ev::Notify { peer, state });
        }
        Cmd::SetProxy(_) => {}
        // Made-up pings; a proxy on port 1 never answers.
        Cmd::CheckProxy(link) => {
            let result = if link.contains("port=1&") || link.ends_with(":1") { Err("timed out".to_owned()) } else { Ok(40 + (link.len() as u32 * 7) % 160) };
            app.push_event(Ev::ProxyChecked { link, result });
        }
        Cmd::LoadRingtones => {
            // No files: listening falls back to the built-in chime.
            let list = app.demo_ringtones().unwrap_or_else(|| vec![Ringtone { id: 7001, title: "Marimba".into(), duration: 2.0 }, Ringtone { id: 7002, title: "Glass bell".into(), duration: 3.0 }]);
            app.push_event(Ev::Ringtones(list));
        }
        Cmd::UploadRingtone(path) => {
            let mut list = app.demo_ringtones().unwrap_or_default();
            let title = path.file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_default();
            list.push(Ringtone { id: 7000 + list.len() as i64 + 10, title, duration: 4.0 });
            app.push_event(Ev::Ringtones(list));
        }
        Cmd::DeleteRingtone(id) => {
            let list = app.demo_ringtones().unwrap_or_default().into_iter().filter(|r| r.id != id).collect();
            app.push_event(Ev::Ringtones(list));
        }
        Cmd::LoadReactionNotify => app.push_event(Ev::ReactionNotify(app.demo_reaction_notify().unwrap_or(ReactionNotify { messages: ReactFrom::All, stories: ReactFrom::Contacts, polls: ReactFrom::All, previews: true, sound: Sound::Default }))),
        Cmd::SetReactionNotify(s) => app.push_event(Ev::ReactionNotify(s)),
        Cmd::LoadProfile => {
            let p = app.demo_profile();
            app.push_event(Ev::Profile(if p.id.is_some() { p } else { demo_profile() }));
        }
        Cmd::SaveProfile { first, last, bio } => {
            app.push_event(Ev::Profile(Profile { first, last, bio, ..app.demo_profile() }));
            app.push_event(Ev::Notice(l!("Профіль збережено (демо)", "Profile saved (demo)").into()));
        }
        // A few names are "taken" so every answer can be seen.
        Cmd::CheckUsername(name) => {
            let taken = ["durov", "telegram", "glassgram", "admin"].iter().any(|t| name.eq_ignore_ascii_case(t));
            app.push_event(Ev::UsernameCheck { name, state: if taken { UsernameState::Taken } else { UsernameState::Free } });
        }
        Cmd::SetUsername(username) => {
            app.push_event(Ev::Profile(Profile { username, ..app.demo_profile() }));
            app.push_event(Ev::Notice(l!("Ім'я користувача збережено (демо)", "Username saved (demo)").into()));
        }
        Cmd::SetProfilePhoto(path) => match image::open(&path) {
            Ok(img) => {
                let side = img.width().min(img.height());
                let img = img.crop_imm((img.width() - side) / 2, (img.height() - side) / 2, side, side).thumbnail(256, 256).to_rgba8();
                let image = ColorImage::from_rgba_unmultiplied([img.width() as usize, img.height() as usize], img.as_raw());
                app.push_event(Ev::Image { key: ImgKey::Avatar(PeerId::user_unchecked(ME)), image });
                app.push_event(Ev::Notice(l!("Фото профілю оновлено (демо)", "Profile photo updated (demo)").into()));
            }
            Err(e) => app.push_event(Ev::Error(e.to_string())),
        },
        // `#offline` in the text pretends there is no network: the clock, retries, then the red mark.
        cmd => account(app, cmd),
    }
}

/// Account-wide switches: global privacy, cache, number and deletion, auto-delete, websites, birthday and usernames.
fn account(app: &mut App, cmd: Cmd) {
    match cmd {
        Cmd::LoadGlobalPrivacy => app.push_event(Ev::GlobalPrivacy(GlobalPrivacy { archive_new: true, keep_unmuted: false, keep_folders: true, hide_read: false, premium_only: false, paid_stars: None })),
        Cmd::SetGlobalPrivacy(p) => app.push_event(Ev::GlobalPrivacy(p)),
        Cmd::LoadCacheUsage | Cmd::ClearCache(_) | Cmd::TrimCache(_) => {
            use crate::backend::CacheKind as K;
            if let Cmd::ClearCache(kinds) = &cmd {
                DEMO_CLEARED.with_borrow_mut(|c| c.extend(kinds.iter().copied()));
            }
            let made_up = [(K::Photos, 184 << 20, 640), (K::Videos, 1210 << 20, 38), (K::Audio, 23 << 20, 51), (K::Stickers, 46 << 20, 412), (K::Profiles, 12 << 20, 230), (K::Other, 3 << 20, 4)];
            let usage = made_up.into_iter().map(|(k, b, n)| if DEMO_CLEARED.with_borrow(|c| c.contains(&k)) { (k, 0, 0) } else { (k, b, n) }).collect();
            app.push_event(Ev::CacheUsage(usage));
        }
        cmd @ (Cmd::LoadCountries(_) | Cmd::SendChangePhoneCode(_) | Cmd::ChangePhone { .. } | Cmd::DeleteAccount { .. }) => {
            let check = |pw: &str| PASSWORD.with_borrow(|(p, s)| s.has_password.then(|| p == pw));
            account_actions::answer(app, cmd, check);
        }
        Cmd::LoadAccountTtl => app.push_event(Ev::AccountTtl(DEMO_ACCOUNT_TTL.get())),
        Cmd::SetAccountTtl(days) => {
            DEMO_ACCOUNT_TTL.set(days);
            app.push_event(Ev::AccountTtl(days));
        }
        Cmd::LoadDefaultTtl => app.push_event(Ev::DefaultTtl(DEMO_DEFAULT_TTL.get())),
        Cmd::SetDefaultTtl(period) => {
            DEMO_DEFAULT_TTL.set(period);
            app.push_event(Ev::DefaultTtl(period));
        }
        Cmd::SetHistoryTtl { peer, period } => {
            DEMO_TTL.with_borrow_mut(|t| t.insert(peer, period));
            app.push_event(Ev::HistoryTtl { peer, period });
        }
        Cmd::LoadWebsites | Cmd::LogoutWebsite(_) | Cmd::LogoutAllWebsites => {
            let gone = match &cmd {
                Cmd::LogoutWebsite(h) => vec![*h],
                Cmd::LogoutAllWebsites => vec![1, 2],
                _ => Vec::new(),
            };
            DEMO_WEB_GONE.with_borrow_mut(|g| g.extend(gone));
            let site = |hash, domain: &str, bot: &str, device: &str, place: &str, active: &str| crate::backend::WebSession { hash, domain: domain.into(), bot: bot.into(), device: device.into(), place: place.into(), active: active.into() };
            let list = [site(1, "example.org", "Example Login", "Firefox 140 · Linux", "Kyiv, Ukraine · 192.0.2.10", "yesterday 21:14"), site(2, "forum.example.net", "Forum Bot", "Chrome 141 · Windows", "Lviv, Ukraine · 198.51.100.7", "12.10.2026 09:30")];
            let list = list.into_iter().filter(|s| !DEMO_WEB_GONE.with_borrow(|g| g.contains(&s.hash))).collect();
            app.push_event(Ev::Websites(list));
        }
        Cmd::LoadContentSettings => app.push_event(Ev::ContentSettings { enabled: false, can_change: true }),
        Cmd::SetSensitive(enabled) => app.push_event(Ev::ContentSettings { enabled, can_change: true }),
        Cmd::SetBirthday(birthday) => app.push_event(Ev::Profile(Profile { birthday, ..app.demo_profile() })),
        Cmd::ToggleUsername { name, active } => {
            let mut p = app.demo_profile();
            if let Some(u) = p.usernames.iter_mut().find(|u| u.0 == name) {
                u.1 = active;
            }
            app.push_event(Ev::Profile(p));
        }
        Cmd::ReorderUsernames(order) => {
            let mut p = app.demo_profile();
            p.usernames.sort_by_key(|u| order.iter().position(|o| *o == u.0));
            app.push_event(Ev::Profile(p));
        }
        _ => {}
    }
}
