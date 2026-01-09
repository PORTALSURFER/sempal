use crate::egui_app::controller::library::collection_items_helpers::file_metadata;
use super::*;
use crate::egui_app::state::DestructiveSelectionEdit;
use hound::SampleFormat;
use std::time::Duration;

mod buffer;
mod ops;
mod prompt;
mod undo_entries;

mod selection_click;
mod selection_normalize;

use buffer::write_selection_wav;
use buffer::{SelectionEditBuffer, SelectionTarget};
pub(crate) use selection_click::repair_clicks_selection as repair_clicks_buffer;
use selection_normalize::normalize_selection;

use ops::{apply_directional_fade, crop_buffer, reverse_buffer, trim_buffer};

#[cfg(test)]
use buffer::selection_frame_bounds;
#[cfg(test)]
use ops::{apply_muted_selection, fade_factor, slice_frames};

use crate::egui_app::controller::undo;

/// Direction of a fade applied over the active selection.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum FadeDirection {
    /// Fade from full level at the left edge to silence at the right edge.
    LeftToRight,
    /// Fade from silence at the left edge to full level at the right edge.
    RightToLeft,
}

/// Result of a destructive edit request.
pub(crate) enum SelectionEditRequest {
    Applied,
    Prompted,
}

impl EguiController {
    /// Request a destructive edit, showing a confirmation unless yolo mode is enabled.
    pub(crate) fn request_destructive_selection_edit(
        &mut self,
        edit: DestructiveSelectionEdit,
    ) -> Result<SelectionEditRequest, String> {
        if let Err(err) = self.selection_target() {
            self.set_status(err.clone(), StatusTone::Error);
            return Err(err);
        }
        if self.settings.controls.destructive_yolo_mode {
            self.ui.waveform.pending_destructive = None;
            self.apply_selection_edit_kind(edit)?;
            return Ok(SelectionEditRequest::Applied);
        }
        self.ui.waveform.pending_destructive = Some(prompt::prompt_for_edit(edit));
        Ok(SelectionEditRequest::Prompted)
    }

    /// Apply the pending destructive edit after user confirmation.
    pub(crate) fn apply_confirmed_destructive_edit(&mut self, edit: DestructiveSelectionEdit) {
        self.ui.waveform.pending_destructive = None;
        let _ = self.apply_selection_edit_kind(edit);
    }

    /// Clear any pending destructive edit prompt without applying it.
    pub(crate) fn clear_destructive_prompt(&mut self) {
        self.ui.waveform.pending_destructive = None;
    }

    /// Crop the loaded sample to the active selection range and refresh caches/exports.
    pub(crate) fn crop_waveform_selection(&mut self) -> Result<(), String> {
        let result = self.apply_selection_edit("Cropped selection", crop_buffer);
        if let Err(err) = &result {
            self.set_status(err.clone(), StatusTone::Error);
        }
        result
    }

    /// Write the cropped selection to a new sample file alongside the original.
    pub(crate) fn crop_waveform_selection_to_new_sample(&mut self) -> Result<(), String> {
        let context = self.selection_target()?;
        let new_relative =
            buffer::next_crop_relative_path(&context.relative_path, &context.source.root)?;
        let new_absolute = context.source.root.join(&new_relative);

        let mut buffer = buffer::load_selection_buffer(&context.absolute_path, context.selection)?;
        crop_buffer(&mut buffer)?;
        if buffer.samples.is_empty() {
            return Err("Selection has no audio to crop".into());
        }
        let spec = hound::WavSpec {
            channels: buffer.spec_channels,
            sample_rate: buffer.sample_rate.max(1),
            bits_per_sample: 32,
            sample_format: SampleFormat::Float,
        };
        write_selection_wav(&new_absolute, &buffer.samples, spec)?;
        let (file_size, modified_ns) = file_metadata(&new_absolute)?;
        let tag = self.sample_tag_for(&context.source, &context.relative_path)?;
        let db = self
            .database_for(&context.source)
            .map_err(|err| format!("Database unavailable: {err}"))?;
        db.upsert_file(&new_relative, file_size, modified_ns)
            .map_err(|err| format!("Failed to sync database entry: {err}"))?;
        db.set_tag(&new_relative, tag)
            .map_err(|err| format!("Failed to sync tag: {err}"))?;

        self.insert_cached_entry(
            &context.source,
            WavEntry {
                relative_path: new_relative.clone(),
                file_size,
                modified_ns,
                content_hash: None,
                tag,
                missing: false,
                last_played_at: None,
            },
        );
        self.enqueue_similarity_for_new_sample(
            &context.source,
            &new_relative,
            file_size,
            modified_ns,
        );
        self.refresh_waveform_for_sample(&context.source, &context.relative_path);
        self.reexport_collections_for_sample(&context.source.id, &new_relative);

        if let Ok(backup) = undo::OverwriteBackup::capture_before(&new_absolute) {
            if backup.capture_after(&new_absolute).is_ok() {
                self.push_undo_entry(self.crop_new_sample_undo_entry(
                    format!("Cropped to new sample {}", new_relative.display()),
                    context.source.id.clone(),
                    new_relative.clone(),
                    new_absolute.clone(),
                    tag,
                    backup,
                ));
            }
        }

        let _ = self.load_waveform_for_selection(&context.source, &new_relative);
        self.focus_waveform();
        self.set_status(
            format!("Cropped to new sample {}", new_relative.display()),
            StatusTone::Info,
        );
        Ok(())
    }

