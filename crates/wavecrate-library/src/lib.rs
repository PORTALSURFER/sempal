#![deny(missing_docs)]
#![deny(warnings)]

//! Shared application-directory and library-storage helpers.

/// Application directory helpers anchored to the `.wavecrate` root.
pub mod app_dirs;
/// Shared structured debug diagnostics helpers for storage-owned seams.
pub mod diagnostics;
mod env_flags;
/// Stable cross-platform filesystem-object identity helpers.
pub mod filesystem_identity;
/// Sample-source identifiers and persistent storage helpers.
pub mod sample_sources;
/// Optional SQLite extension loader shared by storage code.
pub mod sqlite_ext;
mod sqlite_wal;
/// Process-global test runtime isolation helpers.
#[cfg(any(test, feature = "test-support"))]
pub mod test_runtime;
