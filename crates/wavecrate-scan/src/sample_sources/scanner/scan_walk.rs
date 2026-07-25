#![allow(clippy::type_complexity)]

use std::{
    cell::Cell,
    io::{Seek, SeekFrom},
    path::Path,
    sync::atomic::{AtomicBool, Ordering},
};

use crate::sample_sources::SourceDatabase;
use wavecrate_library::sample_sources::{SourceIndexDiagnostic, SourceTraversalPolicy};

use super::scan::{CommittedScanBatch, ScanContext, ScanError};
use super::scan_capability::{SourcePathBinding, SourceRootCapability};
use super::scan_diff::{PreparedFile, apply_diff};
use super::scan_diff_phase::prepare_diff_from_facts;
use super::scan_fs::{
    compute_content_hash_with_reader, is_supported_scannable_audio_file, read_facts_from_open_file,
    visit_dir_with_cancel_check,
};
use super::scan_index::inaccessible_index_entry;
use super::scan_writer::{ScanWritePhase, ScanWriter};

const APPLY_BATCH_SIZE: usize = 64;

pub(super) fn walk_phase(
    db: &SourceDatabase,
    root: &Path,
    source_root: &SourceRootCapability,
    policy: SourceTraversalPolicy,
    cancel: Option<&AtomicBool>,
    on_progress: &mut Option<&mut dyn FnMut(usize, &Path)>,
    context: &mut ScanContext,
    on_committed_batch: &mut dyn FnMut(CommittedScanBatch),
    writer: &impl ScanWriter,
) -> Result<(), ScanError> {
    // Each committed batch is a valid checkpoint. Cancellation may stop the scan between batches;
    // the completion metadata and missing-row reconciliation are not published, so a later scan
    // resumes from the durable partial index without presenting it as a complete generation.
    let committed = Cell::new(false);
    let mut pending = Vec::with_capacity(APPLY_BATCH_SIZE);
    let mut pending_noops = Vec::with_capacity(APPLY_BATCH_SIZE);
    let source_tree_snapshot = visit_dir_with_cancel_check(
        source_root,
        root,
        policy,
        &mut || cancel_requested(cancel),
        &mut |path| {
            if cancel_requested(cancel) {
                return Err(ScanError::Canceled);
            }
            let relative_path = path
                .strip_prefix(root)
                .map(Path::to_path_buf)
                .map_err(|_| ScanError::InvalidRoot(path.to_path_buf()))?;
            let mut prepared =
                match prepare_diff_from_capability(&source_root, root, &relative_path, context) {
                    Ok(Some(prepared)) => prepared,
                    Ok(None) => {
                        record_unavailable_preparation(
                            context,
                            relative_path,
                            path,
                            committed.get(),
                        );
                        return Ok(());
                    }
                    Err(error) => {
                        record_inaccessible_preparation(
                            context,
                            relative_path,
                            path,
                            committed.get(),
                            &error,
                        );
                        return Ok(());
                    }
                };
            if !context.resumable_manifest_audit_active() {
                context.stats.total_files += 1;
                if let Some(on_progress) = on_progress.as_mut() {
                    on_progress(context.stats.total_files, path);
                }
            }
            if !prepared.requires_apply {
                let Some(refreshed) = refresh_noop_preparation_or_skip(
                    db,
                    root,
                    &source_root,
                    context,
                    prepared,
                    committed.get(),
                )?
                else {
                    return Ok(());
                };
                prepared = refreshed;
            }
            if !prepared.requires_apply {
                if context.resumable_manifest_audit_active() {
                    context.record_manifest_audit_paths([relative_path]);
                    pending_noops.push(prepared);
                    if context.manifest_audit_checkpoint_due() {
                        flush_manifest_audit_checkpoint_with_noops(
                            db,
                            root,
                            &source_root,
                            context,
                            &mut pending_noops,
                        )?;
                    }
                } else {
                    skip_noop(context, &prepared);
                }
                publish_manifest_audit_progress(context, root, path, on_progress);
                return Ok(());
            }
            pending.push(prepared);
            if pending.len() == APPLY_BATCH_SIZE {
                let files = std::mem::replace(&mut pending, Vec::with_capacity(APPLY_BATCH_SIZE));
                let outcome = apply_batch_with_source_root(
                    db,
                    root,
                    source_root,
                    cancel,
                    context,
                    files,
                    committed.get(),
                    on_committed_batch,
                    writer,
                )?;
                if outcome.committed {
                    committed.set(true);
                }
                let last_path = outcome.audited_paths.last().cloned();
                context.record_manifest_audit_paths(outcome.audited_paths);
                if context.manifest_audit_checkpoint_due() {
                    flush_manifest_audit_checkpoint_with_noops(
                        db,
                        root,
                        &source_root,
                        context,
                        &mut pending_noops,
                    )?;
                }
                if let Some(last_path) = last_path {
                    publish_manifest_audit_progress(
                        context,
                        root,
                        &root.join(last_path),
                        on_progress,
                    );
                }
            }
            Ok(())
        },
    )?;
    if !pending.is_empty() {
        let outcome = apply_batch_with_source_root(
            db,
            root,
            source_root,
            cancel,
            context,
            pending,
            committed.get(),
            on_committed_batch,
            writer,
        )?;
        if outcome.committed {
            committed.set(true);
        }
        let last_path = outcome.audited_paths.last().cloned();
        context.record_manifest_audit_paths(outcome.audited_paths);
        if let Some(last_path) = last_path {
            publish_manifest_audit_progress(context, root, &root.join(last_path), on_progress);
        }
    }
    context.mark_uncertain_prefixes(source_tree_snapshot.uncertain_prefixes.clone());
    if !context.has_uncertain_prefixes() {
        context.complete_missing_manifest_audit_paths();
    }
    flush_manifest_audit_checkpoint_with_noops(
        db,
        root,
        &source_root,
        context,
        &mut pending_noops,
    )?;
    context.observe_index_entries(source_tree_snapshot.index_entries.clone());
    context.stats.source_tree_snapshot =
        Some(finalize_source_tree_snapshot(context, source_tree_snapshot));
    Ok(())
}

fn finalize_source_tree_snapshot(
    context: &ScanContext,
    mut source_tree_snapshot: super::scan::SourceTreeSnapshot,
) -> super::scan::SourceTreeSnapshot {
    if context.source_tree_incomplete() {
        source_tree_snapshot
            .diagnostics
            .push(super::scan::SourceTreeDiagnostic::TraversalChanged);
    }
    source_tree_snapshot
}

