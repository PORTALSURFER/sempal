use super::*;
use std::time::{Duration, Instant};

impl EguiController {
    /// Start playback over the current selection or full range.
    pub fn play_audio(&mut self, looped: bool, start_override: Option<f32>) -> Result<(), String> {
        self.audio.pending_loop_disable_at = None;
        if self.sample_view.wav.loaded_audio.is_none() {
            if let Some(pending) = self.runtime.jobs.pending_audio.clone() {
                self.runtime.jobs.pending_playback = Some(PendingPlayback {
                    source_id: pending.source_id,
                    relative_path: pending.relative_path,
                    looped,
                    start_override,
                });
                self.set_status("Loading audio…", StatusTone::Busy);
                return Ok(());
            }
            let Some(selected) = self.sample_view.wav.selected_wav.clone() else {
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
            self.runtime.jobs.pending_playback = Some(pending_playback.clone());
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
            .selection_state
            .range
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
        self.audio.player
            .as_ref()
            .map(|p| p.borrow().is_playing())
            .unwrap_or(false)
    }

    /// Advance playhead position and visibility from the underlying player.
    pub fn tick_playhead(&mut self) {
        self.poll_wav_loader();
        self.poll_audio_loader();
        self.poll_scan();
        self.poll_trash_move();
        let Some(player) = self.audio.player.as_ref().cloned() else {
            if self.sample_view.waveform.decoded.is_none() {
                self.hide_waveform_playhead();
            }
            return;
        };
        let should_resume = {
            let player_ref = player.borrow();
            match self.audio.pending_loop_disable_at {
                Some(_) if !player_ref.is_playing() || !player_ref.is_looping() => {
                    self.audio.pending_loop_disable_at = None;
                    false
                }
                Some(deadline) => Instant::now() >= deadline,
                None => false,
            }
        };
        if should_resume {
            self.audio.pending_loop_disable_at = None;
            player.borrow_mut().stop();
        }
        let player_ref = player.borrow();
        let is_playing = player_ref.is_playing();
        let progress = player_ref.progress();
        let is_looping = player_ref.is_looping();
        drop(player_ref);
        self.update_playhead_from_progress(progress, is_looping);
        if !is_playing && self.sample_view.waveform.decoded.is_none() {
            self.hide_waveform_playhead();
        }
    }

    pub(super) fn update_playhead_from_progress(&mut self, progress: Option<f32>, is_looping: bool) {
        if let Some(progress) = progress {
            self.ui.waveform.playhead.position = progress;
            if self.playhead_completed_span(progress, is_looping) {
                self.hide_waveform_playhead();
            } else {
                self.ui.waveform.playhead.visible = true;
            }
        } else {
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

    pub(super) fn hide_waveform_playhead(&mut self) {
        self.ui.waveform.playhead.visible = false;
        self.ui.waveform.playhead.active_span_end = None;
    }

    #[cfg(test)]
    /// Expose playhead completion logic for unit tests.
    pub(crate) fn playhead_completed_span_for_tests(&self, progress: f32, is_looping: bool) -> bool {
        self.playhead_completed_span(progress, is_looping)
    }

    #[cfg(test)]
    /// Allow tests to reset the playhead visibility tracking.
    pub(crate) fn hide_waveform_playhead_for_tests(&mut self) {
        self.hide_waveform_playhead();
    }

    pub(in crate::egui_app::controller) fn apply_selection(&mut self, range: Option<SelectionRange>) {
        let label = range.and_then(|selection| self.selection_duration_label(selection));
        self.ui.waveform.selection = range;
        self.ui.waveform.selection_duration = label;
    }

    /// Update the cached hover time label for the waveform cursor.
    pub fn update_waveform_hover_time(&mut self, position: Option<f32>) {
        if let (Some(position), Some(audio)) = (position, self.sample_view.wav.loaded_audio.as_ref()) {
            let clamped = position.clamp(0.0, 1.0);
            let seconds = audio.duration_seconds * clamped;
            self.ui.waveform.hover_time_label = Some(format_timestamp_hms_ms(seconds));
        } else {
            self.ui.waveform.hover_time_label = None;
        }
    }

    pub(super) fn selection_duration_label(&self, range: SelectionRange) -> Option<String> {
        let audio = self.sample_view.wav.loaded_audio.as_ref()?;
        let seconds = (audio.duration_seconds * range.width()).max(0.0);
        Some(format_selection_duration(seconds))
    }

    pub(in crate::egui_app::controller) fn apply_volume(&mut self, volume: f32) {
        let clamped = volume.clamp(0.0, 1.0);
        self.ui.volume = clamped;
        if let Some(player) = self.audio.player.as_ref() {
            player.borrow_mut().set_volume(clamped);
        }
    }

    pub(in crate::egui_app::controller) fn ensure_player(
        &mut self,
    ) -> Result<Option<Rc<RefCell<AudioPlayer>>>, String> {
        if self.audio.player.is_none() {
            let mut created = AudioPlayer::from_config(&self.settings.audio_output)
                .map_err(|err| format!("Audio init failed: {err}"))?;
            created.set_volume(self.ui.volume);
            self.audio.player = Some(Rc::new(RefCell::new(created)));
            self.update_audio_output_status();
        }
        Ok(self.audio.player.clone())
    }

    pub(super) fn defer_loop_disable_after_cycle(&mut self) -> Result<(), String> {
        self.audio.pending_loop_disable_at = None;
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

        self.audio.pending_loop_disable_at = Some(Instant::now() + remaining);
        Ok(())
    }
}
