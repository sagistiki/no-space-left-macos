//! Thin wrappers around macOS file actions.

use std::path::Path;
use std::process::Command;

/// Reveal `path` in Finder, selecting it inside its containing folder.
pub fn reveal_in_finder(path: &Path) {
    let _ = Command::new("open").arg("-R").arg(path).spawn();
}
