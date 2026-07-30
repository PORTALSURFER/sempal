use crate::native_app::app::{
    ExtractedFilePlaybackType, GuiMessage, NativeAppState, PendingProtectedExtractionAction,
};
use crate::native_app::app::{emit_gui_action, sample_path_label};
use crate::native_app::sample_library::committed_file_mutations::{
    FileMutationChange, FileMutationOperation, FileMutationProjection,
};
use crate::native_app::sample_library::folder_browser::BrowserListingRevealReason;
use crate::native_app::sample_library::sample_list::{
    SAMPLE_BROWSER_LIST_ID, SAMPLE_BROWSER_ROW_HEIGHT, SAMPLE_BROWSER_SELECTION_CONTEXT_ROWS,
};
use crate::native_app::waveform::{
    WaveformExtractionCompletion, WaveformExtractionRequest, WaveformSelectionKind,
    execute_waveform_extraction,
};
use radiant::gui::types::Point;
use std::{
    path::{Path, PathBuf},
    time::Instant,
};
use wavecrate::sample_sources::{
    HarvestDerivationOperation, SourceRole, WholeFileHarvestExtractionPlan,
    WholeFileHarvestExtractionRequest, WholeFileHarvestExtractionResult,
    execute_whole_file_harvest_extraction,
};

#[derive(Clone, Copy)]
enum PlaymarkedExtractionTarget {
    Default,
    HarvestDestination,
}

impl PlaymarkedExtractionTarget {
    fn action_name(self) -> &'static str {
        match self {
            Self::Default => "waveform.extract_playmarked_range",
            Self::HarvestDestination => "waveform.extract_playmarked_range.harvest_destination",
        }
    }

    fn status_text(self) -> &'static str {
        match self {
            Self::Default => "Extracting play range",
            Self::HarvestDestination => "Extracting play range to harvest destination",
        }
    }

    fn pending_protected_extraction_action(self) -> PendingProtectedExtractionAction {
        match self {
            Self::Default => PendingProtectedExtractionAction::ExtractPlaymarkedRange,
            Self::HarvestDestination => {
                PendingProtectedExtractionAction::ExtractPlaymarkedRangeToHarvestDestination
            }
        }
    }
}

impl NativeAppState {
    pub(in crate::native_app) fn focus_loaded_file(
        &mut self,
        context: &mut radiant::prelude::UiUpdateContext<GuiMessage>,
    ) {
        let started_at = Instant::now();
        if !self.waveform.current.has_loaded_sample() {
            self.ui.status.sample = String::from("Load a sample to focus it");
            emit_gui_action(
                "browser.focus_loaded_file",
                Some("browser"),
                None,
                "empty",
                started_at,
                None,
            );
            return;
        }
        let path = self.waveform.current.path();
        if self
            .library
            .folder_browser
            .focus_file_across_sources_matching_tags(&path, &self.metadata.tags_by_file)
        {
            if let Some(index) = self
                .library
                .folder_browser
                .selected_audio_file_index_matching_tags(&self.metadata.tags_by_file)
            {
                context.scroll_into_view_snapped(
                    SAMPLE_BROWSER_LIST_ID,
                    index as f32 * SAMPLE_BROWSER_ROW_HEIGHT,
                    SAMPLE_BROWSER_ROW_HEIGHT,
                    SAMPLE_BROWSER_SELECTION_CONTEXT_ROWS as f32 * SAMPLE_BROWSER_ROW_HEIGHT,
                    SAMPLE_BROWSER_SELECTION_CONTEXT_ROWS as f32 * SAMPLE_BROWSER_ROW_HEIGHT,
                    SAMPLE_BROWSER_ROW_HEIGHT,
                );
            }
            self.ui.status.sample = format!("Focused {}", sample_path_label(&path));
            emit_gui_action(
                "browser.focus_loaded_file",
                Some("browser"),
                None,
                "success",
                started_at,
                None,
            );
        } else {
            let error = format!(
                "Loaded sample is not visible in sources: {}",
                path.display()
            );
            self.ui.status.sample = error.clone();
            emit_gui_action(
                "browser.focus_loaded_file",
                Some("browser"),
                None,
                "not_found",
                started_at,
                Some(&error),
            );
        }
    }

