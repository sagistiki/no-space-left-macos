//! The egui application: window state, the background scan worker, the main
//! treemap view with drill-down, and safe deletion (Trash) with confirmation.
//! Scanning runs on a worker thread so the UI never blocks.

use disk_core::model::Node;
use disk_core::scanner::{self, ScanOptions, ScanOutcome};
use eframe::egui::{self, Align2};
use std::path::{Path, PathBuf};
use std::sync::mpsc::{channel, Receiver};
use std::thread;

/// Where the current scan stands.
enum ScanState {
    Idle,
    Scanning,
    Done(ScanOutcome),
    Error(String),
}

/// A deletion awaiting the user's confirmation.
#[derive(Clone)]
struct PendingTrash {
    path: PathBuf,
    nav: Vec<usize>,
    index: usize,
    name: String,
}

pub struct OrganizerApp {
    root: PathBuf,
    state: ScanState,
    rx: Option<Receiver<Result<ScanOutcome, String>>>,
    /// Drill path from the scan root: successive child indices.
    nav: Vec<usize>,
    /// Selected child index within the current node.
    selected: Option<usize>,
    /// A trash action awaiting confirmation.
    pending_trash: Option<PendingTrash>,
    /// A transient message (text, expiry time in seconds).
    notice: Option<(String, f64)>,
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
            pending_trash: None,
            notice: None,
        }
    }

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

    fn set_notice(&mut self, ctx: &egui::Context, text: String) {
        let now = ctx.input(|i| i.time);
        self.notice = Some((text, now + 4.0));
        ctx.request_repaint();
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

/// Absolute filesystem path of the currently-viewed node.
fn current_path(root: &Path, outcome: &ScanOutcome, nav: &[usize]) -> PathBuf {
    let mut path = root.to_path_buf();
    let mut node = &outcome.root;
    for &i in nav {
        match node.children.get(i) {
            Some(child) => {
                path.push(&child.name);
                node = child;
            }
            None => break,
        }
    }
    path
}

impl eframe::App for OrganizerApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        self.poll_scan();

        let now = ctx.input(|i| i.time);
        if let Some((_, expiry)) = &self.notice {
            if now >= *expiry {
                self.notice = None;
            }
        }

        // Intents gathered this frame, applied after the UI is laid out.
        let mut scan_requested = false;
        let mut nav_to: Option<usize> = None;
        let mut drill: Option<usize> = None;
        let mut select: Option<usize> = None;
        let mut reveal_request: Option<PathBuf> = None;
        let mut set_pending: Option<PendingTrash> = None;
        let mut do_trash = false;
        let mut cancel_trash = false;

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

        // Floating detail + actions popover for the selected item.
        if let ScanState::Done(outcome) = &self.state {
            if let Some(sel) = self.selected {
                let current = current_node(outcome, &self.nav);
                if let Some(child) = current.children.get(sel) {
                    let name = child.name.clone();
                    let size = child.size;
                    let current_name = current.name.clone();
                    let current_size = current.size;
                    let child_path = current_path(&self.root, outcome, &self.nav).join(&child.name);
                    let nav_clone = self.nav.clone();

                    egui::Area::new(egui::Id::new("detail_popover"))
                        .anchor(Align2::CENTER_BOTTOM, egui::vec2(0.0, -22.0))
                        .show(ctx, |ui| {
                            egui::Frame::popup(ui.style()).show(ui, |ui| {
                                ui.set_max_width(580.0);
                                ui.horizontal(|ui| {
                                    ui.vertical(|ui| {
                                        ui.strong(&name);
                                        let pct = if current_size > 0 {
                                            size as f64 / current_size as f64 * 100.0
                                        } else {
                                            0.0
                                        };
                                        ui.weak(format!(
                                            "{} · {pct:.0}% of {current_name}",
                                            crate::format::human_size(size)
                                        ));
                                        ui.small(child_path.display().to_string());
                                    });
                                    ui.add_space(16.0);
                                    ui.vertical(|ui| {
                                        if ui.button("Reveal in Finder").clicked() {
                                            reveal_request = Some(child_path.clone());
                                        }
                                        let trash_btn = egui::Button::new(
                                            egui::RichText::new("Move to Trash")
                                                .color(egui::Color32::WHITE),
                                        )
                                        .fill(egui::Color32::from_rgb(255, 59, 48));
                                        if ui.add(trash_btn).clicked() {
                                            set_pending = Some(PendingTrash {
                                                path: child_path.clone(),
                                                nav: nav_clone.clone(),
                                                index: sel,
                                                name: name.clone(),
                                            });
                                        }
                                    });
                                });
                            });
                        });
                }
            }
        }

        // Confirmation dialog for a pending trash.
        if let Some(pending) = self.pending_trash.clone() {
            egui::Window::new("Move to Trash?")
                .collapsible(false)
                .resizable(false)
                .anchor(Align2::CENTER_CENTER, egui::vec2(0.0, 0.0))
                .show(ctx, |ui| {
                    ui.label(format!(
                        "Move “{}” to the Trash? You can restore it from the Trash later.",
                        pending.name
                    ));
                    ui.add_space(10.0);
                    ui.horizontal(|ui| {
                        if ui.button("Cancel").clicked() {
                            cancel_trash = true;
                        }
                        let go = egui::Button::new(
                            egui::RichText::new("Move to Trash").color(egui::Color32::WHITE),
                        )
                        .fill(egui::Color32::from_rgb(255, 59, 48));
                        if ui.add(go).clicked() {
                            do_trash = true;
                        }
                    });
                });
        }

        // Transient notice toast.
        if let Some((text, _)) = &self.notice {
            let text = text.clone();
            egui::Area::new(egui::Id::new("notice"))
                .anchor(Align2::CENTER_TOP, egui::vec2(0.0, 64.0))
                .show(ctx, |ui| {
                    egui::Frame::popup(ui.style()).show(ui, |ui| {
                        ui.label(text);
                    });
                });
            ctx.request_repaint_after(std::time::Duration::from_millis(250));
        }

        // Apply intents.
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
        if let Some(path) = reveal_request {
            crate::actions::reveal_in_finder(&path);
        }
        if let Some(pending) = set_pending {
            self.pending_trash = Some(pending);
        }
        if cancel_trash {
            self.pending_trash = None;
        }
        if do_trash {
            if let Some(pending) = self.pending_trash.take() {
                match disk_core::delete::move_to_trash(&pending.path, std::slice::from_ref(&self.root))
                {
                    Ok(()) => {
                        if let ScanState::Done(outcome) = &mut self.state {
                            outcome.root.remove_descendant(&pending.nav, pending.index);
                        }
                        self.selected = None;
                        self.set_notice(ctx, format!("Moved “{}” to the Trash", pending.name));
                    }
                    Err(e) => self.set_notice(ctx, format!("Couldn’t move to Trash: {e}")),
                }
            }
        }
    }
}
