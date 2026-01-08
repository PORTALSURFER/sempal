use crate::selection::SelectionRange;
use std::cmp::Ordering;

#[derive(Debug, Clone)]
pub(crate) struct SliceSnapState {
    pub(crate) bpm_snap_enabled: bool,
    pub(crate) bpm_value: Option<f32>,
    pub(crate) duration_seconds: Option<f32>,
    pub(crate) transient_markers_enabled: bool,
    pub(crate) transient_snap_enabled: bool,
    pub(crate) transients: Vec<f32>,
}

#[derive(Debug, Clone, PartialEq)]
pub(crate) struct SliceUpdateResult {
    pub(crate) slices: Vec<SelectionRange>,
    pub(crate) selected_indices: Vec<usize>,
    pub(crate) new_index: Option<usize>,
}

pub(crate) fn apply_painted_slice(
    slices: &[SelectionRange],
    range: SelectionRange,
    min_width: f32,
) -> Option<Vec<SelectionRange>> {
    if range.width() < min_width {
        return None;
    }
    let mut updated = Vec::with_capacity(slices.len() + 1);
    for slice in slices.iter().copied() {
        if !ranges_overlap(slice, range) {
            updated.push(slice);
            continue;
        }
        if slice.start() < range.start() {
            let left = SelectionRange::new(slice.start(), range.start());
            if left.width() >= min_width {
                updated.push(left);
            }
        }
        if slice.end() > range.end() {
            let right = SelectionRange::new(range.end(), slice.end());
            if right.width() >= min_width {
                updated.push(right);
            }
        }
    }
    updated.push(range);
    updated.sort_by(|a, b| a.start().partial_cmp(&b.start()).unwrap_or(Ordering::Equal));
    Some(updated)
}

pub(crate) fn update_slice_range(
    slices: &[SelectionRange],
    selected_indices: &[usize],
    index: usize,
    range: SelectionRange,
    min_width: f32,
) -> Option<SliceUpdateResult> {
    if index >= slices.len() {
        return None;
    }
    if range.width() < min_width {
        return None;
    }
    let was_selected = selected_indices.contains(&index);
    let selected_ranges: Vec<SelectionRange> = selected_indices
        .iter()
        .filter_map(|&selected| slices.get(selected).copied())
        .collect();
    let mut updated = Vec::with_capacity(slices.len() + 1);
    for (current_index, slice) in slices.iter().copied().enumerate() {
        if current_index == index {
            continue;
        }
        if !ranges_overlap(slice, range) {
            updated.push(slice);
            continue;
        }
        if slice.start() < range.start() {
            let left = SelectionRange::new(slice.start(), range.start());
            if left.width() >= min_width {
                updated.push(left);
            }
        }
        if slice.end() > range.end() {
            let right = SelectionRange::new(range.end(), slice.end());
            if right.width() >= min_width {
                updated.push(right);
            }
        }
    }
    updated.push(range);
    updated.sort_by(|a, b| a.start().partial_cmp(&b.start()).unwrap_or(Ordering::Equal));
    let new_index = updated.iter().position(|slice| *slice == range);
    let mut new_selected = Vec::new();
    if was_selected {
        if let Some(index) = new_index {
            new_selected.push(index);
        }
    }
    for selected in selected_ranges {
        if let Some(index) = updated.iter().position(|slice| *slice == selected) {
            new_selected.push(index);
        }
    }
    new_selected.sort_unstable();
    new_selected.dedup();
    Some(SliceUpdateResult {
        slices: updated,
        selected_indices: new_selected,
        new_index,
    })
}

pub(crate) fn snap_slice_paint_position(
    state: &SliceSnapState,
    position: f32,
    snap_override: bool,
) -> f32 {
    if snap_override {
        return position;
    }
    if let Some(step) = slice_bpm_snap_step(state) {
        return snap_to_step(position, step);
    }
    if let Some(snapped) = snap_to_transient(state, position) {
        return snapped;
    }
    position
}

pub(crate) fn ranges_overlap(a: SelectionRange, b: SelectionRange) -> bool {
    a.start() < b.end() && a.end() > b.start()
}

fn slice_bpm_snap_step(state: &SliceSnapState) -> Option<f32> {
    if !state.bpm_snap_enabled {
        return None;
    }
    let bpm = state.bpm_value?;
    if !bpm.is_finite() || bpm <= 0.0 {
        return None;
    }
    let duration = state.duration_seconds?;
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

fn snap_to_step(position: f32, step: f32) -> f32 {
    if !position.is_finite() || !step.is_finite() || step <= 0.0 {
        return position;
    }
    (position / step).round().mul_add(step, 0.0).clamp(0.0, 1.0)
}

fn snap_to_transient(state: &SliceSnapState, position: f32) -> Option<f32> {
    const TRANSIENT_SNAP_RADIUS: f32 = 0.01;
    if !state.transient_markers_enabled || !state.transient_snap_enabled {
        return None;
    }
    let mut closest = None;
    let mut best_distance = TRANSIENT_SNAP_RADIUS;
    for &marker in &state.transients {
        let distance = (marker - position).abs();
        if distance <= best_distance {
            best_distance = distance;
            closest = Some(marker);
        }
    }
    closest
}
