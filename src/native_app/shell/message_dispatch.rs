mod browser;
mod chrome;
mod files;
mod frame;
mod metadata;
mod navigation;
mod playback;
mod sample_loading;
mod settings;
#[cfg(test)]
mod tests;
pub(in crate::native_app) mod waveform;

use radiant::prelude as ui;
use std::time::{Duration, Instant};

use crate::native_app::app::{GuiMessage, NativeAppState, WaveformInteraction, sample_path_label};
use crate::native_app::app_chrome::view_models::sample_browser::{
    SampleBrowserFramePreparationState, prepare_sample_browser_view,
};
use crate::native_app::sample_library::folder_browser::commands::FolderBrowserMessage;

const FRAME_MESSAGE_PROFILE_LABEL: &str = "Frame";
const SLOW_UI_INTERACTION_MESSAGE_THRESHOLD: Duration = Duration::from_millis(4);
const SLOW_UI_FRAME_MESSAGE_THRESHOLD: Duration = Duration::from_micros(16_667);

impl NativeAppState {
    pub(in crate::native_app) fn handle_message(
        &mut self,
        message: GuiMessage,
        context: &mut ui::UiUpdateContext<GuiMessage>,
    ) {
        let starmap_retained_scene_message = starmap_message_uses_retained_scene(&message);
        let starmap_scene_active_before = self.starmap_retained_scene_active();
        let frame_preparation_state = matches!(message, GuiMessage::Frame)
            .then(|| SampleBrowserFramePreparationState::capture(self));
        self.apply_message(message, context);
        if frame_preparation_state
            .map(|state| state.requires_preparation(self))
            .unwrap_or(true)
        {
            prepare_sample_browser_view(self);
        }
        if starmap_retained_scene_message
            && (starmap_scene_active_before || self.starmap_retained_scene_active())
        {
            context.request_paint_only();
        }
    }

    pub(in crate::native_app) fn apply_message(
        &mut self,
        message: GuiMessage,
        context: &mut ui::UiUpdateContext<GuiMessage>,
    ) {
        if !ui_message_diagnostics_enabled() {
            self.apply_message_inner(message, context);
            return;
        }
        let started_at = Instant::now();
        let message_label = gui_message_profile_label(&message);
        self.apply_message_inner(message, context);
        self.log_slow_ui_message(message_label, started_at);
    }

