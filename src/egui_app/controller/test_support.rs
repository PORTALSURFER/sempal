use super::*;
use hound::{SampleFormat, wavspec, WavWriter};
use std::path::{Path, PathBuf};
use tempfile::tempdir;

pub(super) fn dummy_controller() -> (EguiController, SampleSource) {
    let renderer = WaveformRenderer::new(10, 10);
    let mut controller = EguiController::new(renderer, None);
    let dir = tempdir().unwrap();
    let root_dir = dir.path().to_path_buf();
    let root = root_dir.join("source");
    std::mem::forget(dir);
    std::fs::create_dir_all(&root).unwrap();
    let source = SampleSource::new(root);
    controller.selection_state.ctx.selected_source = Some(source.id.clone());
    (controller, source)
}

pub(super) fn sample_entry(name: &str, tag: SampleTag) -> WavEntry {
    WavEntry {
        relative_path: PathBuf::from(name),
        file_size: 0,
        modified_ns: 0,
        content_hash: None,
        tag,
        missing: false,
    }
}

pub(super) fn prepare_with_source_and_wav_entries(
    entries: Vec<WavEntry>,
) -> (EguiController, SampleSource) {
    let (mut controller, source) = dummy_controller();
    controller.library.sources.push(source.clone());
    controller.cache_db(&source).unwrap();
    controller.set_wav_entries_for_tests(entries);
    controller.rebuild_wav_lookup();
    controller.rebuild_browser_lists();
    (controller, source)
}

pub(super) fn load_waveform_selection(
    controller: &mut EguiController,
    source: &SampleSource,
    filename: &str,
    samples: &[f32],
    selection: SelectionRange,
) -> PathBuf {
    let wav_path = source.root.join(filename);
    write_test_wav(&wav_path, samples);
    controller.set_wav_entries_for_tests(vec![sample_entry(filename, SampleTag::Neutral)]);
    controller.rebuild_wav_lookup();
    controller.rebuild_browser_lists();
    controller
        .load_waveform_for_selection(source, Path::new(filename))
        .unwrap();
    controller.ui.waveform.selection = Some(selection);
    wav_path
}

pub(super) fn write_test_wav(path: &Path, samples: &[f32]) {
    let spec = wavspec {
        channels: 1,
        sample_rate: 8,
        bits_per_sample: 32,
        sample_format: SampleFormat::Float,
    };
    let mut writer = WavWriter::create(path, spec).unwrap();
    for sample in samples {
        writer.write_sample(*sample).unwrap();
    }
    writer.finalize().unwrap();
}
