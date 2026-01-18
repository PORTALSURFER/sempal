use super::*;
use crate::egui_app::state::FocusContext;
use crate::selection::SelectionEdge;

const TRANSIENT_SNAP_RADIUS: f32 = 0.01;
const SELECTION_START_SNAP_RADIUS: f32 = 0.01;
const SELECTION_START_SNAP_VIEW_FRACTION: f32 = 0.03;
const SELECTION_START_SNAP_SECONDS: f32 = 0.1;

pub(crate) fn start_selection_drag(controller: &mut EguiController, position: f32) {
    controller.selection_state.bpm_scale_beats = None;
    controller.begin_selection_undo("Selection");
    let start = snap_selection_start(controller, position)
        .or_else(|| snap_to_transient(controller, position))
        .unwrap_or(position);
    let range = controller.selection_state.range.begin_new(start);
    controller.apply_selection(Some(range));
}

pub(crate) fn start_edit_selection_drag(controller: &mut EguiController, position: f32) {
    let start = snap_to_transient(controller, position).unwrap_or(position);
    let range = controller.selection_state.edit_range.begin_new(start);
    controller.apply_edit_selection(Some(range));
}

pub(crate) fn start_selection_edge_drag(
    controller: &mut EguiController,
    edge: SelectionEdge,
    bpm_scale: bool,
) -> bool {
    if !controller.selection_state.range.begin_edge_drag(edge) {
        return false;
    }
    controller.begin_selection_undo("Selection");
    controller.selection_state.bpm_scale_beats = if bpm_scale {
        selection_scale_beats(controller)
    } else {
        None
    };
    controller.apply_selection(controller.selection_state.range.range());
    true
}

pub(crate) fn update_selection_drag(
    controller: &mut EguiController,
    position: f32,
    snap_override: bool,
) {
    let range = if controller.selection_state.bpm_scale_beats.is_some() {
        controller.selection_state.range.update_drag(position)
    } else if snap_override {
        controller.selection_state.range.update_drag(position)
    } else if let Some(step) = bpm_snap_step(controller) {
        controller
            .selection_state
            .range
            .update_drag_snapped(position, step)
    } else {
        let snapped = snap_to_transient(controller, position).unwrap_or(position);
        controller.selection_state.range.update_drag(snapped)
    };
    if let Some(range) = range {
        controller.apply_selection(Some(range));
        if let Some(beats) = controller.selection_state.bpm_scale_beats {
            apply_scaled_bpm(controller, beats, range);
        }
    } else if controller.selection_state.range.range().is_none() {
        controller.apply_selection(None);
    }
}

pub(crate) fn update_edit_selection_drag(
    controller: &mut EguiController,
    position: f32,
    snap_override: bool,
) {
    let range = if snap_override {
        controller.selection_state.edit_range.update_drag(position)
    } else {
        let snapped = snap_to_transient(controller, position).unwrap_or(position);
        controller.selection_state.edit_range.update_drag(snapped)
    };
    if let Some(range) = range {
        controller.apply_edit_selection(Some(range));
    } else if controller.selection_state.edit_range.range().is_none() {
        controller.apply_edit_selection(None);
    }
}

pub(crate) fn finish_selection_drag(controller: &mut EguiController) {
    controller.selection_state.range.finish_drag();
    controller.selection_state.bpm_scale_beats = None;
    clear_too_small_bpm_selection(controller);
    controller.commit_selection_undo();
    let is_playing = controller
        .audio
        .player
        .as_ref()
        .map(|p| p.borrow().is_playing())
        .unwrap_or(false);
    if !is_playing || !controller.ui.waveform.loop_enabled {
        return;
    }
    let Some(selection) = controller
        .selection_state
        .range
        .range()
        .or(controller.ui.waveform.selection)
        .filter(|range| super::selection_meets_bpm_min_for_playback(controller, *range))
    else {
        return;
    };
    let playhead = controller.ui.waveform.playhead.position;
    let start_override = if playhead >= selection.start() && playhead <= selection.end() {
        Some(playhead)
    } else {
        Some(selection.start())
    };
    if let Err(err) = controller.play_audio(true, start_override) {
        controller.set_status(err, StatusTone::Error);
    }
}

pub(crate) fn finish_edit_selection_drag(controller: &mut EguiController) {
    controller.selection_state.edit_range.finish_drag();
}

