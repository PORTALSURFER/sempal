use std::{
    collections::HashMap,
    path::{Path, PathBuf},
    sync::atomic::AtomicBool,
};

use crate::sample_sources::db::{SourceWriteBatch, WavEntry};

use super::scan::{ChangedSample, ScanError, ScanMode, ScanStats};
use super::scan_fs::{FileFacts, compute_content_hash};

const QUICK_HASH_MAX_BYTES: u64 = 8 * 1024 * 1024;

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

pub(super) fn index_by_facts(
    existing: &HashMap<PathBuf, WavEntry>,
) -> HashMap<(u64, i64), Vec<PathBuf>> {
    let mut map: HashMap<(u64, i64), Vec<PathBuf>> = HashMap::new();
    for entry in existing.values() {
        map.entry((entry.file_size, entry.modified_ns))
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
    existing_by_facts: &mut HashMap<(u64, i64), Vec<PathBuf>>,
    stats: &mut ScanStats,
    root: &Path,
    mode: ScanMode,
    cancel: Option<&AtomicBool>,
) -> Result<(), ScanError> {
    let path = facts.relative.clone();
    let should_hash = should_compute_full_hash(mode, facts.size);
    match existing.remove(&path) {
        Some(entry) if entry.file_size == facts.size && entry.modified_ns == facts.modified_ns => {
            remove_from_hash_index(existing_by_hash, entry.content_hash.as_deref(), &path);
            remove_from_facts_index(existing_by_facts, entry.file_size, entry.modified_ns, &path);
            if entry.missing {
                batch.set_missing(&path, false)?;
            }
            if entry.content_hash.is_none() {
                if should_hash {
                    let absolute = root.join(&path);
                    let hash = compute_content_hash(&absolute, cancel)?;
                    batch.upsert_file_with_hash(&path, facts.size, facts.modified_ns, &hash)?;
                    stats.hashes_computed += 1;
                } else {
                    stats.hashes_pending += 1;
                }
            }
        }
        Some(entry) => {
            remove_from_hash_index(existing_by_hash, entry.content_hash.as_deref(), &path);
            remove_from_facts_index(existing_by_facts, entry.file_size, entry.modified_ns, &path);
            let absolute = root.join(&path);
            let previous_hash = entry.content_hash.as_deref();
            if should_hash {
                let hash = compute_content_hash(&absolute, cancel)?;
                batch.upsert_file_with_hash(&path, facts.size, facts.modified_ns, &hash)?;
                stats.hashes_computed += 1;
                if previous_hash != Some(hash.as_str()) {
                    stats.content_changed += 1;
                    stats.changed_samples.push(ChangedSample {
                        relative_path: path.clone(),
                        file_size: facts.size,
                        modified_ns: facts.modified_ns,
                        content_hash: hash,
                    });
                }
            } else {
                batch.upsert_file_without_hash(&path, facts.size, facts.modified_ns)?;
                stats.hashes_pending += 1;
            }
            stats.updated += 1;
        }
        None => {
            let absolute = root.join(&path);
            if should_hash {
                let hash = compute_content_hash(&absolute, cancel)?;
                if let Some(entry) =
                    take_rename_candidate(existing, existing_by_hash, existing_by_facts, &hash)
                {
                    apply_rename(batch, &path, &facts, &hash, entry)?;
                    stats.updated += 1;
                    stats.renames_reconciled += 1;
                    return Ok(());
                }
                batch.upsert_file_with_hash(&path, facts.size, facts.modified_ns, &hash)?;
                stats.added += 1;
                stats.content_changed += 1;
                stats.hashes_computed += 1;
                stats.changed_samples.push(ChangedSample {
                    relative_path: path.clone(),
                    file_size: facts.size,
                    modified_ns: facts.modified_ns,
                    content_hash: hash,
                });
            } else {
                if let Some(entry) = take_rename_candidate_by_facts(
                    existing,
                    existing_by_facts,
                    facts.size,
                    facts.modified_ns,
                ) {
                    remove_from_hash_index(
                        existing_by_hash,
                        entry.content_hash.as_deref(),
                        &entry.relative_path,
                    );
                    apply_rename_without_hash(batch, &path, &facts, entry)?;
                    stats.updated += 1;
                    stats.renames_reconciled += 1;
                    stats.hashes_pending += 1;
                    return Ok(());
                }
                batch.upsert_file_without_hash(&path, facts.size, facts.modified_ns)?;
                stats.added += 1;
                stats.hashes_pending += 1;
            }
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
    if entry.looped {
        batch.set_looped(new_path, entry.looped)?;
    }
    if let Some(last_played_at) = entry.last_played_at {
        batch.set_last_played_at(new_path, last_played_at)?;
    }
    Ok(())
}

fn apply_rename_without_hash(
    batch: &mut SourceWriteBatch<'_>,
    new_path: &Path,
    facts: &FileFacts,
    entry: WavEntry,
) -> Result<(), ScanError> {
    batch.remove_file(&entry.relative_path)?;
    batch.upsert_file_without_hash(new_path, facts.size, facts.modified_ns)?;
    batch.set_tag(new_path, entry.tag)?;
    if entry.looped {
        batch.set_looped(new_path, entry.looped)?;
    }
    if let Some(last_played_at) = entry.last_played_at {
        batch.set_last_played_at(new_path, last_played_at)?;
    }
    Ok(())
}

fn take_rename_candidate(
    existing: &mut HashMap<PathBuf, WavEntry>,
    existing_by_hash: &mut HashMap<String, Vec<PathBuf>>,
    existing_by_facts: &mut HashMap<(u64, i64), Vec<PathBuf>>,
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
    remove_from_facts_index(existing_by_facts, entry.file_size, entry.modified_ns, &path);
    Some(entry)
}

fn take_rename_candidate_by_facts(
    existing: &mut HashMap<PathBuf, WavEntry>,
    existing_by_facts: &mut HashMap<(u64, i64), Vec<PathBuf>>,
    size: u64,
    modified_ns: i64,
) -> Option<WavEntry> {
    let candidates = existing_by_facts.get(&(size, modified_ns))?;
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
    remove_from_facts_index(existing_by_facts, entry.file_size, entry.modified_ns, &path);
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

fn remove_from_facts_index(
    existing_by_facts: &mut HashMap<(u64, i64), Vec<PathBuf>>,
    size: u64,
    modified_ns: i64,
    path: &Path,
) {
    let key = (size, modified_ns);
    if let Some(paths) = existing_by_facts.get_mut(&key) {
        paths.retain(|candidate| candidate != path);
        if paths.is_empty() {
            existing_by_facts.remove(&key);
        }
    }
}

fn should_compute_full_hash(mode: ScanMode, size: u64) -> bool {
    match mode {
        ScanMode::Quick => size <= QUICK_HASH_MAX_BYTES,
        ScanMode::Hard => true,
    }
}
