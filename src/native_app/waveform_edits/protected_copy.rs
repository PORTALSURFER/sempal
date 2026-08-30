use std::{
    path::{Path, PathBuf},
    time::Instant,
};

use radiant::prelude as ui;
use radiant::prelude::PlatformResultExt as _;
use wavecrate::sample_sources::HarvestDerivationOperation;

use crate::native_app::app::{
    ExtractedFilePlaybackType, GuiMessage, NativeAppState, PendingProtectedExtractionAction,
    PendingProtectedExtractionTargetSource, PendingWaveformDestructiveEdit,
    WaveformDestructiveEditKind, WaveformDestructiveEditTarget,
};
use crate::native_app::waveform::execute_waveform_extraction;

use super::prompt::destructive_edit_prompt;
use super::queue::WaveformDestructiveEditQueueOptions;
pub(super) fn harvest_region_copy_operation(
    kind: WaveformDestructiveEditKind,
) -> Option<HarvestDerivationOperation> {
    match kind {
        WaveformDestructiveEditKind::CropSelection => Some(HarvestDerivationOperation::CropCopy),
        WaveformDestructiveEditKind::ExtractAndTrimSelection => {
            Some(HarvestDerivationOperation::Extract)
        }
        WaveformDestructiveEditKind::TrimSelection
        | WaveformDestructiveEditKind::ReverseSelection
        | WaveformDestructiveEditKind::MuteSelection
        | WaveformDestructiveEditKind::ApplyEditSelectionEffects
        | WaveformDestructiveEditKind::SlideSampleAudio { .. } => None,
    }
}

pub(super) fn harvest_whole_file_copy_operation(
    kind: WaveformDestructiveEditKind,
) -> Option<HarvestDerivationOperation> {
    match kind {
        WaveformDestructiveEditKind::TrimSelection => Some(HarvestDerivationOperation::TrimCopy),
        WaveformDestructiveEditKind::ReverseSelection => {
            Some(HarvestDerivationOperation::ReverseCopy)
        }
        WaveformDestructiveEditKind::MuteSelection => Some(HarvestDerivationOperation::EditCopy),
        WaveformDestructiveEditKind::ApplyEditSelectionEffects => {
            Some(HarvestDerivationOperation::EditCopy)
        }
        WaveformDestructiveEditKind::SlideSampleAudio { .. } => {
            Some(HarvestDerivationOperation::SlideCopy)
        }
        WaveformDestructiveEditKind::CropSelection
        | WaveformDestructiveEditKind::ExtractAndTrimSelection => None,
    }
}

impl NativeAppState {
    pub(super) fn queue_harvest_region_copy_request(
        &mut self,
        kind: WaveformDestructiveEditKind,
        target: WaveformDestructiveEditTarget,
        harvest_operation: HarvestDerivationOperation,
        started_at: Instant,
        context: &mut ui::UiUpdateContext<GuiMessage>,
    ) -> Result<bool, String> {
        let (absolute_path, selection) = self.destructive_edit_target_for_kind(kind, target)?;
        if !self.harvest_copy_edits_enabled_for_path(&absolute_path) {
            return Ok(false);
        }
        if self.protected_extraction_needs_target_source(&absolute_path) {
            self.request_protected_extraction_target_source(
                PendingProtectedExtractionAction::WaveformDestructiveEdit { kind, target },
            );
            return Ok(true);
        }
        let request = self
            .waveform
            .current
            .selection_extraction_request(None, selection)?
            .with_gain(self.normalized_audition_gain_for_span(selection.start(), selection.end()));
        let request = self.route_harvest_destination_extraction_request(request)?;
        self.validate_waveform_extraction_target(&request)?;
        let playback_type = ExtractedFilePlaybackType::from_loop_active(self.audio.loop_playback);
        self.ui.status.sample = String::from("Extracting selection to harvest destination");
        context.business().background("gui-waveform-extract").run(
            move |_| execute_waveform_extraction(request),
            move |completion| GuiMessage::PlaySelectionExtractionFinished {
                completion,
                drag_position: None,
                playback_type,
                harvest_operation,
                focus_derivative: true,
                started_at,
            },
        );
        Ok(true)
    }

    pub(super) fn queue_harvest_whole_file_copy_edit_request(
        &mut self,
        kind: WaveformDestructiveEditKind,
        target: WaveformDestructiveEditTarget,
        harvest_operation: HarvestDerivationOperation,
        context: &mut ui::UiUpdateContext<GuiMessage>,
    ) -> Result<bool, String> {
        let (source_path, selection) = self.destructive_edit_target_for_kind(kind, target)?;
        if !self.harvest_copy_edits_enabled_for_path(&source_path) {
            return Ok(false);
        }
        let target_folder = self.harvest_destination_for_origin(&source_path)?;
        if let Some(error) = self
            .library
            .folder_browser
            .folder_target_lock_error(&target_folder, kind.action_label())
        {
            return Err(error);
        }
        let Some(primary_source) = self.library.folder_browser.primary_sample_source() else {
            return Err(String::from(
                "Set a Primary source before editing to a harvest destination",
            ));
        };
        let child_path = next_copy_edit_path(&source_path, &target_folder, kind)?;
        let relative_path = child_path
            .strip_prefix(&primary_source.root)
            .map(Path::to_path_buf)
            .map_err(|_| String::from("Harvest edit copy is outside the Primary source"))?;
        let request = PendingWaveformDestructiveEdit {
            prompt: destructive_edit_prompt(kind, &self.ui.settings.persisted.audio_write_format),
            source: primary_source,
            relative_path,
            absolute_path: child_path.clone(),
            selection,
        };
        self.queue_destructive_edit_request_with_options(
            request,
            WaveformDestructiveEditQueueOptions {
                copy_source_path: Some(source_path.clone()),
                output_focus_path: Some(child_path),
                harvest_whole_file_derivation: Some((source_path, harvest_operation)),
            },
            context,
        )
        .map_err(|error| error.to_string())?;
        Ok(true)
    }

