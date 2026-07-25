use radiant::gui::types::Point;
use radiant::prelude as ui;
use radiant::runtime::NativeFileDrop;
use radiant::widgets::{DragHandleMessage, PointerModifiers};
use std::{path::PathBuf, time::Instant};
use wavecrate::sample_sources::{
    HarvestDerivationOperation, HarvestSeenPersistResult, SampleCollection,
    StarmapLayoutLoadResult, config::AppSettingsCore,
};
use wavecrate::selection::SelectionRange;
use wavecrate_analysis::aspects::SimilarityAspect;

use crate::native_app::app::ExtractedFilePlaybackType;
use crate::native_app::app::{
    ActiveFolderCacheWarmPlanProgress, ActiveFolderCacheWarmPlanResult,
    ActiveFolderCacheWarmProgress, ActiveFolderCacheWarmResult, AppSettingsTab,
    AudioOpenTaskCompletion, AudioOptionsRefreshResult, FileMoveProgress, NormalizationProgress,
    NormalizationResult, PreviewAuditionResult, PreviewAuditionWarmResult,
    SampleLoadPathValidation, SampleLoadResult, SamplePlaybackReady, SourceProcessingHealth,
    SourceProcessingProgress, StarmapViewportChange, WaveformCacheIndicatorRefreshResult,
    WaveformCacheWarmResult,
};
use crate::native_app::audio::playback_history::{
    LastPlayedPersistRequest, LastPlayedPersistResult,
};
use crate::native_app::metadata::{
    MetadataRatingPersistResult, MetadataTagLoadResult, MetadataTagPersistResult,
};
use crate::native_app::sample_library::committed_file_mutations::{
    FileMutationOutcome, FileMutationWork,
};
use crate::native_app::sample_library::context_menu_target::BrowserContextTargetKind;
use crate::native_app::sample_library::folder_browser::commands::FolderBrowserMessage;
use crate::native_app::sample_library::folder_browser::commands::RenameCommitCompletion;
use crate::native_app::sample_library::folder_browser::commands::{
    FileMoveConflictCompletion, FileMoveConflictResolutionRequest, FolderMoveCompletion,
};
use crate::native_app::sample_library::folder_browser::scan::{
    FolderScanDiscoveryBatch, FolderScanProgress, FolderTreeRefreshResult, FolderVerifyResult,
    PreparedFolderScanResult,
};
use crate::native_app::sample_library::folder_scan_actions::FolderScanMaintenanceResult;
use crate::native_app::sample_library::native_file_open_actions::NativeAudioDocumentOpenValidation;
use crate::native_app::sample_library::similarity_scores::SimilarityScoresResult;
use crate::native_app::sample_library::trash_actions::movement::TrashMoveOutcome;
use crate::native_app::waveform::{PlaymarkLabelMessage, WaveformInteraction};
use crate::native_app::waveform::{SimilarSectionsResult, WaveformExtractionCompletion};
use crate::native_app::waveform_edits::WaveformDestructiveEditResult;

#[derive(Clone, Debug, PartialEq)]
pub(in crate::native_app) enum TrashMoveTarget {
    Folder(PathBuf),
    Files(Vec<PathBuf>),
}

