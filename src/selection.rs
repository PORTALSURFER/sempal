//! Helpers for tracking waveform selection ranges and drag interactions.
//! This module keeps selection math pure and testable so the UI integration code can stay small.
/// Normalized selection bounds over a waveform (0.0 - 1.0).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct SelectionRange {
    start: f32,
    end: f32,
}

impl SelectionRange {
    /// Create a clamped range, ensuring `start` is not greater than `end`.
    pub fn new(start: f32, end: f32) -> Self {
        let a = clamp01(start);
        let b = clamp01(end);
        if a <= b {
            Self { start: a, end: b }
        } else {
            Self { start: b, end: a }
        }
    }

    /// Start position within the waveform.
    pub fn start(&self) -> f32 {
        self.start
    }

    /// End position within the waveform.
    pub fn end(&self) -> f32 {
        self.end
    }

    /// Width of the selection.
    pub fn width(&self) -> f32 {
        (self.end - self.start).abs()
    }

    /// True when the selection has zero width.
    #[allow(dead_code)]
    pub fn is_empty(&self) -> bool {
        self.width() == 0.0
    }

    /// Shift the selection by the given delta, clamping to the waveform bounds.
    pub fn shift(self, delta: f32) -> Self {
        if !delta.is_finite() {
            return self;
        }
        let width = self.width().clamp(0.0, 1.0);
        if width >= 1.0 {
            return SelectionRange::new(0.0, 1.0);
        }
        let mut start = self.start + delta;
        let mut end = self.end + delta;
        if start < 0.0 {
            end -= start;
            start = 0.0;
        }
        if end > 1.0 {
            let over = end - 1.0;
            start -= over;
            end = 1.0;
        }
        SelectionRange::new(start, end)
    }
}

/// The selection edge being dragged.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum SelectionEdge {
    /// Adjust the starting edge.
    Start,
    /// Adjust the ending edge.
    End,
}

#[derive(Clone, Copy, Debug, PartialEq)]
enum DragKind {
    Create { anchor: f32 },
    StartEdge,
    EndEdge,
}

impl From<SelectionEdge> for DragKind {
    fn from(edge: SelectionEdge) -> Self {
        match edge {
            SelectionEdge::Start => DragKind::StartEdge,
            SelectionEdge::End => DragKind::EndEdge,
        }
    }
}

/// Tracks active selection and drag gestures.
#[derive(Default, Debug)]
pub struct SelectionState {
    range: Option<SelectionRange>,
    drag: Option<DragKind>,
}

impl SelectionState {
    /// Create an empty selection state.
    pub fn new() -> Self {
        Self::default()
    }

    /// Current selection range, if one exists.
    pub fn range(&self) -> Option<SelectionRange> {
        self.range
    }

    /// True while a drag gesture is active.
    pub fn is_dragging(&self) -> bool {
        self.drag.is_some()
    }

    /// Begin creating a new selection from the given anchor point.
    pub fn begin_new(&mut self, position: f32) -> SelectionRange {
        let range = SelectionRange::new(position, position);
        self.range = Some(range);
        self.drag = Some(DragKind::Create { anchor: position });
        range
    }

    /// Begin dragging an existing edge; returns false if no selection is present.
    pub fn begin_edge_drag(&mut self, edge: SelectionEdge) -> bool {
        if self.range.is_none() {
            return false;
        }
        self.drag = Some(edge.into());
        true
    }

    /// Update the active drag with a new cursor position.
    pub fn update_drag(&mut self, position: f32) -> Option<SelectionRange> {
        let drag = self.drag?;
        let next_range = match drag {
            DragKind::Create { anchor } => SelectionRange::new(anchor, position),
            DragKind::StartEdge => {
                let range = self.range?;
                SelectionRange::new(position, range.end())
            }
            DragKind::EndEdge => {
                let range = self.range?;
                SelectionRange::new(range.start(), position)
            }
        };
        self.range = Some(next_range);
        Some(next_range)
    }

