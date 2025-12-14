use super::*;
use hound::{SampleFormat, WavSpec, WavWriter};
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
        tag,
        missing: false,
    }
}

pub(super) fn write_test_wav(path: &Path, samples: &[f32]) {
    let spec = WavSpec {
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
