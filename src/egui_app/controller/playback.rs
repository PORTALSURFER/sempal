use super::*;
use std::path::PathBuf;

mod random_nav;
mod player;
mod transport;

#[cfg(test)]
const SHOULD_PLAY_RANDOM_SAMPLE: bool = false;
#[cfg(not(test))]
const SHOULD_PLAY_RANDOM_SAMPLE: bool = true;
const PLAYHEAD_COMPLETION_EPSILON: f32 = 0.001;

impl EguiController {
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
        let mut applied: Vec<(SourceId, PathBuf, SampleTag)> = Vec::new();
        for row in rows {
            let before = match self.resolve_browser_sample(row) {
                Ok(ctx) => (ctx.source.id.clone(), ctx.entry.relative_path.clone(), ctx.entry.tag),
                Err(err) => {
                    last_error = Some(err);
                    continue;
                }
            };
            match self.tag_browser_sample(row, target) {
                Ok(()) => applied.push(before),
                Err(err) => last_error = Some(err),
            }
        }
        if !applied.is_empty() {
            let label = match target {
                SampleTag::Keep => "Tag keep",
                SampleTag::Trash => "Tag trash",
                SampleTag::Neutral => "Tag neutral",
            };
            let redo_updates: Vec<(SourceId, PathBuf, SampleTag)> = applied
                .iter()
                .map(|(source_id, path, _)| (source_id.clone(), path.clone(), target))
                .collect();
            self.push_undo_entry(super::undo::UndoEntry::<EguiController>::new(
                label,
                move |controller: &mut EguiController| {
                    for (source_id, path, tag) in applied.iter() {
                        let source = controller
                            .sources
                            .iter()
                            .find(|s| &s.id == source_id)
                            .cloned()
                            .ok_or_else(|| "Source not available".to_string())?;
                        controller.set_sample_tag_for_source(&source, path, *tag, false)?;
                    }
                    Ok(())
                },
                move |controller: &mut EguiController| {
                    for (source_id, path, tag) in redo_updates.iter() {
                        let source = controller
                            .sources
                            .iter()
                            .find(|s| &s.id == source_id)
                            .cloned()
                            .ok_or_else(|| "Source not available".to_string())?;
                        controller.set_sample_tag_for_source(&source, path, *tag, false)?;
                    }
                    Ok(())
                },
            ));
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
