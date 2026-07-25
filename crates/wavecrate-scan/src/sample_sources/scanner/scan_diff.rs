use std::path::Path;

use crate::sample_sources::SourceDatabase;
use crate::sample_sources::db::{RenameMetadataSnapshot, SourceWriteBatch, WavEntry};
use wavecrate_library::sample_sources::SourceTraversalPolicy;

use super::scan::{
    ChangedSample, RenamedSample, ScanContext, ScanError, ScanMode, ScanStats, UpdatedSample,
};
use super::scan_fs::{FileFacts, is_supported_scannable_audio_file};

const QUICK_HASH_MAX_BYTES: u64 = 8 * 1024 * 1024;

pub(super) struct PreparedFile {
    pub(super) facts: FileFacts,
    pub(super) hash_required: bool,
    pub(super) needs_hash: bool,
    pub(super) requires_apply: bool,
    pub(super) revalidate_checkpoint: bool,
    pub(super) identity_replaced: bool,
    pub(super) content_hash: Option<String>,
    /// A descriptor opened through the source-root capability and retained through apply.
    pub(super) source_file: Option<std::fs::File>,
    pub(super) source_handle_verified: bool,
}

pub(super) fn apply_diff(
    db: &SourceDatabase,
    batch: &mut SourceWriteBatch<'_>,
    prepared: PreparedFile,
    context: &mut ScanContext,
) -> Result<(), ScanError> {
    let PreparedFile {
        facts,
        hash_required,
        needs_hash: _,
        requires_apply: _,
        revalidate_checkpoint,
        identity_replaced,
        content_hash,
        ..
    } = prepared;
    let path = facts.relative.clone();
    let should_hash = hash_required;
    let _ = context.existing.remove(&path);
    batch.remove_source_index_entry(&path)?;
    let existing = db.entry_for_path(&path)?;
    match existing {
        Some(entry)
            if context.mode != ScanMode::Targeted
                && entry.file_size == facts.size
                && entry.modified_ns == facts.modified_ns
                && !identity_replaced =>
        {
            if entry.missing {
                batch.set_missing(&path, false)?;
            }
            if should_hash && (entry.content_hash.is_none() || revalidate_checkpoint) {
                let hash = required_prepared_hash(content_hash);
                let hash_changed = entry.content_hash.as_deref() != Some(hash.as_str());
                if entry.content_hash.is_none() || hash_changed {
                    batch.upsert_file_with_hash(&path, facts.size, facts.modified_ns, &hash)?;
                }
                if hash_changed && entry.content_hash.is_some() {
                    context.stats.updated += 1;
                    context.stats.updated_samples.push(UpdatedSample {
                        relative_path: path.clone(),
                        file_size: facts.size,
                        modified_ns: facts.modified_ns,
                        content_hash: Some(hash.clone()),
                    });
                    context.stats.content_changed += 1;
                    context.stats.changed_samples.push(ChangedSample {
                        relative_path: path.clone(),
                        file_size: facts.size,
                        modified_ns: facts.modified_ns,
                        content_hash: hash,
                    });
                }
                context.stats.hashes_computed += 1;
            } else if entry.content_hash.is_none() {
                context.stats.hashes_pending += 1;
            }
            if context.committed_file_identity(&path) != facts.file_identity.as_deref() {
                batch.set_file_identity(&path, facts.file_identity.as_deref())?;
            }
        }
        Some(entry) => {
            let previous_hash = entry.content_hash.as_deref();
            if should_hash {
                let hash = required_prepared_hash(content_hash);
                batch.upsert_file_with_hash(&path, facts.size, facts.modified_ns, &hash)?;
                context.stats.hashes_computed += 1;
                context.stats.updated_samples.push(UpdatedSample {
                    relative_path: path.clone(),
                    file_size: facts.size,
                    modified_ns: facts.modified_ns,
                    content_hash: Some(hash.clone()),
                });
                if previous_hash != Some(hash.as_str()) {
                    context.stats.content_changed += 1;
                    context.stats.changed_samples.push(ChangedSample {
                        relative_path: path.clone(),
                        file_size: facts.size,
                        modified_ns: facts.modified_ns,
                        content_hash: hash,
                    });
                }
            } else {
                batch.upsert_file_without_hash(&path, facts.size, facts.modified_ns)?;
                context.stats.hashes_pending += 1;
                context.stats.updated_samples.push(UpdatedSample {
                    relative_path: path.clone(),
                    file_size: facts.size,
                    modified_ns: facts.modified_ns,
                    content_hash: None,
                });
            }
            batch.set_file_identity(&path, facts.file_identity.as_deref())?;
            context.stats.updated += 1;
        }
        None => {
            if should_hash {
                let hash = required_prepared_hash(content_hash);
                batch.upsert_file_with_hash(&path, facts.size, facts.modified_ns, &hash)?;
                batch.set_file_identity(&path, facts.file_identity.as_deref())?;
                if let Some(generation) = context.rename_candidate_generation {
                    batch.stage_pending_rename_destination(&path, generation)?;
                }
                context.stats.added += 1;
                context.stats.record_rename_candidate(path.clone());
                context.stats.content_changed += 1;
                context.stats.hashes_computed += 1;
                context.stats.changed_samples.push(ChangedSample {
                    relative_path: path.clone(),
                    file_size: facts.size,
                    modified_ns: facts.modified_ns,
                    content_hash: hash,
                });
            } else {
                // Size and modification time are not identity. Recover only a
                // unique same-file move here; otherwise wait for deep hashing.
                if let Some(file_identity) = facts.file_identity.as_deref()
                    && let Some(entry) = batch.take_pending_rename_by_file_identity(
                        file_identity,
                        facts.size,
                        facts.modified_ns,
                    )?
                {
                    let old_relative_path = entry.relative_path.clone();
                    apply_rename(
                        batch,
                        &path,
                        &facts,
                        None,
                        &old_relative_path,
                        &entry.metadata,
                    )?;
                    batch.set_file_identity(&path, Some(file_identity))?;
                    context.stats.updated += 1;
                    context.stats.renames_reconciled += 1;
                    context.stats.hashes_pending += 1;
                    context.stats.renamed_samples.push(RenamedSample {
                        old_relative_path,
                        new_relative_path: path.clone(),
                        file_size: facts.size,
                        modified_ns: facts.modified_ns,
                        content_hash: None,
                    });
                    return Ok(());
                }
                batch.upsert_file_without_hash(&path, facts.size, facts.modified_ns)?;
                batch.set_file_identity(&path, facts.file_identity.as_deref())?;
                if let Some(generation) = context.rename_candidate_generation {
                    batch.stage_pending_rename_destination(&path, generation)?;
                }
                context.stats.added += 1;
                context.stats.record_rename_candidate(path.clone());
                context.stats.hashes_pending += 1;
            }
        }
    }
    Ok(())
}

