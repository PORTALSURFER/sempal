use std::{
    collections::HashMap,
    path::{Path, PathBuf},
    sync::atomic::AtomicBool,
};

use crate::sample_sources::db::{SourceWriteBatch, WavEntry};

use super::scan::{ChangedSample, ScanError, ScanMode, ScanStats};
use super::scan_fs::{FileFacts, compute_content_hash};

pub(super) fn index_by_hash(
    existing: &HashMap<PathBuf, WavEntry>,
) -> HashMap<String, Vec<PathBuf>> {
    let mut map: HashMap<String, Vec<PathBuf>> = HashMap::new();
    for entry in existing.values() {
        let Some(hash) = entry.content_hash.as_deref() else {
            continue;
        };
        map.entry(hash.to_string())
            .or_default()
            .push(entry.relative_path.clone());
    }
    map
}

pub(super) fn apply_diff(
    batch: &mut SourceWriteBatch<'_>,
    facts: FileFacts,
    existing: &mut HashMap<PathBuf, WavEntry>,
    existing_by_hash: &mut HashMap<String, Vec<PathBuf>>,
    stats: &mut ScanStats,
    root: &Path,
    cancel: Option<&AtomicBool>,
) -> Result<(), ScanError> {
    let path = facts.relative.clone();
    match existing.remove(&path) {
        Some(entry) if entry.file_size == facts.size && entry.modified_ns == facts.modified_ns => {
            remove_from_hash_index(existing_by_hash, entry.content_hash.as_deref(), &path);
            if entry.missing {
                batch.set_missing(&path, false)?;
            }
        }
        Some(entry) => {
            remove_from_hash_index(existing_by_hash, entry.content_hash.as_deref(), &path);
            let absolute = root.join(&path);
            let hash = compute_content_hash(&absolute, cancel)?;
            let previous_hash = entry.content_hash.as_deref();
            batch.upsert_file_with_hash(&path, facts.size, facts.modified_ns, &hash)?;
            if previous_hash != Some(hash.as_str()) {
                stats.content_changed += 1;
                stats.changed_samples.push(ChangedSample {
                    relative_path: path.clone(),
                    file_size: facts.size,
                    modified_ns: facts.modified_ns,
                    content_hash: hash,
                });
            }
            stats.updated += 1;
        }
        None => {
            let absolute = root.join(&path);
            let hash = compute_content_hash(&absolute, cancel)?;
            if let Some(entry) = take_rename_candidate(existing, existing_by_hash, &hash) {
                apply_rename(batch, &path, &facts, &hash, entry)?;
                stats.updated += 1;
                return Ok(());
            }
            batch.upsert_file_with_hash(&path, facts.size, facts.modified_ns, &hash)?;
            stats.added += 1;
            stats.content_changed += 1;
            stats.changed_samples.push(ChangedSample {
                relative_path: path.clone(),
                file_size: facts.size,
                modified_ns: facts.modified_ns,
                content_hash: hash,
            });
        }
    }
    Ok(())
}

pub(super) fn mark_missing(
    batch: &mut SourceWriteBatch<'_>,
    existing: HashMap<PathBuf, WavEntry>,
    stats: &mut ScanStats,
    mode: ScanMode,
) -> Result<(), ScanError> {
    for leftover in existing.values() {
        match mode {
            ScanMode::Quick => {
                if leftover.missing {
                    continue;
                }
                batch.set_missing(&leftover.relative_path, true)?;
                stats.missing += 1;
            }
            ScanMode::Hard => {
                batch.remove_file(&leftover.relative_path)?;
                stats.missing += 1;
            }
        }
    }
    Ok(())
}

fn apply_rename(
    batch: &mut SourceWriteBatch<'_>,
    new_path: &Path,
    facts: &FileFacts,
    hash: &str,
    entry: WavEntry,
) -> Result<(), ScanError> {
    batch.remove_file(&entry.relative_path)?;
    batch.upsert_file_with_hash_and_tag(
        new_path,
        facts.size,
        facts.modified_ns,
        hash,
        entry.tag,
        false,
    )?;
    if let Some(last_played_at) = entry.last_played_at {
        batch.set_last_played_at(new_path, last_played_at)?;
    }
    Ok(())
}

fn take_rename_candidate(
    existing: &mut HashMap<PathBuf, WavEntry>,
    existing_by_hash: &mut HashMap<String, Vec<PathBuf>>,
    hash: &str,
) -> Option<WavEntry> {
    let candidates = existing_by_hash.get(hash)?;
    let matching: Vec<PathBuf> = candidates
        .iter()
        .filter(|path| existing.contains_key(*path))
        .cloned()
        .collect();
    if matching.len() != 1 {
        return None;
    }
    let path = matching[0].clone();
    let entry = existing.remove(&path)?;
    remove_from_hash_index(existing_by_hash, entry.content_hash.as_deref(), &path);
    Some(entry)
}

fn remove_from_hash_index(
    existing_by_hash: &mut HashMap<String, Vec<PathBuf>>,
    hash: Option<&str>,
    path: &Path,
) {
    let Some(hash) = hash else {
        return;
    };
    if let Some(paths) = existing_by_hash.get_mut(hash) {
        paths.retain(|candidate| candidate != path);
        if paths.is_empty() {
            existing_by_hash.remove(hash);
        }
    }
}
