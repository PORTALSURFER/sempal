use super::*;
pub(crate) use super::{EguiController, StatusTone, MIN_SELECTION_WIDTH, BPM_MIN_SELECTION_DIVISOR};
pub(crate) use crate::egui_app::state::*;
pub(crate) use crate::sample_sources::*;
pub(crate) use crate::selection::SelectionRange;

use std::path::PathBuf;

pub(crate) mod audio_cache;
pub(crate) mod audio_loader;
pub(crate) mod audio_options;
pub(crate) mod audio_samples;
pub(crate) mod loop_crossfade;
pub(crate) mod recording;

mod browser_nav;
mod formatting;
mod player;
mod playhead_trail;
mod random_nav;
mod tagging;
mod transport;

#[cfg(test)]
mod audio_options_tests;

use formatting::{format_selection_duration, format_timestamp_hms_ms};
use tracing::warn;

#[cfg(test)]
const SHOULD_PLAY_RANDOM_SAMPLE: bool = false;
#[cfg(not(test))]
const SHOULD_PLAY_RANDOM_SAMPLE: bool = true;
const PLAYHEAD_COMPLETION_EPSILON: f32 = 0.001;

fn selection_meets_bpm_min(controller: &EguiController, range: SelectionRange) -> bool {
    if !controller.ui.waveform.bpm_snap_enabled {
        return true;
    }
    let Some(min_seconds) = bpm_min_selection_seconds(controller) else {
        return true;
    };
    let Some(duration) = controller.loaded_audio_duration_seconds() else {
        return true;
    };
    if !min_seconds.is_finite() || min_seconds <= 0.0 {
        return true;
    }
    if !duration.is_finite() || duration <= 0.0 {
        return true;
    }
    let selection_seconds = range.width() * duration;
    let epsilon = min_seconds * 1.0e-3;
    selection_seconds + epsilon >= min_seconds
}

fn selection_min_width(controller: &EguiController) -> f32 {
    if !controller.ui.waveform.bpm_snap_enabled {
        return 0.0;
    }
    let duration = match controller.loaded_audio_duration_seconds() {
        Some(duration) if duration.is_finite() && duration > 0.0 => duration,
        _ => return 0.0,
    };
    let min_seconds = bpm_min_selection_seconds(controller).unwrap_or(0.0);
    if min_seconds <= 0.0 {
        return 0.0;
    }
    min_seconds / duration
}

pub(crate) fn bpm_min_selection_seconds(controller: &EguiController) -> Option<f32> {
    if !controller.ui.waveform.bpm_snap_enabled {
        return None;
    }
    let bpm = controller.ui.waveform.bpm_value?;
    if !bpm.is_finite() || bpm <= 0.0 {
        return None;
    }
    let beat = 60.0 / bpm;
    let min_seconds = beat / BPM_MIN_SELECTION_DIVISOR;
    if min_seconds.is_finite() && min_seconds > 0.0 {
        Some(min_seconds)
    } else {
        None
    }
}

pub(crate) fn selection_meets_bpm_min_for_playback(
    controller: &EguiController,
    range: SelectionRange,
) -> bool {
    selection_meets_bpm_min(controller, range)
}

fn now_epoch_seconds() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs() as i64
}

impl EguiController {
    pub fn start_selection_drag(&mut self, position: f32) {
        transport::start_selection_drag(self, position);
    }

    pub fn start_selection_edge_drag(
        &mut self,
        edge: crate::selection::SelectionEdge,
        bpm_scale: bool,
    ) -> bool {
        transport::start_selection_edge_drag(self, edge, bpm_scale)
    }

    pub fn update_selection_drag(&mut self, position: f32, snap_override: bool) {
        transport::update_selection_drag(self, position, snap_override);
    }

    pub fn finish_selection_drag(&mut self) {
        transport::finish_selection_drag(self);
    }

    pub fn set_selection_range(&mut self, range: SelectionRange) {
        transport::set_selection_range(self, range);
    }

    pub fn is_selection_dragging(&self) -> bool {
        transport::is_selection_dragging(self)
    }

    pub fn clear_selection(&mut self) {
        transport::clear_selection(self);
    }