fn flush_manifest_audit_checkpoint_with_noops(
    db: &SourceDatabase,
    root: &Path,
    source_root: &SourceRootCapability,
    context: &mut ScanContext,
    pending_noops: &mut Vec<PreparedFile>,
) -> Result<(), ScanError> {
    flush_manifest_audit_checkpoint_with_noops_hook(
        db,
        root,
        source_root,
        context,
        pending_noops,
        |_| {},
    )
}

fn flush_manifest_audit_checkpoint_with_noops_hook(
    db: &SourceDatabase,
    root: &Path,
    source_root: &SourceRootCapability,
    context: &mut ScanContext,
    pending_noops: &mut Vec<PreparedFile>,
    mut precheckpoint: impl FnMut(&Path),
) -> Result<(), ScanError> {
    for prepared in pending_noops.iter() {
        precheckpoint(&prepared.facts.relative);
    }
    let retained = std::mem::take(pending_noops);
    let mut valid = Vec::with_capacity(retained.len());
    let mut invalid_paths = Vec::new();
    for prepared in retained {
        let relative_path = prepared.facts.relative.clone();
        let Some(file) = prepared.source_file.as_ref() else {
            context.existing.remove(&relative_path);
            invalid_paths.push(relative_path);
            context.mark_source_tree_incomplete();
            continue;
        };
        let absolute = root.join(&relative_path);
        let descriptor_matches = read_facts_from_open_file(root, &absolute, file)
            .is_ok_and(|facts| prepared_still_current(&prepared, &facts));
        let binding = source_root
            .path_binding(&relative_path, file)
            .unwrap_or(SourcePathBinding::Changed);
        if descriptor_matches && binding == SourcePathBinding::Matches {
            skip_noop(context, &prepared);
            valid.push(prepared);
            continue;
        }
        if binding != SourcePathBinding::Retire {
            context.existing.remove(&relative_path);
        }
        invalid_paths.push(relative_path);
        context.mark_source_tree_incomplete();
    }
    context.discard_manifest_audit_paths(invalid_paths);
    source_root.ensure_current_generation()?;
    context.flush_manifest_audit_checkpoint(db)?;
    drop(valid);
    Ok(())
}

fn publish_manifest_audit_progress(
    context: &ScanContext,
    root: &Path,
    path: &Path,
    on_progress: &mut Option<&mut dyn FnMut(usize, &Path)>,
) {
    let Some((checked, _expected)) = context.manifest_audit_progress() else {
        return;
    };
    if let Some(on_progress) = on_progress.as_mut() {
        on_progress(checked, path.strip_prefix(root).unwrap_or(path));
    }
}

fn prepare_diff_from_capability(
    source_root: &SourceRootCapability,
    root: &Path,
    relative_path: &Path,
    context: &ScanContext,
) -> Result<Option<PreparedFile>, ScanError> {
    let Some(file) = source_root.open_regular_file(relative_path)? else {
        return Ok(None);
    };
    let absolute = root.join(relative_path);
    let facts = read_facts_from_open_file(root, &absolute, &file)?;
    let mut prepared = prepare_diff_from_facts(facts, context);
    prepared.source_file = Some(file);
    prepared.source_handle_verified = true;
    Ok(Some(prepared))
}

fn record_unavailable_preparation(
    context: &mut ScanContext,
    relative_path: std::path::PathBuf,
    path: &Path,
    committed: bool,
) {
    context.mark_source_tree_incomplete();
    context.existing.remove(&relative_path);
    context.mark_uncertain_prefixes([relative_path]);
    tracing::warn!(
        path = %path.display(),
        committed,
        "Persisting a source file that no longer opens without following links for retry"
    );
}

fn record_inaccessible_preparation(
    context: &mut ScanContext,
    relative_path: std::path::PathBuf,
    path: &Path,
    committed: bool,
    error: &ScanError,
) {
    context.mark_source_tree_incomplete();
    context.existing.remove(&relative_path);
    context.mark_uncertain_prefixes([relative_path.clone()]);
    context.observe_index_entries([inaccessible_index_entry(
        relative_path,
        SourceIndexDiagnostic::MetadataUnavailable,
    )]);
    tracing::warn!(
        path = %path.display(),
        %error,
        committed,
        "Persisting an inaccessible supported file for retry"
    );
}

pub(super) fn apply_prepared_chunk(
    db: &SourceDatabase,
    root: &Path,
    source_root: &SourceRootCapability,
    cancel: Option<&AtomicBool>,
    context: &mut ScanContext,
    prepared: Vec<PreparedFile>,
    tolerate_file_errors: bool,
    writer: &impl ScanWriter,
) -> Result<bool, ScanError> {
    let mut pending = Vec::with_capacity(prepared.len());
    for mut file in prepared {
        if !file.requires_apply {
            let Some(refreshed) = refresh_noop_preparation_or_skip(
                db,
                root,
                &source_root,
                context,
                file,
                tolerate_file_errors,
            )?
            else {
                continue;
            };
            file = refreshed;
        }
        if file.requires_apply {
            pending.push(file);
        } else {
            skip_noop(context, &file);
        }
    }
    if pending.is_empty() {
        return Ok(false);
    }
    let mut on_committed_batch = |_| {};
    apply_batch_with_source_root(
        db,
        root,
        source_root,
        cancel,
        context,
        pending,
        tolerate_file_errors,
        &mut on_committed_batch,
        writer,
    )
    .map(|outcome| outcome.committed)
}

fn refresh_noop_preparation_or_skip(
    db: &SourceDatabase,
    root: &Path,
    source_root: &SourceRootCapability,
    context: &mut ScanContext,
    prepared: PreparedFile,
    tolerate_file_errors: bool,
) -> Result<Option<PreparedFile>, ScanError> {
    let relative_path = prepared.facts.relative.clone();
    match refresh_noop_preparation(db, root, source_root, context, prepared) {
        Ok(Some(prepared)) => Ok(Some(prepared)),
        Ok(None) => {
            context.mark_source_tree_incomplete();
            Ok(None)
        }
        Err(error) if tolerate_file_errors => {
            context.mark_source_tree_incomplete();
            skip_changed_or_unavailable(context, root, &relative_path);
            tracing::warn!(
                path = %root.join(&relative_path).display(),
                error = %error,
                "Skipping no-op refresh failure after an earlier scan batch committed"
            );
            Ok(None)
        }
        Err(error) => Err(error),
    }
}