    fn apply_message_inner(
        &mut self,
        message: GuiMessage,
        context: &mut ui::UiUpdateContext<GuiMessage>,
    ) {
        if closes_waveform_context_menu(&message) {
            self.ui.browser_interaction.waveform_context_menu = None;
        }
        match message {
            GuiMessage::ResizeFolder(_)
            | GuiMessage::AddSourceDialogFinished(_)
            | GuiMessage::FolderBrowser(_)
            | GuiMessage::SetSimilarityAspectWeightingEnabled(_)
            | GuiMessage::SetSimilarityAspectEnabled { .. }
            | GuiMessage::SetSimilarityAspectWeight { .. }
            | GuiMessage::SimilaritySettingsPersisted(_)
            | GuiMessage::StarmapLayoutLoaded(_)
            | GuiMessage::SimilarityScoresResolved(_)
            | GuiMessage::SimilarityReadinessAdvanced { .. }
            | GuiMessage::SourceProcessingHealth(_)
            | GuiMessage::SourceProcessingProgress(_)
            | GuiMessage::FolderScanProgress(_)
            | GuiMessage::FolderScanDiscoveryBatch(_)
            | GuiMessage::FolderScanFinished(_)
            | GuiMessage::FolderScanMaintenanceFinished(_)
            | GuiMessage::FolderTreeRefreshFinished(_)
            | GuiMessage::SelectedFolderVerifyFinished(_)
            | GuiMessage::SourceFilesystemChanged { .. }
            | GuiMessage::SourceWatcherReady { .. }
            | GuiMessage::SourceWatcherJournalGap { .. }
            | GuiMessage::SourceFilesystemSyncFinished(_)
            | GuiMessage::SourceManifestAuditCommitted { .. }
            | GuiMessage::SourceManifestAuditFinished { .. }
            | GuiMessage::NormalizationProgress(_)
            | GuiMessage::NormalizationFinished(_)
            | GuiMessage::SelectSampleWithModifiers { .. }
            | GuiMessage::OpenSampleContextMenu { .. }
            | GuiMessage::DragSampleFile { .. }
            | GuiMessage::ExternalDragCompleted(_) => self.apply_browser_dispatch(message, context),
            GuiMessage::DeferredSampleLoad { .. }
            | GuiMessage::SettledSamplePromotion { .. }
            | GuiMessage::SampleLoadPathValidated { .. }
            | GuiMessage::SampleLoadProgress(_, _, _)
            | GuiMessage::SamplePlaybackReady(_)
            | GuiMessage::PreviewAuditionDecoded { .. }
            | GuiMessage::PreviewAuditionWarmFinished { .. }
            | GuiMessage::SampleLoadFinished(_)
            | GuiMessage::WaveformCacheIndicatorRefreshFinished(_)
            | GuiMessage::WaveformCacheWarmFinished(_)
            | GuiMessage::ActiveFolderCacheWarmPlanProgress(_)
            | GuiMessage::ActiveFolderCacheWarmPlanned(_)
            | GuiMessage::ActiveFolderCacheWarmReady(_)
            | GuiMessage::ActiveFolderCacheWarmProgress(_)
            | GuiMessage::ActiveFolderCacheWarmFinished(_) => {
                self.apply_sample_loading_dispatch(message, context);
            }
            GuiMessage::AudioPlayerOpenFinished(_)
            | GuiMessage::AudioOptionsRefreshFinished(_)
            | GuiMessage::AudioOutputPersisted(_)
            | GuiMessage::PlaySelectedSample
            | GuiMessage::PlayFromCurrentPlayStart
            | GuiMessage::PlayRandomSampleRange
            | GuiMessage::PlayRandomListedSampleRange
            | GuiMessage::PlayPreviousPlaybackHistory
            | GuiMessage::PlayNextPlaybackHistory
            | GuiMessage::LastPlayedPersistReady { .. }
            | GuiMessage::LastPlayedPersisted(_)
            | GuiMessage::HarvestSeenPersisted(_)
            | GuiMessage::VolumeSettingsPersisted(_)
            | GuiMessage::StopPlayback
            | GuiMessage::ToggleLoopPlayback
            | GuiMessage::ToggleMetronome => self.apply_playback_dispatch(message, context),
            GuiMessage::Settings(message) => self.apply_settings_message(message, context),
            GuiMessage::Metadata(message) => self.apply_metadata_message(message, context),
            GuiMessage::FocusLoadedFile
            | GuiMessage::AdjustSelectedRatingWithoutAdvance(_)
            | GuiMessage::AssignSelectedCollection(_)
            | GuiMessage::RemoveContextSampleFromCollection
            | GuiMessage::CleanMissingContextSampleFromCollection
            | GuiMessage::CleanMissingFilesFromActiveCollection
            | GuiMessage::MarkContextSampleHarvestDone
            | GuiMessage::MarkContextSampleHarvestIgnored
            | GuiMessage::ResetContextSampleHarvest
            | GuiMessage::ToggleSelectedHarvestDone
            | GuiMessage::ShowContextSampleHarvestOrigin
            | GuiMessage::ShowContextSampleHarvestDerivatives
            | GuiMessage::OpenContextSampleHarvestDestination
            | GuiMessage::ShowSelectedSampleHarvestOrigin
            | GuiMessage::ShowSelectedSampleHarvestDerivatives
            | GuiMessage::OpenSelectedSampleHarvestDestination
            | GuiMessage::NormalizeSelectedSamples
            | GuiMessage::CopySelectedFiles
            | GuiMessage::CutSelectedFiles
            | GuiMessage::PasteCutFiles
            | GuiMessage::DuplicateContextSampleSame
            | GuiMessage::DuplicateContextSampleDouble
            | GuiMessage::ContextSampleSameFinished { .. }
            | GuiMessage::ContextSampleDoubleFinished { .. }
            | GuiMessage::SelectedFilesCopyFinished { .. }
            | GuiMessage::WaveformSelectionCopyExtracted { .. }
            | GuiMessage::WaveformSelectionCopyFinished { .. }
            | GuiMessage::FileMoveProgress(_)
            | GuiMessage::SetFileMoveConflictApplyToRemaining(_)
            | GuiMessage::ResolveFileMoveConflict(_)
            | GuiMessage::FolderMoveFinished { .. }
            | GuiMessage::FileMoveConflictFinished { .. }
            | GuiMessage::CancelFileMoveConflicts
            | GuiMessage::CopyContextPath
            | GuiMessage::TrashFolderDialogFinished(_)
            | GuiMessage::ContextPathCopyFinished { .. }
            | GuiMessage::OpenContextTarget { .. }
            | GuiMessage::ContextTargetOpenValidated { .. }
            | GuiMessage::CreateFolderAtContextTarget
            | GuiMessage::RenameContextFolder
            | GuiMessage::ContextFolderCreateFinished { .. }
            | GuiMessage::MoveContextTargetToTrash
            | GuiMessage::UnlockContextSample
            | GuiMessage::ToggleContextFolderLock
            | GuiMessage::RequestDeleteContextFolder
            | GuiMessage::ConfirmContextFolderDelete
            | GuiMessage::CancelContextFolderDelete
            | GuiMessage::TrashMoveFinished { .. }
            | GuiMessage::ContextTargetOpenFinished { .. }
            | GuiMessage::RefreshContextSource
            | GuiMessage::ProcessContextSource
            | GuiMessage::ToggleContextSourceProtection
            | GuiMessage::SetContextSourcePrimary
            | GuiMessage::ClearContextSourcePrimary
            | GuiMessage::RemoveContextSource
            | GuiMessage::CloseContextMenu
            | GuiMessage::ExternalWaveformFileDropFinished { .. }
            | GuiMessage::CommittedFileMutationRequested(_)
            | GuiMessage::CommittedFileMutationFinished(_)
            | GuiMessage::NativeAudioDocumentOpenValidated { .. }
            | GuiMessage::WaveformFileDrop(_) => self.apply_file_dispatch(message, context),
            GuiMessage::ToggleJobDetails
            | GuiMessage::CloseJobDetails
            | GuiMessage::RetryActiveSourceScan
            | GuiMessage::CancelActiveSourceScan
            | GuiMessage::ReleaseUpdateCheckFinished(_)
            | GuiMessage::OpenReleaseDownloadPage
            | GuiMessage::ToggleShortcutHelp
            | GuiMessage::CloseShortcutHelp
            | GuiMessage::ToggleStickyRandomSampleRangePlayback
            | GuiMessage::ToggleCurationFilterDropdown
            | GuiMessage::CloseCurationFilterDropdown
            | GuiMessage::ToggleHarvestFilterDropdown
            | GuiMessage::CloseHarvestFilterDropdown
            | GuiMessage::ToggleZeroCrossingSnap
            | GuiMessage::ToggleBpmSnap
            | GuiMessage::ToggleBeatGuides
            | GuiMessage::SetBeatGuideCount(_)
            | GuiMessage::ChangeBeatGuideCountInput(_)
            | GuiMessage::CommitBeatGuideCountInput(_)
            | GuiMessage::ToggleSimilarSections
            | GuiMessage::SimilarSectionsResolved(_)
            | GuiMessage::UndoTransaction
            | GuiMessage::RedoTransaction
            | GuiMessage::UndoTransactionsThrough(_)
            | GuiMessage::RedoTransactionsThrough(_)
            | GuiMessage::ToggleTransactionList
            | GuiMessage::CloseTransactionList
            | GuiMessage::FocusRenameInput(_)
            | GuiMessage::FolderBrowserRenameFinished(_)
            | GuiMessage::DeleteSelectedItem
            | GuiMessage::RequestCropWaveformSelection
            | GuiMessage::RequestTrimWaveformSelection
            | GuiMessage::RequestReverseWaveformSelection
            | GuiMessage::RequestMuteWaveformSelection
            | GuiMessage::RequestExtractAndTrimWaveformSelection
            | GuiMessage::RequestCropPlaymarkSelection
            | GuiMessage::RequestTrimPlaymarkSelection
            | GuiMessage::RequestReversePlaymarkSelection
            | GuiMessage::RequestExtractAndTrimPlaymarkSelection
            | GuiMessage::RequestApplyEditSelectionEffects
            | GuiMessage::OpenContextMenu
            | GuiMessage::ConfirmPendingWaveformDestructiveEdit
            | GuiMessage::CancelPendingWaveformDestructiveEdit
            | GuiMessage::AddProtectedExtractionTargetSource
            | GuiMessage::ProtectedExtractionTargetSourceDialogFinished(_)
            | GuiMessage::CancelProtectedExtractionTargetSource
            | GuiMessage::WaveformDestructiveEditFinished(_)
            | GuiMessage::ExtractPlaymarkedRange
            | GuiMessage::ExtractPlaymarkedRangeToHarvestDestination
            | GuiMessage::PlaySelectionExtractionFinished { .. }
            | GuiMessage::SelectedWholeFilesHarvestExtractionFinished { .. } => {
                self.apply_chrome_dispatch(message, context);
            }
            GuiMessage::NavigateBrowser { .. }
            | GuiMessage::ToggleSelectedSampleAndAdvance
            | GuiMessage::SelectAllSamples
            | GuiMessage::ToggleRandomNavigationMode
            | GuiMessage::ToggleSampleBrowserMapView
            | GuiMessage::FocusSelectedStarmapNode
            | GuiMessage::ChangeStarmapViewport(_)
            | GuiMessage::BeginStarmapAuditionDrag { .. }
            | GuiMessage::UpdateStarmapAuditionDrag { .. }
            | GuiMessage::AdvanceStarmapAudition { .. }
            | GuiMessage::PromoteStarmapAudition { .. }
            | GuiMessage::FinishStarmapAuditionDrag
            | GuiMessage::SampleBrowserWindowChanged(_)
            | GuiMessage::FolderTreeWindowChanged(_)
            | GuiMessage::CollapseSelectedFolder
            | GuiMessage::CancelBrowserDragOnSampleList
            | GuiMessage::DropWaveformSelectionOnSampleList => {
                self.apply_navigation_dispatch(message, context);
            }
            GuiMessage::Waveform(message) => self.apply_waveform_message(message, context),
            GuiMessage::PlaymarkLabel(message) => {
                self.apply_playmark_label_message(message, context)
            }
            GuiMessage::WaveformDetailRefined(result) => {
                self.finish_waveform_detail_refinement(result, context);
            }
            GuiMessage::Frame => self.apply_frame_message(context),
        }
    }

