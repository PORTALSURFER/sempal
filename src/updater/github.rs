use serde::Deserialize;
use std::time::Duration;
use time::OffsetDateTime;
use time::format_description::well_known::Rfc2822;

use crate::http_client;

use super::{
    RuntimeIdentity, UpdateChannel, UpdateError, expected_checksums_name,
    expected_checksums_signature_name, expected_zip_asset_name,
};

const MAX_RELEASE_JSON_BYTES: usize = 2 * 1024 * 1024;
const GITHUB_RETRY_CONFIG: http_client::RetryConfig = http_client::RetryConfig {
    max_attempts: 4,
    base_delay: Duration::from_millis(200),
    max_delay: Duration::from_secs(2),
};

#[derive(Debug, Clone, Deserialize)]
pub(super) struct ReleaseAsset {
    pub(super) name: String,
    pub(super) browser_download_url: String,
}

#[derive(Debug, Clone, Deserialize)]
pub(super) struct Release {
    pub(super) tag_name: String,
    #[allow(dead_code)]
    pub(super) prerelease: bool,
    pub(super) html_url: String,
    pub(super) published_at: Option<String>,
    pub(super) assets: Vec<ReleaseAsset>,
}

/// Public-facing release metadata for the updater UI.
#[derive(Debug, Clone)]
pub struct ReleaseSummary {
    /// Git tag name (e.g. `v0.384.0` or `nightly`).
    pub tag: String,
    /// HTML URL for the release page.
    pub html_url: String,
    /// Publication timestamp (RFC3339), if present.
    pub published_at: Option<String>,
}

pub(super) fn fetch_release_with_assets(
    repo: &str,
    channel: UpdateChannel,
    identity: &RuntimeIdentity,
) -> Result<Release, UpdateError> {
    let releases = fetch_releases(repo)?;
    select_release_with_assets(releases, channel, identity)
}

fn fetch_releases(repo: &str) -> Result<Vec<Release>, UpdateError> {
    let url = format!("https://api.github.com/repos/{repo}/releases?per_page=20");
    get_json(&url)
}

fn fetch_release_by_tag(repo: &str, tag: &str) -> Result<Release, UpdateError> {
    let url = format!("https://api.github.com/repos/{repo}/releases/tags/{tag}");
    get_json(&url)
}

fn get_json<T: for<'de> Deserialize<'de>>(url: &str) -> Result<T, UpdateError> {
    get_json_with_retry(url, GITHUB_RETRY_CONFIG)
}

fn get_json_with_retry<T: for<'de> Deserialize<'de>>(
    url: &str,
    retry_config: http_client::RetryConfig,
) -> Result<T, UpdateError> {
    get_json_with_retry_from(retry_config, || {
        http_client::agent()
            .get(url)
            .set("User-Agent", "sempal-updater")
            .set("Accept", "application/vnd.github+json")
            .call()
    })
}

fn get_json_with_retry_from<T: for<'de> Deserialize<'de>, F>(
    retry_config: http_client::RetryConfig,
    mut request: F,
) -> Result<T, UpdateError>
where
    F: FnMut() -> Result<ureq::Response, ureq::Error>,
{
    let mut attempt = 0usize;
    loop {
        attempt += 1;
        match request() {
            Ok(response) => {
                let bytes = http_client::read_response_bytes(response, MAX_RELEASE_JSON_BYTES)
                    .map_err(|err| UpdateError::Http(err.to_string()))?;
                let parsed = serde_json::from_slice(&bytes)?;
                return Ok(parsed);
            }
            Err(err) => {
                let retryable = is_retryable_github_error(&err);
                if attempt >= retry_config.max_attempts || !retryable {
                    return Err(map_github_error(err));
                }
                let delay = retry_delay_for_error(&err, retry_config, attempt);
                if delay > Duration::from_secs(0) {
                    std::thread::sleep(delay);
                }
            }
        }
    }
}

fn is_retryable_github_error(err: &ureq::Error) -> bool {
    match err {
        ureq::Error::Transport(_) => true,
        ureq::Error::Status(code, _) => *code == 429 || (500..=599).contains(code),
    }
}

fn retry_delay_for_error(
    err: &ureq::Error,
    config: http_client::RetryConfig,
    attempt: usize,
) -> Duration {
    let retry_after = retry_after_delay(err);
    match retry_after {
        Some(delay) => delay.min(config.max_delay),
        None => backoff_delay(config.base_delay, config.max_delay, attempt),
    }
}

