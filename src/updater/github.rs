use serde::Deserialize;

use super::{UpdateError, UpdateChannel};

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

pub(super) fn fetch_release(repo: &str, channel: UpdateChannel) -> Result<Release, UpdateError> {
    let url = match channel {
        UpdateChannel::Stable => format!("https://api.github.com/repos/{repo}/releases/latest"),
        UpdateChannel::Nightly => format!("https://api.github.com/repos/{repo}/releases/tags/nightly"),
    };
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
