use std::time::Instant;

use crate::native_app::{
    app::{NativeAppState, emit_gui_action, logging},
    sample_library::folder_browser::scan::{FolderScanDiscoveryBatch, FolderScanProgress},
};

impl NativeAppState {
    pub(in crate::native_app) fn apply_folder_scan_progress(
        &mut self,
        progress: FolderScanProgress,
    ) {
        let started_at = Instant::now();
        let label = progress.label.clone();
        let phase = progress.lifecycle.label();
        let source_id = progress.source_id.clone();
        let task_id = progress.task_id;
        let lifecycle_generation = progress.lifecycle_generation;
        let retry_count = progress.retry_count;
        let queue_age_ms = progress.queued_at.elapsed().as_millis();
        let last_progress_age_ms = progress.last_progress_at.elapsed().as_millis();
        let state_age_ms = progress.state_changed_at.elapsed().as_millis();
        let current_owner = match &progress.lifecycle {
            crate::native_app::sample_library::folder_browser::scan::FolderScanLifecycle::WaitingForScanCapacity {
                current_owner,
            } => current_owner.clone(),
            _ => None,
        };
        let blocking_subsystem = match &progress.lifecycle {
            crate::native_app::sample_library::folder_browser::scan::FolderScanLifecycle::WaitingForSourceRegistration => Some("source_registration"),
            crate::native_app::sample_library::folder_browser::scan::FolderScanLifecycle::WaitingForScanCapacity { .. } => Some("scan_capacity"),
            _ => None,
        };
        if self.library.apply_folder_scan_progress(progress) {
            self.ui.status.sample = format!("{phase} source {label}");
            tracing::info!(
                target: "wavecrate::source_processing",
                task_id,
                source_id,
                lifecycle_generation = ?lifecycle_generation,
                wait_reason = phase,
                queue_age_ms,
                last_progress_age_ms,
                state_age_ms,
                current_owner = ?current_owner,
                blocking_subsystem,
                retry_count,
                "Source scan lifecycle changed"
            );
            emit_gui_action(
                "folder_browser.scan.progress",
                Some("folder_browser"),
                Some(phase),
                "active",
                started_at,
                None,
            );
        }
    }

    pub(in crate::native_app) fn apply_folder_scan_discovery_batch(
        &mut self,
        batch: FolderScanDiscoveryBatch,
    ) {
        let started_at = Instant::now();
        let count = batch.events.len();
        self.library.apply_folder_scan_discovery_batch(batch);
        self.reapply_desired_rating_overlay();
        if logging::debug_logging_enabled() {
            tracing::debug!(
                target: logging::ACTION_EVENT_TARGET,
                event = "action_detail",
                action = "folder_browser.scan.discovery_batch",
                pane = "folder_browser",
                item_count = count,
                "Folder browser scan discovery batch applied"
            );
        }
        emit_gui_action(
            "folder_browser.scan.discovery_batch",
            Some("folder_browser"),
            None,
            "applied",
            started_at,
            None,
        );
    }
}
