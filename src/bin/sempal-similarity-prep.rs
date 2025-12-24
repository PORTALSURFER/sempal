use sempal::egui_app::controller::EguiController;
use sempal::waveform::WaveformRenderer;
use std::path::PathBuf;
use std::time::{Duration, Instant};

struct Options {
    source: PathBuf,
    fast_mode: bool,
    fast_sample_rate: Option<u32>,
    duration_cap_seconds: Option<f32>,
    worker_count: Option<u32>,
    skip_finalize: bool,
    poll_ms: u64,
}

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let opts = match parse_args(&args) {
        Ok(opts) => opts,
        Err(err) => {
            eprintln!("{err}");
            std::process::exit(1);
        }
    };

    let renderer = WaveformRenderer::new(1, 1);
    let mut controller = EguiController::new(renderer, None);
    if let Err(err) = controller.load_configuration() {
        eprintln!("Failed to load config: {err}");
        std::process::exit(1);
    }

    if let Some(workers) = opts.worker_count {
        controller.set_analysis_worker_count(workers);
    }
    if let Some(seconds) = opts.duration_cap_seconds {
        controller.set_similarity_prep_duration_cap_enabled(true);
        controller.set_max_analysis_duration_seconds(seconds);
    }
    if opts.fast_mode {
        controller.set_similarity_prep_fast_mode_enabled(true);
    }
    if let Some(rate) = opts.fast_sample_rate {
        controller.set_similarity_prep_fast_sample_rate(rate);
    }

    let normalized = sempal::sample_sources::config::normalize_path(&opts.source);
    if !controller.select_source_by_root(&normalized) {
        if let Err(err) = controller.add_source_from_path(normalized.clone()) {
            eprintln!("Failed to add source: {err}");
            std::process::exit(1);
        }
    } else {
        controller.prepare_similarity_for_selected_source();
    }

    let started = Instant::now();
    let mut last_dot = Instant::now();
    let mut last_status = Instant::now();
    while controller.similarity_prep_in_progress() {
        controller.tick_playhead();
        if opts.skip_finalize && controller.similarity_prep_is_finalizing() {
            println!();
            println!("Finalizing started; exiting early due to --skip-finalize.");
            break;
        }
        if last_dot.elapsed() >= Duration::from_secs(1) {
            print!(".");
            let _ = std::io::Write::flush(&mut std::io::stdout());
            last_dot = Instant::now();
        }
        if last_status.elapsed() >= Duration::from_secs(5) {
            println!();
            println!("status: {}", controller.similarity_prep_debug_snapshot());
            last_status = Instant::now();
        }
        std::thread::sleep(Duration::from_millis(opts.poll_ms));
    }
    println!();

    println!(
        "Similarity prep completed in {:.2}s",
        started.elapsed().as_secs_f64()
    );
}

fn parse_args(args: &[String]) -> Result<Options, String> {
    let mut source: Option<PathBuf> = None;
    let mut fast_mode = false;
    let mut fast_sample_rate = None;
    let mut duration_cap_seconds = None;
    let mut worker_count = None;
    let mut skip_finalize = false;
    let mut poll_ms = 25_u64;

    let mut idx = 0usize;
    while idx < args.len() {
        match args[idx].as_str() {
            "-h" | "--help" => {
                return Err(help_text().to_string());
            }
            "--source" => {
                idx += 1;
                let value = args
                    .get(idx)
                    .ok_or_else(|| "--source requires a value".to_string())?;
                source = Some(PathBuf::from(value));
            }
            "--fast" => {
                fast_mode = true;
            }
            "--fast-sample-rate" => {
                idx += 1;
                let value = args
                    .get(idx)
                    .ok_or_else(|| "--fast-sample-rate requires a value".to_string())?;
                fast_sample_rate = Some(parse_u32(value, "--fast-sample-rate")?);
            }
            "--duration-cap-seconds" => {
                idx += 1;
                let value = args
                    .get(idx)
                    .ok_or_else(|| "--duration-cap-seconds requires a value".to_string())?;
                duration_cap_seconds = Some(parse_f32(value, "--duration-cap-seconds")?);
            }
            "--analysis-workers" => {
                idx += 1;
                let value = args
                    .get(idx)
                    .ok_or_else(|| "--analysis-workers requires a value".to_string())?;
                worker_count = Some(parse_u32(value, "--analysis-workers")?);
            }
            "--skip-finalize" => {
                skip_finalize = true;
            }
            "--poll-ms" => {
                idx += 1;
                let value = args
                    .get(idx)
                    .ok_or_else(|| "--poll-ms requires a value".to_string())?;
                poll_ms = parse_u64(value, "--poll-ms")?;
            }
            flag => {
                return Err(format!("Unknown argument: {flag}\n\n{}", help_text()));
            }
        }
        idx += 1;
    }

    let source = source.ok_or_else(|| format!("--source is required\n\n{}", help_text()))?;

    Ok(Options {
        source,
        fast_mode,
        fast_sample_rate,
        duration_cap_seconds,
        worker_count,
        skip_finalize,
        poll_ms,
    })
}

fn parse_u32(value: &str, flag: &str) -> Result<u32, String> {
    value
        .parse::<u32>()
        .map_err(|_| format!("Invalid {flag} value: {value}"))
}

fn parse_u64(value: &str, flag: &str) -> Result<u64, String> {
    value
        .parse::<u64>()
        .map_err(|_| format!("Invalid {flag} value: {value}"))
}

fn parse_f32(value: &str, flag: &str) -> Result<f32, String> {
    value
        .parse::<f32>()
        .map_err(|_| format!("Invalid {flag} value: {value}"))
}

fn help_text() -> &'static str {
    "Usage: sempal-similarity-prep --source <path> [options]\n\n\
Options:\n\
  --source <path>             Source folder to prepare\n\
  --fast                      Enable fast similarity prep mode\n\
  --fast-sample-rate <hz>     Sample rate for fast mode\n\
  --duration-cap-seconds <s>  Skip analysis beyond this duration\n\
  --analysis-workers <n>      Override analysis worker count\n\
  --skip-finalize             Exit after analysis before UMAP/cluster finalize\n\
  --poll-ms <n>               Poll interval (default: 25)\n\
  -h, --help                  Show this help\n"
}