fn refresh_noop_preparation(
    db: &SourceDatabase,
    root: &Path,
    source_root: &SourceRootCapability,
    context: &mut ScanContext,
    prepared: PreparedFile,
) -> Result<Option<PreparedFile>, ScanError> {
    let relative_path = prepared.facts.relative.clone();
    let current = db.entry_for_path(&relative_path)?;
    let snapshot = context.existing.get(&relative_path);
    let database_snapshot_matches =
        current
            .as_ref()
            .zip(snapshot)
            .is_some_and(|(entry, snapshot)| {
                !entry.missing
                    && entry.file_size == prepared.facts.size
                    && entry.modified_ns == prepared.facts.modified_ns
                    && entry.content_hash == snapshot.content_hash
            });
    if database_snapshot_matches {
        let mut prepared = prepared;
        let file = match prepared.source_file.take() {
            Some(file) => file,
            None => {
                let Some(file) = source_root.open_regular_file(&relative_path)? else {
                    return Ok(None);
                };
                file
            }
        };
        let absolute = root.join(&relative_path);
        let descriptor_matches = read_facts_from_open_file(root, &absolute, &file)
            .is_ok_and(|facts| prepared_still_current(&prepared, &facts));
        match source_root
            .path_binding(&relative_path, &file)
            .unwrap_or(SourcePathBinding::Changed)
        {
            SourcePathBinding::Matches if descriptor_matches => {
                prepared.source_file = Some(file);
                prepared.source_handle_verified = true;
                return Ok(Some(prepared));
            }
            // Leave the prior row eligible for missing reconciliation.
            SourcePathBinding::Retire => return Ok(None),
            SourcePathBinding::Matches | SourcePathBinding::Changed => {
                context.existing.remove(&relative_path);
                return Ok(None);
            }
        }
    }
    match current {
        Some(entry) => {
            context.existing.insert(relative_path.clone(), entry);
        }
        None => {
            context.existing.remove(&relative_path);
        }
    }
    prepare_diff_from_capability(source_root, root, &relative_path, context)
}

pub(super) fn skip_noop(context: &mut ScanContext, prepared: &PreparedFile) {
    if !prepared.needs_hash
        && context
            .existing
            .get(&prepared.facts.relative)
            .is_some_and(|entry| entry.content_hash.is_none())
    {
        context.stats.hashes_pending += 1;
    }
    let _ = context.existing.remove(&prepared.facts.relative);
}

#[cfg(test)]
fn apply_batch(
    db: &SourceDatabase,
    root: &Path,
    cancel: Option<&AtomicBool>,
    context: &mut ScanContext,
    prepared: Vec<PreparedFile>,
    tolerate_file_errors: bool,
    writer: &impl ScanWriter,
) -> Result<ApplyBatchOutcome, ScanError> {
    let source_root = SourceRootCapability::open(root)?;
    let mut on_committed_batch = |_| {};
    apply_batch_with_source_root(
        db,
        root,
        &source_root,
        cancel,
        context,
        prepared,
        tolerate_file_errors,
        &mut on_committed_batch,
        writer,
    )
}

fn apply_batch_with_source_root(
    db: &SourceDatabase,
    root: &Path,
    source_root: &SourceRootCapability,
    cancel: Option<&AtomicBool>,
    context: &mut ScanContext,
    prepared: Vec<PreparedFile>,
    tolerate_file_errors: bool,
    on_committed_batch: &mut dyn FnMut(CommittedScanBatch),
    writer: &impl ScanWriter,
) -> Result<ApplyBatchOutcome, ScanError> {
    apply_batch_with_source_root_and_precommit_hook(
        db,
        root,
        source_root,
        cancel,
        context,
        prepared,
        tolerate_file_errors,
        writer,
        |_| {},
        on_committed_batch,
    )
}

#[allow(clippy::too_many_arguments)]
#[cfg(test)]
fn apply_batch_with_precommit_hook(
    db: &SourceDatabase,
    root: &Path,
    cancel: Option<&AtomicBool>,
    context: &mut ScanContext,
    prepared: Vec<PreparedFile>,
    tolerate_file_errors: bool,
    writer: &impl ScanWriter,
    precommit: impl FnMut(&Path),
) -> Result<ApplyBatchOutcome, ScanError> {
    let source_root = SourceRootCapability::open(root)?;
    let mut on_committed_batch = |_| {};
    apply_batch_with_source_root_and_precommit_hook(
        db,
        root,
        &source_root,
        cancel,
        context,
        prepared,
        tolerate_file_errors,
        writer,
        precommit,
        &mut on_committed_batch,
    )
}

