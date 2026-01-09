use std::{fs::File, io::Read, path::Path};

use ed25519_dalek::{Signature, VerifyingKey};
use sha2::{Digest, Sha256};

use base64::Engine;

use crate::http_client;

use super::{CHECKSUMS_PUBLIC_KEY_BASE64, UpdateError, github};

const MAX_CHECKSUM_BYTES: usize = 1024 * 1024;
const MAX_RELEASE_ASSET_BYTES: usize = 1024 * 1024 * 1024;
const MAX_SIGNATURE_BYTES: usize = 8 * 1024;
const MAX_ZIP_ENTRIES: usize = 10_000;
const MAX_ZIP_ENTRY_UNCOMPRESSED_BYTES: u64 = 512 * 1024 * 1024;
const MAX_ZIP_TOTAL_UNCOMPRESSED_BYTES: u64 = 2 * 1024 * 1024 * 1024;
const MAX_ZIP_COMPRESSION_RATIO: u64 = 200;

#[derive(Clone, Copy)]
struct ZipExtractionLimits {
    max_entries: usize,
    max_entry_uncompressed_bytes: u64,
    max_total_uncompressed_bytes: u64,
    max_compression_ratio: u64,
}

impl ZipExtractionLimits {
    fn standard() -> Self {
        Self {
            max_entries: MAX_ZIP_ENTRIES,
            max_entry_uncompressed_bytes: MAX_ZIP_ENTRY_UNCOMPRESSED_BYTES,
            max_total_uncompressed_bytes: MAX_ZIP_TOTAL_UNCOMPRESSED_BYTES,
            max_compression_ratio: MAX_ZIP_COMPRESSION_RATIO,
        }
    }
}

/// Download a small text asset into memory with a strict size cap.
pub(super) fn download_text(url: &str) -> Result<Vec<u8>, UpdateError> {
    let response = http_client::agent()
        .get(url)
        .set("User-Agent", "sempal-updater")
        .call()
        .map_err(|err| UpdateError::Http(err.to_string()))?;
    let bytes = http_client::read_response_bytes(response, MAX_CHECKSUM_BYTES)?;
    Ok(bytes)
}

/// Parse a checksums file and return the hash for the requested asset name.
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

/// Verify the checksums signature using the embedded public key.
pub(super) fn verify_checksums_signature(
    checksums: &[u8],
    signature_bytes: &[u8],
) -> Result<(), UpdateError> {
    verify_checksums_signature_with_key(checksums, signature_bytes, CHECKSUMS_PUBLIC_KEY_BASE64)
}

fn verify_checksums_signature_with_key(
    checksums: &[u8],
    signature_bytes: &[u8],
    public_key_base64: &str,
) -> Result<(), UpdateError> {
    let signature_text = std::str::from_utf8(signature_bytes)
        .map_err(|err| UpdateError::Invalid(format!("Invalid signature file: {err}")))?;
    let signature_text = signature_text.trim();
    if signature_text.is_empty() {
        return Err(UpdateError::Invalid("Empty signature file".into()));
    }
    if signature_text.len() > MAX_SIGNATURE_BYTES {
        return Err(UpdateError::Invalid("Signature file too large".into()));
    }
    let signature_raw = base64::engine::general_purpose::STANDARD
        .decode(signature_text)
        .map_err(|err| UpdateError::Invalid(format!("Invalid signature encoding: {err}")))?;
    let signature = Signature::from_slice(&signature_raw)
        .map_err(|err| UpdateError::Invalid(format!("Invalid signature length: {err}")))?;
    let public_key_raw = base64::engine::general_purpose::STANDARD
        .decode(public_key_base64)
        .map_err(|err| UpdateError::Invalid(format!("Invalid public key encoding: {err}")))?;
    let public_key = VerifyingKey::from_bytes(
        public_key_raw
            .as_slice()
            .try_into()
            .map_err(|_| UpdateError::Invalid("Invalid public key length".into()))?,
    )
    .map_err(|err| UpdateError::Invalid(format!("Invalid public key: {err}")))?;
    public_key
        .verify_strict(checksums, &signature)
        .map_err(|err| UpdateError::Invalid(format!("Checksums signature mismatch: {err}")))?;
    Ok(())
}

