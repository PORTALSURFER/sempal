//! Helpers for tracking waveform selection ranges and drag interactions.
//! This module keeps selection math pure and testable so the UI integration code can stay small.

/// Parameters for a fade curve (in or out).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct FadeParams {
    /// Fade length as a fraction of selection width (0.0-1.0).
    pub length: f32,
    /// Curve tension: 0.0 = linear, 0.5 = medium S-curve, 1.0 = maximum S-curve.
    pub curve: f32,
    /// Muted region length as a fraction of selection width.
    /// This region extends outward from the selection edge.
    pub mute: f32,
}

impl FadeParams {
    /// Create new fade parameters with default curve.
    pub fn new(length: f32) -> Self {
        Self {
            length: length.clamp(0.0, 1.0),
            curve: 0.5,
            mute: 0.0,
        }
    }

    /// Create fade parameters with custom curve.
    pub fn with_curve(length: f32, curve: f32) -> Self {
        Self {
            length: length.clamp(0.0, 1.0),
            curve: curve.clamp(0.0, 1.0),
            mute: 0.0,
        }
    }

    /// Create fade parameters with custom curve and muted length.
    pub fn with_curve_and_mute(length: f32, curve: f32, mute: f32) -> Self {
        let clamped_length = length.clamp(0.0, 1.0);
        let clamped_mute = mute.max(0.0);
        Self {
            length: clamped_length,
            curve: curve.clamp(0.0, 1.0),
            mute: clamped_mute,
        }
    }
}

/// Normalized selection bounds over a waveform (0.0 - 1.0).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct SelectionRange {
    start: f32,
    end: f32,
    /// Gain applied across the selection (1.0 = unity).
    gain: f32,
    /// Fade-in parameters (length and curve).
    fade_in: Option<FadeParams>,
    /// Fade-out parameters (length and curve).
    fade_out: Option<FadeParams>,
}