    /// Update the active drag, snapping the selection length to a beat-sized step.
    pub fn update_drag_snapped(&mut self, position: f32, beat_step: f32) -> Option<SelectionRange> {
        if !beat_step.is_finite() || beat_step <= 0.0 {
            return self.update_drag(position);
        }
        let drag = self.drag?;
        let step = beat_step.clamp(1.0e-6, 1.0);
        let next_range = match drag {
            DragKind::Create { anchor } => {
                let delta = position - anchor;
                let snapped = anchor + snap_delta(delta, step);
                let clamped = clamp01(snapped);
                let mut range = SelectionRange::new(anchor, clamped);
                if range.width() < step {
                    if snapped < 0.0 || snapped > 1.0 {
                        if snapped >= anchor {
                            let end = (anchor + step).min(1.0);
                            let start = (end - step).max(0.0);
                            range = SelectionRange::new(start, end);
                        } else {
                            let start = (anchor - step).max(0.0);
                            let end = (start + step).min(1.0);
                            range = SelectionRange::new(start, end);
                        }
                    }
                }
                range
            }
            DragKind::StartEdge => {
                let range = self.range?;
                let delta = range.end() - position;
                let snapped = range.end() - snap_delta(delta, step);
                SelectionRange::new(snapped, range.end())
            }
            DragKind::EndEdge => {
                let range = self.range?;
                let delta = position - range.start();
                let snapped = range.start() + snap_delta(delta, step);
                SelectionRange::new(range.start(), snapped)
            }
        };
        let next_range = match drag {
            DragKind::Create { .. } => {
                if next_range.width() < step {
                    self.range = None;
                    return None;
                }
                next_range
            }
            DragKind::StartEdge => enforce_min_width(next_range, step, SelectionEdge::Start),
            DragKind::EndEdge => enforce_min_width(next_range, step, SelectionEdge::End),
        };
        self.range = Some(next_range);
        Some(next_range)
    }

    /// Clear the active drag, keeping the current range intact.
    pub fn finish_drag(&mut self) {
        self.drag = None;
    }

    /// Remove any active selection; returns true if something changed.
    pub fn clear(&mut self) -> bool {
        let changed = self.range.is_some();
        self.range = None;
        self.drag = None;
        changed
    }

    /// Replace the current selection without marking a drag active.
    pub fn set_range(&mut self, range: Option<SelectionRange>) {
        self.range = range;
        self.drag = None;
    }
}

fn clamp01(value: f32) -> f32 {
    value.clamp(0.0, 1.0)
}

fn snap_delta(delta: f32, step: f32) -> f32 {
    if !delta.is_finite() || !step.is_finite() || step <= 0.0 {
        return delta;
    }
    (delta / step).round() * step
}

