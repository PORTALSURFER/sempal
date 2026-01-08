//! Runtime state and job coordination for the controller.

use crate::egui_app::controller::library::analysis_jobs;
use crate::egui_app::controller::jobs;
use crate::sample_sources::{ScanMode, SourceId, WavEntry};
use crate::sample_sources::db::SourceDbError;
use crate::sample_sources::config::PannsBackendChoice;
use std::path::PathBuf;
use std::time::{Duration, Instant};

pub(crate) struct ControllerRuntimeState {
    pub(crate) jobs: jobs::ControllerJobs,
    pub(crate) analysis: analysis_jobs::AnalysisWorkerPool,
    pub(crate) performance: PerformanceGovernorState,
    pub(crate) pending_backend_switch: Option<PannsBackendChoice>,
    pub(crate) similarity_prep: Option<SimilarityPrepState>,
    pub(crate) similarity_prep_last_error: Option<String>,
    pub(crate) similarity_prep_force_full_analysis_next: bool,
    #[cfg(test)]
    pub(crate) progress_cancel_after: Option<usize>,
}

impl ControllerRuntimeState {
    pub(crate) fn new(
        jobs: jobs::ControllerJobs,
        analysis: analysis_jobs::AnalysisWorkerPool,
    ) -> Self {
        Self {
            jobs,
            analysis,
            performance: PerformanceGovernorState::new(),
            pending_backend_switch: None,
            similarity_prep: None,
            similarity_prep_last_error: None,
            similarity_prep_force_full_analysis_next: false,
            #[cfg(test)]
            progress_cancel_after: None,
        }
    }
}

pub(crate) struct PerformanceGovernorState {
    pub(crate) last_user_activity_at: Option<Instant>,
    pub(crate) last_slow_frame_at: Option<Instant>,
    pub(crate) last_frame_at: Option<Instant>,
    pub(crate) last_worker_count: Option<u32>,
    pub(crate) idle_worker_override: Option<u32>,
}

impl PerformanceGovernorState {
    pub(crate) fn new() -> Self {
        Self {
            last_user_activity_at: None,
            last_slow_frame_at: None,
            last_frame_at: None,
            last_worker_count: None,
            idle_worker_override: None,
        }
    }
}

#[derive(Clone, Debug)]
pub(crate) struct SimilarityPrepState {
    pub(crate) source_id: SourceId,
    pub(crate) stage: SimilarityPrepStage,
    pub(crate) umap_version: String,
    pub(crate) scan_completed_at: Option<i64>,
    pub(crate) skip_backfill: bool,
    pub(crate) force_full_analysis: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum SimilarityPrepStage {
    AwaitScan,
    AwaitEmbeddings,
    Finalizing,
}

pub(crate) struct WavLoadJob {
    pub(crate) source_id: SourceId,
    pub(crate) root: PathBuf,
    pub(crate) page_size: usize,
}

pub(crate) struct WavLoadResult {
    pub(crate) source_id: SourceId,
    pub(crate) result: Result<Vec<WavEntry>, LoadEntriesError>,
    pub(crate) elapsed: Duration,
    pub(crate) total: usize,
    pub(crate) page_index: usize,
}

pub(crate) struct ScanResult {
    pub(crate) source_id: SourceId,
    pub(crate) mode: ScanMode,
    pub(crate) result: Result<
        crate::sample_sources::scanner::ScanStats,
        crate::sample_sources::scanner::ScanError,
    >,
}

pub(crate) enum ScanJobMessage {
    Progress {
        completed: usize,
        detail: Option<String>,
    },
    Finished(ScanResult),
}

#[derive(Clone)]
pub(crate) struct UpdateCheckResult {
    pub(crate) result: Result<crate::updater::UpdateCheckOutcome, String>,
}

#[derive(Debug)]
pub(crate) enum LoadEntriesError {
    Db(SourceDbError),
    Message(String),
}

impl std::fmt::Display for LoadEntriesError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            LoadEntriesError::Db(err) => write!(f, "{err}"),
            LoadEntriesError::Message(msg) => f.write_str(msg),
        }
    }
}

impl From<String> for LoadEntriesError {
    fn from(value: String) -> Self {
        LoadEntriesError::Message(value)
    }
}
