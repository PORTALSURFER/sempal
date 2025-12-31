use crate::egui_app::controller::test_support::write_test_wav;
use crate::egui_app::controller::EguiController;
use crate::sample_sources::SampleSource;
use crate::selection::SelectionRange;
use std::path::Path;
use tempfile::tempdir;

#[test]
fn next_slice_path_in_dir_skips_existing_suffixes() {
    let temp = tempdir().unwrap();
    let root = temp.path();
    std::fs::write(root.join("clip_slice001.wav"), b"").unwrap();
    std::fs::write(root.join("clip_slice002.wav"), b"").unwrap();

    let renderer = crate::waveform::WaveformRenderer::new(12, 12);
    let mut controller = EguiController::new(renderer, None);
    let source = SampleSource::new(root.to_path_buf());
    controller.library.sources.push(source.clone());

    let mut counter = 1usize;
    let candidate = controller.next_slice_path_in_dir(&source, Path::new("clip.wav"), &mut counter);

    assert_eq!(candidate, Path::new("clip_slice003.wav"));
}

#[test]
fn accept_waveform_slices_exports_files() {
    let temp = tempdir().unwrap();
    let root = temp.path().join("source");
    std::fs::create_dir_all(&root).unwrap();
    let renderer = crate::waveform::WaveformRenderer::new(12, 12);
    let mut controller = EguiController::new(renderer, None);
    let source = SampleSource::new(root.clone());
    controller.library.sources.push(source.clone());
    controller.cache_db(&source).unwrap();

    let wav_path = root.join("clip.wav");
    write_test_wav(&wav_path, &[0.2, 0.2, 0.0, 0.0, 0.3, 0.3]);
    controller
        .load_waveform_for_selection(&source, Path::new("clip.wav"))
        .unwrap();
    controller.ui.waveform.slices = vec![
        SelectionRange::new(0.0, 0.5),
        SelectionRange::new(0.5, 1.0),
    ];

    let count = controller.accept_waveform_slices().unwrap();

    assert_eq!(count, 2);
    assert!(root.join("clip_slice001.wav").exists());
    assert!(root.join("clip_slice002.wav").exists());
}

#[test]
fn detect_waveform_slices_uses_transients_when_enabled() {
    let temp = tempdir().unwrap();
    let root = temp.path().join("source");
    std::fs::create_dir_all(&root).unwrap();
    let renderer = crate::waveform::WaveformRenderer::new(12, 12);
    let mut controller = EguiController::new(renderer, None);
    let source = SampleSource::new(root.clone());
    controller.library.sources.push(source.clone());
    controller.cache_db(&source).unwrap();

    let wav_path = root.join("clip.wav");
    write_test_wav(&wav_path, &vec![0.5_f32; 8]);
    controller
        .load_waveform_for_selection(&source, Path::new("clip.wav"))
        .unwrap();
    controller.ui.waveform.transient_markers_enabled = true;
    controller.ui.waveform.transient_snap_enabled = true;
    controller.ui.waveform.transients = vec![0.25, 0.5, 0.75];

    let count = controller.detect_waveform_slices_from_silence().unwrap();

    assert_eq!(count, 4);
    assert_eq!(controller.ui.waveform.slices.len(), 4);
    let first = controller.ui.waveform.slices[0];
    let last = controller.ui.waveform.slices[3];
    assert!((first.start() - 0.0).abs() < 1.0e-6);
    assert!((last.end() - 1.0).abs() < 1.0e-6);
}