fn enforce_min_width(range: SelectionRange, min_width: f32, anchor: SelectionEdge) -> SelectionRange {
    if range.width() >= min_width {
        return range;
    }
    let step = min_width.clamp(0.0, 1.0);
    match anchor {
        SelectionEdge::Start => {
            let mut end = range.end();
            let mut start = (end - step).max(0.0);
            if (end - start) < step {
                end = (start + step).min(1.0);
                start = (end - step).max(0.0);
            }
            SelectionRange::new(start, end)
        }
        SelectionEdge::End => {
            let mut start = range.start();
            let mut end = (start + step).min(1.0);
            if (end - start) < step {
                start = (end - step).max(0.0);
                end = (start + step).min(1.0);
            }
            SelectionRange::new(start, end)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn assert_range_close(actual: SelectionRange, expected: SelectionRange) {
        let eps = 1e-6;
        assert!((actual.start() - expected.start()).abs() < eps);
        assert!((actual.end() - expected.end()).abs() < eps);
    }

    #[test]
    fn new_range_orders_bounds() {
        let range = SelectionRange::new(0.8, 0.2);
        assert_eq!(range.start(), 0.2);
        assert_eq!(range.end(), 0.8);
    }

    #[test]
    fn empty_range_reports_zero_width() {
        let range = SelectionRange::new(0.5, 0.5);
        assert!(range.is_empty());
        assert_eq!(range.width(), 0.0);
    }

    #[test]
    fn drag_create_tracks_anchor() {
        let mut state = SelectionState::new();
        state.begin_new(0.1);
        let updated = state.update_drag(0.6).unwrap();
        assert_eq!(updated, SelectionRange::new(0.1, 0.6));
    }

    #[test]
    fn drag_updates_clamp_outside_bounds() {
        let mut state = SelectionState::new();
        state.begin_new(0.3);
        let first = state.update_drag(-0.5).unwrap();
        assert_eq!(first, SelectionRange::new(0.0, 0.3));
        let second = state.update_drag(1.4).unwrap();
        assert_eq!(second, SelectionRange::new(0.3, 1.0));
    }

    #[test]
    fn drag_edges_updates_individually() {
        let mut state = SelectionState::new();
        state.begin_new(0.2);
        state.update_drag(0.7);
        assert!(state.begin_edge_drag(SelectionEdge::Start));
        assert!(state.is_dragging());
        state.update_drag(0.1);
        assert_eq!(state.range().unwrap(), SelectionRange::new(0.1, 0.7));
        assert!(state.begin_edge_drag(SelectionEdge::End));
        state.update_drag(0.9);
        assert_eq!(state.range().unwrap(), SelectionRange::new(0.1, 0.9));
        assert!(state.is_dragging());
    }

    #[test]
    fn dragging_state_clears_on_finish() {
        let mut state = SelectionState::new();
        state.begin_new(0.2);
        state.update_drag(0.7);
        assert!(state.is_dragging());
        state.finish_drag();
        assert!(!state.is_dragging());
    }

    #[test]
    fn drag_create_snaps_to_beats() {
        let mut state = SelectionState::new();
        state.begin_new(0.1);
        let updated = state.update_drag_snapped(0.45, 0.25).unwrap();
        assert_range_close(updated, SelectionRange::new(0.1, 0.35));
    }

    #[test]
    fn drag_edge_snaps_to_beats() {
        let mut state = SelectionState::new();
        state.set_range(Some(SelectionRange::new(0.2, 0.8)));
        assert!(state.begin_edge_drag(SelectionEdge::Start));
        let updated = state.update_drag_snapped(0.1, 0.25).unwrap();
        assert_range_close(updated, SelectionRange::new(0.05, 0.8));
    }

    #[test]
    fn drag_create_below_step_clears_range() {
        let mut state = SelectionState::new();
        state.begin_new(0.2);
        let updated = state.update_drag_snapped(0.22, 0.25);
        assert!(updated.is_none());
        assert!(state.range().is_none());
    }

    #[test]
    fn drag_edge_enforces_min_width() {
        let mut state = SelectionState::new();
        state.set_range(Some(SelectionRange::new(0.2, 0.8)));
        assert!(state.begin_edge_drag(SelectionEdge::Start));
        let updated = state.update_drag_snapped(0.75, 0.25).unwrap();
        assert_range_close(updated, SelectionRange::new(0.55, 0.8));
    }

    #[test]
    fn clear_resets_state() {
        let mut state = SelectionState::new();
        state.begin_new(0.2);
        assert!(state.clear());
        assert!(state.range().is_none());
    }

    #[test]
    fn shift_clamps_within_bounds() {
        let range = SelectionRange::new(0.2, 0.4);
        assert_range_close(range.shift(0.1), SelectionRange::new(0.3, 0.5));
        assert_range_close(range.shift(-0.3), SelectionRange::new(0.0, 0.2));
        assert_range_close(range.shift(1.0), SelectionRange::new(0.8, 1.0));
    }

    #[test]
    fn shift_noops_on_nan() {
        let range = SelectionRange::new(0.2, 0.4);
        assert_eq!(range.shift(f32::NAN), range);
    }
}