    fn harvest_copy_edits_enabled_for_path(&self, source_path: &Path) -> bool {
        self.library
            .folder_browser
            .sample_source_for_file_path(source_path)
            .is_some_and(|(source, _)| {
                source.is_protected() || self.library.folder_browser.harvest_filter().is_some()
            })
    }

    pub(in crate::native_app) fn protected_extraction_needs_target_source(
        &self,
        source_path: &Path,
    ) -> bool {
        let protected_origin = self
            .library
            .folder_browser
            .sample_source_for_file_path(source_path)
            .is_some_and(|(source, _)| source.is_protected());
        protected_origin
            && self
                .library
                .folder_browser
                .default_writable_extraction_source(
                    "Set a Primary source before extracting from a protected source",
                )
                .is_err()
    }

    pub(in crate::native_app) fn request_protected_extraction_target_source(
        &mut self,
        action: PendingProtectedExtractionAction,
    ) {
        let prompt = PendingProtectedExtractionTargetSource {
            action,
            title: String::from("Target source required"),
            message: String::from(
                "This sample is in a protected source. Add a writable target source and Wavecrate will continue the extraction there.",
            ),
        };
        self.ui
            .browser_interaction
            .pending_protected_extraction_target_source = Some(prompt);
        self.ui.status.sample =
            String::from("Add a writable target source before extracting from a protected source");
    }

    pub(in crate::native_app) fn add_protected_extraction_target_source(
        &mut self,
        context: &mut ui::UiUpdateContext<GuiMessage>,
    ) {
        if self
            .ui
            .browser_interaction
            .pending_protected_extraction_target_source
            .is_none()
        {
            return;
        }
        context.pick_folder(
            ui::FileDialogRequest::new().title("Add target source"),
            GuiMessage::ProtectedExtractionTargetSourceDialogFinished,
        );
    }

    pub(in crate::native_app) fn finish_protected_extraction_target_source_dialog(
        &mut self,
        result: ui::PlatformResult,
        context: &mut ui::UiUpdateContext<GuiMessage>,
    ) {
        let started_at = Instant::now();
        let path = match result.into_path_or_canceled() {
            Ok(Some(path)) => path,
            Ok(None) => return,
            Err(error) => {
                self.ui.status.sample = format!("Add target source failed: {error}");
                return;
            }
        };
        let Some(source_id) =
            self.queue_add_source_path_preserving_selection(path, started_at, context)
        else {
            self.ui.status.sample = String::from("Add target source failed: source was not queued");
            return;
        };
        if let Err(error) = self.library.folder_browser.set_primary_source(&source_id) {
            self.ui.status.sample = format!("Add target source failed: {error}");
            return;
        }
        self.sync_source_watcher();
        self.retry_pending_protected_extraction(context);
    }

    pub(in crate::native_app) fn cancel_protected_extraction_target_source(&mut self) {
        self.ui
            .browser_interaction
            .pending_protected_extraction_target_source = None;
        self.ui.status.sample = String::from("Protected extraction cancelled");
    }

    fn retry_pending_protected_extraction(
        &mut self,
        context: &mut ui::UiUpdateContext<GuiMessage>,
    ) {
        let Some(pending) = self
            .ui
            .browser_interaction
            .pending_protected_extraction_target_source
            .take()
        else {
            return;
        };
        match pending.action {
            PendingProtectedExtractionAction::WaveformDestructiveEdit { kind, target } => {
                let Some(harvest_operation) = harvest_region_copy_operation(kind) else {
                    self.ui.status.sample =
                        String::from("Protected extraction target is unavailable");
                    return;
                };
                match self.queue_harvest_region_copy_request(
                    kind,
                    target,
                    harvest_operation,
                    Instant::now(),
                    context,
                ) {
                    Ok(true) => {}
                    Ok(false) => {
                        self.ui.status.sample =
                            String::from("Protected extraction target is unavailable");
                    }
                    Err(error) => {
                        self.flash_denied_destructive_selection_for_error(&error, kind, target);
                        self.ui.status.sample =
                            self.denied_destructive_edit_status(&error, kind, target);
                    }
                }
            }
            PendingProtectedExtractionAction::ExtractPlaymarkedRange => {
                self.extract_playmarked_range(context);
            }
            PendingProtectedExtractionAction::ExtractPlaymarkedRangeToHarvestDestination => {
                self.extract_playmarked_range_to_harvest_destination(context);
            }
        }
    }
}

fn next_copy_edit_path(
    source_path: &Path,
    target_folder: &Path,
    kind: WaveformDestructiveEditKind,
) -> Result<PathBuf, String> {
    let base_suffix = match kind {
        WaveformDestructiveEditKind::TrimSelection => "_trim",
        WaveformDestructiveEditKind::ReverseSelection => "_reverse",
        WaveformDestructiveEditKind::MuteSelection => "_mute",
        WaveformDestructiveEditKind::ApplyEditSelectionEffects => "_edit",
        WaveformDestructiveEditKind::SlideSampleAudio { .. } => "_slide",
        WaveformDestructiveEditKind::CropSelection
        | WaveformDestructiveEditKind::ExtractAndTrimSelection => {
            return Err(String::from("Unsupported protected copy edit"));
        }
    };
    wavecrate::sample_sources::harvest_file_ops::next_available_wav_copy_path(
        source_path,
        target_folder,
        base_suffix,
        "Could not find an available edit copy file name",
    )
}