    pub(in crate::native_app) fn delete_selected_item(
        &mut self,
        context: &mut radiant::prelude::UiUpdateContext<GuiMessage>,
    ) {
        if self.library.folder_browser.selected_file_id().is_some() {
            self.delete_selected_files(context);
        } else {
            self.delete_selected_folder(context);
        }
    }

    fn delete_selected_folder(
        &mut self,
        context: &mut radiant::prelude::UiUpdateContext<GuiMessage>,
    ) {
        let started_at = Instant::now();
        let target = match self.library.folder_browser.selected_delete_target() {
            Ok(target) => target,
            Err(error) => {
                self.ui.status.sample = error.clone();
                emit_gui_action(
                    "folder_browser.delete_selected",
                    Some("folder_browser"),
                    None,
                    "short_circuit",
                    started_at,
                    Some(&error),
                );
                return;
            }
        };
        self.move_selected_folder_to_trash(target.path, started_at, context);
    }

    fn delete_selected_files(
        &mut self,
        context: &mut radiant::prelude::UiUpdateContext<GuiMessage>,
    ) {
        let started_at = Instant::now();
        let target = match self.library.folder_browser.selected_file_delete_target() {
            Ok(target) => target,
            Err(error) => {
                self.ui.status.sample = error.clone();
                emit_gui_action(
                    "browser.delete_selected_files",
                    Some("browser"),
                    None,
                    "short_circuit",
                    started_at,
                    Some(&error),
                );
                return;
            }
        };
        self.move_selected_files_to_trash(target.paths, started_at, context);
    }

    pub(in crate::native_app) fn extract_playmarked_range(
        &mut self,
        context: &mut radiant::prelude::UiUpdateContext<GuiMessage>,
    ) {
        self.extract_playmarked_range_to_target(context, PlaymarkedExtractionTarget::Default);
    }

    pub(in crate::native_app) fn extract_playmarked_range_to_harvest_destination(
        &mut self,
        context: &mut radiant::prelude::UiUpdateContext<GuiMessage>,
    ) {
        self.extract_playmarked_range_to_target(
            context,
            PlaymarkedExtractionTarget::HarvestDestination,
        );
    }

    fn extract_playmarked_range_to_target(
        &mut self,
        context: &mut radiant::prelude::UiUpdateContext<GuiMessage>,
        target: PlaymarkedExtractionTarget,
    ) {
        let started_at = Instant::now();
        let action = target.action_name();
        if matches!(target, PlaymarkedExtractionTarget::Default)
            && self.waveform.current.play_selection().is_none()
        {
            self.extract_selected_whole_files_to_harvest(context, started_at);
            return;
        }
        match self
            .waveform
            .current
            .play_selection_extraction_request(None)
        {
            Ok(request) => {
                let selection = request.selection();
                let request = request.with_gain(
                    self.normalized_audition_gain_for_span(selection.start(), selection.end()),
                );
                if self.protected_playmark_extraction_needs_primary_source(request.source_path()) {
                    self.request_protected_extraction_target_source(
                        target.pending_protected_extraction_action(),
                    );
                    emit_gui_action(
                        action,
                        Some("waveform"),
                        None,
                        "blocked",
                        started_at,
                        Some("target_source_required"),
                    );
                    return;
                }
                let request = match self.route_playmarked_extraction_request(request, target) {
                    Ok(request) => request,
                    Err(error) => {
                        self.flash_denied_waveform_selection_for_error(
                            &error,
                            Some(selection),
                            WaveformSelectionKind::Play,
                        );
                        self.ui.status.sample = error.clone();
                        emit_gui_action(
                            action,
                            Some("waveform"),
                            None,
                            "blocked",
                            started_at,
                            Some(&error),
                        );
                        return;
                    }
                };
                let playback_type =
                    ExtractedFilePlaybackType::from_loop_active(self.audio.loop_playback);
                if let Err(error) = self.validate_waveform_extraction_target(&request) {
                    self.flash_denied_waveform_selection_for_error(
                        &error,
                        Some(request.selection()),
                        WaveformSelectionKind::Play,
                    );
                    self.ui.status.sample = error.clone();
                    emit_gui_action(
                        action,
                        Some("waveform"),
                        None,
                        "blocked",
                        started_at,
                        Some(&error),
                    );
                    return;
                }
                self.ui.status.sample = String::from(target.status_text());
                context.business().interactive("gui-waveform-extract").run(
                    move |_| execute_waveform_extraction(request),
                    move |completion| GuiMessage::PlaySelectionExtractionFinished {
                        completion,
                        drag_position: None,
                        playback_type,
                        harvest_operation: HarvestDerivationOperation::Extract,
                        focus_derivative: matches!(
                            target,
                            PlaymarkedExtractionTarget::HarvestDestination
                        ),
                        started_at,
                    },
                );
            }
            Err(error) => {
                self.ui.status.sample = error.clone();
                emit_gui_action(
                    action,
                    Some("waveform"),
                    None,
                    "error",
                    started_at,
                    Some(&error),
                );
            }
        }
    }

