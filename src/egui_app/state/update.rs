//! UI state for update checks and update notifications.

/// Status for the background update check.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum UpdateStatus {
    Idle,
    Checking,
    UpdateAvailable,
    Error,
}

impl Default for UpdateStatus {
    fn default() -> Self {
        Self::Idle
    }
}

/// UI state surfaced in the status bar when a newer release exists.
#[derive(Clone, Debug, Default)]
pub struct UpdateUiState {
    pub status: UpdateStatus,
    pub available_tag: Option<String>,
    pub available_url: Option<String>,
    pub available_published_at: Option<String>,
    pub last_error: Option<String>,
    /// Nightly-only bookkeeping: timestamp (RFC3339) that the user last dismissed.
    pub last_seen_nightly_published_at: Option<String>,
}
