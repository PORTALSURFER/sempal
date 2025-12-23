use std::time::{Duration, SystemTime, UNIX_EPOCH};

pub(super) fn library_db_path() -> Result<std::path::PathBuf, String> {
    let dir = crate::app_dirs::app_root_dir().map_err(|err| err.to_string())?;
    Ok(dir.join(crate::sample_sources::library::LIBRARY_DB_FILE_NAME))
}

pub(super) fn now_epoch_seconds() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_else(|_| Duration::from_secs(0))
        .as_secs() as i64
}

pub(super) fn fast_content_hash(size: u64, modified_ns: i64) -> String {
    format!("fast-{}-{}", size, modified_ns)
}
