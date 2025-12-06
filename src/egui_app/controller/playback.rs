use super::*;

impl EguiController {
    /// Begin a selection drag at the given normalized position.
    pub fn start_selection_drag(&mut self, position: f32) {
        let range = self.selection.begin_new(position);
        self.apply_selection(Some(range));
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
        if is_playing && self.ui.waveform.loop_enabled {
            if let Err(err) = self.play_audio(true, None) {
                self.set_status(err, StatusTone::Error);
            }
        }
    }

    /// Clear any active selection.
    pub fn clear_selection(&mut self) {
        if self.selection.clear() {
            self.apply_selection(None);
        }
    }

    /// Toggle loop playback state, resuming current playback without looping when turned off.
    pub fn toggle_loop(&mut self) {
        let was_looping = self.ui.waveform.loop_enabled;
        self.ui.waveform.loop_enabled = !self.ui.waveform.loop_enabled;
        if was_looping && !self.ui.waveform.loop_enabled {
            if let Err(err) = self.resume_without_looping() {
                self.set_status(err, StatusTone::Error);
            }
        }
    }

    /// Seek to a normalized position and start playback.
    pub fn seek_to(&mut self, position: f32) {
        if let Err(err) = self.play_audio(false, Some(position)) {
            self.set_status(err, StatusTone::Error);
        }
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

    /// Tag the currently selected wav and advance to the next row in the original column.
    pub fn tag_selected(&mut self, target: SampleTag) {
        let Some(TriageIndex { column, row }) = self.ui.triage.selected else {
            return;
        };
        self.ui.collections.selected_sample = None;
        self.ui.triage.autoscroll = true;
        let Some(selected_index) = self.selected_row_index() else {
            return;
        };
        let moved_entry_index = selected_index;
        let original_list: Vec<usize> = self.triage_indices(column).to_vec();
        let next_candidate = if row + 1 < original_list.len() {
            original_list.get(row + 1).copied()
        } else if row > 0 {
            original_list.get(row - 1).copied()
        } else {
            None
        };
        let path = match self.wav_entries.get(selected_index) {
            Some(entry) => entry.relative_path.clone(),
            None => return,
        };
        let Some(source) = self.current_source() else {
            return;
        };
        let db = match self.database_for(&source) {
            Ok(db) => db,
            Err(err) => {
                self.set_status(err.to_string(), StatusTone::Error);
                return;
            }
        };
        if let Some(entry) = self.wav_entries.get_mut(selected_index) {
            entry.tag = target;
        }
        if let Some(cache) = self.wav_cache.get_mut(&source.id) {
            if let Some(entry) = cache.get_mut(selected_index) {
                entry.tag = target;
            }
        }
        let _ = db.set_tag(&path, target);
        self.rebuild_triage_lists();
        // If we moved the last item out of a column, keep selection on the moved item.
        if original_list.len() == 1 || (row + 1 == original_list.len() && next_candidate.is_none())
        {
            self.select_wav_by_index(moved_entry_index);
            return;
        }
        if let Some(next_index) = next_candidate {
            self.select_wav_by_index(next_index);
        }
    }

    /// Move selection within the current triage column by an offset and play.
    pub fn nudge_selection(&mut self, offset: isize) {
        let selected_triage = self.ui.triage.selected;
        let Some(TriageIndex { column, row }) = selected_triage else {
            return;
        };
        self.ui.collections.selected_sample = None;
        self.ui.triage.autoscroll = true;
        let list = self.triage_indices(column);
        if list.is_empty() {
            return;
        }
        let current_row = row as isize;
        let next_row = (current_row + offset).clamp(0, list.len() as isize - 1) as usize;
        if let Some(entry_index) = list.get(next_row).copied() {
            self.select_wav_by_index(entry_index);
            let _ = self.play_audio(self.ui.waveform.loop_enabled, None);
        }
    }

    /// Move selection to the same row in a neighboring column (-1 left, +1 right).
    pub fn move_selection_column(&mut self, delta: isize) {
        use crate::egui_app::state::TriageColumn::*;
        let columns = [Trash, Neutral, Keep];
        let current = self.ui.triage.selected.map(|t| t.column).unwrap_or(Neutral);
        let current_idx = columns.iter().position(|c| c == &current).unwrap_or(1) as isize;
        let target_idx = (current_idx + delta).clamp(0, (columns.len() as isize) - 1) as usize;
        if target_idx == current_idx as usize {
            return;
        }
        self.ui.collections.selected_sample = None;
        self.ui.triage.autoscroll = true;
        let target_col = columns[target_idx];
        let list = self.triage_indices(target_col);
        if list.is_empty() {
            return;
        }
        let row = self.ui.triage.selected.map(|t| t.row).unwrap_or(0);
        let clamped_row = row.min(list.len().saturating_sub(1));
        if let Some(entry_index) = list.get(clamped_row).copied() {
            self.select_wav_by_index(entry_index);
            let _ = self.play_audio(self.ui.waveform.loop_enabled, None);
        }
    }

    /// Start playback over the current selection or full range.
    pub fn play_audio(&mut self, looped: bool, start_override: Option<f32>) -> Result<(), String> {
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
        player.borrow_mut().play_range(start, span_end, looped)?;
        self.ui.waveform.playhead.visible = true;
        self.ui.waveform.playhead.position = start;
        Ok(())
    }

    /// Advance playhead position and visibility from the underlying player.
    pub fn tick_playhead(&mut self) {
        self.poll_wav_loader();
        self.poll_scan();
        let Some(player) = self.player.as_ref() else {
            self.ui.waveform.playhead.visible = false;
            return;
        };
        let player_ref = player.borrow();
        if let Some(progress) = player_ref.progress() {
            self.ui.waveform.playhead.position = progress;
            self.ui.waveform.playhead.visible = player_ref.is_playing();
        } else {
            self.ui.waveform.playhead.visible = false;
        }
    }

    fn apply_selection(&mut self, range: Option<SelectionRange>) {
        if let Some(range) = range {
            self.ui.waveform.selection = Some(range);
        } else {
            self.ui.waveform.selection = None;
        }
    }

    pub(super) fn ensure_player(&mut self) -> Result<Option<Rc<RefCell<AudioPlayer>>>, String> {
        if self.player.is_none() {
            let created = AudioPlayer::new().map_err(|err| format!("Audio init failed: {err}"))?;
            self.player = Some(Rc::new(RefCell::new(created)));
        }
        Ok(self.player.clone())
    }

    fn resume_without_looping(&mut self) -> Result<(), String> {
        let Some(player_rc) = self.ensure_player()? else {
            return Ok(());
        };
        if !player_rc.borrow().is_playing() {
            return Ok(());
        }
        let progress = player_rc.borrow().progress();
        drop(player_rc);
        if let Some(position) = progress {
            self.suppress_autoplay_once = true;
            self.play_audio(false, Some(position))?;
        }
        Ok(())
    }
}
