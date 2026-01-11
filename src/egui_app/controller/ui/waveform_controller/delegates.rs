use super::*;

impl EguiController {
    pub(crate) fn focus_waveform(&mut self) {
        self.waveform().focus_waveform();
    }

    pub(crate) fn move_playhead_steps(&mut self, steps: isize, fine: bool, resume_playback: bool) {
        self.waveform()
            .move_playhead_steps(steps, fine, resume_playback);
    }


    #[cfg(test)]
    pub(crate) fn zoom_waveform_steps(&mut self, zoom_in: bool, steps: u32, focus: Option<f64>) {
        self.waveform().zoom_waveform_steps(zoom_in, steps, focus);
    }

    pub(crate) fn zoom_waveform_steps_with_factor(
        &mut self,
        zoom_in: bool,
        steps: u32,
        focus: Option<f64>,
        factor_override: Option<f32>,
        playhead_focus_when_playing: bool,
        keep_playhead_visible: bool,
    ) {
        self.waveform().zoom_waveform_steps_with_factor(
            zoom_in,
            steps,
            focus,
            factor_override,
            playhead_focus_when_playing,
            keep_playhead_visible,
        );
    }


    pub(crate) fn nudge_selection_range(&mut self, steps: isize, fine: bool) {
        self.waveform().nudge_selection_range(steps, fine);
    }

    pub(crate) fn slide_selection_range(&mut self, steps: isize) {
        self.waveform().slide_selection_range(steps);
    }

    pub(crate) fn waveform_ready(&self) -> bool {
        self.sample_view.waveform.decoded.is_some()
    }

    pub(crate) fn set_waveform_cursor(&mut self, position: f32) {
        self.waveform().set_waveform_cursor(position);
    }

    pub(crate) fn set_waveform_cursor_from_hover(&mut self, position: f32) {
        self.waveform().set_waveform_cursor_from_hover(position);
    }

    pub(crate) fn waveform_cursor_alpha(&mut self, hovering: bool) -> f32 {
        self.waveform().waveform_cursor_alpha(hovering)
    }

    pub(crate) fn scroll_waveform_view(&mut self, center: f64) {
        self.waveform().scroll_waveform_view(center);
    }
}
