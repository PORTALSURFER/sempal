use crate::native_app::sample_library::folder_browser::scan::{
    FolderScanLifecycle, FolderScanProgress,
};
use crate::native_app::sample_library::folder_browser::{
    FolderBrowserState,
    scan::{FolderScanDiscoveryBatch, FolderScanRequest, FolderScanResult},
};
use crate::native_app::sample_library::similarity_artifacts::SimilarityArtifactRefreshState;
use crate::native_app::sample_library::source_watcher::{
    GuiSourceWatcherHandle, WatcherContinuityProof,
};

use super::{
    PendingSourceRefresh, SourceFilesystemChangePlan, SourceRefreshCause, SourceRefreshRequest,
    SourceScanFinish, SourceScanWorkflow, SourceSelectionRequest,
};

pub(in crate::native_app) struct LibraryAppState {
    pub(in crate::native_app) folder_browser: FolderBrowserState,
    pub(in crate::native_app) similarity_artifacts: SimilarityArtifactRefreshState,
    source_scan: SourceScanWorkflow,
    pub(in crate::native_app) source_watcher: Option<GuiSourceWatcherHandle>,
    pending_audio_document_opens: Vec<std::path::PathBuf>,
}

impl LibraryAppState {
    pub(in crate::native_app) fn new(
        folder_browser: FolderBrowserState,
        source_watcher: Option<GuiSourceWatcherHandle>,
    ) -> Self {
        Self {
            folder_browser,
            similarity_artifacts: SimilarityArtifactRefreshState::default(),
            source_scan: SourceScanWorkflow::new(),
            source_watcher,
            pending_audio_document_opens: Vec::new(),
        }
    }

    pub(in crate::native_app) fn folder_progress(&self) -> Option<&FolderScanProgress> {
        self.source_scan.progress()
    }

    pub(in crate::native_app) fn folder_scan_active(&self) -> bool {
        self.source_scan.active()
    }

    pub(in crate::native_app) fn begin_add_source_path(
        &mut self,
        root: std::path::PathBuf,
        task_id: u64,
    ) -> Option<FolderScanRequest> {
        self.source_scan
            .begin_add_source_path(&mut self.folder_browser, root, task_id)
    }

    pub(in crate::native_app) fn begin_add_source_path_preserving_selection(
        &mut self,
        root: std::path::PathBuf,
        task_id: u64,
    ) -> Option<FolderScanRequest> {
        self.source_scan.begin_add_source_path_preserving_selection(
            &mut self.folder_browser,
            root,
            task_id,
        )
    }

    pub(in crate::native_app) fn begin_select_source(
        &mut self,
        id: String,
        task_id: u64,
    ) -> SourceSelectionRequest {
        self.source_scan
            .begin_select_source(&mut self.folder_browser, id, task_id)
    }

    pub(in crate::native_app) fn begin_source_scan(
        &mut self,
        id: String,
        task_id: u64,
    ) -> Option<FolderScanRequest> {
        self.source_scan
            .begin_source_scan(&mut self.folder_browser, id, task_id)
    }

    pub(in crate::native_app) fn begin_selected_source_scan(
        &mut self,
        task_id: u64,
    ) -> Option<FolderScanRequest> {
        self.source_scan
            .begin_selected_source_scan(&mut self.folder_browser, task_id)
    }

    pub(in crate::native_app) fn start_folder_scan(&mut self, request: &FolderScanRequest) {
        self.source_scan.start_scan(request);
    }

    pub(in crate::native_app) fn apply_folder_scan_progress(
        &mut self,
        progress: FolderScanProgress,
    ) -> bool {
        self.source_scan
            .apply_progress(&self.folder_browser, progress)
    }

    pub(in crate::native_app) fn transition_folder_scan(
        &mut self,
        task_id: u64,
        source_id: &str,
        lifecycle_generation: Option<u64>,
        lifecycle: FolderScanLifecycle,
        detail: impl Into<String>,
    ) -> bool {
        self.source_scan.transition_current_scan(
            task_id,
            source_id,
            lifecycle_generation,
            lifecycle,
            detail,
        )
    }

    pub(in crate::native_app) fn finish_folder_scan_terminal(
        &mut self,
        task_id: u64,
        source_id: &str,
        lifecycle_generation: Option<u64>,
        lifecycle: FolderScanLifecycle,
    ) -> Option<FolderScanProgress> {
        self.source_scan.finish_current_scan_terminal(
            task_id,
            source_id,
            lifecycle_generation,
            lifecycle,
        )
    }

    pub(in crate::native_app) fn resume_folder_scan_progress_after_projection(
        &mut self,
        progress: FolderScanProgress,
    ) -> bool {
        self.source_scan.resume_progress_after_projection(progress)
    }

    pub(in crate::native_app) fn apply_folder_scan_discovery_batch(
        &mut self,
        batch: FolderScanDiscoveryBatch,
    ) -> bool {
        self.source_scan
            .apply_discovery_batch(&mut self.folder_browser, batch)
    }

