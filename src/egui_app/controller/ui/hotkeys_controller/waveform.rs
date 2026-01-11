use super::HotkeysController;
use crate::egui_app::controller::StatusTone;
use crate::egui_app::controller::ui::hotkeys::HotkeyCommand;
use crate::egui_app::state::DestructiveSelectionEdit;
use crate::sample_sources::WavEntry;

pub(crate) fn handle_waveform_command(
    controller: &mut HotkeysController<'_>,
    command: HotkeyCommand,
) -> bool {
    match command {
        HotkeyCommand::NormalizeWaveform => {
            controller.normalize_waveform_selection_or_sample();
            true
        }
        HotkeyCommand::AlignWaveformStartToMarker => {
            if let Err(err) = controller.align_waveform_start_to_last_marker() {
                controller.set_status(err, StatusTone::Error);
            }
            true
        }
        HotkeyCommand::CropSelection => {
            let _ = controller
                .request_destructive_selection_edit(DestructiveSelectionEdit::CropSelection);
            true
        }
        HotkeyCommand::CropSelectionNewSample => {
            if let Err(err) = controller.crop_waveform_selection_to_new_sample() {
                controller.set_status(err, StatusTone::Error);
            }
            true
        }
        HotkeyCommand::SaveSelectionToBrowser => {
            if !controller.ui.waveform.slices.is_empty() {
                match controller.accept_waveform_slices() {
                    Ok(count) => {
                        controller.set_status(format!("Saved {count} slices"), StatusTone::Info);
                    }
                    Err(err) => controller.set_status(err, StatusTone::Error),
                }
            } else if let Err(err) = controller.save_waveform_selection_to_browser(true) {
                controller.set_status(err, StatusTone::Error);
            }
            true
        }
        HotkeyCommand::TrimSelection => {
            let _ = controller
                .request_destructive_selection_edit(DestructiveSelectionEdit::TrimSelection);
            true
        }
        HotkeyCommand::ReverseSelection => {
            let _ = controller
                .request_destructive_selection_edit(DestructiveSelectionEdit::ReverseSelection);
            true
        }
        HotkeyCommand::FadeSelectionLeftToRight => {
            let _ = controller
                .request_destructive_selection_edit(DestructiveSelectionEdit::FadeLeftToRight);
            true
        }
        HotkeyCommand::FadeSelectionRightToLeft => {
            let _ = controller
                .request_destructive_selection_edit(DestructiveSelectionEdit::FadeRightToLeft);
            true
        }
        HotkeyCommand::DeleteSliceMarkers => {
            if controller.ui.waveform.slice_mode_enabled {
                let removed = controller.delete_selected_slices();
                if removed > 0 {
                    controller.set_status(format!("Deleted {removed} slices"), StatusTone::Info);
                } else {
                    controller.set_status("Select slices to delete", StatusTone::Info);
                }
            }
            true
        }
        HotkeyCommand::MuteSelection => {
            if controller.ui.waveform.slice_mode_enabled {
                let selected = controller.ui.waveform.selected_slices.len();
                if selected < 2 {
                    controller.set_status("Select at least 2 slices to merge", StatusTone::Info);
                } else if controller.merge_selected_slices().is_some() {
                    controller.set_status(format!("Merged {selected} slices"), StatusTone::Info);
                } else {
                    controller.set_status("No slices merged", StatusTone::Info);
                }
            } else {
                let _ = controller
                    .request_destructive_selection_edit(DestructiveSelectionEdit::MuteSelection);
            }
            true
        }
        HotkeyCommand::ToggleBpmSnap => {
            controller.toggle_bpm_snap();
            true
        }
        HotkeyCommand::ToggleTransientMarkers => {
            controller.toggle_transient_markers();
            true
        }
        HotkeyCommand::ZoomInSelection => {
            controller.waveform().zoom_to_selection();
            true
        }
        HotkeyCommand::SlideSelectionLeft => {
            controller.waveform().slide_selection_range(-1);
            true
        }
        HotkeyCommand::SlideSelectionRight => {
            controller.waveform().slide_selection_range(1);
            true
        }
        HotkeyCommand::NudgeSelectionLeft => {
            controller.waveform().nudge_selection_range(-1, true);
            true
        }
        HotkeyCommand::NudgeSelectionRight => {
            controller.waveform().nudge_selection_range(1, true);
            true
        }
        HotkeyCommand::ZoomOutSelection => {
            controller.waveform().zoom_out_full();
            true
        }
        _ => false,
    }
}