    fn extract_selected_whole_files_to_harvest(
        &mut self,
        context: &mut radiant::prelude::UiUpdateContext<GuiMessage>,
        started_at: Instant,
    ) {
        let action = "browser.extract_selected_whole_files_to_harvest";
        let sources = self.library.folder_browser.selected_file_paths();
        if sources.is_empty() {
            let error = String::from("Mark a play range or select samples before extracting");
            self.ui.status.sample = error.clone();
            emit_gui_action(
                action,
                Some("browser"),
                None,
                "blocked",
                started_at,
                Some(&error),
            );
            return;
        }
        if sources
            .iter()
            .any(|source_path| self.protected_extraction_needs_target_source(source_path))
        {
            self.request_protected_extraction_target_source(
                PendingProtectedExtractionAction::ExtractPlaymarkedRange,
            );
            emit_gui_action(
                action,
                Some("browser"),
                None,
                "blocked",
                started_at,
                Some("target_source_required"),
            );
            return;
        }
        let request = match self.selected_whole_file_harvest_extraction_request(sources) {
            Ok(request) => request,
            Err(error) => {
                self.ui.status.sample = error.clone();
                emit_gui_action(
                    action,
                    Some("browser"),
                    None,
                    "blocked",
                    started_at,
                    Some(&error),
                );
                return;
            }
        };
        let count = request.copies.len();
        self.ui.status.sample = format!(
            "Extracting {count} selected {} to Harvest",
            if count == 1 { "file" } else { "files" }
        );
        context
            .business()
            .blocking_io("gui-whole-file-harvest-extract")
            .run(
                move |_| execute_whole_file_harvest_extraction(request),
                move |result| GuiMessage::SelectedWholeFilesHarvestExtractionFinished {
                    started_at,
                    result,
                },
            );
    }

    fn selected_whole_file_harvest_extraction_request(
        &self,
        sources: Vec<PathBuf>,
    ) -> Result<WholeFileHarvestExtractionRequest, String> {
        let mut copies = Vec::with_capacity(sources.len());
        for source_path in sources {
            let target_folder = self.harvest_destination_for_origin(&source_path)?;
            if let Some(error) = self
                .library
                .folder_browser
                .folder_target_lock_error(&target_folder, "Extraction")
            {
                return Err(error);
            }
            copies.push(WholeFileHarvestExtractionPlan {
                operation: self.harvest_copy_operation_for_target_folder(&target_folder),
                source_path,
                target_folder,
            });
        }
        Ok(WholeFileHarvestExtractionRequest { copies })
    }

