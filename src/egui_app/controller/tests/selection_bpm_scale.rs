use super::super::test_support::{dummy_controller, load_waveform_selection};
use super::super::*;
use crate::selection::SelectionEdge;

#[test]
fn alt_drag_scales_selection_and_recalculates_bpm() {
    let (mut controller, source) = dummy_controller();
    let samples = vec![0.0; 32];
    let selection = SelectionRange::new(0.0, 0.5);
    load_waveform_selection(&mut controller, &source, "scale.wav", &samples, selection);

    controller.selection_state.range.set_range(Some(selection));
    controller.apply_selection(Some(selection));
    controller.ui.waveform.loop_enabled = true;
    controller.set_bpm_snap_enabled(true);
    controller.set_bpm_value(120.0);
    controller.ui.waveform.bpm_input = "120".to_string();

    assert!(controller.start_selection_edge_drag(SelectionEdge::End, true));
    controller.update_selection_drag(0.75);

    let updated = controller.ui.waveform.selection.unwrap();
    assert_eq!(updated, SelectionRange::new(0.0, 0.75));
    let bpm = controller.ui.waveform.bpm_value.unwrap();
    assert!((bpm - 80.0).abs() < 0.1);
}
