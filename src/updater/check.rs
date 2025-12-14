use semver::Version;

use super::github;
use super::{UpdateChannel, UpdateError};

/// Input for checking whether an update is available.
#[derive(Debug, Clone)]
pub struct UpdateCheckRequest {
    pub repo: String,
    pub channel: UpdateChannel,
    /// Current app version (stable channel only).
    pub current_version: Version,
    /// Last nightly release timestamp that was already shown to the user (RFC3339).
    pub last_seen_nightly_published_at: Option<String>,
}

/// Result of the update check used by the UI.
#[derive(Debug, Clone)]
pub enum UpdateCheckOutcome {
    UpToDate,
    UpdateAvailable {
        tag: String,
        html_url: String,
        published_at: Option<String>,
    },
}

pub(super) fn check_for_updates(
    request: UpdateCheckRequest,
) -> Result<UpdateCheckOutcome, UpdateError> {
    let release = github::fetch_release(&request.repo, request.channel)?;

    match request.channel {
        UpdateChannel::Stable => stable_outcome(&request.current_version, release),
        UpdateChannel::Nightly => nightly_outcome(&request.last_seen_nightly_published_at, release),
    }
}

fn stable_outcome(
    current: &Version,
    release: github::Release,
) -> Result<UpdateCheckOutcome, UpdateError> {
    let tag = release.tag_name.trim().to_string();
    let Some(version_text) = tag.strip_prefix('v') else {
        return Err(UpdateError::Invalid(format!(
            "Stable release tag must be 'v{{VERSION}}', got '{tag}'"
        )));
    };
    let latest = Version::parse(version_text).map_err(|err| {
        UpdateError::Invalid(format!("Invalid stable version '{version_text}': {err}"))
    })?;
    if &latest > current {
        Ok(UpdateCheckOutcome::UpdateAvailable {
            tag,
            html_url: release.html_url,
            published_at: release.published_at,
        })
    } else {
        Ok(UpdateCheckOutcome::UpToDate)
    }
}

fn nightly_outcome(
    last_seen: &Option<String>,
    release: github::Release,
) -> Result<UpdateCheckOutcome, UpdateError> {
    let published_at = release.published_at.clone();
    let Some(published) = published_at.as_deref() else {
        return Ok(UpdateCheckOutcome::UpdateAvailable {
            tag: release.tag_name,
            html_url: release.html_url,
            published_at,
        });
    };

    let Some(last_seen) = last_seen.as_deref() else {
        return Ok(UpdateCheckOutcome::UpdateAvailable {
            tag: release.tag_name,
            html_url: release.html_url,
            published_at,
        });
    };

    if published > last_seen {
        Ok(UpdateCheckOutcome::UpdateAvailable {
            tag: release.tag_name,
            html_url: release.html_url,
            published_at,
        })
    } else {
        Ok(UpdateCheckOutcome::UpToDate)
    }
}
