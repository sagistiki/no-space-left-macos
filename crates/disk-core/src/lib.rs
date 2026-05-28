//! `disk-core` — the engine behind Disk Space Organizer.
//!
//! Pure-Rust logic with **no UI dependencies**: filesystem scanning, snapshot
//! persistence, diffing snapshots over time, and safe (Trash-based) deletion.
//!
//! The UI crate (`disk-app`) is a thin consumer of the types and functions
//! exposed here, which keeps all logic independently unit-testable.

pub mod delete;
pub mod model;
pub mod scanner;
