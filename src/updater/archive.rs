use std::{fs::File, io::Read, path::Path};

use sha2::{Digest, Sha256};

use super::{UpdateError, github};

pub(super) fn download_text(url: &str) -> Result<Vec<u8>, UpdateError> {
    let response = ureq::get(url)
        .set("User-Agent", "sempal-updater")
        .call()
        .map_err(|err| UpdateError::Http(err.to_string()))?;
    let mut reader = response.into_reader();
    let mut bytes = Vec::new();
    reader.read_to_end(&mut bytes)?;
    Ok(bytes)
}

pub(super) fn parse_checksums_for_asset(
    checksums: &[u8],
    asset_name: &str,
) -> Result<String, UpdateError> {
    let text = std::str::from_utf8(checksums)
        .map_err(|err| UpdateError::Invalid(format!("Invalid checksums file: {err}")))?;
    for line in text.lines() {
        let line = line.trim();
        if line.is_empty() {
            continue;
        }
        let Some((hash, filename)) = line.split_once("  ") else {
            continue;
        };
        if filename.trim() == asset_name {
            return Ok(hash.trim().to_string());
        }
    }
    Err(UpdateError::Invalid(format!(
        "Checksums file did not include {asset_name}"
    )))
}

pub(super) fn download_to_file(url: &str, dest: &Path) -> Result<(), UpdateError> {
    let response = ureq::get(url)
        .set("User-Agent", "sempal-updater")
        .call()
        .map_err(|err| UpdateError::Http(err.to_string()))?;
    let mut reader = response.into_reader();
    let mut file = File::create(dest)?;
    std::io::copy(&mut reader, &mut file)?;
    Ok(())
}

pub(super) fn sha256_file(path: &Path) -> Result<String, UpdateError> {
    let mut file = File::open(path)?;
    let mut hasher = Sha256::new();
    let mut buf = [0u8; 64 * 1024];
    loop {
        let read = file.read(&mut buf)?;
        if read == 0 {
            break;
        }
        hasher.update(&buf[..read]);
    }
    Ok(format!("{:x}", hasher.finalize()))
}

pub(super) fn verify_zip_checksum(path: &Path, expected: &str) -> Result<(), UpdateError> {
    let actual = sha256_file(path)?;
    if actual != expected {
        return Err(UpdateError::ChecksumMismatch {
            filename: path
                .file_name()
                .and_then(|s| s.to_str())
                .unwrap_or("archive.zip")
                .to_string(),
            expected: expected.to_string(),
            actual,
        });
    }
    Ok(())
}

pub(super) fn unzip_to_dir(zip_path: &Path, dest_dir: &Path) -> Result<(), UpdateError> {
    let file = File::open(zip_path)?;
    let mut archive =
        zip::ZipArchive::new(file).map_err(|err| UpdateError::Zip(err.to_string()))?;
    for i in 0..archive.len() {
        let mut entry = archive
            .by_index(i)
            .map_err(|err| UpdateError::Zip(err.to_string()))?;
        let outpath = match entry.enclosed_name() {
            Some(path) => dest_dir.join(path),
            None => continue,
        };
        if entry.name().ends_with('/') {
            std::fs::create_dir_all(&outpath)?;
            continue;
        }
        if let Some(parent) = outpath.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let mut outfile = File::create(&outpath)?;
        std::io::copy(&mut entry, &mut outfile)?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            if let Some(mode) = entry.unix_mode() {
                std::fs::set_permissions(&outpath, std::fs::Permissions::from_mode(mode))?;
            }
        }
    }
    Ok(())
}

pub(super) fn download_release_asset(
    release: &github::Release,
    asset_name: &str,
    dest: &Path,
) -> Result<(), UpdateError> {
    let asset = github::find_asset(release, asset_name)
        .ok_or_else(|| UpdateError::Invalid(format!("Missing release asset {asset_name}")))?;
    download_to_file(&asset.browser_download_url, dest)?;
    Ok(())
}

pub(super) fn download_release_asset_bytes(
    release: &github::Release,
    asset_name: &str,
) -> Result<Vec<u8>, UpdateError> {
    let asset = github::find_asset(release, asset_name)
        .ok_or_else(|| UpdateError::Invalid(format!("Missing release asset {asset_name}")))?;
    download_text(&asset.browser_download_url)
}
