/// UI state for submitting feedback as a GitHub issue.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct FeedbackIssueUiState {
    /// Whether the feedback panel is open.
    pub open: bool,
    /// Issue title input.
    pub title: String,
    /// Issue body input.
    pub body: String,
    /// Whether to focus the title field.
    pub focus_title_requested: bool,
    /// Whether the auth token modal is open.
    pub token_modal_open: bool,
    /// Token input string.
    pub token_input: String,
    /// Whether to focus the token input field.
    pub focus_token_requested: bool,
    /// Last autofilled token value.
    pub token_autofill_last: Option<String>,
    /// True while connecting to the auth flow.
    pub connecting: bool,
    /// True while submitting the issue.
    pub submitting: bool,
    /// Last error message, if any.
    pub last_error: Option<String>,
    /// URL of the last created issue.
    pub last_success_url: Option<String>,
}