#[allow(clippy::too_many_arguments)]
fn apply_batch_with_source_root_and_precommit_hook(
    db: &SourceDatabase,
    root: &Path,
    source_root: &SourceRootCapability,
    cancel: Option<&AtomicBool>,
    context: &mut ScanContext,
    prepared: Vec<PreparedFile>,
    tolerate_file_errors: bool,
    writer: &impl ScanWriter,
    mut precommit: impl FnMut(&Path),
    on_committed_batch: &mut dyn FnMut(CommittedScanBatch),
) -> Result<ApplyBatchOutcome, ScanError> {
    let mut ready = Vec::with_capacity(prepared.len());
    let mut post_hash = |_: &Path| {};
    for file in prepared {
        let relative_path = file.facts.relative.clone();
        let outcome = match prepare_for_apply_with_capability(
            db,
            root,
            &source_root,
            cancel,
            file,
            &mut post_hash,
        ) {
            Ok(outcome) => outcome,
            Err(ScanError::Canceled) => return Err(ScanError::Canceled),
            Err(error) if tolerate_file_errors => {
                context.mark_source_tree_incomplete();
                skip_changed_or_unavailable(context, root, &relative_path);
                tracing::warn!(
                    path = %root.join(&relative_path).display(),
                    error = %error,
                    "Skipping file preparation failure after an earlier scan batch committed"
                );
                continue;
            }
            Err(error) => return Err(error),
        };
        match outcome {
            PrepareForApply::Ready(file) => ready.push(file),
            PrepareForApply::Gone => context.mark_source_tree_incomplete(),
            PrepareForApply::Skip => {
                context.mark_source_tree_incomplete();
                skip_changed_or_unavailable(context, root, &relative_path);
            }
        }
    }
    if ready.is_empty() {
        return Ok(ApplyBatchOutcome::default());
    }
    if cancel_requested(cancel) {
        return Err(ScanError::Canceled);
    }
    let _writer = writer.lock(ScanWritePhase::Manifest);
    if cancel_requested(cancel) {
        return Err(ScanError::Canceled);
    }
    let mut batch = db.write_batch()?;
    let stats_before_staging = context.stats.clone();
    let generation_before_staging = context.rename_candidate_generation;
    context.ensure_rename_candidate_generation(&mut batch)?;
    let mut audited_paths = Vec::with_capacity(ready.len());
    let mut retained = Vec::with_capacity(ready.len());
    for mut file in ready {
        let relative_path = file.facts.relative.clone();
        let file_handle = file
            .source_file
            .as_ref()
            .expect("prepared source files retain their descriptor through apply");
        let absolute = root.join(&relative_path);
        let descriptor_still_current = read_facts_from_open_file(root, &absolute, file_handle)
            .is_ok_and(|current| prepared_still_current(&file, &current));
        if !descriptor_still_current {
            context.mark_source_tree_incomplete();
            context.existing.remove(&relative_path);
            continue;
        }
        match source_root
            .path_binding(&relative_path, file_handle)
            .unwrap_or(SourcePathBinding::Changed)
        {
            SourcePathBinding::Matches => {}
            // A link or absence is a definite deletion: leave its old row in
            // `existing` for the missing phase to retire.
            SourcePathBinding::Retire => continue,
            SourcePathBinding::Changed => {
                context.mark_source_tree_incomplete();
                context.existing.remove(&relative_path);
                continue;
            }
        }
        let retained_file = file
            .source_file
            .take()
            .expect("validated source files retain their descriptor until commit");
        let retained_facts = file.facts.clone();
        let content_was_hashed = file.content_hash.is_some();
        let existing_before = context.existing.get(&relative_path).cloned();
        apply_diff(db, &mut batch, file, context)?;
        audited_paths.push(relative_path.clone());
        retained.push(RetainedPublication {
            relative_path,
            facts: retained_facts,
            content_was_hashed,
            file: retained_file,
            existing_before,
        });
    }
    if cancel_requested(cancel) {
        return Err(ScanError::Canceled);
    }
    for publication in &retained {
        precommit(&publication.relative_path);
    }
    let final_bindings = retained
        .iter()
        .map(|publication| {
            let absolute = root.join(&publication.relative_path);
            let descriptor_matches = read_facts_from_open_file(root, &absolute, &publication.file)
                .is_ok_and(|facts| {
                    if publication.content_was_hashed {
                        publication.facts.same_content_snapshot(&facts)
                    } else {
                        publication.facts.same_file_facts(&facts)
                    }
                });
            let binding = source_root
                .path_binding(&publication.relative_path, &publication.file)
                .unwrap_or(SourcePathBinding::Changed);
            (
                publication.relative_path.clone(),
                descriptor_matches,
                binding,
            )
        })
        .collect::<Vec<_>>();
    if final_bindings
        .iter()
        .any(|(_, descriptor_matches, binding)| {
            !descriptor_matches || *binding != SourcePathBinding::Matches
        })
    {
        drop(batch);
        context.stats = stats_before_staging;
        context.rename_candidate_generation = generation_before_staging;
        for publication in &retained {
            if let Some(existing) = publication.existing_before.clone() {
                context
                    .existing
                    .insert(publication.relative_path.clone(), existing);
            } else {
                context.existing.remove(&publication.relative_path);
            }
        }
        for (relative_path, _descriptor_matches, binding) in final_bindings {
            if binding == SourcePathBinding::Retire {
                continue;
            }
            context.existing.remove(&relative_path);
        }
        context.mark_source_tree_incomplete();
        return Ok(ApplyBatchOutcome::default());
    }
    source_root.ensure_current_generation()?;
    let revision = context.commit_batch(db, batch)?;
    on_committed_batch(CommittedScanBatch {
        revision,
        paths: audited_paths.clone(),
    });
    Ok(ApplyBatchOutcome {
        committed: true,
        audited_paths,
    })
}

struct RetainedPublication {
    relative_path: std::path::PathBuf,
    facts: super::scan_fs::FileFacts,
    content_was_hashed: bool,
    file: std::fs::File,
    existing_before: Option<crate::sample_sources::WavEntry>,
}

#[derive(Default)]
struct ApplyBatchOutcome {
    committed: bool,
    audited_paths: Vec<std::path::PathBuf>,
}

fn skip_changed_or_unavailable(context: &mut ScanContext, root: &Path, relative_path: &Path) {
    if is_supported_scannable_audio_file(root, relative_path, context.traversal_policy()) {
        context.existing.remove(relative_path);
    }
}

enum PrepareForApply {
    Ready(PreparedFile),
    Gone,
    Skip,
}

#[cfg(test)]
fn prepare_for_apply(
    db: &SourceDatabase,
    root: &Path,
    cancel: Option<&AtomicBool>,
    prepared: PreparedFile,
) -> Result<PrepareForApply, ScanError> {
    prepare_for_apply_with_post_hash_hook(db, root, cancel, prepared, |_| {})
}

#[cfg(test)]
fn prepare_for_apply_with_post_hash_hook(
    db: &SourceDatabase,
    root: &Path,
    cancel: Option<&AtomicBool>,
    prepared: PreparedFile,
    mut post_hash: impl FnMut(&Path),
) -> Result<PrepareForApply, ScanError> {
    let source_root = SourceRootCapability::open(root)?;
    prepare_for_apply_with_capability(db, root, &source_root, cancel, prepared, &mut post_hash)
}

