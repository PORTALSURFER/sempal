use crate::egui_app::state::TriageFlagColumn;
use super::super::test_support::{sample_entry, write_test_wav};
use super::super::*;
use crate::egui_app::state::{DragPayload, DragSource, DragTarget};
use std::path::PathBuf;
use tempfile::tempdir;

#[test]
fn waveform_sample_drop_copies_and_registers() {
    let temp = tempdir().unwrap();
    let root = temp.path().join("source");
    std::fs::create_dir_all(&root).unwrap();
    let renderer = WaveformRenderer::new(12, 12);
    let mut controller = EguiController::new(renderer, None);
    let source = SampleSource::new(root.clone());
    controller.library.sources.push(source.clone());
    controller.selection_state.ctx.selected_source = Some(source.id.clone());
    controller.cache_db(&source).unwrap();

    write_test_wav(&root.join("one.wav"), &[0.1, 0.2]);
    controller.set_wav_entries_for_tests(vec![sample_entry("one.wav", crate::sample_sources::Rating::NEUTRAL)]);
    controller.rebuild_wav_lookup();
    controller.rebuild_browser_lists();

    controller.ui.drag.payload = Some(DragPayload::Sample {
        source_id: source.id.clone(),
        relative_path: PathBuf::from("one.wav"),
    });
    controller.ui.drag.origin_source = Some(DragSource::Waveform);
    controller.ui.drag.set_target(
        DragSource::Browser,
        DragTarget::BrowserTriage(TriageFlagColumn::Keep),
    );
    controller.finish_active_drag();

    assert!(root.join("one_copy001.wav").is_file());
    assert!(controller.ui.status.text.contains("Copied sample"));
}

#[test]
fn waveform_sample_drop_reports_missing_source_file() {
    let temp = tempdir().unwrap();
    let root = temp.path().join("source");
    std::fs::create_dir_all(&root).unwrap();
    let renderer = WaveformRenderer::new(12, 12);
    let mut controller = EguiController::new(renderer, None);
    let source = SampleSource::new(root.clone());
    controller.library.sources.push(source.clone());
    controller.selection_state.ctx.selected_source = Some(source.id.clone());

    controller.ui.drag.payload = Some(DragPayload::Sample {
        source_id: source.id.clone(),
        relative_path: PathBuf::from("missing.wav"),
    });
    controller.ui.drag.origin_source = Some(DragSource::Waveform);
    controller.ui.drag.set_target(
        DragSource::Browser,
        DragTarget::BrowserTriage(TriageFlagColumn::Keep),
    );
    controller.finish_active_drag();

    assert!(controller.ui.status.text.contains("Source file missing"));
}