fn retry_after_delay(err: &ureq::Error) -> Option<Duration> {
    let ureq::Error::Status(_, response) = err else {
        return None;
    };
    let header = response.header("Retry-After")?;
    parse_retry_after(header)
}

fn parse_retry_after(value: &str) -> Option<Duration> {
    let trimmed = value.trim();
    if let Ok(seconds) = trimmed.parse::<u64>() {
        return Some(Duration::from_secs(seconds));
    }
    let Ok(retry_at) = OffsetDateTime::parse(trimmed, &Rfc2822) else {
        return None;
    };
    let now = OffsetDateTime::now_utc();
    let delta = retry_at - now;
    if !delta.is_positive() {
        return Some(Duration::from_secs(0));
    }
    let secs = u64::try_from(delta.whole_seconds()).ok()?;
    let nanos = u32::try_from(delta.subsec_nanoseconds()).ok()?;
    Some(Duration::new(secs, nanos))
}

fn backoff_delay(base: Duration, max: Duration, attempt: usize) -> Duration {
    let exponent = u32::try_from(attempt.saturating_sub(1)).unwrap_or(u32::MAX);
    let factor = 1u32.checked_shl(exponent).unwrap_or(u32::MAX);
    let delay = base.checked_mul(factor).unwrap_or(max);
    if delay > max {
        max
    } else {
        delay
    }
}

fn map_github_error(err: ureq::Error) -> UpdateError {
    match err {
        ureq::Error::Status(code, _) => UpdateError::Http(format!("HTTP {code}")),
        ureq::Error::Transport(err) => UpdateError::Http(err.to_string()),
    }
}

pub(super) fn find_asset<'a>(release: &'a Release, name: &str) -> Option<&'a ReleaseAsset> {
    release.assets.iter().find(|asset| asset.name == name)
}

pub(super) fn list_releases_with_assets(
    repo: &str,
    channel: UpdateChannel,
    identity: &RuntimeIdentity,
    limit: usize,
) -> Result<Vec<ReleaseSummary>, UpdateError> {
    let releases = fetch_releases(repo)?;
    let mut matches = Vec::new();
    for release in releases.into_iter() {
        if channel == UpdateChannel::Stable && release.prerelease {
            continue;
        }
        match channel {
            UpdateChannel::Stable => {
                let Some(version_text) = release.tag_name.strip_prefix('v') else {
                    continue;
                };
                let zip_name = expected_zip_asset_name(identity, Some(version_text))?;
                let checksums_name = expected_checksums_name(identity, Some(version_text))?;
                let sig_name = expected_checksums_signature_name(identity, Some(version_text))?;
                if has_assets(&release, &[zip_name, checksums_name, sig_name]) {
                    matches.push(ReleaseSummary {
                        tag: release.tag_name,
                        html_url: release.html_url,
                        published_at: release.published_at,
                    });
                }
            }
            UpdateChannel::Nightly => {
                if release.tag_name != "nightly" {
                    continue;
                }
                let zip_name = expected_zip_asset_name(identity, None)?;
                let checksums_name = expected_checksums_name(identity, None)?;
                let sig_name = expected_checksums_signature_name(identity, None)?;
                if has_assets(&release, &[zip_name, checksums_name, sig_name]) {
                    matches.push(ReleaseSummary {
                        tag: release.tag_name,
                        html_url: release.html_url,
                        published_at: release.published_at,
                    });
                }
            }
        }
        if matches.len() >= limit {
            break;
        }
    }
    Ok(matches)
}

pub(super) fn fetch_release_by_tag_with_assets(
    repo: &str,
    tag: &str,
    channel: UpdateChannel,
    identity: &RuntimeIdentity,
) -> Result<Release, UpdateError> {
    let release = fetch_release_by_tag(repo, tag)?;
    let (zip_name, checksums_name, sig_name) = match channel {
        UpdateChannel::Stable => {
            let version_text = tag.strip_prefix('v').ok_or_else(|| {
                UpdateError::Invalid(format!("Stable tag must start with 'v', got '{tag}'"))
            })?;
            (
                expected_zip_asset_name(identity, Some(version_text))?,
                expected_checksums_name(identity, Some(version_text))?,
                expected_checksums_signature_name(identity, Some(version_text))?,
            )
        }
        UpdateChannel::Nightly => {
            if tag != "nightly" {
                return Err(UpdateError::Invalid(format!(
                    "Nightly tag must be 'nightly', got '{tag}'"
                )));
            }
            (
                expected_zip_asset_name(identity, None)?,
                expected_checksums_name(identity, None)?,
                expected_checksums_signature_name(identity, None)?,
            )
        }
    };
    if !has_assets(&release, &[zip_name, checksums_name, sig_name]) {
        return Err(UpdateError::Invalid(format!(
            "Release '{tag}' missing required assets"
        )));
    }
    Ok(release)
}

