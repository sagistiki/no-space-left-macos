//! Thin wrappers around macOS file actions.

use std::path::Path;
use std::process::Command;

/// Reveal `path` in Finder, selecting it inside its containing folder.
pub fn reveal_in_finder(path: &Path) {
    let _ = Command::new("open").arg("-R").arg(path).spawn();
}

/// Open System Settings at the Full Disk Access pane so the user can grant it.
pub fn open_full_disk_access_settings() {
    let _ = Command::new("open")
        .arg("x-apple.systempreferences:com.apple.preference.security?Privacy_AllFiles")
        .spawn();
}
