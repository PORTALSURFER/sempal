use super::super::test_support::{dummy_controller, load_waveform_selection};
use super::super::*;

#[test]
fn edit_selection_drag_does_not_snap_to_bpm() {
    let (mut controller, source) = dummy_controller();
    let samples = vec![0.0; 32];
    let selection = SelectionRange::new(0.0, 0.5);
    load_waveform_selection(&mut controller, &source, "edit_no_snap.wav", &samples, selection);

    controller.set_bpm_snap_enabled(true);
    controller.set_bpm_value(120.0);
    controller.ui.waveform.bpm_input = "120".to_string();

    // Start edit selection drag at a position that would normally snap to start (0.005)
    controller.start_edit_selection_drag(0.005);
    
    let updated = controller.ui.waveform.edit_selection.unwrap();
    // It should NOT have snapped to 0.0
    assert!((updated.start() - 0.005).abs() < 1e-6);

    // Update drag to a position that would normally snap to a beat
    // 120 BPM, say the samples duration is 1 second (implicit in some logic, but let's be explicit if possible)
    // Actually bpm_snap_step depends on loaded_audio.duration_seconds.
    // In dummy_controller it might be different.
    
    let duration = controller.loaded_audio_duration_seconds().unwrap();
    let step = 60.0 / 120.0 / duration; // 0.5 / duration
    
    let target = step * 0.55; // Slightly past half-way to first beat
    controller.update_edit_selection_drag(target, false);
    
    let updated = controller.ui.waveform.edit_selection.unwrap();
    // It should NOT have snapped to 0.0 or step
    assert!((updated.end() - target).abs() < 1e-6);
}
