use std::{path::PathBuf, time::Instant};

use radiant::prelude as ui;
use wavecrate::selection::SelectionRange;

use crate::native_app::app::{
    GuiMessage, NativeAppState, WaveformDestructiveEditKind, WaveformDestructiveEditTarget,
};
use crate::native_app::waveform::WaveformSelectionKind;

use super::protected_copy::{harvest_region_copy_operation, harvest_whole_file_copy_operation};
impl WaveformDestructiveEditTarget {
    pub(super) fn missing_selection_message(self, kind: WaveformDestructiveEditKind) -> String {
        match self {
            Self::ActiveSelection => {
                format!("Mark an edit or play range before {}", kind.gerund_label())
            }
            Self::PlaySelection => format!("Mark a play range before {}", kind.gerund_label()),
        }
    }

    pub(super) fn fallback_selection_kind(self) -> WaveformSelectionKind {
        match self {
            Self::ActiveSelection => WaveformSelectionKind::Edit,
            Self::PlaySelection => WaveformSelectionKind::Play,
        }
    }
}

impl NativeAppState {
    pub(in crate::native_app) fn request_crop_waveform_selection(
        &mut self,
        context: &mut ui::UiUpdateContext<GuiMessage>,
    ) {
        self.request_waveform_destructive_edit(
            WaveformDestructiveEditKind::CropSelection,
            WaveformDestructiveEditTarget::ActiveSelection,
            context,
        );
    }

    pub(in crate::native_app) fn request_trim_waveform_selection(
        &mut self,
        context: &mut ui::UiUpdateContext<GuiMessage>,
    ) {
        self.request_waveform_destructive_edit(
            WaveformDestructiveEditKind::TrimSelection,
            WaveformDestructiveEditTarget::ActiveSelection,
            context,
        );
    }

    pub(in crate::native_app) fn request_reverse_waveform_selection(
        &mut self,
        context: &mut ui::UiUpdateContext<GuiMessage>,
    ) {
        self.request_waveform_destructive_edit(
            WaveformDestructiveEditKind::ReverseSelection,
            WaveformDestructiveEditTarget::ActiveSelection,
            context,
        );
    }

    pub(in crate::native_app) fn request_mute_waveform_selection(
        &mut self,
        context: &mut ui::UiUpdateContext<GuiMessage>,
    ) {
        self.request_waveform_destructive_edit(
            WaveformDestructiveEditKind::MuteSelection,
            WaveformDestructiveEditTarget::ActiveSelection,
            context,
        );
    }

    pub(in crate::native_app) fn request_extract_and_trim_waveform_selection(
        &mut self,
        context: &mut ui::UiUpdateContext<GuiMessage>,
    ) {
        self.request_waveform_destructive_edit(
            WaveformDestructiveEditKind::ExtractAndTrimSelection,
            WaveformDestructiveEditTarget::ActiveSelection,
            context,
        );
    }

    pub(in crate::native_app) fn request_crop_playmark_selection(
        &mut self,
        context: &mut ui::UiUpdateContext<GuiMessage>,
    ) {
        self.request_waveform_destructive_edit(
            WaveformDestructiveEditKind::CropSelection,
            WaveformDestructiveEditTarget::PlaySelection,
            context,
        );
    }

    pub(in crate::native_app) fn request_trim_playmark_selection(
        &mut self,
        context: &mut ui::UiUpdateContext<GuiMessage>,
    ) {
        self.request_waveform_destructive_edit(
            WaveformDestructiveEditKind::TrimSelection,
            WaveformDestructiveEditTarget::PlaySelection,
            context,
        );
    }

    pub(in crate::native_app) fn request_reverse_playmark_selection(
        &mut self,
        context: &mut ui::UiUpdateContext<GuiMessage>,
    ) {
        self.request_waveform_destructive_edit(
            WaveformDestructiveEditKind::ReverseSelection,
            WaveformDestructiveEditTarget::PlaySelection,
            context,
        );
    }

    pub(in crate::native_app) fn request_extract_and_trim_playmark_selection(
        &mut self,
        context: &mut ui::UiUpdateContext<GuiMessage>,
    ) {
        self.request_waveform_destructive_edit(
            WaveformDestructiveEditKind::ExtractAndTrimSelection,
            WaveformDestructiveEditTarget::PlaySelection,
            context,
        );
    }

    pub(in crate::native_app) fn request_apply_edit_selection_effects(
        &mut self,
        context: &mut ui::UiUpdateContext<GuiMessage>,
    ) {
        self.request_waveform_destructive_edit(
            WaveformDestructiveEditKind::ApplyEditSelectionEffects,
            WaveformDestructiveEditTarget::ActiveSelection,
            context,
        );
    }

    pub(in crate::native_app) fn request_slide_loaded_sample_audio(
        &mut self,
        frame_offset: i64,
        context: &mut ui::UiUpdateContext<GuiMessage>,
    ) {
        if frame_offset == 0 {
            self.ui.status.sample = String::from("Sample slide cancelled");
            return;
        }
        self.request_waveform_destructive_edit(
            WaveformDestructiveEditKind::SlideSampleAudio { frame_offset },
            WaveformDestructiveEditTarget::ActiveSelection,
            context,
        );
    }

