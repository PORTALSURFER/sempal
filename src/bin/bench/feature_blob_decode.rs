use super::options::BenchOptions;
use serde::Serialize;
use std::time::Instant;

#[derive(Clone, Debug, Serialize)]
pub(super) struct FeatureBlobDecodeBenchResult {
    pub(super) blobs: usize,
    pub(super) feature_len_f32: usize,
    pub(super) total_elapsed_ms: u64,
    pub(super) blobs_per_sec: f64,
    pub(super) mb_per_sec: f64,
}

pub(super) fn run(options: &BenchOptions) -> Result<FeatureBlobDecodeBenchResult, String> {
    let blobs = options.query_rows.max(1);
    let feature_len_f32 = sempal::analysis::FEATURE_VECTOR_LEN_V1;
    let bytes_per_blob = feature_len_f32.saturating_mul(4);

    let mut payload = vec![0u8; bytes_per_blob];
    fill_deterministic_bytes(&mut payload, options.seed);

    for _ in 0..options.warmup_iters.max(1) {
        let _ = sempal::analysis::decode_f32_le_blob(&payload)?;
    }

    let started = Instant::now();
    let mut checksum = 0.0_f64;
    for _ in 0..options.measure_iters.max(1) {
        for _ in 0..blobs {
            let decoded = sempal::analysis::decode_f32_le_blob(&payload)?;
            checksum += decoded.get(0).copied().unwrap_or(0.0) as f64;
        }
    }
    let elapsed = started.elapsed();
    if checksum.is_nan() {
        return Err("Decode checksum is NaN".to_string());
    }

    let total_blobs = blobs.saturating_mul(options.measure_iters.max(1));
    let total_bytes = total_blobs.saturating_mul(bytes_per_blob);
    let blobs_per_sec = if elapsed.as_secs_f64() <= 0.0 {
        0.0
    } else {
        total_blobs as f64 / elapsed.as_secs_f64()
    };
    let mb_per_sec = if elapsed.as_secs_f64() <= 0.0 {
        0.0
    } else {
        (total_bytes as f64 / (1024.0 * 1024.0)) / elapsed.as_secs_f64()
    };
    Ok(FeatureBlobDecodeBenchResult {
        blobs: total_blobs,
        feature_len_f32,
        total_elapsed_ms: elapsed.as_millis() as u64,
        blobs_per_sec,
        mb_per_sec,
    })
}

fn fill_deterministic_bytes(buf: &mut [u8], seed: u64) {
    let mut state = seed ^ 0x9E37_79B9_7F4A_7C15;
    for byte in buf.iter_mut() {
        state ^= state >> 12;
        state ^= state << 25;
        state ^= state >> 27;
        let out = state.wrapping_mul(0x2545_F491_4F6C_DD1D);
        *byte = (out & 0xFF) as u8;
    }
}

