use super::*;
use std::path::PathBuf;

mod random_nav;
mod player;
mod browser_nav;
mod tagging;
mod transport;
mod formatting;

use formatting::{format_selection_duration, format_timestamp_hms_ms};

#[cfg(test)]
const SHOULD_PLAY_RANDOM_SAMPLE: bool = false;
#[cfg(not(test))]
const SHOULD_PLAY_RANDOM_SAMPLE: bool = true;
const PLAYHEAD_COMPLETION_EPSILON: f32 = 0.001;

impl EguiController {
    /// Tag the focused/selected wavs and keep the current focus.
    pub fn tag_selected(&mut self, target: SampleTag) {
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
    pub(super) fn play_random_visible_sample_with_seed(&mut self, seed: u64) {
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

}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::egui_app::controller::test_support;
    use std::path::PathBuf;

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
