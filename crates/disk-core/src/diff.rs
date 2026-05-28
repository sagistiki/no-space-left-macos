//! Diffing two scanned trees: what grew, shrank, appeared, or vanished.
//!
//! Trees are compared by path *relative to the scan root*, so two scans of the
//! same root line up regardless of where the root lives. The change list is
//! folder-aware (a directory that grew is reported alongside the file inside it
//! that caused it), while the added/removed totals are computed at the file
//! level so a change is never counted twice.

use crate::model::{EntryKind, Node};
use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};

/// How a single path changed between two scans.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Change {
    New,
    Vanished,
    Grew,
    Shrank,
    Unchanged,
}

/// The change for one path (relative to the scan root).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Delta {
    pub path: PathBuf,
    pub old_size: u64,
    pub new_size: u64,
    pub change: Change,
}

impl Delta {
    /// Absolute size change, used for ranking.
    pub fn magnitude(&self) -> u64 {
        self.new_size.abs_diff(self.old_size)
    }
}

/// The result of diffing two trees.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DiffResult {
    /// Changed paths only (no `Unchanged`), sorted by magnitude, largest first.
    pub deltas: Vec<Delta>,
    /// Total bytes added across files (growth + new files).
    pub total_added: u64,
    /// Total bytes removed across files (shrinkage + vanished files).
    pub total_removed: u64,
}

/// Diff `old` against `new`. Size changes within `threshold` count as unchanged.
pub fn diff(old: &Node, new: &Node, threshold: u64) -> DiffResult {
    let mut old_all = HashMap::new();
    let mut new_all = HashMap::new();
    flatten_all(old, Path::new(""), &mut old_all);
    flatten_all(new, Path::new(""), &mut new_all);

    let mut all_paths: HashSet<&PathBuf> = old_all.keys().collect();
    all_paths.extend(new_all.keys());

    let mut deltas = Vec::new();
    for path in all_paths {
        let (old_size, new_size, change) =
            match (old_all.get(path).copied(), new_all.get(path).copied()) {
                (Some(o), Some(n)) => {
                    let change = if n > o && n - o > threshold {
                        Change::Grew
                    } else if o > n && o - n > threshold {
                        Change::Shrank
                    } else {
                        Change::Unchanged
                    };
                    (o, n, change)
                }
                (None, Some(n)) => (0, n, Change::New),
                (Some(o), None) => (o, 0, Change::Vanished),
                (None, None) => continue,
            };
        if change != Change::Unchanged {
            deltas.push(Delta {
                path: path.clone(),
                old_size,
                new_size,
                change,
            });
        }
    }
    deltas.sort_by_key(|d| std::cmp::Reverse(d.magnitude()));

    let mut old_files = HashMap::new();
    let mut new_files = HashMap::new();
    flatten_files(old, Path::new(""), &mut old_files);
    flatten_files(new, Path::new(""), &mut new_files);

    let mut total_added = 0u64;
    let mut total_removed = 0u64;
    let mut file_paths: HashSet<&PathBuf> = old_files.keys().collect();
    file_paths.extend(new_files.keys());
    for path in file_paths {
        let o = old_files.get(path).copied().unwrap_or(0);
        let n = new_files.get(path).copied().unwrap_or(0);
        if n > o {
            total_added += n - o;
        } else if o > n {
            total_removed += o - n;
        }
    }

    DiffResult {
        deltas,
        total_added,
        total_removed,
    }
}

/// Flatten every node (dirs and files) into `out`, keyed by path relative to the
/// scan root.
fn flatten_all(node: &Node, prefix: &Path, out: &mut HashMap<PathBuf, u64>) {
    for child in &node.children {
        let rel = prefix.join(&child.name);
        out.insert(rel.clone(), child.size);
        if child.kind == EntryKind::Dir {
            flatten_all(child, &rel, out);
        }
    }
}

/// Flatten only space-holding leaves (files and symlinks), so totals computed
/// from this map never double-count a directory and its contents.
fn flatten_files(node: &Node, prefix: &Path, out: &mut HashMap<PathBuf, u64>) {
    for child in &node.children {
        let rel = prefix.join(&child.name);
        if child.kind == EntryKind::Dir {
            flatten_files(child, &rel, out);
        } else {
            out.insert(rel, child.size);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn file(name: &str, size: u64) -> Node {
        Node {
            name: name.into(),
            size,
            kind: EntryKind::File,
            modified: None,
            children: vec![],
        }
    }

    fn dir(name: &str, children: Vec<Node>) -> Node {
        let size = children.iter().map(|c| c.size).sum();
        Node {
            name: name.into(),
            size,
            kind: EntryKind::Dir,
            modified: None,
            children,
        }
    }

    fn find<'a>(result: &'a DiffResult, path: &str) -> Option<&'a Delta> {
        result.deltas.iter().find(|d| d.path == Path::new(path))
    }

    #[test]
    fn classifies_new_grown_shrunk_and_vanished() {
        let old = dir(
            "root",
            vec![
                file("keep.txt", 100),
                file("gone.txt", 50),
                file("steady.txt", 10),
                file("shrink.txt", 200),
            ],
        );
        let new = dir(
            "root",
            vec![
                file("keep.txt", 180),
                file("steady.txt", 10),
                file("shrink.txt", 120),
                file("fresh.txt", 70),
            ],
        );

        let result = diff(&old, &new, 0);

        assert_eq!(find(&result, "keep.txt").unwrap().change, Change::Grew);
        assert_eq!(find(&result, "keep.txt").unwrap().new_size, 180);
        assert_eq!(find(&result, "shrink.txt").unwrap().change, Change::Shrank);
        assert_eq!(find(&result, "gone.txt").unwrap().change, Change::Vanished);
        assert_eq!(find(&result, "gone.txt").unwrap().old_size, 50);
        assert_eq!(find(&result, "fresh.txt").unwrap().change, Change::New);
        assert!(find(&result, "steady.txt").is_none(), "unchanged is excluded");
    }

    #[test]
    fn totals_count_files_once_not_parent_dirs() {
        let old = dir("root", vec![dir("sub", vec![file("big.bin", 100)])]);
        let new = dir("root", vec![dir("sub", vec![file("big.bin", 200)])]);

        let result = diff(&old, &new, 0);

        assert_eq!(result.total_added, 100, "the 100-byte growth counts once");
        assert_eq!(result.total_removed, 0);
        assert!(find(&result, "sub").is_some(), "folder-aware: dir reported");
        assert!(find(&result, "sub/big.bin").is_some(), "file reported too");
    }

    #[test]
    fn respects_threshold_for_grew_and_shrank() {
        let old = dir("root", vec![file("a.txt", 100), file("b.txt", 100)]);
        let new = dir("root", vec![file("a.txt", 105), file("b.txt", 300)]);

        let result = diff(&old, &new, 10);

        assert!(find(&result, "a.txt").is_none(), "5-byte change below threshold");
        assert_eq!(find(&result, "b.txt").unwrap().change, Change::Grew);
    }

    #[test]
    fn deltas_sorted_by_magnitude_descending() {
        let old = dir("root", vec![file("small.txt", 10), file("huge.txt", 10)]);
        let new = dir("root", vec![file("small.txt", 20), file("huge.txt", 1000)]);

        let result = diff(&old, &new, 0);

        assert_eq!(result.deltas.first().unwrap().path, PathBuf::from("huge.txt"));
    }
}
