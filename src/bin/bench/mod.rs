mod analysis_throughput;
mod feature_blob_decode;
mod options;
mod query_latency;
mod report;
mod similarity_latency;
mod stats;

use report::{BenchReport, SystemInfo};
use std::time::Instant;

pub(super) fn run(args: Vec<String>) -> Result<(), String> {
    let Some(options) = options::parse_args(args)? else {
        return Ok(());
    };
    let started_at = Instant::now();
    let system = sysinfo::System::new_all();

    let mut report = BenchReport::new(options, SystemInfo::from_system(&system));
    if report.params.analysis {
        report.analysis = Some(analysis_throughput::run(&report.params)?);
    }
    if report.params.query {
        report.query = Some(query_latency::run(&report.params)?);
    }
    if report.params.similarity {
        report.similarity = Some(similarity_latency::run(&report.params)?);
    }
    report.feature_blob_decode = Some(feature_blob_decode::run(&report.params)?);
    report.total_elapsed_ms = started_at.elapsed().as_millis() as u64;

    let json = serde_json::to_vec_pretty(&report)
        .map_err(|err| format!("Serialize JSON failed: {err}"))?;
    options::write_output(&report.params.out, &json)?;
    println!("Wrote {}", report.params.out.display());
    Ok(())
}