fn prepare_for_apply_with_capability(
    db: &SourceDatabase,
    root: &Path,
    source_root: &SourceRootCapability,
    cancel: Option<&AtomicBool>,
    mut prepared: PreparedFile,
    post_hash: &mut impl FnMut(&Path),
) -> Result<PrepareForApply, ScanError> {
    let absolute = root.join(&prepared.facts.relative);
    if !prepared.source_handle_verified {
        let Some(file) = source_root.open_regular_file(&prepared.facts.relative)? else {
            return Ok(PrepareForApply::Gone);
        };
        prepared.source_file = Some(file);
        prepared.source_handle_verified = true;
    }
    let before_hash = read_facts_from_open_file(
        root,
        &absolute,
        prepared
            .source_file
            .as_ref()
            .expect("verified source files retain their open descriptor until preparation"),
    )?;
    if !facts_match(&prepared, &before_hash) {
        return Ok(PrepareForApply::Skip);
    }
    let file = prepared
        .source_file
        .as_mut()
        .expect("verified source files retain their open descriptor until preparation");
    let current_needs_hash = db
        .entry_for_path(&prepared.facts.relative)?
        .is_none_or(|entry| {
            entry.file_size != prepared.facts.size
                || entry.modified_ns != prepared.facts.modified_ns
                || entry.content_hash.is_none()
        });
    if prepared.hash_required && (prepared.needs_hash || current_needs_hash) {
        prepared.facts = before_hash;
        file.seek(SeekFrom::Start(0))
            .map_err(|source| ScanError::Io {
                path: absolute.clone(),
                source,
            })?;
        prepared.content_hash = Some(compute_content_hash_with_reader(&absolute, file, cancel)?);
        post_hash(&absolute);
        let after_hash = read_facts_from_open_file(root, &absolute, &file)?;
        if !prepared.facts.same_content_snapshot(&after_hash) {
            return Ok(PrepareForApply::Skip);
        }
        prepared.facts = after_hash;
    } else {
        prepared.facts = before_hash;
    }
    Ok(PrepareForApply::Ready(prepared))
}

fn facts_match(prepared: &PreparedFile, current: &super::scan_fs::FileFacts) -> bool {
    current.same_file_facts(&prepared.facts)
}

fn prepared_still_current(prepared: &PreparedFile, current: &super::scan_fs::FileFacts) -> bool {
    if prepared.content_hash.is_some() {
        current.same_content_snapshot(&prepared.facts)
    } else {
        facts_match(prepared, current)
    }
}

fn cancel_requested(cancel: Option<&AtomicBool>) -> bool {
    cancel.is_some_and(|cancel| cancel.load(Ordering::Relaxed))
}

#[cfg(test)]
mod tests {
    use super::{
        PrepareForApply, SourceRootCapability, apply_batch, apply_batch_with_precommit_hook,
        finalize_source_tree_snapshot, flush_manifest_audit_checkpoint_with_noops_hook,
        prepare_diff_from_capability, prepare_for_apply, prepare_for_apply_with_post_hash_hook,
        refresh_noop_preparation_or_skip,
    };
    use crate::sample_sources::SourceDatabase;
    use crate::sample_sources::scanner::scan::{ScanContext, ScanError, ScanMode, scan_once};
    use crate::sample_sources::scanner::scan_diff::PreparedFile;
    use crate::sample_sources::scanner::scan_diff_phase::prepare_diff;
    use crate::sample_sources::scanner::scan_fs::read_facts;
    use std::collections::HashMap;
    use std::path::Path;
    use tempfile::tempdir;

    #[test]
    fn missing_repair_with_existing_hash_skips_prepared_hash() {
        let dir = tempdir().unwrap();
        let file_path = dir.path().join("one.wav");
        std::fs::write(&file_path, b"one").unwrap();
        let db = SourceDatabase::open_for_scan(dir.path()).unwrap();
        scan_once(&db).unwrap();
        db.set_missing(Path::new("one.wav"), true).unwrap();
        let prepared = PreparedFile {
            facts: read_facts(dir.path(), &file_path).unwrap(),
            hash_required: true,
            needs_hash: false,
            requires_apply: true,
            revalidate_checkpoint: false,
            identity_replaced: false,
            content_hash: None,
            source_file: None,
            source_handle_verified: false,
        };

        assert!(prepared.requires_apply);
        assert!(!prepared.needs_hash);
        let outcome = prepare_for_apply(&db, dir.path(), None, prepared).unwrap();
        let PrepareForApply::Ready(prepared) = outcome else {
            panic!("restored missing file should remain ready for apply");
        };
        assert!(prepared.content_hash.is_none());
    }

    #[test]
    fn committed_batch_tolerates_noop_refresh_failure() {
        let dir = tempdir().unwrap();
        let file_path = dir.path().join("one.wav");
        std::fs::write(&file_path, b"one").unwrap();
        let db = SourceDatabase::open_for_scan(dir.path()).unwrap();
        scan_once(&db).unwrap();
        let entry = db.entry_for_path(Path::new("one.wav")).unwrap().unwrap();
        let mut context = ScanContext::from_existing(
            HashMap::from([(Path::new("one.wav").to_path_buf(), entry)]),
            ScanMode::Quick,
            db.get_revision().unwrap(),
            db.list_manifest_entries().unwrap(),
        );
        let prepared = prepare_diff(dir.path(), &file_path, &context).unwrap();
        assert!(!prepared.requires_apply);

        std::fs::remove_file(&file_path).unwrap();
        let mut batch = db.write_batch().unwrap();
        batch.remove_file(Path::new("one.wav")).unwrap();
        batch.commit().unwrap();

        let source_root = SourceRootCapability::open(dir.path()).unwrap();
        let refreshed = refresh_noop_preparation_or_skip(
            &db,
            dir.path(),
            &source_root,
            &mut context,
            prepared,
            true,
        )
        .unwrap();

        assert!(refreshed.is_none());
        assert!(context.source_tree_incomplete());
        let snapshot = finalize_source_tree_snapshot(&context, Default::default());
        assert!(!snapshot.is_complete());
    }

    #[cfg(unix)]
    #[test]
    fn full_scan_noop_does_not_audit_a_late_outside_link_replacement() {
        use std::os::unix::fs::symlink;

        let dir = tempdir().unwrap();
        let relative = Path::new("one.wav");
        let source = dir.path().join(relative);
        let outside = tempdir().unwrap();
        let outside_file = outside.path().join("outside.wav");
        std::fs::write(&source, b"inside").unwrap();
        std::fs::write(&outside_file, b"outside").unwrap();
        let db = SourceDatabase::open_for_scan(dir.path()).unwrap();
        scan_once(&db).unwrap();
        let entry = db.entry_for_path(relative).unwrap().unwrap();
        let mut context = ScanContext::from_existing(
            HashMap::from([(relative.to_path_buf(), entry)]),
            ScanMode::Quick,
            db.get_revision().unwrap(),
            db.list_manifest_entries().unwrap(),
        );
        let source_root = SourceRootCapability::open(dir.path()).unwrap();
        let prepared = prepare_diff_from_capability(&source_root, dir.path(), relative, &context)
            .unwrap()
            .unwrap();
        assert!(!prepared.requires_apply);

        std::fs::remove_file(&source).unwrap();
        symlink(&outside_file, &source).unwrap();

        let refreshed = refresh_noop_preparation_or_skip(
            &db,
            dir.path(),
            &source_root,
            &mut context,
            prepared,
            false,
        )
        .unwrap();

        assert!(refreshed.is_none());
        assert!(context.source_tree_incomplete());
        assert!(context.existing.contains_key(relative));
    }

