use super::*;
use crate::egui_app::controller::library::analysis_jobs;
use crate::egui_app::controller::test_support::write_test_wav;
use hound::WavReader;
use std::path::{Path, PathBuf};
use tempfile::tempdir;

#[test]
fn export_selection_clip_to_root_can_flatten_name_hint() {
    let temp = tempdir().unwrap();
    let source_root = temp.path().join("source");
    let clip_root = temp.path().join("export");
    std::fs::create_dir_all(source_root.join("drums")).unwrap();
    std::fs::create_dir_all(&clip_root).unwrap();

    let renderer = crate::waveform::WaveformRenderer::new(12, 12);
    let mut controller = EguiController::new(renderer, None);
    let source = SampleSource::new(source_root.clone());
    controller.library.sources.push(source.clone());

    let orig = source_root.join("drums").join("clip.wav");
    write_test_wav(&orig, &[0.1, 0.2, 0.3, 0.4]);
    controller
        .load_waveform_for_selection(&source, Path::new("drums/clip.wav"))
        .unwrap();

    let entry = controller
        .export_selection_clip_to_root(
            &source.id,
            Path::new("drums/clip.wav"),
            SelectionRange::new(0.25, 0.75),
            None,
            &clip_root,
            Path::new("clip.wav"),
        )
        .unwrap();

    assert!(
        entry
            .relative_path
            .parent()
            .is_none_or(|p| p.as_os_str().is_empty())
    );
    assert!(clip_root.join(&entry.relative_path).is_file());
    assert!(!clip_root.join("drums").join(&entry.relative_path).exists());
}

#[test]
fn next_selection_path_in_dir_strips_existing_suffix() {
    let temp = tempdir().unwrap();
    let root = temp.path();
    std::fs::write(root.join("clip_sel.wav"), b"").unwrap();

    let renderer = crate::waveform::WaveformRenderer::new(12, 12);
    let controller = EguiController::new(renderer, None);
    let candidate = controller.next_selection_path_in_dir(root, Path::new("clip_sel.wav"));

    assert_eq!(candidate, PathBuf::from("clip_sel_2.wav"));
}

#[test]
fn export_selection_clip_marks_loop_and_bpm_when_looping() {
    let temp = tempdir().unwrap();
    let source_root = temp.path().join("source");
    std::fs::create_dir_all(&source_root).unwrap();

    let renderer = crate::waveform::WaveformRenderer::new(12, 12);
    let mut controller = EguiController::new(renderer, None);
    let source = SampleSource::new(source_root.clone());
    controller.library.sources.push(source.clone());

    let wav_path = source_root.join("looping.wav");
    write_test_wav(&wav_path, &[0.1, 0.2, 0.3, 0.4]);
    controller
        .load_waveform_for_selection(&source, Path::new("looping.wav"))
        .unwrap();
    controller.ui.waveform.loop_enabled = true;
    controller.ui.waveform.bpm_value = Some(120.0);

    let entry = controller
        .export_selection_clip(
            &source.id,
            Path::new("looping.wav"),
            SelectionRange::new(0.0, 1.0),
            None,
            true,
            true,
        )
        .unwrap();

    assert!(entry.looped);
    let db = controller.database_for(&source).unwrap();
    assert_eq!(
        db.looped_for_path(&entry.relative_path).unwrap(),
        Some(true)
    );
    let conn = analysis_jobs::open_source_db(&source.root).unwrap();
    let sample_id = analysis_jobs::build_sample_id(source.id.as_str(), &entry.relative_path);
    let bpm = analysis_jobs::sample_bpm(&conn, &sample_id).unwrap();
    assert_eq!(bpm, Some(120.0));
}

#[test]
fn export_selection_clip_applies_short_edge_fades_when_enabled() {
    let temp = tempdir().unwrap();
    let source_root = temp.path().join("source");
    std::fs::create_dir_all(&source_root).unwrap();

    let renderer = crate::waveform::WaveformRenderer::new(12, 12);
    let mut controller = EguiController::new(renderer, None);
    let source = SampleSource::new(source_root.clone());
    controller.library.sources.push(source.clone());
    controller.settings.controls.auto_edge_fades_on_selection_exports = true;
    controller.ui.controls.auto_edge_fades_on_selection_exports = true;
    controller.settings.controls.anti_clip_fade_ms = 250.0;
    controller.ui.controls.anti_clip_fade_ms = 250.0;

    let wav_path = source_root.join("fades.wav");
    write_test_wav(&wav_path, &vec![1.0; 8]);
    controller
        .load_waveform_for_selection(&source, Path::new("fades.wav"))
        .unwrap();

    let entry = controller
        .export_selection_clip(
            &source.id,
            Path::new("fades.wav"),
            SelectionRange::new(0.0, 1.0),
            None,
            true,
            true,
        )
        .unwrap();

    let target = source_root.join(&entry.relative_path);
    let mut reader = WavReader::open(&target).unwrap();
    let samples: Vec<f32> = reader.samples::<f32>().map(|s| s.unwrap()).collect();

    assert_eq!(samples.len(), 8);
    assert!(samples[0].abs() < 1e-6);
    assert!(samples[7].abs() < 1e-6);
    assert!((samples[1] - 1.0).abs() < 1e-6);
    assert!((samples[6] - 1.0).abs() < 1e-6);
}
