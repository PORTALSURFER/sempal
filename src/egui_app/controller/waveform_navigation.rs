use super::*;
use crate::selection::SelectionEdge;

const PLAYHEAD_STEP_PX: f32 = 32.0;
const PLAYHEAD_STEP_PX_FINE: f32 = 8.0;
const MIN_VIEW_WIDTH: f32 = 0.01;
const ZOOM_IN_FACTOR: f32 = 0.8;
const ZOOM_OUT_FACTOR: f32 = 1.0 / ZOOM_IN_FACTOR;

impl EguiController {
    /// Focus the waveform viewer when a sample is loaded.
    pub(crate) fn focus_waveform(&mut self) {
        if self.waveform_ready() {
            self.focus_waveform_context();
            self.ensure_playhead_visible_in_view();
        } else {
            self.set_status("Load a sample to focus the waveform", StatusTone::Info);
        }
    }

    /// Move the playhead left/right by a fixed visual step.
    pub(crate) fn move_playhead_steps(&mut self, steps: isize, fine: bool) {
        if !self.waveform_ready() {
            return;
        }
        let step = self.waveform_step_size(fine);
        if step <= 0.0 {
            return;
        }
        let delta = step * steps as f32;
        let next = (self.ui.waveform.playhead.position + delta).clamp(0.0, 1.0);
        self.set_playhead_and_seek(next);
    }

    /// Zoom the waveform while keeping the playhead centered.
    pub(crate) fn zoom_waveform(&mut self, zoom_in: bool) {
        if !self.waveform_ready() {
            return;
        }
        let factor = if zoom_in { ZOOM_IN_FACTOR } else { ZOOM_OUT_FACTOR };
        let focus = self.waveform_focus_point();
        let mut view = self.ui.waveform.view;
        let width = (view.width() * factor).clamp(MIN_VIEW_WIDTH, 1.0);
        view.start = focus - width * 0.5;
        view.end = focus + width * 0.5;
        self.ui.waveform.view = view.clamp();
        self.ensure_playhead_visible_in_view();
    }

    /// Create or replace a selection anchored to the playhead.
    pub(crate) fn create_selection_from_playhead(&mut self, to_left: bool) {
        if !self.waveform_ready() {
            return;
        }
        let step = self.waveform_step_size(false).max(MIN_SELECTION_WIDTH);
        let anchor = self.waveform_focus_point();
        let range = if to_left {
            SelectionRange::new((anchor - step).clamp(0.0, 1.0), anchor)
        } else {
            SelectionRange::new(anchor, (anchor + step).clamp(0.0, 1.0))
        };
        self.selection.set_range(Some(range));
        self.apply_selection(Some(range));
        self.set_playhead_and_seek(anchor);
    }

    /// Grow an existing selection from the focused edge or create a new one.
    pub(crate) fn grow_selection_from_playhead(&mut self, to_left: bool) {
        if !self.waveform_ready() {
            return;
        }
        let step = self.waveform_step_size(false).max(MIN_SELECTION_WIDTH);
        if let Some(selection) = self.selection.range().or(self.ui.waveform.selection) {
            let mut start = selection.start();
            let mut end = selection.end();
            if to_left {
                start -= step;
            } else {
                end += step;
            }
            let (clamped_start, clamped_end) = clamp_selection_bounds(start, end);
            let range = SelectionRange::new(clamped_start, clamped_end);
            self.selection.set_range(Some(range));
            self.apply_selection(Some(range));
            let playhead = if to_left { range.start() } else { range.end() };
            self.set_playhead_and_seek(playhead);
        } else {
            self.create_selection_from_playhead(to_left);
        }
    }

    /// Nudge a selection edge in or out by a fixed visual step.
    pub(crate) fn nudge_selection_edge(&mut self, edge: SelectionEdge, outward: bool) {
        if !self.waveform_ready() {
            return;
        }
        let step = self.waveform_step_size(false).max(MIN_SELECTION_WIDTH);
        let Some(selection) = self
            .selection
            .range()
            .or(self.ui.waveform.selection)
        else {
            self.set_status("Create a selection first", StatusTone::Info);
            return;
        };
        let mut start = selection.start();
        let mut end = selection.end();
        match (edge, outward) {
            (SelectionEdge::Start, true) => start -= step,
            (SelectionEdge::Start, false) => start += step,
            (SelectionEdge::End, true) => end += step,
            (SelectionEdge::End, false) => end -= step,
        }
        let (clamped_start, clamped_end) = clamp_selection_bounds(start, end);
        let range = SelectionRange::new(clamped_start, clamped_end);
        self.selection.set_range(Some(range));
        self.apply_selection(Some(range));
    }

    fn waveform_ready(&self) -> bool {
        self.decoded_waveform.is_some()
    }

    fn waveform_step_size(&self, fine: bool) -> f32 {
        let width_px = self.waveform_size[0].max(1) as f32;
        let px = if fine { PLAYHEAD_STEP_PX_FINE } else { PLAYHEAD_STEP_PX };
        let px_fraction = (px / width_px).min(1.0);
        self.ui.waveform.view.width() * px_fraction
    }

    fn set_playhead_and_seek(&mut self, position: f32) {
        if !self.waveform_ready() {
            return;
        }
        self.ui.waveform.playhead.position = position.clamp(0.0, 1.0);
        self.ui.waveform.playhead.visible = true;
        self.ensure_playhead_visible_in_view();
        let looped = self.ui.waveform.loop_enabled;
        let _ = self.play_audio(looped, Some(self.ui.waveform.playhead.position));
    }

    fn ensure_playhead_visible_in_view(&mut self) {
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

    fn waveform_focus_point(&self) -> f32 {
        if self.ui.waveform.playhead.visible {
            self.ui.waveform.playhead.position
        } else if let Some(selection) = self.selection.range() {
            (selection.start() + selection.end()) * 0.5
        } else {
            let view = self.ui.waveform.view;
            (view.start + view.end) * 0.5
        }
    }
}

fn clamp_selection_bounds(start: f32, end: f32) -> (f32, f32) {
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