    fn harvest_copy_operation_for_target_folder(
        &self,
        target_folder: &Path,
    ) -> HarvestDerivationOperation {
        let Some((child_source, _)) = self
            .library
            .folder_browser
            .sample_source_for_file_path(target_folder)
        else {
            return HarvestDerivationOperation::Copy;
        };
        if child_source.is_primary() {
            HarvestDerivationOperation::CopyToPrimary
        } else {
            HarvestDerivationOperation::Copy
        }
    }

    fn route_playmarked_extraction_request(
        &self,
        request: WaveformExtractionRequest,
        target: PlaymarkedExtractionTarget,
    ) -> Result<WaveformExtractionRequest, String> {
        match target {
            PlaymarkedExtractionTarget::Default => self.route_harvest_extraction_request(request),
            PlaymarkedExtractionTarget::HarvestDestination => {
                self.route_harvest_destination_extraction_request(request)
            }
        }
    }

    pub(in crate::native_app) fn route_harvest_extraction_request(
        &self,
        request: WaveformExtractionRequest,
    ) -> Result<WaveformExtractionRequest, String> {
        if let Some(target_folder) =
            self.harvest_destination_for_protected_origin(request.source_path())?
        {
            wavecrate::sample_sources::harvest_file_ops::ensure_dir(
                &target_folder,
                "Could not create harvest destination",
            )?;
            return Ok(request.with_target_folder(target_folder));
        }

        if !request.has_explicit_target_folder() {
            return Ok(request);
        }
        let target_folder = request.target_folder()?;
        if !self
            .library
            .folder_browser
            .path_is_in_protected_source(target_folder)
        {
            return Ok(request);
        }
        let target_source = self
            .library
            .folder_browser
            .default_writable_extraction_source(
                "Set a Primary source before extracting into a protected source",
            )?;
        let target_folder = target_source.primary_import_path();
        wavecrate::sample_sources::harvest_file_ops::ensure_dir(
            &target_folder,
            "Could not create harvest destination",
        )?;
        Ok(request.with_target_folder(target_folder))
    }

    pub(in crate::native_app) fn route_harvest_destination_extraction_request(
        &self,
        request: WaveformExtractionRequest,
    ) -> Result<WaveformExtractionRequest, String> {
        let target_folder =
            match self.harvest_destination_for_protected_origin(request.source_path())? {
                Some(target_folder) => target_folder,
                None => self.harvest_destination_for_origin(request.source_path())?,
            };
        wavecrate::sample_sources::harvest_file_ops::ensure_dir(
            &target_folder,
            "Could not create harvest destination",
        )?;
        Ok(request.with_target_folder(target_folder))
    }

    pub(in crate::native_app) fn validate_waveform_extraction_target(
        &self,
        request: &WaveformExtractionRequest,
    ) -> Result<(), String> {
        let target_folder = request.target_folder()?;
        if let Some(error) = self
            .library
            .folder_browser
            .folder_target_lock_error(target_folder, "Extraction")
        {
            return Err(error);
        }
        Ok(())
    }

