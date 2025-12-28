//! Runtime state and job coordination for the controller.

use super::super::{ScanMode, SourceDbError, SourceId, WavEntry, analysis_jobs, jobs};
use std::path::PathBuf;
use std::time::{Duration, Instant};

pub(in crate::egui_app::controller) struct ControllerRuntimeState {
    pub(in crate::egui_app::controller) jobs: jobs::ControllerJobs,
    pub(in crate::egui_app::controller) analysis: analysis_jobs::AnalysisWorkerPool,
    pub(in crate::egui_app::controller) performance: PerformanceGovernorState,
    pub(in crate::egui_app::controller) similarity_prep: Option<SimilarityPrepState>,
    pub(in crate::egui_app::controller) similarity_prep_last_error: Option<String>,
    pub(in crate::egui_app::controller) similarity_prep_force_full_analysis_next: bool,
    #[cfg(test)]
    pub(in crate::egui_app::controller) progress_cancel_after: Option<usize>,
}

impl ControllerRuntimeState {
    pub(in crate::egui_app::controller) fn new(
        jobs: jobs::ControllerJobs,
        analysis: analysis_jobs::AnalysisWorkerPool,
    ) -> Self {
        Self {
            jobs,
            analysis,
            performance: PerformanceGovernorState::new(),
            similarity_prep: None,
            similarity_prep_last_error: None,
            similarity_prep_force_full_analysis_next: false,
            #[cfg(test)]
            progress_cancel_after: None,
        }
    }
}

pub(in crate::egui_app::controller) struct PerformanceGovernorState {
    pub(in crate::egui_app::controller) last_user_activity_at: Option<Instant>,
    pub(in crate::egui_app::controller) last_slow_frame_at: Option<Instant>,
    pub(in crate::egui_app::controller) last_frame_at: Option<Instant>,
    pub(in crate::egui_app::controller) last_worker_count: Option<u32>,
    pub(in crate::egui_app::controller) idle_worker_override: Option<u32>,
}

impl PerformanceGovernorState {
    pub(in crate::egui_app::controller) fn new() -> Self {
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
pub(in crate::egui_app::controller) struct SimilarityPrepState {
    pub(in crate::egui_app::controller) source_id: SourceId,
    pub(in crate::egui_app::controller) stage: SimilarityPrepStage,
    pub(in crate::egui_app::controller) umap_version: String,
    pub(in crate::egui_app::controller) scan_completed_at: Option<i64>,
    pub(in crate::egui_app::controller) skip_backfill: bool,
    pub(in crate::egui_app::controller) force_full_analysis: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(in crate::egui_app::controller) enum SimilarityPrepStage {
    AwaitScan,
    AwaitEmbeddings,
    Finalizing,
}

pub(in crate::egui_app::controller) struct WavLoadJob {
    pub(in crate::egui_app::controller) source_id: SourceId,
    pub(in crate::egui_app::controller) root: PathBuf,
    pub(in crate::egui_app::controller) page_size: usize,
}

pub(in crate::egui_app::controller) struct WavLoadResult {
    pub(in crate::egui_app::controller) source_id: SourceId,
    pub(in crate::egui_app::controller) result: Result<Vec<WavEntry>, LoadEntriesError>,
    pub(in crate::egui_app::controller) elapsed: Duration,
    pub(in crate::egui_app::controller) total: usize,
    pub(in crate::egui_app::controller) page_index: usize,
}

pub(in crate::egui_app::controller) struct ScanResult {
    pub(in crate::egui_app::controller) source_id: SourceId,
    pub(in crate::egui_app::controller) mode: ScanMode,
    pub(in crate::egui_app::controller) result: Result<
        crate::sample_sources::scanner::ScanStats,
        crate::sample_sources::scanner::ScanError,
    >,
}

pub(in crate::egui_app::controller) enum ScanJobMessage {
    Progress {
        completed: usize,
        detail: Option<String>,
    },
    Finished(ScanResult),
}

#[derive(Clone)]
pub(in crate::egui_app::controller) struct UpdateCheckResult {
    pub(in crate::egui_app::controller) result: Result<crate::updater::UpdateCheckOutcome, String>,
}

#[derive(Debug)]
pub(in crate::egui_app::controller) enum LoadEntriesError {
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
