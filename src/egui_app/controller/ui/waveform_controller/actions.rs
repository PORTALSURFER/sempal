use super::*;

pub(crate) trait WaveformActions {
    fn focus_waveform(&mut self);
    fn zoom_waveform_steps_with_factor(
        &mut self,
        zoom_in: bool,
        steps: u32,
        focus: Option<f64>,
        factor_override: Option<f32>,
        playhead_focus_when_playing: bool,
        keep_playhead_visible: bool,
    );
    fn nudge_selection_range(&mut self, steps: isize, fine: bool);
    fn slide_selection_range(&mut self, steps: isize);
    fn scroll_waveform_view(&mut self, center: f64);
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

    fn zoom_waveform_steps_with_factor(
        &mut self,
        zoom_in: bool,
        steps: u32,
        focus: Option<f64>,
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
        self.ensure_selection_visible_in_view(range);
        self.refresh_loop_after_selection_change(range);
        self.push_selection_undo("Selection", before, Some(range));
    }

    fn slide_selection_range(&mut self, steps: isize) {
        if !self.waveform_ready() {
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
        let width = selection.width();
        let mut delta = width * steps as f32;
        if let Some(step) = self
            .bpm_snap_step()
            .filter(|step| step.is_finite() && *step > 0.0)
        {
            let snapped = (delta / step).round() * step;
            if snapped != 0.0 {
                delta = snapped;
            } else if steps != 0 {
                delta = step * steps.signum() as f32;
            }
        }
        let range = selection.shift(delta);
        self.selection_state.range.set_range(Some(range));
        self.apply_selection(Some(range));
        self.ensure_selection_visible_in_view(range);
        self.refresh_loop_after_selection_change(range);
        self.push_selection_undo("Selection", before, Some(range));
    }

    fn scroll_waveform_view(&mut self, center: f64) {
        let view = self.ui.waveform.view;  // Use actual view, not display_view
        let width = view.width();
        if width >= 1.0 {
            self.ui.waveform.view = WaveformView {
                start: 0.0,
                end: 1.0,
            };
            self.refresh_waveform_image();
            return;
        }
        let half = width * 0.5;
        let start = (center - half).clamp(0.0, 1.0 - width);
        self.ui.waveform.view = WaveformView {
            start,
            end: (start + width).min(1.0),
        }.clamp();
        self.refresh_waveform_image();
    }
}
