use super::options::BenchOptions;
use serde::Serialize;

#[derive(Clone, Debug, Serialize)]
pub(super) struct BenchReport {
    pub(super) sempal_version: String,
    pub(super) os: String,
    pub(super) arch: String,
    pub(super) cpu_cores: usize,
    pub(super) total_elapsed_ms: u64,
    pub(super) params: BenchOptions,
    pub(super) system: SystemInfo,
    pub(super) analysis: Option<super::analysis_throughput::AnalysisBenchResult>,
    pub(super) similarity: Option<super::similarity_latency::SimilarityBenchResult>,
    pub(super) feature_blob_decode: Option<super::feature_blob_decode::FeatureBlobDecodeBenchResult>,
}

impl BenchReport {
    pub(super) fn new(params: BenchOptions, system: SystemInfo) -> Self {
        Self {
            sempal_version: env!("CARGO_PKG_VERSION").to_string(),
            os: std::env::consts::OS.to_string(),
            arch: std::env::consts::ARCH.to_string(),
            cpu_cores: std::thread::available_parallelism()
                .map(|n| n.get())
                .unwrap_or(1),
            total_elapsed_ms: 0,
            params,
            system,
            analysis: None,
            similarity: None,
            feature_blob_decode: None,
        }
    }
}

#[derive(Clone, Debug, Serialize)]
pub(super) struct SystemInfo {
    pub(super) cpu_brand: String,
    pub(super) memory_total_bytes: u64,
}

impl SystemInfo {
    pub(super) fn from_system(system: &sysinfo::System) -> Self {
        Self {
            cpu_brand: system
                .cpus()
                .first()
                .map(|cpu| cpu.brand().to_string())
                .unwrap_or_default(),
            memory_total_bytes: system.total_memory(),
        }
    }
}
