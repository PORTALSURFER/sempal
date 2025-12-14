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

pub(super) const PLAYHEAD_STEP_PX: f32 = 32.0;
pub(super) const PLAYHEAD_STEP_PX_FINE: f32 = 1.0;
pub(super) const VIEW_EPSILON: f32 = 1e-5;
pub(super) const CURSOR_IDLE_FADE: Duration = Duration::from_millis(500);

#[derive(Clone, Copy, Debug)]
pub(super) enum CursorUpdateSource {
    Hover,
    Navigation,
}

impl WaveformController<'_> {
    pub(crate) fn waveform_ready(&self) -> bool {
        self.waveform.decoded.is_some()
    }

    #[cfg(test)]
    pub(crate) fn zoom_waveform_steps(&mut self, zoom_in: bool, steps: u32, focus: Option<f32>) {
        self.zoom_waveform_steps_with_factor(zoom_in, steps, focus, None, true, true);
    }

    pub(super) fn waveform_step_size(&self, fine: bool) -> f32 {
        let width_px = self.waveform.size[0].max(1) as f32;
        let px = if fine {
            PLAYHEAD_STEP_PX_FINE
        } else {
            PLAYHEAD_STEP_PX
        };
        let px_fraction = (px / width_px).min(1.0);
        self.ui.waveform.view.width() * px_fraction
    }

    pub(crate) fn set_waveform_cursor(&mut self, position: f32) {
        self.set_waveform_cursor_with_source(position, CursorUpdateSource::Navigation);
    }

    pub(crate) fn set_waveform_cursor_from_hover(&mut self, position: f32) {
        self.set_waveform_cursor_with_source(position, CursorUpdateSource::Hover);
    }

    pub(super) fn set_waveform_cursor_with_source(
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

    pub(super) fn waveform_navigation_anchor(&self) -> f32 {
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
        (view.start + view.end) * 0.5
    }

    pub(super) fn set_playhead_after_selection(&mut self, position: f32, resume_playback: bool) {
        if resume_playback && self.is_playing() {
            self.set_playhead_and_seek(position);
        } else {
            self.set_playhead_no_seek(position);
        }
    }

    fn set_playhead_and_seek(&mut self, position: f32) {
        if !self.waveform_ready() {
            return;
        }
        self.set_waveform_cursor_with_source(position, CursorUpdateSource::Navigation);
        self.ui.waveform.playhead.position = position.clamp(0.0, 1.0);
        self.ui.waveform.playhead.visible = true;
        self.ensure_playhead_visible_in_view();
        let looped = self.ui.waveform.loop_enabled;
        let pos = self.ui.waveform.playhead.position;
        let _ = self.play_audio(looped, Some(pos));
    }

    fn set_playhead_no_seek(&mut self, position: f32) {
        if !self.waveform_ready() {
            return;
        }
        self.set_waveform_cursor_with_source(position, CursorUpdateSource::Navigation);
        self.ui.waveform.playhead.position = position.clamp(0.0, 1.0);
        self.ui.waveform.playhead.visible = true;
        self.ensure_playhead_visible_in_view();
    }

    pub(super) fn ensure_playhead_visible_in_view(&mut self) {
        let mut view = self.ui.waveform.view;
        let width = view.width();
        let pos = self.ui.waveform.playhead.position;
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

    pub(super) fn waveform_focus_point(&self) -> f32 {
        if let Some(cursor) = self.ui.waveform.cursor {
            cursor
        } else if let Some(marker) = self.ui.waveform.last_start_marker {
            marker
        } else if self.ui.waveform.playhead.visible {
            self.ui.waveform.playhead.position
        } else if let Some(selection) = self.selection_state.range.range() {
            (selection.start() + selection.end()) * 0.5
        } else {
            let view = self.ui.waveform.view;
            (view.start + view.end) * 0.5
        }
    }

    pub(super) fn apply_zoom_step(
        &mut self,
        zoom_in: bool,
        focus: Option<f32>,
        factor_override: Option<f32>,
        playhead_focus_when_playing: bool,
        keep_playhead_visible: bool,
    ) -> bool {
        if !self.waveform_ready() {
            return false;
        }
        let default_factor = self.ui.controls.keyboard_zoom_factor.max(0.01);
        let base = factor_override.unwrap_or(default_factor).max(0.01);
        let factor = if zoom_in { base } else { 1.0 / base };
        let focus = if playhead_focus_when_playing && self.is_playing() {
            self.ui.waveform.playhead.visible = true;
            self.ui.waveform.playhead.position
        } else {
            focus.unwrap_or_else(|| self.waveform_focus_point())
        };
        let min_width = self.min_view_width();
        let original = self.ui.waveform.view;
        let width = (original.width() * factor).clamp(min_width, 1.0);
        let mut view = self.ui.waveform.view;
        view.start = focus - width * 0.5;
        view.end = focus + width * 0.5;
        self.ui.waveform.view = view.clamp();
        if keep_playhead_visible {
            self.ensure_playhead_visible_in_view();
        }
        views_differ(original, self.ui.waveform.view)
    }
}

pub(super) fn clamp_selection_bounds(start: f32, end: f32) -> (f32, f32) {
    let mut a = start.clamp(0.0, 1.0);
    let mut b = end.clamp(0.0, 1.0);
    if a > b {
        std::mem::swap(&mut a, &mut b);
    }
    if (b - a) < MIN_SELECTION_WIDTH {
        b = (a + MIN_SELECTION_WIDTH).min(1.0);
        a = (b - MIN_SELECTION_WIDTH).max(0.0);
    }
    (a, b)
}

pub(super) fn views_differ(a: WaveformView, b: WaveformView) -> bool {
    (a.start - b.start).abs() > VIEW_EPSILON || (a.end - b.end).abs() > VIEW_EPSILON
}