    /// Remove the selected span from the loaded sample.
    pub(crate) fn trim_waveform_selection(&mut self) -> Result<(), String> {
        let result = self.apply_selection_edit("Trimmed selection", trim_buffer);
        if let Err(err) = &result {
            self.set_status(err.clone(), StatusTone::Error);
        }
        result
    }

    /// Fade the selected span down to silence using the given direction.
    pub(crate) fn fade_waveform_selection(
        &mut self,
        direction: FadeDirection,
    ) -> Result<(), String> {
        let result = self.apply_selection_edit("Applied fade", |buffer| {
            apply_directional_fade(
                &mut buffer.samples,
                buffer.channels,
                buffer.start_frame,
                buffer.end_frame,
                direction,
            );
            Ok(())
        });
        if let Err(err) = &result {
            self.set_status(err.clone(), StatusTone::Error);
        }
        result
    }

    /// Normalize the active selection and apply short fades at the edges.
    pub(crate) fn normalize_waveform_selection(&mut self) -> Result<(), String> {
        let preserved_view = self.ui.waveform.view;
        let preserved_selection = self.ui.waveform.selection;
        let preserved_cursor = self.ui.waveform.cursor;
        let was_playing = self.is_playing();
        let was_looping = self.ui.waveform.loop_enabled;
        let playhead_position = self.ui.waveform.playhead.position;
        let result = self.apply_selection_edit("Normalized selection", |buffer| {
            normalize_selection(buffer, Duration::from_millis(5))
        });
        if result.is_ok() {
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
        }
        if let Err(err) = &result {
            self.set_status(err.clone(), StatusTone::Error);
        }
        result
    }

    /// Repair clicks inside the selection by interpolating the span.
    pub(crate) fn repair_clicks_selection(&mut self) -> Result<(), String> {
        let preserved_view = self.ui.waveform.view;
        let preserved_selection = self.ui.waveform.selection;
        let preserved_cursor = self.ui.waveform.cursor;
        let was_playing = self.is_playing();
        let was_looping = self.ui.waveform.loop_enabled;
        let playhead_position = self.ui.waveform.playhead.position;
        let result =
            self.apply_selection_edit("Removed clicks", |buffer| repair_clicks_buffer(buffer));
        if result.is_ok() {
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
        }
        if let Err(err) = &result {
            self.set_status(err.clone(), StatusTone::Error);
        }
        result
    }

    /// Silence the selected span without applying fades.
    pub(crate) fn mute_waveform_selection(&mut self) -> Result<(), String> {
        let result = self.apply_selection_edit("Muted selection", ops::mute_buffer);
        if let Err(err) = &result {
            self.set_status(err.clone(), StatusTone::Error);
        }
        result
    }

    /// Reverse the selected span in time.
    pub(crate) fn reverse_waveform_selection(&mut self) -> Result<(), String> {
        let result = self.apply_selection_edit("Reversed selection", reverse_buffer);
        if let Err(err) = &result {
            self.set_status(err.clone(), StatusTone::Error);
        }
        result
    }

