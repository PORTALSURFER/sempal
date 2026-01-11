use super::*;

pub(crate) struct WaveformController<'a> {
    controller: &'a mut EguiController,
}

impl<'a> WaveformController<'a> {
    pub(crate) fn new(controller: &'a mut EguiController) -> Self {
        Self { controller }
    }
}

impl std::ops::Deref for WaveformController<'_> {
    type Target = EguiController;

    fn deref(&self) -> &Self::Target {
        self.controller
    }
}

impl std::ops::DerefMut for WaveformController<'_> {
    fn deref_mut(&mut self) -> &mut Self::Target {
        self.controller
    }
}

pub(crate) const PLAYHEAD_STEP_PX: f32 = 32.0;
pub(crate) const PLAYHEAD_STEP_PX_FINE: f32 = 1.0;
pub(crate) const VIEW_EPSILON: f64 = 1e-9;
pub(crate) const MIN_VIEW_WIDTH_BASE: f64 = 1e-9;
pub(crate) const CURSOR_IDLE_FADE: Duration = Duration::from_millis(500);

#[derive(Clone, Copy, Debug)]
pub(crate) enum CursorUpdateSource {
    Hover,
    Navigation,
}

impl WaveformController<'_> {
    pub(crate) fn display_view(&self) -> WaveformView {
        self.ui
            .waveform
            .image
            .as_ref()
            .map(|image| WaveformView {
                start: image.view_start,
                end: image.view_end,
            })
            .unwrap_or(self.ui.waveform.view)
    }

    pub(crate) fn waveform_ready(&self) -> bool {
        self.sample_view.waveform.decoded.is_some()
    }

    #[cfg(test)]
    pub(crate) fn zoom_waveform_steps(&mut self, zoom_in: bool, steps: u32, focus: Option<f64>) {
        self.zoom_waveform_steps_with_factor(zoom_in, steps, focus, None, true, true);
    }

    pub(crate) fn waveform_step_size(&self, fine: bool) -> f32 {
        let width_px = self.sample_view.waveform.size[0].max(1) as f32;
        let px = if fine {
            PLAYHEAD_STEP_PX_FINE
        } else {
            PLAYHEAD_STEP_PX
        };
        let px_fraction = (px / width_px).min(1.0);
        let px_fraction = (px / width_px).min(1.0);
        (self.ui.waveform.view.width() as f32) * px_fraction
    }

    pub(crate) fn bpm_snap_step(&self) -> Option<f32> {
        if !self.ui.waveform.bpm_snap_enabled {
            return None;
        }
        let bpm = self.ui.waveform.bpm_value?;
        if !bpm.is_finite() || bpm <= 0.0 {
            return None;
        }
        let duration = self.loaded_audio_duration_seconds()?;
        if !duration.is_finite() || duration <= 0.0 {
            return None;
        }
        let step = 60.0 / bpm / duration;
        if step.is_finite() && step > 0.0 {
            Some(step)
        } else {
            None
        }
    }

    pub(crate) fn selection_min_width(&self) -> f64 {
        if !self.ui.waveform.bpm_snap_enabled {
            return 0.0;
        }
        self.bpm_snap_step()
            .map(|step| (step / BPM_MIN_SELECTION_DIVISOR) as f64)
            .unwrap_or(0.0)
    }

    pub(crate) fn refresh_loop_after_selection_change(&mut self, selection: SelectionRange) {
        if !self.ui.waveform.loop_enabled || !self.is_playing() {
            return;
        }
        if (selection.width() as f64) < self.selection_min_width() {
            return;
        }
        let playhead = self.ui.waveform.playhead.position;
        let start_override = if playhead >= selection.start() && playhead <= selection.end() {
            Some(playhead)
        } else {
            Some(selection.start())
        };
        if let Err(err) = self.play_audio(true, start_override) {
            self.set_status(err, StatusTone::Error);
        }
    }

    pub(crate) fn ensure_selection_visible_in_view(&mut self, selection: SelectionRange) {
        if !self.waveform_ready() {
            return;
        }
        let mut view = self.ui.waveform.view;
        let width = view.width().max(self.min_view_width());
        if width >= 1.0 {
            return;
        }
        let sel_width = selection.width() as f64;
        let sel_start = selection.start() as f64;
        let sel_end = selection.end() as f64;
        if sel_width >= width {
            let center = (sel_start + sel_end) * 0.5;
            let start = (center - width * 0.5).clamp(0.0, 1.0 - width);
            view.start = start;
            view.end = start + width;
        } else if sel_start < view.start {
            view.start = sel_start;
            view.end = (view.start + width).min(1.0);
        } else if sel_end > view.end {
            view.end = sel_end;
            view.start = (view.end - width).max(0.0);
        }
        let clamped = view.clamp();
        if views_differ(self.ui.waveform.view, clamped) {
            self.ui.waveform.view = clamped;
            self.refresh_waveform_image();
        }
    }

    pub(crate) fn set_waveform_cursor(&mut self, position: f32) {
        self.set_waveform_cursor_with_source(position, CursorUpdateSource::Navigation);
    }

    pub(crate) fn set_waveform_cursor_from_hover(&mut self, position: f32) {
        self.set_waveform_cursor_with_source(position, CursorUpdateSource::Hover);
    }

    pub(crate) fn set_waveform_cursor_with_source(
        &mut self,
        position: f32,
        source: CursorUpdateSource,
    ) {
        if !self.waveform_ready() {
            return;
        }
        let clamped = position.clamp(0.0, 1.0);
        self.ui.waveform.cursor = Some(clamped);
        let now = Instant::now();
        match source {
            CursorUpdateSource::Hover => self.ui.waveform.cursor_last_hover_at = Some(now),
            CursorUpdateSource::Navigation => {
                self.ui.waveform.cursor_last_navigation_at = Some(now)
            }
        }
        self.ensure_cursor_visible_in_view(clamped);
    }

    pub(crate) fn waveform_navigation_anchor(&self) -> f32 {
        if let Some(cursor) = self.ui.waveform.cursor {
            return cursor;
        }
        if let Some(marker) = self.ui.waveform.last_start_marker {
            return marker;
        }
        if self.ui.waveform.playhead.visible {
            return self.ui.waveform.playhead.position;
        }
        if let Some(selection) = self.selection_state.range.range() {
            return (selection.start() + selection.end()) * 0.5;
        }
        let view = self.ui.waveform.view;
        ((view.start + view.end) * 0.5) as f32
    }


    pub(crate) fn ensure_playhead_visible_in_view(&mut self) {
        let mut view = self.ui.waveform.view;
        let width = view.width();
        let pos = self.ui.waveform.playhead.position as f64;
        if pos < view.start {
            view.start = pos;
            view.end = (view.start + width).min(1.0);
        } else if pos > view.end {
            view.end = pos;
            view.start = (view.end - width).max(0.0);
        }
        self.ui.waveform.view = view.clamp();
    }

    fn ensure_cursor_visible_in_view(&mut self, position: f32) {
        let mut view = self.ui.waveform.view;
        let width = view.width();
        let position = position as f64;
        if position < view.start {
            view.start = position;
            view.end = (view.start + width).min(1.0);
        } else if position > view.end {
            view.end = position;
            view.start = (view.end - width).max(0.0);
        }
        let clamped = view.clamp();
        if views_differ(self.ui.waveform.view, clamped) {
            self.ui.waveform.view = clamped;
            self.refresh_waveform_image();
        }
    }

    pub(crate) fn waveform_cursor_alpha(&mut self, hovering: bool) -> f32 {
        if hovering {
            self.ui.waveform.cursor_last_hover_at = Some(Instant::now());
            return 1.0;
        }
        if !self.waveform_ready() {
            return 0.0;
        }
        if self.ui.focus.context == FocusContext::Waveform {
            return 1.0;
        }
        let Some(last_activity) = self.cursor_last_activity() else {
            return 1.0;
        };
        let idle = Instant::now().saturating_duration_since(last_activity);
        if idle >= CURSOR_IDLE_FADE {
            self.ui.waveform.cursor = Some(0.0);
            return 0.0;
        }
        let fraction = idle.as_secs_f32() / CURSOR_IDLE_FADE.as_secs_f32();
        (1.0 - fraction).clamp(0.0, 1.0)
    }

    fn cursor_last_activity(&self) -> Option<Instant> {
        match (
            self.ui.waveform.cursor_last_hover_at,
            self.ui.waveform.cursor_last_navigation_at,
        ) {
            (Some(hover), Some(nav)) => Some(hover.max(nav)),
            (Some(hover), None) => Some(hover),
            (None, Some(nav)) => Some(nav),
            (None, None) => None,
        }
    }

    pub(crate) fn waveform_focus_point(&self) -> f64 {
        if let Some(cursor) = self.ui.waveform.cursor {
            cursor as f64
        } else if let Some(marker) = self.ui.waveform.last_start_marker {
            marker as f64
        } else if self.ui.waveform.playhead.visible {
            self.ui.waveform.playhead.position as f64
        } else if let Some(selection) = self.selection_state.range.range() {
            ((selection.start() + selection.end()) * 0.5) as f64
        } else {
            let view = self.ui.waveform.view;
            (view.start + view.end) * 0.5
        }
    }

    pub(crate) fn apply_zoom_step(
        &mut self,
        zoom_in: bool,
        focus: Option<f64>,
        factor_override: Option<f32>,
        playhead_focus_when_playing: bool,
        keep_playhead_visible: bool,
    ) -> bool {
        if !self.waveform_ready() {
            return false;
        }
        let focus_from_pointer = focus.is_some();
        let original = self.ui.waveform.view;  // Use actual view, not display_view
        let default_factor = self.ui.controls.keyboard_zoom_factor.max(0.01);
        let base = factor_override.unwrap_or(default_factor).max(0.01) as f64;
        let factor = if zoom_in { base } else { 1.0 / base };
        let focus = if playhead_focus_when_playing && self.is_playing() {
            self.ui.waveform.playhead.visible = true;
            self.ui.waveform.playhead.position as f64
        } else {
            focus.unwrap_or_else(|| self.waveform_focus_point())
        };
        let min_width = MIN_VIEW_WIDTH_BASE;

        // Allow zooming beyond the "ideal" 1 sample per pixel
        let width = (original.width() * factor).clamp(min_width, 1.0);
        if (width - original.width()).abs() <= VIEW_EPSILON {
            return false;
        }
        self.ui.waveform.suppress_hover_cursor = !focus_from_pointer;
        if focus.is_finite() && focus_from_pointer {
            self.set_waveform_cursor_with_source(focus as f32, CursorUpdateSource::Hover);
        }
        let mut view = original;
        if focus_from_pointer {
            let ratio = ((focus - original.start) / original.width()).clamp(0.0, 1.0);
            view.start = focus - width * ratio;
            view.end = view.start + width;
        } else {
            view.start = focus - width * 0.5;
            view.end = focus + width * 0.5;
        }
        self.ui.waveform.view = view.clamp();
        if keep_playhead_visible && self.ui.waveform.cursor.is_none() {
            self.ensure_playhead_visible_in_view();
        }
        views_differ(original, self.ui.waveform.view)
    }

    pub(crate) fn zoom_to_selection(&mut self) {
        if !self.waveform_ready() {
            return;
        }
        let Some(selection) = self
            .selection_state
            .range
            .range()
            .or(self.ui.waveform.selection)
        else {
            // Fallback to regular zoom in if no selection
            self.zoom_waveform_steps_with_factor(true, 1, None, None, true, true);
            return;
        };
        if (selection.width() as f64) <= VIEW_EPSILON {
            self.zoom_waveform_steps_with_factor(
                true,
                1,
                Some(selection.start() as f64),
                None,
                true,
                true,
            );
            return;
        }

        // Calculate target view
        let width = (selection.width() as f64).max(self.min_view_width());
        // Center the view on selection center
        let center = ((selection.start() + selection.end()) * 0.5) as f64;
        let start = (center - width * 0.5).clamp(0.0, 1.0 - width);
        let end = (start + width).min(1.0);

        let view = WaveformView { start, end };
        self.ui.waveform.view = view.clamp();
        self.refresh_waveform_image();
    }

    pub(crate) fn zoom_out_full(&mut self) {
        if !self.waveform_ready() {
            return;
        }
        self.ui.waveform.view = WaveformView {
            start: 0.0,
            end: 1.0,
        };
        self.refresh_waveform_image();
    }
}


pub(crate) fn views_differ(a: WaveformView, b: WaveformView) -> bool {
    (a.start - b.start).abs() > VIEW_EPSILON || (a.end - b.end).abs() > VIEW_EPSILON
}