/// Download a release asset to disk with a hard size limit.
pub(super) fn download_to_file(url: &str, dest: &Path) -> Result<(), UpdateError> {
    let response = http_client::agent()
        .get(url)
        .set("User-Agent", "sempal-updater")
        .call()
        .map_err(|err| UpdateError::Http(err.to_string()))?;
    let mut file = File::create(dest)?;
    http_client::copy_response_to_writer(response, &mut file, MAX_RELEASE_ASSET_BYTES)?;
    Ok(())
}

/// Compute the SHA-256 hex digest for a local file.
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

/// Compare the on-disk zip checksum to the expected SHA-256 digest.
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

/// Extract a zip archive into a directory while enforcing safety limits.
pub(super) fn unzip_to_dir(zip_path: &Path, dest_dir: &Path) -> Result<(), UpdateError> {
    unzip_to_dir_with_limits(zip_path, dest_dir, ZipExtractionLimits::standard())
}

#[cfg(unix)]
fn safe_unix_file_mode(archive_mode: u32) -> u32 {
    let is_executable = archive_mode & 0o111 != 0;
    if is_executable {
        0o755
    } else {
        0o644
    }
}

fn unzip_to_dir_with_limits(
    zip_path: &Path,
    dest_dir: &Path,
    limits: ZipExtractionLimits,
) -> Result<(), UpdateError> {
    let file = File::open(zip_path)?;
    let mut archive =
        zip::ZipArchive::new(file).map_err(|err| UpdateError::Zip(err.to_string()))?;
    let entry_count = archive.len();
    if entry_count > limits.max_entries {
        return Err(UpdateError::Invalid(format!(
            "Archive has {entry_count} entries, limit is {}",
            limits.max_entries
        )));
    }
    let mut total_uncompressed: u64 = 0;
    for i in 0..entry_count {
        let mut entry = archive
            .by_index(i)
            .map_err(|err| UpdateError::Zip(err.to_string()))?;
        let uncompressed_size = entry.size();
        if uncompressed_size > limits.max_entry_uncompressed_bytes {
            return Err(UpdateError::Invalid(format!(
                "Archive entry '{}' is too large ({} bytes, limit {})",
                entry.name(),
                uncompressed_size,
                limits.max_entry_uncompressed_bytes
            )));
        }
        if uncompressed_size > 0 {
            let compressed_size = entry.compressed_size();
            if compressed_size == 0 {
                return Err(UpdateError::Invalid(format!(
                    "Archive entry '{}' has zero compressed size",
                    entry.name()
                )));
            }
            let max_uncompressed =
                compressed_size.saturating_mul(limits.max_compression_ratio);
            if uncompressed_size > max_uncompressed {
                return Err(UpdateError::Invalid(format!(
                    "Archive entry '{}' exceeds compression ratio limit",
                    entry.name()
                )));
            }
        }
        total_uncompressed = total_uncompressed
            .checked_add(uncompressed_size)
            .ok_or_else(|| UpdateError::Invalid("Archive size overflow".into()))?;
        if total_uncompressed > limits.max_total_uncompressed_bytes {
            return Err(UpdateError::Invalid(format!(
                "Archive extracted size {} exceeds limit {}",
                total_uncompressed, limits.max_total_uncompressed_bytes
            )));
        }
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
                let safe_mode = safe_unix_file_mode(mode);
                std::fs::set_permissions(&outpath, std::fs::Permissions::from_mode(safe_mode))?;
            }
        }
    }
    Ok(())
}

/// Download a named asset from a GitHub release to disk.
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

