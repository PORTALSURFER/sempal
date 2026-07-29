use std::{
    path::{Path, PathBuf},
    time::Instant,
};

use radiant::prelude as ui;
use radiant::prelude::PlatformResultExt as _;
use wavecrate::sample_sources::HarvestDerivationOperation;
use wavecrate::selection::SelectionRange;

use crate::native_app::app::{
    ClipboardHandoffTarget, ExtractedFilePlaybackType, GuiMessage, NativeAppState, emit_gui_action,
    sample_path_label,
};
use crate::native_app::sample_library::committed_file_mutations::{
    FileMutationChange, FileMutationOperation,
};
use crate::native_app::waveform::{
    WaveformExtractionCompletion, WaveformSelectionKind, execute_waveform_extraction,
};

const WAVEFORM_CLIPBOARD_HANDOFF_TASK_NAME: &str = "gui-copy-waveform-selection";

impl NativeAppState {
    pub(in crate::native_app) fn copy_selected_files(
        &mut self,
        context: &mut ui::UiUpdateContext<GuiMessage>,
    ) {
        let started_at = Instant::now();
        if self.copy_waveform_play_selection_if_requested(started_at, context) {
            return;
        }

        let paths = self.library.folder_browser.selected_file_paths();
        if paths.is_empty() {
            self.ui.status.sample = String::from("Select files before copying");
            emit_gui_action(
                "browser.copy_selected_files",
                Some("browser"),
                None,
                "skipped",
                started_at,
                Some("no selection"),
            );
            return;
        }

        let count = paths.len();
        self.yield_sample_cache_warm_for_user_handoff(context);
        self.library.folder_browser.flash_copied_file_paths(&paths);
        self.waveform.current.flash_copied_file();
        self.ui.status.sample = match count {
            1 => String::from("Copying selected file"),
            count => format!("Copying {count} selected files"),
        };
        context.copy_file_paths(paths, move |result| GuiMessage::SelectedFilesCopyFinished {
            count,
            started_at,
            result: result.into_completed(),
        });
    }

    pub(in crate::native_app) fn finish_copy_selected_files(
        &mut self,
        count: usize,
        started_at: Instant,
        result: Result<(), String>,
    ) {
        match result {
            Ok(()) => {
                self.ui.status.sample = match count {
                    1 => String::from("Copied selected file"),
                    count => format!("Copied {count} selected files"),
                };
                emit_gui_action(
                    "browser.copy_selected_files",
                    Some("browser"),
                    None,
                    "success",
                    started_at,
                    None,
                );
            }
            Err(error) => {
                self.ui.status.sample = format!("Copy failed: {error}");
                emit_gui_action(
                    "browser.copy_selected_files",
                    Some("browser"),
                    None,
                    "error",
                    started_at,
                    Some(&error),
                );
            }
        }
    }

    fn copy_waveform_play_selection_if_requested(
        &mut self,
        started_at: Instant,
        context: &mut ui::UiUpdateContext<GuiMessage>,
    ) -> bool {
        if self.ui.browser_interaction.clipboard_handoff_target
            != ClipboardHandoffTarget::WaveformSelection
        {
            return false;
        }
        if self.waveform.current.play_selection().is_none() {
            return false;
        }
        let target_folder = match self.library.folder_browser.selected_folder_path() {
            Some(target_folder) => target_folder,
            None => {
                let error = String::from("Select a folder before copying a range");
                self.flash_denied_waveform_selection_for_error(
                    &error,
                    self.waveform.current.play_selection(),
                    WaveformSelectionKind::Play,
                );
                self.ui.status.sample = error.clone();
                emit_gui_action(
                    "waveform.copy_playmarked_range",
                    Some("waveform"),
                    None,
                    "error",
                    started_at,
                    Some(&error),
                );
                return true;
            }
        };
        if let Some(error) = self
            .library
            .folder_browser
            .folder_target_lock_error(&target_folder, "Extraction")
        {
            self.flash_denied_waveform_selection_for_error(
                &error,
                self.waveform.current.play_selection(),
                WaveformSelectionKind::Play,
            );
            self.ui.status.sample = error.clone();
            emit_gui_action(
                "waveform.copy_playmarked_range",
                Some("waveform"),
                None,
                "blocked",
                started_at,
                Some(&error),
            );
            return true;
        }
        let request = match self
            .waveform
            .current
            .play_selection_extraction_request(Some(target_folder))
        {
            Ok(request) => request,
            Err(error) => {
                self.ui.status.sample = format!("Copy failed: {error}");
                emit_gui_action(
                    "waveform.copy_playmarked_range",
                    Some("waveform"),
                    None,
                    "error",
                    started_at,
                    Some(&error),
                );
                return true;
            }
        };
        let selection = request.selection();
        let request = request
            .with_gain(self.normalized_audition_gain_for_span(selection.start(), selection.end()));
        let request = match self.route_harvest_extraction_request(request) {
            Ok(request) => request,
            Err(error) => {
                self.flash_denied_waveform_selection_for_error(
                    &error,
                    Some(selection),
                    WaveformSelectionKind::Play,
                );
                self.ui.status.sample = error.clone();
                emit_gui_action(
                    "waveform.copy_playmarked_range",
                    Some("waveform"),
                    None,
                    "blocked",
                    started_at,
                    Some(&error),
                );
                return true;
            }
        };
        let playback_type = ExtractedFilePlaybackType::from_loop_active(self.audio.loop_playback);
        self.waveform.current.flash_play_selection();
        self.yield_sample_cache_warm_for_user_handoff(context);
        self.ui.status.sample = String::from("Extracting play range for copy");
        context
            .business()
            .interactive(WAVEFORM_CLIPBOARD_HANDOFF_TASK_NAME)
            .run(
                move |_| execute_waveform_extraction(request),
                move |completion| GuiMessage::WaveformSelectionCopyExtracted {
                    completion,
                    playback_type,
                    started_at,
                },
            );
        true
    }

