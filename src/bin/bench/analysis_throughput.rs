use super::options::BenchOptions;
use rand::Rng;
use rand::SeedableRng;
use rand::rngs::StdRng;
use serde::Serialize;
use std::path::{Path, PathBuf};
use std::time::Instant;

#[derive(Clone, Debug, Serialize)]
pub(super) struct AnalysisBenchResult {
    pub(super) samples: usize,
    pub(super) total_elapsed_ms: u64,
    pub(super) mean_ms_per_sample: f64,
    pub(super) samples_per_sec: f64,
}

pub(super) fn run(options: &BenchOptions) -> Result<AnalysisBenchResult, String> {
    let dir = tempfile::tempdir().map_err(|err| format!("Create temp dir failed: {err}"))?;
    let audio_dir = dir.path().join("audio");
    std::fs::create_dir_all(&audio_dir)
        .map_err(|err| format!("Create temp audio dir failed: {err}"))?;
    write_fixtures(options, &audio_dir)?;

    let paths = sorted_wav_paths(&audio_dir)?;
    if let Some(path) = paths.first() {
        let _ = sempal::analysis::compute_feature_vector_v1_for_path(path)?;
    }

    let started = Instant::now();
    for path in paths {
        let vec = sempal::analysis::compute_feature_vector_v1_for_path(&path)?;
        if vec.len() != sempal::analysis::FEATURE_VECTOR_LEN_V1 {
            source_err(&path, vec.len())?;
        }
    }
    Ok(summarize(options.analysis_samples, started.elapsed()))
}

fn write_fixtures(options: &BenchOptions, audio_dir: &Path) -> Result<(), String> {
    let mut rng = StdRng::seed_from_u64(options.seed);
    for i in 0..options.analysis_samples {
        write_synth_wav(
            &audio_dir.join(format!("{i:06}.wav")),
            options.analysis_sample_rate,
            options.analysis_duration_ms,
            &mut rng,
        )?;
    }
    Ok(())
}

fn sorted_wav_paths(dir: &Path) -> Result<Vec<PathBuf>, String> {
    let mut paths = Vec::new();
    for entry in std::fs::read_dir(dir).map_err(|err| format!("Read dir failed: {err}"))? {
        let entry = entry.map_err(|err| format!("Read dir entry failed: {err}"))?;
        let path = entry.path();
        if path.extension().and_then(|ext| ext.to_str()) == Some("wav") {
            paths.push(path);
        }
    }
    paths.sort();
    Ok(paths)
}

fn write_synth_wav(
    path: &Path,
    sample_rate: u32,
    duration_ms: u32,
    rng: &mut StdRng,
) -> Result<(), String> {
    let spec = hound::WavSpec {
        channels: 1,
        sample_rate,
        bits_per_sample: 16,
        sample_format: hound::SampleFormat::Int,
    };
    let mut writer =
        hound::WavWriter::create(path, spec).map_err(|err| format!("Create WAV failed: {err}"))?;
    let samples = ((sample_rate as f64 * duration_ms as f64) / 1000.0)
        .round()
        .max(1.0) as usize;
    let freq = rng.random_range(55.0_f32..880.0_f32);
    let phase = rng.random_range(0.0_f32..1.0_f32) * std::f32::consts::TAU;
    let amp = rng.random_range(0.1_f32..0.9_f32);
    for i in 0..samples {
        let t = i as f32 / sample_rate as f32;
        let sample = (t * freq * std::f32::consts::TAU + phase).sin() * amp;
        let sample_i16 = (sample.clamp(-1.0, 1.0) * i16::MAX as f32) as i16;
        writer
            .write_sample(sample_i16)
            .map_err(|err| format!("Write WAV sample failed: {err}"))?;
    }
    writer
        .finalize()
        .map_err(|err| format!("Finalize WAV failed: {err}"))?;
    Ok(())
}

fn summarize(samples: usize, elapsed: std::time::Duration) -> AnalysisBenchResult {
    let total_elapsed_ms = elapsed.as_millis() as u64;
    let mean_ms_per_sample = if samples == 0 {
        0.0
    } else {
        elapsed.as_secs_f64() * 1000.0 / samples as f64
    };
    let samples_per_sec = if elapsed.as_secs_f64() <= 0.0 {
        0.0
    } else {
        samples as f64 / elapsed.as_secs_f64()
    };
    AnalysisBenchResult {
        samples,
        total_elapsed_ms,
        mean_ms_per_sample,
        samples_per_sec,
    }
}

fn source_err(path: &Path, len: usize) -> Result<(), String> {
    Err(format!(
        "Unexpected feature vector length for {}: {len}",
        path.display()
    ))
}