    pub fn toggle_loop(&mut self) {
        transport::toggle_loop(self);
    }

    pub fn seek_to(&mut self, position: f32) {
        transport::seek_to(self, position);
    }

    pub fn replay_from_last_start(&mut self) -> bool {
        transport::replay_from_last_start(self)
    }

    pub fn play_from_cursor(&mut self) -> bool {
        transport::play_from_cursor(self)
    }

    pub fn record_play_start(&mut self, position: f32) {
        transport::record_play_start(self, position);
    }

    pub fn set_volume(&mut self, volume: f32) {
        transport::set_volume(self, volume);
    }

    pub fn toggle_play_pause(&mut self) {
        transport::toggle_play_pause(self);
    }

    pub fn stop_playback_if_active(&mut self) -> bool {
        transport::stop_playback_if_active(self)
    }

    pub fn handle_escape(&mut self) {
        transport::handle_escape(self);
    }

    pub fn play_audio(&mut self, looped: bool, start_override: Option<f32>) -> Result<(), String> {
        player::play_audio(self, looped, start_override)
    }

    /// Record playback for the currently loaded audio, updating caches and UI.
    pub(crate) fn record_loaded_audio_playback(&mut self) {
        let Some(audio) = self.sample_view.wav.loaded_audio.as_ref() else {
            return;
        };
        let source_id = audio.source_id.clone();
        let root = audio.root.clone();
        let relative_path = audio.relative_path.clone();
        let played_at = now_epoch_seconds();
        let source = SampleSource {
            id: source_id.clone(),
            root,
        };
        match self.database_for(&source) {
            Ok(db) => {
                if let Err(err) = db.set_last_played_at(&relative_path, played_at) {
                    warn!(
                        "Failed to update playback age for {}: {}",
                        relative_path.display(),
                        err
                    );
                }
            }
            Err(err) => {
                warn!(
                    "Database unavailable for playback age update {}: {}",
                    relative_path.display(),
                    err
                );
            }
        }
        let mut updated_browser = false;
        if self.selection_state.ctx.selected_source.as_ref() == Some(&source_id)
            && let Some(index) = self.wav_index_for_path(&relative_path)
        {
            let _ = self.ensure_wav_page_loaded(index);
            if let Some(entry) = self.wav_entries.entry(index).cloned() {
                let mut updated_entry = entry.clone();
                updated_entry.last_played_at = Some(played_at);
                updated_browser = self
                    .wav_entries
                    .update_entry(&relative_path, updated_entry);
            }
        }
        if let Some(cache) = self.cache.wav.entries.get_mut(&source_id) {
            if let Some(index) = cache.lookup.get(&relative_path).copied()
                && let Some(entry) = cache.entry(index).cloned()
            {
                let mut updated_entry = entry.clone();
                updated_entry.last_played_at = Some(played_at);
                cache.update_entry(&relative_path, updated_entry);
            }
        }
        if updated_browser {
            self.rebuild_browser_lists();
        }
        for sample in &mut self.ui.collections.samples {
            if sample.source_id == source_id && sample.path == relative_path {
                sample.last_played_at = Some(played_at);
            }
        }
    }

    pub fn is_playing(&self) -> bool {
        player::is_playing(self)
    }

    pub fn tick_playhead(&mut self) {
        player::tick_playhead(self);
    }

    #[allow(dead_code)]
    pub(crate) fn update_playhead_from_progress(
        &mut self,
        progress: Option<f32>,
        is_looping: bool,
    ) {
        player::update_playhead_from_progress(self, progress, is_looping, false);
    }

    pub(crate) fn hide_waveform_playhead(&mut self) {
        player::hide_waveform_playhead(self);
    }

    #[cfg(test)]
    pub(crate) fn playhead_completed_span_for_tests(
        &self,
        progress: f32,
        is_looping: bool,
    ) -> bool {
        player::playhead_completed_span_for_tests(self, progress, is_looping)
    }

    #[cfg(test)]
    pub(crate) fn hide_waveform_playhead_for_tests(&mut self) {
        player::hide_waveform_playhead_for_tests(self);
    }

    pub(crate) fn apply_selection(
        &mut self,
        range: Option<SelectionRange>,
    ) {
        player::apply_selection(self, range);
    }

