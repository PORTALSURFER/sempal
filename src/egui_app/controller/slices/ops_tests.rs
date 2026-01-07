use super::ops::{
    SliceSnapState, apply_painted_slice, snap_slice_paint_position, update_slice_range,
};
use crate::selection::SelectionRange;

#[test]
fn apply_painted_slice_splits_overlaps() {
    let slices = vec![SelectionRange::new(0.1, 0.4), SelectionRange::new(0.5, 0.8)];
    let range = SelectionRange::new(0.3, 0.6);
    let updated = apply_painted_slice(&slices, range, 0.01).expect("valid range");

    assert_eq!(updated.len(), 3);
    assert_eq!(updated[0], SelectionRange::new(0.1, 0.3));
    assert_eq!(updated[1], SelectionRange::new(0.3, 0.6));
    assert_eq!(updated[2], SelectionRange::new(0.6, 0.8));
}

#[test]
fn update_slice_range_keeps_selection_when_updated_slice_selected() {
    let slices = vec![SelectionRange::new(0.1, 0.3), SelectionRange::new(0.4, 0.6)];
    let selected = vec![0];
    let range = SelectionRange::new(0.15, 0.25);
    let result = update_slice_range(&slices, &selected, 0, range, 0.01).expect("valid update");

    assert_eq!(result.new_index, Some(0));
    assert_eq!(result.selected_indices, vec![0]);
    assert_eq!(result.slices[0], SelectionRange::new(0.15, 0.25));
}

#[test]
fn snap_slice_paint_position_uses_bpm_step() {
    let state = SliceSnapState {
        bpm_snap_enabled: true,
        bpm_value: Some(120.0),
        duration_seconds: Some(10.0),
        transient_markers_enabled: false,
        transient_snap_enabled: false,
        transients: Vec::new(),
    };

    let snapped = snap_slice_paint_position(&state, 0.22, false);
    assert!((snapped - 0.2).abs() < 0.0001);
}

#[test]
fn snap_slice_paint_position_uses_transients() {
    let state = SliceSnapState {
        bpm_snap_enabled: false,
        bpm_value: None,
        duration_seconds: None,
        transient_markers_enabled: true,
        transient_snap_enabled: true,
        transients: vec![0.12, 0.5],
    };

    let snapped = snap_slice_paint_position(&state, 0.115, false);
    assert!((snapped - 0.12).abs() < 0.0001);
}
