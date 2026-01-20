/// Token status for GitHub issue reporting.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum IssueTokenStatus {
    /// Token state has not been loaded yet.
    Unknown,
    /// Token is present and ready for use.
    Connected,
    /// Token is missing or has been removed.
    NotConnected,
    /// Token storage returned an error.
    Error(String),
}

impl Default for IssueTokenStatus {
    fn default() -> Self {
        Self::Unknown
    }
}

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
    /// True while loading the token from storage.
    pub token_loading: bool,
    /// True while persisting the token to storage.
    pub token_saving: bool,
    /// True while deleting the token from storage.
    pub token_deleting: bool,
    /// Current token status for UI messaging.
    pub token_status: IssueTokenStatus,
    /// Cached token value for authenticated requests.
    pub token_cached: Option<String>,
    /// Last error message, if any.
    pub last_error: Option<String>,
    /// URL of the last created issue.
    pub last_success_url: Option<String>,
}
