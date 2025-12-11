use super::*;
use crate::selection::SelectionEdge;
use rand::Rng;
use rand::seq::IteratorRandom;
#[cfg(test)]
use rand::{SeedableRng, rngs::StdRng};
use std::path::PathBuf;
use std::time::{Duration, Instant};

#[cfg(test)]
const SHOULD_PLAY_RANDOM_SAMPLE: bool = false;
#[cfg(not(test))]
const SHOULD_PLAY_RANDOM_SAMPLE: bool = true;
const PLAYHEAD_COMPLETION_EPSILON: f32 = 0.001;

impl EguiController {
    /// Begin a selection drag at the given normalized position.
    pub fn start_selection_drag(&mut self, position: f32) {
        let range = self.selection.begin_new(position);
        self.apply_selection(Some(range));
    }

    /// Begin dragging a specific selection edge; returns true when a selection exists.
    pub fn start_selection_edge_drag(&mut self, edge: SelectionEdge) -> bool {
        if !self.selection.begin_edge_drag(edge) {
            return false;
        }
        self.apply_selection(self.selection.range());
        true
    }

    /// Update the active selection drag with a new normalized position.
    pub fn update_selection_drag(&mut self, position: f32) {
        if let Some(range) = self.selection.update_drag(position) {
            self.apply_selection(Some(range));
        }
    }