fn select_release_with_assets(
    releases: Vec<Release>,
    channel: UpdateChannel,
    identity: &RuntimeIdentity,
) -> Result<Release, UpdateError> {
    for release in releases.into_iter() {
        if channel == UpdateChannel::Stable && release.prerelease {
            continue;
        }
        match channel {
            UpdateChannel::Stable => {
                let Some(version_text) = release.tag_name.strip_prefix('v') else {
                    continue;
                };
                let zip_name = expected_zip_asset_name(identity, Some(version_text))?;
                let checksums_name = expected_checksums_name(identity, Some(version_text))?;
                let sig_name = expected_checksums_signature_name(identity, Some(version_text))?;
                if has_assets(&release, &[zip_name, checksums_name, sig_name]) {
                    return Ok(release);
                }
            }
            UpdateChannel::Nightly => {
                if release.tag_name != "nightly" {
                    continue;
                }
                let zip_name = expected_zip_asset_name(identity, None)?;
                let checksums_name = expected_checksums_name(identity, None)?;
                let sig_name = expected_checksums_signature_name(identity, None)?;
                if has_assets(&release, &[zip_name, checksums_name, sig_name]) {
                    return Ok(release);
                }
            }
        }
    }
    Err(UpdateError::Invalid(format!(
        "No {channel:?} release with required assets found"
    )))
}

fn has_assets(release: &Release, required: &[String]) -> bool {
    required
        .iter()
        .all(|name| find_asset(release, name).is_some())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicUsize, Ordering};

    #[test]
    fn parses_release_shape() {
        let json = r#"
        {
          "tag_name": "v0.1.0",
          "prerelease": false,
          "html_url": "https://example.invalid/release",
          "published_at": "2025-01-01T00:00:00Z",
          "assets": [
            { "name": "foo.zip", "browser_download_url": "https://example.invalid/foo.zip" }
          ]
        }"#;
        let parsed: Release = serde_json::from_str(json).unwrap();
        assert_eq!(parsed.tag_name, "v0.1.0");
        assert!(!parsed.prerelease);
        assert_eq!(parsed.assets.len(), 1);
        assert_eq!(parsed.assets[0].name, "foo.zip");
    }

    #[test]
    fn retries_on_transient_github_errors() {
        let body = r#"{"ok": true}"#;
        let responses = vec![
            "HTTP/1.1 500 Internal Server Error\r\nContent-Length: 0\r\n\r\n".to_string(),
            "HTTP/1.1 429 Too Many Requests\r\nRetry-After: 0\r\nContent-Length: 0\r\n\r\n"
                .to_string(),
            format!(
                "HTTP/1.1 200 OK\r\nContent-Length: {}\r\n\r\n{}",
                body.len(),
                body
            ),
        ];
        let attempts = AtomicUsize::new(0);
        let config = http_client::RetryConfig {
            max_attempts: 4,
            base_delay: Duration::from_millis(0),
            max_delay: Duration::from_millis(0),
        };

        let mut index = 0usize;
        let value: serde_json::Value = get_json_with_retry_from(config, || {
            let current = response_from_str(
                responses
                    .get(index)
                    .expect("response sequence exhausted"),
            );
            attempts.fetch_add(1, Ordering::SeqCst);
            index += 1;
            if index < 3 {
                Err(ureq::Error::Status(current.status(), current))
            } else {
                Ok(current)
            }
        })
        .unwrap();
        assert_eq!(value["ok"].as_bool(), Some(true));
        assert_eq!(attempts.load(Ordering::SeqCst), 3);
    }

    fn response_from_str(raw: &str) -> ureq::Response {
        raw.parse().expect("valid test response")
    }
}
