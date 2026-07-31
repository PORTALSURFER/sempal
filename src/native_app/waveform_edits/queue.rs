use std::path::PathBuf;

use radiant::prelude as ui;
use wavecrate::sample_sources::HarvestDerivationOperation;
use wavecrate::selection::SelectionRange;

use crate::native_app::app::{
    ExtractedFilePlaybackType, GuiMessage, NativeAppState, PendingWaveformDestructiveEdit,
    WaveformDestructiveEditKind, WaveformDestructiveEditTarget, WaveformDestructiveEditUiContext,
    sample_path_label,
};
use crate::native_app::waveform::WaveformSelectionKind;

use super::worker::{self, WaveformDestructiveEditWorkerRequest};

const WAVEFORM_DESTRUCTIVE_EDIT_TASK_NAME: &str = "gui-waveform-destructive-edit";
#[derive(Default)]
pub(super) struct WaveformDestructiveEditQueueOptions {
    pub(super) copy_source_path: Option<PathBuf>,
    pub(super) output_focus_path: Option<PathBuf>,
    pub(super) harvest_whole_file_derivation: Option<(PathBuf, HarvestDerivationOperation)>,
}

impl NativeAppState {
    pub(super) fn queue_destructive_edit_request(
        &mut self,
        request: PendingWaveformDestructiveEdit,
        context: &mut ui::UiUpdateContext<GuiMessage>,
    ) -> Result<(), String> {
        self.queue_destructive_edit_request_with_options(
            request,
            WaveformDestructiveEditQueueOptions::default(),
            context,
        )
    }

    pub(super) fn queue_destructive_edit_request_with_options(
        &mut self,
        request: PendingWaveformDestructiveEdit,
        options: WaveformDestructiveEditQueueOptions,
        context: &mut ui::UiUpdateContext<GuiMessage>,
    ) -> Result<(), String> {
        let recovery_root = self
            .background
            .waveform_recovery_root
            .clone()
            .ok_or_else(|| String::from("waveform recovery root is not ready"))?;
        if let Some(error) = self
            .library
            .folder_browser
            .file_change_lock_error(&request.absolute_path, request.prompt.edit.action_label())
        {
            return Err(error);
        }
        let extraction_request =
            if request.prompt.edit == WaveformDestructiveEditKind::ExtractAndTrimSelection {
                Some(
                    self.waveform
                        .current
                        .selection_extraction_request(None, request.selection)?
                        .with_gain(self.normalized_audition_gain_for_span(
                            request.selection.start(),
                            request.selection.end(),
                        )),
                )
            } else {
                None
            };
        let source_duration_seconds = (self.waveform.current.path() == request.absolute_path)
            .then(|| self.waveform.current.duration_seconds() as f64);
        let playback_was_active = self.waveform.current.is_playing();
        let extracted_playback_type =
            ExtractedFilePlaybackType::from_loop_active(self.audio.loop_playback);
        self.stop_audio_output_playback();
        self.waveform.current.stop_playback();
        self.audio.current_playback_span = None;
        self.audio.pending_playback_start = None;
        self.audio.pending_sample_playback = None;
        self.audio.clear_sample_playback_session();

        let preserved_marks = self.preserved_marks_after_destructive_edit(&request);
        let mut worker_request = WaveformDestructiveEditWorkerRequest::new(
            request.clone(),
            extraction_request,
            recovery_root,
        );
        if let Some(copy_source_path) = options.copy_source_path.clone() {
            worker_request = worker_request.with_copy_source(copy_source_path);
        }
        self.background.waveform_destructive_edit_context =
            Some(WaveformDestructiveEditUiContext {
                request: request.clone(),
                playback_was_active,
                source_duration_seconds,
                extracted_playback_type,
                preserved_marks,
                output_focus_path: options.output_focus_path,
                harvest_whole_file_derivation: options.harvest_whole_file_derivation,
            });
        self.ui.status.sample = format!(
            "{} {}",
            request.prompt.edit.action_label(),
            sample_path_label(&request.absolute_path)
        );
        context
            .business()
            .blocking_io(WAVEFORM_DESTRUCTIVE_EDIT_TASK_NAME)
            .latest(&mut self.background.waveform_destructive_edit_task)
            .run(
                move |_| worker::execute_destructive_edit(worker_request),
                GuiMessage::WaveformDestructiveEditFinished,
            );
        Ok(())
    }

    pub(super) fn flash_denied_destructive_selection_for_error(
        &mut self,
        error: &str,
        kind: WaveformDestructiveEditKind,
        target: WaveformDestructiveEditTarget,
    ) {
        let target_result = self.destructive_edit_target_for_kind(kind, target).ok();
        if let Some((path, _)) = target_result.as_ref() {
            self.flash_protected_source_block_if_error(error, path);
        }
        let selection = target_result.map(|(_, selection)| selection);
        self.flash_denied_waveform_selection_for_error(
            error,
            selection,
            target.fallback_selection_kind(),
        );
    }

    pub(super) fn denied_destructive_edit_status(
        &self,
        error: &str,
        kind: WaveformDestructiveEditKind,
        target: WaveformDestructiveEditTarget,
    ) -> String {
        let Some((path, _)) = self.destructive_edit_target_for_kind(kind, target).ok() else {
            return error.to_string();
        };
        self.protected_source_status_or_error(error, &path)
    }

    pub(in crate::native_app) fn flash_denied_waveform_selection_for_error(
        &mut self,
        error: &str,
        selection: Option<SelectionRange>,
        fallback_kind: WaveformSelectionKind,
    ) {
        if !waveform_action_denied_error(error) {
            return;
        }
        if let Some(selection) = selection {
            self.waveform
                .current
                .flash_denied_selection_matching(selection, fallback_kind);
        } else {
            self.waveform.current.flash_denied_selection(fallback_kind);
        }
    }
}

fn waveform_action_denied_error(error: &str) -> bool {
    error.contains("This source is protected") || error.contains("blocked by locked folder")
}
