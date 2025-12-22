use serde::Serialize;
use std::path::{Path, PathBuf};

#[derive(Clone, Debug, Serialize)]
pub(super) struct BenchOptions {
    pub(super) out: PathBuf,
    pub(super) analysis: bool,
    pub(super) query: bool,
    pub(super) similarity: bool,
    pub(super) seed: u64,
    pub(super) warmup_iters: usize,
    pub(super) measure_iters: usize,
    pub(super) analysis_samples: usize,
    pub(super) analysis_duration_ms: u32,
    pub(super) analysis_sample_rate: u32,
    pub(super) query_rows: usize,
    pub(super) similarity_rows: usize,
}

pub(super) fn parse_args(args: Vec<String>) -> Result<Option<BenchOptions>, String> {
    let mut options = default_options();
    apply_args(&mut options, &args)?;
    if !options.analysis && !options.query && !options.similarity {
        return Err("Nothing to benchmark: enable --analysis, --query, or --similarity".to_string());
    }
    Ok(Some(options))
}

pub(super) fn write_output(path: &Path, payload: &[u8]) -> Result<(), String> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)
            .map_err(|err| format!("Create output dir {} failed: {err}", parent.display()))?;
    }
    std::fs::write(path, payload)
        .map_err(|err| format!("Write output {} failed: {err}", path.display()))?;
    Ok(())
}

fn default_options() -> BenchOptions {
    BenchOptions {
        out: PathBuf::from("bench.json"),
        analysis: true,
        query: true,
        similarity: true,
        seed: 1,
        warmup_iters: 5,
        measure_iters: 30,
        analysis_samples: 200,
        analysis_duration_ms: 500,
        analysis_sample_rate: 44_100,
        query_rows: 50_000,
        similarity_rows: 20_000,
    }
}

fn apply_args(options: &mut BenchOptions, args: &[String]) -> Result<(), String> {
    let mut idx = 0usize;
    while idx < args.len() {
        if apply_arg(options, args, &mut idx)? {
            return Ok(());
        }
        idx += 1;
    }
    Ok(())
}

fn apply_arg(options: &mut BenchOptions, args: &[String], idx: &mut usize) -> Result<bool, String> {
    let flag = args.get(*idx).map(String::as_str).unwrap_or_default();
    if flag == "-h" || flag == "--help" {
        println!("{}", help_text());
        return Ok(true);
    }
    if apply_toggle(options, flag) {
        return Ok(false);
    }
    if apply_value(options, args, idx, flag)? {
        return Ok(false);
    }
    Err(format!("Unknown argument: {flag}\n\n{}", help_text()))
}

fn apply_toggle(options: &mut BenchOptions, flag: &str) -> bool {
    match flag {
        "--analysis" => options.analysis = true,
        "--no-analysis" => options.analysis = false,
        "--query" => options.query = true,
        "--no-query" => options.query = false,
        "--similarity" => options.similarity = true,
        "--no-similarity" => options.similarity = false,
        _ => return false,
    }
    true
}

fn apply_value(
    options: &mut BenchOptions,
    args: &[String],
    idx: &mut usize,
    flag: &str,
) -> Result<bool, String> {
    match flag {
        "--out" => options.out = PathBuf::from(value_after(args, idx, "--out")?),
        "--seed" => options.seed = parse_u64(args, idx, "--seed")?,
        "--warmup-iters" => options.warmup_iters = parse_usize(args, idx, "--warmup-iters")?,
        "--measure-iters" => options.measure_iters = parse_usize(args, idx, "--measure-iters")?,
        "--analysis-samples" => {
            options.analysis_samples = parse_usize(args, idx, "--analysis-samples")?;
        }
        "--analysis-duration-ms" => {
            options.analysis_duration_ms = parse_u32(args, idx, "--analysis-duration-ms")?;
        }
        "--analysis-sample-rate" => {
            options.analysis_sample_rate = parse_u32(args, idx, "--analysis-sample-rate")?;
        }
        "--query-rows" => options.query_rows = parse_usize(args, idx, "--query-rows")?,
        "--similarity-rows" => {
            options.similarity_rows = parse_usize(args, idx, "--similarity-rows")?;
        }
        _ => return Ok(false),
    }
    Ok(true)
}

fn parse_u64(args: &[String], idx: &mut usize, flag: &str) -> Result<u64, String> {
    let value = value_after(args, idx, flag)?;
    value
        .parse::<u64>()
        .map_err(|_| format!("Invalid {flag} value: {value}"))
}

fn parse_u32(args: &[String], idx: &mut usize, flag: &str) -> Result<u32, String> {
    let value = value_after(args, idx, flag)?;
    value
        .parse::<u32>()
        .map_err(|_| format!("Invalid {flag} value: {value}"))
}

fn parse_usize(args: &[String], idx: &mut usize, flag: &str) -> Result<usize, String> {
    let value = value_after(args, idx, flag)?;
    value
        .parse::<usize>()
        .map_err(|_| format!("Invalid {flag} value: {value}"))
}

fn value_after<'a>(args: &'a [String], idx: &mut usize, flag: &str) -> Result<&'a str, String> {
    *idx += 1;
    let value = args.get(*idx).ok_or_else(|| format!("{flag} requires a value"))?;
    Ok(value)
}

fn help_text() -> &'static str {
    "Usage: sempal-bench [options]\n\n\
Options:\n\
  --out <path>                Output JSON path (default: bench.json)\n\
  --analysis / --no-analysis   Enable/disable analysis throughput bench (default: enabled)\n\
  --query / --no-query         Enable/disable SQLite query latency bench (default: enabled)\n\
  --similarity / --no-similarity Enable/disable ANN similarity bench (default: enabled)\n\
  --seed <u64>                 RNG seed for analysis fixtures (default: 1)\n\
  --warmup-iters <n>           Warmup iterations for each query (default: 5)\n\
  --measure-iters <n>          Measured iterations for each query (default: 30)\n\
  --analysis-samples <n>       Number of synthetic wavs to analyze (default: 200)\n\
  --analysis-duration-ms <ms>  Synthetic wav duration (default: 500)\n\
  --analysis-sample-rate <hz>  Synthetic wav sample rate (default: 44100)\n\
  --query-rows <n>             Seed rows for query benchmarks (default: 50000)\n\
  --similarity-rows <n>        Seed rows for similarity benchmark (default: 20000)\n\
  -h, --help                   Show this help\n"
}
