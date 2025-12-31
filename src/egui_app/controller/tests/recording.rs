use super::super::test_support::dummy_controller;

#[test]
fn stop_recording_without_active_recorder_is_noop() {
    let (mut controller, _source) = dummy_controller();
    let result = controller.stop_recording().unwrap();
    assert!(result.is_none());
}