    #[cfg(unix)]
    #[test]
    fn manifest_audit_noop_rejects_a_final_link_before_checkpoint() {
        use std::os::unix::fs::symlink;

        let dir = tempdir().unwrap();
        let relative = Path::new("one.wav");
        let source = dir.path().join(relative);
        let outside = tempdir().unwrap();
        let outside_file = outside.path().join("outside.wav");
        std::fs::write(&source, b"inside").unwrap();
        std::fs::write(&outside_file, b"outside").unwrap();
        let db = SourceDatabase::open_for_scan(dir.path()).unwrap();
        scan_once(&db).unwrap();
        let entry = db.entry_for_path(relative).unwrap().unwrap();
        let mut context = ScanContext::from_existing(
            HashMap::from([(relative.to_path_buf(), entry)]),
            ScanMode::Quick,
            db.get_revision().unwrap(),
            db.list_manifest_entries().unwrap(),
        );
        context.resume_manifest_audit(&db, 100).unwrap();
        let source_root = SourceRootCapability::open(dir.path()).unwrap();
        let prepared = prepare_diff_from_capability(&source_root, dir.path(), relative, &context)
            .unwrap()
            .unwrap();
        let prepared = refresh_noop_preparation_or_skip(
            &db,
            dir.path(),
            &source_root,
            &mut context,
            prepared,
            false,
        )
        .unwrap()
        .unwrap();
        context.record_manifest_audit_paths([relative.to_path_buf()]);
        let mut pending_noops = vec![prepared];

        flush_manifest_audit_checkpoint_with_noops_hook(
            &db,
            dir.path(),
            &source_root,
            &mut context,
            &mut pending_noops,
            |_| {
                std::fs::remove_file(&source).unwrap();
                symlink(&outside_file, &source).unwrap();
            },
        )
        .unwrap();

        assert!(
            !db.begin_or_resume_manifest_audit(101)
                .unwrap()
                .contains(&relative.to_path_buf())
        );
        assert!(context.existing.contains_key(relative));
        assert!(context.source_tree_incomplete());
    }

    #[cfg(unix)]
    #[test]
    fn manifest_audit_noop_rejects_a_link_ancestor_before_checkpoint() {
        use std::os::unix::fs::symlink;

        let dir = tempdir().unwrap();
        let nested = dir.path().join("nested");
        std::fs::create_dir(&nested).unwrap();
        let relative = Path::new("nested/one.wav");
        std::fs::write(nested.join("one.wav"), b"inside").unwrap();
        let outside = tempdir().unwrap();
        std::fs::write(outside.path().join("one.wav"), b"outside").unwrap();
        let db = SourceDatabase::open_for_scan(dir.path()).unwrap();
        scan_once(&db).unwrap();
        let entry = db.entry_for_path(relative).unwrap().unwrap();
        let mut context = ScanContext::from_existing(
            HashMap::from([(relative.to_path_buf(), entry)]),
            ScanMode::Quick,
            db.get_revision().unwrap(),
            db.list_manifest_entries().unwrap(),
        );
        context.resume_manifest_audit(&db, 100).unwrap();
        let source_root = SourceRootCapability::open(dir.path()).unwrap();
        let prepared = prepare_diff_from_capability(&source_root, dir.path(), relative, &context)
            .unwrap()
            .unwrap();
        let prepared = refresh_noop_preparation_or_skip(
            &db,
            dir.path(),
            &source_root,
            &mut context,
            prepared,
            false,
        )
        .unwrap()
        .unwrap();
        context.record_manifest_audit_paths([relative.to_path_buf()]);
        let mut pending_noops = vec![prepared];

        flush_manifest_audit_checkpoint_with_noops_hook(
            &db,
            dir.path(),
            &source_root,
            &mut context,
            &mut pending_noops,
            |_| {
                std::fs::rename(&nested, dir.path().join("moved")).unwrap();
                symlink(outside.path(), &nested).unwrap();
            },
        )
        .unwrap();

        assert!(
            !db.begin_or_resume_manifest_audit(101)
                .unwrap()
                .contains(&relative.to_path_buf())
        );
        assert!(context.existing.contains_key(relative));
        assert!(context.source_tree_incomplete());
    }

    #[test]
    fn committed_batch_marks_disappeared_file_projection_incomplete() {
        let dir = tempdir().unwrap();
        let file_path = dir.path().join("one.wav");
        std::fs::write(&file_path, b"one").unwrap();
        let db = SourceDatabase::open_for_scan(dir.path()).unwrap();
        let mut context = ScanContext::from_existing(
            HashMap::new(),
            ScanMode::Quick,
            db.get_revision().unwrap(),
            db.list_manifest_entries().unwrap(),
        );
        let prepared = prepare_diff(dir.path(), &file_path, &context).unwrap();
        assert!(prepared.requires_apply);

        std::fs::remove_file(&file_path).unwrap();
        let outcome = apply_batch(
            &db,
            dir.path(),
            None,
            &mut context,
            vec![prepared],
            true,
            &super::super::scan_writer::UncoordinatedScanWriter,
        )
        .expect("tolerated committed batch");

        assert!(!outcome.committed);
        assert!(context.source_tree_incomplete());
        let snapshot = finalize_source_tree_snapshot(&context, Default::default());
        assert!(!snapshot.is_complete());
    }

