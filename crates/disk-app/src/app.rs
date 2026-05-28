//! The egui application: window state, the background scan worker, the main
//! treemap view with drill-down, safe deletion (Trash), and snapshot
//! comparison over time. Scanning runs on a worker thread so the UI never
//! blocks; each completed scan is also saved as a snapshot.

use disk_core::diff::{self, Change};
use disk_core::model::Node;
use disk_core::scanner::{self, ScanOptions, ScanOutcome};
use disk_core::snapshot::{self, Snapshot, SnapshotMeta};
use eframe::egui::{self, Align2, Color32};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::mpsc::{channel, Receiver};
use std::thread;
use std::time::{SystemTime, UNIX_EPOCH};

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

/// An active comparison against an earlier snapshot.
struct CompareState {
    label: String,
    /// Change per path, relative to the scan root (vanished entries excluded).
    changes: HashMap<PathBuf, Change>,
    total_added: u64,
    total_removed: u64,
    /// Paths present in the snapshot but gone now (relative path, old size).
    vanished: Vec<(PathBuf, u64)>,
}

pub struct OrganizerApp {
    root: PathBuf,
    state: ScanState,
    rx: Option<Receiver<Result<ScanOutcome, String>>>,
    nav: Vec<usize>,
    selected: Option<usize>,
    pending_trash: Option<PendingTrash>,
    notice: Option<(String, f64)>,
    snapshots: Vec<SnapshotMeta>,
    compare: Option<CompareState>,
}

impl OrganizerApp {
    pub fn new(cc: &eframe::CreationContext<'_>) -> Self {
        crate::theme::apply(&cc.egui_ctx);
        let root = dirs::home_dir().unwrap_or_else(|| PathBuf::from("/"));
        let mut app = Self {
            root,
            state: ScanState::Idle,
            rx: None,
            nav: Vec::new(),
            selected: None,
            pending_trash: None,
            notice: None,
            snapshots: Vec::new(),
            compare: None,
        };
        app.refresh_snapshots();
        app
    }

    fn start_scan(&mut self, ctx: &egui::Context) {
        self.nav.clear();
        self.selected = None;
        self.compare = None;
        let root = self.root.clone();
        let (tx, rx) = channel();
        self.rx = Some(rx);
        self.state = ScanState::Scanning;

        let ctx = ctx.clone();
        thread::spawn(move || {
            let result = scanner::scan(&root, &ScanOptions::default()).map_err(|e| e.to_string());
            // Best-effort: persist this scan as a snapshot for later comparison.
            if let Ok(outcome) = &result {
                if let Some(dir) = snapshots_dir() {
                    let _ = snapshot::save(&build_snapshot(&root, outcome), &dir);
                }
            }
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
                self.refresh_snapshots();
            }
        }
    }

    fn refresh_snapshots(&mut self) {
        if let Some(dir) = snapshots_dir() {
            if let Ok(mut metas) = snapshot::list(&dir) {
                metas.sort_by(|a, b| b.created_at.cmp(&a.created_at));
                self.snapshots = metas;
            }
        }
    }

    fn set_notice(&mut self, ctx: &egui::Context, text: String) {
        let now = ctx.input(|i| i.time);
        self.notice = Some((text, now + 4.0));
        ctx.request_repaint();
    }

    fn start_compare(&mut self, ctx: &egui::Context, meta: &SnapshotMeta) {
        let Some(dir) = snapshots_dir() else { return };
        let old = match snapshot::load(&dir.join(format!("{}.snap", meta.id))) {
            Ok(s) => s,
            Err(e) => {
                self.set_notice(ctx, format!("Couldn’t load snapshot: {e}"));
                return;
            }
        };

        let built = if let ScanState::Done(outcome) = &self.state {
            let result = diff::diff(&old.root, &outcome.root, 0);
            let mut changes = HashMap::new();
            let mut vanished = Vec::new();
            for d in &result.deltas {
                if d.change == Change::Vanished {
                    vanished.push((d.path.clone(), d.old_size));
                } else {
                    changes.insert(d.path.clone(), d.change);
                }
            }
            Some(CompareState {
                label: relative_time(meta.created_at),
                changes,
                total_added: result.total_added,
                total_removed: result.total_removed,
                vanished,
            })
        } else {
            None
        };

        match built {
            Some(cs) => {
                let label = cs.label.clone();
                self.compare = Some(cs);
                self.set_notice(ctx, format!("Comparing with snapshot from {label}"));
            }
            None => self.set_notice(ctx, "Scan first, then compare.".to_string()),
        }
    }
}

