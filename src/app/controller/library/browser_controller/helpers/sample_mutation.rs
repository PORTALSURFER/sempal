//! Browser sample delete, rename, and auto-rename execution helpers.

use super::*;

mod auto_rename;
mod controller;
mod persistence;
mod provenance;
mod rename;

#[cfg(not(test))]
pub(super) const SAMPLE_RENAME_DB_RETRIES: usize = SAMPLE_RENAME_DB_RETRIES_PRODUCTION;
#[cfg(test)]
pub(super) const SAMPLE_RENAME_DB_RETRIES: usize = 4;
#[cfg(not(test))]
pub(super) const SAMPLE_RENAME_DB_RETRY_DELAY: std::time::Duration =
    SAMPLE_RENAME_DB_RETRY_DELAY_PRODUCTION;
#[cfg(test)]
pub(super) const SAMPLE_RENAME_DB_RETRY_DELAY: std::time::Duration =
    std::time::Duration::from_millis(50);

/// Production retry count for browser sample rename DB rewrites.
pub(super) const SAMPLE_RENAME_DB_RETRIES_PRODUCTION: usize = 80;
/// Production retry delay for browser sample rename DB rewrites.
pub(super) const SAMPLE_RENAME_DB_RETRY_DELAY_PRODUCTION: std::time::Duration =
    std::time::Duration::from_millis(100);

/// Request payload for one browser auto-rename target.
pub(crate) struct SampleAutoRenameRequest {
    pub(crate) old_relative: PathBuf,
    pub(crate) new_relative: PathBuf,
    pub(crate) tag: crate::sample_sources::Rating,
    pub(crate) looped: bool,
    pub(crate) locked: bool,
    /// Sound type inferred during controller-side request preparation when the
    /// source DB row does not already store one.
    pub(crate) sound_type: Option<crate::sample_sources::SampleSoundType>,
    pub(crate) user_tag: Option<String>,
    pub(crate) tag_named: bool,
    pub(crate) last_played_at: Option<i64>,
    pub(crate) resume_playback: bool,
    pub(crate) resume_looped: bool,
    pub(crate) resume_start_override: Option<f64>,
}

#[derive(Clone, Copy)]
pub(super) enum RenameLoopedMetadata {
    DbOrFallback(bool),
    RequestSnapshot(bool),
}

impl RenameLoopedMetadata {
    pub(super) fn request_value(self) -> bool {
        match self {
            RenameLoopedMetadata::DbOrFallback(looped)
            | RenameLoopedMetadata::RequestSnapshot(looped) => looped,
        }
    }

    pub(super) fn resolved(self, db_looped: Option<bool>) -> bool {
        match self {
            RenameLoopedMetadata::DbOrFallback(fallback_looped) => {
                db_looped.unwrap_or(fallback_looped)
            }
            RenameLoopedMetadata::RequestSnapshot(looped) => looped,
        }
    }
}

pub(crate) use auto_rename::run_sample_auto_rename_job;
pub(super) use rename::perform_sample_rename;
