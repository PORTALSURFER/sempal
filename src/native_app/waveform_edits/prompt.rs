use radiant::prelude as ui;
use wavecrate::sample_sources::config::AudioWriteFormatConfig;

use crate::native_app::app::{
    GuiMessage, NativeAppState, PendingWaveformDestructiveEdit, WaveformDestructiveEditKind,
    WaveformDestructiveEditTarget,
};
use crate::native_app::waveform::WaveformSelectionKind;
impl NativeAppState {
    pub(in crate::native_app) fn confirm_pending_waveform_destructive_edit(
        &mut self,
        context: &mut ui::UiUpdateContext<GuiMessage>,
    ) {
        let Some(request) = self
            .ui
            .browser_interaction
            .pending_waveform_destructive_edit
            .take()
        else {
            return;
        };

        let denied_selection = request.selection;
        let denied_path = request.absolute_path.clone();
        let result = self.queue_destructive_edit_request(request, context);
        if let Err(error) = result {
            let error = error.to_string();
            self.flash_protected_source_block_if_error(&error, &denied_path);
            self.flash_denied_waveform_selection_for_error(
                &error,
                Some(denied_selection),
                WaveformSelectionKind::Edit,
            );
            self.ui.status.sample = format!(
                "Edit failed: {}",
                self.protected_source_status_or_error(&error, &denied_path)
            );
        }
    }

    pub(in crate::native_app) fn cancel_pending_waveform_destructive_edit(&mut self) {
        self.ui
            .browser_interaction
            .pending_waveform_destructive_edit = None;
    }

    pub(super) fn pending_destructive_edit_request(
        &self,
        kind: WaveformDestructiveEditKind,
        target: WaveformDestructiveEditTarget,
    ) -> Result<PendingWaveformDestructiveEdit, String> {
        let (absolute_path, selection) = self.destructive_edit_target_for_kind(kind, target)?;
        let (source, relative_path) = self
            .library
            .folder_browser
            .sample_source_for_file_path(&absolute_path)
            .ok_or_else(|| String::from("Loaded sample is not inside a configured source"))?;
        if let Some(error) = self
            .library
            .folder_browser
            .file_change_lock_error(&absolute_path, kind.action_label())
        {
            return Err(error);
        }
        Ok(PendingWaveformDestructiveEdit {
            prompt: destructive_edit_prompt(kind, &self.ui.settings.persisted.audio_write_format),
            source,
            relative_path,
            absolute_path,
            selection,
        })
    }
}

impl WaveformDestructiveEditKind {
    pub(super) fn action_label(self) -> &'static str {
        match self {
            Self::CropSelection => "Crop",
            Self::TrimSelection => "Trim",
            Self::ReverseSelection => "Reverse",
            Self::MuteSelection => "Mute",
            Self::ExtractAndTrimSelection => "Extract and trim",
            Self::ApplyEditSelectionEffects => "Apply edit mark edits",
            Self::SlideSampleAudio { .. } => "Slide",
        }
    }

    pub(super) fn gerund_label(self) -> &'static str {
        match self {
            Self::CropSelection => "cropping",
            Self::TrimSelection => "trimming",
            Self::ReverseSelection => "reversing",
            Self::MuteSelection => "muting",
            Self::ExtractAndTrimSelection => "extracting and trimming",
            Self::ApplyEditSelectionEffects => "applying edit mark edits",
            Self::SlideSampleAudio { .. } => "sliding",
        }
    }

    pub(super) fn past_tense_label(self) -> &'static str {
        match self {
            Self::CropSelection => "Cropped",
            Self::TrimSelection => "Trimmed",
            Self::ReverseSelection => "Reversed",
            Self::MuteSelection => "Muted",
            Self::ExtractAndTrimSelection => "Extracted and trimmed",
            Self::ApplyEditSelectionEffects => "Applied edit mark edits to",
            Self::SlideSampleAudio { .. } => "Slid",
        }
    }

    pub(super) fn transaction_label(self) -> &'static str {
        match self {
            Self::CropSelection => "Crop waveform selection",
            Self::TrimSelection => "Trim waveform selection",
            Self::ReverseSelection => "Reverse waveform selection",
            Self::MuteSelection => "Mute waveform selection",
            Self::ExtractAndTrimSelection => "Extract and trim waveform selection",
            Self::ApplyEditSelectionEffects => "Apply edit mark edits",
            Self::SlideSampleAudio { .. } => "Slide sample audio",
        }
    }

    pub(super) fn undo_label(self) -> &'static str {
        match self {
            Self::CropSelection => "Restore cropped audio",
            Self::TrimSelection => "Restore trimmed audio",
            Self::ReverseSelection => "Restore reversed audio",
            Self::MuteSelection => "Restore muted audio",
            Self::ExtractAndTrimSelection => "Restore extracted and trimmed audio",
            Self::ApplyEditSelectionEffects => "Restore edit mark edits",
            Self::SlideSampleAudio { .. } => "Restore slid audio",
        }
    }
}

pub(super) fn destructive_edit_prompt(
    edit: WaveformDestructiveEditKind,
    write_format: &AudioWriteFormatConfig,
) -> crate::native_app::app::WaveformDestructiveEditPrompt {
    let message = match edit {
        WaveformDestructiveEditKind::CropSelection => {
            "This will keep only the selected region and remove audio outside it from the source file."
        }
        WaveformDestructiveEditKind::TrimSelection => {
            "This will remove the selected region and close the gap in the source file."
        }
        WaveformDestructiveEditKind::ReverseSelection => {
            "This will reverse the selected region in place, or the selected file when no region is marked."
        }
        WaveformDestructiveEditKind::MuteSelection => {
            "This will silence the selected region in place without changing its duration."
        }
        WaveformDestructiveEditKind::ExtractAndTrimSelection => {
            "This will extract the selected region into a new sibling file, then remove that region and close the gap in the source file."
        }
        WaveformDestructiveEditKind::ApplyEditSelectionEffects => {
            "This will overwrite the edit selection with the currently previewed fade and gain edits."
        }
        WaveformDestructiveEditKind::SlideSampleAudio { frame_offset } => {
            let direction = if frame_offset > 0 { "right" } else { "left" };
            return crate::native_app::app::WaveformDestructiveEditPrompt {
                edit,
                title: destructive_edit_title(edit),
                message: format!(
                    "This will circularly slide the source file audio {direction} by {} frame{} without changing its duration. Wavecrate will rewrite the file using the current write format: {}.",
                    frame_offset.unsigned_abs(),
                    if frame_offset.unsigned_abs() == 1 {
                        ""
                    } else {
                        "s"
                    },
                    write_format.summary_label()
                ),
            };
        }
    };
    crate::native_app::app::WaveformDestructiveEditPrompt {
        edit,
        title: destructive_edit_title(edit),
        message: format!(
            "{message} Wavecrate will rewrite the file using the current write format: {}.",
            write_format.summary_label()
        ),
    }
}

fn destructive_edit_title(edit: WaveformDestructiveEditKind) -> String {
    match edit {
        WaveformDestructiveEditKind::ApplyEditSelectionEffects => {
            String::from("Apply edit mark edits")
        }
        _ => format!("{} selection", edit.action_label()),
    }
}