fn snapshots_dir() -> Option<PathBuf> {
    dirs::data_dir().map(|d| d.join("no-space-left").join("snapshots"))
}

fn build_snapshot(root: &Path, outcome: &ScanOutcome) -> Snapshot {
    let created_at = SystemTime::now();
    let id = created_at
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
        .to_string();
    Snapshot {
        meta: SnapshotMeta {
            id,
            created_at,
            roots: vec![root.to_path_buf()],
            total_size: outcome.root.size,
            entry_count: outcome.root.total_entries(),
            skipped_count: outcome.skipped.len() as u64,
        },
        root: outcome.root.clone(),
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
    walk_names(outcome, nav, |name| path.push(name));
    path
}

/// Path of the current node relative to the scan root (matches diff keys).
fn current_rel_path(outcome: &ScanOutcome, nav: &[usize]) -> PathBuf {
    let mut path = PathBuf::new();
    walk_names(outcome, nav, |name| path.push(name));
    path
}

fn walk_names(outcome: &ScanOutcome, nav: &[usize], mut push: impl FnMut(&str)) {
    let mut node = &outcome.root;
    for &i in nav {
        match node.children.get(i) {
            Some(child) => {
                push(&child.name);
                node = child;
            }
            None => break,
        }
    }
}

fn compare_color(change: Option<Change>) -> Color32 {
    match change {
        Some(Change::New) => Color32::from_rgb(0xBC, 0xE6, 0xC8), // pastel green
        Some(Change::Grew) => Color32::from_rgb(0xFF, 0xD9, 0xB0), // pastel amber
        Some(Change::Shrank) => Color32::from_rgb(0xC2, 0xD5, 0xF2), // pastel blue
        _ => Color32::from_rgb(0xE4, 0xE8, 0xEE),                 // pale gray
    }
}

fn relative_time(created: SystemTime) -> String {
    match SystemTime::now().duration_since(created) {
        Ok(d) => {
            let s = d.as_secs();
            if s < 60 {
                "just now".to_string()
            } else if s < 3600 {
                format!("{} min ago", s / 60)
            } else if s < 86_400 {
                format!("{} h ago", s / 3600)
            } else {
                format!("{} d ago", s / 86_400)
            }
        }
        Err(_) => "the future".to_string(),
    }
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

        let mut scan_requested = false;
        let mut nav_to: Option<usize> = None;
        let mut drill: Option<usize> = None;
        let mut select: Option<usize> = None;
        let mut reveal_request: Option<PathBuf> = None;
        let mut set_pending: Option<PendingTrash> = None;
        let mut do_trash = false;
        let mut cancel_trash = false;
        let mut compare_with: Option<SnapshotMeta> = None;
        let mut exit_compare = false;
        let mut pick_folder = false;
        let mut open_fda = false;

        egui::TopBottomPanel::top("toolbar")
            .exact_height(54.0)
            .show(ctx, |ui| {
                ui.add_space(8.0);
                ui.horizontal(|ui| {
                    ui.add_space(12.0);
                    ui.heading("no space left");
                    ui.add_space(4.0);
                    if !self.nav.is_empty() && ui.button("↑ Up").clicked() {
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
                        if ui.button("Choose folder…").clicked() {
                            pick_folder = true;
                        }
                        ui.add_space(8.0);
                        ui.menu_button("Snapshots ▾", |ui| {
                            if self.snapshots.is_empty() {
                                ui.label("No snapshots yet");
                            }
                            for meta in &self.snapshots {
                                let label = format!(
                                    "{} · {}",
                                    relative_time(meta.created_at),
                                    crate::format::human_size(meta.total_size)
                                );
                                if ui.button(label).clicked() {
                                    compare_with = Some(meta.clone());
                                    ui.close_menu();
                                }
                            }
                        });
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
            if !outcome.skipped.is_empty() {
                let n = outcome.skipped.len();
                egui::TopBottomPanel::top("fda_hint").show(ctx, |ui| {
                    ui.add_space(3.0);
                    ui.horizontal(|ui| {
                        ui.add_space(12.0);
                        ui.label(format!("⚠ {n} item(s) couldn’t be read."));
                        ui.weak("Grant Full Disk Access to scan everything.");
                        if ui.button("Open Privacy Settings").clicked() {
                            open_fda = true;
                        }
                    });
                    ui.add_space(3.0);
                });
            }
        }

        if let Some(cs) = &self.compare {
            egui::TopBottomPanel::bottom("compare_bar").show(ctx, |ui| {
                ui.add_space(4.0);
                ui.horizontal(|ui| {
                    ui.add_space(12.0);
                    ui.strong(format!("Comparing with snapshot from {}", cs.label));
                    ui.colored_label(
                        crate::theme::ADDED,
                        format!("+{}", crate::format::human_size(cs.total_added)),
                    );
                    ui.colored_label(
                        crate::theme::REMOVED,
                        format!("−{}", crate::format::human_size(cs.total_removed)),
                    );
                    if !cs.vanished.is_empty() {
                        ui.weak(format!("· {} vanished", cs.vanished.len()));
                    }
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        ui.add_space(12.0);
                        if ui.button("Exit compare").clicked() {
                            exit_compare = true;
                        }
                    });
                });
                if !cs.vanished.is_empty() {
                    ui.collapsing("Vanished items", |ui| {
                        egui::ScrollArea::vertical()
                            .max_height(160.0)
                            .show(ui, |ui| {
                                for (path, size) in cs.vanished.iter().take(50) {
                                    ui.horizontal(|ui| {
                                        ui.weak(crate::format::human_size(*size));
                                        ui.label(path.display().to_string());
                                    });
                                }
                            });
                    });
                }
                ui.add_space(4.0);
            });
        }

        let selected = self.selected;
        egui::CentralPanel::default()
            .frame(egui::Frame::central_panel(&ctx.style()).fill(crate::theme::BG))
            .show(ctx, |ui| match &self.state {
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
                        ui.colored_label(crate::theme::DANGER, e.clone());
                    });
                }
                ScanState::Done(outcome) => {
                    let current = current_node(outcome, &self.nav);
                    let colors: Option<Vec<Color32>> = self.compare.as_ref().map(|cs| {
                        let prefix = current_rel_path(outcome, &self.nav);
                        current
                            .children
                            .iter()
                            .map(|child| {
                                compare_color(cs.changes.get(&prefix.join(&child.name)).copied())
                            })
                            .collect()
                    });
                    let action = crate::treemap::show(ui, current, selected, colors.as_deref());
                    drill = action.drill;
                    select = action.selected;
                }
            });

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
                        .anchor(Align2::CENTER_BOTTOM, egui::vec2(0.0, -64.0))
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
                                        .fill(crate::theme::DANGER);
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
                        .fill(crate::theme::DANGER);
                        if ui.add(go).clicked() {
                            do_trash = true;
                        }
                    });
                });
        }

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
        if let Some(meta) = compare_with {
            self.start_compare(ctx, &meta);
        }
        if exit_compare {
            self.compare = None;
        }
        if pick_folder {
            if let Some(dir) = rfd::FileDialog::new().pick_folder() {
                self.root = dir;
                self.start_scan(ctx);
            }
        }
        if open_fda {
            crate::actions::open_full_disk_access_settings();
        }
        if do_trash {
            if let Some(pending) = self.pending_trash.take() {
                match disk_core::delete::move_to_trash(
                    &pending.path,
                    std::slice::from_ref(&self.root),
                ) {
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
