# Disk Space Organizer — Design Spec

- **Date:** 2026-05-29
- **Status:** Approved (brainstorming complete)
- **Platform:** macOS only
- **Language/UI:** Rust + egui/eframe (pure Rust)

> **תקציר בעברית:** אפליקציית macOS ב-Rust שמראה במבט אחד לאן נעלם המקום בדיסק — אילו תיקיות וקבצים הכי כבדים (Treemap), ומה השתנה לאורך זמן (גדל/קטן/חדש/נעלם) דרך סריקות ידניות ו-snapshots. עיצוב מינימלי בסגנון אפל. תומך במחיקה בטוחה (סל מיחזור) עם הגנות.

## 1. Overview

A macOS desktop application that answers, at a glance: **where did my disk space go?** It ranks the heaviest folders and files via an interactive treemap, and tracks change over time by comparing on-demand scan snapshots (what grew, shrank, appeared, or vanished). It supports reclaiming space through safe, recoverable deletion (move to Trash) protected by guards.

## 2. Goals

- See the largest folders and files at a glance, ordered by size.
- Track change over time: manual scans → saved snapshots → diff (grew / shrank / new / vanished).
- Reclaim space safely: move to Trash with confirmation and protected-path guards.
- Clean, minimal, Apple-style aesthetic.
- Professional, high-quality, well-tested, all-Rust codebase.

## 3. Non-Goals (v1 — YAGNI)

- No background daemon / real-time file watching.
- No automatic scheduled scans.
- No file categorization / type classification — **pure size ranking** only.
- No cross-platform support (macOS only).
- No "smart cleaning" rules or automatic delete suggestions.
- No cloud / sync / multi-user.

## 4. Decisions (from brainstorming)

| Topic | Decision |
|---|---|
| Platform | macOS only |
| Actions | Analyze **+ safe delete** (Trash, confirmation, guards) |
| Monitoring | Manual scan + saved snapshots + comparison (no daemon) |
| Scan scope | Home (`~`) by default + user-added folders/drives; request Full Disk Access |
| Categorization | None — pure size ranking |
| GUI framework | egui / eframe (pure Rust) |
| Aesthetic | Clean / minimal "Apple-style" (not native-chrome-identical) |
| Core visualization | Treemap (custom painter), with drill-down |
| Window layout | Single-pane: full-bleed treemap + unified top toolbar + floating detail/action popover; compare = treemap color-mode |

## 5. Architecture

A **Cargo workspace** with two crates, enforcing a hard separation between logic and UI:

- **`disk-core`** — library crate, **zero UI dependencies**. All logic lives here and is unit-tested without running a UI.
- **`disk-app`** — binary crate (eframe/egui). Pure view + interaction; a consumer of `disk-core`.

```
disk-app (egui/eframe)
  app state · treemap renderer · toolbar · popover · sheets · theme
        │  calls
        ▼
disk-core (pure Rust, no UI)
  scanner · model · snapshot · diff · delete+guards · config
        │  reads/writes
        ▼
local storage (~/Library/Application Support/…)  +  macOS integration (Trash, Reveal, FDA, system font)
```

**Why a workspace:** the core is independently testable and reusable (a future CLI or alternative UI could sit on top), and compilation is faster. This is the standard professional structure.

### Data flow

- **Scan:** user triggers scan → scanner walks roots in parallel → builds a sized `Node` tree → saved as a snapshot → treemap renders it.
- **Compare:** user picks a snapshot → `diff(current, old)` → treemap recolors by delta + a summary strip + a "vanished" list.
- **Delete:** click tile → detail popover → Move to Trash → guard check → confirm sheet → `trash::delete`.

## 6. `disk-core` Modules

### 6.1 `model`

```
enum EntryKind { Dir, File, Symlink }
struct Node {
    name: String,            // component name; full path reconstructed from parents
    size: u64,               // aggregate logical size in bytes (dir = sum of children)
    kind: EntryKind,
    modified: Option<SystemTime>,
    children: Vec<Node>,     // empty for files
}
```

- Store the component `name`, not the full `PathBuf`, to keep large trees memory-light; reconstruct full paths on demand during traversal.
- Directory size is the bottom-up sum of its children.
- Provide helpers: `children_sorted_by_size()`, `find_by_path()`, `total_entries()`.

### 6.2 `scanner`

- Parallel directory traversal with **`jwalk`** (multi-threaded walkdir).
- Use `symlink_metadata` so symlinks are **not** followed (avoids cycles and double counting); a symlink counts as its own tiny entry.
- Logical size from `metadata.len()` (`st_size`).
- **Device boundary:** by default do not cross filesystem boundaries within a root (compare `st_dev`), so a scan doesn't wander into mounted network/external volumes unexpectedly. Configurable.
- **Permissions:** unreadable directories (no Full Disk Access) are recorded as skipped errors and the scan continues — never abort the whole scan. Return a count + sample of skipped paths.
- **Progress:** report progress (entries scanned, bytes, current path) through an `mpsc` channel / callback so the UI stays responsive. Runs off the UI thread.
- **APFS note:** `st_size` is logical size; APFS clones/snapshots/purgeable space mean logical totals can differ from real on-disk usage. v1 ranks by logical size (consistent, good for "what's biggest"). An allocated-size mode (`st_blocks`) can be added later.

### 6.3 `snapshot`

- A snapshot = the scanned `Node` tree + metadata: `id`, `created_at`, `roots`, `total_size`, `entry_count`, `skipped_count`, `app_version`.
- Persist to `~/Library/Application Support/<bundle-id>/snapshots/<timestamp>.snap`.
- Serialize with **`bincode`** (compact, fast) via `serde`.
- Maintain a lightweight `index.json` listing snapshots (id, timestamp, roots, totals) for fast listing without loading full trees.
- Load full tree on demand.

