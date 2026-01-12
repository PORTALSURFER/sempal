//! UI state for update checks and update notifications.

/// Status for the background update check.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum UpdateStatus {
    /// No update activity in progress.
    Idle,
    /// Update check in progress.
    Checking,
    /// A newer update is available.
    UpdateAvailable,
    /// Update check failed.
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
    /// Current update check status.
    pub status: UpdateStatus,
    /// Tag of the available release.
    pub available_tag: Option<String>,
    /// URL of the available release.
    pub available_url: Option<String>,
    /// Published timestamp of the available release.
    pub available_published_at: Option<String>,
    /// Last error message, if any.
    pub last_error: Option<String>,
    /// Nightly-only bookkeeping: timestamp (RFC3339) that the user last dismissed.
    pub last_seen_nightly_published_at: Option<String>,
}
