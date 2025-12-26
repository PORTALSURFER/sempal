use super::super::test_support::{sample_entry, write_test_wav};
use super::super::*;
use hound::WavReader;
use std::path::Path;

pub(super) fn max_sample_amplitude(path: &Path) -> f32 {
    WavReader::open(path)
        .unwrap()
        .samples::<f32>()
        .map(|s| s.unwrap().abs())
        .fold(0.0, f32::max)
}

pub(super) fn prepare_browser_sample(
    controller: &mut EguiController,
    source: &SampleSource,
    name: &str,
) {
    controller.library.sources.push(source.clone());
    write_test_wav(&source.root.join(name), &[0.0, 0.1, -0.1]);
    controller.set_wav_entries_for_tests(vec![sample_entry(name, SampleTag::Neutral)]);
    controller.rebuild_browser_lists();
}

pub(super) fn visible_indices(controller: &EguiController) -> Vec<usize> {
    (0..controller.visible_browser_len())
        .filter_map(|row| controller.visible_browser_index(row))
        .collect()
}
