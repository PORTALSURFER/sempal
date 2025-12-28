use serde::{Deserialize, Serialize};

use super::super::config_defaults::default_true;

/// Persisted preferences for update checks.
///
/// Config keys: `channel`, `check_on_startup`, `last_seen_nightly_published_at`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UpdateSettings {
    #[serde(default)]
    pub channel: UpdateChannel,
    #[serde(default = "default_true")]
    pub check_on_startup: bool,
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
    Stable,
    Nightly,
}

impl Default for UpdateChannel {
    fn default() -> Self {
        Self::Stable
    }
}
