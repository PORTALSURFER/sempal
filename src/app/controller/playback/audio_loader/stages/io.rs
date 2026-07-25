use super::super::{AudioLoadError, AudioLoadJob};
use super::IoStageOutput;
use crate::app::controller::playback::audio_cache::FileMetadata;
use std::{
    fs::{self, Metadata},
    io::Read,
    path::{Component, Path},
    sync::atomic::AtomicU64,
    time::Instant,
};
use wavecrate_library::timestamps::system_time_to_unix_nanos;

use super::super::telemetry::{
    StaleDropStage, audio_loader_telemetry_enabled, record_io_duration, record_read_bytes,
    record_request_stage, stage_timer, stale_and_record,
};

/// Chunk size for stale-aware incremental audio file reads.
pub(super) const AUDIO_LOADER_READ_CHUNK_BYTES: usize = 128 * 1024;

pub(super) fn load_metadata_stage(
    job: &AudioLoadJob,
    latest_request_id: &AtomicU64,
) -> Result<Option<FileMetadata>, AudioLoadError> {
    let started_at = stage_timer();
    ensure_safe_relative_path(&job.relative_path)?;
    if stale_and_record(job.request_id, latest_request_id, StaleDropStage::PreIo) {
        return Ok(None);
    }
    let full_path = job.root.join(&job.relative_path);
    let fs_metadata = fs::metadata(&full_path).map_err(|err| {
        let missing = err.kind() == std::io::ErrorKind::NotFound;
        if missing {
            AudioLoadError::Missing(format!("File missing: {} ({err})", full_path.display()))
        } else {
            AudioLoadError::Failed(format!(
                "Failed to read metadata for {}: {err}",
                full_path.display()
            ))
        }
    })?;
    let metadata = file_metadata(&full_path, &fs_metadata)?;
    record_request_stage(
        job.request_id,
        &job.source_id,
        &job.relative_path,
        "metadata_stage",
        started_at,
        Some(metadata.file_size),
        None,
        None,
    );
    Ok(Some(metadata))
}

/// Read metadata/bytes for one load request and sanitize bytes for decoding.
pub(super) fn load_io_stage(
    job: &AudioLoadJob,
    latest_request_id: &AtomicU64,
) -> Result<Option<IoStageOutput>, AudioLoadError> {
    let stage_started_at = stage_timer();
    ensure_safe_relative_path(&job.relative_path)?;
    if stale_and_record(job.request_id, latest_request_id, StaleDropStage::PreIo) {
        return Ok(None);
    }

    let io_start = audio_loader_telemetry_enabled().then(Instant::now);
    let full_path = job.root.join(&job.relative_path);
    let metadata_started_at = stage_timer();
    let fs_metadata = fs::metadata(&full_path).map_err(|err| {
        let missing = err.kind() == std::io::ErrorKind::NotFound;
        if missing {
            AudioLoadError::Missing(format!("File missing: {} ({err})", full_path.display()))
        } else {
            AudioLoadError::Failed(format!(
                "Failed to read metadata for {}: {err}",
                full_path.display()
            ))
        }
    })?;
    record_request_stage(
        job.request_id,
        &job.source_id,
        &job.relative_path,
        "io_metadata",
        metadata_started_at,
        Some(fs_metadata.len()),
        None,
        None,
    );

    let open_started_at = stage_timer();
    let file = fs::File::open(&full_path).map_err(|err| {
        let missing = err.kind() == std::io::ErrorKind::NotFound;
        if missing {
            AudioLoadError::Missing(format!("File missing: {} ({err})", full_path.display()))
        } else {
            AudioLoadError::Failed(format!("Failed to read {}: {err}", full_path.display()))
        }
    })?;
    record_request_stage(
        job.request_id,
        &job.source_id,
        &job.relative_path,
        "io_open",
        open_started_at,
        Some(fs_metadata.len()),
        None,
        None,
    );

    let reserve_len = usize::try_from(fs_metadata.len()).unwrap_or(0);
    let read_started_at = stage_timer();
    let bytes = read_bytes_chunked_with_stale_check(file, reserve_len, || {
        stale_and_record(job.request_id, latest_request_id, StaleDropStage::PostIo)
    })
    .map_err(|err| {
        AudioLoadError::Failed(format!("Failed to read {}: {err}", full_path.display()))
    })?;
    let Some(bytes) = bytes else {
        return Ok(None);
    };
    record_request_stage(
        job.request_id,
        &job.source_id,
        &job.relative_path,
        "io_read",
        read_started_at,
        Some(fs_metadata.len()),
        Some(bytes.len()),
        None,
    );

    record_read_bytes(bytes.len());
    let sanitize_started_at = stage_timer();
    let bytes = crate::wav_sanitize::sanitize_wav_bytes(bytes);
    record_request_stage(
        job.request_id,
        &job.source_id,
        &job.relative_path,
        "io_sanitize",
        sanitize_started_at,
        Some(fs_metadata.len()),
        Some(bytes.len()),
        None,
    );
    if let Some(start) = io_start {
        record_io_duration(start.elapsed());
    }

    if stale_and_record(job.request_id, latest_request_id, StaleDropStage::PostIo) {
        return Ok(None);
    }

    let metadata = file_metadata(&full_path, &fs_metadata)?;
    record_request_stage(
        job.request_id,
        &job.source_id,
        &job.relative_path,
        "io_stage",
        stage_started_at,
        Some(metadata.file_size),
        Some(bytes.len()),
        None,
    );
    Ok(Some(IoStageOutput { bytes, metadata }))
}

fn file_metadata(full_path: &Path, fs_metadata: &Metadata) -> Result<FileMetadata, AudioLoadError> {
    let modified_ns = system_time_to_unix_nanos(fs_metadata.modified().map_err(|err| {
        AudioLoadError::Failed(format!(
            "Missing modified time for {}: {err}",
            full_path.display()
        ))
    })?);
    Ok(FileMetadata {
        file_size: fs_metadata.len(),
        modified_ns,
    })
}

/// Read bytes incrementally and abort early when a stale request is detected.
pub(super) fn read_bytes_chunked_with_stale_check(
    mut reader: impl Read,
    reserve_len: usize,
    mut stale_check: impl FnMut() -> bool,
) -> std::io::Result<Option<Vec<u8>>> {
    let mut bytes = Vec::with_capacity(reserve_len);
    let mut chunk = [0u8; AUDIO_LOADER_READ_CHUNK_BYTES];
    loop {
        if stale_check() {
            return Ok(None);
        }
        let read = reader.read(&mut chunk)?;
        if read == 0 {
            break;
        }
        bytes.extend_from_slice(&chunk[..read]);
    }
    if stale_check() {
        return Ok(None);
    }
    Ok(Some(bytes))
}

/// Reject paths that escape the selected source root.
pub(super) fn ensure_safe_relative_path(path: &Path) -> Result<(), AudioLoadError> {
    let mut saw_component = false;
    for component in path.components() {
        match component {
            Component::CurDir => {}
            Component::Normal(_) => {
                saw_component = true;
            }
            Component::ParentDir | Component::RootDir | Component::Prefix(_) => {
                return Err(AudioLoadError::Failed(format!(
                    "Invalid relative path: {}",
                    path.display()
                )));
            }
        }
    }
    if !saw_component {
        return Err(AudioLoadError::Failed(format!(
            "Invalid relative path: {}",
            path.display()
        )));
    }
    Ok(())
}
