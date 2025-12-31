/// UI state for submitting feedback as a GitHub issue.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct FeedbackIssueUiState {
    pub open: bool,
    pub title: String,
    pub body: String,
    pub focus_title_requested: bool,
    pub token_modal_open: bool,
    pub token_input: String,
    pub focus_token_requested: bool,
    pub token_autofill_last: Option<String>,
    pub connecting: bool,
    pub submitting: bool,
    pub last_error: Option<String>,
    pub last_success_url: Option<String>,
}
