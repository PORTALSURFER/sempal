//! Application directory helpers anchored to a single `.wavecrate` folder.
//!
//! The helpers centralize where config and log files live across platforms,
//! defaulting to the OS config directory (e.g., `%APPDATA%` on Windows) and
//! allowing a `WAVECRATE_CONFIG_HOME` override for tests or portable setups.
//! Test executables default to an isolated `automated-tests` profile unless
//! they explicitly request the live profile or another config root.

mod error;
mod overrides;
mod paths;
mod profile;
mod profile_ownership;
mod resolution;
mod state;
mod storage_usage;

pub use error::AppDirError;
pub use overrides::{AppRootGuard, ConfigBaseGuard};
pub use paths::{
    clear_rebuildable_cache_payloads, handoff_staging_dir, logs_dir, operation_journal_dir,
    rebuildable_cache_root_dir, waveform_cache_dir,
};
pub use profile::{PersistenceMode, PersistenceProfileGuard, ResolvedPersistence};
pub use profile_ownership::{ProfileOwnershipError, WritableProfileGuard};
pub use resolution::{
    app_root_dir, config_base_dir_path, ensure_test_config_base, persistence_mode,
    resolve_persistence, set_app_root_override,
};
pub use storage_usage::{GlobalStorageUsage, global_storage_usage};

/// Name of the application directory that lives under the OS config root.
pub const APP_DIR_NAME: &str = ".wavecrate";
/// Name of the directory that stores explicit non-live persistence profiles.
pub const PROFILE_DIR_NAME: &str = "profiles";
/// Canonical non-live profile name used for sandbox/manual QA runs.
pub const SANDBOX_PROFILE_NAME: &str = "sandbox";
/// Canonical non-live profile name used for automated validation runs.
pub const AUTOMATED_PROFILE_NAME: &str = "automated-tests";

const CONFIG_PROFILE_ENV: &str = "WAVECRATE_CONFIG_PROFILE";
const TEST_EXECUTABLE_DIR_NAME: &str = "deps";

#[cfg(test)]
mod tests;