impl HotkeysController<'_> {
    fn toggle_bpm_snap(&mut self) {
        let enabled = !self.ui.waveform.bpm_snap_enabled;
        let prev_value = self.ui.waveform.bpm_value;
        self.set_bpm_snap_enabled(enabled);
        if enabled && prev_value.is_none() {
            let fallback = 142.0;
            self.set_bpm_value(fallback);
            self.ui.waveform.bpm_input = format!("{fallback:.0}");
        }
    }

    fn toggle_transient_markers(&mut self) {
        let enabled = !self.ui.waveform.transient_markers_enabled;
        self.set_transient_markers_enabled(enabled);
    }

    fn normalize_waveform_selection_or_sample(&mut self) {
        if self
            .ui
            .waveform
            .selection
            .is_some_and(|selection| selection.width() > 0.0)
        {
            let _ = self
                .request_destructive_selection_edit(DestructiveSelectionEdit::NormalizeSelection);
            return;
        }
        if let Err(err) = self.normalize_loaded_sample_like_browser() {
            self.set_status(err, StatusTone::Error);
        }
    }

    fn normalize_loaded_sample_like_browser(&mut self) -> Result<(), String> {
        let preserved_view = self.ui.waveform.view;
        let preserved_cursor = self.ui.waveform.cursor;
        let preserved_selection = self.ui.waveform.selection;
        let was_playing = self.is_playing();
        let was_looping = self.ui.waveform.loop_enabled;
        let playhead_position = self.ui.waveform.playhead.position;
        let audio = self
            .sample_view
            .wav
            .loaded_audio
            .as_ref()
            .ok_or_else(|| "Load a sample to normalize it".to_string())?;
        let source = self
            .library
            .sources
            .iter()
            .find(|s| s.id == audio.source_id)
            .cloned()
            .ok_or_else(|| "Source not available for loaded sample".to_string())?;
        let relative_path = audio.relative_path.clone();
        let absolute_path = source.root.join(&relative_path);
        let (file_size, modified_ns, tag) =
            self.normalize_and_save_for_path(&source, &relative_path, &absolute_path)?;
        self.upsert_metadata_for_source(&source, &relative_path, file_size, modified_ns)?;
        let last_played_at = self
            .sample_last_played_for(&source, &relative_path)
            .unwrap_or(None);
        let looped = self.sample_looped_for(&source, &relative_path).unwrap_or(false);
        let updated = WavEntry {
            relative_path: relative_path.clone(),
            file_size,
            modified_ns,
            content_hash: None,
            tag,
            looped,
            missing: false,
            last_played_at,
        };
        self.update_cached_entry(&source, &relative_path, updated);
        if self.selection_state.ctx.selected_source.as_ref() == Some(&source.id) {
            self.rebuild_browser_lists();
        }
        self.refresh_waveform_for_sample(&source, &relative_path);
        self.reexport_collections_for_sample(&source.id, &relative_path);
        self.ui.waveform.view = preserved_view.clamp();
        self.ui.waveform.cursor = preserved_cursor;
        self.selection_state.range.set_range(preserved_selection);
        self.apply_selection(preserved_selection);
        if was_playing {
            let start_override = if playhead_position.is_finite() {
                Some(playhead_position.clamp(0.0, 1.0))
            } else {
                None
            };
            if let Err(err) = self.play_audio(was_looping, start_override) {
                self.set_status(err, StatusTone::Error);
            }
        }
        self.set_status(
            format!("Normalized {}", relative_path.display()),
            StatusTone::Info,
        );
        Ok(())
    }
}
