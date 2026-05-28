# Disk Space Organizer

A minimal, Apple-flavored macOS app, written in Rust, that shows **where your
disk space went** — which folders and files are largest (as an interactive
treemap), and what changed over time (grew / shrank / appeared / vanished) by
comparing scan snapshots. It can reclaim space with safe, recoverable deletion
(Move to Trash) behind guards.

> אפליקציית macOS ב-Rust שמראה במבט אחד לאן נעלם המקום בדיסק — אילו תיקיות
> וקבצים הכי כבדים (Treemap), ומה השתנה לאורך זמן (גדל/קטן/חדש/נעלם) דרך
> סריקות ו-snapshots. מחיקה בטוחה לסל המיחזור עם הגנות.

## Features

- **Treemap view** — tile area is proportional to size; the biggest things pop.
- **Drill-down** — double-click a folder to zoom in; navigate with the
  breadcrumb or the **Up** button.
- **Scan any folder** — your home folder by default, or pick any folder/drive.
- **Snapshots & compare** — every scan is saved; pick an earlier one to recolor
  the treemap by change (grew = orange, shrank = blue, new = green) with a
  summary of bytes added/removed and a list of items that vanished.
- **Safe deletion** — selecting a tile shows its size, %, and path; *Reveal in
  Finder* or *Move to Trash* (confirmed, recoverable, and blocked for system
  paths and anything outside your scan roots).

## Build & run

Requires a recent stable Rust toolchain.

```sh
# run the app
cargo run -p disk-app --release

# run the test suite (engine logic is fully unit-tested)
cargo test

# lint
cargo clippy --workspace
```

### Build a double-clickable `.app`

```sh
cargo install cargo-bundle
cargo bundle --release        # produces target/release/bundle/osx/Disk Space Organizer.app
```

## Full Disk Access

macOS protects parts of `~/Library` (caches, Mail, Safari, …) and other
locations behind **Full Disk Access**. Without it those folders are skipped and
the app shows a banner offering to open the right Settings pane. To scan
everything, grant access in **System Settings → Privacy & Security → Full Disk
Access** and re-scan.

## Architecture

A Cargo workspace with a strict logic/UI split:

- **`crates/disk-core`** — the engine, with **no UI dependencies**, fully
  unit-tested: `scanner` (sized file tree), `model`, `snapshot` (save/load/list),
  `diff` (grew/shrank/new/vanished), and `delete` (Trash + safety guards).
- **`crates/disk-app`** — the egui/eframe UI: a worker-thread scan, the
  squarified `treemap` renderer, navigation, the detail popover, and snapshots
  compare.

The full design lives in
[`docs/superpowers/specs/2026-05-29-disk-space-organizer-design.md`](docs/superpowers/specs/2026-05-29-disk-space-organizer-design.md).

## Notes & limitations (v1)

- macOS only; on-demand scans (no background daemon).
- Sizes are logical file lengths (`st_size`); APFS clones/snapshots can differ
  from real on-disk usage.
- Scanning is currently single-threaded and recursive; parallel scanning is a
  planned optimization (guarded by the existing tests).
