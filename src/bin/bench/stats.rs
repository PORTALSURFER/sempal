use super::options::BenchOptions;
use rusqlite::Error as SqlError;
use serde::Serialize;
use std::time::Instant;

#[derive(Clone, Debug, Serialize)]
pub(super) struct LatencySummary {
    pub(super) warmup_iters: usize,
    pub(super) measure_iters: usize,
    pub(super) min_us: u64,
    pub(super) p50_us: u64,
    pub(super) p95_us: u64,
    pub(super) max_us: u64,
    pub(super) mean_us: f64,
}

pub(super) fn bench_sql_query(
    options: &BenchOptions,
    mut f: impl FnMut() -> Result<(), SqlError>,
) -> Result<LatencySummary, String> {
    run_warmup(options.warmup_iters, &mut f)?;
    let samples_us = run_measure(options.measure_iters, &mut f)?;
    Ok(summarize(options.warmup_iters, options.measure_iters, samples_us))
}

fn run_warmup(warmup_iters: usize, f: &mut impl FnMut() -> Result<(), SqlError>) -> Result<(), String> {
    for _ in 0..warmup_iters.max(1) {
        f().map_err(|err| format!("Warmup query failed: {err}"))?;
    }
    Ok(())
}

fn run_measure(
    measure_iters: usize,
    f: &mut impl FnMut() -> Result<(), SqlError>,
) -> Result<Vec<u64>, String> {
    let mut samples_us = Vec::with_capacity(measure_iters.max(1));
    for _ in 0..measure_iters.max(1) {
        let started = Instant::now();
        f().map_err(|err| format!("Measured query failed: {err}"))?;
        samples_us.push(started.elapsed().as_micros() as u64);
    }
    samples_us.sort_unstable();
    Ok(samples_us)
}

fn summarize(warmup_iters: usize, measure_iters: usize, samples_us: Vec<u64>) -> LatencySummary {
    let min_us = *samples_us.first().unwrap_or(&0);
    let max_us = *samples_us.last().unwrap_or(&0);
    let p50_us = percentile(&samples_us, 0.50);
    let p95_us = percentile(&samples_us, 0.95);
    let mean_us = if samples_us.is_empty() {
        0.0
    } else {
        samples_us.iter().copied().map(|v| v as f64).sum::<f64>() / samples_us.len() as f64
    };
    LatencySummary {
        warmup_iters,
        measure_iters,
        min_us,
        p50_us,
        p95_us,
        max_us,
        mean_us,
    }
}

fn percentile(sorted: &[u64], p: f64) -> u64 {
    if sorted.is_empty() {
        return 0;
    }
    let idx = ((sorted.len() - 1) as f64 * p.clamp(0.0, 1.0)).round() as usize;
    sorted[idx.min(sorted.len() - 1)]
}

