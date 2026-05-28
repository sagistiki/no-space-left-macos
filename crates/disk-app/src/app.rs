//! The egui application: window state, the background scan worker, and the
//! main treemap view with drill-down navigation. Scanning runs on a worker
//! thread so the UI never blocks.

use disk_core::model::Node;
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
    /// Drill path from the scan root: successive child indices.
    nav: Vec<usize>,
    /// Selected child index within the current node.
    selected: Option<usize>,
}

impl OrganizerApp {
    pub fn new(cc: &eframe::CreationContext<'_>) -> Self {
        crate::theme::apply(&cc.egui_ctx);
        let root = dirs::home_dir().unwrap_or_else(|| PathBuf::from("/"));
        Self {
            root,
            state: ScanState::Idle,
            rx: None,
            nav: Vec::new(),
            selected: None,
        }
    }

    /// Spawn a scan of `self.root` on a worker thread.
    fn start_scan(&mut self, ctx: &egui::Context) {
        self.nav.clear();
        self.selected = None;
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

/// Follow `nav` from the scan root to the currently-viewed node.
fn current_node<'a>(outcome: &'a ScanOutcome, nav: &[usize]) -> &'a Node {
    let mut node = &outcome.root;
    for &i in nav {
        match node.children.get(i) {
            Some(child) => node = child,
            None => break,
        }
    }
    node
}

/// Names from the scan root down to the current node, for the breadcrumb.
fn breadcrumb_names(outcome: &ScanOutcome, nav: &[usize]) -> Vec<String> {
    let mut names = vec![outcome.root.name.clone()];
    let mut node = &outcome.root;
    for &i in nav {
        match node.children.get(i) {
            Some(child) => {
                names.push(child.name.clone());
                node = child;
            }
            None => break,
        }
    }
    names
}

impl eframe::App for OrganizerApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        self.poll_scan();

        let mut scan_requested = false;
        let mut nav_to: Option<usize> = None; // truncate nav to this length
        let mut drill: Option<usize> = None;
        let mut select: Option<usize> = None;

        egui::TopBottomPanel::top("toolbar")
            .exact_height(54.0)
            .show(ctx, |ui| {
                ui.add_space(8.0);
                ui.horizontal(|ui| {
                    ui.add_space(12.0);
                    ui.heading("Disk Space Organizer");
                    ui.add_space(4.0);
                    if !self.nav.is_empty() && ui.button("⟵ Up").clicked() {
                        nav_to = Some(self.nav.len() - 1);
                    }
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        ui.add_space(12.0);
                        let scanning = matches!(self.state, ScanState::Scanning);
                        if ui
                            .add_enabled(!scanning, egui::Button::new("Scan home folder"))
                            .clicked()
                        {
                            scan_requested = true;
                        }
                        ui.add_space(8.0);
                        ui.weak(self.root.display().to_string());
                    });
                });
            });

        if let ScanState::Done(outcome) = &self.state {
            let names = breadcrumb_names(outcome, &self.nav);
            egui::TopBottomPanel::top("breadcrumb").show(ctx, |ui| {
                ui.add_space(2.0);
                ui.horizontal(|ui| {
                    ui.add_space(12.0);
                    for (i, name) in names.iter().enumerate() {
                        if i > 0 {
                            ui.weak("›");
                        }
                        if ui.link(name).clicked() {
                            nav_to = Some(i);
                        }
                    }
                });
                ui.add_space(2.0);
            });
        }

        if let ScanState::Done(outcome) = &self.state {
            if let Some(sel) = self.selected {
                let current = current_node(outcome, &self.nav);
                if let Some(child) = current.children.get(sel) {
                    egui::TopBottomPanel::bottom("status").show(ctx, |ui| {
                        ui.add_space(4.0);
                        ui.horizontal(|ui| {
                            ui.add_space(12.0);
                            ui.strong(&child.name);
                            ui.weak(crate::format::human_size(child.size));
                            let pct = if current.size > 0 {
                                child.size as f64 / current.size as f64 * 100.0
                            } else {
                                0.0
                            };
                            ui.weak(format!("· {pct:.0}% of {}", current.name));
                        });
                        ui.add_space(4.0);
                    });
                }
            }
        }

        let selected = self.selected;
        egui::CentralPanel::default().show(ctx, |ui| match &self.state {
            ScanState::Idle => {
                ui.centered_and_justified(|ui| {
                    ui.label("Press “Scan home folder” to see what’s taking up space.");
                });
            }
            ScanState::Scanning => {
                ui.centered_and_justified(|ui| {
                    ui.horizontal(|ui| {
                        ui.spinner();
                        ui.label("Scanning…");
                    });
                });
            }
            ScanState::Error(e) => {
                ui.centered_and_justified(|ui| {
                    ui.colored_label(egui::Color32::from_rgb(255, 69, 58), e.clone());
                });
            }
            ScanState::Done(outcome) => {
                let current = current_node(outcome, &self.nav);
                let action = crate::treemap::show(ui, current, selected);
                drill = action.drill;
                select = action.selected;
            }
        });

        if scan_requested {
            self.start_scan(ctx);
        }
        if let Some(len) = nav_to {
            self.nav.truncate(len);
            self.selected = None;
        }
        if let Some(i) = drill {
            self.nav.push(i);
            self.selected = None;
        }
        if let Some(i) = select {
            self.selected = Some(i);
        }
    }
}
