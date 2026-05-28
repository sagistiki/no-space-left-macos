//! Core data model: the sized file tree produced by a scan.

use serde::{Deserialize, Serialize};
use std::time::SystemTime;

/// What kind of filesystem entry a [`Node`] represents.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum EntryKind {
    Dir,
    File,
    Symlink,
}

/// One node in the scanned tree.
///
/// Only the component `name` is stored (not the full path) to keep large trees
/// memory-light; full paths are reconstructed during traversal.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Node {
    pub name: String,
    /// Aggregate logical size in bytes. For directories this is the sum of all
    /// descendants; for files it is the file's own length.
    pub size: u64,
    pub kind: EntryKind,
    pub modified: Option<SystemTime>,
    /// Children. Empty for files. Sorted largest-first after a scan.
    pub children: Vec<Node>,
}