    fn log_slow_ui_message(&self, message_label: &'static str, started_at: Instant) {
        let elapsed = started_at.elapsed();
        if elapsed < slow_ui_message_threshold(message_label) {
            return;
        }
        let selected = self
            .library
            .folder_browser
            .selected_file_id()
            .map(sample_path_label)
            .unwrap_or_default();
        tracing::warn!(
            target: "wavecrate::debug::ui_frame",
            event = "ui.message.slow",
            message = message_label,
            elapsed_ms = duration_ms(elapsed),
            sample_loading = self.active_sample_load_task().is_some(),
            waveform_loading = self.waveform_sample_load_active(),
            playing = self.waveform.current.is_playing(),
            pending_playback = self.audio.pending_playback_start.is_some(),
            selected = selected.as_str(),
            "Slow UI message dispatch"
        );
    }
}

fn starmap_message_uses_retained_scene(message: &GuiMessage) -> bool {
    matches!(
        message,
        GuiMessage::BeginStarmapAuditionDrag { .. }
            | GuiMessage::UpdateStarmapAuditionDrag { .. }
            | GuiMessage::AdvanceStarmapAudition { .. }
            | GuiMessage::PromoteStarmapAudition { .. }
            | GuiMessage::FinishStarmapAuditionDrag
            | GuiMessage::DeferredSampleLoad { .. }
            | GuiMessage::SettledSamplePromotion { .. }
            | GuiMessage::SampleLoadPathValidated { .. }
            | GuiMessage::SampleLoadProgress(..)
            | GuiMessage::SamplePlaybackReady(_)
            | GuiMessage::PreviewAuditionDecoded { .. }
            | GuiMessage::PreviewAuditionWarmFinished { .. }
            | GuiMessage::SampleLoadFinished(_)
            | GuiMessage::AudioPlayerOpenFinished(_)
            | GuiMessage::LastPlayedPersistReady { .. }
            | GuiMessage::LastPlayedPersisted(_)
            | GuiMessage::SourceProcessingProgress(_)
            | GuiMessage::FolderScanProgress(_)
            | GuiMessage::NormalizationProgress(_)
    )
}