    #[test]
    fn targeted_hash_preparation_rejects_mutation_during_hashing() {
        let dir = tempdir().unwrap();
        let file_path = dir.path().join("one.wav");
        std::fs::write(&file_path, [1_u8; 32]).unwrap();
        let db = SourceDatabase::open_for_scan(dir.path()).unwrap();
        db.upsert_file(Path::new("one.wav"), 32, 1).unwrap();
        let prepared = PreparedFile {
            facts: read_facts(dir.path(), &file_path).unwrap(),
            hash_required: true,
            needs_hash: true,
            requires_apply: true,
            revalidate_checkpoint: false,
            identity_replaced: false,
            content_hash: None,
            source_file: None,
            source_handle_verified: false,
        };

        let outcome =
            prepare_for_apply_with_post_hash_hook(&db, dir.path(), None, prepared, |path| {
                std::fs::write(path, [2_u8; 32]).unwrap()
            })
            .unwrap();

        assert!(matches!(outcome, PrepareForApply::Skip));
    }

    #[cfg(unix)]
    #[test]
    fn full_scan_hash_open_rejects_an_outside_link_replacement_after_classification() {
        use std::os::unix::fs::symlink;

        let dir = tempdir().unwrap();
        let source = dir.path().join("one.wav");
        let outside = tempdir().unwrap();
        let outside_file = outside.path().join("outside.wav");
        std::fs::write(&source, b"inside").unwrap();
        std::fs::write(&outside_file, b"outside").unwrap();
        let db = SourceDatabase::open_for_scan(dir.path()).unwrap();
        let context = ScanContext::from_existing(
            HashMap::new(),
            ScanMode::Quick,
            db.get_revision().unwrap(),
            db.list_manifest_entries().unwrap(),
        );
        let prepared = prepare_diff(dir.path(), &source, &context).unwrap();

        std::fs::remove_file(&source).unwrap();
        symlink(&outside_file, &source).unwrap();

        assert!(matches!(
            prepare_for_apply(&db, dir.path(), None, prepared).unwrap(),
            PrepareForApply::Gone
        ));
    }

    #[cfg(unix)]
    #[test]
    fn full_scan_fact_collection_rejects_an_outside_link_replacement() {
        use std::os::unix::fs::symlink;

        let dir = tempdir().unwrap();
        let source = dir.path().join("one.wav");
        let outside = tempdir().unwrap();
        let outside_file = outside.path().join("outside.wav");
        std::fs::write(&source, b"inside").unwrap();
        std::fs::write(&outside_file, b"outside").unwrap();
        let db = SourceDatabase::open_for_scan(dir.path()).unwrap();
        let context = ScanContext::from_existing(
            HashMap::new(),
            ScanMode::Quick,
            db.get_revision().unwrap(),
            db.list_manifest_entries().unwrap(),
        );
        let source_root = SourceRootCapability::open(dir.path()).unwrap();

        std::fs::remove_file(&source).unwrap();
        symlink(&outside_file, &source).unwrap();

        assert!(
            prepare_diff_from_capability(&source_root, dir.path(), Path::new("one.wav"), &context,)
                .unwrap()
                .is_none()
        );
    }

    #[cfg(unix)]
    #[test]
    fn full_scan_fact_collection_rejects_an_outside_link_ancestor() {
        use std::os::unix::fs::symlink;

        let dir = tempdir().unwrap();
        let nested = dir.path().join("nested");
        let moved = dir.path().join("moved");
        std::fs::create_dir(&nested).unwrap();
        std::fs::write(nested.join("one.wav"), b"inside").unwrap();
        let outside = tempdir().unwrap();
        std::fs::write(outside.path().join("one.wav"), b"outside").unwrap();
        let db = SourceDatabase::open_for_scan(dir.path()).unwrap();
        let context = ScanContext::from_existing(
            HashMap::new(),
            ScanMode::Quick,
            db.get_revision().unwrap(),
            db.list_manifest_entries().unwrap(),
        );
        let source_root = SourceRootCapability::open(dir.path()).unwrap();

        std::fs::rename(&nested, &moved).unwrap();
        symlink(outside.path(), &nested).unwrap();

        assert!(!matches!(
            prepare_diff_from_capability(
                &source_root,
                dir.path(),
                Path::new("nested/one.wav"),
                &context,
            ),
            Ok(Some(_))
        ));
    }

    #[cfg(unix)]
    #[test]
    fn full_scan_hash_rejects_an_outside_link_replacement_during_hashing() {
        use std::os::unix::fs::symlink;

        let dir = tempdir().unwrap();
        let source = dir.path().join("one.wav");
        let outside = tempdir().unwrap();
        let outside_file = outside.path().join("outside.wav");
        std::fs::write(&source, b"inside").unwrap();
        std::fs::write(&outside_file, b"outside").unwrap();
        let db = SourceDatabase::open_for_scan(dir.path()).unwrap();
        let prepared = PreparedFile {
            facts: read_facts(dir.path(), &source).unwrap(),
            hash_required: true,
            needs_hash: true,
            requires_apply: true,
            revalidate_checkpoint: false,
            identity_replaced: false,
            content_hash: None,
            source_file: None,
            source_handle_verified: false,
        };

        let outcome =
            prepare_for_apply_with_post_hash_hook(&db, dir.path(), None, prepared, |path| {
                std::fs::remove_file(path).unwrap();
                symlink(&outside_file, path).unwrap();
            })
            .unwrap();
        assert!(matches!(outcome, PrepareForApply::Skip));
    }

    #[cfg(unix)]
    #[test]
    fn full_scan_does_not_publish_an_outside_link_replacement_before_commit() {
        use std::os::unix::fs::symlink;

        let dir = tempdir().unwrap();
        let relative = Path::new("one.wav");
        let source = dir.path().join(relative);
        let outside = tempdir().unwrap();
        let outside_file = outside.path().join("outside.wav");
        std::fs::write(&source, b"inside").unwrap();
        std::fs::write(&outside_file, b"outside").unwrap();
        let db = SourceDatabase::open_for_scan(dir.path()).unwrap();
        let mut context = ScanContext::from_existing(
            HashMap::new(),
            ScanMode::Quick,
            db.get_revision().unwrap(),
            db.list_manifest_entries().unwrap(),
        );
        let source_root = SourceRootCapability::open(dir.path()).unwrap();
        let prepared = prepare_diff_from_capability(&source_root, dir.path(), relative, &context)
            .unwrap()
            .unwrap();

        let outcome = apply_batch_with_precommit_hook(
            &db,
            dir.path(),
            None,
            &mut context,
            vec![prepared],
            false,
            &super::super::scan_writer::UncoordinatedScanWriter,
            |path| {
                let absolute = dir.path().join(path);
                std::fs::remove_file(&absolute).unwrap();
                symlink(&outside_file, absolute).unwrap();
            },
        )
        .unwrap();

        assert!(!outcome.committed);
        assert!(context.source_tree_incomplete());
        assert!(db.entry_for_path(relative).unwrap().is_none());
    }

