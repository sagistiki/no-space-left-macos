//! no space left — macOS desktop app (egui / eframe).

mod actions;
mod app;
mod format;
mod theme;
mod treemap;

fn main() -> eframe::Result {
    let native_options = eframe::NativeOptions {
        viewport: eframe::egui::ViewportBuilder::default()
            .with_inner_size([1100.0, 720.0])
            .with_min_inner_size([680.0, 440.0])
            .with_title("no space left"),
        ..Default::default()
    };

    eframe::run_native(
        "no space left",
        native_options,
        Box::new(|cc| Ok(Box::new(app::OrganizerApp::new(cc)))),
    )
}
