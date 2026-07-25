mod cache;
mod drop;
mod loading;
mod message;
mod progress;
mod settings;
mod source_processing_events;
mod state;

pub(in crate::native_app) use cache::{
    ActiveFolderCacheWarmPlanProgress, ActiveFolderCacheWarmPlanResult,
    ActiveFolderCacheWarmProgress, ActiveFolderCacheWarmRequest, ActiveFolderCacheWarmResult,
    ActiveFolderCacheWarmStage, WaveformCacheEntry, WaveformCacheIndicatorRefreshResult,
    WaveformCacheWarmResult,
};
pub(in crate::native_app) use drop::NativeFileDropHover;
pub(in crate::native_app) use loading::{
    PreviewAuditionResult, PreviewAuditionWarmResult, SampleLoadResult, SampleLoadTaskCompletion,
    SamplePlaybackReady, SampleSelectionLoadState,
};
pub(in crate::native_app) use message::{
    AudioOutputPersistResult, BrowserProjectionDelta, GuiMessage, MetadataMessage, SettingsMessage,
    SimilaritySettingsPersistResult, SourceFilesystemSyncResult, SourceFilesystemSyncSuccess,
    TrashMoveTarget, VolumeSettingsPersistResult,
};
pub(in crate::native_app) use progress::{
    FileMoveProgress, NormalizationFailure, NormalizationHarvestDerivation, NormalizationProgress,
    NormalizationQueueItem, NormalizationResult, SourceProcessingHealth,
    SourceProcessingHealthStatus, SourceProcessingProgress,
};
pub(in crate::native_app) use settings::{
    AppSettingsTab, AudioSettingsDropdown, GlobalStorageUsageState, SampleNameViewMode,
};
pub(in crate::native_app) use source_processing_events::GuiSourceProcessingEventSink;
#[cfg(test)]
pub(in crate::native_app) use state::DEFAULT_VOLUME;
#[cfg(test)]
pub(in crate::native_app) use state::ReleaseUpdateStatus;
pub(in crate::native_app) use state::{
    AudioAppState, AudioOpenCompletion, AudioOpenTaskCompletion, AudioOptionsRefreshResult,
    BackgroundTaskState, ChromeUiState, ClipboardHandoffTarget, CompletedTransientSamplePlayback,
    CutFileClipboard, ExtractedFilePlaybackType, FolderScanWorkerEvent, LibraryAppState,
    MAX_BEAT_GUIDE_COUNT, MIN_BEAT_GUIDE_COUNT, MetadataAppState, NativeAppState,
    PendingFolderDelete, PendingPlaySelectionRetargetCycle, PendingPlaybackStart,
    PendingProtectedExtractionAction, PendingProtectedExtractionTargetSource,
    PendingWaveformDestructiveEdit, PlaybackSpanRetargetRejection, SampleBrowserDisplayMode,
    SamplePlaybackHistory, SamplePlaybackIntent, SamplePlaybackNormalization,
    SamplePlaybackRequest, SamplePlaybackSession, SamplePlaybackSessionState,
    SamplePlaybackSourceProbe, SamplePlaybackVisibility, SettingsAppState,
    SourceFilesystemChangePlan, SourceRefreshCause, SourceRefreshRequest, SourceScanFinish,
    SourceSelectionRequest, StarmapAuditionDragState, StarmapViewport, StarmapViewportChange,
    StartupState, StatusState, UiAppState, WaveformAppState, WaveformDestructiveEditKind,
    WaveformDestructiveEditPrompt, WaveformDestructiveEditTarget, WaveformDestructiveEditUiContext,
    WaveformEditSelectionSnapshot, WaveformPlaySelectionSnapshot, WaveformVisualSnapshot,
    run_folder_scan_worker,
};

pub(super) use crate::native_app::app_chrome::scene::view;
pub(super) use crate::native_app::audio::sample_load_actions::{
    NormalizedWaveformReload, SampleLoadPathValidation, WaveformPlaybackResume,
};
pub(super) use crate::native_app::sample_library::file_actions::sample_path_label;
pub(super) use crate::native_app::sample_library::folder_browser::FolderBrowserState;
pub(super) use crate::native_app::sample_library::folder_browser::commands::{
    FileMoveConflictResolution, FileMoveConflictResolutionRequest, FolderBrowserMessage,
};
pub(super) use crate::native_app::sample_library::folder_browser::scan::FolderScanProgress;
pub(super) use crate::native_app::shell::emit_gui_action;
#[cfg(test)]
pub(super) use crate::native_app::shell::shortcuts::shortcut_help_bindings;
pub(super) use crate::native_app::shell::shortcuts::{
    ShortcutHelpItem, ShortcutHelpSection, default_gui_shortcuts, shortcut_help_sections,
};
pub(super) use crate::native_app::ui::display::format_sample_rate_label;
pub(super) use crate::native_app::waveform::{
    WaveformActiveDragKind, WaveformContextMenu, WaveformInteraction, WaveformSelectionKind,
    WaveformState,
};
pub(super) use wavecrate::logging;
