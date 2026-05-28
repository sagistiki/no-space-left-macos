//! The egui application: window state, the background scan worker, and the
//! main view. Scanning runs on a worker thread so the UI never blocks.

use disk_core::scanner::{self, ScanOptions, ScanOutcome};
use eframe::egui;
use std::path::PathBuf;
use std::sync::mpsc::{channel, Receiver};
use std::thread;

/// Where the current scan stands.
enum ScanState {
    Idle,
    Scanning,
    Done(ScanOutcome),
    Error(String),
}

pub struct OrganizerApp {
    root: PathBuf,
    state: ScanState,
    rx: Option<Receiver<Result<ScanOutcome, String>>>,
}

impl OrganizerApp {
    pub fn new(cc: &eframe::CreationContext<'_>) -> Self {
        crate::theme::apply(&cc.egui_ctx);
        let root = dirs::home_dir().unwrap_or_else(|| PathBuf::from("/"));
        Self {
            root,
            state: ScanState::Idle,
            rx: None,
        }
    }

    /// Spawn a scan of `self.root` on a worker thread.
    fn start_scan(&mut self, ctx: &egui::Context) {
        let root = self.root.clone();
        let (tx, rx) = channel();
        self.rx = Some(rx);
        self.state = ScanState::Scanning;

        let ctx = ctx.clone();
        thread::spawn(move || {
            let result = scanner::scan(&root, &ScanOptions::default()).map_err(|e| e.to_string());
            let _ = tx.send(result);
            ctx.request_repaint();
        });
    }

    /// Pull a finished scan result, if the worker has delivered one.
    fn poll_scan(&mut self) {
        if let Some(rx) = &self.rx {
            if let Ok(result) = rx.try_recv() {
                self.state = match result {
                    Ok(outcome) => ScanState::Done(outcome),
                    Err(e) => ScanState::Error(e),
                };
                self.rx = None;
            }
        }
    }
}

impl eframe::App for OrganizerApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        self.poll_scan();

        let mut scan_requested = false;
        egui::TopBottomPanel::top("toolbar")
            .exact_height(52.0)
            .show(ctx, |ui| {
                ui.add_space(6.0);
                ui.horizontal(|ui| {
                    ui.add_space(12.0);
                    ui.heading("Disk Space Organizer");
                    ui.separator();
                    ui.label(self.root.display().to_string());
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        ui.add_space(12.0);
                        let scanning = matches!(self.state, ScanState::Scanning);
                        if ui
                            .add_enabled(!scanning, egui::Button::new("Scan"))
                            .clicked()
                        {
                            scan_requested = true;
                        }
                    });
                });
            });

        if scan_requested {
            self.start_scan(ctx);
        }

        egui::CentralPanel::default().show(ctx, |ui| match &self.state {
            ScanState::Idle => {
                ui.centered_and_justified(|ui| {
                    ui.label("Press Scan to analyze your home folder.");
                });
            }
            ScanState::Scanning => {
                ui.centered_and_justified(|ui| {
                    ui.spinner();
                });
            }
            ScanState::Done(outcome) => {
                ui.add_space(8.0);
                ui.heading(format!("{} — {} bytes", outcome.root.name, outcome.root.size));
                ui.separator();
                egui::ScrollArea::vertical().show(ui, |ui| {
                    for child in outcome.root.children.iter().take(40) {
                        ui.label(format!("{} — {} bytes", child.name, child.size));
                    }
                });
            }
            ScanState::Error(e) => {
                ui.centered_and_justified(|ui| {
                    ui.colored_label(egui::Color32::from_rgb(255, 69, 58), e.clone());
                });
            }
        });
    }
}