    pub(in crate::native_app) fn plan_filesystem_change(
        &mut self,
        source_id: String,
        paths: &[std::path::PathBuf],
        overflowed: bool,
        source_root_available: bool,
        lifecycle_generation: Option<u64>,
        journal_checkpoint_event_id: Option<u64>,
        watcher_continuity_proof: Option<WatcherContinuityProof>,
    ) -> SourceFilesystemChangePlan {
        self.source_scan.plan_filesystem_change_for_generation(
            &mut self.folder_browser,
            source_id,
            paths,
            overflowed,
            source_root_available,
            lifecycle_generation,
            journal_checkpoint_event_id,
            watcher_continuity_proof,
        )
    }

    pub(in crate::native_app) fn next_pending_source_refresh_if_idle(
        &mut self,
    ) -> Option<PendingSourceRefresh> {
        self.source_scan.next_pending_refresh_context_if_idle()
    }

    pub(in crate::native_app) fn next_pending_targeted_source_sync(
        &mut self,
    ) -> Option<super::PendingTargetedSourceSync> {
        self.source_scan.next_pending_targeted_sync()
    }

    pub(in crate::native_app) fn mark_targeted_source_sync_started(
        &mut self,
        source_id: &str,
        lifecycle_generation: u64,
    ) -> bool {
        self.source_scan
            .mark_targeted_sync_started(source_id, lifecycle_generation)
    }

    pub(in crate::native_app) fn mark_targeted_source_sync_finished(
        &mut self,
        source_id: &str,
        lifecycle_generation: u64,
    ) {
        self.source_scan
            .mark_targeted_sync_finished(source_id, lifecycle_generation);
    }

    #[cfg(test)]
    pub(in crate::native_app) fn targeted_source_sync_active_for_tests(
        &self,
        source_id: &str,
    ) -> bool {
        self.source_scan.targeted_sync_active_for_tests(source_id)
    }

    pub(in crate::native_app) fn retire_source_workflow(&mut self, source_id: &str) -> bool {
        self.source_scan.retire_source(source_id)
    }

    pub(in crate::native_app) fn cancel_active_folder_scan_by_user(
        &mut self,
    ) -> Option<(String, String)> {
        self.source_scan
            .cancel_active_scan_by_user(&mut self.folder_browser)
    }

    pub(in crate::native_app) fn fail_active_folder_scan(
        &mut self,
        task_id: u64,
        source_id: &str,
        lifecycle_generation: Option<u64>,
    ) -> Option<FolderScanProgress> {
        self.source_scan.fail_active_scan(
            &mut self.folder_browser,
            task_id,
            source_id,
            lifecycle_generation,
        )
    }

    pub(in crate::native_app) fn begin_filesystem_refresh(
        &mut self,
        source_id: String,
        task_id: u64,
        cause: SourceRefreshCause,
        lifecycle_generation: Option<u64>,
    ) -> SourceRefreshRequest {
        self.source_scan.begin_filesystem_refresh_with_context(
            &mut self.folder_browser,
            source_id,
            task_id,
            cause,
            lifecycle_generation,
        )
    }

    pub(in crate::native_app) fn finish_folder_scan(
        &mut self,
        result: FolderScanResult,
        lifecycle_generation: Option<u64>,
        lifecycle_is_current: bool,
    ) -> SourceScanFinish {
        self.source_scan.finish_scan_with_lifecycle(
            &mut self.folder_browser,
            result,
            lifecycle_generation,
            lifecycle_is_current,
        )
    }

    pub(in crate::native_app) fn queue_pending_audio_document_open(
        &mut self,
        path: std::path::PathBuf,
    ) {
        if !self.pending_audio_document_opens.contains(&path) {
            self.pending_audio_document_opens.push(path);
        }
    }

    pub(in crate::native_app) fn take_pending_audio_document_opens(
        &mut self,
    ) -> Vec<std::path::PathBuf> {
        std::mem::take(&mut self.pending_audio_document_opens)
    }

    pub(in crate::native_app) fn restore_pending_audio_document_opens(
        &mut self,
        paths: Vec<std::path::PathBuf>,
    ) {
        self.pending_audio_document_opens = paths;
    }

    #[cfg(test)]
    pub(in crate::native_app) fn pending_audio_document_open_count_for_tests(&self) -> usize {
        self.pending_audio_document_opens.len()
    }

    #[cfg(test)]
    pub(in crate::native_app) fn set_folder_progress_for_tests(
        &mut self,
        progress: FolderScanProgress,
    ) {
        self.source_scan.set_progress_for_tests(progress);
    }

    #[cfg(test)]
    pub(in crate::native_app) fn pending_source_refresh_contains_for_tests(
        &self,
        source_id: &str,
    ) -> bool {
        self.source_scan
            .pending_refresh_contains_for_tests(source_id)
    }
}
