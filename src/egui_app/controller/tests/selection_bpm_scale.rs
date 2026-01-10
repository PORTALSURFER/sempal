use super::super::library::analysis_jobs;
use super::super::test_support::{dummy_controller, load_waveform_selection};
use super::super::*;
use crate::selection::SelectionEdge;
use rusqlite::params;
use std::path::Path;

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
    controller.update_selection_drag(0.75, false);

    let updated = controller.ui.waveform.selection.unwrap();
    assert_eq!(updated, SelectionRange::new(0.0, 0.75));
    let bpm = controller.ui.waveform.bpm_value.unwrap();
    assert!((bpm - 80.0).abs() < 0.1);
}

#[test]
fn alt_drag_scales_without_loop_enabled() {
    let (mut controller, source) = dummy_controller();
    let samples = vec![0.0; 32];
    let selection = SelectionRange::new(0.0, 0.5);
    load_waveform_selection(
        &mut controller,
        &source,
        "scale_no_loop.wav",
        &samples,
        selection,
    );

    controller.selection_state.range.set_range(Some(selection));
    controller.apply_selection(Some(selection));
    controller.ui.waveform.loop_enabled = false;
    controller.set_bpm_snap_enabled(true);
    controller.set_bpm_value(120.0);
    controller.ui.waveform.bpm_input = "120".to_string();

    assert!(controller.start_selection_edge_drag(SelectionEdge::End, true));
    controller.update_selection_drag(0.75, false);

    let updated = controller.ui.waveform.selection.unwrap();
    assert_eq!(updated, SelectionRange::new(0.0, 0.75));
    let bpm = controller.ui.waveform.bpm_value.unwrap();
    assert!((bpm - 80.0).abs() < 0.1);
}

#[test]
fn shift_resizedrag_overrides_bpm_snapping() {
    let (mut controller, source) = dummy_controller();
    let samples = vec![0.0; 32];
    let selection = SelectionRange::new(0.0, 0.5);
    load_waveform_selection(
        &mut controller,
        &source,
        "shift_override.wav",
        &samples,
        selection,
    );

    controller.selection_state.range.set_range(Some(selection));
    controller.apply_selection(Some(selection));
    controller.set_bpm_snap_enabled(true);
    controller.set_bpm_value(120.0);
    controller.ui.waveform.bpm_input = "120".to_string();

    assert!(controller.start_selection_edge_drag(SelectionEdge::End, false));
    controller.update_selection_drag(0.73, true);

    let updated = controller.ui.waveform.selection.unwrap();
    assert_eq!(updated, SelectionRange::new(0.0, 0.73));
}

#[test]
fn start_drag_snaps_to_start_when_bpm_snap_enabled() {
    let (mut controller, source) = dummy_controller();
    let samples = vec![0.0; 32];
    let selection = SelectionRange::new(0.2, 0.4);
    load_waveform_selection(
        &mut controller,
        &source,
        "start_snap.wav",
        &samples,
        selection,
    );

    controller.set_bpm_snap_enabled(true);
    controller.set_bpm_value(120.0);
    controller.ui.waveform.bpm_input = "120".to_string();

    controller.start_selection_drag(0.005);

    let updated = controller.ui.waveform.selection.unwrap();
    assert!((updated.start() - 0.0).abs() < 1e-6);
}

#[test]
fn bpm_snap_value_does_not_persist_to_sample_metadata() {
    let (mut controller, source) = dummy_controller();
    let samples = vec![0.0; 32];
    let selection = SelectionRange::new(0.0, 0.5);
    let filename = "bpm_metadata.wav";
    let sample_id = analysis_jobs::build_sample_id(source.id.as_str(), Path::new(filename));
    let conn = analysis_jobs::open_source_db(&source.root).unwrap();
    conn.execute(
        "INSERT INTO samples (sample_id, content_hash, size, mtime_ns) VALUES (?1, ?2, ?3, ?4)",
        params![sample_id, "test-hash", 1i64, 1i64],
    )
    .unwrap();
    analysis_jobs::update_sample_bpm(&conn, &sample_id, Some(100.0)).unwrap();

    load_waveform_selection(&mut controller, &source, filename, &samples, selection);
    controller.set_bpm_snap_enabled(true);
    controller.set_bpm_value(120.0);

    let bpm = analysis_jobs::sample_bpm(&conn, &sample_id).unwrap();
    assert_eq!(bpm, Some(100.0));
}
