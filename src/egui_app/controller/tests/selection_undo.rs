use super::super::test_support::dummy_controller;
use super::super::*;

#[test]
fn undo_redo_selection_drag() {
    let (mut controller, _source) = dummy_controller();

    controller.start_selection_drag(0.2);
    controller.update_selection_drag(0.6, false);
    controller.finish_selection_drag();

    let expected = SelectionRange::new(0.2, 0.6);
    assert_eq!(controller.ui.waveform.selection, Some(expected));

    controller.undo();
    assert!(controller.ui.waveform.selection.is_none());

    controller.redo();
    assert_eq!(controller.ui.waveform.selection, Some(expected));
}

#[test]
fn undo_restores_cleared_selection() {
    let (mut controller, _source) = dummy_controller();
    let selection = SelectionRange::new(0.1, 0.4);
    controller.selection_state.range.set_range(Some(selection));
    controller.apply_selection(Some(selection));

    controller.clear_selection();
    assert!(controller.ui.waveform.selection.is_none());

    controller.undo();
    assert_eq!(controller.ui.waveform.selection, Some(selection));
}
