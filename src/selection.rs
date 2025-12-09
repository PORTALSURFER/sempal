//! Helpers for tracking waveform selection ranges and drag interactions.
///
/// This module keeps selection math pure and testable so the UI integration code can stay small.

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

#[cfg(test)]
mod tests {
    use super::*;

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
    fn clear_resets_state() {
        let mut state = SelectionState::new();
        state.begin_new(0.2);
        assert!(state.clear());
        assert!(state.range().is_none());
    }
}
