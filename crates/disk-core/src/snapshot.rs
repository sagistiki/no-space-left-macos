//! Snapshot persistence: save a scanned tree to disk and load it back, plus a
//! cheap listing of all snapshots via sidecar metadata files.
//!
//! Each snapshot is stored as two files in the snapshots directory:
//! - `<id>.snap` — the full tree, encoded with `bincode` (compact).
//! - `<id>.meta.json` — lightweight metadata, so listing never loads trees.

use crate::model::Node;
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use std::time::SystemTime;

/// Lightweight metadata describing a snapshot, cheap to list.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SnapshotMeta {
    pub id: String,
    pub created_at: SystemTime,
    pub roots: Vec<PathBuf>,
    pub total_size: u64,
    pub entry_count: u64,
    pub skipped_count: u64,
}

/// A full snapshot: metadata plus the scanned tree.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Snapshot {
    pub meta: SnapshotMeta,
    pub root: Node,
}

/// Errors from snapshot persistence.
#[derive(Debug, thiserror::Error)]
pub enum SnapshotError {
    #[error("snapshot I/O error: {0}")]
    Io(#[from] std::io::Error),
    #[error("snapshot metadata error: {0}")]
    Json(#[from] serde_json::Error),
    #[error("snapshot encode error: {0}")]
    Encode(String),
    #[error("snapshot decode error: {0}")]
    Decode(String),
}

/// Save `snapshot` into `dir`, returning the path of the written `.snap` file.
///
/// Writes the compact tree (`<id>.snap`) and a human-readable metadata sidecar
/// (`<id>.meta.json`) used by [`list`].
pub fn save(snapshot: &Snapshot, dir: &Path) -> Result<PathBuf, SnapshotError> {
    std::fs::create_dir_all(dir)?;
    let snap_path = dir.join(format!("{}.snap", snapshot.meta.id));
    let meta_path = dir.join(format!("{}.meta.json", snapshot.meta.id));

    let bytes = bincode::serialize(snapshot).map_err(|e| SnapshotError::Encode(e.to_string()))?;
    std::fs::write(&snap_path, bytes)?;

    let meta_json = serde_json::to_vec_pretty(&snapshot.meta)?;
    std::fs::write(&meta_path, meta_json)?;

    Ok(snap_path)
}

/// Load a full snapshot from a `.snap` file.
pub fn load(path: &Path) -> Result<Snapshot, SnapshotError> {
    let bytes = std::fs::read(path)?;
    let snapshot =
        bincode::deserialize(&bytes).map_err(|e| SnapshotError::Decode(e.to_string()))?;
    Ok(snapshot)
}

/// List metadata for every snapshot in `dir` (reads only the sidecar files).
///
/// A missing directory yields an empty list rather than an error.
pub fn list(dir: &Path) -> Result<Vec<SnapshotMeta>, SnapshotError> {
    let mut metas = Vec::new();
    let entries = match std::fs::read_dir(dir) {
        Ok(entries) => entries,
        Err(ref e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(metas),
        Err(e) => return Err(e.into()),
    };
    for entry in entries.flatten() {
        let path = entry.path();
        let is_meta = path
            .file_name()
            .and_then(|n| n.to_str())
            .is_some_and(|n| n.ends_with(".meta.json"));
        if is_meta {
            let bytes = std::fs::read(&path)?;
            let meta: SnapshotMeta = serde_json::from_slice(&bytes)?;
            metas.push(meta);
        }
    }
    Ok(metas)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::EntryKind;

    fn sample() -> Snapshot {
        let root = Node {
            name: "root".into(),
            size: 30,
            kind: EntryKind::Dir,
            modified: None,
            children: vec![
                Node {
                    name: "a.txt".into(),
                    size: 10,
                    kind: EntryKind::File,
                    modified: None,
                    children: vec![],
                },
                Node {
                    name: "b.txt".into(),
                    size: 20,
                    kind: EntryKind::File,
                    modified: None,
                    children: vec![],
                },
            ],
        };
        Snapshot {
            meta: SnapshotMeta {
                id: "20260529-1200".into(),
                created_at: SystemTime::UNIX_EPOCH,
                roots: vec![PathBuf::from("/Users/me")],
                total_size: 30,
                entry_count: 3,
                skipped_count: 0,
            },
            root,
        }
    }

    #[test]
    fn save_then_load_round_trips() {
        let dir = tempfile::tempdir().unwrap();
        let snap = sample();

        let path = save(&snap, dir.path()).unwrap();
        let loaded = load(&path).unwrap();

        assert_eq!(loaded, snap);
    }

    #[test]
    fn list_returns_metadata_for_each_saved_snapshot() {
        let dir = tempfile::tempdir().unwrap();
        let mut a = sample();
        a.meta.id = "aaa".into();
        let mut b = sample();
        b.meta.id = "bbb".into();
        save(&a, dir.path()).unwrap();
        save(&b, dir.path()).unwrap();

        let mut ids: Vec<String> = list(dir.path())
            .unwrap()
            .into_iter()
            .map(|m| m.id)
            .collect();
        ids.sort();

        assert_eq!(ids, vec!["aaa".to_string(), "bbb".to_string()]);
    }

    #[test]
    fn list_is_empty_for_missing_directory() {
        let dir = tempfile::tempdir().unwrap();
        let missing = dir.path().join("nope");
        assert!(list(&missing).unwrap().is_empty());
    }
}
