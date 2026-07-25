use crate::native_app::app::{
    FileMoveProgress, FolderScanProgress, NativeAppState, NormalizationProgress,
    SourceProcessingHealth, SourceProcessingHealthStatus,
};
use std::time::Instant;

#[derive(Clone, Debug, PartialEq)]
pub(in crate::native_app) struct StatusBarViewModel {
    pub(in crate::native_app) selected_sample_count: usize,
    pub(in crate::native_app) listed_audio_count: usize,
    pub(in crate::native_app) listing_includes_subfolders: bool,
    pub(in crate::native_app) status_text: String,
    pub(in crate::native_app) status_severity: StatusSeverity,
    pub(in crate::native_app) worker_progress: Option<WorkerProgressViewModel>,
    pub(in crate::native_app) job_details: Option<JobDetailsViewModel>,
    pub(in crate::native_app) progress_tick: f32,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(in crate::native_app) enum StatusSeverity {
    Normal,
    Warning,
}

impl StatusBarViewModel {
    pub(in crate::native_app) fn from_app_state(state: &NativeAppState) -> Self {
        let worker = active_worker(state);
        Self {
            selected_sample_count: state.library.folder_browser.selected_audio_file_count(),
            listed_audio_count: state
                .library
                .folder_browser
                .selected_audio_file_count_matching_tags(&state.metadata.tags_by_file),
            listing_includes_subfolders: state
                .library
                .folder_browser
                .folder_subtree_listing_enabled(),
            status_text: bottom_status_text(state),
            status_severity: bottom_status_severity(state, worker.is_some()),
            worker_progress: worker.as_ref().map(|worker| worker.progress),
            job_details: worker.map(|worker| worker.details),
            progress_tick: state.background.progress_tick,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub(in crate::native_app) struct WorkerProgressViewModel {
    pub(in crate::native_app) completed: usize,
    pub(in crate::native_app) total: usize,
    pub(in crate::native_app) current_fraction: Option<f32>,
    pub(in crate::native_app) active_animation: bool,
    pub(in crate::native_app) compact_activity: bool,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(in crate::native_app) struct JobDetailsViewModel {
    pub(in crate::native_app) rows: [String; 4],
    pub(in crate::native_app) source_scan_recovery: bool,
}

struct ActiveWorkerViewModel {
    progress: WorkerProgressViewModel,
    details: JobDetailsViewModel,
}

fn bottom_status_text(state: &NativeAppState) -> String {
    let status = state.ui.status.sample.clone();
    let status = match state.library.folder_browser.selected_source_status_label() {
        Some(source_status) if source_status.starts_with(&status) => source_status,
        Some(source_status) if !status.starts_with(&source_status) => {
            format!("{source_status} | {status}")
        }
        _ => status,
    };
    match selected_source_health(state).and_then(source_health_status_text) {
        Some(health) if !status.contains(&health) => format!("{status} | {health}"),
        _ => status,
    }
}

fn bottom_status_severity(state: &NativeAppState, worker_active: bool) -> StatusSeverity {
    if !worker_active
        && (state
            .library
            .folder_browser
            .source_is_missing(state.library.folder_browser.selected_source_id())
            || selected_source_health(state).is_some_and(|health| {
                matches!(
                    health.status,
                    SourceProcessingHealthStatus::BlockedByPrerequisites
                        | SourceProcessingHealthStatus::Offline
                        | SourceProcessingHealthStatus::DegradedTerminal
                        | SourceProcessingHealthStatus::ReconciliationFailed
                )
            }))
    {
        StatusSeverity::Warning
    } else {
        StatusSeverity::Normal
    }
}

fn selected_source_health(state: &NativeAppState) -> Option<&SourceProcessingHealth> {
    state
        .background
        .source_processing_health
        .get(state.library.folder_browser.selected_source_id())
}

fn source_health_status_text(health: &SourceProcessingHealth) -> Option<String> {
    let (permanent, unsupported, stale, deleted) = health.stage_counts.values().fold(
        (0_usize, 0_usize, 0_usize, 0_usize),
        |totals, counts| {
            (
                totals.0.saturating_add(counts.permanent),
                totals.1.saturating_add(counts.unsupported),
                totals.2.saturating_add(counts.stale),
                totals.3.saturating_add(counts.deleted),
            )
        },
    );
    match health.status {
        SourceProcessingHealthStatus::Ready => None,
        SourceProcessingHealthStatus::Processing => Some(String::from("Source processing")),
        SourceProcessingHealthStatus::WaitingForRetry => {
            Some(String::from("Source waiting to retry"))
        }
        SourceProcessingHealthStatus::BlockedByPrerequisites => {
            Some(String::from("Source readiness blocked by prerequisites"))
        }
        SourceProcessingHealthStatus::Offline => Some(String::from("Source offline")),
        SourceProcessingHealthStatus::Disabled => Some(String::from("Source disabled")),
        SourceProcessingHealthStatus::DegradedTerminal => Some(format!(
            "Source usable with limits: {unsupported} unsupported, {permanent} failed, {stale} stale, {deleted} deleted"
        )),
        SourceProcessingHealthStatus::ReconciliationFailed => {
            Some(String::from("Source readiness needs repair"))
        }
    }
}

fn active_worker(state: &NativeAppState) -> Option<ActiveWorkerViewModel> {
    if let Some(progress) = state.library.folder_progress() {
        return Some(ActiveWorkerViewModel {
            progress: WorkerProgressViewModel::from_folder_progress(progress),
            details: JobDetailsViewModel::from_folder_progress(progress),
        });
    }
    if let Some(progress) = state.background.normalization_progress.as_ref() {
        return Some(ActiveWorkerViewModel {
            progress: WorkerProgressViewModel::from_normalization_progress(progress),
            details: JobDetailsViewModel::from_normalization_progress(progress),
        });
    }
    if let Some(progress) = state.background.file_move_progress.as_ref() {
        return Some(ActiveWorkerViewModel {
            progress: WorkerProgressViewModel::from_file_move_progress(progress),
            details: JobDetailsViewModel::from_file_move_progress(progress),
        });
    }
    if let Some(progress) = WorkerProgressViewModel::from_source_cache_warm(state) {
        return Some(ActiveWorkerViewModel {
            progress,
            details: JobDetailsViewModel::from_source_cache_warm(state),
        });
    }
    state
        .background
        .source_processing_progress
        .as_ref()
        .map(|source_progress| ActiveWorkerViewModel {
            progress: WorkerProgressViewModel::from_source_processing(source_progress),
            details: JobDetailsViewModel::from_source_processing(state, source_progress),
        })
}

impl WorkerProgressViewModel {
    pub(in crate::native_app) fn from_folder_progress(progress: &FolderScanProgress) -> Self {
        Self {
            completed: progress.completed,
            total: progress.total,
            current_fraction: None,
            active_animation: false,
            compact_activity: progress.lifecycle.is_waiting(),
        }
    }

    fn from_normalization_progress(progress: &NormalizationProgress) -> Self {
        Self {
            completed: progress.work_completed,
            total: progress.work_total,
            current_fraction: None,
            active_animation: false,
            compact_activity: false,
        }
    }

    fn from_file_move_progress(progress: &FileMoveProgress) -> Self {
        Self {
            completed: progress.completed,
            total: progress.total,
            current_fraction: None,
            active_animation: false,
            compact_activity: false,
        }
    }

    fn from_source_cache_warm(state: &NativeAppState) -> Option<Self> {
        let cache = &state.waveform.cache;
        (cache.active_folder_warm_folder_id.is_some() && cache.active_folder_warm_total > 0)
            .then_some(Self {
                completed: cache.active_folder_warm_completed,
                total: cache.active_folder_warm_total,
                current_fraction: cache
                    .active_folder_warm_current
                    .as_ref()
                    .map(|_| cache.active_folder_warm_current_progress.clamp(0.0, 1.0)),
                active_animation: true,
                compact_activity: false,
            })
    }

    fn from_source_processing(progress: &crate::native_app::app::SourceProcessingProgress) -> Self {
        Self {
            completed: progress.completed,
            total: progress.total,
            current_fraction: None,
            // Source processing has no independently measurable current-item fraction. Use one
            // determinate track when totals are known and a compact activity indicator when they
            // are not, so discovery cannot look like a frozen determinate bar.
            active_animation: false,
            compact_activity: true,
        }
    }
}

impl JobDetailsViewModel {
    pub(in crate::native_app) fn from_folder_progress(progress: &FolderScanProgress) -> Self {
        Self::from_folder_progress_at(progress, Instant::now())
    }

    pub(in crate::native_app) fn from_folder_progress_at(
        progress: &FolderScanProgress,
        now: Instant,
    ) -> Self {
        let queue_age = progress.queue_age_at(now).as_secs();
        let last_progress_age = progress.last_progress_age_at(now).as_secs();
        let taking_longer = progress.taking_longer_than_expected_at(now);
        let progress_label = if taking_longer {
            format!("Taking longer than expected — {last_progress_age}s without progress")
        } else {
            match &progress.lifecycle {
                crate::native_app::sample_library::folder_browser::scan::FolderScanLifecycle::Queued => {
                    format!("Queued — {queue_age}s")
                }
                crate::native_app::sample_library::folder_browser::scan::FolderScanLifecycle::WaitingForSourceRegistration => {
                    format!("Waiting for source update — {queue_age}s")
                }
                crate::native_app::sample_library::folder_browser::scan::FolderScanLifecycle::WaitingForScanCapacity { .. } => {
                    format!("Queued — another source is being reconciled — {queue_age}s")
                }
                crate::native_app::sample_library::folder_browser::scan::FolderScanLifecycle::RetryScheduled => {
                    format!("Retry scheduled — attempt {}", progress.retry_count.saturating_add(1))
                }
                _ if progress.total == 0 => {
                    format!("{} — {} found", progress.lifecycle.label(), progress.completed)
                }
                _ => format!(
                    "{} — {}/{}",
                    progress.lifecycle.label(),
                    progress.completed.min(progress.total),
                    progress.total
                ),
            }
        };
        let current = if taking_longer {
            format!(
                "{} | Queue age: {queue_age}s | Last progress: {last_progress_age}s | Retry or cancel safely",
                progress.detail
            )
        } else if progress.detail.is_empty() {
            format!("{} | Queue age: {queue_age}s", progress.lifecycle.label())
        } else {
            format!("{} | Queue age: {queue_age}s", progress.detail)
        };
        Self {
            rows: [
                String::from("Type: Source scan"),
                format!("Source: {}", progress.label),
                format!("Progress: {progress_label}"),
                format!("Current: {current}"),
            ],
            source_scan_recovery: taking_longer,
        }
    }

    fn from_normalization_progress(progress: &NormalizationProgress) -> Self {
        let detail = if progress.queued == 0 {
            progress.detail.clone()
        } else {
            format!("{} | {} queued", progress.detail, progress.queued)
        };
        Self {
            rows: job_rows(
                "Normalization",
                progress.label.as_str(),
                progress.work_completed,
                progress.work_total,
                detail.as_str(),
                "processed",
            ),
            source_scan_recovery: false,
        }
    }

    fn from_file_move_progress(progress: &FileMoveProgress) -> Self {
        Self {
            rows: job_rows(
                "File operation",
                progress.label.as_str(),
                progress.completed,
                progress.total,
                progress.detail.as_str(),
                "processed",
            ),
            source_scan_recovery: false,
        }
    }

    fn from_source_cache_warm(state: &NativeAppState) -> Self {
        let cache = &state.waveform.cache;
        let checking = cache.active_folder_warm_plan_task.active().is_some();
        let path = cache
            .active_folder_warm_current
            .as_ref()
            .map(|path| path.to_string_lossy().to_string())
            .unwrap_or_default();
        let stage = cache.active_folder_warm_current_stage.map(|stage| {
            let percent = (cache.active_folder_warm_current_progress.clamp(0.0, 1.0) * 100.0)
                .round() as usize;
            format!("{} {percent}%", stage.label())
        });
        let detail = match (stage, path.is_empty()) {
            (Some(stage), false) => format!("{stage} | {path}"),
            (Some(stage), true) => stage,
            (None, false) => path,
            (None, true) => String::new(),
        };
        Self {
            rows: job_rows(
                if checking {
                    "Cache check"
                } else {
                    "Waveform cache"
                },
                selected_source_label(state).as_str(),
                cache.active_folder_warm_completed,
                cache.active_folder_warm_total,
                detail.as_str(),
                if checking { "checked" } else { "cached" },
            ),
            source_scan_recovery: false,
        }
    }

    fn from_source_processing(
        state: &NativeAppState,
        progress: &crate::native_app::app::SourceProcessingProgress,
    ) -> Self {
        let current = if progress.detail.is_empty() {
            progress.stage.clone()
        } else {
            format!("{} | {}", progress.stage, progress.detail)
        };
        debug_assert!(
            !progress.active || !progress.source_id.is_empty(),
            "active source processing feedback must identify exactly one source"
        );
        let source = state
            .library
            .folder_browser
            .source_label(progress.source_id.as_str())
            .unwrap_or(progress.source_id.as_str());
        let progress_label = if progress.total == 0 {
            String::from("Active (total not available)")
        } else {
            let counts = format!(
                "{}/{}",
                progress.completed.min(progress.total),
                progress.total
            );
            match source_processing_progress_unit(progress.stage.as_str()) {
                Some(unit) => format!("{counts} {unit}"),
                None => counts,
            }
        };
        Self {
            rows: [
                String::from("Type: Source processing"),
                format!("Source: {source}"),
                format!("Progress: {progress_label}"),
                format!("Current: {current}"),
            ],
            source_scan_recovery: false,
        }
    }
}

fn source_processing_progress_unit(stage: &str) -> Option<&'static str> {
    match stage {
        "Inspecting source manifest" => Some("files inspected"),
        "Preparing readiness targets" => Some("files prepared"),
        "Comparing source readiness" | "Comparing changed readiness" => {
            Some("readiness targets compared")
        }
        "Queueing unfinished work" => Some("readiness targets checked"),
        _ => None,
    }
}

fn selected_source_label(state: &NativeAppState) -> String {
    let source_id = state.library.folder_browser.selected_source_id();
    state
        .library
        .folder_browser
        .source_label(source_id)
        .unwrap_or(source_id)
        .to_string()
}

fn job_rows(
    kind: &str,
    source: &str,
    completed: usize,
    total: usize,
    detail: &str,
    indeterminate_suffix: &str,
) -> [String; 4] {
    let progress = if total == 0 {
        format!("{completed} {indeterminate_suffix}")
    } else {
        format!("{}/{}", completed.min(total), total)
    };
    let detail = if detail.is_empty() {
        "Waiting for next item"
    } else {
        detail
    };
    [
        format!("Type: {kind}"),
        format!("Source: {source}"),
        format!("Progress: {progress}"),
        format!("Current: {detail}"),
    ]
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::native_app::sample_library::folder_browser::scan::{
        FolderScanLifecycle, FolderScanProgress, SOURCE_SCAN_LONG_WAIT_THRESHOLD,
    };
    use std::time::Duration;

    fn progress(lifecycle: FolderScanLifecycle) -> FolderScanProgress {
        FolderScanProgress::new(
            7,
            String::from("source-id"),
            String::from("Samples"),
            lifecycle,
            2,
            5,
            String::from("Working"),
        )
    }

    #[test]
    fn every_source_scan_lifecycle_has_typed_user_facing_copy() {
        let states = [
            (FolderScanLifecycle::Queued, "Queued"),
            (
                FolderScanLifecycle::WaitingForSourceRegistration,
                "Waiting for source update",
            ),
            (
                FolderScanLifecycle::WaitingForScanCapacity {
                    current_owner: Some(String::from("other-source")),
                },
                "another source is being reconciled",
            ),
            (FolderScanLifecycle::Scanning, "Scanning"),
            (FolderScanLifecycle::ApplyingResults, "Applying results"),
            (FolderScanLifecycle::PersistingResults, "Saving results"),
            (FolderScanLifecycle::RetryScheduled, "Retry scheduled"),
            (FolderScanLifecycle::Canceled, "Canceled"),
            (FolderScanLifecycle::Failed, "Failed"),
            (
                FolderScanLifecycle::CompleteWithWarnings,
                "Complete with warnings",
            ),
            (FolderScanLifecycle::Complete, "Complete"),
        ];

        for (state, expected) in states {
            let progress = progress(state.clone());
            let projection =
                JobDetailsViewModel::from_folder_progress_at(&progress, progress.queued_at);
            assert_eq!(projection.rows[0], "Type: Source scan");
            assert_eq!(projection.rows[1], "Source: Samples");
            assert!(
                projection.rows[2].contains(expected),
                "missing lifecycle copy for {state:?}: {:?}",
                projection.rows
            );
        }
    }

    #[test]
    fn fake_clock_exposes_queue_and_last_progress_age_then_offers_recovery() {
        let mut progress = progress(FolderScanLifecycle::WaitingForScanCapacity {
            current_owner: Some(String::from("other-source")),
        });
        let now = progress.queued_at + SOURCE_SCAN_LONG_WAIT_THRESHOLD + Duration::from_secs(2);
        progress.last_progress_at = progress.queued_at;

        let projection = JobDetailsViewModel::from_folder_progress_at(&progress, now);

        assert!(projection.source_scan_recovery);
        assert!(projection.rows[2].contains("Taking longer than expected"));
        assert!(projection.rows[2].contains("32s without progress"));
        assert!(projection.rows[3].contains("Queue age: 32s"));
        assert!(projection.rows[3].contains("Retry or cancel safely"));
    }

    #[test]
    fn long_queued_work_with_recent_progress_is_not_marked_stalled() {
        let mut progress = progress(FolderScanLifecycle::Scanning);
        let now = progress.queued_at + Duration::from_secs(90);
        progress.last_progress_at = now - Duration::from_secs(2);

        let projection = JobDetailsViewModel::from_folder_progress_at(&progress, now);

        assert!(!projection.source_scan_recovery);
        assert!(!projection.rows[2].contains("Taking longer than expected"));
        assert!(projection.rows[3].contains("Queue age: 90s"));
    }
}
