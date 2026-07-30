use radiant::prelude as ui;

use crate::native_app::app::{GuiMessage, NativeAppState};
use crate::native_app::app_chrome::view_models::status_bar::StatusBarViewModel;
use crate::native_app::waveform::{SimilarSectionsResult, execute_similar_sections_scan};

impl NativeAppState {
    pub(super) fn apply_chrome_dispatch(
        &mut self,
        message: GuiMessage,
        context: &mut ui::UiUpdateContext<GuiMessage>,
    ) {
        match message {
            GuiMessage::ToggleJobDetails => {
                let job_active = StatusBarViewModel::from_app_state(self)
                    .job_details
                    .is_some();
                self.ui.chrome.job_details_open = job_active && !self.ui.chrome.job_details_open;
            }
            GuiMessage::CloseJobDetails => {
                self.ui.chrome.job_details_open = false;
            }
            GuiMessage::RetryActiveSourceScan => {
                self.retry_active_source_scan(context);
            }
            GuiMessage::CancelActiveSourceScan => {
                let _ = self.cancel_active_source_scan(false);
            }
            GuiMessage::ReleaseUpdateCheckFinished(completion) => {
                self.finish_release_update_check(completion);
            }
            GuiMessage::OpenReleaseDownloadPage => {
                self.open_release_download_page();
            }
            GuiMessage::ToggleShortcutHelp => {
                self.ui.chrome.shortcut_help_open = !self.ui.chrome.shortcut_help_open;
            }
            GuiMessage::CloseShortcutHelp => {
                self.ui.chrome.shortcut_help_open = false;
            }
            GuiMessage::ToggleStickyRandomSampleRangePlayback => {
                self.ui.chrome.sticky_random_sample_range_playback =
                    !self.ui.chrome.sticky_random_sample_range_playback;
                self.ui.status.sample = if self.ui.chrome.sticky_random_sample_range_playback {
                    String::from("Sticky random playback on: Space plays random sample sections")
                } else {
                    String::from("Sticky random playback off: Space plays selected samples")
                };
            }
            GuiMessage::ToggleCurationFilterDropdown => {
                self.ui.chrome.curation_filter_dropdown_open =
                    !self.ui.chrome.curation_filter_dropdown_open;
                self.ui.chrome.harvest_filter_dropdown_open = false;
            }
            GuiMessage::CloseCurationFilterDropdown => {
                self.ui.chrome.curation_filter_dropdown_open = false;
            }
            GuiMessage::ToggleHarvestFilterDropdown => {
                self.ui.chrome.harvest_filter_dropdown_open =
                    !self.ui.chrome.harvest_filter_dropdown_open;
                self.ui.chrome.curation_filter_dropdown_open = false;
            }
            GuiMessage::CloseHarvestFilterDropdown => {
                self.ui.chrome.harvest_filter_dropdown_open = false;
            }
            GuiMessage::ToggleZeroCrossingSnap => {
                let enabled = self.waveform.current.toggle_zero_crossing_snap();
                self.ui.status.sample = if enabled {
                    String::from("Zero crossing snap enabled")
                } else {
                    String::from("Zero crossing snap disabled")
                };
            }
            GuiMessage::ToggleBpmSnap => {
                self.ui.chrome.bpm_snap_enabled = !self.ui.chrome.bpm_snap_enabled;
                self.ui.status.sample = if self.ui.chrome.bpm_snap_enabled {
                    String::from("BPM snap enabled")
                } else {
                    String::from("BPM snap disabled")
                };
            }
            GuiMessage::ToggleBeatGuides => {
                self.ui.chrome.beat_guides_enabled = !self.ui.chrome.beat_guides_enabled;
            }
            GuiMessage::SetBeatGuideCount(count) => {
                self.ui.chrome.set_beat_guide_count(count);
            }
            GuiMessage::ChangeBeatGuideCountInput(value) => {
                self.ui.chrome.preview_beat_guide_count_input(&value);
            }
            GuiMessage::CommitBeatGuideCountInput(value) => {
                self.ui.chrome.commit_beat_guide_count_input(&value);
            }
            GuiMessage::ToggleSimilarSections => {
                self.toggle_similar_sections(context);
            }
            GuiMessage::SimilarSectionsResolved(result) => {
                self.finish_similar_sections(result);
            }
            GuiMessage::UndoTransaction => self.undo_transaction(context),
            GuiMessage::RedoTransaction => self.redo_transaction(context),
            GuiMessage::UndoTransactionsThrough(target_id) => {
                self.undo_transactions_through(target_id, context);
            }
            GuiMessage::RedoTransactionsThrough(target_id) => {
                self.redo_transactions_through(target_id, context);
            }
            GuiMessage::ToggleTransactionList => self.toggle_transaction_list(),
            GuiMessage::CloseTransactionList => {
                self.ui.chrome.transaction_list_open = false;
            }
            GuiMessage::FocusRenameInput(input_id) => {
                self.focus_rename_input(input_id, context);
            }
            GuiMessage::FolderBrowserRenameFinished(completion) => {
                self.finish_folder_browser_rename(completion, context);
            }
            GuiMessage::DeleteSelectedItem => self.delete_selected_item(context),
            GuiMessage::RequestCropWaveformSelection => {
                self.request_crop_waveform_selection(context);
            }
            GuiMessage::RequestTrimWaveformSelection => {
                self.request_trim_waveform_selection(context);
            }
            GuiMessage::RequestReverseWaveformSelection => {
                self.request_reverse_waveform_selection(context);
            }
            GuiMessage::RequestMuteWaveformSelection => {
                self.request_mute_waveform_selection(context);
            }
            GuiMessage::RequestExtractAndTrimWaveformSelection => {
                self.request_extract_and_trim_waveform_selection(context);
            }
            GuiMessage::RequestCropPlaymarkSelection => {
                self.request_crop_playmark_selection(context);
            }
            GuiMessage::RequestTrimPlaymarkSelection => {
                self.request_trim_playmark_selection(context);
            }
            GuiMessage::RequestReversePlaymarkSelection => {
                self.request_reverse_playmark_selection(context);
            }
            GuiMessage::RequestExtractAndTrimPlaymarkSelection => {
                self.request_extract_and_trim_playmark_selection(context);
            }
            GuiMessage::RequestApplyEditSelectionEffects => {
                self.request_apply_edit_selection_effects(context);
            }
            GuiMessage::OpenContextMenu => {
                self.open_context_menu_from_shortcut(context);
            }
            GuiMessage::ConfirmPendingWaveformDestructiveEdit => {
                self.confirm_pending_waveform_destructive_edit(context);
            }
            GuiMessage::CancelPendingWaveformDestructiveEdit => {
                self.cancel_pending_waveform_destructive_edit();
            }
            GuiMessage::AddProtectedExtractionTargetSource => {
                self.add_protected_extraction_target_source(context);
            }
            GuiMessage::ProtectedExtractionTargetSourceDialogFinished(result) => {
                self.finish_protected_extraction_target_source_dialog(result, context);
            }
            GuiMessage::CancelProtectedExtractionTargetSource => {
                self.cancel_protected_extraction_target_source();
            }
            GuiMessage::WaveformDestructiveEditFinished(completion) => {
                self.finish_waveform_destructive_edit(completion, context);
            }
            GuiMessage::ExtractPlaymarkedRange => self.extract_playmarked_range(context),
            GuiMessage::ExtractPlaymarkedRangeToHarvestDestination => {
                self.extract_playmarked_range_to_harvest_destination(context);
            }
            GuiMessage::PlaySelectionExtractionFinished {
                completion,
                drag_position,
                playback_type,
                harvest_operation,
                focus_derivative,
                started_at,
            } => self.finish_play_selection_extraction(
                completion,
                drag_position,
                playback_type,
                harvest_operation,
                focus_derivative,
                started_at,
                context,
            ),
            GuiMessage::HarvestSelectionDerivationFinished {
                source_path,
                child_path,
                result,
            } => self.finish_harvest_selection_derivation(source_path, child_path, result),
            GuiMessage::SelectedWholeFilesHarvestExtractionFinished { started_at, result } => {
                self.finish_selected_whole_files_harvest_extraction(started_at, result, context);
            }
            _ => unreachable!("chrome dispatcher received a non-chrome message"),
        }
    }