### 6.4 `diff`

- Input: two trees (current + a snapshot, or two snapshots).
- Index the old tree by full path in a `HashMap<PathBuf, &Node>`; traverse the new tree and classify each path:
  - **New** — present in new, absent in old.
  - **Vanished** — present in old, absent in new.
  - **Grew** / **Shrank** — size delta beyond a configurable threshold.
  - **Unchanged** — within threshold.
- Output: `DiffResult` mapping path → `Delta { old, new, change }`, plus a sorted list of top changes and aggregate totals (+bytes, −bytes). Drives treemap recoloring and the changes list.

### 6.5 `delete` + `guards`

- `move_to_trash(path)` via the **`trash`** crate (uses `NSFileManager` on macOS → recoverable user Trash).
- **Guards** (`guards::check(path)`), refuse to delete when:
  - path is `/`, `~`, or a configured scan root itself;
  - path is outside all configured scan roots;
  - path matches a denylist of system-critical roots (`/System`, `/usr`, `/bin`, `/sbin`, `/Library` system areas, etc.).
- The core enforces guards and performs the trash op. The **UI always requires explicit confirmation** (Confirm Sheet) before calling delete.

### 6.6 `config`

- Scan roots (default: home dir; plus user-added folders/drives).
- Preferences: theme (system/light/dark), cross-device toggle, diff size threshold.
- Persisted as `config.json` in the app support dir.

## 7. `disk-app` (UI)

### 7.1 Layout (single-pane / Layout B)

- **Unified top toolbar:** Source selector (▾), clickable breadcrumb path (navigate up), **Scan** button, **Snapshots** menu (▾, includes Compare).
- **Full-bleed treemap** fills the window.
- **Detail Popover** (on tile click): name, full path, size, % of parent, modified date; actions **Reveal in Finder**, **Move to Trash**.
- **Drill-down:** double-click a directory tile to zoom in; breadcrumb updates; navigate back up via breadcrumb.
- **Compare mode:** pick a snapshot → treemap recolors by delta (grew = warm, shrank = cool, new = green); vanished items (absent from the treemap) appear in a side list; a thin summary strip shows totals.

### 7.2 `treemap` renderer

- **Squarified treemap** algorithm for good tile aspect ratios.
- Custom painting via egui `Painter`: rounded rects + labels (label drawn only when the tile is large enough).
- Interaction: hover highlight, click select, double-click drill.
- Coloring: by depth/size normally; by delta in compare mode.
- **Performance:** cull tiles below a pixel-size threshold; bound drawn depth; treemap layout computed for the current sub-tree only.

### 7.3 `theme`

- Clean typography: use the system font (SF Pro) if available, else bundle **Inter**.
- Follow system light/dark. Apple-ish palette, generous spacing, subtle shadows, rounded corners, restrained accent color.

### 7.4 Threading

- Scans run on a worker thread; progress + result delivered over an `mpsc` channel. UI polls each frame and shows a progress indicator. The UI never blocks.

## 8. Error Handling

- **Scanner:** per-path errors collected; scan continues. Summary of skipped/unreadable shown; if many permission errors, prompt to enable Full Disk Access.
- **Snapshot I/O:** load/save failures surfaced as non-fatal notifications.
- **Delete:** guard violations and Trash failures shown in a sheet; never crash.
- Error types: `thiserror` in `disk-core`; `anyhow` for context in `disk-app`.

## 9. Testing

- `disk-core` unit tests (TDD):
  - **scanner** — build a temp dir tree (`tempfile`); assert sizes/structure; symlink handling; permission-skip behavior; device-boundary behavior.
  - **diff** — synthetic old/new trees; assert New / Vanished / Grew / Shrank / Unchanged classification and totals.
  - **snapshot** — save/load round-trip equality.
  - **guards** — protected/system/out-of-root paths refused; in-root paths allowed.
  - **treemap squarify** — geometry/aspect-ratio unit tests (pure function).
- UI: light; focus tests on pure logic (view-model state transitions, squarify).

## 10. Dependencies (proposed)

- **`disk-core`:** `jwalk`, `serde`, `bincode`, `serde_json`, `thiserror`, `trash`, `dirs`; dev: `tempfile`.
- **`disk-app`:** `eframe`/`egui`, `egui_extras`, `anyhow`, `rfd` (native folder picker). Reveal in Finder via `open -R` (`std::process::Command`).

## 11. Packaging

- Produce a proper `.app` bundle (`cargo-bundle` or manual `Info.plist`) so the app can be granted **Full Disk Access** in System Settings → Privacy & Security. Document the FDA-granting steps for the user.

## 12. Implementation Plan (phases)

1. **Scaffold** workspace + two crates + shared lints/`Cargo.toml`; `git init`; commit.
2. **`core::model` + `core::scanner`** (TDD) — scan a directory into a sized tree.
3. **`core::snapshot`** — save/load (TDD).
4. **`core::diff`** (TDD).
5. **`core::delete` + `guards`** (TDD).
6. **App skeleton** — eframe window + theme + toolbar; run a scan on a worker thread with progress.
7. **Treemap renderer** — squarify + paint + interaction (hover/select/drill/breadcrumb).
8. **Detail popover** — reveal + trash (confirm sheet).
9. **Snapshots UI** — save/list + compare mode (recolor + summary + vanished list).
10. **Polish** — empty/error states, Full Disk Access prompt, `.app` packaging.