impl SelectionRange {
    /// Create a clamped range, ensuring `start` is not greater than `end`.
    pub fn new(start: f32, end: f32) -> Self {
        let a = clamp01(start);
        let b = clamp01(end);
        if a <= b {
            Self {
                start: a,
                end: b,
                gain: 1.0,
                fade_in: None,
                fade_out: None,
            }
        } else {
            Self {
                start: b,
                end: a,
                gain: 1.0,
                fade_in: None,
                fade_out: None,
            }
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

    /// Gain applied across the selection.
    pub fn gain(&self) -> f32 {
        self.gain
    }

    /// True when the selection has zero width.
    #[allow(dead_code)]
    pub fn is_empty(&self) -> bool {
        self.width() == 0.0
    }

    /// Get fade-in parameters if set.
    pub fn fade_in(&self) -> Option<FadeParams> {
        self.fade_in
    }

    /// Get fade-out parameters if set.
    pub fn fade_out(&self) -> Option<FadeParams> {
        self.fade_out
    }

    /// Get fade-in length (0.0 if no fade).
    pub fn fade_in_length(&self) -> f32 {
        self.fade_in.map(|f| f.length).unwrap_or(0.0)
    }

    /// Get fade-in muted length (0.0 if no fade).
    pub fn fade_in_mute_length(&self) -> f32 {
        self.fade_in.map(|f| f.mute).unwrap_or(0.0)
    }

    /// Get fade-out length (0.0 if no fade).
    pub fn fade_out_length(&self) -> f32 {
        self.fade_out.map(|f| f.length).unwrap_or(0.0)
    }

    /// Get fade-out muted length (0.0 if no fade).
    pub fn fade_out_mute_length(&self) -> f32 {
        self.fade_out.map(|f| f.mute).unwrap_or(0.0)
    }

    /// True when the selection has a non-zero fade-in or fade-out length configured.
    pub fn has_fades(&self) -> bool {
        self.fade_in_length() > 0.0 || self.fade_out_length() > 0.0
    }

    /// True when the selection has any edit effects configured.
    pub fn has_edit_effects(&self) -> bool {
        self.has_fades()
            || self.fade_in_mute_length() > 0.0
            || self.fade_out_mute_length() > 0.0
            || (self.gain - 1.0).abs() > f32::EPSILON
    }

    /// Set fade-in parameters.
    ///
    /// Keeps a zero-length fade when a mute region is configured so mute handles persist.
    pub fn with_fade_in(mut self, length: f32, curve: f32) -> Self {
        let clamped_length = clamp_fade_length(length, self.fade_out_length());
        let current_mute = self.fade_in.map(|f| f.mute).unwrap_or(0.0);
        let clamped_mute = clamp_mute_length(current_mute, self.max_fade_in_mute_length());
        if clamped_length > 0.0 || clamped_mute > 0.0 {
            self.fade_in = Some(FadeParams::with_curve_and_mute(
                clamped_length,
                curve,
                clamped_mute,
            ));
        } else {
            self.fade_in = None;
        }
        self
    }

    /// Set fade-out parameters.
    ///
    /// Keeps a zero-length fade when a mute region is configured so mute handles persist.
    pub fn with_fade_out(mut self, length: f32, curve: f32) -> Self {
        let clamped_length = clamp_fade_length(length, self.fade_in_length());
        let current_mute = self.fade_out.map(|f| f.mute).unwrap_or(0.0);
        let clamped_mute = clamp_mute_length(current_mute, self.max_fade_out_mute_length());
        if clamped_length > 0.0 || clamped_mute > 0.0 {
            self.fade_out = Some(FadeParams::with_curve_and_mute(
                clamped_length,
                curve,
                clamped_mute,
            ));
        } else {
            self.fade_out = None;
        }
        self
    }

    /// Set fade-in muted length while preserving the curve.
    pub fn with_fade_in_mute(mut self, mute: f32) -> Self {
        if let Some(fade) = self.fade_in {
            let clamped_mute = clamp_mute_length(mute, self.max_fade_in_mute_length());
            self.fade_in = Some(FadeParams::with_curve_and_mute(
                fade.length,
                fade.curve,
                clamped_mute,
            ));
        }
        self
    }

    /// Set fade-out muted length while preserving the curve.
    pub fn with_fade_out_mute(mut self, mute: f32) -> Self {
        if let Some(fade) = self.fade_out {
            let clamped_mute = clamp_mute_length(mute, self.max_fade_out_mute_length());
            self.fade_out = Some(FadeParams::with_curve_and_mute(
                fade.length,
                fade.curve,
                clamped_mute,
            ));
        }
        self
    }

    /// Maximum fade-in mute length based on distance to the sample start.
    pub fn max_fade_in_mute_length(&self) -> f32 {
        let width = self.width();
        if width <= 0.0 {
            return 0.0;
        }
        (self.start / width).max(0.0)
    }

    /// Maximum fade-out mute length based on distance to the sample end.
    pub fn max_fade_out_mute_length(&self) -> f32 {
        let width = self.width();
        if width <= 0.0 {
            return 0.0;
        }
        ((1.0 - self.end) / width).max(0.0)
    }

    /// Set the selection gain (0.0-4.0).
    pub fn with_gain(mut self, gain: f32) -> Self {
        self.gain = clamp_gain(gain);
        self
    }

    /// Clear all fades.
    pub fn clear_fades(mut self) -> Self {
        self.fade_in = None;
        self.fade_out = None;
        self
    }

    /// Shift the selection by the given delta, clamping to the waveform bounds.
    pub fn shift(self, delta: f32) -> Self {
        if !delta.is_finite() {
            return self;
        }
        let width = self.width().clamp(0.0, 1.0);
        if width >= 1.0 {
            let mut range = SelectionRange::new(0.0, 1.0);
            if let Some(fade_in) = self.fade_in {
                range = range
                    .with_fade_in(fade_in.length, fade_in.curve)
                    .with_fade_in_mute(fade_in.mute);
            }
            if let Some(fade_out) = self.fade_out {
                range = range
                    .with_fade_out(fade_out.length, fade_out.curve)
                    .with_fade_out_mute(fade_out.mute);
            }
            range.gain = self.gain;
            return range;
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
        let mut result = SelectionRange::new(start, end);
        result.fade_in = self.fade_in;
        result.fade_out = self.fade_out;
        result.gain = self.gain;
        result
    }
}

/// Compute the fade gain for a position within or outside a selection span.
///
/// The position and selection bounds share the same unit (seconds or normalized 0-1).
/// Returns 1.0 outside the selection or when the selection is empty.
/// `selection_gain` scales the entire selection after fades/mutes are applied.
/// `min_fade_len` is an optional minimum fade length in the same unit, used when a mute
/// region exists but the fade length is zero (for click-free edges).
pub(crate) fn fade_gain_at_position(
    position: f32,
    selection_start: f32,
    selection_end: f32,
    selection_gain: f32,
    fade_in: Option<FadeParams>,
    fade_out: Option<FadeParams>,
    min_fade_len: f32,
) -> f32 {
    let start = selection_start.min(selection_end);
    let end = selection_start.max(selection_end);
    let width = end - start;
    if width <= 0.0 {
        return 1.0;
    }
    if let Some(fade_in) = fade_in {
        let mute_len = (width * fade_in.mute).max(0.0);
        if mute_len > 0.0 {
            let mute_start = start - mute_len;
            if position >= mute_start && position <= start {
                return 0.0;
            }
        }
    }
    if let Some(fade_out) = fade_out {
        let mute_len = (width * fade_out.mute).max(0.0);
        if mute_len > 0.0 {
            let mute_end = end + mute_len;
            if position >= end && position <= mute_end {
                return 0.0;
            }
        }
    }
    if position < start || position > end {
        return 1.0;
    }
    let mut gain = 1.0;
    if let Some(fade_in) = fade_in {
        let fade_len = width * fade_in.length;
        let fade_len = if fade_len > 0.0 {
            fade_len
        } else if fade_in.mute > 0.0 && min_fade_len > 0.0 {
            min_fade_len.min(width)
        } else {
            0.0
        };
        if fade_len > 0.0 {
            let time_in = position - start;
            if time_in < fade_len {
                let t = (time_in / fade_len).clamp(0.0, 1.0);
                gain *= fade_curve_value(t, fade_in.curve);
            }
        }
    }
    if let Some(fade_out) = fade_out {
        let fade_len = width * fade_out.length;
        let fade_len = if fade_len > 0.0 {
            fade_len
        } else if fade_out.mute > 0.0 && min_fade_len > 0.0 {
            min_fade_len.min(width)
        } else {
            0.0
        };
        if fade_len > 0.0 {
            let time_until_end = end - position;
            if time_until_end < fade_len {
                let t = (time_until_end / fade_len).clamp(0.0, 1.0);
                gain *= fade_curve_value(t, fade_out.curve);
            }
        }
    }
    gain * clamp_gain(selection_gain)
}

/// Apply an S-curve easing for fade ramps.
pub(crate) fn fade_curve_value(t: f32, curve: f32) -> f32 {
    if curve <= 0.0 {
        return t;
    }
    let t = t.clamp(0.0, 1.0);
    let t2 = t * t;
    let t3 = t2 * t;
    let smootherstep = t3 * (t * (t * 6.0 - 15.0) + 10.0);
    t * (1.0 - curve) + smootherstep * curve
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

fn clamp_fade_length(fade: f32, other_fade: f32) -> f32 {
    let clamped = fade.clamp(0.0, 1.0);
    let other = (other_fade as f64).clamp(0.0, 1.0);
    let max_allowed = (1.0_f64 - other).max(0.0) as f32;
    round_fade_length(clamped.min(max_allowed))
}

fn clamp_mute_length(mute: f32, max_mute: f32) -> f32 {
    mute.clamp(0.0, max_mute.max(0.0))
}

fn round_fade_length(value: f32) -> f32 {
    let scale = 1_000_000.0;
    (value * scale).round() / scale
}

fn clamp_gain(gain: f32) -> f32 {
    gain.clamp(0.0, 4.0)
}

fn snap_delta(delta: f32, step: f32) -> f32 {
    if !delta.is_finite() || !step.is_finite() || step <= 0.0 {
        return delta;
    }
    (delta / step).round() * step
}

fn enforce_min_width(
    range: SelectionRange,
    min_width: f32,
    anchor: SelectionEdge,
) -> SelectionRange {
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

    #[test]
    fn fade_values_are_clamped() {
        let range = SelectionRange::new(0.2, 0.8)
            .with_fade_in(0.6, 0.5)
            .with_fade_out(0.6, 0.5);
        // fade_in + fade_out should not exceed 1.0
        assert!(range.fade_in_length() + range.fade_out_length() <= 1.0);
    }

    #[test]
    fn fade_in_clamps_when_fade_out_exists() {
        let range = SelectionRange::new(0.2, 0.8)
            .with_fade_out(0.7, 0.5)
            .with_fade_in(0.5, 0.5);
        assert_eq!(range.fade_out_length(), 0.7);
        assert_eq!(range.fade_in_length(), 0.3); // Clamped to 1.0 - 0.7
    }

    #[test]
    fn fade_out_clamps_when_fade_in_exists() {
        let range = SelectionRange::new(0.2, 0.8)
            .with_fade_in(0.6, 0.5)
            .with_fade_out(0.8, 0.5);
        assert_eq!(range.fade_in_length(), 0.6);
        assert_eq!(range.fade_out_length(), 0.4); // Clamped to 1.0 - 0.6
    }

    #[test]
    fn fades_preserved_during_shift() {
        let range = SelectionRange::new(0.2, 0.4)
            .with_fade_in(0.2, 0.5)
            .with_fade_out(0.3, 0.5);
        let shifted = range.shift(0.1);
        assert_eq!(shifted.fade_in_length(), 0.2);
        assert_eq!(shifted.fade_out_length(), 0.3);
    }

    #[test]
    fn fade_mute_sections_zero_gain() {
        let range = SelectionRange::new(0.2, 0.8)
            .with_fade_in(0.4, 0.0)
            .with_fade_out(0.4, 0.0)
            .with_fade_in_mute(0.2)
            .with_fade_out_mute(0.1);
        let muted_start = fade_gain_at_position(
            0.1,
            range.start(),
            range.end(),
            range.gain(),
            range.fade_in(),
            range.fade_out(),
            0.0,
        );
        let muted_end = fade_gain_at_position(
            0.82,
            range.start(),
            range.end(),
            range.gain(),
            range.fade_in(),
            range.fade_out(),
            0.0,
        );
        let ramp_mid = fade_gain_at_position(
            0.5,
            range.start(),
            range.end(),
            range.gain(),
            range.fade_in(),
            range.fade_out(),
            0.0,
        );
        assert!(muted_start.abs() < 1e-6);
        assert!(muted_end.abs() < 1e-6);
        assert!(ramp_mid > 0.0 && ramp_mid < 1.0);
    }

    #[test]
    fn fade_mute_can_extend_past_selection_width() {
        let range = SelectionRange::new(0.4, 0.5)
            .with_fade_in(0.2, 0.0)
            .with_fade_out(0.2, 0.0)
            .with_fade_in_mute(4.0);
        let muted_far_left = fade_gain_at_position(
            0.05,
            range.start(),
            range.end(),
            range.gain(),
            range.fade_in(),
            range.fade_out(),
            0.0,
        );
        assert!(muted_far_left.abs() < 1e-6);
    }

    #[test]
    fn fade_mute_persists_when_fade_length_collapses() {
        let range = SelectionRange::new(0.2, 0.8)
            .with_fade_in(0.3, 0.5)
            .with_fade_in_mute(0.2)
            .with_fade_in(0.0, 0.5);
        assert_eq!(range.fade_in_length(), 0.0);
        assert!(range.fade_in().is_some());
        assert!(range.fade_in_mute_length() > 0.0);
    }

    #[test]
    fn new_range_has_zero_fades() {
        let range = SelectionRange::new(0.3, 0.7);
        assert_eq!(range.fade_in_length(), 0.0);
        assert_eq!(range.fade_out_length(), 0.0);
    }

    #[test]
    fn fade_gain_ramps_selection_edges() {
        let range = SelectionRange::new(0.0, 1.0)
            .with_fade_in(0.2, 0.0)
            .with_fade_out(0.2, 0.0);
        let gain_start = fade_gain_at_position(
            0.0,
            range.start(),
            range.end(),
            range.gain(),
            range.fade_in(),
            range.fade_out(),
            0.0,
        );
        let gain_mid = fade_gain_at_position(
            0.5,
            range.start(),
            range.end(),
            range.gain(),
            range.fade_in(),
            range.fade_out(),
            0.0,
        );
        let gain_end = fade_gain_at_position(
            1.0,
            range.start(),
            range.end(),
            range.gain(),
            range.fade_in(),
            range.fade_out(),
            0.0,
        );
        assert!(gain_start.abs() < 1e-6);
        assert!((gain_mid - 1.0).abs() < 1e-6);
        assert!(gain_end.abs() < 1e-6);
    }

    #[test]
    fn fade_mute_does_not_extend_fade_curve() {
        let range = SelectionRange::new(0.0, 1.0)
            .with_fade_in(0.2, 0.0)
            .with_fade_in_mute(0.3);
        let post_fade_gain = fade_gain_at_position(
            0.25,
            range.start(),
            range.end(),
            range.gain(),
            range.fade_in(),
            range.fade_out(),
            0.0,
        );
        assert!((post_fade_gain - 1.0).abs() < 1e-6);
    }
}
