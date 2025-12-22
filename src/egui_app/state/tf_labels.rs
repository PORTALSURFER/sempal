/// UI state for training-free label creation and editing.
#[derive(Clone, Debug)]
pub struct TfLabelsUiState {
    pub editor_open: bool,
    pub create_prompt: Option<TfLabelCreatePrompt>,
}

impl Default for TfLabelsUiState {
    fn default() -> Self {
        Self {
            editor_open: false,
            create_prompt: None,
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