#[derive(Clone, Debug, PartialEq)]
pub(in crate::native_app) enum GuiMessage {
    ResizeFolder(DragHandleMessage),
    FolderBrowser(FolderBrowserMessage),
    AddSourceDialogFinished(ui::PlatformResult),
    ContextPathCopyFinished {
        kind: BrowserContextTargetKind,
        path: PathBuf,
        result: ui::PlatformResult,
    },
    TrashFolderDialogFinished(ui::PlatformResult),
    ContextTargetOpenFinished {
        kind: BrowserContextTargetKind,
        path: PathBuf,
        result: ui::PlatformResult,
    },
    FolderScanProgress(FolderScanProgress),
    FolderScanDiscoveryBatch(FolderScanDiscoveryBatch),
    FolderScanFinished(PreparedFolderScanResult),
    #[cfg_attr(test, allow(dead_code))]
    FolderScanMaintenanceFinished(FolderScanMaintenanceResult),
    FolderTreeRefreshFinished(ui::TaskCompletion<FolderTreeRefreshResult>),
    SelectedFolderVerifyFinished(ui::TaskCompletion<FolderVerifyResult>),
    SourceFilesystemChanged {
        source_id: String,
        paths: Vec<PathBuf>,
        overflowed: bool,
        source_root_available: bool,
        /// A durable FSEvents cursor that may advance only after this targeted sync commits.
        journal_checkpoint_event_id: Option<u64>,
    },
    /// The initial watcher stream is live and journal recovery has completed. This boundary
    /// admits the durable lifecycle gate without assuming that every source needs a traversal.
    SourceWatcherReady {
        /// Sources whose earlier unavailable-watcher fallback is still completing. Their
        /// lifecycle probes remain held until the watcher captures a fresh audit barrier.
        deferred_audit_sources: Vec<String>,
    },
    /// Durable closed-application watcher coverage was unavailable for one source. The
    /// supervisor owns the bounded manifest-audit fallback so browser projection remains last-good
    /// until the committed reconciliation delta is ready.
    SourceWatcherJournalGap {
        source_id: String,
        reason: &'static str,
    },
    SourceFilesystemSyncFinished(SourceFilesystemSyncResult),
    CommittedFileMutationRequested(FileMutationWork),
    CommittedFileMutationFinished(FileMutationOutcome),
    SourceManifestAuditCommitted {
        source_id: String,
        lifecycle_generation: u64,
        committed_delta: wavecrate::sample_sources::scanner::CommittedSourceDelta,
    },
    SourceManifestAuditFinished {
        source_id: String,
        lifecycle_generation: u64,
        complete: bool,
    },
    NormalizationProgress(NormalizationProgress),
    NormalizationFinished(NormalizationResult),
    SelectSampleWithModifiers {
        path: String,
        modifiers: PointerModifiers,
    },
    OpenSampleContextMenu {
        path: String,
        position: Point,
    },
    DragSampleFile {
        path: String,
        drag: DragHandleMessage,
    },
    ExternalDragCompleted(Result<radiant::runtime::ExternalDragOutcome, String>),
    ExternalWaveformFileDropFinished {
        source: PathBuf,
        started_at: Instant,
        result: Result<PathBuf, String>,
    },
    NativeAudioDocumentOpenValidated {
        started_at: Instant,
        validation: NativeAudioDocumentOpenValidation,
    },
    DeferredSampleLoad {
        ticket: ui::TaskTicket,
        path: String,
        autoplay: bool,
        check_cache: bool,
        scheduled_at: Instant,
    },
    SettledSamplePromotion {
        ticket: ui::TaskTicket,
        path: String,
        scheduled_at: Instant,
    },
    SampleLoadPathValidated {
        completion: ui::TaskCompletion<SampleLoadPathValidation>,
        started_at: Instant,
    },
    SampleLoadProgress(ui::ResourceKey, ui::TaskTicket, f32),
    SamplePlaybackReady(ui::KeyedTaskCompletion<ui::ResourceKey, SamplePlaybackReady>),
    PreviewAuditionDecoded {
        completion: ui::TaskCompletion<PreviewAuditionResult>,
        started_at: Instant,
    },
    PreviewAuditionWarmFinished {
        completion: ui::TaskCompletion<PreviewAuditionWarmResult>,
        started_at: Instant,
    },
    SampleLoadFinished(ui::KeyedTaskCompletion<ui::ResourceKey, SampleLoadResult>),
    WaveformCacheIndicatorRefreshFinished(ui::TaskCompletion<WaveformCacheIndicatorRefreshResult>),
    WaveformCacheWarmFinished(ui::KeyedTaskCompletion<ui::ResourceKey, WaveformCacheWarmResult>),
    ActiveFolderCacheWarmPlanProgress(ui::TaskCompletion<ActiveFolderCacheWarmPlanProgress>),
    ActiveFolderCacheWarmPlanned(ui::TaskCompletion<ActiveFolderCacheWarmPlanResult>),
    ActiveFolderCacheWarmReady(ui::TaskTicket),
    ActiveFolderCacheWarmProgress(
        ui::KeyedTaskCompletion<ui::ResourceKey, ActiveFolderCacheWarmProgress>,
    ),
    ActiveFolderCacheWarmFinished(
        ui::KeyedTaskCompletion<ui::ResourceKey, ActiveFolderCacheWarmResult>,
    ),
    AudioOptionsRefreshFinished(ui::TaskCompletion<AudioOptionsRefreshResult>),
    AudioOutputPersisted(ui::TaskCompletion<AudioOutputPersistResult>),
    AudioPlayerOpenFinished(AudioOpenTaskCompletion),
    PlaySelectedSample,
    PlayFromCurrentPlayStart,
    PlayRandomSampleRange,
    PlayRandomListedSampleRange,
    PlayPreviousPlaybackHistory,
    PlayNextPlaybackHistory,
    ToggleStickyRandomSampleRangePlayback,
    LastPlayedPersistReady {
        ticket: ui::TaskTicket,
        request: LastPlayedPersistRequest,
    },
    LastPlayedPersisted(LastPlayedPersistResult),
    HarvestSeenPersisted(HarvestSeenPersistResult),
    VolumeSettingsPersisted(VolumeSettingsPersistResult),
    StopPlayback,
    ToggleLoopPlayback,
    SetSimilarityAspectWeightingEnabled(bool),
    SetSimilarityAspectEnabled {
        aspect: SimilarityAspect,
        enabled: bool,
    },
    SetSimilarityAspectWeight {
        aspect: SimilarityAspect,
        weight: f32,
    },
    SimilaritySettingsPersisted(SimilaritySettingsPersistResult),
    StarmapLayoutLoaded(StarmapLayoutLoadResult),
    SimilarityScoresResolved(SimilarityScoresResult),
    SimilarityReadinessAdvanced {
        source_id: String,
        lifecycle_generation: u64,
    },
    SourceProcessingHealth(SourceProcessingHealth),
    SourceProcessingProgress(SourceProcessingProgress),
    Settings(SettingsMessage),
    Metadata(MetadataMessage),
    FocusLoadedFile,
    AdjustSelectedRatingWithoutAdvance(i8),
    AssignSelectedCollection(SampleCollection),
    RemoveContextSampleFromCollection,
    CleanMissingContextSampleFromCollection,
    CleanMissingFilesFromActiveCollection,
    MarkContextSampleHarvestDone,
    MarkContextSampleHarvestIgnored,
    ResetContextSampleHarvest,
    ToggleSelectedHarvestDone,
    ShowContextSampleHarvestOrigin,
    ShowContextSampleHarvestDerivatives,
    OpenContextSampleHarvestDestination,
    ShowSelectedSampleHarvestOrigin,
    ShowSelectedSampleHarvestDerivatives,
    OpenSelectedSampleHarvestDestination,
    NormalizeSelectedSamples,
    CopySelectedFiles,
    CutSelectedFiles,
    PasteCutFiles,
    DuplicateContextSampleSame,
    DuplicateContextSampleDouble,
    ContextSampleSameFinished {
        source_path: PathBuf,
        started_at: Instant,
        result: Result<wavecrate::sample_sources::ContextSampleSameResult, String>,
    },
    ContextSampleDoubleFinished {
        source_path: PathBuf,
        started_at: Instant,
        result: Result<wavecrate::sample_sources::ContextSampleDoubleResult, String>,
    },
    SelectedFilesCopyFinished {
        paths: Vec<PathBuf>,
        count: usize,
        started_at: Instant,
        result: Result<(), String>,
    },
    WaveformSelectionCopyExtracted {
        completion: WaveformExtractionCompletion,
        playback_type: ExtractedFilePlaybackType,
        started_at: Instant,
    },
    WaveformSelectionCopyFinished {
        source_path: PathBuf,
        selection: SelectionRange,
        copied_path: PathBuf,
        playback_type: ExtractedFilePlaybackType,
        source_duration_seconds: f64,
        started_at: Instant,
        result: Result<(), String>,
    },
    FileMoveProgress(FileMoveProgress),
    SetFileMoveConflictApplyToRemaining(bool),
    ResolveFileMoveConflict(FileMoveConflictResolutionRequest),
    FolderMoveFinished {
        started_at: Instant,
        completion: FolderMoveCompletion,
    },
    FileMoveConflictFinished {
        started_at: Instant,
        completion: FileMoveConflictCompletion,
    },
    CancelFileMoveConflicts,
    CopyContextPath,
    OpenContextTarget {
        kind: BrowserContextTargetKind,
        path: PathBuf,
    },
    ContextTargetOpenValidated {
        kind: BrowserContextTargetKind,
        path: PathBuf,
        result: Result<(), String>,
    },
    CreateFolderAtContextTarget,
    RenameContextFolder,
    ContextFolderCreateFinished {
        parent_id: String,
        started_at: Instant,
        result: Result<PathBuf, String>,
    },
    MoveContextTargetToTrash,
    UnlockContextSample,
    ToggleContextFolderLock,
    RequestDeleteContextFolder,
    ConfirmContextFolderDelete,
    CancelContextFolderDelete,
    TrashMoveFinished {
        target: TrashMoveTarget,
        action: &'static str,
        started_at: Instant,
        outcomes: Vec<TrashMoveOutcome>,
    },
    RefreshContextSource,
    ProcessContextSource,
    ToggleContextSourceProtection,
    SetContextSourcePrimary,
    ClearContextSourcePrimary,
    RemoveContextSource,
    CloseContextMenu,
    ToggleJobDetails,
    CloseJobDetails,
    RetryActiveSourceScan,
    CancelActiveSourceScan,
    ReleaseUpdateCheckFinished(
        ui::TaskCompletion<Result<Option<wavecrate::updater::PublicReleaseInfo>, String>>,
    ),
    OpenReleaseDownloadPage,
    ToggleShortcutHelp,
    CloseShortcutHelp,
    ToggleCurationFilterDropdown,
    CloseCurationFilterDropdown,
    ToggleHarvestFilterDropdown,
    CloseHarvestFilterDropdown,
    ToggleZeroCrossingSnap,
    ToggleBpmSnap,
    ToggleBeatGuides,
    SetBeatGuideCount(u8),
    ChangeBeatGuideCountInput(String),
    CommitBeatGuideCountInput(String),
    ToggleMetronome,
    ToggleSimilarSections,
    SimilarSectionsResolved(SimilarSectionsResult),
    UndoTransaction,
    RedoTransaction,
    UndoTransactionsThrough(u64),
    RedoTransactionsThrough(u64),
    ToggleTransactionList,
    CloseTransactionList,
    FocusRenameInput(u64),
    FolderBrowserRenameFinished(RenameCommitCompletion),
    DeleteSelectedItem,
    RequestCropWaveformSelection,
    RequestTrimWaveformSelection,
    RequestReverseWaveformSelection,
    RequestMuteWaveformSelection,
    RequestExtractAndTrimWaveformSelection,
    RequestCropPlaymarkSelection,
    RequestTrimPlaymarkSelection,
    RequestReversePlaymarkSelection,
    RequestExtractAndTrimPlaymarkSelection,
    RequestApplyEditSelectionEffects,
    OpenContextMenu,
    ConfirmPendingWaveformDestructiveEdit,
    CancelPendingWaveformDestructiveEdit,
    AddProtectedExtractionTargetSource,
    ProtectedExtractionTargetSourceDialogFinished(ui::PlatformResult),
    CancelProtectedExtractionTargetSource,
    WaveformDestructiveEditFinished(ui::TaskCompletion<WaveformDestructiveEditResult>),
    ExtractPlaymarkedRange,
    ExtractPlaymarkedRangeToHarvestDestination,
    PlaySelectionExtractionFinished {
        completion: WaveformExtractionCompletion,
        drag_position: Option<Point>,
        playback_type: ExtractedFilePlaybackType,
        harvest_operation: HarvestDerivationOperation,
        focus_derivative: bool,
        started_at: Instant,
    },
    SelectedWholeFilesHarvestExtractionFinished {
        started_at: Instant,
        result: wavecrate::sample_sources::WholeFileHarvestExtractionResult,
    },
    NavigateBrowser {
        delta: i32,
        extend: bool,
        preserve_selection: bool,
    },
    ToggleSelectedSampleAndAdvance,
    SelectAllSamples,
    ToggleRandomNavigationMode,
    ToggleSampleBrowserMapView,
    FocusSelectedStarmapNode,
    ChangeStarmapViewport(StarmapViewportChange),
    BeginStarmapAuditionDrag {
        path: Option<String>,
        position: Point,
        modifiers: PointerModifiers,
    },
    UpdateStarmapAuditionDrag {
        paths: Vec<String>,
        position: Point,
        modifiers: PointerModifiers,
    },
    AdvanceStarmapAudition {
        ticket: ui::TaskTicket,
    },
    PromoteStarmapAudition {
        ticket: ui::TaskTicket,
        path: String,
    },
    FinishStarmapAuditionDrag,
    SampleBrowserWindowChanged(ui::VirtualListWindowChange),
    FolderTreeWindowChanged(ui::VirtualListWindowChange),
    CollapseSelectedFolder,
    CancelBrowserDragOnSampleList,
    DropWaveformSelectionOnSampleList,
    Waveform(WaveformInteraction),
    PlaymarkLabel(PlaymarkLabelMessage),
    WaveformDetailRefined(crate::native_app::waveform::WaveformDetailResult),
    WaveformFileDrop(NativeFileDrop),
    Frame,
}

