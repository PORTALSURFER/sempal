/// UI state for submitting feedback as a GitHub issue.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct FeedbackIssueUiState {
    pub open: bool,
    pub draft: String,
    pub focus_requested: bool,
    pub submitting: bool,
    pub last_error: Option<String>,
}