    pub(in crate::native_app) fn finish_waveform_selection_copy_extracted(
        &mut self,
        completion: WaveformExtractionCompletion,
        playback_type: ExtractedFilePlaybackType,
        started_at: Instant,
        context: &mut ui::UiUpdateContext<GuiMessage>,
    ) {
        let source_path = completion.source_path;
        let selection = completion.selection;
        match completion.result {
            Ok(path) => {
                self.evict_waveform_cache_path(&path);
                self.waveform
                    .current
                    .mark_extracted_play_selection(&source_path, selection);
                self.waveform.current.flash_play_selection();
                let source_duration_seconds = self.waveform.current.duration_seconds() as f64;
                let copied_path = path.clone();
                let label = sample_path_label(&path);
                self.ui.status.sample = format!("Copying extracted {label}");
                context.copy_file_paths(vec![path], move |result| {
                    GuiMessage::WaveformSelectionCopyFinished {
                        source_path,
                        selection,
                        copied_path,
                        playback_type,
                        source_duration_seconds,
                        started_at,
                        result: result.into_completed(),
                    }
                });
            }
            Err(error) => {
                self.finish_waveform_selection_copy(
                    source_path,
                    selection,
                    PathBuf::new(),
                    playback_type,
                    self.waveform.current.duration_seconds() as f64,
                    started_at,
                    Err(error),
                    context,
                );
            }
        }
    }

    pub(in crate::native_app) fn finish_waveform_selection_copy(
        &mut self,
        source_path: PathBuf,
        selection: SelectionRange,
        copied_path: PathBuf,
        playback_type: ExtractedFilePlaybackType,
        source_duration_seconds: f64,
        started_at: Instant,
        result: Result<(), String>,
        context: &mut ui::UiUpdateContext<GuiMessage>,
    ) {
        match result {
            Ok(()) => {
                let metadata_error = self.finish_waveform_selection_copy_bookkeeping(
                    &source_path,
                    selection,
                    &copied_path,
                    playback_type,
                    source_duration_seconds,
                    context,
                );
                let label = sample_path_label(&copied_path);
                self.waveform
                    .current
                    .flash_play_selection_if_current(&source_path, selection);
                self.ui.status.sample = match metadata_error.as_ref() {
                    Some(error) => {
                        format!("Copied {label}; extracted metadata incomplete: {error}")
                    }
                    None => format!("Copied {label} to clipboard"),
                };
                emit_gui_action(
                    "waveform.copy_playmarked_range",
                    Some("waveform"),
                    Some(&label),
                    "success",
                    started_at,
                    None,
                );
                self.queue_partially_committed_file_mutation(
                    FileMutationOperation::Extract,
                    vec![FileMutationChange::created(copied_path)],
                    metadata_error
                        .into_iter()
                        .map(|error| (None, error))
                        .collect(),
                    context,
                );
            }
            Err(error) => {
                if copied_path.as_os_str().is_empty() {
                    self.record_failed_file_mutation(
                        FileMutationOperation::Extract,
                        None,
                        error.clone(),
                        context,
                    );
                } else {
                    let metadata_error = self.finish_waveform_selection_copy_bookkeeping(
                        &source_path,
                        selection,
                        &copied_path,
                        playback_type,
                        source_duration_seconds,
                        context,
                    );
                    let mut failures = vec![(None, error.clone())];
                    failures.extend(metadata_error.map(|error| (None, error)));
                    self.queue_partially_committed_file_mutation(
                        FileMutationOperation::Extract,
                        vec![FileMutationChange::created(copied_path.clone())],
                        failures,
                        context,
                    );
                }
                self.ui.status.sample = format!("Copy failed: {error}");
                emit_gui_action(
                    "waveform.copy_playmarked_range",
                    Some("waveform"),
                    Some(&format!(
                        "{} {:.1}-{:.1}%",
                        sample_path_label(&source_path),
                        selection.start() * 100.0,
                        selection.end() * 100.0
                    )),
                    "error",
                    started_at,
                    Some(&error),
                );
            }
        }
    }

