//! Network download helpers for updater release assets.

use std::{fs::File, path::Path, time::Duration};

use crate::http_client;

use super::super::{UpdateError, github};

const MAX_CHECKSUM_BYTES: usize = 1024 * 1024;
const MAX_RELEASE_ASSET_BYTES: usize = 1024 * 1024 * 1024;
const DOWNLOAD_RETRY_CONFIG: http_client::RetryConfig = http_client::RetryConfig {
    max_attempts: 3,
    base_delay: Duration::from_millis(200),
    max_delay: Duration::from_secs(2),
};

/// Download a named asset from a GitHub release to disk.
pub(crate) fn download_release_asset(
    release: &github::Release,
    asset_name: &str,
    dest: &Path,
) -> Result<(), UpdateError> {
    let asset = github::find_asset(release, asset_name)
        .ok_or_else(|| UpdateError::Invalid(format!("Missing release asset {asset_name}")))?;
    download_to_file(&asset.browser_download_url, dest)
}

/// Download a named asset from a GitHub release into memory.
pub(crate) fn download_release_asset_bytes(
    release: &github::Release,
    asset_name: &str,
) -> Result<Vec<u8>, UpdateError> {
    let asset = github::find_asset(release, asset_name)
        .ok_or_else(|| UpdateError::Invalid(format!("Missing release asset {asset_name}")))?;
    download_text(&asset.browser_download_url)
}

fn download_text(url: &str) -> Result<Vec<u8>, UpdateError> {
    let response = get_with_retry(url)?;
    Ok(http_client::read_response_bytes(
        response,
        MAX_CHECKSUM_BYTES,
    )?)
}

fn download_to_file(url: &str, dest: &Path) -> Result<(), UpdateError> {
    let response = get_with_retry(url)?;
    let mut file = File::create(dest)?;
    http_client::copy_response_to_writer(response, &mut file, MAX_RELEASE_ASSET_BYTES)?;
    Ok(())
}

fn get_with_retry(url: &str) -> Result<ureq::Response, UpdateError> {
    let response = http_client::retry_with_backoff(
        DOWNLOAD_RETRY_CONFIG,
        || {
            http_client::agent()
                .get(url)
                .set("User-Agent", "wavecrate")
                .call()
        },
        |err| match err {
            ureq::Error::Transport(_) => true,
            ureq::Error::Status(code, _) => (500..=599).contains(code),
        },
    );
    match response {
        Ok(response) => Ok(response),
        Err(ureq::Error::Status(code, _)) => Err(UpdateError::Http(format!("HTTP {code}"))),
        Err(ureq::Error::Transport(err)) => Err(UpdateError::Http(err.to_string())),
    }
}