    fn toggle_similar_sections(&mut self, context: &mut ui::UiUpdateContext<GuiMessage>) {
        if self.waveform.current.similar_sections_enabled() {
            self.waveform.current.clear_similar_sections();
            self.ui.status.sample = String::from("Similar section marks off");
            return;
        }

        let request = match self.waveform.current.similar_sections_request() {
            Ok(request) => request,
            Err(error) => {
                self.ui.status.sample = error;
                return;
            }
        };
        let anchor = request.anchor();
        self.waveform.current.start_similar_sections(anchor);
        self.ui.status.sample = String::from("Finding similar sections");
        context
            .business()
            .background("gui-waveform-similar-sections")
            .run(
                move |_| execute_similar_sections_scan(request),
                GuiMessage::SimilarSectionsResolved,
            );
    }

    fn finish_similar_sections(&mut self, result: SimilarSectionsResult) {
        if !self
            .waveform
            .current
            .similar_sections_result_applies(&result)
        {
            return;
        }
        match result.result {
            Ok(payload) => {
                let count = payload.ranges.len();
                self.waveform
                    .current
                    .finish_similar_sections_scan(payload.ranges);
                self.ui.status.sample = if count == 0 {
                    String::from("No similar sections found")
                } else {
                    format!(
                        "Found {count} similar section{}",
                        if count == 1 { "" } else { "s" }
                    )
                };
            }
            Err(error) => {
                self.waveform.current.clear_similar_sections();
                self.ui.status.sample = error;
            }
        }
    }
}
