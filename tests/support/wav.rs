use std::path::Path;

pub fn write_test_wav(path: &Path, samples: &[f32]) {
    let spec = hound::WavSpec {
        channels: 1,
        sample_rate: 8,
        bits_per_sample: 32,
        sample_format: hound::SampleFormat::Float,
    };
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).expect("create wav parent dirs");
    }
    let mut writer = hound::WavWriter::create(path, spec).expect("create wav writer");
    for &sample in samples {
        writer.write_sample(sample).expect("write wav sample");
    }
    writer.finalize().expect("finalize wav");
}
