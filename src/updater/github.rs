use serde::Deserialize;

use super::{
    RuntimeIdentity, UpdateChannel, UpdateError, expected_checksums_name, expected_zip_asset_name,
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

fn get_json<T: for<'de> Deserialize<'de>>(url: &str) -> Result<T, UpdateError> {
    let response = ureq::get(url)
        .set("User-Agent", "sempal-updater")
        .set("Accept", "application/vnd.github+json")
        .call()
        .map_err(|err| UpdateError::Http(err.to_string()))?;
    response
        .into_json::<T>()
        .map_err(|err| UpdateError::Http(err.to_string()))
}

pub(super) fn find_asset<'a>(release: &'a Release, name: &str) -> Option<&'a ReleaseAsset> {
    release.assets.iter().find(|asset| asset.name == name)
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
                if has_assets(&release, &[zip_name, checksums_name]) {
                    return Ok(release);
                }
            }
            UpdateChannel::Nightly => {
                if release.tag_name != "nightly" {
                    continue;
                }
                let zip_name = expected_zip_asset_name(identity, None)?;
                let checksums_name = expected_checksums_name(identity, None)?;
                if has_assets(&release, &[zip_name, checksums_name]) {
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
}
