use std::{collections::BTreeSet, path::PathBuf};

use wavecrate_library::sample_sources::{
    SOURCE_FORMAT_POLICY_VERSION, SourceFileClassification, SourceIndexClassification,
    SourceIndexDiagnostic, SourceIndexEntry,
};

use crate::sample_sources::SourceDatabase;

use super::scan::{ScanContext, ScanError};
use super::scan_capability::SourceRootCapability;
use super::scan_writer::{ScanWritePhase, ScanWriter};

pub(super) fn index_entry_from_file_facts(
    relative_path: PathBuf,
    classification: SourceFileClassification,
    file_size: u64,
    modified_ns: i64,
    file_identity: Option<String>,
) -> Option<SourceIndexEntry> {
    let (classification, diagnostic) = match classification {
        SourceFileClassification::UnsupportedAudio => {
            (SourceIndexClassification::UnsupportedAudio, None)
        }
        SourceFileClassification::UnsupportedNonAudio => {
            (SourceIndexClassification::UnsupportedNonAudio, None)
        }
        SourceFileClassification::PracticallyUnsupportedAudio => (
            SourceIndexClassification::PracticallyUnsupportedAudio,
            Some(SourceIndexDiagnostic::PracticalSupportLimit),
        ),
        SourceFileClassification::SupportedAudio => return None,
    };
    Some(SourceIndexEntry {
        relative_path,
        classification,
        file_size: Some(file_size),
        modified_ns: Some(modified_ns),
        file_identity,
        diagnostic,
        format_policy_version: SOURCE_FORMAT_POLICY_VERSION,
    })
}

pub(super) fn inaccessible_index_entry(
    relative_path: PathBuf,
    diagnostic: SourceIndexDiagnostic,
) -> SourceIndexEntry {
    SourceIndexEntry {
        relative_path,
        classification: SourceIndexClassification::Inaccessible,
        file_size: None,
        modified_ns: None,
        file_identity: None,
        diagnostic: Some(diagnostic),
        format_policy_version: SOURCE_FORMAT_POLICY_VERSION,
    }
}

pub(super) fn non_unicode_index_entry(
    relative_path: PathBuf,
    file_size: Option<u64>,
    modified_ns: Option<i64>,
    file_identity: Option<String>,
) -> SourceIndexEntry {
    SourceIndexEntry {
        relative_path,
        classification: SourceIndexClassification::Inaccessible,
        file_size,
        modified_ns,
        file_identity,
        diagnostic: Some(SourceIndexDiagnostic::NonUnicodePath),
        format_policy_version: SOURCE_FORMAT_POLICY_VERSION,
    }
}

pub(super) fn reconcile_index_entries(
    database: &SourceDatabase,
    source_root: &SourceRootCapability,
    context: &mut ScanContext,
    writer: &impl ScanWriter,
) -> Result<(), ScanError> {
    let (existing, observed) = context.take_index_reconciliation();
    let unavailable_manifest_paths = observed
        .values()
        .filter(|entry| entry.classification == SourceIndexClassification::Inaccessible)
        .map(|entry| &entry.relative_path)
        .filter(|path| context.has_committed_manifest_path(path))
        .cloned()
        .collect::<BTreeSet<_>>();

    let removals = existing
        .keys()
        .filter(|path| {
            (context.has_committed_manifest_path(path)
                && !unavailable_manifest_paths.contains(*path))
                || (!observed.contains_key(*path) && !context.preserves_missing_row(path))
        })
        .cloned()
        .collect::<Vec<_>>();
    let upserts = observed
        .into_values()
        .filter(|entry| {
            !context.has_committed_manifest_path(&entry.relative_path)
                || entry.classification == SourceIndexClassification::Inaccessible
        })
        .filter(|entry| existing.get(&entry.relative_path) != Some(entry))
        .collect::<Vec<SourceIndexEntry>>();

    if removals.is_empty() && upserts.is_empty() && unavailable_manifest_paths.is_empty() {
        return Ok(());
    }
    source_root.ensure_current_generation()?;
    let _writer = writer.lock(ScanWritePhase::Manifest);
    let mut batch = database.write_batch()?;
    for path in &unavailable_manifest_paths {
        batch.set_missing(path, true)?;
        context.stats.missing = context.stats.missing.saturating_add(1);
    }
    for path in removals {
        batch.remove_source_index_entry(&path)?;
    }
    for entry in upserts {
        batch.upsert_source_index_entry(&entry)?;
    }
    source_root.ensure_current_generation()?;
    source_root.ensure_current_generation()?;
    // Index-only facts are source projection facts, not auxiliary metadata. Route every
    // mutation through the bounded source commit so the scanner can publish the exact
    // revision only after this transaction commits.
    context.commit_batch(database, batch)?;
    Ok(())
}
