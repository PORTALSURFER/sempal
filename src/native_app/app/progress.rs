use std::{collections::BTreeMap, path::PathBuf};

use crate::native_app::sample_library::folder_browser::RefreshedFileEntry;
use wavecrate::sample_sources::HarvestDerivationOperation;
use wavecrate::sample_sources::readiness::{ReadinessStage, ReadinessStageCounts};

#[derive(Clone, Debug, PartialEq, Eq)]
pub(in crate::native_app) struct NormalizationProgress {
    pub(in crate::native_app) task_id: u64,
    pub(in crate::native_app) label: String,
    pub(in crate::native_app) completed: usize,
    pub(in crate::native_app) total: usize,
    pub(in crate::native_app) work_completed: usize,
    pub(in crate::native_app) work_total: usize,
    pub(in crate::native_app) queued: usize,
    pub(in crate::native_app) detail: String,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(in crate::native_app) struct NormalizationQueueItem {
    pub(in crate::native_app) paths: Vec<PathBuf>,
    pub(in crate::native_app) source_id: String,
    pub(in crate::native_app) source_root: PathBuf,
    pub(in crate::native_app) source_database_root: PathBuf,
    pub(in crate::native_app) harvest_derivations: Vec<NormalizationHarvestDerivation>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(in crate::native_app) struct NormalizationHarvestDerivation {
    pub(in crate::native_app) source_path: PathBuf,
    pub(in crate::native_app) child_path: PathBuf,
    pub(in crate::native_app) operation: HarvestDerivationOperation,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(in crate::native_app) struct FileMoveProgress {
    pub(in crate::native_app) task_id: u64,
    pub(in crate::native_app) label: String,
    pub(in crate::native_app) completed: usize,
    pub(in crate::native_app) total: usize,
    pub(in crate::native_app) detail: String,
}

/// Durable source-readiness work currently advancing in the background.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(in crate::native_app) struct SourceProcessingProgress {
    pub(in crate::native_app) source_id: String,
    pub(in crate::native_app) lifecycle_generation: u64,
    pub(in crate::native_app) active: bool,
    /// Whether this work owns the source-processing lane strongly enough to
    /// identify the source row as actively advancing. Brief maintenance checks
    /// and manifest audits remain visible in job details without pulsing a row.
    pub(in crate::native_app) source_row_active: bool,
    pub(in crate::native_app) completed: usize,
    pub(in crate::native_app) total: usize,
    pub(in crate::native_app) stage: String,
    pub(in crate::native_app) detail: String,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(in crate::native_app) enum SourceProcessingHealthStatus {
    Ready,
    Processing,
    WaitingForRetry,
    BlockedByPrerequisites,
    Offline,
    Disabled,
    DegradedTerminal,
    ReconciliationFailed,
}

/// Last durable readiness observation for one configured source lifecycle.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(in crate::native_app) struct SourceProcessingHealth {
    pub(in crate::native_app) source_id: String,
    pub(in crate::native_app) lifecycle_generation: u64,
    pub(in crate::native_app) status: SourceProcessingHealthStatus,
    pub(in crate::native_app) source_generation: i64,
    pub(in crate::native_app) readiness_revision: i64,
    pub(in crate::native_app) stage_counts: BTreeMap<ReadinessStage, ReadinessStageCounts>,
    pub(in crate::native_app) retry_at: Option<i64>,
    pub(in crate::native_app) failure_codes: Vec<String>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(in crate::native_app) struct NormalizationFailure {
    pub(in crate::native_app) path: PathBuf,
    pub(in crate::native_app) error: String,
}

#[derive(Clone, Debug, PartialEq)]
pub(in crate::native_app) struct NormalizationResult {
    pub(in crate::native_app) task_id: u64,
    pub(in crate::native_app) source_id: String,
    pub(in crate::native_app) loaded_path: PathBuf,
    pub(in crate::native_app) normalizing_loaded: bool,
    pub(in crate::native_app) was_playing: bool,
    pub(in crate::native_app) restart_ratio: f32,
    pub(in crate::native_app) restart_span: Option<(f32, f32)>,
    pub(in crate::native_app) normalized: Vec<PathBuf>,
    pub(in crate::native_app) refreshed_files: Vec<RefreshedFileEntry>,
    pub(in crate::native_app) skipped: Vec<PathBuf>,
    pub(in crate::native_app) failed: Vec<NormalizationFailure>,
    pub(in crate::native_app) harvest_derivations: Vec<NormalizationHarvestDerivation>,
}
