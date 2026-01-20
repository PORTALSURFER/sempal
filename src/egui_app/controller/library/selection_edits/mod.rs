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

use ops::{
    apply_directional_fade,
    apply_edge_fades,
    apply_selection_fades,
    crop_buffer,
    reverse_buffer,
    trim_buffer,
};

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

    /// Apply edit-selection fades to disk and clear preview fades.
    pub(crate) fn commit_edit_selection_fades(&mut self) -> Result<bool, String> {
        let Some(selection) = self.ui.waveform.edit_selection else {
            return Ok(false);
        };
        if !selection.has_edit_effects() {
            return Ok(false);
        }
        let result = self.apply_selection_edit("Applied edit fades", true, |buffer| {
            apply_selection_fades(
                &mut buffer.samples,
                buffer.channels,
                buffer.start_frame,
                buffer.end_frame,
                selection.gain(),
                selection.fade_in(),
                selection.fade_out(),
            );
            Ok(())
        });
        match result {
            Ok(()) => {
                let cleared = selection.clear_fades().with_gain(1.0);
                self.selection_state.edit_range.set_range(Some(cleared));
                self.apply_edit_selection(Some(cleared));
                Ok(true)
            }
            Err(err) => {
                self.set_status(err.clone(), StatusTone::Error);
                Err(err)
            }
        }
    }

    /// Cancel preview fades for the edit selection without writing audio.
    pub(crate) fn cancel_edit_selection_fades(&mut self) -> bool {
        let Some(selection) = self.ui.waveform.edit_selection else {
            return false;
        };
        if !selection.has_edit_effects() {
            return false;
        }
        let cleared = selection.clear_fades().with_gain(1.0);
        self.selection_state.edit_range.set_range(Some(cleared));
        self.apply_edit_selection(Some(cleared));
        true
    }

    /// Crop the loaded sample to the active selection range and refresh caches/exports.
    pub(crate) fn crop_waveform_selection(&mut self) -> Result<(), String> {
        let result = self.apply_selection_edit("Cropped selection", false, crop_buffer);
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
                looped: false,
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

        let was_playing = self.is_playing();
        let was_looping = self.ui.waveform.loop_enabled;
        let playhead_position = self.ui.waveform.playhead.position;

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

        if was_playing {
            let start_override = if playhead_position.is_finite() {
                Some(playhead_position.clamp(0.0, 1.0))
            } else {
                None
            };
            self.runtime.jobs.set_pending_playback(Some(PendingPlayback {
                source_id: context.source.id.clone(),
                relative_path: new_relative.clone(),
                looped: was_looping,
                start_override,
            }));
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
        let result = self.apply_selection_edit("Trimmed selection", false, trim_buffer);
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
        let result = self.apply_selection_edit("Applied fade", true, |buffer| {
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
        let result = self.apply_selection_edit("Normalized selection", true, |buffer| {
            normalize_selection(buffer, Duration::from_millis(5))
        });
        if let Err(err) = &result {
            self.set_status(err.clone(), StatusTone::Error);
        }
        result
    }

    /// Apply short fade-in/out ramps at the selection edges to reduce clicks.
    pub(crate) fn soften_waveform_selection_edges(&mut self) -> Result<(), String> {
        let fade_ms = self.ui.controls.anti_clip_fade_ms.max(0.0);
        let fade_duration = Duration::from_secs_f32(fade_ms / 1000.0);
        let result = self.apply_selection_edit("Applied short fades", true, |buffer| {
            let selection_frames = buffer.end_frame.saturating_sub(buffer.start_frame);
            let fade_frames = edge_fade_frame_count(
                buffer.sample_rate.max(1),
                selection_frames,
                fade_duration,
            );
            if fade_frames == 0 {
                return Err("Selection is too short for edge fades".into());
            }
            apply_edge_fades(
                &mut buffer.samples,
                buffer.channels,
                buffer.start_frame,
                buffer.end_frame,
                fade_frames,
            );
            Ok(())
        });
        if let Err(err) = &result {
            self.set_status(err.clone(), StatusTone::Error);
        }
        result
    }

    /// Repair clicks inside the selection by interpolating the span.
    pub(crate) fn repair_clicks_selection(&mut self) -> Result<(), String> {
        let result =
            self.apply_selection_edit("Removed clicks", true, |buffer| repair_clicks_buffer(buffer));
        if let Err(err) = &result {
            self.set_status(err.clone(), StatusTone::Error);
        }
        result
    }

    /// Silence the selected span without applying fades.
    pub(crate) fn mute_waveform_selection(&mut self) -> Result<(), String> {
        let result = self.apply_selection_edit("Muted selection", true, ops::mute_buffer);
        if let Err(err) = &result {
            self.set_status(err.clone(), StatusTone::Error);
        }
        result
    }

    /// Reverse the selected span in time.
    pub(crate) fn reverse_waveform_selection(&mut self) -> Result<(), String> {
        let result = self.apply_selection_edit("Reversed selection", true, reverse_buffer);
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
            DestructiveSelectionEdit::ShortEdgeFades => self.soften_waveform_selection_edges(),
            DestructiveSelectionEdit::MuteSelection => self.mute_waveform_selection(),
            DestructiveSelectionEdit::NormalizeSelection => self.normalize_waveform_selection(),
            DestructiveSelectionEdit::ClickRemoval => self.repair_clicks_selection(),
        }
    }

    fn apply_selection_edit<F>(
        &mut self,
        action_label: &str,
        preserve_selection: bool,
        mut edit: F,
    ) -> Result<(), String>
    where
        F: FnMut(&mut SelectionEditBuffer) -> Result<(), String>,
    {
        let context = self.selection_target()?;
        let backup = undo::OverwriteBackup::capture_before(&context.absolute_path)?;
        
        let preserved_view = self.ui.waveform.view;
        let preserved_selection = self.ui.waveform.selection;
        let preserved_edit_selection = self.ui.waveform.edit_selection;
        let preserved_cursor = self.ui.waveform.cursor;
        let preserved_loop_enabled = self.ui.waveform.loop_enabled;
        let was_playing = self.is_playing();
        let was_looping = if self.ui.waveform.loop_enabled {
            true
        } else if self.audio.pending_loop_disable_at.is_some() {
            false
        } else {
            self.audio
                .player
                .as_ref()
                .is_some_and(|player| player.borrow().is_looping())
        };
        let playhead_position = self.ui.waveform.playhead.position;

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
        let looped = self.sample_looped_for(&context.source, &context.relative_path)?;
        let entry = WavEntry {
            relative_path: context.relative_path.clone(),
            file_size,
            modified_ns,
            content_hash: None,
            tag,
            looped,
            missing: false,
            last_played_at,
        };
        self.update_cached_entry(&context.source, &context.relative_path, entry);
        
        // Force a full reload by clearing loaded_wav (the file was modified on disk)
        self.sample_view.wav.loaded_wav = None;
        self.ui.loaded_wav = None;

        self.refresh_waveform_for_sample(&context.source, &context.relative_path);

        if preserve_selection {
            // Restore visuals and selection AFTER refresh
            self.ui.waveform.view = preserved_view.clamp();
            self.ui.waveform.cursor = preserved_cursor;
            self.ui.waveform.loop_enabled = preserved_loop_enabled;
            self.selection_state.range.set_range(preserved_selection);
            self.apply_selection(preserved_selection);
            self.selection_state
                .edit_range
                .set_range(preserved_edit_selection);
            self.apply_edit_selection(preserved_edit_selection);
        } else {
            self.clear_waveform_selection();
            self.clear_edit_selection();
        }

        if was_playing {
            let start_override = if playhead_position.is_finite() {
                Some(playhead_position.clamp(0.0, 1.0))
            } else {
                None
            };
            self.runtime.jobs.set_pending_playback(Some(PendingPlayback {
                source_id: context.source.id.clone(),
                relative_path: context.relative_path.clone(),
                looped: was_looping,
                start_override,
            }));
        }

        self.reexport_collections_for_sample(&context.source.id, &context.relative_path);
        self.maybe_trigger_pending_playback();
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
        let selection = selection_target_range(
            self.ui.waveform.edit_selection,
            self.ui.waveform.selection,
        );
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

fn selection_target_range(
    edit_selection: Option<SelectionRange>,
    play_selection: Option<SelectionRange>,
) -> SelectionRange {
    let edit_selection = edit_selection.filter(|range| range.width() > 0.0);
    let play_selection = play_selection.filter(|range| range.width() > 0.0);
    edit_selection
        .or(play_selection)
        .unwrap_or_else(|| SelectionRange::new(0.0, 1.0))
}

fn edge_fade_frame_count(sample_rate: u32, selection_frames: usize, duration: Duration) -> usize {
    if selection_frames == 0 {
        return 0;
    }
    let frames = (sample_rate as f32 * duration.as_secs_f32()).round() as usize;
    frames.min(selection_frames / 2)
}

#[cfg(test)]
#[path = "../selection_edits_tests.rs"]
mod selection_edits_tests;
