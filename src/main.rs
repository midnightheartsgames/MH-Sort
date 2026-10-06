// В релизе без консольного окна
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod app;
mod classify;
mod config;
mod history;
mod icons;
mod logo;
mod scanner;
mod sorter;
mod theme;
mod util;
mod widgets;

use eframe::egui;

fn main() -> eframe::Result {
    // Папку можно передать аргументом, например перетащив её на exe
    let initial = std::env::args_os().nth(1).map(std::path::PathBuf::from);
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_title("MH Sort")
            .with_inner_size([1180.0, 760.0])
            .with_min_inner_size([900.0, 560.0])
            .with_drag_and_drop(true)
            .with_icon(egui::IconData {
                rgba: logo::rgba(64),
                width: 64,
                height: 64,
            }),
        ..Default::default()
    };
    eframe::run_native(
        "MH Sort",
        options,
        Box::new(move |cc| Ok(Box::new(app::App::new(cc, initial)))),
    )
}
