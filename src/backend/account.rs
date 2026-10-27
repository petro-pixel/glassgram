//! "My account": own profile, username and photo.

use std::path::{Path, PathBuf};

use grammers_client::InvocationError;
use grammers_client::peer::{Peer, User};
use grammers_client::tl;

use super::{Birthday, Cmd, Ev, Profile, UsernameState, Worker, log_error};
use crate::i18n::{l, lf};

/// Side of the square profile photo, as after cropping in Telegram Desktop.
const PHOTO_PX: u32 = 1280;

fn rpc_name(e: &InvocationError) -> Option<&str> {
    match e {
        InvocationError::Rpc(r) => Some(r.name.as_str()),
        _ => None,
    }
}

/// Readable explanation of common server refusals; the rest as is.
fn profile_error(e: &InvocationError) -> String {
    match rpc_name(e) {
        Some("ABOUT_TOO_LONG") => l!("Біо задовге", "Bio is too long").into(),
        Some("FIRSTNAME_INVALID") => l!("Некоректне ім'я", "Invalid first name").into(),
        Some("LASTNAME_INVALID") => l!("Некоректне прізвище", "Invalid last name").into(),
        Some("USERNAME_OCCUPIED") => l!("Це ім'я користувача вже зайняте", "This username is already taken").into(),
        Some("USERNAME_INVALID") => l!("Некоректне ім'я користувача", "Invalid username").into(),
        Some("USERNAME_PURCHASE_AVAILABLE") => l!("Це ім'я користувача продається на Fragment", "This username is for sale on Fragment").into(),
        Some("FLOOD_WAIT") => l!("Забагато спроб, зачекайте", "Too many attempts, please wait").into(),
        _ => e.to_string(),
    }
}

impl Worker {
    pub(super) async fn load_profile(&mut self) -> Result<(), String> {
        let client = self.client()?;
        let me = client.get_me().await.map_err(|e| lf!("Профіль: {e}", "Profile: {e}"))?;
        let full = client.invoke(&tl::functions::users::GetFullUser { id: tl::enums::InputUser::UserSelf }).await;
        let (bio, birthday) = match full {
            Ok(tl::enums::users::UserFull::Full(f)) => {
                let tl::enums::UserFull::Full(u) = f.full_user;
                (u.about.unwrap_or_default(), u.birthday.as_ref().map(Birthday::from_raw))
            }
            Err(e) => {
                log_error(&lf!("Біо: {e}", "Bio: {e}"));
                (String::new(), None)
            }
        };
        self.emit_profile(&me, bio, birthday);
        Ok(())
    }

    fn emit_profile(&self, me: &User, bio: String, birthday: Option<Birthday>) {
        if let Some(loader) = &self.loader {
            loader.avatar(&Peer::User(me.clone()));
        }
        let profile = Profile {
            id: Some(me.id()),
            first: me.first_name().unwrap_or_default().to_owned(),
            last: me.last_name().unwrap_or_default().to_owned(),
            bio,
            username: me.username().unwrap_or_default().to_owned(),
            phone: me.phone().map(|p| format!("+{p}")).unwrap_or_default(),
            birthday,
            usernames: match &me.raw {
                tl::enums::User::User(u) => u.usernames.iter().flatten().filter_map(|tl::enums::Username::Username(n)| (!n.editable).then(|| (n.username.clone(), n.active))).collect(),
                _ => Vec::new(),
            },
        };
        self.out.send(Ev::Profile(profile));
    }

    pub(super) async fn save_profile(&mut self, first: String, last: String, bio: String) -> Result<(), String> {
        let client = self.client()?;
        let req = tl::functions::account::UpdateProfile { first_name: Some(first), last_name: Some(last), about: Some(bio.clone()) };
        client.invoke(&req).await.map_err(|e| profile_error(&e))?;
        // Read back the whole profile: the reply has no birthday.
        self.load_profile().await?;
        self.out.send(Ev::Notice(l!("Профіль збережено", "Profile saved").into()));
        Ok(())
    }

