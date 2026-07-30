use radiant::prelude as ui;

use crate::native_app::app::{GuiMessage, NativeAppState};

impl NativeAppState {
    pub(super) fn apply_file_dispatch(
        &mut self,
        message: GuiMessage,
        context: &mut ui::UiUpdateContext<GuiMessage>,
    ) {
        match message {
            GuiMessage::FocusLoadedFile => self.focus_loaded_file(context),
            GuiMessage::AdjustSelectedRatingWithoutAdvance(delta) => {
                self.adjust_selected_rating_without_advance(delta, context)
            }
            GuiMessage::AssignSelectedCollection(collection) => {
                self.assign_selected_collection(collection, context)
            }
            GuiMessage::RemoveContextSampleFromCollection => {
                self.remove_context_sample_from_collection(context)
            }
            GuiMessage::CleanMissingContextSampleFromCollection => {
                self.clean_missing_context_sample_from_collection()
            }
            GuiMessage::CleanMissingFilesFromActiveCollection => {
                self.clean_missing_files_from_active_collection()
            }
            GuiMessage::MarkContextSampleHarvestDone => self.mark_context_sample_harvest_done(),
            GuiMessage::MarkContextSampleHarvestIgnored => {
                self.mark_context_sample_harvest_ignored()
            }
            GuiMessage::ResetContextSampleHarvest => self.reset_context_sample_harvest(),
            GuiMessage::ToggleSelectedHarvestDone => self.toggle_selected_harvest_done(),
            GuiMessage::ShowContextSampleHarvestOrigin => {
                self.show_context_sample_harvest_origin(context)
            }
            GuiMessage::ShowContextSampleHarvestDerivatives => {
                self.show_context_sample_harvest_derivatives(context)
            }
            GuiMessage::OpenContextSampleHarvestDestination => {
                self.open_context_sample_harvest_destination(context)
            }
            GuiMessage::ShowSelectedSampleHarvestOrigin => {
                self.show_selected_sample_harvest_origin(context)
            }
            GuiMessage::ShowSelectedSampleHarvestDerivatives => {
                self.show_selected_sample_harvest_derivatives(context)
            }
            GuiMessage::OpenSelectedSampleHarvestDestination => {
                self.open_selected_sample_harvest_destination(context)
            }
            GuiMessage::NormalizeSelectedSamples => self.normalize_selected_samples(context),
            GuiMessage::CopySelectedFiles => self.copy_selected_files(context),
            GuiMessage::CutSelectedFiles => self.cut_selected_files(),
            GuiMessage::PasteCutFiles => self.paste_cut_files(context),
            GuiMessage::DuplicateContextSampleSame => self.duplicate_context_sample_same(context),
            GuiMessage::DuplicateContextSampleDouble => {
                self.duplicate_context_sample_double(context)
            }
            GuiMessage::ContextSampleSameFinished {
                source_path,
                started_at,
                result,
            } => self.finish_context_sample_same(source_path, started_at, result, context),
            GuiMessage::ContextSampleDoubleFinished {
                source_path,
                started_at,
                result,
            } => self.finish_context_sample_double(source_path, started_at, result, context),
            GuiMessage::SelectedFilesCopyFinished {
                count,
                started_at,
                result,
            } => self.finish_copy_selected_files(count, started_at, result),
            GuiMessage::WaveformSelectionCopyExtracted {
                completion,
                playback_type,
                started_at,
            } => self.finish_waveform_selection_copy_extracted(
                completion,
                playback_type,
                started_at,
                context,
            ),
            GuiMessage::WaveformSelectionCopyFinished {
                source_path,
                selection,
                copied_path,
                playback_type,
                source_duration_seconds,
                started_at,
                evidence,
                result,
            } => self.finish_waveform_selection_copy(
                source_path,
                selection,
                copied_path,
                playback_type,
                source_duration_seconds,
                started_at,
                evidence,
                result,
                context,
            ),
            GuiMessage::FileMoveProgress(progress) => self.apply_file_move_progress(progress),
            GuiMessage::SetFileMoveConflictApplyToRemaining(apply_to_remaining) => {
                self.ui
                    .browser_interaction
                    .file_move_conflict_apply_to_remaining = apply_to_remaining;
            }
            GuiMessage::ResolveFileMoveConflict(request) => {
                self.resolve_file_move_conflict(request, context);
            }
            GuiMessage::FolderMoveFinished {
                started_at,
                completion,
            } => {
                self.finish_folder_move(started_at, completion, context);
            }
            GuiMessage::FileMoveConflictFinished {
                started_at,
                completion,
            } => {
                self.finish_file_move_conflict(started_at, completion, context);
            }
            GuiMessage::CancelFileMoveConflicts => self.cancel_file_move_conflicts(),
            GuiMessage::CopyContextPath => self.copy_context_path(context),
            GuiMessage::TrashFolderDialogFinished(result) => {
                self.finish_trash_folder_dialog(result, context);
            }
            GuiMessage::UnsupportedFilesDialogFinished { source_id, result } => {
                self.finish_unsupported_files_dialog(source_id, result);
            }
            GuiMessage::ContextPathCopyFinished { kind, path, result } => {
                self.finish_context_path_copy(kind, path, result);
            }
            GuiMessage::OpenContextTarget { kind, path } => {
                self.open_context_target(kind, path, context)
            }
            GuiMessage::ContextTargetOpenValidated { kind, path, result } => {
                self.finish_context_target_validation(kind, path, result, context)
            }
            GuiMessage::CreateFolderAtContextTarget => {
                self.create_folder_at_context_target(context)
            }
            GuiMessage::RenameContextFolder => self.rename_context_folder(context),
            GuiMessage::ContextFolderCreateFinished {
                parent_id,
                started_at,
                result,
            } => self.finish_context_folder_create(parent_id, started_at, result, context),
            GuiMessage::MoveContextTargetToTrash => self.move_context_target_to_trash(context),
            GuiMessage::ChooseTrashFolderForPendingMove => {
                self.choose_trash_folder_for_pending_move(context)
            }
            GuiMessage::CancelTrashFolderSetup => self.cancel_trash_folder_setup(),
            GuiMessage::RevealUnsupportedFile(path) => self.reveal_unsupported_file(path, context),
            GuiMessage::MoveUnsupportedFileToTrash(path) => {
                self.move_unsupported_file_to_trash(path, context)
            }
            GuiMessage::UnlockContextSample => self.unlock_context_sample(context),
            GuiMessage::ToggleContextFolderLock => self.toggle_context_folder_lock(),
            GuiMessage::RequestDeleteContextFolder => self.request_delete_context_folder(),
            GuiMessage::ConfirmContextFolderDelete => self.confirm_context_folder_delete(context),
            GuiMessage::CancelContextFolderDelete => self.cancel_context_folder_delete(),
            GuiMessage::TrashMoveFinished {
                target,
                action,
                started_at,
                outcomes,
            } => self.finish_trash_move(target, action, started_at, outcomes, context),
            GuiMessage::ContextTargetOpenFinished { kind, path, result } => {
                self.finish_context_target_open(kind, path, result);
            }
            GuiMessage::RefreshContextSource => self.refresh_context_source(context),
            GuiMessage::ProcessContextSource => self.process_context_source(context),
            GuiMessage::ToggleContextSourceProtection => self.toggle_context_source_protection(),
            GuiMessage::SetContextSourcePrimary => self.set_context_source_primary(),
            GuiMessage::ClearContextSourcePrimary => self.clear_context_source_primary(),
            GuiMessage::RemoveContextSource => self.remove_context_source(),
            GuiMessage::CloseContextMenu => {
                self.ui.browser_interaction.context_menu = None;
                self.ui.browser_interaction.waveform_context_menu = None;
            }
            GuiMessage::ExternalWaveformFileDropFinished {
                source,
                started_at,
                result,
            } => self.finish_external_waveform_file_drop(source, started_at, result, context),
            GuiMessage::CommittedFileMutationRequested(work) => {
                self.start_committed_file_mutation(work, context)
            }
            GuiMessage::CommittedFileMutationFinished(outcome) => {
                self.finish_committed_file_mutation(outcome, context)
            }
            GuiMessage::HistoryFileIoRequested(command) => {
                self.start_history_file_io(command, context)
            }
            GuiMessage::HistoryFileIoFinished(result) => {
                self.finish_history_file_io(result, context)
            }
            GuiMessage::NativeAudioDocumentOpenValidated {
                started_at,
                validation,
            } => self.finish_audio_document_open_validation(started_at, validation, context),
            GuiMessage::WaveformFileDrop(drop) => self.apply_native_file_drop(drop, context),
            _ => unreachable!("file dispatcher received a non-file message"),
        }
    }
}
