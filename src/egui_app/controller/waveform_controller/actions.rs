use super::*;

pub(crate) trait WaveformActions {
    fn focus_waveform(&mut self);
    fn move_playhead_steps(&mut self, steps: isize, fine: bool, resume_playback: bool);
    fn zoom_waveform(&mut self, zoom_in: bool);
    fn zoom_waveform_steps_with_factor(
        &mut self,
        zoom_in: bool,
        steps: u32,
        focus: Option<f32>,
        factor_override: Option<f32>,
        playhead_focus_when_playing: bool,
        keep_playhead_visible: bool,
    );
    fn create_selection_from_playhead(&mut self, to_left: bool, resume_playback: bool, fine: bool);
    fn nudge_selection_edge(&mut self, edge: SelectionEdge, outward: bool, fine: bool);
    fn nudge_selection_range(&mut self, steps: isize, fine: bool);
    fn scroll_waveform_view(&mut self, center: f32);
}

impl WaveformActions for WaveformController<'_> {
    fn focus_waveform(&mut self) {
        if self.waveform_ready() {
            self.focus_waveform_context();
            self.ensure_playhead_visible_in_view();
        } else if self.sample_view.wav.selected_wav.is_some() || self.ui.waveform.loading.is_some()
        {
            self.focus_waveform_context();
        } else {
            self.set_status("Load a sample to focus the waveform", StatusTone::Info);
        }
    }

    fn move_playhead_steps(&mut self, steps: isize, fine: bool, _resume_playback: bool) {
        if !self.waveform_ready() {
            return;
        }
        let step = self.waveform_step_size(fine);
        if step <= 0.0 {
            return;
        }
        let delta = step * steps as f32;
        let start = self.waveform_navigation_anchor();
        let next = (start + delta).clamp(0.0, 1.0);
        self.set_waveform_cursor(next);
    }

    fn zoom_waveform(&mut self, zoom_in: bool) {
        self.zoom_waveform_steps_with_factor(zoom_in, 1, None, None, true, true);
    }

    fn zoom_waveform_steps_with_factor(
        &mut self,
        zoom_in: bool,
        steps: u32,
        focus: Option<f32>,
        factor_override: Option<f32>,
        playhead_focus_when_playing: bool,
        keep_playhead_visible: bool,
    ) {
        if !self.waveform_ready() {
            return;
        }
        let steps = steps.max(1);
        let mut changed = false;
        for _ in 0..steps {
            changed |= self.apply_zoom_step(
                zoom_in,
                focus,
                factor_override,
                playhead_focus_when_playing,
                keep_playhead_visible,
            );
        }
        if changed {
            self.refresh_waveform_image();
        }
    }

    fn create_selection_from_playhead(&mut self, to_left: bool, resume_playback: bool, fine: bool) {
        if !self.waveform_ready() {
            return;
        }
        let before = self
            .selection_state
            .range
            .range()
            .or(self.ui.waveform.selection);
        let step = self.waveform_step_size(fine).max(MIN_SELECTION_WIDTH);
        let anchor = self.waveform_focus_point();
        let range = if to_left {
            SelectionRange::new((anchor - step).clamp(0.0, 1.0), anchor)
        } else {
            SelectionRange::new(anchor, (anchor + step).clamp(0.0, 1.0))
        };
        self.selection_state.range.set_range(Some(range));
        self.apply_selection(Some(range));
        self.push_selection_undo("Selection", before, Some(range));
        self.set_playhead_after_selection(anchor, resume_playback);
    }

    fn nudge_selection_edge(&mut self, edge: SelectionEdge, outward: bool, fine: bool) {
        if !self.waveform_ready() {
            return;
        }
        let step = if fine {
            self.waveform_step_size(true)
        } else {
            self.bpm_snap_step()
                .unwrap_or_else(|| self.waveform_step_size(false))
        }
        .max(MIN_SELECTION_WIDTH);
        let Some(selection) = self
            .selection_state
            .range
            .range()
            .or(self.ui.waveform.selection)
        else {
            self.set_status("Create a selection first", StatusTone::Info);
            return;
        };
        let before = Some(selection);
        let mut start = selection.start();
        let mut end = selection.end();
        match (edge, outward) {
            (SelectionEdge::Start, true) => start -= step,
            (SelectionEdge::Start, false) => start += step,
            (SelectionEdge::End, true) => end += step,
            (SelectionEdge::End, false) => end -= step,
        }
        let (clamped_start, clamped_end) = helpers::clamp_selection_bounds(start, end);
        let range = SelectionRange::new(clamped_start, clamped_end);
        self.selection_state.range.set_range(Some(range));
        self.apply_selection(Some(range));
        self.refresh_loop_after_selection_change(range);
        self.push_selection_undo("Selection", before, Some(range));
    }

    fn nudge_selection_range(&mut self, steps: isize, fine: bool) {
        if !self.waveform_ready() {
            return;
        }
        let step = if fine {
            self.waveform_step_size(true)
        } else {
            self.bpm_snap_step()
                .unwrap_or_else(|| self.waveform_step_size(false))
        };
        if step <= 0.0 {
            return;
        }
        let Some(selection) = self
            .selection_state
            .range
            .range()
            .or(self.ui.waveform.selection)
        else {
            self.set_status("Create a selection first", StatusTone::Info);
            return;
        };
        let before = Some(selection);
        let delta = step * steps as f32;
        let range = selection.shift(delta);
        self.selection_state.range.set_range(Some(range));
        self.apply_selection(Some(range));
        self.refresh_loop_after_selection_change(range);
        self.push_selection_undo("Selection", before, Some(range));
    }

    fn scroll_waveform_view(&mut self, center: f32) {
        let mut view = self.ui.waveform.view;
        let min_width = self.min_view_width();
        let width = view.width().max(min_width);
        if width >= 1.0 {
            view.start = 0.0;
            view.end = 1.0;
            self.ui.waveform.view = view;
            self.refresh_waveform_image();
            return;
        }
        let half = width * 0.5;
        let start = (center - half).clamp(0.0, 1.0 - width);
        view.start = start;
        view.end = (start + width).min(1.0);
        self.ui.waveform.view = view.clamp();
        self.refresh_waveform_image();
    }
}