    pub(in crate::native_app) fn finish_play_selection_extraction(
        &mut self,
        completion: WaveformExtractionCompletion,
        drag_position: Option<Point>,
        playback_type: ExtractedFilePlaybackType,
        harvest_operation: HarvestDerivationOperation,
        focus_derivative: bool,
        started_at: Instant,
        context: &mut radiant::prelude::UiUpdateContext<GuiMessage>,
    ) {
        match completion.result {
            Ok(path) => {
                self.evict_waveform_cache_path(&path);
                self.waveform
                    .current
                    .mark_extracted_play_selection(&completion.source_path, completion.selection);
                self.waveform.current.flash_play_selection();
                let cross_source_derivative =
                    self.extraction_derivative_crosses_sources(&completion.source_path, &path);
                let protected_origin = self
                    .library
                    .folder_browser
                    .path_is_in_protected_source(&completion.source_path);
                self.flash_primary_source_acceptance_for_protected_extraction(
                    &completion.source_path,
                    &path,
                );
                let focus_derivative = focus_derivative && cross_source_derivative;
                self.log_sample_identity_checkpoint(
                    "waveform.extract.finished_after_refresh",
                    "finish_play_selection_extraction",
                    Some(&path),
                    Some(if protected_origin && cross_source_derivative {
                        "protected_origin_cross_source_derivative"
                    } else if cross_source_derivative {
                        "cross_source_derivative"
                    } else {
                        "same_source_derivative"
                    }),
                );
                let metadata_error = self
                    .assign_extracted_file_metadata(&path, playback_type, context)
                    .err();
                if let Some(request) = self.harvest_selection_derivation_request(
                    &completion.source_path,
                    &path,
                    completion.selection,
                    self.waveform.current.duration_seconds() as f64,
                    harvest_operation,
                ) {
                    self.background.harvest_selection_derivation.enqueue(request);
                    self.background
                        .harvest_selection_derivation
                        .schedule_if_idle(context);
                }
                if let Some(position) = drag_position {
                    self.log_sample_identity_checkpoint(
                        "waveform.extract.drag_started",
                        "finish_play_selection_extraction",
                        Some(&path),
                        Some("drag_position_present"),
                    );
                    self.library
                        .folder_browser
                        .begin_extracted_file_drag(path.clone(), position);
                    self.arm_browser_drag_without_handoff_rating(context);
                    let label = sample_path_label(&path);
                    self.ui.status.sample = match metadata_error.as_ref() {
                        Some(error) => {
                            format!("Dragging {label}; extracted metadata incomplete: {error}")
                        }
                        None => format!("Dragging {label}"),
                    };
                    emit_gui_action(
                        "waveform.selection_drag.start",
                        Some("waveform"),
                        None,
                        "success",
                        started_at,
                        None,
                    );
                } else {
                    if focus_derivative {
                        self.log_sample_identity_checkpoint(
                            "waveform.extract.focused_derivative",
                            "finish_play_selection_extraction",
                            Some(&path),
                            Some("auto_focused_and_load_queued"),
                        );
                    } else if protected_origin {
                        self.log_sample_identity_checkpoint(
                            "waveform.extract.protected_derivative_left_unfocused",
                            "finish_play_selection_extraction",
                            Some(&path),
                            Some("preserve_protected_source_focus"),
                        );
                    } else {
                        self.log_sample_identity_checkpoint(
                            "waveform.extract.same_source_derivative_left_unfocused",
                            "finish_play_selection_extraction",
                            Some(&path),
                            Some("current_behavior_no_auto_focus"),
                        );
                    }
                    let label = sample_path_label(&path);
                    self.ui.status.sample = match metadata_error.as_ref() {
                        Some(error) => {
                            format!("Extracted {label}; extracted metadata incomplete: {error}")
                        }
                        None => format!("Extracted {label}"),
                    };
                    emit_gui_action(
                        "waveform.extract_playmarked_range",
                        Some("waveform"),
                        Some(&label),
                        "success",
                        started_at,
                        None,
                    );
                }
                self.queue_partially_committed_file_mutation(
                    FileMutationOperation::Extract,
                    vec![if focus_derivative && drag_position.is_none() {
                        FileMutationChange::created(path.clone()).with_projection(
                            FileMutationProjection::FocusAndLoad {
                                path,
                                reason: BrowserListingRevealReason::LoadedFileFocus,
                            },
                        )
                    } else {
                        FileMutationChange::created(path)
                    }],
                    metadata_error
                        .into_iter()
                        .map(|error| (None, error))
                        .collect(),
                    context,
                );
            }
            Err(error) => {
                self.record_failed_file_mutation(
                    FileMutationOperation::Extract,
                    None,
                    error.clone(),
                    context,
                );
                let action = if drag_position.is_some() {
                    "waveform.selection_drag.start"
                } else {
                    "waveform.extract_playmarked_range"
                };
                self.ui.status.sample = error.clone();
                emit_gui_action(
                    action,
                    Some("waveform"),
                    None,
                    "error",
                    started_at,
                    Some(&error),
                );
            }
        }
    }

