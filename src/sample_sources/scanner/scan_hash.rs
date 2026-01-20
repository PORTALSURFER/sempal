use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};

use crate::sample_sources::db::{SourceWriteBatch, WavEntry};
use crate::sample_sources::SourceDatabase;

use super::scan::{ScanError, ScanStats};
use super::scan_fs::{compute_content_hash, ensure_root_dir, read_facts};

pub(super) fn deep_hash_scan(
    db: &SourceDatabase,
    cancel: Option<&AtomicBool>,
) -> Result<ScanStats, ScanError> {
    let root = ensure_root_dir(db)?;
    let entries = db.list_files()?;
    let mut entries_by_path: HashMap<PathBuf, WavEntry> = entries
        .into_iter()
        .map(|entry| (entry.relative_path.clone(), entry))
        .collect();
    let mut stats = ScanStats::default();
    let mut batch = db.write_batch()?;
    let mut present_by_hash = HashMap::new();
    let mut missing_by_hash = HashMap::new();

    for entry in entries_by_path.values() {
        let Some(hash) = entry.content_hash.as_deref() else {
            continue;
        };
        let target_map = if entry.missing {
            &mut missing_by_hash
        } else {
            &mut present_by_hash
        };
        target_map
            .entry(hash.to_string())
            .or_insert_with(Vec::new)
            .push(entry.relative_path.clone());
    }

    for entry in entries_by_path.values_mut() {
        if let Some(cancel) = cancel
            && cancel.load(Ordering::Relaxed)
        {
            return Err(ScanError::Canceled);
        }
        if entry.missing || entry.content_hash.is_some() {
            continue;
        }
        let absolute = root.join(&entry.relative_path);
        if !absolute.exists() {
            continue;
        }
        let facts = read_facts(&root, &absolute)?;
        let hash = compute_content_hash(&absolute, cancel)?;
        batch.upsert_file_with_hash(&entry.relative_path, facts.size, facts.modified_ns, &hash)?;
        entry.file_size = facts.size;
        entry.modified_ns = facts.modified_ns;
        entry.content_hash = Some(hash.clone());
        present_by_hash
            .entry(hash)
            .or_insert_with(Vec::new)
            .push(entry.relative_path.clone());
        stats.hashes_computed += 1;
    }

    stats.renames_reconciled = reconcile_missing_renames(
        &mut batch,
        &entries_by_path,
        &present_by_hash,
        &missing_by_hash,
    )?;

    batch.commit()?;
    Ok(stats)
}

fn reconcile_missing_renames(
    batch: &mut SourceWriteBatch<'_>,
    entries_by_path: &HashMap<PathBuf, WavEntry>,
    present_by_hash: &HashMap<String, Vec<PathBuf>>,
    missing_by_hash: &HashMap<String, Vec<PathBuf>>,
) -> Result<usize, ScanError> {
    let mut reconciled = 0;
    for (hash, missing_paths) in missing_by_hash {
        if missing_paths.len() != 1 {
            continue;
        }
        let Some(present_paths) = present_by_hash.get(hash) else {
            continue;
        };
        if present_paths.len() != 1 {
            continue;
        }
        let missing_path = &missing_paths[0];
        let present_path = &present_paths[0];
        if missing_path == present_path {
            continue;
        }
        let Some(missing_entry) = entries_by_path.get(missing_path) else {
            continue;
        };
        let Some(present_entry) = entries_by_path.get(present_path) else {
            continue;
        };
        apply_deep_rename(batch, present_entry, missing_entry, hash)?;
        reconciled += 1;
    }
    Ok(reconciled)
}

fn apply_deep_rename(
    batch: &mut SourceWriteBatch<'_>,
    present_entry: &WavEntry,
    missing_entry: &WavEntry,
    hash: &str,
) -> Result<(), ScanError> {
    batch.remove_file(&missing_entry.relative_path)?;
    batch.upsert_file_with_hash_and_tag(
        &present_entry.relative_path,
        present_entry.file_size,
        present_entry.modified_ns,
        hash,
        missing_entry.tag,
        false,
    )?;
    if let Some(last_played_at) = missing_entry.last_played_at {
        batch.set_last_played_at(&present_entry.relative_path, last_played_at)?;
    }
    Ok(())
}