pub(crate) fn set_selection_range(controller: &mut EguiController, range: SelectionRange) {
    controller.selection_state.range.set_range(Some(range));
    controller.apply_selection(Some(range));
}

pub(crate) fn set_edit_selection_range(controller: &mut EguiController, range: SelectionRange) {
    controller.selection_state.edit_range.set_range(Some(range));
    controller.apply_edit_selection(Some(range));
}

pub(crate) fn is_selection_dragging(controller: &EguiController) -> bool {
    controller.selection_state.range.is_dragging()
}

pub(crate) fn is_edit_selection_dragging(controller: &EguiController) -> bool {
    controller.selection_state.edit_range.is_dragging()
}

pub(crate) fn clear_selection(controller: &mut EguiController) {
    let before = controller
        .selection_state
        .range
        .range()
        .or(controller.ui.waveform.selection);
    let cleared = controller.selection_state.range.clear();
    if cleared || controller.ui.waveform.selection.is_some() {
        controller.apply_selection(None);
        controller.push_selection_undo("Selection", before, None);
    }
}

pub(crate) fn clear_edit_selection(controller: &mut EguiController) {
    let cleared = controller.selection_state.edit_range.clear();
    if cleared || controller.ui.waveform.edit_selection.is_some() {
        controller.apply_edit_selection(None);
    }
}

pub(crate) fn toggle_loop(controller: &mut EguiController) {
    let was_looping = controller.ui.waveform.loop_enabled;
    controller.ui.waveform.loop_enabled = !controller.ui.waveform.loop_enabled;
    let new_loop_state = controller.ui.waveform.loop_enabled;
    
    // Try to update loop markers for all selected samples in the browser
    let loaded_path = controller.sample_view.wav.loaded_audio.as_ref()
        .map(|audio| audio.relative_path.clone());
    
    // Get the primary row (currently loaded sample if visible in browser)
    let primary_row = loaded_path.as_ref()
        .and_then(|path| controller.visible_row_for_path(path));
    
    // Get all action rows (selected samples + primary if not selected)
    let action_rows = if let Some(row) = primary_row {
        controller.action_rows_from_primary(row)
    } else {
        Vec::new()
    };
    
    // If we have browser rows to update, use the multi-sample approach
    if !action_rows.is_empty() {
        if let Err(err) = controller.set_loop_marker_browser_samples(
            &action_rows,
            new_loop_state,
            primary_row.unwrap_or(0),
        ) {
            tracing::warn!("Failed to update loop markers for browser samples: {err}");
        }
        
        // When enabling loop, also save the current BPM value to all selected samples
        if new_loop_state && !was_looping {
            if let Some(bpm) = controller.ui.waveform.bpm_value {
                if bpm.is_finite() && bpm > 0.0 {
                    if let Err(err) = controller.set_bpm_browser_samples(
                        &action_rows,
                        bpm,
                        primary_row.unwrap_or(0),
                    ) {
                        tracing::warn!("Failed to save BPM to browser samples: {err}");
                    }
                }
            }
        }
    } else {
        // Fallback: Update loop marker for just the currently loaded sample
        let loop_marker_update = controller.sample_view.wav.loaded_audio.as_ref().and_then(|loaded_audio| {
            controller.library.sources.iter()
                .find(|s| s.id == loaded_audio.source_id)
                .map(|source| (source.clone(), loaded_audio.relative_path.clone()))
        });
        
        if let Some((source, relative_path)) = loop_marker_update {
            if let Err(err) = controller.set_sample_looped_for_source(
                &source,
                &relative_path,
                new_loop_state,
                false,
            ) {
                tracing::warn!("Failed to update loop marker: {err}");
            }
        }
    }
    
    if controller.ui.waveform.loop_enabled {
        controller.audio.pending_loop_disable_at = None;
        if !was_looping {
            if let Some(player_rc) = controller.audio.player.as_ref().cloned() {
                let (is_playing, progress) = {
                    let player_ref = player_rc.borrow();
                    (player_ref.is_playing(), player_ref.progress())
                };
                if is_playing {
                    let has_selection = controller
                        .selection_state
                        .range
                        .range()
                        .or(controller.ui.waveform.selection)
                        .filter(|range| {
                            super::selection_meets_bpm_min_for_playback(controller, *range)
                        })
                        .is_some();
                    let start_override = if has_selection {
                        None
                    } else {
                        progress.or_else(|| {
                            if controller.ui.waveform.playhead.visible {
                                Some(controller.ui.waveform.playhead.position)
                            } else {
                                controller
                                    .ui
                                    .waveform
                                    .cursor
                                    .or(controller.ui.waveform.last_start_marker)
                            }
                        })
                    };
                    if let Err(err) = controller.play_audio(true, start_override) {
                        controller.set_status(err, StatusTone::Error);
                    }
                }
            }
        }
        return;
    }
    if was_looping && let Err(err) = controller.defer_loop_disable_after_cycle() {
        controller.set_status(err, StatusTone::Error);
    }
}

