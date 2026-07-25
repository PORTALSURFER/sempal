#[cfg(test)]
use super::scan::ScanError;
use super::scan::{ScanContext, ScanMode};
use super::scan_diff::{PreparedFile, should_compute_full_hash};
use super::scan_fs::FileFacts;
#[cfg(test)]
use super::scan_fs::read_facts;
#[cfg(test)]
use std::path::Path;

#[cfg(test)]
pub(super) fn prepare_diff(
    root: &Path,
    path: &Path,
    context: &ScanContext,
) -> Result<PreparedFile, ScanError> {
    let facts = read_facts(root, path)?;
    Ok(prepare_diff_from_facts(facts, context))
}

pub(super) fn prepare_diff_from_facts(facts: FileFacts, context: &ScanContext) -> PreparedFile {
    let existing = context.existing.get(&facts.relative);
    let revalidate_checkpoint = context.manifest_audit_revalidates_path(&facts.relative);
    let persisted_identity = context
        .committed_file_identity(&facts.relative)
        .map(str::trim)
        .filter(|identity| !identity.is_empty());
    let observed_identity = facts
        .file_identity
        .as_deref()
        .map(str::trim)
        .filter(|identity| !identity.is_empty());
    let identity_requires_update =
        observed_identity.is_some_and(|identity| persisted_identity != Some(identity));
    let identity_replaced = persisted_identity
        .zip(observed_identity)
        .is_some_and(|(previous, current)| previous != current);
    let force_targeted_verification = context.mode == ScanMode::Targeted;
    let hash_required = should_compute_full_hash(context.mode, facts.size)
        || (force_targeted_verification && existing.is_some())
        || revalidate_checkpoint;
    let needs_hash = hash_required
        && existing.is_none_or(|entry| {
            force_targeted_verification
                || revalidate_checkpoint
                || entry.file_size != facts.size
                || entry.modified_ns != facts.modified_ns
                || entry.content_hash.is_none()
                || identity_replaced
        });
    let requires_apply = existing.is_none_or(|entry| {
        force_targeted_verification
            || revalidate_checkpoint
            || entry.file_size != facts.size
            || entry.modified_ns != facts.modified_ns
            || entry.missing
            || (entry.content_hash.is_none() && needs_hash)
            || identity_requires_update
    });
    PreparedFile {
        facts,
        hash_required,
        needs_hash,
        requires_apply,
        revalidate_checkpoint,
        identity_replaced,
        content_hash: None,
        source_file: None,
        source_handle_verified: false,
    }
}