    pub fn update_waveform_hover_time(&mut self, position: Option<f32>) {
        player::update_waveform_hover_time(self, position);
    }

    #[allow(dead_code)]
    pub(crate) fn selection_duration_label(&self, range: SelectionRange) -> Option<String> {
        player::selection_duration_label(self, range)
    }

    pub(crate) fn apply_volume(&mut self, volume: f32) {
        player::apply_volume(self, volume);
    }

    pub(crate) fn ensure_player(
        &mut self,
    ) -> Result<Option<Rc<RefCell<AudioPlayer>>>, String> {
        player::ensure_player(self)
    }

    pub(crate) fn defer_loop_disable_after_cycle(&mut self) -> Result<(), String> {
        player::defer_loop_disable_after_cycle(self)
    }

    /// Tag the focused/selected wavs and keep the current focus.
    pub fn tag_selected(&mut self, target: crate::sample_sources::Rating) {
        tagging::tag_selected(self, target);
    }

    /// Move selection within the current sample browser list by an offset and play.
    pub fn nudge_selection(&mut self, offset: isize) {
        browser_nav::nudge_selection(self, offset);
    }

    /// Extend selection with shift navigation while keeping the current focus for playback.
    pub fn grow_selection(&mut self, offset: isize) {
        browser_nav::grow_selection(self, offset);
    }

    /// Jump to a random visible sample in the browser and start playback.
    pub fn play_random_visible_sample(&mut self) {
        random_nav::play_random_visible_sample(self);
    }

    #[cfg(test)]
    pub(crate) fn play_random_visible_sample_with_seed(&mut self, seed: u64) {
        random_nav::play_random_visible_sample_with_seed(self, seed);
    }

    /// Focus a random visible sample without starting playback (used for navigation flows).
    pub fn focus_random_visible_sample(&mut self) {
        random_nav::focus_random_visible_sample(self);
    }

    /// Play the previous entry from the random history stack.
    pub fn play_previous_random_sample(&mut self) {
        random_nav::play_previous_random_sample(self);
    }

    /// Toggle sticky random navigation for Up/Down in the browser.
    pub fn toggle_random_navigation_mode(&mut self) {
        random_nav::toggle_random_navigation_mode(self);
    }

    /// Return whether sticky random navigation mode is enabled.
    pub fn random_navigation_mode_enabled(&self) -> bool {
        random_nav::random_navigation_mode_enabled(self)
    }

    /// Cycle the triage flag filter (-1 left, +1 right) to mirror old column navigation.
    pub fn move_selection_column(&mut self, delta: isize) {
        tagging::move_selection_column(self, delta);
    }

    /// Tag leftwards: Keep -> Neutral, otherwise -> Trash.
    pub fn tag_selected_left(&mut self) {
        tagging::tag_selected_left(self);
    }

    /// Adjust rating for selected items by a delta (-3 to 3 relative change).
    pub fn adjust_selected_rating(&mut self, delta: i8) {
        tagging::adjust_selected_rating(self, delta);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::egui_app::controller::test_support;
    use std::path::PathBuf;

    #[test]
    fn selection_duration_label_uses_loaded_audio() {
        let (mut controller, source) = test_support::dummy_controller();
        controller.sample_view.wav.loaded_audio = Some(LoadedAudio {
            source_id: source.id.clone(),
            root: source.root.clone(),
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
    fn playhead_progress_updates_position_without_play_state() {
        let (mut controller, _source) = test_support::dummy_controller();

        controller.update_playhead_from_progress(Some(0.42), false);

        assert!(controller.ui.waveform.playhead.visible);
        assert!((controller.ui.waveform.playhead.position - 0.42).abs() < 0.0001);
    }

    #[test]
    fn playhead_progress_completion_hides_playhead() {
        let (mut controller, _source) = test_support::dummy_controller();
        controller.ui.waveform.playhead.active_span_end = Some(1.0);

        controller.update_playhead_from_progress(Some(0.9995), false);

        assert!(!controller.ui.waveform.playhead.visible);
        assert!(controller.ui.waveform.playhead.active_span_end.is_none());
    }
}