/// Download a named asset from a GitHub release into memory.
pub(super) fn download_release_asset_bytes(
    release: &github::Release,
    asset_name: &str,
) -> Result<Vec<u8>, UpdateError> {
    let asset = github::find_asset(release, asset_name)
        .ok_or_else(|| UpdateError::Invalid(format!("Missing release asset {asset_name}")))?;
    download_text(&asset.browser_download_url)
}

#[cfg(test)]
mod tests {
    use super::*;
    use ed25519_dalek::{Signer, SigningKey};
    use std::io::Write;
    #[cfg(unix)]
    use std::os::unix::fs::PermissionsExt;
    use tempfile::tempdir;

    fn write_zip(path: &Path, entries: &[(&str, &[u8])]) -> Result<(), UpdateError> {
        let file = File::create(path)?;
        let mut zip = zip::ZipWriter::new(file);
        let options = zip::write::SimpleFileOptions::default()
            .compression_method(zip::CompressionMethod::Deflated);
        for (name, data) in entries {
            zip.start_file(name, options)
                .map_err(|err| UpdateError::Zip(err.to_string()))?;
            zip.write_all(data)?;
        }
        zip.finish()
            .map_err(|err| UpdateError::Zip(err.to_string()))?;
        Ok(())
    }

    #[cfg(unix)]
    fn write_zip_with_modes(
        path: &Path,
        entries: &[(&str, &[u8], u32)],
    ) -> Result<(), UpdateError> {
        let file = File::create(path)?;
        let mut zip = zip::ZipWriter::new(file);
        for (name, data, mode) in entries {
            let options = zip::write::SimpleFileOptions::default()
                .compression_method(zip::CompressionMethod::Deflated)
                .unix_permissions(*mode);
            zip.start_file(name, options)
                .map_err(|err| UpdateError::Zip(err.to_string()))?;
            zip.write_all(data)?;
        }
        zip.finish()
            .map_err(|err| UpdateError::Zip(err.to_string()))?;
        Ok(())
    }

    #[test]
    fn verify_checksums_signature_rejects_invalid_signature() {
        let checksums = b"deadbeef  sempal.zip\n";
        let signing_key = SigningKey::from_bytes(&[7u8; 32]);
        let signature = signing_key.sign(b"other");
        let signature_text = base64::engine::general_purpose::STANDARD.encode(signature.to_bytes());
        let public_key_text = base64::engine::general_purpose::STANDARD
            .encode(signing_key.verifying_key().to_bytes());
        let err = verify_checksums_signature_with_key(
            checksums,
            signature_text.as_bytes(),
            &public_key_text,
        )
        .unwrap_err();
        assert!(err.to_string().contains("Checksums signature mismatch"));
    }

    #[test]
    fn verify_checksums_signature_rejects_empty_signature() {
        let checksums = b"deadbeef  sempal.zip\n";
        let signing_key = SigningKey::from_bytes(&[7u8; 32]);
        let public_key_text = base64::engine::general_purpose::STANDARD
            .encode(signing_key.verifying_key().to_bytes());
        let err =
            verify_checksums_signature_with_key(checksums, b"  \n", &public_key_text).unwrap_err();
        assert!(err.to_string().contains("Empty signature file"));
    }

    #[test]
    fn verify_checksums_signature_rejects_tampered_checksums() {
        let checksums = b"deadbeef  sempal.zip\n";
        let tampered = b"beefdead  sempal.zip\n";
        let signing_key = SigningKey::from_bytes(&[7u8; 32]);
        let signature = signing_key.sign(checksums);
        let signature_text = base64::engine::general_purpose::STANDARD.encode(signature.to_bytes());
        let public_key_text = base64::engine::general_purpose::STANDARD
            .encode(signing_key.verifying_key().to_bytes());
        let err = verify_checksums_signature_with_key(
            tampered,
            signature_text.as_bytes(),
            &public_key_text,
        )
        .unwrap_err();
        assert!(err.to_string().contains("Checksums signature mismatch"));
    }

