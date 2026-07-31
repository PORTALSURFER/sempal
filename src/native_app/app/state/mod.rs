mod audio;
mod background;
mod journal;
mod library;
mod metadata;
mod source_refresh;
/// Background scan worker transport and batching for source scans.
mod source_scan_worker;
mod source_scan_workflow;
mod transactions;
mod ui_state;
mod waveform;

#[cfg(test)]
pub(in crate::native_app) const DEFAULT_VOLUME: f32 = 1.0;

pub(in crate::native_app) use audio::{
    AudioAppState, AudioOptionsRefreshResult, CompletedTransientSamplePlayback,
    PendingPlaybackStart, PlaybackSpanRetargetRejection, SamplePlaybackHistory,
    SamplePlaybackIntent, SamplePlaybackNormalization, SamplePlaybackRequest,
    SamplePlaybackSession, SamplePlaybackSessionState, SamplePlaybackSourceProbe,
    SamplePlaybackVisibility,
};
pub(in crate::native_app) use background::{
    AudioOpenCompletion, AudioOpenTaskCompletion, BackgroundTaskState,
    HarvestSelectionDerivationBatchResult, HarvestTouchedPersistAdmission,
    HarvestTouchedPersistBatchResult, RatingPersistBatchResult, WaveformDestructiveEditUiContext,
};
pub(in crate::native_app) use journal::{OperationJournalOwner, OperationJournalStatus};
pub(in crate::native_app) use library::LibraryAppState;
pub(in crate::native_app) use metadata::MetadataAppState;
pub(in crate::native_app) use source_refresh::{
    PendingSourceRefresh, PendingTargetedSourceSync, SourceRefreshCause,
};
pub(in crate::native_app) use source_scan_worker::{FolderScanWorkerEvent, run_folder_scan_worker};
pub(in crate::native_app) use source_scan_workflow::{
    SourceFilesystemChangePlan, SourceRefreshRequest, SourceScanFinish, SourceScanWorkflow,
    SourceSelectionRequest,
};
pub(in crate::native_app) use transactions::{PendingHistoryCommit, TransactionState};
#[cfg(test)]
pub(in crate::native_app) use ui_state::ReleaseUpdateStatus;
pub(in crate::native_app) use ui_state::{
    ChromeUiState, ClipboardHandoffTarget, CutFileClipboard, ExtractedFilePlaybackType,
    MAX_BEAT_GUIDE_COUNT, MIN_BEAT_GUIDE_COUNT, OverflowFadeAnimations, PendingFolderDelete,
    PendingProtectedExtractionAction, PendingProtectedExtractionTargetSource,
    PendingTrashFolderSetup, PendingWaveformDestructiveEdit, SampleBrowserDisplayMode,
    SettingsAppState, StarmapAuditionDragState, StarmapViewport, StarmapViewportChange,
    StartupState, StatusState, UiAppState, UnsupportedFilesDialogState,
    WaveformDestructiveEditKind, WaveformDestructiveEditPrompt, WaveformDestructiveEditTarget,
};
pub(in crate::native_app) use waveform::{
    PendingPlaySelectionRetargetCycle, WaveformAppState, WaveformEditSelectionSnapshot,
    WaveformPlaySelectionSnapshot, WaveformVisualSnapshot,
};

pub(in crate::native_app) struct NativeAppState {
    pub(in crate::native_app) ui: UiAppState,
    pub(in crate::native_app) library: LibraryAppState,
    pub(in crate::native_app) waveform: WaveformAppState,
    pub(in crate::native_app) background: BackgroundTaskState,
    pub(in crate::native_app) audio: AudioAppState,
    pub(in crate::native_app) transactions: TransactionState,
    pub(in crate::native_app) metadata: MetadataAppState,
    pub(in crate::native_app) frame_surface_revision_tracker:
        crate::native_app::audio::playback::FrameSurfaceRevisionTracker,
    pub(in crate::native_app) playhead_frame_diagnostics:
        crate::native_app::audio::playback::PlayheadFrameDiagnosticsState,
    #[cfg(test)]
    pub(in crate::native_app) scheduled_timer_messages: Vec<super::GuiMessage>,
}

#[cfg(test)]
impl NativeAppState {
    pub(in crate::native_app) fn record_scheduled_timer_message(
        &mut self,
        message: super::GuiMessage,
    ) {
        self.scheduled_timer_messages.push(message);
    }

    pub(in crate::native_app) fn take_scheduled_timer_messages(
        &mut self,
    ) -> Vec<super::GuiMessage> {
        std::mem::take(&mut self.scheduled_timer_messages)
    }
}
