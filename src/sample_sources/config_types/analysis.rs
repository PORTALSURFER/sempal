use serde::{Deserialize, Serialize};

use super::super::config_defaults::{
    default_analysis_worker_count, default_false, default_fast_similarity_prep_sample_rate,
    default_long_sample_threshold_seconds, default_max_analysis_duration_seconds, default_true,
};

/// Global preferences for analysis and feature extraction.
///
///   `limit_similarity_prep_duration`, `long_sample_threshold_seconds`,
///   `fast_similarity_prep`, `fast_similarity_prep_sample_rate`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AnalysisSettings {
    /// Skip analysis for files longer than this many seconds.
    #[serde(default = "default_max_analysis_duration_seconds")]
    pub max_analysis_duration_seconds: f32,
    /// Apply the duration cap when preparing similarity search.
    #[serde(default = "default_true")]
    pub limit_similarity_prep_duration: bool,
    /// Threshold in seconds above which samples are marked as long in the browser.
    #[serde(default = "default_long_sample_threshold_seconds")]
    pub long_sample_threshold_seconds: f32,
    /// Analysis worker count override (0 = auto).
    #[serde(default = "default_analysis_worker_count")]
    pub analysis_worker_count: u32,
    /// Use a faster, lower-quality analysis pass during similarity prep.
    #[serde(default = "default_false")]
    pub fast_similarity_prep: bool,
    /// Sample rate used during fast similarity prep analysis.
    #[serde(default = "default_fast_similarity_prep_sample_rate")]
    pub fast_similarity_prep_sample_rate: u32,
}

impl Default for AnalysisSettings {
    fn default() -> Self {
        Self {
            max_analysis_duration_seconds: default_max_analysis_duration_seconds(),
            limit_similarity_prep_duration: default_true(),
            long_sample_threshold_seconds: default_long_sample_threshold_seconds(),
            analysis_worker_count: default_analysis_worker_count(),
            fast_similarity_prep: default_false(),
            fast_similarity_prep_sample_rate: default_fast_similarity_prep_sample_rate(),
        }
    }
}