    /// Runs in the background: other commands don't wait while the user types.
    pub(super) fn check_username(&mut self, name: String) -> Result<(), String> {
        let client = self.client()?;
        let out = self.out.clone();
        tokio::spawn(async move {
            let state = match client.invoke(&tl::functions::account::CheckUsername { username: name.clone() }).await {
                Ok(true) => UsernameState::Free,
                Ok(false) => UsernameState::Taken,
                Err(e) => match rpc_name(&e) {
                    Some("USERNAME_INVALID") => UsernameState::Invalid,
                    Some("USERNAME_OCCUPIED" | "USERNAME_PURCHASE_AVAILABLE") => UsernameState::Taken,
                    _ => return out.send(Ev::Error(lf!("Ім'я користувача: {e}", "Username: {e}"))),
                },
            };
            out.send(Ev::UsernameCheck { name, state });
        });
        Ok(())
    }

    pub(super) async fn toggle_username(&mut self, name: String, active: bool) -> Result<(), String> {
        let client = self.client()?;
        client.invoke(&tl::functions::account::ToggleUsername { username: name, active }).await.map_err(|e| profile_error(&e))?;
        self.load_profile().await
    }

    pub(super) async fn reorder_usernames(&mut self, order: Vec<String>) -> Result<(), String> {
        let client = self.client()?;
        client.invoke(&tl::functions::account::ReorderUsernames { order }).await.map_err(|e| profile_error(&e))?;
        self.load_profile().await
    }

    pub(super) async fn set_username(&mut self, name: String) -> Result<(), String> {
        let client = self.client()?;
        match client.invoke(&tl::functions::account::UpdateUsername { username: name }).await {
            Ok(_) => self.out.send(Ev::Notice(l!("Ім'я користувача збережено", "Username saved").into())),
            // The same name is not an error.
            Err(e) if rpc_name(&e) == Some("USERNAME_NOT_MODIFIED") => {}
            Err(e) => return Err(profile_error(&e)),
        }
        self.load_profile().await
    }

    /// Crop, compress and upload in the background; then the profile is reloaded with the new avatar.
    pub(super) fn set_profile_photo(&mut self, path: PathBuf) -> Result<(), String> {
        let client = self.client()?;
        let (out, back) = (self.out.clone(), self.back.clone());
        tokio::spawn(async move {
            let square = tokio::task::spawn_blocking(move || square_photo(&path)).await;
            let file = match square {
                Ok(Ok(f)) => f,
                Ok(Err(e)) => return out.send(Ev::Error(e)),
                Err(e) => return out.send(Ev::Error(e.to_string())),
            };
            let uploaded = client.upload_file(&file).await;
            let _ = tokio::fs::remove_file(&file).await;
            let up = match uploaded {
                Ok(up) => up,
                Err(e) => return out.send(Ev::Error(lf!("Фото: {e}", "Photo: {e}"))),
            };
            let req = tl::functions::photos::UploadProfilePhoto {
                fallback: false,
                bot: None,
                file: Some(up.raw),
                video: None,
                video_start_ts: None,
                video_emoji_markup: None,
            };
            match client.invoke(&req).await {
                Ok(_) => {
                    out.send(Ev::Notice(l!("Фото профілю оновлено", "Profile photo updated").into()));
                    let _ = back.send(Cmd::LoadProfile);
                }
                Err(e) => out.send(Ev::Error(lf!("Фото: {}", "Photo: {}", profile_error(&e)))),
            }
        });
        Ok(())
    }
}

/// Center square of the image, at most `PHOTO_PX`, as JPEG in the temp dir.
pub(super) fn square_photo(path: &Path) -> Result<PathBuf, String> {
    let img = image::open(path).map_err(|e| lf!("Не вдалося відкрити зображення: {e}", "Could not open the image: {e}"))?;
    let side = img.width().min(img.height());
    let img = img.crop_imm((img.width() - side) / 2, (img.height() - side) / 2, side, side);
    let img = if side > PHOTO_PX { img.resize_exact(PHOTO_PX, PHOTO_PX, image::imageops::FilterType::Lanczos3) } else { img };
    let out = crate::platform::temp_file("avatar.jpg");
    let file = std::fs::File::create(&out).map_err(|e| e.to_string())?;
    image::codecs::jpeg::JpegEncoder::new_with_quality(std::io::BufWriter::new(file), 90)
        .encode_image(&img.to_rgb8())
        .map_err(|e| e.to_string())?;
    Ok(out)
}
