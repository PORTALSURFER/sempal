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
}
