use super::options::BenchOptions;
use rand::Rng;
use rand::SeedableRng;
use rand::rngs::StdRng;
use serde::Serialize;
use std::time::Instant;

#[derive(Clone, Debug, Serialize)]
pub(super) struct AnalysisBenchResult {
    pub(super) mode: String,
    pub(super) samples: usize,
    pub(super) total_elapsed_ms: u64,
    pub(super) mean_ms_per_sample: f64,
    pub(super) samples_per_sec: f64,
}

pub(super) fn run(options: &BenchOptions) -> Result<AnalysisBenchResult, String> {
    let mut rng = StdRng::seed_from_u64(options.seed);
    let started = Instant::now();
    for _ in 0..options.analysis_samples {
        let samples = synth_mono_samples(
            options.analysis_sample_rate,
            options.analysis_duration_ms,
            &mut rng,
        );
        let vec = sempal::analysis::compute_feature_vector_v1_for_mono_samples(
            &samples,
            options.analysis_sample_rate,
        )?;
        if vec.len() != sempal::analysis::FEATURE_VECTOR_LEN_V1 {
            source_err(vec.len())?;
        }
        if options.analysis_full {
            let embedding = sempal::analysis::compute_similarity_embedding_for_mono_samples(
                &samples,
                options.analysis_sample_rate,
            )?;
            if embedding.len() != sempal::analysis::similarity::SIMILARITY_DIM {
                return Err(format!("Unexpected embedding dim: {}", embedding.len()));
            }
        }
    }
    Ok(summarize(
        options.analysis_samples,
        started.elapsed(),
        options.analysis_full,
    ))
}

fn synth_mono_samples(sample_rate: u32, duration_ms: u32, rng: &mut StdRng) -> Vec<f32> {
    let samples = ((sample_rate as f64 * duration_ms as f64) / 1000.0)
        .round()
        .max(1.0) as usize;
    let freq = rng.random_range(55.0_f32..880.0_f32);
    let phase = rng.random_range(0.0_f32..1.0_f32) * std::f32::consts::TAU;
    let amp = rng.random_range(0.1_f32..0.9_f32);
    let mut out = Vec::with_capacity(samples);
    for i in 0..samples {
        let t = i as f32 / sample_rate as f32;
        let sample = (t * freq * std::f32::consts::TAU + phase).sin() * amp;
        out.push(sample.clamp(-1.0, 1.0));
    }
    out
}

fn summarize(
    samples: usize,
    elapsed: std::time::Duration,
    analysis_full: bool,
) -> AnalysisBenchResult {
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
        mode: if analysis_full {
            "features+embeddings".to_string()
        } else {
            "features".to_string()
        },
        samples,
        total_elapsed_ms,
        mean_ms_per_sample,
        samples_per_sec,
    }
}

fn source_err(len: usize) -> Result<(), String> {
    Err(format!("Unexpected feature vector length: {len}"))
}