    #[test]
    fn scan_batch_rejects_a_changed_traversal_policy_before_commit() {
        let dir = tempdir().unwrap();
        let relative = Path::new("one.wav");
        let source = dir.path().join(relative);
        std::fs::write(&source, b"inside").unwrap();
        let db = SourceDatabase::open_for_scan(dir.path()).unwrap();
        let mut context = ScanContext::from_existing(
            HashMap::new(),
            ScanMode::Quick,
            db.get_revision().unwrap(),
            db.list_manifest_entries().unwrap(),
        );
        let source_root = SourceRootCapability::open(dir.path()).unwrap();
        let prepared = prepare_diff_from_capability(&source_root, dir.path(), relative, &context)
            .unwrap()
            .unwrap();
        db.set_source_traversal_policy(
            wavecrate_library::sample_sources::SourceTraversalPolicy::exclude_hidden_directories(),
        )
        .unwrap();

        let result = apply_batch(
            &db,
            dir.path(),
            None,
            &mut context,
            vec![prepared],
            false,
            &super::super::scan_writer::UncoordinatedScanWriter,
        );

        assert!(matches!(result, Err(ScanError::TraversalPolicyChanged)));
        assert!(db.entry_for_path(relative).unwrap().is_none());
    }

    #[cfg(unix)]
    #[test]
    fn targeted_sync_does_not_publish_an_outside_link_replacement_before_commit() {
        use std::os::unix::fs::symlink;

        let dir = tempdir().unwrap();
        let relative = Path::new("one.wav");
        let source = dir.path().join(relative);
        let outside = tempdir().unwrap();
        let outside_file = outside.path().join("outside.wav");
        std::fs::write(&source, b"inside").unwrap();
        std::fs::write(&outside_file, b"outside").unwrap();
        let db = SourceDatabase::open_for_scan(dir.path()).unwrap();
        let mut context = ScanContext::from_existing(
            HashMap::new(),
            ScanMode::Quick,
            db.get_revision().unwrap(),
            db.list_manifest_entries().unwrap(),
        );
        let prepared = prepare_diff(dir.path(), &source, &context).unwrap();

        let outcome = apply_batch_with_precommit_hook(
            &db,
            dir.path(),
            None,
            &mut context,
            vec![prepared],
            false,
            &super::super::scan_writer::UncoordinatedScanWriter,
            |path| {
                let absolute = dir.path().join(path);
                std::fs::remove_file(&absolute).unwrap();
                symlink(&outside_file, absolute).unwrap();
            },
        )
        .unwrap();

        assert!(!outcome.committed);
        assert!(context.source_tree_incomplete());
        assert!(db.entry_for_path(relative).unwrap().is_none());
    }

    #[cfg(unix)]
    #[test]
    fn targeted_apply_rejects_a_file_replaced_by_a_symlink_after_hashing() {
        use std::os::unix::fs::symlink;

        let dir = tempdir().unwrap();
        let source = dir.path().join("one.wav");
        let outside = tempdir().unwrap();
        let outside_file = outside.path().join("outside.wav");
        std::fs::write(&source, b"source").unwrap();
        std::fs::write(&outside_file, b"outside").unwrap();
        let expected = std::fs::File::open(&source).unwrap();

        std::fs::remove_file(&source).unwrap();
        symlink(&outside_file, &source).unwrap();

        assert!(matches!(
            super::SourceRootCapability::open(dir.path())
                .unwrap()
                .path_binding(Path::new("one.wav"), &expected)
                .unwrap(),
            super::SourcePathBinding::Retire
        ));
    }

    #[cfg(unix)]
    #[test]
    fn targeted_apply_rejects_a_descendant_of_a_replaced_directory() {
        use std::os::unix::fs::symlink;

        let dir = tempdir().unwrap();
        let nested = dir.path().join("nested");
        std::fs::create_dir(&nested).unwrap();
        let source = nested.join("one.wav");
        std::fs::write(&source, b"source").unwrap();
        let expected = std::fs::File::open(&source).unwrap();
        let outside = tempdir().unwrap();
        std::fs::write(outside.path().join("one.wav"), b"outside").unwrap();

        std::fs::rename(&nested, dir.path().join("moved")).unwrap();
        symlink(outside.path(), &nested).unwrap();

        assert!(matches!(
            super::SourceRootCapability::open(dir.path())
                .unwrap()
                .path_binding(Path::new("nested/one.wav"), &expected)
                .unwrap(),
            super::SourcePathBinding::Retire
        ));
    }

    #[cfg(unix)]
    #[test]
    fn targeted_apply_retires_a_file_replaced_by_a_link_before_commit() {
        use std::os::unix::fs::symlink;

        let dir = tempdir().unwrap();
        let source = dir.path().join("one.wav");
        let outside = tempdir().unwrap();
        let outside_file = outside.path().join("outside.wav");
        std::fs::write(&source, b"source").unwrap();
        std::fs::write(&outside_file, b"outside").unwrap();
        let db = SourceDatabase::open_for_scan(dir.path()).unwrap();
        scan_once(&db).unwrap();
        let entry = db.entry_for_path(Path::new("one.wav")).unwrap().unwrap();
        let mut context = ScanContext::from_existing(
            HashMap::from([(Path::new("one.wav").to_path_buf(), entry)]),
            ScanMode::Targeted,
            db.get_revision().unwrap(),
            db.list_manifest_entries().unwrap(),
        );
        let file = std::fs::File::open(&source).unwrap();
        let prepared = PreparedFile {
            facts: super::super::scan_fs::read_facts_from_open_file(dir.path(), &source, &file)
                .unwrap(),
            hash_required: false,
            needs_hash: false,
            requires_apply: true,
            revalidate_checkpoint: false,
            identity_replaced: false,
            content_hash: None,
            source_file: Some(file),
            source_handle_verified: true,
        };

        std::fs::remove_file(&source).unwrap();
        symlink(&outside_file, &source).unwrap();
        apply_batch(
            &db,
            dir.path(),
            None,
            &mut context,
            vec![prepared],
            false,
            &super::super::scan_writer::UncoordinatedScanWriter,
        )
        .unwrap();

        assert!(context.existing.contains_key(Path::new("one.wav")));
    }
}