    pub(in crate::native_app) fn finish_selected_whole_files_harvest_extraction(
        &mut self,
        started_at: Instant,
        result: WholeFileHarvestExtractionResult,
        context: &mut radiant::prelude::UiUpdateContext<GuiMessage>,
    ) {
        let action = "browser.extract_selected_whole_files_to_harvest";
        for copy in &result.copied {
            self.record_harvest_whole_file_derivation(
                &copy.source_path,
                &copy.output_path,
                copy.operation.clone(),
            );
        }

        let copied_count = result.copied.len();
        let failed_count = result.failed.len();
        if result.copied.iter().any(|copy| {
            self.protected_extraction_was_accepted_by_primary(&copy.source_path, &copy.output_path)
        }) {
            self.library
                .folder_browser
                .flash_primary_source_acceptance();
        }
        self.queue_partially_committed_file_mutation(
            FileMutationOperation::Extract,
            result
                .copied
                .iter()
                .map(|copy| FileMutationChange::created(copy.output_path.clone()))
                .collect(),
            result
                .failed
                .iter()
                .map(|failure| {
                    (
                        None,
                        format!(
                            "{}: {}",
                            sample_path_label(&failure.source_path),
                            failure.error
                        ),
                    )
                })
                .collect(),
            context,
        );
        if copied_count == 0 {
            let error = result
                .failed
                .first()
                .map(|failure| {
                    format!(
                        "Whole-file extraction failed for {}: {}",
                        sample_path_label(&failure.source_path),
                        failure.error
                    )
                })
                .unwrap_or_else(|| String::from("No selected files were extracted"));
            self.ui.status.sample = error.clone();
            emit_gui_action(
                action,
                Some("browser"),
                None,
                "error",
                started_at,
                Some(&error),
            );
            return;
        }

        self.ui.status.sample = if failed_count == 0 {
            format!(
                "Extracted {copied_count} selected {} to Harvest",
                if copied_count == 1 { "file" } else { "files" }
            )
        } else {
            format!(
                "Extracted {copied_count} selected {} to Harvest; {failed_count} failed",
                if copied_count == 1 { "file" } else { "files" }
            )
        };
        let label = if copied_count == 1 {
            result
                .copied
                .first()
                .map(|copy| sample_path_label(&copy.output_path))
        } else {
            Some(format!("{copied_count} files"))
        };
        emit_gui_action(
            action,
            Some("browser"),
            label.as_deref(),
            if failed_count == 0 {
                "success"
            } else {
                "partial"
            },
            started_at,
            result.failed.first().map(|failure| failure.error.as_str()),
        );
    }

    fn extraction_derivative_crosses_sources(
        &self,
        source_path: &std::path::Path,
        child_path: &std::path::Path,
    ) -> bool {
        let source_id = self
            .library
            .folder_browser
            .sample_source_for_file_path(source_path)
            .map(|(source, _)| source.id);
        let child_id = self
            .library
            .folder_browser
            .sample_source_for_file_path(child_path)
            .map(|(source, _)| source.id);
        source_id.is_some() && child_id.is_some() && source_id != child_id
    }

    fn flash_primary_source_acceptance_for_protected_extraction(
        &mut self,
        source_path: &std::path::Path,
        child_path: &std::path::Path,
    ) {
        if self.protected_extraction_was_accepted_by_primary(source_path, child_path) {
            self.library
                .folder_browser
                .flash_primary_source_acceptance();
        }
    }

    fn protected_extraction_was_accepted_by_primary(
        &self,
        source_path: &std::path::Path,
        child_path: &std::path::Path,
    ) -> bool {
        self.library
            .folder_browser
            .path_is_in_protected_source(source_path)
            && self
                .library
                .folder_browser
                .sample_source_for_file_path(child_path)
                .is_some_and(|(source, _)| source.role == SourceRole::Primary)
    }
}