fn gui_message_profile_label(message: &GuiMessage) -> &'static str {
    match message {
        GuiMessage::Frame => FRAME_MESSAGE_PROFILE_LABEL,
        GuiMessage::NavigateBrowser { .. } => "NavigateBrowser",
        GuiMessage::SelectSampleWithModifiers { .. } => "SelectSampleWithModifiers",
        GuiMessage::DeferredSampleLoad { .. } => "DeferredSampleLoad",
        GuiMessage::SettledSamplePromotion { .. } => "SettledSamplePromotion",
        GuiMessage::SampleLoadPathValidated { .. } => "SampleLoadPathValidated",
        GuiMessage::SampleLoadProgress(_, _, _) => "SampleLoadProgress",
        GuiMessage::SamplePlaybackReady(_) => "SamplePlaybackReady",
        GuiMessage::PreviewAuditionDecoded { .. } => "PreviewAuditionDecoded",
        GuiMessage::PreviewAuditionWarmFinished { .. } => "PreviewAuditionWarmFinished",
        GuiMessage::PromoteStarmapAudition { .. } => "PromoteStarmapAudition",
        GuiMessage::SampleLoadFinished(_) => "SampleLoadFinished",
        GuiMessage::AudioPlayerOpenFinished(_) => "AudioPlayerOpenFinished",
        GuiMessage::AudioOptionsRefreshFinished(_) => "AudioOptionsRefreshFinished",
        GuiMessage::AudioOutputPersisted(_) => "AudioOutputPersisted",
        GuiMessage::LastPlayedPersistReady { .. } => "LastPlayedPersistReady",
        GuiMessage::LastPlayedPersisted(_) => "LastPlayedPersisted",
        GuiMessage::HarvestSeenPersisted(_) => "HarvestSeenPersisted",
        GuiMessage::WaveformCacheIndicatorRefreshFinished(_) => {
            "WaveformCacheIndicatorRefreshFinished"
        }
        GuiMessage::WaveformCacheWarmFinished(_) => "WaveformCacheWarmFinished",
        GuiMessage::ActiveFolderCacheWarmPlanProgress(_) => "ActiveFolderCacheWarmPlanProgress",
        GuiMessage::ActiveFolderCacheWarmPlanned(_) => "ActiveFolderCacheWarmPlanned",
        GuiMessage::ActiveFolderCacheWarmReady(_) => "ActiveFolderCacheWarmReady",
        GuiMessage::ActiveFolderCacheWarmProgress(_) => "ActiveFolderCacheWarmProgress",
        GuiMessage::ActiveFolderCacheWarmFinished(_) => "ActiveFolderCacheWarmFinished",
        GuiMessage::SampleBrowserWindowChanged(_) => "SampleBrowserWindowChanged",
        GuiMessage::FolderBrowser(message) => folder_browser_profile_label(message),
        GuiMessage::FolderScanProgress(_) => "FolderScanProgress",
        GuiMessage::FolderScanDiscoveryBatch(_) => "FolderScanDiscoveryBatch",
        GuiMessage::FolderScanFinished(_) => "FolderScanFinished",
        GuiMessage::FolderScanMaintenanceFinished(_) => "FolderScanMaintenanceFinished",
        GuiMessage::FolderTreeRefreshFinished(_) => "FolderTreeRefreshFinished",
        GuiMessage::SelectedFolderVerifyFinished(_) => "SelectedFolderVerifyFinished",
        GuiMessage::SourceFilesystemChanged { .. } => "SourceFilesystemChanged",
        GuiMessage::SourceWatcherReady { .. } => "SourceWatcherReady",
        GuiMessage::SourceWatcherJournalGap { .. } => "SourceWatcherJournalGap",
        GuiMessage::SourceFilesystemSyncFinished(_) => "SourceFilesystemSyncFinished",
        GuiMessage::SourceManifestAuditCommitted { .. } => "SourceManifestAuditCommitted",
        GuiMessage::SourceManifestAuditFinished { .. } => "SourceManifestAuditFinished",
        GuiMessage::SourceProcessingProgress(_) => "SourceProcessingProgress",
        GuiMessage::NormalizationProgress(_) => "NormalizationProgress",
        GuiMessage::NormalizationFinished(_) => "NormalizationFinished",
        GuiMessage::Waveform(_) => "Waveform",
        GuiMessage::PlaymarkLabel(_) => "PlaymarkLabel",
        GuiMessage::PlaySelectedSample => "PlaySelectedSample",
        GuiMessage::PlayFromCurrentPlayStart => "PlayFromCurrentPlayStart",
        GuiMessage::PlayRandomSampleRange => "PlayRandomSampleRange",
        GuiMessage::PlayRandomListedSampleRange => "PlayRandomListedSampleRange",
        GuiMessage::StopPlayback => "StopPlayback",
        GuiMessage::ToggleLoopPlayback => "ToggleLoopPlayback",
        GuiMessage::ToggleMetronome => "ToggleMetronome",
        GuiMessage::PlayPreviousPlaybackHistory => "PlayPreviousPlaybackHistory",
        GuiMessage::PlayNextPlaybackHistory => "PlayNextPlaybackHistory",
        GuiMessage::Settings(_) => "Settings",
        GuiMessage::Metadata(_) => "Metadata",
        _ => "Other",
    }
}