#[derive(Clone, Debug, PartialEq)]
pub(in crate::native_app) struct SourceFilesystemSyncResult {
    pub(in crate::native_app) source_id: String,
    pub(in crate::native_app) lifecycle_generation: u64,
    pub(in crate::native_app) changed_count: usize,
    pub(in crate::native_app) journal_checkpoint_event_id: Option<u64>,
    pub(in crate::native_app) cancelled: bool,
    pub(in crate::native_app) result: Result<SourceFilesystemSyncSuccess, String>,
}

#[derive(Clone, Debug, PartialEq)]
pub(in crate::native_app) struct SourceFilesystemSyncSuccess {
    pub(in crate::native_app) renames_reconciled: usize,
    pub(in crate::native_app) incomplete_error: Option<String>,
    pub(in crate::native_app) committed_delta:
        wavecrate::sample_sources::scanner::CommittedSourceDelta,
    pub(in crate::native_app) browser_projection_delta: Option<BrowserProjectionDelta>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(in crate::native_app) struct BrowserProjectionDelta {
    pub(in crate::native_app) manifest_revision: u64,
    pub(in crate::native_app) snapshot_revision: u64,
    pub(in crate::native_app) folders: Vec<PathBuf>,
    pub(in crate::native_app) removed_file_ids: Vec<String>,
    pub(in crate::native_app) upserted_files:
        Vec<crate::native_app::sample_library::folder_browser::model::FileEntry>,
}

#[derive(Clone, Debug)]
pub(in crate::native_app) struct VolumeSettingsPersistResult {
    pub(in crate::native_app) persisted: AppSettingsCore,
    pub(in crate::native_app) result: Result<(), String>,
}

#[derive(Clone, Debug)]
pub(in crate::native_app) struct AudioOutputPersistResult {
    pub(in crate::native_app) persisted: AppSettingsCore,
    pub(in crate::native_app) result: Result<(), String>,
}

impl PartialEq for AudioOutputPersistResult {
    fn eq(&self, other: &Self) -> bool {
        self.result == other.result && self.persisted.audio_output == other.persisted.audio_output
    }
}

#[derive(Clone, Debug)]
pub(in crate::native_app) struct SimilaritySettingsPersistResult {
    pub(in crate::native_app) persisted: AppSettingsCore,
    pub(in crate::native_app) result: Result<(), String>,
}

impl PartialEq for SimilaritySettingsPersistResult {
    fn eq(&self, other: &Self) -> bool {
        self.result == other.result && self.persisted.similarity == other.persisted.similarity
    }
}

impl PartialEq for VolumeSettingsPersistResult {
    fn eq(&self, other: &Self) -> bool {
        self.result == other.result
            && self.persisted.volume.to_bits() == other.persisted.volume.to_bits()
    }
}

#[derive(Clone, Debug, PartialEq)]
pub(in crate::native_app) enum MetadataMessage {
    FocusMetadataTagInput,
    MetadataTagInput(radiant::widgets::TextInputMessage),
    CancelMetadataTagEntry,
    MoveMetadataTagCompletion(i32),
    HoverMetadataTagCompletion(String),
    SelectMetadataTagCompletion(String),
    ToggleMetadataTagLibrary,
    ToggleMetadataTagCategory(String),
    SelectMetadataTag(String),
    ToggleMetadataTag(String),
    RemoveMetadataTag(String),
    #[cfg(test)]
    ToggleMetadataTagForFiles {
        tag: String,
        file_ids: Vec<String>,
    },
    DragMetadataTag {
        tag: String,
        drag: DragHandleMessage,
    },
    HoverMetadataTagDropCategory {
        category_id: String,
    },
    ClearMetadataTagDropCategoryUnless {
        category_id: String,
    },
    DropMetadataTagOnCategory {
        category_id: String,
    },
    OpenMetadataTagContextMenu {
        tag: String,
        position: ui::Point,
    },
    DeleteContextMetadataTag,
    DeleteSelectedMetadataTag,
    MetadataRatingPersisted(MetadataRatingPersistResult),
    MetadataTagsPersisted(MetadataTagPersistResult),
    MetadataTagsLoaded(MetadataTagLoadResult),
    ToggleSampleNameViewMode,
}

#[derive(Clone, Debug, PartialEq)]
pub(in crate::native_app) enum SettingsMessage {
    SetVolume(f32),
    SetNormalizedAuditionEnabled(bool),
    ToggleHelpTooltips,
    ToggleAudioSettings,
    OpenGeneralSettings,
    SelectSettingsTab(AppSettingsTab),
    CloseAudioSettings,
    ToggleAudioBackendDropdown,
    ToggleAudioOutputDropdown,
    ToggleAudioSampleRateDropdown,
    CloseAudioSettingsDropdowns,
    SetAudioOutputHost(Option<String>),
    SetAudioOutputDevice(Option<String>),
    SetAudioOutputSampleRate(Option<u32>),
    SetRatingDecayWeeks(u16),
    PickTrashFolder,
    ClearTrashFolder,
    ClearRebuildableCaches,
    GlobalStorageUsageFinished(
        ui::TaskCompletion<Result<wavecrate::app_dirs::GlobalStorageUsage, String>>,
    ),
}
