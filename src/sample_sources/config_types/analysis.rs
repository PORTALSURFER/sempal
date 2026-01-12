use serde::{Deserialize, Serialize};

use super::super::config_defaults::{
    default_analysis_worker_count, default_false, default_fast_similarity_prep_sample_rate,
    default_max_analysis_duration_seconds, default_true,
};

/// Global preferences for analysis and feature extraction.
///
///   `limit_similarity_prep_duration`, `fast_similarity_prep`, `fast_similarity_prep_sample_rate`,
///   `wgpu_power_preference`, `wgpu_adapter_name`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AnalysisSettings {
    /// Skip analysis for files longer than this many seconds.
    #[serde(default = "default_max_analysis_duration_seconds")]
    pub max_analysis_duration_seconds: f32,
    /// Apply the duration cap when preparing similarity search.
    #[serde(default = "default_true")]
    pub limit_similarity_prep_duration: bool,
    /// Analysis worker count override (0 = auto).
    #[serde(default = "default_analysis_worker_count")]
    pub analysis_worker_count: u32,
    /// Use a faster, lower-quality analysis pass during similarity prep.
    #[serde(default = "default_false")]
    pub fast_similarity_prep: bool,
    /// Sample rate used during fast similarity prep analysis.
    #[serde(default = "default_fast_similarity_prep_sample_rate")]
    pub fast_similarity_prep_sample_rate: u32,
    /// WGPU adapter power preference when using the WGPU backend.
    #[serde(default)]
    pub wgpu_power_preference: WgpuPowerPreference,
    /// Optional WGPU adapter name override (substring match).
    #[serde(default)]
    pub wgpu_adapter_name: Option<String>,
}

impl Default for AnalysisSettings {
    fn default() -> Self {
        Self {
            max_analysis_duration_seconds: default_max_analysis_duration_seconds(),
            limit_similarity_prep_duration: default_true(),
            analysis_worker_count: default_analysis_worker_count(),
            fast_similarity_prep: default_false(),
            fast_similarity_prep_sample_rate: default_fast_similarity_prep_sample_rate(),
            wgpu_power_preference: WgpuPowerPreference::default(),
            wgpu_adapter_name: None,
        }
    }
}


/// WGPU adapter power preference.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum WgpuPowerPreference {
    /// Let the system decide the adapter power preference.
    Default,
    /// Prefer low-power adapters.
    Low,
    /// Prefer high-performance adapters.
    High,
}

impl Default for WgpuPowerPreference {
    fn default() -> Self {
        Self::Default
    }
}

impl WgpuPowerPreference {
    /// Return the environment variable value for this preference.
    pub fn as_env(&self) -> Option<&'static str> {
        match self {
            Self::Default => None,
            Self::Low => Some("low"),
            Self::High => Some("high"),
        }
    }

    /// Parse a power preference from an environment variable value.
    pub fn from_env(value: &str) -> Option<Self> {
        match value.trim().to_ascii_lowercase().as_str() {
            "low" | "low-power" | "lowpower" => Some(Self::Low),
            "high" | "high-performance" | "highperformance" => Some(Self::High),
            _ => None,
        }
    }
}