    fn finish_waveform_selection_copy_bookkeeping(
        &mut self,
        source_path: &Path,
        selection: SelectionRange,
        copied_path: &Path,
        playback_type: ExtractedFilePlaybackType,
        source_duration_seconds: f64,
        context: &mut ui::UiUpdateContext<GuiMessage>,
    ) -> Option<String> {
        let started_at = Instant::now();
        let protected_origin = self
            .library
            .folder_browser
            .path_is_in_protected_source(source_path);
        let metadata_error = self
            .assign_extracted_file_metadata(copied_path, playback_type, context)
            .err();
        self.record_harvest_selection_derivation_with_source_duration(
            source_path,
            selection,
            copied_path,
            source_duration_seconds,
            HarvestDerivationOperation::Export,
        );
        let elapsed = started_at.elapsed();
        tracing::debug!(
            target: "wavecrate::waveform_copy",
            source = %source_path.display(),
            copied = %copied_path.display(),
            protected_origin,
            elapsed_ms = elapsed.as_millis(),
            "scheduled waveform selection copy bookkeeping"
        );
        if elapsed >= std::time::Duration::from_millis(16) {
            tracing::warn!(
                target: "wavecrate::waveform_copy",
                source = %source_path.display(),
                copied = %copied_path.display(),
                protected_origin,
                elapsed_ms = elapsed.as_millis(),
                "slow waveform selection copy bookkeeping"
            );
        }
        metadata_error
    }

    pub(in crate::native_app) fn external_drag_completed(
        &mut self,
        result: Result<radiant::runtime::ExternalDragOutcome, String>,
        context: &mut ui::UiUpdateContext<GuiMessage>,
    ) {
        tracing::debug!(
            target: "wavecrate::external_drag",
            event = "external_drag.completed",
            accepted = result.as_ref().is_ok_and(|outcome| outcome.accepted()),
            effect = ?result.as_ref().ok().map(|outcome| outcome.effect),
            error = result.as_ref().err().map(String::as_str).unwrap_or(""),
            "External drag completed"
        );
        let handoff_paths = self
            .ui
            .browser_interaction
            .pending_internal_file_drag_paths
            .iter()
            .cloned()
            .collect::<Vec<_>>();
        let handoff_adds_keep_rating = self
            .ui
            .browser_interaction
            .pending_internal_file_drag_adds_keep_rating;
        context.end_drag();
        self.library.folder_browser.clear_drag();
        self.clear_pending_internal_file_drag_paths();
        self.ui.status.sample = match result {
            Ok(outcome) if outcome.accepted() => match outcome.effect {
                radiant::runtime::ExternalDragEffect::Copy
                | radiant::runtime::ExternalDragEffect::Link => {
                    let rating_error = if handoff_adds_keep_rating {
                        self.add_keep_rating_to_handoff_paths(&handoff_paths, context)
                            .err()
                    } else {
                        None
                    };
                    match (outcome.effect, rating_error) {
                        (radiant::runtime::ExternalDragEffect::Copy, None) => {
                            String::from("Dragged item externally")
                        }
                        (radiant::runtime::ExternalDragEffect::Link, None) => {
                            String::from("Linked item externally")
                        }
                        (radiant::runtime::ExternalDragEffect::Copy, Some(error)) => {
                            format!("Dragged item externally; rating update failed: {error}")
                        }
                        (radiant::runtime::ExternalDragEffect::Link, Some(error)) => {
                            format!("Linked item externally; rating update failed: {error}")
                        }
                        _ => unreachable!("only copy/link outcomes are matched"),
                    }
                }
                radiant::runtime::ExternalDragEffect::Move => String::from("Moved item externally"),
                radiant::runtime::ExternalDragEffect::None => {
                    String::from("External drag cancelled")
                }
            },
            Ok(_) => String::from("External drag cancelled"),
            Err(error) => format!("External drag failed: {error}"),
        };
    }
}