pub(crate) fn seek_to(controller: &mut EguiController, position: f32) {
    let looped = controller.ui.waveform.loop_enabled;
    record_play_start(controller, position);
    if let Err(err) = controller.play_audio(looped, Some(position)) {
        controller.set_status(err, StatusTone::Error);
    }
}

fn bpm_snap_step(controller: &EguiController) -> Option<f32> {
    if !controller.ui.waveform.bpm_snap_enabled {
        return None;
    }
    let bpm = controller.ui.waveform.bpm_value?;
    if !bpm.is_finite() || bpm <= 0.0 {
        return None;
    }
    let duration = controller
        .sample_view
        .wav
        .loaded_audio
        .as_ref()
        .map(|audio| audio.duration_seconds)?;
    if !duration.is_finite() || duration <= 0.0 {
        return None;
    }
    let step = 60.0 / bpm / duration;
    if step.is_finite() && step > 0.0 {
        Some(step)
    } else {
        None
    }
}

fn clear_too_small_bpm_selection(controller: &mut EguiController) {
    let Some(range) = controller.selection_state.range.range() else {
        return;
    };
    if super::selection_meets_bpm_min_for_playback(controller, range) {
        return;
    }
    controller.selection_state.range.set_range(None);
    controller.apply_selection(None);
}

fn snap_to_transient(controller: &EguiController, position: f32) -> Option<f32> {
    if !controller.ui.waveform.transient_markers_enabled
        || !controller.ui.waveform.transient_snap_enabled
    {
        return None;
    }
    let mut closest = None;
    let mut best_distance = TRANSIENT_SNAP_RADIUS;
    for &marker in &controller.ui.waveform.transients {
        let distance = (marker - position).abs();
        if distance <= best_distance {
            best_distance = distance;
            closest = Some(marker);
        }
    }
    closest
}

fn snap_selection_start(controller: &EguiController, position: f32) -> Option<f32> {
    if !controller.ui.waveform.bpm_snap_enabled {
        return None;
    }
    let radius = selection_start_snap_radius(controller);
    if position.is_finite() && radius.is_finite() && radius > 0.0 && position <= radius {
        Some(0.0)
    } else {
        None
    }
}

fn selection_start_snap_radius(controller: &EguiController) -> f32 {
    let mut radius = SELECTION_START_SNAP_RADIUS;
    let view_width = controller.ui.waveform.view.width();
    if view_width.is_finite() && view_width > 0.0 {
        radius = radius.min((view_width * SELECTION_START_SNAP_VIEW_FRACTION as f64) as f32);
    }
    if let Some(duration) = controller.loaded_audio_duration_seconds() {
        if duration.is_finite() && duration > 0.0 {
            radius = radius.min(SELECTION_START_SNAP_SECONDS / duration);
        }
    }
    radius
}

fn selection_scale_beats(controller: &EguiController) -> Option<f32> {
    if !controller.ui.waveform.bpm_snap_enabled {
        return None;
    }
    let bpm = controller.ui.waveform.bpm_value?;
    if !bpm.is_finite() || bpm <= 0.0 {
        return None;
    }
    let duration = controller.loaded_audio_duration_seconds()?;
    if !duration.is_finite() || duration <= 0.0 {
        return None;
    }
    let range = controller
        .selection_state
        .range
        .range()
        .or(controller.ui.waveform.selection)?;
    let seconds = range.width() * duration;
    if !seconds.is_finite() || seconds <= 0.0 {
        return None;
    }
    let beats = seconds * bpm / 60.0;
    if !beats.is_finite() || beats <= 0.0 {
        return None;
    }
    let rounded = beats.round();
    if (beats - rounded).abs() < 1.0e-3 {
        Some(rounded)
    } else {
        Some(beats)
    }
}

