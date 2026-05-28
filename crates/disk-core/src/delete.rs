//! Safe deletion: move files/folders to the macOS Trash, behind guards that
//! refuse dangerous targets (the filesystem root, system paths, anything
//! outside the user's configured scan roots).
//!
//! The guard logic ([`check_deletable`]) is pure and exhaustively unit-tested;
//! the UI must still require explicit user confirmation before calling
//! [`move_to_trash`].

use std::path::{Component, Path, PathBuf};

/// System paths that must never be deleted, regardless of scan roots.
///
/// Note: these are absolute `/`-prefixed system locations. The user's own
/// `~/Library` (e.g. `/Users/alice/Library/Caches`) does **not** match
/// `/Library`, so user caches remain deletable.
const PROTECTED_PREFIXES: &[&str] = &[
    "/System",
    "/usr",
    "/bin",
    "/sbin",
    "/private",
    "/Library",
    "/Applications",
    "/cores",
    "/dev",
    "/etc",
    "/var",
];

/// Reason a path was refused for deletion.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum GuardError {
    #[error("refusing to delete the filesystem root")]
    FilesystemRoot,
    #[error("refusing to delete a protected system path: {0}")]
    ProtectedSystemPath(PathBuf),
    #[error("refusing to delete a scan root itself: {0}")]
    ScanRootItself(PathBuf),
    #[error("refusing to delete outside the configured scan roots: {0}")]
    OutsideScanRoots(PathBuf),
    #[error("refusing to delete an unsafe path (must be absolute, no '..'): {0}")]
    UnsafePath(PathBuf),
}

/// Validate that `target` is safe to move to Trash given the user's `scan_roots`.
///
/// Checks, in order: the path is absolute and `..`-free; it is not the
/// filesystem root; it is not a protected system path; it is not a scan root
/// itself; and it lies inside at least one scan root.
pub fn check_deletable(target: &Path, scan_roots: &[PathBuf]) -> Result<(), GuardError> {
    if !is_safe_shape(target) {
        return Err(GuardError::UnsafePath(target.to_path_buf()));
    }
    if target.parent().is_none() {
        return Err(GuardError::FilesystemRoot);
    }
    for &prefix in PROTECTED_PREFIXES {
        if target.starts_with(prefix) {
            return Err(GuardError::ProtectedSystemPath(target.to_path_buf()));
        }
    }
    if scan_roots.iter().any(|root| target == root.as_path()) {
        return Err(GuardError::ScanRootItself(target.to_path_buf()));
    }
    if !scan_roots.iter().any(|root| target.starts_with(root)) {
        return Err(GuardError::OutsideScanRoots(target.to_path_buf()));
    }
    Ok(())
}

/// Error from [`move_to_trash`].
#[derive(Debug, thiserror::Error)]
pub enum DeleteError {
    #[error(transparent)]
    Guard(#[from] GuardError),
    #[error("failed to move to Trash: {0}")]
    Trash(#[from] trash::Error),
}

/// Move `target` to the macOS Trash after passing [`check_deletable`].
///
/// The item lands in the user's Trash and remains recoverable.
pub fn move_to_trash(target: &Path, scan_roots: &[PathBuf]) -> Result<(), DeleteError> {
    check_deletable(target, scan_roots)?;
    trash::delete(target)?;
    Ok(())
}

fn is_safe_shape(target: &Path) -> bool {
    target.is_absolute() && !target.components().any(|c| c == Component::ParentDir)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn roots() -> Vec<PathBuf> {
        vec![
            PathBuf::from("/Users/me/Documents"),
            PathBuf::from("/Volumes/Backup"),
        ]
    }

    #[test]
    fn allows_files_inside_a_scan_root() {
        let r = roots();
        assert!(check_deletable(Path::new("/Users/me/Documents/big.zip"), &r).is_ok());
        assert!(check_deletable(Path::new("/Volumes/Backup/old/movie.mov"), &r).is_ok());
    }

    #[test]
    fn rejects_filesystem_root() {
        assert_eq!(
            check_deletable(Path::new("/"), &roots()),
            Err(GuardError::FilesystemRoot)
        );
    }

    #[test]
    fn rejects_a_scan_root_itself() {
        assert_eq!(
            check_deletable(Path::new("/Users/me/Documents"), &roots()),
            Err(GuardError::ScanRootItself(PathBuf::from(
                "/Users/me/Documents"
            )))
        );
    }

    #[test]
    fn rejects_paths_outside_all_scan_roots() {
        assert_eq!(
            check_deletable(Path::new("/Users/other/secret.txt"), &roots()),
            Err(GuardError::OutsideScanRoots(PathBuf::from(
                "/Users/other/secret.txt"
            )))
        );
    }

    #[test]
    fn rejects_protected_system_paths_even_with_a_broad_root() {
        let r = vec![PathBuf::from("/")];
        assert_eq!(
            check_deletable(Path::new("/System/Library/Kernels"), &r),
            Err(GuardError::ProtectedSystemPath(PathBuf::from(
                "/System/Library/Kernels"
            )))
        );
        assert_eq!(
            check_deletable(Path::new("/usr/bin/ssh"), &r),
            Err(GuardError::ProtectedSystemPath(PathBuf::from(
                "/usr/bin/ssh"
            )))
        );
    }

    #[test]
    fn keeps_user_library_deletable() {
        // ~/Library is a prime cleanup target and must NOT match /Library.
        let r = vec![PathBuf::from("/Users/me")];
        assert!(check_deletable(Path::new("/Users/me/Library/Caches/big"), &r).is_ok());
    }

    #[test]
    fn rejects_parent_dir_traversal() {
        assert_eq!(
            check_deletable(Path::new("/Users/me/Documents/../../other"), &roots()),
            Err(GuardError::UnsafePath(PathBuf::from(
                "/Users/me/Documents/../../other"
            )))
        );
    }

    #[test]
    fn rejects_relative_paths() {
        assert_eq!(
            check_deletable(Path::new("relative/path"), &roots()),
            Err(GuardError::UnsafePath(PathBuf::from("relative/path")))
        );
    }

    #[test]
    #[ignore = "moves a real file to the user's Trash; run manually with --ignored"]
    fn move_to_trash_removes_a_file_within_root() {
        let dir = tempfile::tempdir().unwrap();
        let f = dir.path().join("junk.bin");
        std::fs::write(&f, b"junk").unwrap();
        let roots = vec![dir.path().to_path_buf()];

        move_to_trash(&f, &roots).unwrap();

        assert!(!f.exists());
    }
}