fn required_prepared_hash(content_hash: Option<String>) -> String {
    content_hash.expect("hash-required scan entries must be prepared before opening a write batch")
}

pub(super) fn mark_missing(
    db: &SourceDatabase,
    policy: SourceTraversalPolicy,
    batch: &mut SourceWriteBatch<'_>,
    existing: impl IntoIterator<Item = WavEntry>,
    stats: &mut ScanStats,
) -> Result<(), ScanError> {
    for stale in existing {
        if is_supported_scannable_audio_file(db.root(), &stale.relative_path, policy) {
            continue;
        }
        let Some(leftover) = db.entry_for_path(&stale.relative_path)? else {
            continue;
        };
        if !batch.clear_retained_pending_rename_destination(&leftover.relative_path)? {
            batch.clear_pending_rename_destination(&leftover.relative_path)?;
            batch.stage_pending_rename(&leftover)?;
        }
        batch.remove_file(&leftover.relative_path)?;
        stats.missing += 1;
    }
    Ok(())
}

fn apply_rename(
    batch: &mut SourceWriteBatch<'_>,
    new_path: &Path,
    facts: &FileFacts,
    hash: Option<&str>,
    old_path: &Path,
    metadata: &RenameMetadataSnapshot,
) -> Result<(), ScanError> {
    if let Some(hash) = hash {
        batch.upsert_file_with_hash_and_tag(
            new_path,
            facts.size,
            facts.modified_ns,
            hash,
            metadata.tag,
            false,
        )?;
    } else {
        batch.upsert_file_without_hash_and_tag(
            new_path,
            facts.size,
            facts.modified_ns,
            metadata.tag,
            false,
        )?;
    }
    batch.restore_rename_metadata(new_path, metadata)?;
    batch.remove_file(old_path)?;
    batch.remap_analysis_sample_identity(old_path, new_path)?;
    Ok(())
}

pub(super) fn should_compute_full_hash(mode: ScanMode, size: u64) -> bool {
    match mode {
        ScanMode::Targeted | ScanMode::Quick => size <= QUICK_HASH_MAX_BYTES,
        ScanMode::Hard => true,
    }
}

#[cfg(test)]
mod tests {
    use super::should_compute_full_hash;
    use crate::sample_sources::scanner::ScanMode;

    #[test]
    fn quick_hash_threshold_keeps_large_files_deferred() {
        assert!(should_compute_full_hash(ScanMode::Quick, 8 * 1024 * 1024));
        assert!(!should_compute_full_hash(
            ScanMode::Quick,
            8 * 1024 * 1024 + 1
        ));
    }
}
