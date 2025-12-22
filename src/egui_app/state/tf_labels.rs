use std::collections::HashMap;

/// UI state for training-free label creation and editing.
#[derive(Clone, Debug)]
pub struct TfLabelsUiState {
    pub editor_open: bool,
    pub create_prompt: Option<TfLabelCreatePrompt>,
    pub aggregation_mode: crate::sample_sources::config::TfLabelAggregationMode,
    pub last_score_sample_id: Option<String>,
    pub last_score_mode: crate::sample_sources::config::TfLabelAggregationMode,
    pub last_scores: Vec<TfLabelScoreCache>,
    pub last_candidate_label_id: Option<String>,
    pub last_candidate_results: Vec<TfLabelCandidateCache>,
    pub auto_tag_prompt: Option<TfAutoTagPrompt>,
    pub calibration: Option<TfLabelCalibrationState>,
    pub coverage_stats: HashMap<String, crate::egui_app::controller::TfLabelCoverageStats>,
}

impl Default for TfLabelsUiState {
    fn default() -> Self {
        Self {
            editor_open: false,
            create_prompt: None,
            aggregation_mode: crate::sample_sources::config::TfLabelAggregationMode::MeanTopK,
            last_score_sample_id: None,
            last_score_mode: crate::sample_sources::config::TfLabelAggregationMode::MeanTopK,
            last_scores: Vec::new(),
            last_candidate_label_id: None,
            last_candidate_results: Vec::new(),
            auto_tag_prompt: None,
            calibration: None,
            coverage_stats: HashMap::new(),
        }
    }
}

/// Modal prompt for creating a training-free label.
#[derive(Clone, Debug)]
pub struct TfLabelCreatePrompt {
    pub name: String,
    pub threshold: f32,
    pub gap: f32,
    pub topk: i64,
    pub anchor_sample_id: Option<String>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct TfLabelScoreCache {
    pub label_id: String,
    pub name: String,
    pub score: f32,
    pub bucket: crate::analysis::anchor_scoring::ConfidenceBucket,
    pub gap: f32,
    pub anchor_count: usize,
}

#[derive(Clone, Debug, PartialEq)]
pub struct TfLabelCandidateCache {
    pub sample_id: String,
    pub score: f32,
    pub bucket: crate::analysis::anchor_scoring::ConfidenceBucket,
}

#[derive(Clone, Debug, PartialEq)]
pub struct TfAutoTagPrompt {
    pub label_id: String,
    pub label_name: String,
}

#[derive(Clone, Debug, PartialEq)]
pub struct TfLabelCalibrationSample {
    pub sample_id: String,
    pub score: f32,
    pub bucket: crate::analysis::anchor_scoring::ConfidenceBucket,
}

#[derive(Clone, Debug)]
pub struct TfLabelCalibrationState {
    pub label_id: String,
    pub label_name: String,
    pub samples: Vec<TfLabelCalibrationSample>,
    pub decisions: HashMap<String, bool>,
    pub suggested_threshold: Option<f32>,
    pub suggested_gap: Option<f32>,
    pub suggested_topk: Option<i64>,
}