fn apply_scaled_bpm(controller: &mut EguiController, beats: f32, range: SelectionRange) {
    if !beats.is_finite() || beats <= 0.0 {
        return;
    }
    let duration = match controller.loaded_audio_duration_seconds() {
        Some(duration) if duration.is_finite() && duration > 0.0 => duration,
        _ => return,
    };
    let seconds = range.width() * duration;
    if !seconds.is_finite() || seconds <= 0.0 {
        return;
    }
    let bpm = beats * 60.0 / seconds;
    if !bpm.is_finite() || bpm <= 0.0 {
        return;
    }
    controller.set_bpm_value(bpm);
    controller.ui.waveform.bpm_input = format_bpm_input(bpm);
}

fn format_bpm_input(value: f32) -> String {
    let rounded = value.round();
    if (value - rounded).abs() < 0.01 {
        format!("{rounded:.0}")
    } else {
        format!("{value:.2}")
    }
}

pub(crate) fn replay_from_last_start(controller: &mut EguiController) -> bool {
    if let Some(position) = controller.ui.waveform.last_start_marker {
        seek_to(controller, position);
        return true;
    }
    if let Some(cursor) = controller.ui.waveform.cursor {
        seek_to(controller, cursor);
        return true;
    }
    if controller.ui.waveform.playhead.visible {
        seek_to(controller, controller.ui.waveform.playhead.position);
        return true;
    }
    false
}

pub(crate) fn play_from_cursor(controller: &mut EguiController) -> bool {
    if !controller.waveform_ready() {
        return false;
    }
    let cursor_from_navigation = match (
        controller.ui.waveform.cursor_last_hover_at,
        controller.ui.waveform.cursor_last_navigation_at,
    ) {
        (_, None) => false,
        (None, Some(_)) => true,
        (Some(hover), Some(nav)) => nav >= hover,
    };
    if cursor_from_navigation && let Some(cursor) = controller.ui.waveform.cursor {
        seek_to(controller, cursor);
        return true;
    }
    replay_from_last_start(controller)
}

pub(crate) fn record_play_start(controller: &mut EguiController, position: f32) {
    let clamped = position.clamp(0.0, 1.0);
    controller.ui.waveform.last_start_marker = Some(clamped);
    controller.set_waveform_cursor(clamped);
}

pub(crate) fn set_volume(controller: &mut EguiController, volume: f32) {
    controller.apply_volume(volume);
    let _ = controller.persist_config("Failed to save volume");
}

pub(crate) fn toggle_play_pause(controller: &mut EguiController) {
    let player_rc = match controller.ensure_player() {
        Ok(Some(p)) => p,
        Ok(None) => {
            controller.set_status("Audio unavailable", StatusTone::Error);
            return;
        }
        Err(err) => {
            controller.set_status(err, StatusTone::Error);
            return;
        }
    };
    let _is_playing = player_rc.borrow().is_playing();
    drop(player_rc);
    let _ = controller.play_audio(controller.ui.waveform.loop_enabled, None);
}

pub(crate) fn stop_playback_if_active(controller: &mut EguiController) -> bool {
    controller.audio.pending_loop_disable_at = None;
    let Some(player_rc) = controller.audio.player.as_ref() else {
        return false;
    };
    let stopped = {
        let mut player = player_rc.borrow_mut();
        if player.is_playing() {
            player.stop();
            true
        } else {
            false
        }
    };
    if stopped {
        controller.hide_waveform_playhead();
    }
    stopped
}

pub(crate) fn handle_escape(controller: &mut EguiController) {
    let selection_active = controller.selection_state.range.range().is_some()
        || controller.ui.waveform.selection.is_some()
        || controller.selection_state.edit_range.range().is_some()
        || controller.ui.waveform.edit_selection.is_some();
    let stopped_playback = stop_playback_if_active(controller);
    if !(selection_active && stopped_playback) {
        clear_selection(controller);
        clear_edit_selection(controller);
    }
    let had_cursor = controller.ui.waveform.cursor.take().is_some();
    if had_cursor {
        controller.ui.waveform.cursor_last_hover_at = None;
        controller.ui.waveform.cursor_last_navigation_at = None;
        controller.ui.waveform.last_start_marker = Some(0.0);
    }
    if !controller.ui.browser.selected_paths.is_empty() {
        controller.clear_browser_selection();
    }
    if matches!(controller.ui.focus.context, FocusContext::SourceFolders) {
        controller.clear_folder_selection();
    }
}