    /// Finish a selection drag gesture.
    pub fn finish_selection_drag(&mut self) {
        self.selection.finish_drag();
        let is_playing = self
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

    /// True while a selection drag gesture is active.
    pub fn is_selection_dragging(&self) -> bool {
        self.selection.is_dragging()
    }

    /// Clear any active selection.
    pub fn clear_selection(&mut self) {
        let cleared = self.selection.clear();
        if cleared || self.ui.waveform.selection.is_some() {
            self.apply_selection(None);
        }
    }

    /// Toggle loop playback state, resuming current playback without looping when turned off.
    pub fn toggle_loop(&mut self) {
        let was_looping = self.ui.waveform.loop_enabled;
        self.ui.waveform.loop_enabled = !self.ui.waveform.loop_enabled;
        if self.ui.waveform.loop_enabled {
            self.pending_loop_disable_at = None;
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
        self.pending_loop_disable_at = None;
        let Some(player_rc) = self.player.as_ref() else {
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
        let selection_active =
            self.selection.range().is_some() || self.ui.waveform.selection.is_some();
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

    /// Tag the focused/selected wavs and keep the current focus.
    pub fn tag_selected(&mut self, target: SampleTag) {
        let Some(selected_index) = self.selected_row_index() else {
            return;
        };
        let primary_row = match self
            .visible_browser_indices()
            .iter()
            .position(|idx| *idx == selected_index)
        {
            Some(row) => row,
            None => return,
        };
        let rows = self.action_rows_from_primary(primary_row);
        self.ui.collections.selected_sample = None;
        self.focus_browser_context();
        self.ui.browser.autoscroll = true;
        let mut last_error = None;
        for row in rows {
            if let Err(err) = self.tag_browser_sample(row, target) {
                last_error = Some(err);
            }
        }
        self.refocus_after_filtered_removal(primary_row);
        if let Some(err) = last_error {
            self.set_status(err, StatusTone::Error);
        }
    }

    /// Move selection within the current sample browser list by an offset and play.
    pub fn nudge_selection(&mut self, offset: isize) {
        let list = self.visible_browser_indices().to_vec();
        if list.is_empty() {
            return;
        };
        let next_row = self.visible_row_after_offset(offset, &list);
        self.focus_browser_row_only(next_row);
        let _ = self.play_audio(self.ui.waveform.loop_enabled, None);
    }

    /// Extend selection with shift navigation while keeping the current focus for playback.
    pub fn grow_selection(&mut self, offset: isize) {
        let list = self.visible_browser_indices().to_vec();
        if list.is_empty() {
            return;
        };
        let next_row = self.visible_row_after_offset(offset, &list);
        self.extend_browser_selection_to_row(next_row);
        let _ = self.play_audio(self.ui.waveform.loop_enabled, None);
    }

    /// Jump to a random visible sample in the browser and start playback.
    pub fn play_random_visible_sample(&mut self) {
        let mut rng = rand::rng();
        self.play_random_visible_sample_internal(&mut rng, SHOULD_PLAY_RANDOM_SAMPLE);
    }

    #[cfg(test)]
    pub(super) fn play_random_visible_sample_with_seed(&mut self, seed: u64) {
        let mut rng = StdRng::seed_from_u64(seed);
        self.play_random_visible_sample_internal(&mut rng, false);
    }

    /// Focus a random visible sample without starting playback (used for navigation flows).
    pub fn focus_random_visible_sample(&mut self) {
        let mut rng = rand::rng();
        self.play_random_visible_sample_internal(&mut rng, false);
    }
    /// Play the previous entry from the random history stack.
    pub fn play_previous_random_sample(&mut self) {
        if self.random_history.is_empty() {
            self.set_status("No random history yet", StatusTone::Info);
            return;
        }
        let current = self
            .random_history_cursor
            .unwrap_or_else(|| self.random_history.len().saturating_sub(1));
        if current == 0 {
            self.random_history_cursor = Some(0);
            self.set_status("Reached start of random history", StatusTone::Info);
            return;
        }
        let target = current - 1;
        self.random_history_cursor = Some(target);
        if let Some(entry) = self.random_history.get(target).cloned() {
            self.play_random_history_entry(entry);
        }
    }

    /// Toggle sticky random navigation for Up/Down in the browser.
    pub fn toggle_random_navigation_mode(&mut self) {
        self.ui.browser.random_navigation_mode = !self.ui.browser.random_navigation_mode;
        if self.ui.browser.random_navigation_mode {
            self.set_status(
                "Random navigation on: Up/Down jump to random samples",
                StatusTone::Info,
            );
        } else {
            self.set_status("Random navigation off", StatusTone::Info);
        }
    }

    pub fn random_navigation_mode_enabled(&self) -> bool {
        self.ui.browser.random_navigation_mode
    }

    fn play_random_visible_sample_internal<R: Rng + ?Sized>(
        &mut self,
        rng: &mut R,
        start_playback: bool,
    ) {
        let Some(source_id) = self.selected_source.clone() else {
            self.set_status("Select a source first", StatusTone::Info);
            return;
        };
        let Some((visible_row, entry_index)) = self
            .visible_browser_indices()
            .iter()
            .copied()
            .enumerate()
            .choose(rng)
        else {
            self.set_status("No samples available to randomize", StatusTone::Info);
            return;
        };
        let Some(path) = self
            .wav_entries
            .get(entry_index)
            .map(|entry| entry.relative_path.clone())
        else {
            return;
        };
        self.push_random_history(source_id, path.clone());
        self.focus_browser_row_only(visible_row);
        if start_playback && let Err(err) = self.play_audio(self.ui.waveform.loop_enabled, None) {
            self.set_status(err, StatusTone::Error);
        }
    }

    fn push_random_history(&mut self, source_id: SourceId, relative_path: PathBuf) {
        if let Some(cursor) = self.random_history_cursor
            && cursor + 1 < self.random_history.len()
        {
            self.random_history.truncate(cursor + 1);
        }
        self.random_history.push_back(RandomHistoryEntry {
            source_id,
            relative_path,
        });
        if self.random_history.len() > RANDOM_HISTORY_LIMIT {
            self.random_history.pop_front();
            if let Some(cursor) = self.random_history_cursor {
                self.random_history_cursor = Some(cursor.saturating_sub(1));
            }
        }
        self.random_history_cursor = Some(self.random_history.len().saturating_sub(1));
    }

    fn play_random_history_entry(&mut self, entry: RandomHistoryEntry) {
        if self.selected_source.as_ref() != Some(&entry.source_id) {
            self.pending_playback = Some(PendingPlayback {
                source_id: entry.source_id.clone(),
                relative_path: entry.relative_path.clone(),
                looped: self.ui.waveform.loop_enabled,
                start_override: None,
            });
            self.pending_select_path = Some(entry.relative_path.clone());
            self.select_source_internal(Some(entry.source_id), Some(entry.relative_path));
            return;
        }
        if let Some(row) = self.visible_row_for_path(&entry.relative_path) {
            self.focus_browser_row_only(row);
        } else {
            self.select_wav_by_path(&entry.relative_path);
        }
        if let Err(err) = self.play_audio(self.ui.waveform.loop_enabled, None) {
            self.set_status(err, StatusTone::Error);
        }
    }

    /// Cycle the triage flag filter (-1 left, +1 right) to mirror old column navigation.
    pub fn move_selection_column(&mut self, delta: isize) {
        use crate::egui_app::state::TriageFlagFilter::*;
        let filters = [All, Keep, Trash, Untagged];
        let current = self.ui.browser.filter;
        let current_idx = filters.iter().position(|f| f == &current).unwrap_or(0) as isize;
        let target_idx = (current_idx + delta).clamp(0, (filters.len() as isize) - 1) as usize;
        let target = filters[target_idx];
        self.set_browser_filter(target);
    }

    /// Tag leftwards: Keep -> Neutral, otherwise -> Trash.
    pub fn tag_selected_left(&mut self) {
        let target = match self.selected_tag() {
            Some(SampleTag::Keep) => SampleTag::Neutral,
            _ => SampleTag::Trash,
        };
        self.tag_selected(target);
    }

    fn visible_row_after_offset(&self, offset: isize, list: &[usize]) -> usize {
        let current_row = self
            .ui
            .browser
            .selected_visible
            .or_else(|| {
                self.selected_row_index()
                    .and_then(|idx| list.iter().position(|i| *i == idx))
            })
            .unwrap_or(0) as isize;
        (current_row + offset).clamp(0, list.len() as isize - 1) as usize
    }

    /// Start playback over the current selection or full range.
    pub fn play_audio(&mut self, looped: bool, start_override: Option<f32>) -> Result<(), String> {
        self.pending_loop_disable_at = None;
        if self.loaded_audio.is_none() {
            if let Some(pending) = self.pending_audio.clone() {
                self.pending_playback = Some(PendingPlayback {
                    source_id: pending.source_id,
                    relative_path: pending.relative_path,
                    looped,
                    start_override,
                });
                self.set_status("Loading audio…", StatusTone::Busy);
                return Ok(());
            }
            let Some(selected) = self.selected_wav.clone() else {
                return Err("Load a .wav file first".into());
            };
            let Some(source) = self.current_source() else {
                return Err("Load a .wav file first".into());
            };
            let pending_playback = PendingPlayback {
                source_id: source.id.clone(),
                relative_path: selected.clone(),
                looped,
                start_override,
            };
            self.pending_playback = Some(pending_playback.clone());
            self.queue_audio_load_for(
                &source,
                &selected,
                AudioLoadIntent::Selection,
                Some(pending_playback),
            )?;
            self.set_status(format!("Loading {}", selected.display()), StatusTone::Busy);
            return Ok(());
        }
        let player = self.ensure_player()?;
        let Some(player) = player else {
            return Err("Audio unavailable".into());
        };
        let selection = self
            .selection
            .range()
            .filter(|range| range.width() >= MIN_SELECTION_WIDTH);
        let start = start_override
            .or_else(|| selection.as_ref().map(|range| range.start()))
            .unwrap_or(0.0);
        let span_end = selection.as_ref().map(|r| r.end()).unwrap_or(1.0);
        if looped && selection.is_none() && start_override.is_some() {
            player.borrow_mut().play_full_wrapped_from(start)?;
        } else {
            player.borrow_mut().play_range(start, span_end, looped)?;
        }
        self.ui.waveform.playhead.active_span_end = Some(span_end.clamp(0.0, 1.0));
        self.ui.waveform.playhead.visible = true;
        self.ui.waveform.playhead.position = start;
        Ok(())
    }

    /// True when the underlying player is currently playing.
    pub fn is_playing(&self) -> bool {
        self.player
            .as_ref()
            .map(|p| p.borrow().is_playing())
            .unwrap_or(false)
    }

    /// Advance playhead position and visibility from the underlying player.
    pub fn tick_playhead(&mut self) {
        self.poll_wav_loader();
        self.poll_audio_loader();
        self.poll_scan();
        let Some(player) = self.player.as_ref().cloned() else {
            if self.decoded_waveform.is_none() {
                self.hide_waveform_playhead();
            }
            return;
        };
        let should_resume = {
            let player_ref = player.borrow();
            match self.pending_loop_disable_at {
                Some(_) if !player_ref.is_playing() || !player_ref.is_looping() => {
                    self.pending_loop_disable_at = None;
                    false
                }
                Some(deadline) => Instant::now() >= deadline,
                None => false,
            }
        };
        if should_resume {
            self.pending_loop_disable_at = None;
            player.borrow_mut().stop();
        }
        let player_ref = player.borrow();
        if player_ref.is_playing() {
            if let Some(progress) = player_ref.progress() {
                self.ui.waveform.playhead.position = progress;
                if self.playhead_completed_span(progress, player_ref.is_looping()) {
                    self.hide_waveform_playhead();
                } else {
                    self.ui.waveform.playhead.visible = true;
                }
            } else {
                self.hide_waveform_playhead();
            }
        } else if self.decoded_waveform.is_none() {
            self.hide_waveform_playhead();
        }
    }

    fn playhead_completed_span(&self, progress: f32, is_looping: bool) -> bool {
        if is_looping {
            return false;
        }
        let target = self
            .ui
            .waveform
            .playhead
            .active_span_end
            .unwrap_or(1.0)
            .clamp(0.0, 1.0);
        progress + PLAYHEAD_COMPLETION_EPSILON >= target
    }

    fn hide_waveform_playhead(&mut self) {
        self.ui.waveform.playhead.visible = false;
        self.ui.waveform.playhead.active_span_end = None;
    }

    #[cfg(test)]
    /// Expose playhead completion logic for unit tests.
    pub(crate) fn playhead_completed_span_for_tests(
        &self,
        progress: f32,
        is_looping: bool,
    ) -> bool {
        self.playhead_completed_span(progress, is_looping)
    }

    #[cfg(test)]
    /// Allow tests to reset the playhead visibility tracking.
    pub(crate) fn hide_waveform_playhead_for_tests(&mut self) {
        self.hide_waveform_playhead();
    }

    pub(super) fn apply_selection(&mut self, range: Option<SelectionRange>) {
        let label = range.and_then(|selection| self.selection_duration_label(selection));
        self.ui.waveform.selection = range;
        self.ui.waveform.selection_duration = label;
    }

    /// Update the cached hover time label for the waveform cursor.
    pub fn update_waveform_hover_time(&mut self, position: Option<f32>) {
        if let (Some(position), Some(audio)) = (position, self.loaded_audio.as_ref()) {
            let clamped = position.clamp(0.0, 1.0);
            let seconds = audio.duration_seconds * clamped;
            self.ui.waveform.hover_time_label = Some(format_timestamp_hms_ms(seconds));
        } else {
            self.ui.waveform.hover_time_label = None;
        }
    }

    fn selection_duration_label(&self, range: SelectionRange) -> Option<String> {
        let audio = self.loaded_audio.as_ref()?;
        let seconds = (audio.duration_seconds * range.width()).max(0.0);
        Some(format_selection_duration(seconds))
    }

    pub(super) fn apply_volume(&mut self, volume: f32) {
        let clamped = volume.clamp(0.0, 1.0);
        self.ui.volume = clamped;
        if let Some(player) = self.player.as_ref() {
            player.borrow_mut().set_volume(clamped);
        }
    }

    pub(super) fn ensure_player(&mut self) -> Result<Option<Rc<RefCell<AudioPlayer>>>, String> {
        if self.player.is_none() {
            let mut created = AudioPlayer::from_config(&self.audio_output)
                .map_err(|err| format!("Audio init failed: {err}"))?;
            created.set_volume(self.ui.volume);
            self.player = Some(Rc::new(RefCell::new(created)));
            self.update_audio_output_status();
        }
        Ok(self.player.clone())
    }

    fn defer_loop_disable_after_cycle(&mut self) -> Result<(), String> {
        self.pending_loop_disable_at = None;
        let Some(player_rc) = self.ensure_player()? else {
            return Ok(());
        };
        let player_ref = player_rc.borrow();
        let remaining = player_ref.remaining_loop_duration();
        let is_playing = player_ref.is_playing();
        let is_looping = player_ref.is_looping();
        drop(player_ref);

        if !is_playing || !is_looping {
            return Ok(());
        }

        let Some(remaining) = remaining else {
            player_rc.borrow_mut().stop();
            return Ok(());
        };
        if remaining <= Duration::from_millis(5) {
            player_rc.borrow_mut().stop();
            return Ok(());
        }

        self.pending_loop_disable_at = Some(Instant::now() + remaining);
        Ok(())
    }
}

fn format_selection_duration(seconds: f32) -> String {
    if !seconds.is_finite() || seconds <= 0.0 {
        return "0 ms".to_string();
    }
    if seconds < 1.0 {
        return format!("{:.0} ms", seconds * 1_000.0);
    }
    if seconds < 60.0 {
        return format!("{:.2} s", seconds);
    }
    let minutes = (seconds / 60.0).floor() as u32;
    let remaining = seconds - minutes as f32 * 60.0;
    format!("{minutes}m {remaining:05.2}s")
}

/// Format an absolute timestamp into `HH:MM:SS:MS` where `MS` is zero-padded milliseconds.
fn format_timestamp_hms_ms(seconds: f32) -> String {
    if !seconds.is_finite() || seconds < 0.0 {
        return "00:00:00:000".to_string();
    }
    let total_ms = (seconds * 1_000.0).round() as u64;
    let hours = total_ms / 3_600_000;
    let minutes = (total_ms / 60_000) % 60;
    let secs = (total_ms / 1_000) % 60;
    let millis = total_ms % 1_000;
    format!("{hours:02}:{minutes:02}:{secs:02}:{millis:03}")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::egui_app::controller::test_support;
    use std::path::PathBuf;

    #[test]
    fn format_selection_duration_scales_units() {
        assert_eq!(format_selection_duration(0.75), "750 ms");
        assert_eq!(format_selection_duration(1.5), "1.50 s");
        assert_eq!(format_selection_duration(125.0), "2m 05.00s");
    }

    #[test]
    fn selection_duration_label_uses_loaded_audio() {
        let (mut controller, source) = test_support::dummy_controller();
        controller.loaded_audio = Some(LoadedAudio {
            source_id: source.id.clone(),
            relative_path: PathBuf::from("clip.wav"),
            bytes: Vec::new(),
            duration_seconds: 4.0,
            sample_rate: 48_000,
            channels: 2,
        });
        let label = controller.selection_duration_label(SelectionRange::new(0.25, 0.75));
        assert_eq!(label.as_deref(), Some("2.00 s"));
    }

    #[test]
    fn selection_duration_label_is_absent_without_audio() {
        let (controller, _) = test_support::dummy_controller();
        let label = controller.selection_duration_label(SelectionRange::new(0.0, 1.0));
        assert!(label.is_none());
    }

    #[test]
    fn format_timestamp_zero_pads_and_rounds() {
        assert_eq!(format_timestamp_hms_ms(0.0), "00:00:00:000");
        assert_eq!(format_timestamp_hms_ms(1.234), "00:00:01:234");
        assert_eq!(format_timestamp_hms_ms(59.9995), "00:01:00:000");
    }

    #[test]
    fn format_timestamp_handles_hours() {
        assert_eq!(format_timestamp_hms_ms(3_661.789), "01:01:01:789");
        assert_eq!(format_timestamp_hms_ms(-0.5), "00:00:00:000");
    }
}