    fn apply_selection_edit_kind(&mut self, edit: DestructiveSelectionEdit) -> Result<(), String> {
        match edit {
            DestructiveSelectionEdit::CropSelection => self.crop_waveform_selection(),
            DestructiveSelectionEdit::TrimSelection => self.trim_waveform_selection(),
            DestructiveSelectionEdit::ReverseSelection => self.reverse_waveform_selection(),
            DestructiveSelectionEdit::FadeLeftToRight => {
                self.fade_waveform_selection(FadeDirection::LeftToRight)
            }
            DestructiveSelectionEdit::FadeRightToLeft => {
                self.fade_waveform_selection(FadeDirection::RightToLeft)
            }
            DestructiveSelectionEdit::MuteSelection => self.mute_waveform_selection(),
            DestructiveSelectionEdit::NormalizeSelection => self.normalize_waveform_selection(),
            DestructiveSelectionEdit::ClickRemoval => self.repair_clicks_selection(),
        }
    }

    fn apply_selection_edit<F>(&mut self, action_label: &str, mut edit: F) -> Result<(), String>
    where
        F: FnMut(&mut SelectionEditBuffer) -> Result<(), String>,
    {
        let context = self.selection_target()?;
        let backup = undo::OverwriteBackup::capture_before(&context.absolute_path)?;
        let mut buffer = buffer::load_selection_buffer(&context.absolute_path, context.selection)?;
        edit(&mut buffer)?;
        if buffer.samples.is_empty() {
            return Err("No audio data after edit".into());
        }
        let spec = hound::WavSpec {
            channels: buffer.spec_channels,
            sample_rate: buffer.sample_rate.max(1),
            bits_per_sample: 32,
            sample_format: SampleFormat::Float,
        };
        write_selection_wav(&context.absolute_path, &buffer.samples, spec)?;
        backup.capture_after(&context.absolute_path)?;
        let (file_size, modified_ns) = file_metadata(&context.absolute_path)?;
        let tag = self.sample_tag_for(&context.source, &context.relative_path)?;
        let db = self
            .database_for(&context.source)
            .map_err(|err| format!("Database unavailable: {err}"))?;
        db.upsert_file(&context.relative_path, file_size, modified_ns)
            .map_err(|err| format!("Failed to sync database entry: {err}"))?;
        db.set_tag(&context.relative_path, tag)
            .map_err(|err| format!("Failed to sync tag: {err}"))?;
        let last_played_at = self
            .sample_last_played_for(&context.source, &context.relative_path)?;
        let entry = WavEntry {
            relative_path: context.relative_path.clone(),
            file_size,
            modified_ns,
            content_hash: None,
            tag,
            missing: false,
            last_played_at,
        };
        self.update_cached_entry(&context.source, &context.relative_path, entry);
        self.refresh_waveform_for_sample(&context.source, &context.relative_path);
        self.reexport_collections_for_sample(&context.source.id, &context.relative_path);
        self.push_undo_entry(self.selection_edit_undo_entry(
            format!("{action_label} {}", context.relative_path.display()),
            context.source.id.clone(),
            context.relative_path.clone(),
            context.absolute_path.clone(),
            backup,
        ));
        self.set_status(
            format!("{} {}", action_label, context.relative_path.display()),
            StatusTone::Info,
        );
        Ok(())
    }

    fn selection_target(&self) -> Result<SelectionTarget, String> {
        let selection = self
            .ui
            .waveform
            .selection
            .ok_or_else(|| "Make a selection first".to_string())?;
        if selection.width() <= 0.0 {
            return Err("Selection is empty".into());
        }
        let audio = self
            .sample_view
            .wav
            .loaded_audio
            .as_ref()
            .ok_or_else(|| "Load a sample to edit it".to_string())?;
        let source = self
            .library
            .sources
            .iter()
            .find(|s| s.id == audio.source_id)
            .cloned()
            .ok_or_else(|| "Source not available for loaded sample".to_string())?;
        let relative_path = audio.relative_path.clone();
        let absolute_path = source.root.join(&relative_path);
        Ok(SelectionTarget {
            source,
            relative_path,
            absolute_path,
            selection,
        })
    }
}

#[cfg(test)]
#[path = "../selection_edits_tests.rs"]
mod selection_edits_tests;
