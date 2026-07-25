//! Worker-side selection-export registration and naming helpers.

use super::helpers::{fast_content_hash, next_selection_path_in_dir};
use crate::app::controller::jobs::{
    SelectionClipDestination, SelectionExportSnapshot, SelectionSliceBatchExportSnapshot,
};
use crate::app::controller::library::analysis_jobs;
use crate::sample_sources::{Rating, SampleSource, SourceDatabase, WavEntry};
use rusqlite::params;
use std::fs;
use std::path::{Path, PathBuf};
use wavecrate_library::timestamps::system_time_to_unix_nanos;

/// Resolve the next clip-export path for one background worker request.
pub(super) fn next_clip_export_path(
    snapshot: &SelectionExportSnapshot,
    destination: &SelectionClipDestination,
) -> PathBuf {
    match destination {
        SelectionClipDestination::Browser {
            folder_override: Some(folder),
            ..
        } => next_selection_path_in_dir(
            &snapshot.source_root,
            &folder.join(file_name_hint(snapshot)),
        ),
        SelectionClipDestination::Folder {
            source_root,
            folder,
            ..
        } => next_selection_path_in_dir(source_root, &folder.join(file_name_hint(snapshot))),
        SelectionClipDestination::Browser { .. } | SelectionClipDestination::ExternalDrag => {
            next_selection_path_in_dir(&snapshot.source_root, &snapshot.relative_path)
        }
    }
}

/// Register one newly written selection clip in the source database.
pub(super) fn record_clip_entry(
    snapshot: &SelectionExportSnapshot,
    destination: &SelectionClipDestination,
    relative_path: PathBuf,
) -> Result<WavEntry, String> {
    let (source_root, source) = match destination {
        SelectionClipDestination::Folder {
            source_id,
            source_root,
            ..
        } => (
            source_root.as_path(),
            SampleSource::new_with_id(source_id.clone(), source_root.clone()),
        ),
        SelectionClipDestination::Browser { .. } | SelectionClipDestination::ExternalDrag => {
            (snapshot.source_root.as_path(), sample_source(snapshot))
        }
    };
    let entry = build_written_entry(
        source_root,
        relative_path,
        snapshot.target_tag.unwrap_or(Rating::NEUTRAL),
        snapshot.looped,
    )?;
    let db = SourceDatabase::open_for_background_job(&source.root)
        .map_err(|err| format!("Database unavailable: {err}"))?;
    db.upsert_file(&entry.relative_path, entry.file_size, entry.modified_ns)
        .map_err(|err| format!("Failed to register clip: {err}"))?;
    if entry.tag != Rating::NEUTRAL {
        db.set_tag(&entry.relative_path, entry.tag)
            .map_err(|err| format!("Failed to tag clip: {err}"))?;
    }
    if entry.looped {
        db.set_looped(&entry.relative_path, true)
            .map_err(|err| format!("Failed to mark clip as looped: {err}"))?;
    }
    if let Some(bpm) = snapshot.bpm.filter(|_| entry.looped) {
        persist_selection_bpm(&source, &entry, bpm)?;
    }
    Ok(entry)
}

/// Register one newly written crop-to-new-sample clip in the source database.
pub(super) fn record_crop_entry(
    snapshot: &SelectionExportSnapshot,
    relative_path: PathBuf,
) -> Result<WavEntry, String> {
    let mut entry = build_written_entry(
        &snapshot.source_root,
        relative_path,
        snapshot.target_tag.unwrap_or(Rating::NEUTRAL),
        false,
    )?;
    entry.looped = false;
    entry.tag = snapshot.target_tag.unwrap_or(Rating::NEUTRAL);
    let source = sample_source(snapshot);
    let db = SourceDatabase::open_for_background_job(&source.root)
        .map_err(|err| format!("Database unavailable: {err}"))?;
    db.upsert_file(&entry.relative_path, entry.file_size, entry.modified_ns)
        .map_err(|err| format!("Failed to sync database entry: {err}"))?;
    db.set_tag(&entry.relative_path, entry.tag)
        .map_err(|err| format!("Failed to sync tag: {err}"))?;
    Ok(entry)
}

/// Register one newly written slice-batch clip in the source database.
pub(super) fn record_slice_batch_entry(
    snapshot: &SelectionSliceBatchExportSnapshot,
    relative_path: PathBuf,
) -> Result<WavEntry, String> {
    let entry = build_written_entry(
        &snapshot.source_root,
        relative_path,
        snapshot.target_tag.unwrap_or(Rating::NEUTRAL),
        false,
    )?;
    let source =
        SampleSource::new_with_id(snapshot.source_id.clone(), snapshot.source_root.clone());
    let db = SourceDatabase::open_for_background_job(&source.root)
        .map_err(|err| format!("Database unavailable: {err}"))?;
    db.upsert_file(&entry.relative_path, entry.file_size, entry.modified_ns)
        .map_err(|err| format!("Failed to register slice: {err}"))?;
    if entry.tag != Rating::NEUTRAL {
        db.set_tag(&entry.relative_path, entry.tag)
            .map_err(|err| format!("Failed to tag slice: {err}"))?;
    }
    Ok(entry)
}

fn build_written_entry(
    source_root: &Path,
    relative_path: PathBuf,
    tag: Rating,
    looped: bool,
) -> Result<WavEntry, String> {
    let metadata = fs::metadata(source_root.join(&relative_path))
        .map_err(|err| format!("Failed to read saved clip: {err}"))?;
    let modified_ns = system_time_to_unix_nanos(
        metadata
            .modified()
            .map_err(|err| format!("Missing modified time for clip: {err}"))?,
    );
    Ok(WavEntry {
        relative_path,
        file_size: metadata.len(),
        modified_ns,
        content_hash: None,
        tag,
        looped,
        sound_type: None,
        locked: false,
        missing: false,
        last_played_at: None,
        last_curated_at: None,
        user_tag: None,
        tag_named: false,
        normal_tags: Vec::new(),
    })
}

fn persist_selection_bpm(source: &SampleSource, entry: &WavEntry, bpm: f32) -> Result<(), String> {
    if !bpm.is_finite() || bpm <= 0.0 {
        return Ok(());
    }
    let size = i64::try_from(entry.file_size)
        .map_err(|_| "Clip size exceeds database limits".to_string())?;
    let content_hash = fast_content_hash(entry.file_size, entry.modified_ns);
    let conn = analysis_jobs::open_source_db(&source.root)
        .map_err(|err| format!("Failed to open analysis database: {err}"))?;
    let sample_id = analysis_jobs::build_sample_id(source.id.as_str(), &entry.relative_path);
    conn.execute(
        "INSERT INTO samples (sample_id, content_hash, size, mtime_ns, duration_seconds, sr_used, analysis_version, bpm)
         VALUES (?1, ?2, ?3, ?4, NULL, NULL, NULL, ?5)
         ON CONFLICT(sample_id) DO UPDATE SET
             content_hash = excluded.content_hash,
             size = excluded.size,
             mtime_ns = excluded.mtime_ns,
             bpm = excluded.bpm",
        params![
            sample_id,
            content_hash,
            size,
            entry.modified_ns,
            bpm as f64
        ],
    )
    .map_err(|err| format!("Failed to store clip BPM: {err}"))?;
    Ok(())
}

fn sample_source(snapshot: &SelectionExportSnapshot) -> SampleSource {
    SampleSource::new_with_id(snapshot.source_id.clone(), snapshot.source_root.clone())
}

fn file_name_hint(snapshot: &SelectionExportSnapshot) -> PathBuf {
    snapshot
        .relative_path
        .file_name()
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("selection.wav"))
}
