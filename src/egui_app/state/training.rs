/// Snapshot of the training data coverage for the current sources.
#[derive(Clone, Debug)]
pub struct TrainingSummary {
    pub updated_at: i64,
    pub sources: usize,
    pub samples_total: i64,
    pub features_v1: i64,
    pub user_labeled: i64,
    pub weak_labeled: i64,
    pub exportable: i64,
    pub predictions_total: Option<i64>,
    pub predictions_unknown: Option<i64>,
    pub min_confidence: f32,
}

/// UI state for the model training workflow.
#[derive(Clone, Debug, Default)]
pub struct TrainingUiState {
    pub panel_open: bool,
    pub summary: Option<TrainingSummary>,
    pub summary_error: Option<String>,
}
