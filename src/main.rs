// Glassgram: a Telegram client with a "liquid glass" interface.
// SPDX-License-Identifier: GPL-3.0-or-later
// No console window next to the app on Windows.
#![cfg_attr(target_os = "windows", windows_subsystem = "windows")]

mod accounts;
mod app;
mod backend;
mod blur;
mod cache;
mod camera;
mod demo;
mod emoji;
mod export;
mod format;
mod i18n;
mod glass;
mod icons;
mod lock;
mod logo;
mod lottie;
mod platform;
mod player;
#[cfg(test)]
mod progress;
mod recorder;
mod settings;
mod tray;
mod video;
mod viewer;
mod webview;

fn main() -> eframe::Result {
    // The mini app window runs as a separate process of the same executable.
    if std::env::args().any(|a| a == "--webview") {
        webview::run();
        return Ok(());
    }
    let demo = std::env::args().any(|a| a == "--demo");
    let logo = logo::render(256, false);
    let icon = eframe::egui::IconData { width: logo.width(), height: logo.height(), rgba: logo.into_raw() };
    let options = eframe::NativeOptions {
        viewport: eframe::egui::ViewportBuilder::default()
            .with_title("Glassgram")
            .with_app_id("io.github.petro_pixel.Glassgram")
            .with_inner_size([1120.0, 740.0])
            .with_min_inner_size([760.0, 520.0])
            .with_icon(icon),
        ..Default::default()
    };
    eframe::run_native("Glassgram", options, Box::new(move |cc| Ok(Box::new(app::App::new(cc, demo)))))
}
