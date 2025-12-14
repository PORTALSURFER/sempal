use super::*;
use crate::selection::SelectionEdge;

impl EguiController {
    /// Begin a selection drag at the given normalized position.
    pub fn start_selection_drag(&mut self, position: f32) {
        let range = self.selection_state.range.begin_new(position);
        self.apply_selection(Some(range));
    }

    /// Begin dragging a specific selection edge; returns true when a selection exists.
    pub fn start_selection_edge_drag(&mut self, edge: SelectionEdge) -> bool {
        if !self.selection_state.range.begin_edge_drag(edge) {
            return false;
        }
        self.apply_selection(self.selection_state.range.range());
        true
    }

    /// Update the active selection drag with a new normalized position.
    pub fn update_selection_drag(&mut self, position: f32) {
        if let Some(range) = self.selection_state.range.update_drag(position) {
            self.apply_selection(Some(range));
        }
    }

    /// Finish a selection drag gesture.
    pub fn finish_selection_drag(&mut self) {
        self.selection_state.range.finish_drag();
        let is_playing = self
            .audio
            .player
            .as_ref()
            .map(|p| p.borrow().is_playing())
            .unwrap_or(false);
        if is_playing
            && self.ui.waveform.loop_enabled
            && let Err(err) = self.play_audio(true, None)
        {
            self.set_status(err, StatusTone::Error);
        }
    }

    /// Replace the current selection range without starting a drag gesture.
    pub fn set_selection_range(&mut self, range: SelectionRange) {
        self.selection_state.range.set_range(Some(range));
        self.apply_selection(Some(range));
    }

    /// True while a selection drag gesture is active.
    pub fn is_selection_dragging(&self) -> bool {
        self.selection_state.range.is_dragging()
    }

    /// Clear any active selection.
    pub fn clear_selection(&mut self) {
        let cleared = self.selection_state.range.clear();
        if cleared || self.ui.waveform.selection.is_some() {
            self.apply_selection(None);
        }
    }

    /// Toggle loop playback state, resuming current playback without looping when turned off.
    pub fn toggle_loop(&mut self) {
        let was_looping = self.ui.waveform.loop_enabled;
        self.ui.waveform.loop_enabled = !self.ui.waveform.loop_enabled;
        if self.ui.waveform.loop_enabled {
            self.audio.pending_loop_disable_at = None;
            if !was_looping {
                if let Some(player_rc) = self.audio.player.as_ref().cloned() {
                    let (is_playing, progress) = {
                        let player_ref = player_rc.borrow();
                        (player_ref.is_playing(), player_ref.progress())
                    };
                    if is_playing {
                        let start_override = progress.or_else(|| {
                            if self.ui.waveform.playhead.visible {
                                Some(self.ui.waveform.playhead.position)
                            } else {
                                self.ui
                                    .waveform
                                    .cursor
                                    .or(self.ui.waveform.last_start_marker)
                            }
                        });
                        if let Err(err) = self.play_audio(true, start_override) {
                            self.set_status(err, StatusTone::Error);
                        }
                    }
                }
            }
            return;
        }
        if was_looping && let Err(err) = self.defer_loop_disable_after_cycle() {
            self.set_status(err, StatusTone::Error);
        }
    }

    /// Seek to a normalized position and start playback.
    pub fn seek_to(&mut self, position: f32) {
        let looped = self.ui.waveform.loop_enabled;
        self.record_play_start(position);
        if let Err(err) = self.play_audio(looped, Some(position)) {
            self.set_status(err, StatusTone::Error);
        }
    }

    /// Replay from the last clicked start (or the current playhead) when available.
    pub fn replay_from_last_start(&mut self) -> bool {
        if let Some(position) = self.ui.waveform.last_start_marker {
            self.seek_to(position);
            return true;
        }
        if let Some(cursor) = self.ui.waveform.cursor {
            self.seek_to(cursor);
            return true;
        }
        if self.ui.waveform.playhead.visible {
            self.seek_to(self.ui.waveform.playhead.position);
            return true;
        }
        false
    }

    /// Play from the waveform cursor when available, falling back to last start/playhead.
    pub fn play_from_cursor(&mut self) -> bool {
        if !self.waveform_ready() {
            return false;
        }
        if let Some(cursor) = self.ui.waveform.cursor {
            self.seek_to(cursor);
            return true;
        }
        self.replay_from_last_start()
    }

    /// Remember the last user-requested play start position.
    pub fn record_play_start(&mut self, position: f32) {
        let clamped = position.clamp(0.0, 1.0);
        self.ui.waveform.last_start_marker = Some(clamped);
        self.set_waveform_cursor(clamped);
    }

    /// Update master output volume and persist the change.
    pub fn set_volume(&mut self, volume: f32) {
        self.apply_volume(volume);
        let _ = self.persist_config("Failed to save volume");
    }

    /// Toggle play/pause, preferring the current selection when present.
    pub fn toggle_play_pause(&mut self) {
        let player_rc = match self.ensure_player() {
            Ok(Some(p)) => p,
            Ok(None) => {
                self.set_status("Audio unavailable", StatusTone::Error);
                return;
            }
            Err(err) => {
                self.set_status(err, StatusTone::Error);
                return;
            }
        };
        let _is_playing = player_rc.borrow().is_playing();
        drop(player_rc);
        // Always start playback from the selection/full track, restarting if currently playing.
        let _ = self.play_audio(self.ui.waveform.loop_enabled, None);
    }

    /// Stop playback when active, returning true if anything was stopped.
    pub fn stop_playback_if_active(&mut self) -> bool {
        self.audio.pending_loop_disable_at = None;
        let Some(player_rc) = self.audio.player.as_ref() else {
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
            self.hide_waveform_playhead();
        }
        stopped
    }

    /// Handle Escape input by stopping playback and clearing selections across panels.
    pub fn handle_escape(&mut self) {
        let selection_active = self.selection_state.range.range().is_some() || self.ui.waveform.selection.is_some();
        let stopped_playback = self.stop_playback_if_active();
        if !(selection_active && stopped_playback) {
            self.clear_selection();
        }
        let had_cursor = self.ui.waveform.cursor.take().is_some();
        if had_cursor {
            self.ui.waveform.cursor_last_hover_at = None;
            self.ui.waveform.cursor_last_navigation_at = None;
            self.ui.waveform.last_start_marker = Some(0.0);
        }
        if !self.ui.browser.selected_paths.is_empty() {
            self.clear_browser_selection();
        }
        self.clear_folder_selection();
    }
}
