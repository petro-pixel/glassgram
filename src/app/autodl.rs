//! Settings → Advanced → "Automatic media download", as in Telegram Desktop: photos, videos and
//! audio by chat type, videos up to a size. Held-back media wait in the bubble for a click.

use super::*;
use crate::settings::AutoKinds;

/// Video size limits offered, in MB (0 — any size).
const VIDEO_LIMITS: [u32; 4] = [10, 50, 200, 0];

impl App {
    /// Tells the backend which photos to hold back; the rest is decided while drawing.
    pub(super) fn send_autodl(&mut self) {
        self.send(Cmd::SetAutoDownload(self.settings.autodl));
    }

    /// The message's media is not downloaded and waits for a click.
    pub(super) fn media_manual(&self, peer: PeerId, chat: &ChatInfo, m: &Msg) -> bool {
        let a = &self.settings.autodl;
        match &m.media {
            Some(info) => !self.playback.files.contains_key(&(peer, m.id)) && !a.media(chat.kind, info),
            None => m.photo && !a.kinds(chat.kind).photos && !self.textures.contains_key(&ImgKey::Photo(peer, m.id)),
        }
    }

    /// Videos and audio that are allowed to download on their own get queued while visible.
    pub(super) fn want_prefetch(&self, peer: PeerId, chat: &ChatInfo, m: &Msg) {
        let Some(info) = &m.media else { return };
        if matches!(info.kind, MediaKind::Video | MediaKind::Voice | MediaKind::Audio) && self.settings.autodl.media(chat.kind, info) {
            self.playback.prefetch.borrow_mut().push((peer, m.id));
        }
    }

    /// Downloads queued videos and audio into the cache, so a click plays at once.
    pub(super) fn prefetch_media(&mut self) {
        for key in std::mem::take(self.playback.prefetch.get_mut()) {
            self.fetch_media(key.0, key.1);
        }
    }

    /// Downloads one media file into the cache without playing it.
    pub(super) fn fetch_media(&mut self, peer: PeerId, id: i32) {
        if !self.playback.files.contains_key(&(peer, id)) && self.playback.loading.insert((peer, id)) {
            self.send(Cmd::LoadMedia { peer, id });
        }
    }

    pub(super) fn autodl_section(&mut self, ui: &mut Ui) {
        section_title(ui, l!("Автозавантаження медіа", "Automatic media download"));
        let before = self.settings.autodl;
        let a = &mut self.settings.autodl;
        let rows: [(&str, &mut AutoKinds); 3] = [
            (l!("В особистих чатах", "In private chats"), &mut a.private),
            (l!("У групах", "In groups"), &mut a.groups),
            (l!("У каналах", "In channels"), &mut a.channels),
        ];
        for (k, (title, kinds)) in rows.into_iter().enumerate() {
            ui.label(egui::RichText::new(title).color(ink()).size(14.0));
            ui.horizontal_wrapped(|ui| {
                ui.spacing_mut().item_spacing = vec2(6.0, 6.0);
                let w = ((ui.available_width() - 12.0) / 3.0).clamp(90.0, 140.0);
                let toggles = [
                    (l!("Фото", "Photos"), &mut kinds.photos),
                    (l!("Відео", "Videos"), &mut kinds.videos),
                    (l!("Аудіо", "Audio"), &mut kinds.audio),
                ];
                for (j, (label, on)) in toggles.into_iter().enumerate() {
                    let (r, _) = ui.allocate_exact_size(vec2(w, 32.0), Sense::hover());
                    if glass::button(ui, r, ("autodl", k, j), label, *on, true).clicked() {
                        *on = !*on;
                    }
                }
            });
            ui.add_space(4.0);
        }
        ui.label(egui::RichText::new(l!("Відео й GIF до", "Videos and GIFs up to")).color(ink()).size(14.0));
        ui.horizontal_wrapped(|ui| {
            ui.spacing_mut().item_spacing = vec2(6.0, 6.0);
            for mb in VIDEO_LIMITS {
                let label = if mb == 0 { l!("Будь-якого розміру", "Any size").to_owned() } else { format!("{mb} MB") };
                let (r, _) = ui.allocate_exact_size(vec2(if mb == 0 { 150.0 } else { 76.0 }, 30.0), Sense::hover());
                if glass::button(ui, r, ("autodl-mb", mb), &label, a.video_mb == mb, true).clicked() {
                    a.video_mb = mb;
                }
            }
        });
        ui.label(egui::RichText::new(l!("Аудіо — це голосові й музика. Решта медіа завантажується після натискання.", "Audio means voice messages and music. Other media download when clicked.")).color(ink_dim()).size(12.5));
        if self.settings.autodl != before {
            self.send_autodl();
        }
    }
}
