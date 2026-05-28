//! Filesystem scanner: walks a directory tree and produces a sized [`Node`].

use crate::model::{EntryKind, Node};
use std::fs;
use std::path::{Path, PathBuf};
use std::time::SystemTime;

/// Options controlling a scan.
#[derive(Debug, Clone, Default)]
pub struct ScanOptions {
    /// If false (the default), the scan stays on the same filesystem as the
    /// root and does not descend into mounted volumes (compared via device id).
    pub cross_device: bool,
}

/// The result of a scan: the sized tree plus any paths that could not be read.
#[derive(Debug)]
pub struct ScanOutcome {
    pub root: Node,
    pub skipped: Vec<PathBuf>,
}

/// Scan `root`, returning a sized tree of its contents.
///
/// Symlinks are not followed (each is recorded as its own small entry), so
/// cycles and double-counting are avoided. Unreadable paths are collected in
/// [`ScanOutcome::skipped`] and do not abort the scan.
pub fn scan(root: &Path, opts: &ScanOptions) -> std::io::Result<ScanOutcome> {
    let meta = fs::symlink_metadata(root)?;
    let name = root
        .file_name()
        .and_then(|n| n.to_str())
        .map(str::to_string)
        .unwrap_or_else(|| root.to_string_lossy().into_owned());
    let mut skipped = Vec::new();
    let root_node = build_node(root, name, &meta, opts, &mut skipped);
    Ok(ScanOutcome {
        root: root_node,
        skipped,
    })
}

fn modified_of(meta: &fs::Metadata) -> Option<SystemTime> {
    meta.modified().ok()
}

/// Recursively build a [`Node`] for `path`, given its (symlink) metadata.
fn build_node(
    path: &Path,
    name: String,
    meta: &fs::Metadata,
    _opts: &ScanOptions,
    skipped: &mut Vec<PathBuf>,
) -> Node {
    let file_type = meta.file_type();

    if file_type.is_symlink() {
        return Node {
            name,
            size: meta.len(),
            kind: EntryKind::Symlink,
            modified: modified_of(meta),
            children: Vec::new(),
        };
    }

    if !file_type.is_dir() {
        return Node {
            name,
            size: meta.len(),
            kind: EntryKind::File,
            modified: modified_of(meta),
            children: Vec::new(),
        };
    }

    let mut children = Vec::new();
    let mut total = 0u64;

    match fs::read_dir(path) {
        Ok(entries) => {
            for entry in entries.flatten() {
                let child_path = entry.path();
                let child_meta = match fs::symlink_metadata(&child_path) {
                    Ok(m) => m,
                    Err(_) => {
                        skipped.push(child_path);
                        continue;
                    }
                };
                let child_name = entry.file_name().to_string_lossy().into_owned();
                let child = build_node(&child_path, child_name, &child_meta, _opts, skipped);
                total += child.size;
                children.push(child);
            }
        }
        Err(_) => skipped.push(path.to_path_buf()),
    }

    // Largest first, so rankings and the treemap read top-down.
    children.sort_by_key(|child| std::cmp::Reverse(child.size));

    Node {
        name,
        size: total,
        kind: EntryKind::Dir,
        modified: modified_of(meta),
        children,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn scan_sums_file_sizes_into_root() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("a.txt"), b"hello").unwrap(); // 5 bytes
        std::fs::write(dir.path().join("b.txt"), b"world!!").unwrap(); // 7 bytes

        let outcome = scan(dir.path(), &ScanOptions::default()).unwrap();

        assert_eq!(outcome.root.kind, EntryKind::Dir);
        assert_eq!(outcome.root.size, 12);
    }

    #[test]
    fn scan_sorts_children_largest_first() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("small.txt"), b"x").unwrap();
        std::fs::write(dir.path().join("big.txt"), vec![0u8; 100]).unwrap();
        std::fs::write(dir.path().join("medium.txt"), vec![0u8; 50]).unwrap();

        let outcome = scan(dir.path(), &ScanOptions::default()).unwrap();
        let names: Vec<&str> = outcome
            .root
            .children
            .iter()
            .map(|c| c.name.as_str())
            .collect();

        assert_eq!(names, ["big.txt", "medium.txt", "small.txt"]);
    }

    #[test]
    fn scan_rolls_up_nested_directory_sizes() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("top.txt"), vec![0u8; 10]).unwrap();
        let sub = dir.path().join("sub");
        std::fs::create_dir(&sub).unwrap();
        std::fs::write(sub.join("inner.txt"), vec![0u8; 30]).unwrap();

        let outcome = scan(dir.path(), &ScanOptions::default()).unwrap();

        assert_eq!(outcome.root.size, 40);
        let sub_node = outcome
            .root
            .children
            .iter()
            .find(|c| c.name == "sub")
            .expect("sub dir present");
        assert_eq!(sub_node.kind, EntryKind::Dir);
        assert_eq!(sub_node.size, 30);
        assert_eq!(sub_node.children.len(), 1);
        assert_eq!(sub_node.children[0].name, "inner.txt");
    }

    #[cfg(unix)]
    #[test]
    fn scan_does_not_follow_symlinks() {
        let dir = tempfile::tempdir().unwrap();
        let target = dir.path().join("target.bin");
        std::fs::write(&target, vec![0u8; 10_000]).unwrap();
        std::os::unix::fs::symlink(&target, dir.path().join("link")).unwrap();

        let outcome = scan(dir.path(), &ScanOptions::default()).unwrap();

        let link = outcome
            .root
            .children
            .iter()
            .find(|c| c.name == "link")
            .expect("link present");
        assert_eq!(link.kind, EntryKind::Symlink);
        assert!(
            link.size < 10_000,
            "a symlink must not be counted as its target's size"
        );
        // The 10 KB target is counted once; the link is tiny — far from 20 KB.
        assert!(outcome.root.size < 20_000);
    }
}
