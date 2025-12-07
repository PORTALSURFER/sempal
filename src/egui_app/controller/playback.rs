use super::*;
use crate::selection::SelectionEdge;
use std::time::{Duration, Instant};

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
        if self.ui.waveform.loop_enabled {
            self.pending_loop_disable_at = None;
            return;
        }
        if was_looping {
            if let Err(err) = self.defer_loop_disable_after_cycle() {
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

    /// Tag the currently selected wav and keep the current row selected.
    pub fn tag_selected(&mut self, target: SampleTag) {
        let Some(selected_index) = self.selected_row_index() else {
            return;
        };
        self.ui.collections.selected_sample = None;
        self.ui.browser.autoscroll = true;
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
        self.rebuild_browser_lists();
    }

    /// Move selection within the current sample browser list by an offset and play.
    pub fn nudge_selection(&mut self, offset: isize) {
        let list = self.visible_browser_indices().to_vec();
        if list.is_empty() {
            return;
        };
        let next_row = self.visible_row_after_offset(offset, &list);
        self.focus_browser_row(next_row);
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
        let Some(player) = self.player.as_ref().cloned() else {
            self.ui.waveform.playhead.visible = false;
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

    pub(super) fn apply_volume(&mut self, volume: f32) {
        let clamped = volume.clamp(0.0, 1.0);
        self.ui.volume = clamped;
        if let Some(player) = self.player.as_ref() {
            player.borrow_mut().set_volume(clamped);
        }
    }

    pub(super) fn ensure_player(&mut self) -> Result<Option<Rc<RefCell<AudioPlayer>>>, String> {
        if self.player.is_none() {
            let mut created =
                AudioPlayer::new().map_err(|err| format!("Audio init failed: {err}"))?;
            created.set_volume(self.ui.volume);
            self.player = Some(Rc::new(RefCell::new(created)));
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
