use super::*;
use crate::selection::SelectionEdge;

pub(super) fn start_selection_drag(controller: &mut EguiController, position: f32) {
    let range = controller.selection_state.range.begin_new(position);
    controller.apply_selection(Some(range));
}

pub(super) fn start_selection_edge_drag(
    controller: &mut EguiController,
    edge: SelectionEdge,
) -> bool {
    if !controller.selection_state.range.begin_edge_drag(edge) {
        return false;
    }
    controller.apply_selection(controller.selection_state.range.range());
    true
}

pub(super) fn update_selection_drag(controller: &mut EguiController, position: f32) {
    let range = if let Some(step) = bpm_snap_step(controller) {
        controller
            .selection_state
            .range
            .update_drag_snapped(position, step)
    } else {
        controller.selection_state.range.update_drag(position)
    };
    if let Some(range) = range {
        controller.apply_selection(Some(range));
    }
}

pub(super) fn finish_selection_drag(controller: &mut EguiController) {
    controller.selection_state.range.finish_drag();
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
        .filter(|range| range.width() >= MIN_SELECTION_WIDTH)
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

pub(super) fn set_selection_range(controller: &mut EguiController, range: SelectionRange) {
    controller.selection_state.range.set_range(Some(range));
    controller.apply_selection(Some(range));
}

pub(super) fn is_selection_dragging(controller: &EguiController) -> bool {
    controller.selection_state.range.is_dragging()
}

pub(super) fn clear_selection(controller: &mut EguiController) {
    let cleared = controller.selection_state.range.clear();
    if cleared || controller.ui.waveform.selection.is_some() {
        controller.apply_selection(None);
    }
}

pub(super) fn toggle_loop(controller: &mut EguiController) {
    let was_looping = controller.ui.waveform.loop_enabled;
    controller.ui.waveform.loop_enabled = !controller.ui.waveform.loop_enabled;
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
                        .filter(|range| range.width() >= MIN_SELECTION_WIDTH)
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

pub(super) fn seek_to(controller: &mut EguiController, position: f32) {
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

pub(super) fn replay_from_last_start(controller: &mut EguiController) -> bool {
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

pub(super) fn play_from_cursor(controller: &mut EguiController) -> bool {
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

pub(super) fn record_play_start(controller: &mut EguiController, position: f32) {
    let clamped = position.clamp(0.0, 1.0);
    controller.ui.waveform.last_start_marker = Some(clamped);
    controller.set_waveform_cursor(clamped);
}

pub(super) fn set_volume(controller: &mut EguiController, volume: f32) {
    controller.apply_volume(volume);
    let _ = controller.persist_config("Failed to save volume");
}

pub(super) fn toggle_play_pause(controller: &mut EguiController) {
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

pub(super) fn stop_playback_if_active(controller: &mut EguiController) -> bool {
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

pub(super) fn handle_escape(controller: &mut EguiController) {
    let selection_active = controller.selection_state.range.range().is_some()
        || controller.ui.waveform.selection.is_some();
    let stopped_playback = stop_playback_if_active(controller);
    if !(selection_active && stopped_playback) {
        clear_selection(controller);
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
    controller.clear_folder_selection();
}