    #[test]
    fn verify_checksums_signature_accepts_valid_signature() {
        let checksums = b"deadbeef  sempal.zip\n";
        let signing_key = SigningKey::from_bytes(&[7u8; 32]);
        let signature = signing_key.sign(checksums);
        let signature_text = base64::engine::general_purpose::STANDARD.encode(signature.to_bytes());
        let public_key_text = base64::engine::general_purpose::STANDARD
            .encode(signing_key.verifying_key().to_bytes());
        verify_checksums_signature_with_key(checksums, signature_text.as_bytes(), &public_key_text)
            .expect("signature should verify");
    }

    #[test]
    fn unzip_rejects_entry_over_size_limit() {
        let temp = tempdir().expect("tempdir");
        let zip_path = temp.path().join("oversize.zip");
        write_zip(&zip_path, &[("big.bin", &[1u8; 8])]).expect("zip write");
        let limits = ZipExtractionLimits {
            max_entries: 10,
            max_entry_uncompressed_bytes: 4,
            max_total_uncompressed_bytes: 100,
            max_compression_ratio: 100,
        };
        let err =
            unzip_to_dir_with_limits(&zip_path, temp.path().join("out").as_path(), limits)
                .unwrap_err();
        assert!(err.to_string().contains("too large"));
    }

    #[test]
    fn unzip_rejects_total_uncompressed_over_limit() {
        let temp = tempdir().expect("tempdir");
        let zip_path = temp.path().join("total.zip");
        write_zip(
            &zip_path,
            &[("a.txt", &[1u8; 6]), ("b.txt", &[2u8; 6])],
        )
        .expect("zip write");
        let limits = ZipExtractionLimits {
            max_entries: 10,
            max_entry_uncompressed_bytes: 10,
            max_total_uncompressed_bytes: 10,
            max_compression_ratio: 100,
        };
        let err =
            unzip_to_dir_with_limits(&zip_path, temp.path().join("out").as_path(), limits)
                .unwrap_err();
        assert!(err.to_string().contains("exceeds limit"));
    }

    #[test]
    fn unzip_rejects_suspicious_compression_ratio() {
        let temp = tempdir().expect("tempdir");
        let zip_path = temp.path().join("ratio.zip");
        let data = vec![b'a'; 2048];
        write_zip(&zip_path, &[("dense.txt", data.as_slice())]).expect("zip write");
        let limits = ZipExtractionLimits {
            max_entries: 10,
            max_entry_uncompressed_bytes: 10_000,
            max_total_uncompressed_bytes: 10_000,
            max_compression_ratio: 2,
        };
        let err =
            unzip_to_dir_with_limits(&zip_path, temp.path().join("out").as_path(), limits)
                .unwrap_err();
        assert!(err.to_string().contains("compression ratio"));
    }

    #[cfg(unix)]
    #[test]
    fn unzip_strips_unsafe_permissions() {
        let temp = tempdir().expect("tempdir");
        let zip_path = temp.path().join("perms.zip");
        write_zip_with_modes(
            &zip_path,
            &[
                ("bin/tool", b"run", 0o7777),
                ("data/config.toml", b"cfg", 0o6666),
            ],
        )
        .expect("zip write");
        unzip_to_dir(&zip_path, temp.path().join("out").as_path()).expect("unzip");
        let exec_mode = std::fs::metadata(temp.path().join("out/bin/tool"))
            .expect("exec metadata")
            .permissions()
            .mode()
            & 0o7777;
        let data_mode = std::fs::metadata(temp.path().join("out/data/config.toml"))
            .expect("data metadata")
            .permissions()
            .mode()
            & 0o7777;
        assert_eq!(exec_mode, 0o755);
        assert_eq!(data_mode, 0o644);
    }
}
