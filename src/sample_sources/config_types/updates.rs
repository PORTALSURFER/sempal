use serde::{Deserialize, Serialize};

use super::super::config_defaults::default_true;

/// Persisted preferences for update checks.
///
/// Config keys: `channel`, `check_on_startup`, `last_seen_nightly_published_at`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UpdateSettings {
    /// Selected update channel (stable or nightly).
    #[serde(default)]
    pub channel: UpdateChannel,
    /// Whether to check for updates on startup.
    #[serde(default = "default_true")]
    pub check_on_startup: bool,
    /// Timestamp of the latest nightly release seen by the user.
    #[serde(default)]
    pub last_seen_nightly_published_at: Option<String>,
}

impl Default for UpdateSettings {
    fn default() -> Self {
        Self {
            channel: UpdateChannel::Stable,
            check_on_startup: true,
            last_seen_nightly_published_at: None,
        }
    }
}

/// Update channel selection for GitHub releases.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum UpdateChannel {
    /// Receive stable releases only.
    Stable,
    /// Receive nightly/pre-release builds.
    Nightly,
}

impl Default for UpdateChannel {
    fn default() -> Self {
        Self::Stable
    }
}