    fn request_waveform_destructive_edit(
        &mut self,
        kind: WaveformDestructiveEditKind,
        target: WaveformDestructiveEditTarget,
        context: &mut ui::UiUpdateContext<GuiMessage>,
    ) {
        let started_at = Instant::now();
        if self.reject_protected_trim_request(kind, target) {
            return;
        }
        if let Some(harvest_operation) = harvest_whole_file_copy_operation(kind) {
            match self.queue_harvest_whole_file_copy_edit_request(
                kind,
                target,
                harvest_operation,
                context,
            ) {
                Ok(true) => return,
                Ok(false) => {}
                Err(error) => {
                    self.flash_denied_destructive_selection_for_error(&error, kind, target);
                    self.ui.status.sample =
                        self.denied_destructive_edit_status(&error, kind, target);
                    return;
                }
            }
        }
        if let Some(harvest_operation) = harvest_region_copy_operation(kind) {
            match self.queue_harvest_region_copy_request(
                kind,
                target,
                harvest_operation,
                started_at,
                context,
            ) {
                Ok(true) => return,
                Ok(false) => {}
                Err(error) => {
                    self.flash_denied_destructive_selection_for_error(&error, kind, target);
                    self.ui.status.sample =
                        self.denied_destructive_edit_status(&error, kind, target);
                    return;
                }
            }
        }

        let request = match self.pending_destructive_edit_request(kind, target) {
            Ok(request) => request,
            Err(error) => {
                self.flash_denied_destructive_selection_for_error(&error, kind, target);
                self.ui.status.sample = self.denied_destructive_edit_status(&error, kind, target);
                return;
            }
        };

        if self.ui.settings.persisted.controls.destructive_yolo_mode {
            self.ui
                .browser_interaction
                .pending_waveform_destructive_edit = None;
            if let Err(error) = self.queue_destructive_edit_request(request, context) {
                let error = error.to_string();
                self.ui.status.sample = format!(
                    "{} failed: {}",
                    kind.action_label(),
                    self.denied_destructive_edit_status(&error, kind, target)
                );
            }
            return;
        }

        self.ui
            .browser_interaction
            .pending_waveform_destructive_edit = Some(request);
    }

    fn reject_protected_trim_request(
        &mut self,
        kind: WaveformDestructiveEditKind,
        target: WaveformDestructiveEditTarget,
    ) -> bool {
        if kind != WaveformDestructiveEditKind::TrimSelection {
            return false;
        }
        let Ok((path, _)) = self.destructive_edit_target_for_kind(kind, target) else {
            return false;
        };
        if !self
            .library
            .folder_browser
            .path_is_in_protected_source(&path)
        {
            return false;
        }
        let Some(error) = self
            .library
            .folder_browser
            .file_change_lock_error(&path, kind.action_label())
        else {
            return false;
        };
        self.flash_denied_destructive_selection_for_error(&error, kind, target);
        self.ui.status.sample = self.denied_destructive_edit_status(&error, kind, target);
        true
    }

    pub(super) fn destructive_edit_target_for_kind(
        &self,
        kind: WaveformDestructiveEditKind,
        target: WaveformDestructiveEditTarget,
    ) -> Result<(PathBuf, SelectionRange), String> {
        let absolute_path = self.waveform.current.path();
        let has_loaded_waveform =
            self.waveform.current.has_loaded_sample() && !absolute_path.as_os_str().is_empty();

        if has_loaded_waveform {
            if let Some(selection) = self.destructive_edit_selection_for_kind(kind, target)? {
                return Ok((absolute_path, selection));
            }
        } else if !matches!(kind, WaveformDestructiveEditKind::ReverseSelection)
            || target == WaveformDestructiveEditTarget::PlaySelection
        {
            return Err(format!("Load a sample before {}", kind.gerund_label()));
        }

        if matches!(kind, WaveformDestructiveEditKind::ReverseSelection)
            && target == WaveformDestructiveEditTarget::ActiveSelection
        {
            return self.selected_file_reverse_edit_target();
        }

        Err(target.missing_selection_message(kind))
    }

    fn selected_file_reverse_edit_target(&self) -> Result<(PathBuf, SelectionRange), String> {
        let absolute_path = self
            .library
            .folder_browser
            .selected_file_id()
            .map(PathBuf::from)
            .ok_or_else(|| {
                String::from("Mark an edit or play range or select a sample before reversing")
            })?;
        Ok((absolute_path, SelectionRange::new(0.0, 1.0)))
    }

    fn destructive_edit_selection_for_kind(
        &self,
        kind: WaveformDestructiveEditKind,
        target: WaveformDestructiveEditTarget,
    ) -> Result<Option<SelectionRange>, String> {
        if target == WaveformDestructiveEditTarget::PlaySelection {
            return Ok(self
                .waveform
                .current
                .play_selection()
                .filter(|selection| selection.width() > 0.0));
        }
        if matches!(kind, WaveformDestructiveEditKind::SlideSampleAudio { .. }) {
            return Ok(Some(SelectionRange::new(0.0, 1.0)));
        }
        if matches!(kind, WaveformDestructiveEditKind::ApplyEditSelectionEffects) {
            let selection = self
                .waveform
                .current
                .edit_selection()
                .filter(|selection| selection.width() > 0.0)
                .ok_or_else(|| String::from("Set an edit selection before applying it"))?;
            if !selection.has_edit_effects() {
                return Err(String::from(
                    "Adjust an edit fade or gain before applying it",
                ));
            }
            return Ok(Some(selection));
        }
        Ok(self.waveform.current.destructive_edit_selection())
    }
}
