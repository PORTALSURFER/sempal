use super::super::test_support::dummy_controller;

#[test]
fn transient_snap_restores_after_marker_toggle() {
    let (mut controller, _source) = dummy_controller();
    controller.settings.controls.transient_markers_enabled = true;
    controller.ui.waveform.transient_markers_enabled = true;
    controller.settings.controls.transient_snap_enabled = true;
    controller.ui.waveform.transient_snap_enabled = true;

    controller.set_transient_markers_enabled(false);

    assert!(!controller.ui.waveform.transient_markers_enabled);
    assert!(!controller.ui.waveform.transient_snap_enabled);
    assert!(controller.settings.controls.transient_snap_enabled);

    controller.set_transient_markers_enabled(true);

    assert!(controller.ui.waveform.transient_markers_enabled);
    assert!(controller.ui.waveform.transient_snap_enabled);
}