fn slow_ui_message_threshold(message_label: &'static str) -> Duration {
    if message_label == FRAME_MESSAGE_PROFILE_LABEL {
        SLOW_UI_FRAME_MESSAGE_THRESHOLD
    } else {
        SLOW_UI_INTERACTION_MESSAGE_THRESHOLD
    }
}

fn ui_message_diagnostics_enabled() -> bool {
    cfg!(debug_assertions)
}

fn folder_browser_profile_label(message: &FolderBrowserMessage) -> &'static str {
    match message {
        FolderBrowserMessage::AddSource => "FolderBrowser::AddSource",
        FolderBrowserMessage::SelectSource(_) => "FolderBrowser::SelectSource",
        FolderBrowserMessage::NavigateSource(_) => "FolderBrowser::NavigateSource",
        FolderBrowserMessage::DragSource(_, _) => "FolderBrowser::DragSource",
        FolderBrowserMessage::OpenSourceContextMenu(_, _) => "FolderBrowser::OpenSourceContextMenu",
        FolderBrowserMessage::ActivateFolder(_, _) => "FolderBrowser::ActivateFolder",
        FolderBrowserMessage::ToggleFolderExpansion(_) => "FolderBrowser::ToggleFolderExpansion",
        FolderBrowserMessage::OpenFolderContextMenu(_, _) => "FolderBrowser::OpenFolderContextMenu",
        FolderBrowserMessage::DragFolder(_, _) => "FolderBrowser::DragFolder",
        FolderBrowserMessage::HoverDropTarget(_, _) => "FolderBrowser::HoverDropTarget",
        FolderBrowserMessage::ClearDropTargetUnless(_, _) => "FolderBrowser::ClearDropTargetUnless",
        FolderBrowserMessage::ClearDropTarget(_) => "FolderBrowser::ClearDropTarget",
        FolderBrowserMessage::DropOnFolder(_) => "FolderBrowser::DropOnFolder",
        FolderBrowserMessage::HoverSourceDropTarget(_, _) => "FolderBrowser::HoverSourceDropTarget",
        FolderBrowserMessage::ClearSourceDropTargetUnless(_, _) => {
            "FolderBrowser::ClearSourceDropTargetUnless"
        }
        FolderBrowserMessage::DropOnSource(_) => "FolderBrowser::DropOnSource",
        FolderBrowserMessage::ToggleFolderSubtreeListing => {
            "FolderBrowser::ToggleFolderSubtreeListing"
        }
        FolderBrowserMessage::ToggleEmptyFolderVisibility => {
            "FolderBrowser::ToggleEmptyFolderVisibility"
        }
        FolderBrowserMessage::ResizeCollectionsPanel(_) => "FolderBrowser::ResizeCollectionsPanel",
        FolderBrowserMessage::ResizeFilterPanel(_) => "FolderBrowser::ResizeFilterPanel",
        FolderBrowserMessage::ResizeMetadataPanel(_) => "FolderBrowser::ResizeMetadataPanel",
        FolderBrowserMessage::ActivateCollection(_) => "FolderBrowser::ActivateCollection",
        FolderBrowserMessage::OpenCollectionContextMenu(_, _) => {
            "FolderBrowser::OpenCollectionContextMenu"
        }
        FolderBrowserMessage::RenameCollection(_) => "FolderBrowser::RenameCollection",
        FolderBrowserMessage::HoverCollectionDropTarget(_, _) => {
            "FolderBrowser::HoverCollectionDropTarget"
        }
        FolderBrowserMessage::DropOnCollection(_) => "FolderBrowser::DropOnCollection",
        FolderBrowserMessage::BeginRenameSelected => "FolderBrowser::BeginRenameSelected",
        FolderBrowserMessage::CancelRename => "FolderBrowser::CancelRename",
        FolderBrowserMessage::BeginCreateSubfolder => "FolderBrowser::BeginCreateSubfolder",
        FolderBrowserMessage::RenameInput(_) => "FolderBrowser::RenameInput",
        FolderBrowserMessage::NameFilterInput(_) => "FolderBrowser::NameFilterInput",
        FolderBrowserMessage::TagFilterInput(_) => "FolderBrowser::TagFilterInput",
        FolderBrowserMessage::SetFilterFamilyEnabled(_, _) => {
            "FolderBrowser::SetFilterFamilyEnabled"
        }
        FolderBrowserMessage::TogglePlaybackTypeFilter(_, _) => {
            "FolderBrowser::TogglePlaybackTypeFilter"
        }
        FolderBrowserMessage::ToggleRatingFilter(_, _) => "FolderBrowser::ToggleRatingFilter",
        FolderBrowserMessage::SetCurationScope(_, _) => "FolderBrowser::SetCurationScope",
        FolderBrowserMessage::SetHarvestFilter(_, _) => "FolderBrowser::SetHarvestFilter",
        FolderBrowserMessage::SortFileColumn(_) => "FolderBrowser::SortFileColumn",
        FolderBrowserMessage::ResizeFileColumn(_, _) => "FolderBrowser::ResizeFileColumn",
        FolderBrowserMessage::DragFileColumn(_, _) => "FolderBrowser::DragFileColumn",
        FolderBrowserMessage::CancelFileColumnDrag => "FolderBrowser::CancelFileColumnDrag",
        FolderBrowserMessage::ExitCollectionFocus => "FolderBrowser::ExitCollectionFocus",
        FolderBrowserMessage::ToggleSimilarityAnchor(_) => "FolderBrowser::ToggleSimilarityAnchor",
    }
}

fn duration_ms(duration: Duration) -> f64 {
    duration.as_secs_f64() * 1_000.0
}

fn closes_waveform_context_menu(message: &GuiMessage) -> bool {
    matches!(
        message,
        GuiMessage::PlaySelectedSample
            | GuiMessage::ExtractPlaymarkedRange
            | GuiMessage::ExtractPlaymarkedRangeToHarvestDestination
            | GuiMessage::RequestCropPlaymarkSelection
            | GuiMessage::RequestTrimPlaymarkSelection
            | GuiMessage::RequestReversePlaymarkSelection
            | GuiMessage::RequestExtractAndTrimPlaymarkSelection
            | GuiMessage::ToggleSimilarSections
            | GuiMessage::Waveform(WaveformInteraction::ZoomToPlaySelection)
    )
}
