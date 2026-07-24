use std::collections::{HashMap, HashSet, VecDeque};
use std::io::{Seek, SeekFrom};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

use crate::sample_sources::SourceDatabase;
use crate::sample_sources::db::{
    ContentAuditForwardCandidate, ContentAuditRetryCandidate, ContentAuditSkipReason,
    PendingRenameEntry, SourceWriteBatch, WavEntry,
};
use wavecrate_library::sample_sources::SourceManifestEntry;

use super::scan::{ChangedSample, RenamedSample, ScanError, ScanStats, UpdatedSample};
use super::scan_capability::{SourcePathBinding, SourceRootCapability};
#[cfg(test)]
use super::scan_fs::read_facts;
use super::scan_fs::{
    FileFacts, compute_content_hash_with_reader, ensure_root_dir, read_facts_from_open_file,
};
use super::scan_writer::{ScanWritePhase, ScanWriter, UncoordinatedScanWriter};

#[derive(Debug)]
struct HashBackfill {
    relative_path: PathBuf,
    facts: FileFacts,
    file_size: u64,
    modified_ns: i64,
    content_hash: String,
    file_identity: Option<String>,
    file: std::fs::File,
}

#[derive(Debug)]
struct RetainedRenameCandidate {
    relative_path: PathBuf,
    facts: FileFacts,
}

#[derive(Debug)]
struct VerifiedContentAudit {
    previous: SourceManifestEntry,
    facts: FileFacts,
    content_hash: String,
    file: std::fs::File,
}

const CONTENT_AUDIT_RETRY_SECONDS: i64 = 15 * 60;
/// Maximum number of source descriptors retained before a hash publication checkpoint.
///
/// This is deliberately independent from the audit's time, byte, and entry budgets. Keeping
/// this window small makes the scanner safe on Windows and low-ulimit Unix hosts while retaining
/// the existing resource budgets for throughput and coverage.
const HASH_DESCRIPTOR_WINDOW: usize = 64;

struct ContentAuditWindow {
    stats: ScanStats,
    attempted_entries: usize,
    attempted_bytes: u64,
    completed_rotation: bool,
}

struct DeepHashWindow {
    stats: ScanStats,
    retained_descriptors: usize,
    last_examined_path: Option<PathBuf>,
    exhausted: bool,
}

enum PlannedContentAuditEntry {
    Forward(ContentAuditForwardCandidate),
    Retry(ContentAuditRetryCandidate),
}

/// Resource ceilings for one resumable content-verification batch.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ContentAuditBudget {
    /// Maximum wall time for one admitted slice.
    pub max_elapsed: Duration,
    /// Maximum bytes hashed by one admitted slice, after allowing one oversize file.
    pub max_bytes: u64,
    /// Maximum entries attempted by one admitted slice.
    pub max_entries: usize,
    /// Desired maximum age of a complete verification rotation.
    pub target_coverage_age: Duration,
    /// Portion of the entry ceiling reserved for due retries.
    pub retry_entries: usize,
}

impl ContentAuditBudget {
    /// Build a compatibility budget bounded only by entry count.
    pub fn entry_limited(max_entries: usize) -> Self {
        Self {
            max_elapsed: Duration::MAX,
            max_bytes: u64::MAX,
            max_entries,
            target_coverage_age: Duration::from_secs(30 * 24 * 60 * 60),
            retry_entries: max_entries.div_ceil(4).max(1),
        }
    }

    /// Derive a finite daily slice from source scale while retaining hard time, byte, and entry
    /// ceilings. Playback/foreground work and slower source classes use the conservative lane.
    pub fn adaptive(
        total_entries: usize,
        activity: ContentAuditActivity,
        storage: ContentAuditStorage,
        accelerated: bool,
    ) -> Self {
        Self::adaptive_for_target(
            total_entries,
            activity,
            storage,
            accelerated,
            Duration::from_secs(30 * 24 * 60 * 60),
        )
    }

    /// Derive an adaptive slice for an explicit content-coverage objective.
    pub fn adaptive_for_target(
        total_entries: usize,
        activity: ContentAuditActivity,
        storage: ContentAuditStorage,
        accelerated: bool,
        target_coverage_age: Duration,
    ) -> Self {
        const AUDIT_INTERVAL_SECONDS: u64 = 24 * 60 * 60;
        let target_intervals = usize::try_from(
            target_coverage_age
                .as_secs()
                .div_ceil(AUDIT_INTERVAL_SECONDS)
                .max(1),
        )
        .unwrap_or(usize::MAX);
        let desired_entries = total_entries.div_ceil(target_intervals).max(1);
        let constrained = activity.playback_active
            || activity.foreground_active
            || storage == ContentAuditStorage::ExternalOrNetwork;
        let (max_elapsed, max_bytes, hard_entry_cap) = match (constrained, accelerated) {
            (true, false) => (Duration::from_secs(1), 64 * 1024 * 1024, 1_024),
            (true, true) => (Duration::from_secs(2), 128 * 1024 * 1024, 2_048),
            (false, false) => (Duration::from_secs(5), 512 * 1024 * 1024, 4_096),
            (false, true) => (Duration::from_secs(10), 1024 * 1024 * 1024, 8_192),
        };
        let minimum_entries = if total_entries > 1 { 2 } else { 1 };
        let max_entries = desired_entries
            .saturating_mul(if accelerated { 4 } else { 1 })
            .clamp(minimum_entries, hard_entry_cap);
        Self {
            max_elapsed,
            max_bytes,
            max_entries,
            target_coverage_age,
            retry_entries: max_entries.div_ceil(4).max(1),
        }
    }
}

/// Foreground resource activity used to select conservative hashing ceilings.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct ContentAuditActivity {
    /// Whether playback is actively consuming source resources.
    pub playback_active: bool,
    /// Whether foreground browsing/loading is active.
    pub foreground_active: bool,
}

/// Coarse storage class used to avoid aggressive hashing on slow sources.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum ContentAuditStorage {
    /// A source on the normal local storage path.
    #[default]
    Local,
    /// A removable, mounted, or network-addressed source.
    ExternalOrNetwork,
}

impl ContentAuditStorage {
    /// Conservatively classify a source root from platform mount information.
    pub fn classify(root: &std::path::Path) -> Self {
        #[cfg(target_os = "windows")]
        {
            classify_windows_storage(root)
        }
        #[cfg(target_os = "macos")]
        {
            classify_macos_storage(root)
        }
        #[cfg(not(any(target_os = "windows", target_os = "macos")))]
        {
            let _ = root;
            classify_unknown_storage()
        }
    }
}

#[cfg(target_os = "macos")]
fn classify_macos_storage(root: &std::path::Path) -> ContentAuditStorage {
    use std::os::unix::fs::MetadataExt;

    let Ok(source) = std::fs::metadata(root) else {
        return ContentAuditStorage::ExternalOrNetwork;
    };
    let Ok(local_anchor) = std::fs::metadata("/Users") else {
        return ContentAuditStorage::ExternalOrNetwork;
    };
    classify_macos_device(source.dev(), local_anchor.dev())
}

#[cfg(any(target_os = "macos", test))]
fn classify_macos_device(source_device: u64, local_device: u64) -> ContentAuditStorage {
    if source_device == local_device {
        ContentAuditStorage::Local
    } else {
        ContentAuditStorage::ExternalOrNetwork
    }
}

#[cfg(any(not(any(target_os = "windows", target_os = "macos")), test))]
fn classify_unknown_storage() -> ContentAuditStorage {
    ContentAuditStorage::ExternalOrNetwork
}

#[cfg(any(target_os = "windows", test))]
const WINDOWS_DRIVE_REMOVABLE: u32 = 2;
#[cfg(any(target_os = "windows", test))]
const WINDOWS_DRIVE_FIXED: u32 = 3;
#[cfg(any(target_os = "windows", test))]
const WINDOWS_DRIVE_REMOTE: u32 = 4;

#[cfg(any(target_os = "windows", test))]
fn classify_windows_drive_type(drive_type: u32) -> ContentAuditStorage {
    match drive_type {
        WINDOWS_DRIVE_FIXED => ContentAuditStorage::Local,
        WINDOWS_DRIVE_REMOVABLE | WINDOWS_DRIVE_REMOTE => ContentAuditStorage::ExternalOrNetwork,
        _ => ContentAuditStorage::ExternalOrNetwork,
    }
}

#[cfg(target_os = "windows")]
fn classify_windows_storage(root: &std::path::Path) -> ContentAuditStorage {
    use std::os::windows::ffi::OsStrExt;
    use std::path::{Component, Prefix};
    use windows::Win32::Storage::FileSystem::GetDriveTypeW;
    use windows::core::PCWSTR;

    if root.to_string_lossy().starts_with(r"\\") {
        return ContentAuditStorage::ExternalOrNetwork;
    }
    let Some(Component::Prefix(prefix)) = root.components().next() else {
        return ContentAuditStorage::ExternalOrNetwork;
    };
    let drive = match prefix.kind() {
        Prefix::Disk(drive) | Prefix::VerbatimDisk(drive) => drive,
        _ => return ContentAuditStorage::ExternalOrNetwork,
    };
    let drive_root = std::ffi::OsString::from(format!("{}:\\", char::from(drive)));
    let mut wide = drive_root.encode_wide().collect::<Vec<_>>();
    wide.push(0);
    // SAFETY: `wide` is a live, nul-terminated UTF-16 root path for the duration of the call.
    let drive_type = unsafe { GetDriveTypeW(PCWSTR(wide.as_ptr())) };
    classify_windows_drive_type(drive_type)
}

fn cancel_requested(cancel: Option<&AtomicBool>) -> bool {
    cancel.is_some_and(|cancel| cancel.load(Ordering::Relaxed))
}

#[cfg(test)]
pub(super) fn verify_content_batch(
    db: &SourceDatabase,
    cancel: Option<&AtomicBool>,
    budget: ContentAuditBudget,
    now: i64,
) -> Result<ScanStats, ScanError> {
    verify_content_batch_with_writer(db, cancel, budget, now, &UncoordinatedScanWriter)
}

#[cfg(test)]
pub(super) fn verify_content_batch_with_writer(
    db: &SourceDatabase,
    cancel: Option<&AtomicBool>,
    budget: ContentAuditBudget,
    now: i64,
    writer: &impl ScanWriter,
) -> Result<ScanStats, ScanError> {
    verify_content_batch_with_post_hash_hook(db, cancel, budget, now, writer, |_| {})
}

pub(super) fn verify_content_batch_with_source_root(
    db: &SourceDatabase,
    source_root: &SourceRootCapability,
    cancel: Option<&AtomicBool>,
    budget: ContentAuditBudget,
    now: i64,
    writer: &impl ScanWriter,
) -> Result<ScanStats, ScanError> {
    let root = db.root().to_path_buf();
    let started = Instant::now();
    verify_content_batch_with_source_root_hooks(
        db,
        &root,
        source_root,
        cancel,
        budget,
        now,
        writer,
        &mut |_| {},
        &mut |_| {},
        &mut || started.elapsed(),
    )
}

#[cfg(test)]
fn verify_content_batch_with_post_hash_hook(
    db: &SourceDatabase,
    cancel: Option<&AtomicBool>,
    budget: ContentAuditBudget,
    now: i64,
    writer: &impl ScanWriter,
    mut post_hash: impl FnMut(&std::path::Path),
) -> Result<ScanStats, ScanError> {
    let started = Instant::now();
    verify_content_batch_with_hooks(
        db,
        cancel,
        budget,
        now,
        writer,
        &mut post_hash,
        &mut |_| {},
        &mut || started.elapsed(),
    )
}

#[cfg(test)]
fn verify_content_batch_with_hooks(
    db: &SourceDatabase,
    cancel: Option<&AtomicBool>,
    budget: ContentAuditBudget,
    now: i64,
    writer: &impl ScanWriter,
    post_hash: &mut impl FnMut(&std::path::Path),
    precommit: &mut impl FnMut(&std::path::Path),
    elapsed: &mut impl FnMut() -> Duration,
) -> Result<ScanStats, ScanError> {
    let root = ensure_root_dir(db)?;
    let source_root = SourceRootCapability::open(&root)?;
    verify_content_batch_with_source_root_hooks(
        db,
        &root,
        &source_root,
        cancel,
        budget,
        now,
        writer,
        post_hash,
        precommit,
        elapsed,
    )
}

#[allow(clippy::too_many_arguments)]
fn verify_content_batch_with_source_root_hooks(
    db: &SourceDatabase,
    root: &Path,
    source_root: &SourceRootCapability,
    cancel: Option<&AtomicBool>,
    budget: ContentAuditBudget,
    now: i64,
    writer: &impl ScanWriter,
    post_hash: &mut impl FnMut(&std::path::Path),
    precommit: &mut impl FnMut(&std::path::Path),
    elapsed: &mut impl FnMut() -> Duration,
) -> Result<ScanStats, ScanError> {
    let mut combined = ScanStats::default();
    let mut remaining_entries = budget.max_entries;
    let mut remaining_bytes = budget.max_bytes;
    let mut first_window = true;
    let mut stale_replayed = false;

    loop {
        let window_budget = ContentAuditBudget {
            max_elapsed: budget.max_elapsed.saturating_sub(elapsed()),
            max_bytes: remaining_bytes,
            max_entries: remaining_entries.min(HASH_DESCRIPTOR_WINDOW),
            target_coverage_age: budget.target_coverage_age,
            retry_entries: budget
                .retry_entries
                .min(remaining_entries.min(HASH_DESCRIPTOR_WINDOW)),
        };
        let window = verify_content_batch_window_with_source_root_hooks(
            db,
            root,
            source_root,
            cancel,
            window_budget,
            now,
            writer,
            post_hash,
            precommit,
            elapsed,
        );
        match window {
            Ok(window) => {
                stale_replayed = false;
                remaining_entries = remaining_entries.saturating_sub(window.attempted_entries);
                remaining_bytes = remaining_bytes.saturating_sub(window.attempted_bytes);
                combined.merge_deferred_hashes(window.stats);
                if window.attempted_entries == 0
                    || window.completed_rotation
                    || remaining_entries == 0
                    || remaining_bytes == 0
                    || elapsed() >= budget.max_elapsed
                {
                    return Ok(combined);
                }
                first_window = false;
            }
            Err(ScanError::Incomplete { committed, error }) => {
                combined.merge_deferred_hashes(*committed);
                return Err(ScanError::Incomplete {
                    committed: Box::new(combined),
                    error,
                });
            }
            Err(ScanError::StaleRevision { .. }) if !first_window && !stale_replayed => {
                stale_replayed = true;
                continue;
            }
            Err(error) => {
                if !first_window {
                    tracing::debug!(%error, "Hash audit window stopped after earlier checkpoints");
                }
                return Err(error);
            }
        }
    }
}

#[allow(clippy::too_many_arguments)]
fn verify_content_batch_window_with_source_root_hooks(
    db: &SourceDatabase,
    root: &Path,
    source_root: &SourceRootCapability,
    cancel: Option<&AtomicBool>,
    budget: ContentAuditBudget,
    now: i64,
    writer: &impl ScanWriter,
    post_hash: &mut impl FnMut(&std::path::Path),
    precommit: &mut impl FnMut(&std::path::Path),
    elapsed: &mut impl FnMut() -> Duration,
) -> Result<ContentAuditWindow, ScanError> {
    let planning_started = Instant::now();
    let manifest_revision = db.get_revision()?;
    source_root.ensure_current_generation()?;
    let checkpoint = {
        let _writer = writer.lock(ScanWritePhase::Manifest);
        db.begin_or_resume_content_audit(now, manifest_revision)?
    };
    let forward = db.content_audit_forward_candidates(
        checkpoint.rotation_id,
        &checkpoint.cursor,
        budget.max_entries,
    )?;
    let forward_is_due = forward.iter().any(|candidate| candidate.needs_verification);
    let retry_limit = budget.retry_entries.min(
        budget
            .max_entries
            .saturating_sub(usize::from(forward_is_due)),
    );
    let retry = db.content_audit_retry_candidates(&checkpoint.retry_cursor, now, retry_limit)?;
    let planned_forward = forward.len();
    let planned_retries = retry.len();
    let mut selected = Vec::with_capacity(retry.len() + forward.len());
    let mut retry = VecDeque::from(retry);
    let mut forward = VecDeque::from(forward);
    let mut planned_retry_next = checkpoint.retry_next;
    while selected.len() < budget.max_entries && (!retry.is_empty() || !forward.is_empty()) {
        let take_retry = !retry.is_empty() && (planned_retry_next || forward.is_empty());
        if take_retry {
            let entry = retry.pop_front().expect("retry queue is non-empty");
            selected.push(PlannedContentAuditEntry::Retry(entry));
            planned_retry_next = false;
        } else if let Some(entry) = forward.pop_front() {
            selected.push(PlannedContentAuditEntry::Forward(entry));
            planned_retry_next = true;
        }
    }
    tracing::debug!(
        manifest_revision,
        rotation_id = checkpoint.rotation_id,
        admitted_entries = budget.max_entries,
        planned_forward,
        planned_retries,
        selected_entries = selected.len(),
        planning_elapsed_us = planning_started.elapsed().as_micros().min(u64::MAX as u128) as u64,
        "Planned bounded source content-audit slice"
    );
    let mut stats = ScanStats::default();
    let mut verified = Vec::new();
    let mut skipped = Vec::new();
    let mut attempted_bytes = 0_u64;
    let mut last_forward = None;
    let mut last_retry = None;
    let mut retry_next = checkpoint.retry_next;
    let mut stale_retries = Vec::new();
    let mut cursor_only_progress = false;
    let mut attempted_entries = 0;
    for planned in &selected {
        if cancel_requested(cancel) {
            break;
        }
        attempted_entries += 1;
        let (entry, is_retry) = match planned {
            PlannedContentAuditEntry::Forward(candidate) => {
                if !candidate.needs_verification || candidate.retry_pending {
                    last_forward = Some(candidate.entry.relative_path.clone());
                    retry_next = true;
                    cursor_only_progress = true;
                    continue;
                }
                (&candidate.entry, false)
            }
            PlannedContentAuditEntry::Retry(candidate) => {
                let Some(entry) = candidate.entry.as_ref() else {
                    last_retry = Some(candidate.relative_path.clone());
                    retry_next = false;
                    stale_retries.push(candidate.relative_path.clone());
                    cursor_only_progress = true;
                    continue;
                };
                (entry, true)
            }
        };
        if !verified.is_empty() || !skipped.is_empty() {
            if elapsed() >= budget.max_elapsed
                || attempted_bytes.saturating_add(entry.file_size) > budget.max_bytes
            {
                break;
            }
        }
        if is_retry {
            last_retry = Some(entry.relative_path.clone());
            retry_next = false;
        } else {
            last_forward = Some(entry.relative_path.clone());
            retry_next = true;
        }
        let absolute = root.join(&entry.relative_path);
        let mut file = match source_root.open_regular_file(&entry.relative_path) {
            Ok(Some(file)) => file,
            Ok(None) => {
                let reason = if source_root
                    .entry_exists_nofollow(&entry.relative_path)
                    .unwrap_or(false)
                {
                    ContentAuditSkipReason::Unsupported
                } else {
                    ContentAuditSkipReason::Unavailable
                };
                skipped.push((entry.clone(), reason, 0));
                continue;
            }
            Err(_) => {
                skipped.push((entry.clone(), ContentAuditSkipReason::Unavailable, 0));
                continue;
            }
        };
        let before_hash = match read_facts_from_open_file(&root, &absolute, &file) {
            Ok(facts) => facts,
            Err(_) => {
                skipped.push((entry.clone(), ContentAuditSkipReason::Unavailable, 0));
                continue;
            }
        };
        if let Err(source) = file.seek(SeekFrom::Start(0)) {
            skipped.push((entry.clone(), ContentAuditSkipReason::HashFailed, 0));
            tracing::warn!(
                path = %absolute.display(),
                error = %source,
                "Source content audit could not rewind its retained descriptor"
            );
            continue;
        }
        let content_hash = match compute_content_hash_with_reader(&absolute, &mut file, cancel) {
            Ok(hash) => hash,
            Err(ScanError::Canceled) => break,
            Err(_) => {
                attempted_bytes = attempted_bytes.saturating_add(before_hash.size);
                skipped.push((
                    entry.clone(),
                    ContentAuditSkipReason::HashFailed,
                    before_hash.size,
                ));
                continue;
            }
        };
        post_hash(&absolute);
        attempted_bytes = attempted_bytes.saturating_add(before_hash.size);
        let after_hash = match read_facts_from_open_file(&root, &absolute, &file) {
            Ok(facts) => facts,
            Err(_) => {
                skipped.push((
                    entry.clone(),
                    ContentAuditSkipReason::Unavailable,
                    before_hash.size,
                ));
                continue;
            }
        };
        if !before_hash.same_content_snapshot(&after_hash) {
            skipped.push((
                entry.clone(),
                ContentAuditSkipReason::ChangedDuringHash,
                before_hash.size,
            ));
            continue;
        }
        verified.push(VerifiedContentAudit {
            previous: entry.clone(),
            facts: after_hash,
            content_hash,
            file,
        });
        stats.hashes_computed += 1;
    }
    let cancelled = cancel_requested(cancel);
    if !verified.is_empty() || !skipped.is_empty() || cursor_only_progress {
        source_root.ensure_current_generation()?;
        let _writer = writer.lock(ScanWritePhase::DeferredHash);
        let mut batch = db.write_batch()?;
        let stats_before_staging = stats.clone();
        if !batch.matches_revision(manifest_revision)? {
            return Err(ScanError::StaleRevision {
                expected: manifest_revision,
                actual: db.get_revision()?,
            });
        }
        let mut committed_verified = Vec::with_capacity(verified.len());
        for verified in verified {
            let absolute = root.join(&verified.previous.relative_path);
            let descriptor_still_current =
                read_facts_from_open_file(&root, &absolute, &verified.file)
                    .is_ok_and(|current| verified.facts.same_content_snapshot(&current));
            let binding =
                source_root.path_binding(&verified.previous.relative_path, &verified.file);
            match (descriptor_still_current, binding) {
                (true, Ok(SourcePathBinding::Matches)) => committed_verified.push(verified),
                (_, Ok(SourcePathBinding::Retire)) | (_, Err(_)) => {
                    skipped.push((verified.previous, ContentAuditSkipReason::Unavailable, 0))
                }
                _ => skipped.push((
                    verified.previous,
                    ContentAuditSkipReason::ChangedDuringHash,
                    0,
                )),
            }
        }
        let mut changed_before = Vec::new();
        for verified in &committed_verified {
            let previous = &verified.previous;
            let facts = &verified.facts;
            let content_hash = &verified.content_hash;
            batch.record_content_audit_verified(
                &previous.relative_path,
                checkpoint.rotation_id,
                now,
                facts.size,
                facts.modified_ns,
                facts.file_identity.as_deref(),
            )?;
            if previous.content_hash.as_deref() == Some(content_hash.as_str())
                && previous.file_size == facts.size
                && previous.modified_ns == facts.modified_ns
                && previous.file_identity == facts.file_identity
            {
                continue;
            }
            changed_before.push(previous.clone());
            if previous.file_identity != facts.file_identity {
                tracing::debug!(
                    path = %previous.relative_path.display(),
                    previous_identity = ?previous.file_identity,
                    current_identity = ?facts.file_identity,
                    "Source content audit refreshed filesystem identity"
                );
            }
            batch.upsert_file_with_hash(
                &previous.relative_path,
                facts.size,
                facts.modified_ns,
                content_hash,
            )?;
            batch.set_file_identity(&previous.relative_path, facts.file_identity.as_deref())?;
            stats.updated += 1;
            stats.updated_samples.push(UpdatedSample {
                relative_path: previous.relative_path.clone(),
                file_size: facts.size,
                modified_ns: facts.modified_ns,
                content_hash: Some(content_hash.clone()),
            });
            if previous.content_hash.as_deref() != Some(content_hash.as_str()) {
                stats.content_changed += 1;
                stats.changed_samples.push(ChangedSample {
                    relative_path: previous.relative_path.clone(),
                    file_size: facts.size,
                    modified_ns: facts.modified_ns,
                    content_hash: content_hash.clone(),
                });
            }
        }
        for (entry, reason, bytes_read) in &skipped {
            batch.record_content_audit_skipped(
                &entry.relative_path,
                now,
                now.saturating_add(CONTENT_AUDIT_RETRY_SECONDS),
                *reason,
                *bytes_read,
            )?;
        }
        for path in &stale_retries {
            batch.remove_stale_content_audit_entry(path)?;
        }
        batch.checkpoint_content_audit(
            last_forward.as_deref(),
            last_retry.as_deref(),
            retry_next,
            manifest_revision,
            attempted_bytes,
            now,
        )?;
        for verified in &committed_verified {
            precommit(&verified.previous.relative_path);
        }
        let final_bindings_match = committed_verified.iter().all(|verified| {
            let absolute = root.join(&verified.previous.relative_path);
            let descriptor_matches = read_facts_from_open_file(&root, &absolute, &verified.file)
                .is_ok_and(|current| verified.facts.same_content_snapshot(&current));
            descriptor_matches
                && matches!(
                    source_root.path_binding(&verified.previous.relative_path, &verified.file),
                    Ok(SourcePathBinding::Matches)
                )
        });
        if !final_bindings_match {
            drop(batch);
            stats = stats_before_staging;
            stats.committed_delta.revision = db.get_revision()?;
            stats.content_audit = Some(db.content_audit_report(now)?);
            return if cancelled {
                Err(ScanError::Incomplete {
                    committed: Box::new(stats),
                    error: ScanError::Canceled.to_string(),
                })
            } else {
                Ok(ContentAuditWindow {
                    stats,
                    attempted_entries,
                    attempted_bytes,
                    completed_rotation: false,
                })
            };
        }
        source_root.ensure_current_generation()?;
        let result = batch.commit_with_bounded_manifest_changes(manifest_revision)?;
        let changed_after = result
            .touched_path_changes
            .into_iter()
            .filter_map(|(_, entry)| entry)
            .collect::<Vec<_>>();
        stats.committed_delta = super::manifest::build_committed_delta(
            &changed_before,
            &changed_after,
            result.revision,
        );
        stats.manifest_updates = changed_after;
    } else {
        stats.committed_delta.revision = db.get_revision()?;
    };
    let report = {
        let _writer = writer.lock(ScanWritePhase::Manifest);
        db.content_audit_report(now)?
    };
    let completed_rotation = report.remaining_entries == 0 && report.total_entries > 0;
    if completed_rotation {
        source_root.ensure_current_generation()?;
        let _writer = writer.lock(ScanWritePhase::Manifest);
        let mut batch = db.write_batch()?;
        if !batch.matches_revision(stats.committed_delta.revision)? {
            return Err(ScanError::StaleRevision {
                expected: stats.committed_delta.revision,
                actual: db.get_revision()?,
            });
        }
        batch.complete_content_audit_rotation(now, stats.committed_delta.revision)?;
        let result = batch.commit_with_bounded_manifest_changes(stats.committed_delta.revision)?;
        debug_assert!(result.touched_path_changes.is_empty());
        stats.committed_delta.revision = result.revision;
    }
    stats.content_audit = Some({
        let _writer = writer.lock(ScanWritePhase::Manifest);
        db.content_audit_report(now)?
    });
    if cancelled {
        Err(ScanError::Incomplete {
            committed: Box::new(stats),
            error: ScanError::Canceled.to_string(),
        })
    } else {
        Ok(ContentAuditWindow {
            stats,
            attempted_entries,
            attempted_bytes,
            completed_rotation,
        })
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum DeferredHashScope {
    AllUnhashed,
    RenameCandidates,
}

pub(super) fn deep_hash_scan(
    db: &SourceDatabase,
    cancel: Option<&AtomicBool>,
    rename_candidates: &HashSet<PathBuf>,
    scope: DeferredHashScope,
    max_hashes: Option<usize>,
    exact_path: Option<&std::path::Path>,
) -> Result<ScanStats, ScanError> {
    deep_hash_scan_with_writer(
        db,
        cancel,
        rename_candidates,
        scope,
        max_hashes,
        exact_path,
        &UncoordinatedScanWriter,
    )
}

pub(super) fn deep_hash_scan_with_writer(
    db: &SourceDatabase,
    cancel: Option<&AtomicBool>,
    rename_candidates: &HashSet<PathBuf>,
    scope: DeferredHashScope,
    max_hashes: Option<usize>,
    exact_path: Option<&std::path::Path>,
    writer: &impl ScanWriter,
) -> Result<ScanStats, ScanError> {
    deep_hash_scan_with_post_hash_hook(
        db,
        cancel,
        rename_candidates,
        scope,
        max_hashes,
        exact_path,
        writer,
        |_| {},
    )
}

pub(super) fn deep_hash_scan_with_source_root_and_writer(
    db: &SourceDatabase,
    source_root: &SourceRootCapability,
    cancel: Option<&AtomicBool>,
    rename_candidates: &HashSet<PathBuf>,
    scope: DeferredHashScope,
    max_hashes: Option<usize>,
    exact_path: Option<&std::path::Path>,
    writer: &impl ScanWriter,
) -> Result<ScanStats, ScanError> {
    let root = db.root().to_path_buf();
    source_root.ensure_current_generation()?;
    deep_hash_scan_with_root_hooks(
        db,
        &root,
        source_root,
        cancel,
        rename_candidates,
        scope,
        max_hashes,
        exact_path,
        writer,
        |_| {},
        |_| {},
    )
}

#[cfg(test)]
pub(super) fn reconcile_hashed_rename_candidates_with_writer(
    db: &SourceDatabase,
    rename_candidates: &HashSet<PathBuf>,
    cancel: Option<&AtomicBool>,
    writer: &impl ScanWriter,
) -> Result<Vec<RenamedSample>, ScanError> {
    let root = ensure_root_dir(db)?;
    let source_root = SourceRootCapability::open(&root)?;
    Ok(
        reconcile_hashed_rename_candidates_with_source_root_and_hook(
            db,
            &root,
            &source_root,
            rename_candidates,
            cancel,
            writer,
            |_| {},
        )?
        .renamed_samples,
    )
}

pub(super) struct RenameReconciliation {
    pub(super) renamed_samples: Vec<RenamedSample>,
    pub(super) committed_revision: Option<u64>,
    pub(super) targeted_manifest_rows_read: usize,
    pub(super) targeted_manifest_query_count: usize,
}

pub(super) fn reconcile_hashed_rename_candidates_with_source_root_and_writer(
    db: &SourceDatabase,
    source_root: &SourceRootCapability,
    rename_candidates: &HashSet<PathBuf>,
    cancel: Option<&AtomicBool>,
    writer: &impl ScanWriter,
) -> Result<RenameReconciliation, ScanError> {
    let root = db.root().to_path_buf();
    reconcile_hashed_rename_candidates_with_source_root_and_hook(
        db,
        &root,
        source_root,
        rename_candidates,
        cancel,
        writer,
        |_| {},
    )
}

#[cfg(test)]
fn reconcile_hashed_rename_candidates_with_hook(
    db: &SourceDatabase,
    rename_candidates: &HashSet<PathBuf>,
    cancel: Option<&AtomicBool>,
    writer: &impl ScanWriter,
    precommit: impl FnMut(&std::path::Path),
) -> Result<Vec<RenamedSample>, ScanError> {
    let root = ensure_root_dir(db)?;
    let source_root = SourceRootCapability::open(&root)?;
    Ok(
        reconcile_hashed_rename_candidates_with_source_root_and_hook(
            db,
            &root,
            &source_root,
            rename_candidates,
            cancel,
            writer,
            precommit,
        )?
        .renamed_samples,
    )
}

fn reconcile_hashed_rename_candidates_with_source_root_and_hook(
    db: &SourceDatabase,
    root: &Path,
    source_root: &SourceRootCapability,
    rename_candidates: &HashSet<PathBuf>,
    cancel: Option<&AtomicBool>,
    writer: &impl ScanWriter,
    mut precommit: impl FnMut(&std::path::Path),
) -> Result<RenameReconciliation, ScanError> {
    if cancel_requested(cancel) {
        return Err(ScanError::Canceled);
    }
    source_root.ensure_current_generation()?;
    let rename_candidates = rename_candidates.clone();
    if rename_candidates.is_empty() {
        return Ok(RenameReconciliation {
            renamed_samples: Vec::new(),
            committed_revision: None,
            targeted_manifest_rows_read: 0,
            targeted_manifest_query_count: 0,
        });
    }

    let (manifest_revision, manifest_before) = db.manifest_snapshot_with_revision_under_paths(
        &rename_candidates.iter().cloned().collect::<Vec<_>>(),
    )?;
    let manifest_rows_read = manifest_before.len();
    let persisted_identities = manifest_before
        .into_iter()
        .filter_map(|entry| {
            entry
                .file_identity
                .map(|identity| (entry.relative_path.clone(), identity))
        })
        .collect::<HashMap<_, _>>();
    let mut entries_by_path = HashMap::new();
    let mut retained = Vec::new();
    for relative_path in &rename_candidates {
        let Some(entry) = db.entry_for_path(relative_path)? else {
            continue;
        };
        if entry.missing {
            continue;
        }
        let Some(file) = source_root.open_regular_file(&entry.relative_path)? else {
            continue;
        };
        let absolute = root.join(&entry.relative_path);
        let facts = read_facts_from_open_file(root, &absolute, &file)?;
        if entry.file_size != facts.size
            || entry.modified_ns != facts.modified_ns
            || persisted_identities.get(&entry.relative_path) != facts.file_identity.as_ref()
        {
            continue;
        }
        entries_by_path.insert(entry.relative_path.clone(), entry);
        retained.push(RetainedRenameCandidate {
            relative_path: facts.relative.clone(),
            facts,
        });
    }
    if !db.has_pending_renames()? {
        return Ok(RenameReconciliation {
            renamed_samples: Vec::new(),
            committed_revision: None,
            targeted_manifest_rows_read: manifest_rows_read,
            targeted_manifest_query_count: 1,
        });
    }
    let candidate_identities = retained
        .iter()
        .filter_map(|candidate| {
            candidate
                .facts
                .file_identity
                .clone()
                .map(|identity| (candidate.relative_path.clone(), identity))
        })
        .collect::<HashMap<_, _>>();

    if cancel_requested(cancel) {
        return Err(ScanError::Canceled);
    }
    let _writer = writer.lock(ScanWritePhase::DeferredHash);
    if cancel_requested(cancel) {
        return Err(ScanError::Canceled);
    }
    let mut batch = db.write_batch()?;
    if !batch.matches_revision(manifest_revision)? {
        return Err(ScanError::StaleRevision {
            expected: manifest_revision,
            actual: db.get_revision()?,
        });
    }
    let (renamed_samples, retained_candidates) = reconcile_indexed_rename_candidates(
        &mut batch,
        root,
        &entries_by_path,
        &candidate_identities,
        &rename_candidates,
    )?;
    if renamed_samples.is_empty() && retained_candidates == 0 {
        return Ok(RenameReconciliation {
            renamed_samples,
            committed_revision: None,
            targeted_manifest_rows_read: manifest_rows_read,
            targeted_manifest_query_count: 1,
        });
    }
    for candidate in &retained {
        precommit(&candidate.relative_path);
    }
    let final_bindings_match = retained.iter().all(|candidate| {
        let Ok(Some(file)) = source_root.open_regular_file(&candidate.relative_path) else {
            return false;
        };
        let absolute = root.join(&candidate.relative_path);
        let descriptor_matches = read_facts_from_open_file(root, &absolute, &file)
            .is_ok_and(|facts| candidate.facts.same_content_snapshot(&facts));
        descriptor_matches
            && matches!(
                source_root.path_binding(&candidate.relative_path, &file),
                Ok(SourcePathBinding::Matches)
            )
    });
    if !final_bindings_match {
        drop(batch);
        return Ok(RenameReconciliation {
            renamed_samples: Vec::new(),
            committed_revision: None,
            targeted_manifest_rows_read: manifest_rows_read,
            targeted_manifest_query_count: 1,
        });
    }
    source_root.ensure_current_generation()?;
    let result = batch.commit_with_bounded_manifest_changes(manifest_revision)?;
    Ok(RenameReconciliation {
        renamed_samples,
        committed_revision: Some(result.revision),
        targeted_manifest_rows_read: manifest_rows_read,
        targeted_manifest_query_count: 1,
    })
}

fn deep_hash_scan_with_post_hash_hook(
    db: &SourceDatabase,
    cancel: Option<&AtomicBool>,
    rename_candidates: &HashSet<PathBuf>,
    scope: DeferredHashScope,
    max_hashes: Option<usize>,
    exact_path: Option<&std::path::Path>,
    writer: &impl ScanWriter,
    post_hash: impl FnMut(&std::path::Path),
) -> Result<ScanStats, ScanError> {
    let root = ensure_root_dir(db)?;
    let source_root = SourceRootCapability::open(&root)?;
    deep_hash_scan_with_root_hooks(
        db,
        &root,
        &source_root,
        cancel,
        rename_candidates,
        scope,
        max_hashes,
        exact_path,
        writer,
        post_hash,
        |_| {},
    )
}

#[allow(clippy::too_many_arguments)]
fn deep_hash_scan_with_root_hooks(
    db: &SourceDatabase,
    root: &Path,
    source_root: &SourceRootCapability,
    cancel: Option<&AtomicBool>,
    rename_candidates: &HashSet<PathBuf>,
    scope: DeferredHashScope,
    max_hashes: Option<usize>,
    exact_path: Option<&std::path::Path>,
    writer: &impl ScanWriter,
    mut post_hash: impl FnMut(&std::path::Path),
    mut precommit: impl FnMut(&std::path::Path),
) -> Result<ScanStats, ScanError> {
    let mut combined = ScanStats::default();
    let mut cursor = None;
    let mut remaining_hashes = max_hashes;
    let mut replayed_cursor = None;

    loop {
        let descriptor_window = remaining_hashes
            .map(|remaining| remaining.min(HASH_DESCRIPTOR_WINDOW))
            .unwrap_or(HASH_DESCRIPTOR_WINDOW);
        let window = match deep_hash_scan_window_with_root_hooks(
            db,
            root,
            source_root,
            cancel,
            rename_candidates,
            scope,
            remaining_hashes,
            descriptor_window,
            exact_path,
            cursor.as_deref(),
            writer,
            &mut post_hash,
            &mut precommit,
        ) {
            Err(ScanError::StaleRevision { .. })
                if cursor.is_some() && replayed_cursor.as_deref() != cursor.as_deref() =>
            {
                replayed_cursor = cursor.clone();
                continue;
            }
            result => result?,
        };
        replayed_cursor = None;
        let hashes_computed = window.stats.hashes_computed;
        debug_assert!(window.retained_descriptors <= HASH_DESCRIPTOR_WINDOW);
        combined.merge_deferred_hashes(window.stats);
        if let Some(remaining) = &mut remaining_hashes {
            *remaining = remaining.saturating_sub(hashes_computed);
        }
        if exact_path.is_some() || window.exhausted || window.last_examined_path.is_none() {
            return Ok(combined);
        }
        cursor = window.last_examined_path;
    }
}

#[allow(clippy::too_many_arguments)]
fn deep_hash_scan_window_with_root_hooks(
    db: &SourceDatabase,
    root: &Path,
    source_root: &SourceRootCapability,
    cancel: Option<&AtomicBool>,
    rename_candidates: &HashSet<PathBuf>,
    scope: DeferredHashScope,
    max_hashes: Option<usize>,
    descriptor_window: usize,
    exact_path: Option<&std::path::Path>,
    cursor: Option<&Path>,
    writer: &impl ScanWriter,
    mut post_hash: impl FnMut(&std::path::Path),
    mut precommit: impl FnMut(&std::path::Path),
) -> Result<DeepHashWindow, ScanError> {
    source_root.ensure_current_generation()?;
    let mut rename_candidates = rename_candidates.clone();
    if scope == DeferredHashScope::AllUnhashed {
        rename_candidates.extend(db.list_pending_rename_destinations()?);
    }
    let bounded_rename_scope = scope == DeferredHashScope::RenameCandidates;
    let rename_candidate_paths = rename_candidates.iter().cloned().collect::<Vec<_>>();
    let (manifest_revision, manifest_before) = if bounded_rename_scope {
        db.manifest_snapshot_with_revision_under_paths(&rename_candidate_paths)?
    } else {
        super::manifest::capture_manifest_with_revision(db)?
    };
    let mut entries = if let Some(exact_path) = exact_path {
        db.entry_for_path(exact_path)?.into_iter().collect()
    } else if scope == DeferredHashScope::AllUnhashed && rename_candidates.is_empty() {
        let query_limit = max_hashes
            .unwrap_or(descriptor_window)
            .min(descriptor_window);
        db.list_pending_hash_files_after(cursor, query_limit)?
    } else if bounded_rename_scope {
        rename_candidates
            .iter()
            .filter_map(|path| db.entry_for_path(path).transpose())
            .collect::<Result<Vec<_>, _>>()?
    } else {
        db.list_files()?
    };
    entries.sort_by(|left, right| left.relative_path.cmp(&right.relative_path));
    let mut entries_by_path: HashMap<PathBuf, WavEntry> = entries
        .into_iter()
        .map(|entry| (entry.relative_path.clone(), entry))
        .collect();
    let has_unhashed_files = scope == DeferredHashScope::AllUnhashed
        && entries_by_path
            .values()
            .any(|entry| !entry.missing && entry.content_hash.is_none());
    if !has_unhashed_files && rename_candidates.is_empty() {
        let mut stats = ScanStats::default();
        if bounded_rename_scope {
            stats.targeted_manifest_rows_read = manifest_before.len();
            stats.targeted_manifest_query_count = 1;
        }
        let committed_snapshot = if bounded_rename_scope {
            let snapshot =
                db.manifest_snapshot_with_revision_under_paths(&rename_candidate_paths)?;
            stats.targeted_manifest_rows_read = stats
                .targeted_manifest_rows_read
                .saturating_add(snapshot.1.len());
            stats.targeted_manifest_query_count =
                stats.targeted_manifest_query_count.saturating_add(1);
            snapshot
        } else {
            db.manifest_snapshot_with_revision()?
        };
        if bounded_rename_scope {
            super::manifest::publish_targeted_committed_delta(
                &mut stats,
                manifest_before,
                committed_snapshot,
            );
            stats.manifest_updates = std::mem::take(&mut stats.manifest_after);
        } else {
            super::manifest::publish_committed_delta(
                &mut stats,
                manifest_before,
                committed_snapshot,
            );
        }
        return Ok(DeepHashWindow {
            stats,
            retained_descriptors: 0,
            last_examined_path: None,
            exhausted: true,
        });
    }
    let mut stats = ScanStats::default();
    if bounded_rename_scope {
        stats.targeted_manifest_rows_read = manifest_before.len();
        stats.targeted_manifest_query_count = 1;
    }
    stats.pending_renames_considered = rename_candidates.len();
    let mut hash_backfills = Vec::new();
    let mut candidate_identities = manifest_before
        .iter()
        .filter(|entry| rename_candidates.contains(&entry.relative_path))
        .filter_map(|entry| {
            entry
                .file_identity
                .clone()
                .map(|identity| (entry.relative_path.clone(), identity))
        })
        .collect::<HashMap<_, _>>();

    let mut entry_paths = entries_by_path.keys().cloned().collect::<Vec<_>>();
    entry_paths.sort();
    let mut last_examined_path = None;
    let mut exhausted = true;
    for path in entry_paths {
        if cursor.is_some_and(|cursor| path <= *cursor) {
            continue;
        }
        if hash_backfills.len() >= descriptor_window {
            exhausted = false;
            break;
        }
        last_examined_path = Some(path.clone());
        let Some(entry) = entries_by_path.get_mut(&path) else {
            continue;
        };
        if let Some(cancel) = cancel
            && cancel.load(Ordering::Relaxed)
        {
            return Err(ScanError::Canceled);
        }
        if entry.missing {
            continue;
        }
        let is_rename_candidate = rename_candidates.contains(&entry.relative_path);
        if entry.content_hash.is_some() && !is_rename_candidate {
            continue;
        }
        if scope == DeferredHashScope::RenameCandidates && !is_rename_candidate {
            continue;
        }
        let was_unhashed = entry.content_hash.is_none();
        if was_unhashed
            && !is_rename_candidate
            && max_hashes.is_some_and(|limit| stats.hashes_computed >= limit)
        {
            continue;
        }
        let absolute = root.join(&entry.relative_path);
        let Some(mut file) = source_root.open_regular_file(&entry.relative_path)? else {
            continue;
        };
        let facts = read_facts_from_open_file(root, &absolute, &file)?;
        file.seek(SeekFrom::Start(0))
            .map_err(|source| ScanError::Io {
                path: absolute.clone(),
                source,
            })?;
        let hash = compute_content_hash_with_reader(&absolute, &mut file, cancel)?;
        post_hash(&absolute);
        let after_hash = read_facts_from_open_file(root, &absolute, &file)?;
        if !facts.same_content_snapshot(&after_hash) {
            continue;
        }
        hash_backfills.push(HashBackfill {
            relative_path: entry.relative_path.clone(),
            facts: after_hash.clone(),
            file_size: after_hash.size,
            modified_ns: after_hash.modified_ns,
            content_hash: hash.clone(),
            file_identity: after_hash.file_identity,
            file,
        });
        if was_unhashed {
            stats.hashes_computed += 1;
        }
    }
    if hash_backfills.len() >= descriptor_window && descriptor_window > 0 {
        exhausted = false;
    }

    if let Some(cancel) = cancel
        && cancel.load(Ordering::Relaxed)
    {
        return Err(ScanError::Canceled);
    }

    source_root.ensure_current_generation()?;
    let _writer = writer.lock(ScanWritePhase::DeferredHash);
    if cancel_requested(cancel) {
        return Err(ScanError::Canceled);
    }
    let mut batch = db.write_batch()?;
    let stats_before_staging = stats.clone();
    if !batch.matches_revision(manifest_revision)? {
        return Err(ScanError::StaleRevision {
            expected: manifest_revision,
            actual: db.get_revision()?,
        });
    }
    let retained_descriptors = hash_backfills.len();
    let mut accepted_backfills = Vec::with_capacity(retained_descriptors);
    for backfill in hash_backfills {
        let absolute = root.join(&backfill.relative_path);
        let descriptor_still_current = read_facts_from_open_file(root, &absolute, &backfill.file)
            .is_ok_and(|facts| backfill.facts.same_content_snapshot(&facts));
        if !descriptor_still_current
            || !matches!(
                source_root.path_binding(&backfill.relative_path, &backfill.file),
                Ok(SourcePathBinding::Matches)
            )
        {
            continue;
        }
        batch.upsert_file_with_hash(
            &backfill.relative_path,
            backfill.file_size,
            backfill.modified_ns,
            &backfill.content_hash,
        )?;
        batch.set_file_identity(&backfill.relative_path, backfill.file_identity.as_deref())?;
        if let Some(entry) = entries_by_path.get_mut(&backfill.relative_path) {
            entry.file_size = backfill.file_size;
            entry.modified_ns = backfill.modified_ns;
            entry.content_hash = Some(backfill.content_hash.clone());
        }
        if let Some(identity) = backfill.file_identity.as_ref() {
            candidate_identities.insert(backfill.relative_path.clone(), identity.clone());
        }
        accepted_backfills.push(backfill);
    }
    let mut admitted_rename_candidates = accepted_backfills
        .iter()
        .filter(|backfill| rename_candidates.contains(&backfill.relative_path))
        .map(|backfill| backfill.relative_path.clone())
        .collect::<HashSet<_>>();
    for path in &rename_candidates {
        if admitted_rename_candidates.contains(path) {
            continue;
        }
        let Some(entry) = entries_by_path.get(path) else {
            continue;
        };
        let Some(file) = source_root.open_regular_file(path)? else {
            continue;
        };
        let absolute = root.join(path);
        let facts = read_facts_from_open_file(root, &absolute, &file)?;
        if entry.file_size == facts.size
            && entry.modified_ns == facts.modified_ns
            && candidate_identities.get(path) == facts.file_identity.as_ref()
        {
            admitted_rename_candidates.insert(path.clone());
        }
    }
    let (renamed_samples, _) = reconcile_indexed_rename_candidates(
        &mut batch,
        root,
        &entries_by_path,
        &candidate_identities,
        &admitted_rename_candidates,
    )?;
    stats.renames_reconciled = renamed_samples.len();
    stats.renamed_samples = renamed_samples;

    if cancel_requested(cancel) {
        return Err(ScanError::Canceled);
    }
    for backfill in &accepted_backfills {
        precommit(&backfill.relative_path);
    }
    let final_bindings_match = accepted_backfills.iter().all(|backfill| {
        let absolute = root.join(&backfill.relative_path);
        let descriptor_matches = read_facts_from_open_file(root, &absolute, &backfill.file)
            .is_ok_and(|facts| backfill.facts.same_content_snapshot(&facts));
        descriptor_matches
            && matches!(
                source_root.path_binding(&backfill.relative_path, &backfill.file),
                Ok(SourcePathBinding::Matches)
            )
    });
    if !final_bindings_match {
        drop(batch);
        let mut stats = stats_before_staging;
        stats.committed_delta.revision = db.get_revision()?;
        stats.pending_rename_diagnostics = Some(db.pending_rename_diagnostics()?);
        return Ok(DeepHashWindow {
            stats,
            retained_descriptors,
            last_examined_path,
            exhausted,
        });
    }
    source_root.ensure_current_generation()?;
    let committed_snapshot = if bounded_rename_scope {
        let result = batch.commit_with_bounded_manifest_changes(manifest_revision)?;
        let snapshot = db.manifest_snapshot_with_revision_under_paths(&rename_candidate_paths)?;
        stats.targeted_manifest_rows_read = stats
            .targeted_manifest_rows_read
            .saturating_add(snapshot.1.len());
        stats.targeted_manifest_query_count = stats.targeted_manifest_query_count.saturating_add(1);
        if snapshot.0 != result.revision {
            return Err(ScanError::StaleRevision {
                expected: result.revision,
                actual: snapshot.0,
            });
        }
        snapshot
    } else {
        batch.commit_with_manifest_snapshot()?
    };
    if bounded_rename_scope {
        super::manifest::publish_targeted_committed_delta(
            &mut stats,
            manifest_before,
            committed_snapshot,
        );
        stats.manifest_updates = std::mem::take(&mut stats.manifest_after);
    } else {
        super::manifest::publish_committed_delta(&mut stats, manifest_before, committed_snapshot);
    }
    stats.pending_rename_diagnostics = Some(db.pending_rename_diagnostics()?);
    Ok(DeepHashWindow {
        stats,
        retained_descriptors,
        last_examined_path,
        exhausted,
    })
}

#[cfg(test)]
#[allow(clippy::too_many_arguments)]
fn deep_hash_scan_with_hooks(
    db: &SourceDatabase,
    cancel: Option<&AtomicBool>,
    rename_candidates: &HashSet<PathBuf>,
    scope: DeferredHashScope,
    max_hashes: Option<usize>,
    exact_path: Option<&std::path::Path>,
    writer: &impl ScanWriter,
    post_hash: impl FnMut(&std::path::Path),
    precommit: impl FnMut(&std::path::Path),
) -> Result<ScanStats, ScanError> {
    let root = ensure_root_dir(db)?;
    let source_root = SourceRootCapability::open(&root)?;
    deep_hash_scan_with_root_hooks(
        db,
        &root,
        &source_root,
        cancel,
        rename_candidates,
        scope,
        max_hashes,
        exact_path,
        writer,
        post_hash,
        precommit,
    )
}

fn reconcile_indexed_rename_candidates(
    batch: &mut SourceWriteBatch<'_>,
    root: &std::path::Path,
    entries_by_path: &HashMap<PathBuf, WavEntry>,
    candidate_identities: &HashMap<PathBuf, String>,
    rename_candidates: &HashSet<PathBuf>,
) -> Result<(Vec<RenamedSample>, usize), ScanError> {
    let mut candidates_by_identity = HashMap::<String, Vec<PathBuf>>::new();
    let mut candidates_by_hash = HashMap::<String, Vec<PathBuf>>::new();
    for path in rename_candidates {
        let Some(entry) = entries_by_path.get(path) else {
            continue;
        };
        if entry.missing {
            continue;
        }
        if let Some(identity) = candidate_identities.get(path) {
            candidates_by_identity
                .entry(identity.clone())
                .or_default()
                .push(path.clone());
        }
        if let Some(hash) = entry.content_hash.as_ref() {
            candidates_by_hash
                .entry(hash.clone())
                .or_default()
                .push(path.clone());
        }
    }

    let mut reconciled = Vec::new();
    let mut reconciled_paths = HashSet::new();
    for (file_identity, present_paths) in &candidates_by_identity {
        let [present_path] = present_paths.as_slice() else {
            continue;
        };
        let Some(present_entry) = entries_by_path.get(present_path) else {
            continue;
        };
        let Some(pending_entry) =
            batch.unique_pending_rename_by_file_identity_only(file_identity)?
        else {
            continue;
        };
        if pending_entry.relative_path == *present_path
            || root.join(&pending_entry.relative_path).exists()
            || pending_entry.file_size != present_entry.file_size
            || pending_entry.modified_ns != present_entry.modified_ns
        {
            continue;
        }
        let Some(hash) = present_entry.content_hash.as_deref() else {
            continue;
        };
        apply_deep_rename(batch, present_entry, &pending_entry, hash)?;
        reconciled_paths.insert(pending_entry.relative_path.clone());
        reconciled_paths.insert(present_entry.relative_path.clone());
        reconciled.push(RenamedSample {
            old_relative_path: pending_entry.relative_path.clone(),
            new_relative_path: present_entry.relative_path.clone(),
            file_size: present_entry.file_size,
            modified_ns: present_entry.modified_ns,
            content_hash: Some(hash.to_string()),
        });
    }

    let mut retained = 0;
    for (hash, present_paths) in &candidates_by_hash {
        let Some(pending_entry) = batch.unique_pending_rename_by_hash(hash)? else {
            continue;
        };
        for present_path in present_paths {
            if reconciled_paths.contains(present_path) {
                continue;
            }
            batch.retain_pending_rename_destination(present_path, hash)?;
            retained += 1;
        }
        let available_paths = present_paths
            .iter()
            .filter(|path| !reconciled_paths.contains(*path))
            .collect::<Vec<_>>();
        let [present_path] = available_paths.as_slice() else {
            continue;
        };
        let present_path = *present_path;
        let Some(present_entry) = entries_by_path.get(present_path) else {
            continue;
        };
        if pending_entry.relative_path == *present_path
            || reconciled_paths.contains(&pending_entry.relative_path)
            || root.join(&pending_entry.relative_path).exists()
        {
            continue;
        }
        apply_deep_rename(batch, present_entry, &pending_entry, hash)?;
        reconciled_paths.insert(pending_entry.relative_path.clone());
        reconciled_paths.insert(present_entry.relative_path.clone());
        reconciled.push(RenamedSample {
            old_relative_path: pending_entry.relative_path.clone(),
            new_relative_path: present_entry.relative_path.clone(),
            file_size: present_entry.file_size,
            modified_ns: present_entry.modified_ns,
            content_hash: Some(hash.clone()),
        });
    }
    Ok((reconciled, retained))
}

fn apply_deep_rename(
    batch: &mut SourceWriteBatch<'_>,
    present_entry: &WavEntry,
    pending_entry: &PendingRenameEntry,
    hash: &str,
) -> Result<(), ScanError> {
    batch.clear_pending_rename(&pending_entry.relative_path)?;
    batch.clear_pending_rename_destination(&present_entry.relative_path)?;
    batch.upsert_file_with_hash_and_tag(
        &present_entry.relative_path,
        present_entry.file_size,
        present_entry.modified_ns,
        hash,
        pending_entry.metadata.tag,
        false,
    )?;
    batch.restore_rename_metadata(&present_entry.relative_path, &pending_entry.metadata)?;
    batch.remap_analysis_sample_identity(
        &pending_entry.relative_path,
        &present_entry.relative_path,
    )?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use std::{
        path::Path,
        sync::{
            Arc,
            atomic::{AtomicBool, AtomicUsize, Ordering},
        },
    };

    use crate::sample_sources::SourceDatabase;

    use super::*;

    #[derive(Clone, Default)]
    struct ObservedWriter {
        active: Arc<AtomicBool>,
        locks: Arc<AtomicUsize>,
    }

    struct ObservedWriterGuard(Arc<AtomicBool>);

    struct InterleavedManifestWriter {
        root: PathBuf,
        committed: AtomicBool,
    }

    struct WindowManifestWriter {
        root: PathBuf,
        lock_count: AtomicUsize,
        on_lock: usize,
    }

    struct HashedRenameFixture {
        _directory: tempfile::TempDir,
        database: SourceDatabase,
        old_relative: PathBuf,
        new_relative: PathBuf,
    }

    fn hashed_rename_fixture(new_relative: &Path) -> HashedRenameFixture {
        let directory = tempfile::tempdir().expect("temp source");
        let old_relative = PathBuf::from("old.wav");
        let old_absolute = directory.path().join(&old_relative);
        let new_relative = new_relative.to_path_buf();
        let new_absolute = directory.path().join(&new_relative);
        if let Some(parent) = new_absolute.parent() {
            std::fs::create_dir_all(parent).expect("create destination parent");
        }
        std::fs::write(&old_absolute, [5_u8; 32]).expect("write original wav");
        let facts = read_facts(directory.path(), &old_absolute).expect("read original facts");
        let database = SourceDatabase::open_for_source_write(directory.path()).expect("source db");
        let mut batch = database.write_batch().expect("seed source batch");
        batch
            .upsert_file_with_hash(&old_relative, facts.size, facts.modified_ns, "shared-hash")
            .expect("insert original row");
        batch
            .set_file_identity(&old_relative, facts.file_identity.as_deref())
            .expect("store original identity");
        batch
            .set_tag(
                &old_relative,
                wavecrate_library::sample_sources::Rating::KEEP_1,
            )
            .expect("curate original row");
        batch.commit().expect("commit original row");

        let original = database
            .entry_for_path(&old_relative)
            .expect("read original row")
            .expect("original row");
        let mut batch = database.write_batch().expect("stage rename batch");
        batch
            .stage_pending_rename(&original)
            .expect("stage pending rename");
        batch
            .remove_file(&old_relative)
            .expect("remove original row");
        batch.commit().expect("commit pending rename");
        std::fs::rename(&old_absolute, &new_absolute).expect("rename wav");

        let mut batch = database.write_batch().expect("seed destination batch");
        batch
            .upsert_file_with_hash(&new_relative, facts.size, facts.modified_ns, "shared-hash")
            .expect("insert destination row");
        batch
            .set_file_identity(&new_relative, facts.file_identity.as_deref())
            .expect("store destination identity");
        batch.commit().expect("commit destination row");

        HashedRenameFixture {
            _directory: directory,
            database,
            old_relative,
            new_relative,
        }
    }

    fn assert_hashed_rename_remains_pending(fixture: &HashedRenameFixture) {
        assert!(
            fixture
                .database
                .list_pending_renames()
                .expect("read pending renames")
                .iter()
                .any(|entry| entry.relative_path == fixture.old_relative),
            "rejected reconciliation must preserve pending rename state"
        );
        assert_eq!(
            fixture
                .database
                .entry_for_path(&fixture.new_relative)
                .expect("read destination row")
                .expect("destination row")
                .tag,
            wavecrate_library::sample_sources::Rating::NEUTRAL,
            "rejected reconciliation must not transfer source metadata"
        );
    }

    impl Drop for ObservedWriterGuard {
        fn drop(&mut self) {
            self.0.store(false, Ordering::Release);
        }
    }

    impl ScanWriter for ObservedWriter {
        type Guard = ObservedWriterGuard;

        fn lock(&self, _phase: ScanWritePhase) -> Self::Guard {
            assert!(!self.active.swap(true, Ordering::AcqRel));
            self.locks.fetch_add(1, Ordering::AcqRel);
            ObservedWriterGuard(Arc::clone(&self.active))
        }
    }

    impl ScanWriter for InterleavedManifestWriter {
        type Guard = ();

        fn lock(&self, _phase: ScanWritePhase) -> Self::Guard {
            if !self.committed.swap(true, Ordering::AcqRel) {
                let database = SourceDatabase::open_for_source_write(&self.root)
                    .expect("open concurrent source writer");
                database
                    .upsert_file(Path::new("unrelated.wav"), 1, 1)
                    .expect("commit unrelated manifest revision");
            }
        }
    }

    impl ScanWriter for WindowManifestWriter {
        type Guard = ();

        fn lock(&self, _phase: ScanWritePhase) -> Self::Guard {
            let lock = self.lock_count.fetch_add(1, Ordering::AcqRel) + 1;
            if lock == self.on_lock {
                std::fs::write(self.root.join("unrelated.wav"), [0_u8])
                    .expect("write unrelated source file");
                let database = SourceDatabase::open_for_source_write(&self.root)
                    .expect("open concurrent source writer");
                database
                    .upsert_file(Path::new("unrelated.wav"), 1, 1)
                    .expect("commit unrelated manifest revision");
            }
        }
    }

    fn content_fixture(count: usize, bytes: usize) -> (tempfile::TempDir, SourceDatabase) {
        let directory = tempfile::tempdir().expect("content source");
        let database =
            SourceDatabase::open_for_source_write(directory.path()).expect("source database");
        for index in 0..count {
            let relative = PathBuf::from(format!("sample-{index:05}.wav"));
            std::fs::write(directory.path().join(&relative), vec![index as u8; bytes])
                .expect("write content fixture");
            let facts = read_facts(directory.path(), &directory.path().join(&relative))
                .expect("content facts");
            database
                .upsert_file(&relative, facts.size, facts.modified_ns)
                .expect("insert content manifest");
        }
        (directory, database)
    }

    #[test]
    fn adaptive_content_budget_has_a_finite_large_source_horizon() {
        let idle = ContentAuditBudget::adaptive(
            10_000,
            ContentAuditActivity::default(),
            ContentAuditStorage::Local,
            false,
        );
        assert_eq!(idle.max_entries, 334);
        assert_eq!(idle.max_elapsed, Duration::from_secs(5));
        assert_eq!(idle.max_bytes, 512 * 1024 * 1024);

        let busy_external = ContentAuditBudget::adaptive(
            29_000,
            ContentAuditActivity {
                playback_active: true,
                foreground_active: true,
            },
            ContentAuditStorage::ExternalOrNetwork,
            false,
        );
        assert_eq!(busy_external.max_entries, 967);
        assert_eq!(busy_external.max_elapsed, Duration::from_secs(1));
        assert_eq!(busy_external.max_bytes, 64 * 1024 * 1024);
        assert_eq!(
            ContentAuditBudget::adaptive(
                30,
                ContentAuditActivity::default(),
                ContentAuditStorage::Local,
                false,
            )
            .max_entries,
            2,
            "multi-entry adaptive slices must reserve retry and forward capacity"
        );

        let seven_day = ContentAuditBudget::adaptive_for_target(
            10_000,
            ContentAuditActivity::default(),
            ContentAuditStorage::Local,
            false,
            Duration::from_secs(7 * 24 * 60 * 60),
        );
        assert_eq!(seven_day.max_entries, 1_429);
        assert_eq!(
            seven_day.target_coverage_age,
            Duration::from_secs(7 * 24 * 60 * 60)
        );
    }

    #[test]
    fn windows_mapped_removable_and_unknown_drive_types_are_conservative() {
        assert_eq!(
            classify_windows_drive_type(WINDOWS_DRIVE_FIXED),
            ContentAuditStorage::Local
        );
        for drive_type in [
            WINDOWS_DRIVE_REMOTE,
            WINDOWS_DRIVE_REMOVABLE,
            0, // DRIVE_UNKNOWN
            1, // DRIVE_NO_ROOT_DIR
            5, // DRIVE_CDROM
            6, // DRIVE_RAMDISK
        ] {
            assert_eq!(
                classify_windows_drive_type(drive_type),
                ContentAuditStorage::ExternalOrNetwork
            );
        }
        assert_eq!(
            classify_unknown_storage(),
            ContentAuditStorage::ExternalOrNetwork,
            "platforms without mount classification must fail conservatively"
        );
    }

    #[test]
    fn macos_only_grants_local_budget_to_the_normal_local_mount() {
        assert_eq!(
            classify_macos_device(7, 7),
            ContentAuditStorage::Local,
            "the normal local device receives the local budget"
        );
        assert_eq!(
            classify_macos_device(8, 7),
            ContentAuditStorage::ExternalOrNetwork,
            "a distinct mounted filesystem receives the conservative budget"
        );
    }

    #[test]
    fn content_audit_resumes_without_recounting_committed_entries() {
        let (_directory, database) = content_fixture(10, 32);
        let budget = ContentAuditBudget {
            max_elapsed: Duration::MAX,
            max_bytes: u64::MAX,
            max_entries: 3,
            target_coverage_age: Duration::from_secs(30 * 24 * 60 * 60),
            retry_entries: 1,
        };

        let first = verify_content_batch(&database, None, budget, 100).expect("first slice");
        let first_report = first.content_audit.expect("first coverage");
        assert_eq!(first_report.verified_entries, 3);
        assert_eq!(first_report.remaining_entries, 7);

        drop(database);
        let database = SourceDatabase::open_for_source_write(_directory.path()).expect("reopen");
        let second = verify_content_batch(&database, None, budget, 101).expect("second slice");
        let second_report = second.content_audit.expect("second coverage");
        assert_eq!(second_report.verified_entries, 6);
        assert_eq!(second_report.remaining_entries, 4);
        assert_eq!(second.hashes_computed, 3);
    }

    #[test]
    fn verified_forward_rows_advance_the_cursor_without_rehashing() {
        let (_directory, database) = content_fixture(3, 32);
        let revision = database.get_revision().unwrap();
        let checkpoint = database
            .begin_or_resume_content_audit(100, revision)
            .unwrap();
        let entries = database.list_manifest_entries().unwrap();
        let mut batch = database.write_batch().unwrap();
        for entry in entries.iter().take(2) {
            batch
                .record_content_audit_verified(
                    &entry.relative_path,
                    checkpoint.rotation_id,
                    100,
                    entry.file_size,
                    entry.modified_ns,
                    entry.file_identity.as_deref(),
                )
                .unwrap();
        }
        batch.commit().unwrap();

        let first =
            verify_content_batch(&database, None, ContentAuditBudget::entry_limited(1), 101)
                .expect("advance verified forward row");

        assert_eq!(first.hashes_computed, 0);
        let report = first.content_audit.expect("coverage");
        assert_eq!(report.cursor, "sample-00000.wav");
        assert_eq!(report.verified_entries, 2);
        assert_eq!(report.remaining_entries, 1);
    }

    #[test]
    fn stale_due_retry_state_is_retired_in_a_bounded_slice() {
        let (directory, database) = content_fixture(1, 32);
        let path = Path::new("sample-00000.wav");
        std::fs::remove_file(directory.path().join(path)).unwrap();
        verify_content_batch(&database, None, ContentAuditBudget::entry_limited(1), 100)
            .expect("record unavailable retry");
        assert!(
            database
                .content_audit_entry_states()
                .unwrap()
                .contains_key(path)
        );
        database.set_missing(path, true).unwrap();

        let cleanup =
            verify_content_batch(&database, None, ContentAuditBudget::entry_limited(1), 1_000)
                .expect("retire stale retry");

        assert_eq!(cleanup.hashes_computed, 0);
        assert!(
            !database
                .content_audit_entry_states()
                .unwrap()
                .contains_key(path)
        );
        assert_eq!(cleanup.content_audit.expect("coverage").total_entries, 0);
    }

    #[test]
    fn final_rotation_completion_preserves_the_committed_content_delta() {
        let (directory, database) = content_fixture(1, 32);
        let relative = Path::new("sample-00000.wav");
        let facts = read_facts(directory.path(), &directory.path().join(relative)).unwrap();
        let mut batch = database.write_batch().expect("stale content batch");
        batch
            .upsert_file_with_hash(
                relative,
                facts.size,
                facts.modified_ns,
                "stale-content-hash",
            )
            .expect("seed stale content generation");
        batch.commit().expect("commit stale content generation");

        let stats =
            verify_content_batch(&database, None, ContentAuditBudget::entry_limited(1), 100)
                .expect("rotation-completing content verification");

        assert_eq!(stats.content_changed, 1);
        assert_eq!(stats.committed_delta.changed.len(), 1);
        assert_eq!(
            stats.committed_delta.changed[0].relative_path,
            Path::new("sample-00000.wav")
        );
        assert_eq!(
            stats.committed_delta.revision,
            database.get_revision().unwrap(),
            "published delta must carry the final committed rotation revision"
        );
    }

    #[test]
    fn content_audit_byte_budget_allows_one_oversize_file_then_stops() {
        let (_directory, database) = content_fixture(4, 32);
        let budget = ContentAuditBudget {
            max_elapsed: Duration::MAX,
            max_bytes: 16,
            max_entries: 4,
            target_coverage_age: Duration::from_secs(30 * 24 * 60 * 60),
            retry_entries: 1,
        };

        let stats = verify_content_batch(&database, None, budget, 100).expect("byte slice");

        assert_eq!(stats.hashes_computed, 1);
        let report = stats.content_audit.expect("coverage");
        assert_eq!(report.verified_entries, 1);
        assert_eq!(report.bytes_read, 32);
    }

    #[test]
    fn content_audit_time_budget_stops_at_the_next_file_boundary() {
        let (_directory, database) = content_fixture(3, 32);
        let elapsed = std::cell::Cell::new(Duration::ZERO);
        let mut post_hash = |_: &std::path::Path| elapsed.set(Duration::from_secs(1));
        let mut observe_elapsed = || elapsed.get();
        let budget = ContentAuditBudget {
            max_elapsed: Duration::from_secs(1),
            max_bytes: u64::MAX,
            max_entries: 3,
            target_coverage_age: Duration::from_secs(30 * 24 * 60 * 60),
            retry_entries: 1,
        };

        let stats = verify_content_batch_with_hooks(
            &database,
            None,
            budget,
            100,
            &UncoordinatedScanWriter,
            &mut post_hash,
            &mut |_| {},
            &mut observe_elapsed,
        )
        .expect("time-bounded slice");

        assert_eq!(stats.hashes_computed, 1);
        let report = stats.content_audit.expect("coverage");
        assert_eq!(report.verified_entries, 1);
        assert_eq!(report.remaining_entries, 2);
    }

    #[test]
    fn cancellation_commits_verified_checkpoint_and_resume_skips_it() {
        let (_directory, database) = content_fixture(3, 32);
        let cancel = AtomicBool::new(false);
        let budget = ContentAuditBudget::entry_limited(3);

        let result = verify_content_batch_with_post_hash_hook(
            &database,
            Some(&cancel),
            budget,
            100,
            &UncoordinatedScanWriter,
            |_| cancel.store(true, Ordering::Release),
        );
        let ScanError::Incomplete { committed, .. } = result.expect_err("cancel content slice")
        else {
            panic!("cancellation after a hash must retain committed coverage");
        };
        assert_eq!(
            committed
                .content_audit
                .as_ref()
                .expect("cancel coverage")
                .verified_entries,
            1
        );

        cancel.store(false, Ordering::Release);
        let resumed = verify_content_batch(
            &database,
            Some(&cancel),
            ContentAuditBudget::entry_limited(1),
            101,
        )
        .expect("resume content slice");
        assert_eq!(resumed.hashes_computed, 1);
        assert_eq!(
            resumed
                .content_audit
                .expect("resumed coverage")
                .verified_entries,
            2
        );
    }

    #[test]
    fn changed_during_hash_remains_due_with_retry_reason() {
        let (directory, database) = content_fixture(2, 32);
        let changing = directory.path().join("sample-00000.wav");
        let result = verify_content_batch_with_post_hash_hook(
            &database,
            None,
            ContentAuditBudget::entry_limited(1),
            100,
            &UncoordinatedScanWriter,
            |path| {
                assert_eq!(path, changing);
                std::fs::write(path, [7_u8; 64]).expect("mutate during hash");
            },
        )
        .expect("skip unstable content");

        let report = result.content_audit.expect("coverage");
        assert_eq!(report.verified_entries, 0);
        assert_eq!(report.skipped_retry_entries, 1);
        let states = database.content_audit_entry_states().expect("entry states");
        assert_eq!(
            states[Path::new("sample-00000.wav")].skip_reason.as_deref(),
            Some("changed_during_hash")
        );

        let forward =
            verify_content_batch(&database, None, ContentAuditBudget::entry_limited(1), 101)
                .expect("continue past delayed retry");
        assert_eq!(forward.hashes_computed, 1);
        assert_eq!(
            forward
                .content_audit
                .expect("forward coverage")
                .skipped_retry_entries,
            1
        );
        assert_eq!(
            database.content_audit_entry_states().unwrap()[Path::new("sample-00000.wav")].attempts,
            1
        );

        let retry =
            verify_content_batch(&database, None, ContentAuditBudget::entry_limited(1), 1_000)
                .expect("retry stable content");
        assert_eq!(retry.hashes_computed, 1);
        let state = database.content_audit_entry_states().unwrap();
        assert_eq!(state[Path::new("sample-00000.wav")].skip_reason, None);
        assert_eq!(state[Path::new("sample-00000.wav")].attempts, 2);
    }

    #[cfg(unix)]
    #[test]
    fn content_audit_does_not_publish_an_outside_link_target() {
        use std::os::unix::fs::symlink;

        let (directory, database) = content_fixture(1, 32);
        let relative = Path::new("sample-00000.wav");
        let outside = tempfile::tempdir().expect("outside directory");
        let outside_file = outside.path().join("outside.wav");
        std::fs::write(&outside_file, b"outside").expect("write outside file");

        let stats = verify_content_batch_with_post_hash_hook(
            &database,
            None,
            ContentAuditBudget::entry_limited(1),
            100,
            &UncoordinatedScanWriter,
            |path| {
                std::fs::remove_file(path).expect("remove source file");
                symlink(&outside_file, path).expect("replace source with outside link");
            },
        )
        .expect("defer replaced audit entry");

        assert_eq!(
            stats
                .content_audit
                .expect("content audit report")
                .verified_entries,
            0
        );
        assert!(
            database
                .entry_for_path(relative)
                .expect("read source row")
                .expect("source row")
                .content_hash
                .is_none()
        );
        drop(directory);
    }

    #[cfg(unix)]
    #[test]
    fn content_audit_does_not_publish_a_link_replacement_before_commit() {
        use std::os::unix::fs::symlink;

        let (directory, database) = content_fixture(1, 32);
        let relative = Path::new("sample-00000.wav");
        let outside = tempfile::tempdir().expect("outside directory");
        let outside_file = outside.path().join("outside.wav");
        std::fs::write(&outside_file, b"outside").expect("write outside file");

        let stats = verify_content_batch_with_hooks(
            &database,
            None,
            ContentAuditBudget::entry_limited(1),
            100,
            &UncoordinatedScanWriter,
            &mut |_| {},
            &mut |path| {
                let absolute = directory.path().join(path);
                std::fs::remove_file(&absolute).expect("remove source file");
                symlink(&outside_file, absolute).expect("replace source with outside link");
            },
            &mut || Duration::ZERO,
        )
        .expect("defer late replacement");

        assert_eq!(
            stats
                .content_audit
                .expect("content audit report")
                .verified_entries,
            0
        );
        assert!(
            database
                .entry_for_path(relative)
                .expect("read source row")
                .expect("source row")
                .content_hash
                .is_none()
        );
    }

    #[test]
    fn due_retry_cannot_consume_the_only_forward_progress_slot() {
        let (directory, database) = content_fixture(2, 32);
        let changing = directory.path().join("sample-00000.wav");
        verify_content_batch_with_post_hash_hook(
            &database,
            None,
            ContentAuditBudget::entry_limited(1),
            100,
            &UncoordinatedScanWriter,
            |path| {
                assert_eq!(path, changing);
                std::fs::write(path, [7_u8; 64]).expect("mutate during hash");
            },
        )
        .expect("record unstable entry");

        let forward =
            verify_content_batch(&database, None, ContentAuditBudget::entry_limited(1), 1_000)
                .expect("reserve the only slot for forward progress");

        assert_eq!(forward.hashes_computed, 1);
        let states = database.content_audit_entry_states().unwrap();
        assert_eq!(
            states[Path::new("sample-00000.wav")].attempts,
            1,
            "the due retry must remain queued while forward work owns the only slot"
        );
        assert!(
            states[Path::new("sample-00001.wav")].verifies(
                &database
                    .list_manifest_entries()
                    .unwrap()
                    .into_iter()
                    .find(|entry| entry.relative_path == Path::new("sample-00001.wav"))
                    .unwrap(),
                1,
            )
        );
    }

    #[test]
    fn oversized_retry_yields_the_next_slice_to_forward_progress() {
        let (directory, database) = content_fixture(2, 32);
        let retry_path = directory.path().join("sample-00000.wav");
        verify_content_batch_with_post_hash_hook(
            &database,
            None,
            ContentAuditBudget::entry_limited(1),
            100,
            &UncoordinatedScanWriter,
            |path| {
                assert_eq!(path, retry_path);
                std::fs::write(path, [7_u8; 64]).expect("make first attempt unstable");
            },
        )
        .expect("record unstable entry");
        let constrained = ContentAuditBudget {
            max_elapsed: Duration::MAX,
            max_bytes: 16,
            max_entries: 2,
            target_coverage_age: Duration::from_secs(30 * 24 * 60 * 60),
            retry_entries: 1,
        };

        let retry_slice = verify_content_batch_with_post_hash_hook(
            &database,
            None,
            constrained,
            1_000,
            &UncoordinatedScanWriter,
            |path| {
                assert_eq!(path, retry_path);
                std::fs::write(path, [9_u8; 96]).expect("keep retry unstable");
            },
        )
        .expect("bounded retry slice");
        assert_eq!(retry_slice.hashes_computed, 0);
        assert!(
            !retry_slice
                .content_audit
                .expect("retry coverage")
                .retry_next,
            "an attempted retry must persist forward as the next lane"
        );
        let after_retry = database.content_audit_entry_states().unwrap();
        assert_eq!(after_retry[Path::new("sample-00000.wav")].attempts, 2);
        assert!(!after_retry.contains_key(Path::new("sample-00001.wav")));

        drop(database);
        let database =
            SourceDatabase::open_for_source_write(directory.path()).expect("reopen source");
        let forward_slice =
            verify_content_batch(&database, None, constrained, 2_000).expect("forward slice");
        assert_eq!(forward_slice.hashes_computed, 1);
        let after_forward = database.content_audit_entry_states().unwrap();
        assert_eq!(
            after_forward[Path::new("sample-00000.wav")].attempts,
            2,
            "the still-due oversized retry must not run before reserved forward work"
        );
        assert_eq!(
            after_forward[Path::new("sample-00001.wav")].skip_reason,
            None
        );
    }

    #[test]
    fn retry_cursor_rotates_past_a_persistently_unavailable_path() {
        let (directory, database) = content_fixture(3, 32);
        std::fs::remove_file(directory.path().join("sample-00000.wav")).unwrap();
        std::fs::remove_file(directory.path().join("sample-00001.wav")).unwrap();
        verify_content_batch(&database, None, ContentAuditBudget::entry_limited(2), 100)
            .expect("record two unavailable entries");

        verify_content_batch(&database, None, ContentAuditBudget::entry_limited(2), 1_000)
            .expect("retry the first path while reserving forward progress");
        let first_retry = database.content_audit_report(1_000).unwrap();
        assert_eq!(first_retry.retry_cursor, "sample-00000.wav");

        verify_content_batch(&database, None, ContentAuditBudget::entry_limited(1), 2_000)
            .expect("rotate the retry slot to the later path");
        let states = database.content_audit_entry_states().unwrap();
        assert_eq!(states[Path::new("sample-00000.wav")].attempts, 2);
        assert_eq!(
            states[Path::new("sample-00001.wav")].attempts,
            2,
            "the later due retry must eventually own the bounded retry slot"
        );
        assert_eq!(
            database.content_audit_report(2_000).unwrap().retry_cursor,
            "sample-00001.wav"
        );
    }

    #[test]
    fn content_hashing_does_not_hold_the_runtime_database_writer() {
        let (_directory, database) = content_fixture(2, 32);
        let writer = ObservedWriter::default();
        let active = Arc::clone(&writer.active);

        verify_content_batch_with_post_hash_hook(
            &database,
            None,
            ContentAuditBudget::entry_limited(2),
            100,
            &writer,
            |_| assert!(!active.load(Ordering::Acquire)),
        )
        .expect("content slice");

        assert!(writer.locks.load(Ordering::Acquire) >= 3);
        assert!(!writer.active.load(Ordering::Acquire));
    }

    #[test]
    fn content_audit_rejects_a_stale_manifest_revision_before_commit() {
        let (_directory, database) = content_fixture(1, 32);

        let result = verify_content_batch_with_post_hash_hook(
            &database,
            None,
            ContentAuditBudget::entry_limited(1),
            100,
            &UncoordinatedScanWriter,
            |_| {
                database
                    .set_metadata("concurrent_manifest_writer", "1")
                    .expect("advance source revision");
            },
        );

        assert!(matches!(result, Err(ScanError::StaleRevision { .. })));
        assert!(database.content_audit_entry_states().unwrap().is_empty());
    }

    #[test]
    fn stale_content_audit_replays_only_the_unpublished_window() {
        let total = HASH_DESCRIPTOR_WINDOW * 2 + 3;
        let (directory, database) = content_fixture(total, 32);
        let writer = WindowManifestWriter {
            root: directory.path().to_path_buf(),
            lock_count: AtomicUsize::new(0),
            on_lock: 6,
        };
        let hashed_paths = Arc::new(std::sync::Mutex::new(Vec::new()));
        let observed_paths = Arc::clone(&hashed_paths);
        let mut post_hash = move |path: &Path| {
            observed_paths.lock().unwrap().push(path.to_path_buf());
        };
        let mut precommit = no_op_path_hook;

        let stats = verify_content_batch_with_hooks(
            &database,
            None,
            ContentAuditBudget::entry_limited(total),
            100,
            &writer,
            &mut post_hash,
            &mut precommit,
            &mut || Duration::ZERO,
        )
        .expect("replay the stale content-audit window");

        assert_eq!(stats.hashes_computed, total);
        assert_eq!(
            hashed_paths.lock().unwrap().len(),
            total + HASH_DESCRIPTOR_WINDOW,
            "only the stale content-audit window should have been replayed"
        );
    }

    fn no_op_path_hook(_path: &Path) {}

    #[test]
    fn deep_hash_scan_checks_cancel_before_writer_lock() {
        let dir = tempfile::tempdir().expect("temp source");
        std::fs::write(dir.path().join("pending.wav"), b"pending").expect("write wav");
        let db = SourceDatabase::open_for_source_write(dir.path()).expect("source db");
        db.upsert_file(Path::new("pending.wav"), 7, 10)
            .expect("file row");
        let lock_db = SourceDatabase::open_for_source_write(dir.path()).expect("lock db");
        let _writer = lock_db.write_batch().expect("writer lock");
        let cancel = AtomicBool::new(true);

        let result = deep_hash_scan(
            &db,
            Some(&cancel),
            &HashSet::new(),
            DeferredHashScope::AllUnhashed,
            None,
            None,
        );

        assert!(matches!(result, Err(ScanError::Canceled)));
    }

    #[test]
    fn deep_hash_scan_bounds_a_large_library_batch() {
        let dir = tempfile::tempdir().expect("temp source");
        let db = SourceDatabase::open_for_source_write(dir.path()).expect("source db");
        for index in 0..512 {
            let relative = PathBuf::from(format!("pending-{index}.wav"));
            std::fs::write(dir.path().join(&relative), [index as u8; 32]).expect("write wav");
            db.upsert_file(&relative, 32, index)
                .expect("insert pending row");
        }

        let stats = deep_hash_scan(
            &db,
            None,
            &HashSet::new(),
            DeferredHashScope::AllUnhashed,
            Some(8),
            None,
        )
        .expect("bounded hash pass");

        assert_eq!(stats.hashes_computed, 8);
        assert_eq!(
            db.list_files()
                .expect("list files")
                .iter()
                .filter(|entry| entry.content_hash.is_some())
                .count(),
            8
        );
    }

    #[cfg(unix)]
    #[test]
    fn content_audit_releases_descriptors_between_publication_windows() {
        let (_directory, database) = content_fixture(HASH_DESCRIPTOR_WINDOW * 4 + 3, 32);
        let baseline_descriptors = std::fs::read_dir("/dev/fd")
            .expect("read process descriptors")
            .count();
        let peak_descriptors = AtomicUsize::new(baseline_descriptors);
        let budget = ContentAuditBudget::entry_limited(HASH_DESCRIPTOR_WINDOW * 4 + 3);

        let stats = verify_content_batch_with_post_hash_hook(
            &database,
            None,
            budget,
            100,
            &UncoordinatedScanWriter,
            |_| {
                let descriptors = std::fs::read_dir("/dev/fd")
                    .expect("read process descriptors")
                    .count();
                peak_descriptors.fetch_max(descriptors, Ordering::AcqRel);
            },
        )
        .expect("complete content audit windows");

        assert_eq!(stats.hashes_computed, HASH_DESCRIPTOR_WINDOW * 4 + 3);
        // The test's descriptor scan and the source database contribute a small,
        // fixed runtime overhead in addition to the retained file descriptors.
        const RUNTIME_DESCRIPTOR_OVERHEAD: usize = 32;
        assert!(
            peak_descriptors.load(Ordering::Acquire)
                <= baseline_descriptors + HASH_DESCRIPTOR_WINDOW + RUNTIME_DESCRIPTOR_OVERHEAD,
            "descriptor peak exceeded the publication window: peak={}, baseline={}",
            peak_descriptors.load(Ordering::Acquire),
            baseline_descriptors
        );
    }

    #[test]
    fn unbounded_deferred_hashes_checkpoint_descriptor_windows() {
        let dir = tempfile::tempdir().expect("temp source");
        let db = SourceDatabase::open_for_source_write(dir.path()).expect("source db");
        let total = HASH_DESCRIPTOR_WINDOW * 2 + 3;
        for index in 0..total {
            let relative = PathBuf::from(format!("pending-{index:03}.wav"));
            std::fs::write(dir.path().join(&relative), [index as u8; 32]).expect("write wav");
            db.upsert_file(&relative, 32, index as i64)
                .expect("insert pending row");
        }

        let stats = deep_hash_scan_with_hooks(
            &db,
            None,
            &HashSet::new(),
            DeferredHashScope::AllUnhashed,
            None,
            None,
            &UncoordinatedScanWriter,
            |_| {},
            |_| {},
        )
        .expect("complete the unbounded queue");

        assert_eq!(stats.hashes_computed, total);
        assert_eq!(
            db.list_pending_hash_files(1)
                .expect("read remaining pending rows")
                .len(),
            0,
            "unbounded deferred hashing must drain every checkpointed window"
        );
    }

    #[test]
    fn stale_deferred_hash_replays_only_the_unpublished_window() {
        let dir = tempfile::tempdir().expect("temp source");
        let db = SourceDatabase::open_for_source_write(dir.path()).expect("source db");
        let total = HASH_DESCRIPTOR_WINDOW * 2 + 3;
        for index in 0..total {
            let relative = PathBuf::from(format!("pending-{index:03}.wav"));
            std::fs::write(dir.path().join(&relative), [index as u8; 32]).expect("write wav");
            db.upsert_file(&relative, 32, index as i64)
                .expect("insert pending row");
        }
        let writer = WindowManifestWriter {
            root: dir.path().to_path_buf(),
            lock_count: AtomicUsize::new(0),
            on_lock: 2,
        };
        let hashed_paths = Arc::new(std::sync::Mutex::new(Vec::new()));
        let observed_paths = Arc::clone(&hashed_paths);

        let result = deep_hash_scan_with_hooks(
            &db,
            None,
            &HashSet::new(),
            DeferredHashScope::AllUnhashed,
            None,
            None,
            &writer,
            move |path| observed_paths.lock().unwrap().push(path.to_path_buf()),
            |_| {},
        );

        assert!(result.is_ok(), "the stale window should replay in place");
        assert_eq!(result.unwrap().hashes_computed, total + 1);
        assert_eq!(
            hashed_paths.lock().unwrap().len(),
            total + 1 + HASH_DESCRIPTOR_WINDOW,
            "only the stale window should have been replayed"
        );
        assert_eq!(
            db.list_pending_hash_files(1)
                .expect("read remaining pending rows")
                .len(),
            0
        );
    }

    #[test]
    fn deep_hash_scan_exact_path_does_not_process_earlier_pending_rows() {
        let dir = tempfile::tempdir().expect("temp source");
        let db = SourceDatabase::open_for_source_write(dir.path()).expect("source db");
        for relative in [Path::new("a-first.wav"), Path::new("z-target.wav")] {
            std::fs::write(dir.path().join(relative), [9_u8; 32]).expect("write wav");
            db.upsert_file(relative, 32, 1).expect("insert pending row");
        }

        let stats = deep_hash_scan(
            &db,
            None,
            &HashSet::new(),
            DeferredHashScope::AllUnhashed,
            Some(1),
            Some(Path::new("z-target.wav")),
        )
        .expect("targeted hash pass");

        assert_eq!(stats.hashes_computed, 1);
        assert!(
            db.entry_for_path(Path::new("a-first.wav"))
                .unwrap()
                .unwrap()
                .content_hash
                .is_none()
        );
        assert!(
            db.entry_for_path(Path::new("z-target.wav"))
                .unwrap()
                .unwrap()
                .content_hash
                .is_some()
        );
    }

    #[test]
    fn deep_hash_scan_does_not_commit_a_file_mutated_during_hashing() {
        let dir = tempfile::tempdir().expect("temp source");
        let relative = Path::new("changing.wav");
        let absolute = dir.path().join(relative);
        std::fs::write(&absolute, [1_u8; 32]).expect("write initial wav");
        let original_modified = std::fs::metadata(&absolute)
            .expect("read initial metadata")
            .modified()
            .expect("read initial modified time");
        let db = SourceDatabase::open_for_source_write(dir.path()).expect("source db");
        db.upsert_file(relative, 32, 1).expect("insert pending row");

        let stats = deep_hash_scan_with_post_hash_hook(
            &db,
            None,
            &HashSet::new(),
            DeferredHashScope::AllUnhashed,
            Some(1),
            Some(relative),
            &UncoordinatedScanWriter,
            |path| {
                std::fs::write(path, [2_u8; 32]).expect("mutate during hashing");
                let file = std::fs::OpenOptions::new()
                    .write(true)
                    .open(path)
                    .expect("reopen mutated wav");
                file.set_times(std::fs::FileTimes::new().set_modified(original_modified))
                    .expect("restore modified time");
            },
        )
        .expect("defer unstable hash");

        assert_eq!(stats.hashes_computed, 0);
        assert!(
            db.entry_for_path(relative)
                .expect("read pending row")
                .expect("pending row")
                .content_hash
                .is_none(),
            "an unstable read must remain pending for a later hash pass"
        );
    }

    #[cfg(unix)]
    #[test]
    fn deferred_exact_hash_does_not_publish_an_outside_link_target() {
        use std::os::unix::fs::symlink;

        let dir = tempfile::tempdir().expect("temp source");
        let relative = Path::new("pending.wav");
        let absolute = dir.path().join(relative);
        let outside = tempfile::tempdir().expect("outside directory");
        let outside_file = outside.path().join("outside.wav");
        std::fs::write(&absolute, b"inside").expect("write source file");
        std::fs::write(&outside_file, b"outside").expect("write outside file");
        let db = SourceDatabase::open_for_source_write(dir.path()).expect("source db");
        db.upsert_file(relative, 6, 1).expect("insert pending row");

        deep_hash_scan_with_post_hash_hook(
            &db,
            None,
            &HashSet::new(),
            DeferredHashScope::AllUnhashed,
            Some(1),
            Some(relative),
            &UncoordinatedScanWriter,
            |path| {
                std::fs::remove_file(path).expect("remove source file");
                symlink(&outside_file, path).expect("replace source with outside link");
            },
        )
        .expect("defer replaced exact hash");

        assert!(
            db.entry_for_path(relative)
                .expect("read pending row")
                .expect("pending row")
                .content_hash
                .is_none()
        );
    }

    #[cfg(unix)]
    #[test]
    fn deferred_exact_hash_does_not_publish_a_link_replacement_before_commit() {
        use std::os::unix::fs::symlink;

        let dir = tempfile::tempdir().expect("temp source");
        let relative = Path::new("pending.wav");
        let absolute = dir.path().join(relative);
        let outside = tempfile::tempdir().expect("outside directory");
        let outside_file = outside.path().join("outside.wav");
        std::fs::write(&absolute, b"inside").expect("write source file");
        std::fs::write(&outside_file, b"outside").expect("write outside file");
        let db = SourceDatabase::open_for_source_write(dir.path()).expect("source db");
        db.upsert_file(relative, 6, 1).expect("insert pending row");

        deep_hash_scan_with_hooks(
            &db,
            None,
            &HashSet::new(),
            DeferredHashScope::AllUnhashed,
            Some(1),
            Some(relative),
            &UncoordinatedScanWriter,
            |_| {},
            |path| {
                let absolute = dir.path().join(path);
                std::fs::remove_file(&absolute).expect("remove source file");
                symlink(&outside_file, absolute).expect("replace source with outside link");
            },
        )
        .expect("defer late replacement");

        assert!(
            db.entry_for_path(relative)
                .expect("read pending row")
                .expect("pending row")
                .content_hash
                .is_none()
        );
    }

    #[cfg(unix)]
    #[test]
    fn deferred_rename_excludes_an_outside_link_before_admission() {
        use std::os::unix::fs::symlink;

        let fixture = hashed_rename_fixture(Path::new("new.wav"));
        let destination = fixture._directory.path().join(&fixture.new_relative);
        let outside = tempfile::tempdir().expect("outside directory");
        let outside_file = outside.path().join("outside.wav");
        std::fs::write(&outside_file, b"outside").expect("write outside file");
        std::fs::remove_file(&destination).expect("remove destination");
        symlink(&outside_file, &destination).expect("replace destination with outside link");

        let stats = deep_hash_scan(
            &fixture.database,
            None,
            &HashSet::from([fixture.new_relative.clone()]),
            DeferredHashScope::RenameCandidates,
            None,
            Some(&fixture.new_relative),
        )
        .expect("defer linked rename candidate");

        assert_eq!(stats.renames_reconciled, 0);
        assert_hashed_rename_remains_pending(&fixture);
    }

    #[cfg(unix)]
    #[test]
    fn deferred_rename_rejects_a_final_link_replacement_before_commit() {
        use std::os::unix::fs::symlink;

        let fixture = hashed_rename_fixture(Path::new("new.wav"));
        let outside = tempfile::tempdir().expect("outside directory");
        let outside_file = outside.path().join("outside.wav");
        std::fs::write(&outside_file, b"outside").expect("write outside file");

        let stats = deep_hash_scan_with_hooks(
            &fixture.database,
            None,
            &HashSet::from([fixture.new_relative.clone()]),
            DeferredHashScope::RenameCandidates,
            None,
            Some(&fixture.new_relative),
            &UncoordinatedScanWriter,
            |_| {},
            |path| {
                let destination = fixture._directory.path().join(path);
                std::fs::remove_file(&destination).expect("remove destination");
                symlink(&outside_file, destination).expect("replace destination with outside link");
            },
        )
        .expect("reject late linked rename candidate");

        assert_eq!(stats.renames_reconciled, 0);
        assert_hashed_rename_remains_pending(&fixture);
    }

    #[cfg(unix)]
    #[test]
    fn deferred_rename_rejects_a_link_ancestor_before_commit() {
        use std::os::unix::fs::symlink;

        let fixture = hashed_rename_fixture(Path::new("nested/new.wav"));
        let outside = tempfile::tempdir().expect("outside directory");
        std::fs::write(outside.path().join("new.wav"), b"outside").expect("write outside file");

        let stats = deep_hash_scan_with_hooks(
            &fixture.database,
            None,
            &HashSet::from([fixture.new_relative.clone()]),
            DeferredHashScope::RenameCandidates,
            None,
            Some(&fixture.new_relative),
            &UncoordinatedScanWriter,
            |_| {},
            |_| {
                let nested = fixture._directory.path().join("nested");
                std::fs::rename(&nested, fixture._directory.path().join("moved"))
                    .expect("move destination ancestor");
                symlink(outside.path(), nested).expect("replace ancestor with outside link");
            },
        )
        .expect("reject late linked rename ancestor");

        assert_eq!(stats.renames_reconciled, 0);
        assert_hashed_rename_remains_pending(&fixture);
    }

    #[cfg(unix)]
    #[test]
    fn fast_hashed_rename_rejects_a_final_link_replacement_before_commit() {
        use std::os::unix::fs::symlink;

        let fixture = hashed_rename_fixture(Path::new("new.wav"));
        let outside = tempfile::tempdir().expect("outside directory");
        let outside_file = outside.path().join("outside.wav");
        std::fs::write(&outside_file, b"outside").expect("write outside file");

        let reconciled = reconcile_hashed_rename_candidates_with_hook(
            &fixture.database,
            &HashSet::from([fixture.new_relative.clone()]),
            None,
            &UncoordinatedScanWriter,
            |path| {
                let destination = fixture._directory.path().join(path);
                std::fs::remove_file(&destination).expect("remove destination");
                symlink(&outside_file, destination).expect("replace destination with outside link");
            },
        )
        .expect("reject late linked fast candidate");

        assert!(reconciled.is_empty());
        assert_hashed_rename_remains_pending(&fixture);
    }

    #[cfg(unix)]
    #[test]
    fn fast_hashed_rename_rejects_a_link_ancestor_before_commit() {
        use std::os::unix::fs::symlink;

        let fixture = hashed_rename_fixture(Path::new("nested/new.wav"));
        let outside = tempfile::tempdir().expect("outside directory");
        std::fs::write(outside.path().join("new.wav"), b"outside").expect("write outside file");

        let reconciled = reconcile_hashed_rename_candidates_with_hook(
            &fixture.database,
            &HashSet::from([fixture.new_relative.clone()]),
            None,
            &UncoordinatedScanWriter,
            |_| {
                let nested = fixture._directory.path().join("nested");
                std::fs::rename(&nested, fixture._directory.path().join("moved"))
                    .expect("move destination ancestor");
                symlink(outside.path(), nested).expect("replace ancestor with outside link");
            },
        )
        .expect("reject late linked fast ancestor");

        assert!(reconciled.is_empty());
        assert_hashed_rename_remains_pending(&fixture);
    }

    #[test]
    fn hashed_rename_reconciliation_rejects_a_stale_manifest_revision() {
        let dir = tempfile::tempdir().expect("temp source");
        let old_relative = Path::new("old.wav");
        let new_relative = Path::new("new.wav");
        let old_absolute = dir.path().join(old_relative);
        let new_absolute = dir.path().join(new_relative);
        std::fs::write(&old_absolute, [5_u8; 32]).expect("write original wav");
        let original_facts = read_facts(dir.path(), &old_absolute).expect("read original facts");
        let db = SourceDatabase::open_for_source_write(dir.path()).expect("source db");
        let mut batch = db.write_batch().expect("seed source batch");
        batch
            .upsert_file_with_hash(
                old_relative,
                original_facts.size,
                original_facts.modified_ns,
                "shared-hash",
            )
            .expect("insert original row");
        batch
            .set_file_identity(old_relative, original_facts.file_identity.as_deref())
            .expect("store original identity");
        batch
            .set_tag(
                old_relative,
                wavecrate_library::sample_sources::Rating::KEEP_1,
            )
            .expect("curate original row");
        batch.commit().expect("commit original row");

        let original = db
            .entry_for_path(old_relative)
            .expect("read original row")
            .expect("original row");
        let mut batch = db.write_batch().expect("stage rename batch");
        batch
            .stage_pending_rename(&original)
            .expect("stage pending rename");
        batch
            .remove_file(old_relative)
            .expect("remove original row");
        batch.commit().expect("commit pending rename");
        std::fs::rename(&old_absolute, &new_absolute).expect("rename wav");
        let mut batch = db.write_batch().expect("seed destination batch");
        batch
            .upsert_file_with_hash(
                new_relative,
                original_facts.size,
                original_facts.modified_ns,
                "shared-hash",
            )
            .expect("insert destination row");
        batch
            .set_file_identity(new_relative, original_facts.file_identity.as_deref())
            .expect("store destination identity");
        batch.commit().expect("commit destination row");

        let writer = InterleavedManifestWriter {
            root: dir.path().to_path_buf(),
            committed: AtomicBool::new(false),
        };
        let result = reconcile_hashed_rename_candidates_with_writer(
            &db,
            &HashSet::from([new_relative.to_path_buf()]),
            None,
            &writer,
        );

        assert!(matches!(result, Err(ScanError::StaleRevision { .. })));
        assert!(
            db.list_pending_renames()
                .expect("read pending renames")
                .iter()
                .any(|entry| entry.relative_path == old_relative),
            "stale reconciliation must leave the pending rename available for retry"
        );
        assert_eq!(
            db.entry_for_path(new_relative)
                .expect("read destination row")
                .expect("destination row")
                .tag,
            wavecrate_library::sample_sources::Rating::NEUTRAL,
            "stale reconciliation must not transfer source metadata"
        );
    }

    #[test]
    fn deep_hash_scan_does_not_recreate_a_row_deleted_after_hash_planning() {
        let dir = tempfile::tempdir().expect("temp source");
        let relative = Path::new("deleted.wav");
        std::fs::write(dir.path().join(relative), [1_u8; 32]).expect("write wav");
        let db = SourceDatabase::open_for_source_write(dir.path()).expect("source db");
        db.upsert_file(relative, 32, 1).expect("insert pending row");

        let result = deep_hash_scan_with_post_hash_hook(
            &db,
            None,
            &HashSet::new(),
            DeferredHashScope::AllUnhashed,
            Some(1),
            Some(relative),
            &UncoordinatedScanWriter,
            |_| {
                let mut batch = db.write_batch().expect("delete batch");
                batch.remove_file(relative).expect("delete planned row");
                batch.commit().expect("commit deletion");
            },
        );

        assert!(matches!(result, Err(ScanError::StaleRevision { .. })));
        assert!(
            db.entry_for_path(relative)
                .expect("read deleted row")
                .is_none(),
            "stale hash publication must not recreate a deleted manifest row"
        );
    }

    #[test]
    fn deep_hash_scan_does_not_overwrite_a_replacement_row() {
        let dir = tempfile::tempdir().expect("temp source");
        let relative = Path::new("replaced.wav");
        std::fs::write(dir.path().join(relative), [2_u8; 32]).expect("write wav");
        let db = SourceDatabase::open_for_source_write(dir.path()).expect("source db");
        db.upsert_file(relative, 32, 1).expect("insert pending row");

        let result = deep_hash_scan_with_post_hash_hook(
            &db,
            None,
            &HashSet::new(),
            DeferredHashScope::AllUnhashed,
            Some(1),
            Some(relative),
            &UncoordinatedScanWriter,
            |_| {
                let mut batch = db.write_batch().expect("replacement batch");
                batch.remove_file(relative).expect("remove planned row");
                batch
                    .upsert_file_with_hash(relative, 64, 99, "replacement-hash")
                    .expect("insert replacement row");
                batch
                    .set_file_identity(relative, Some("replacement-identity"))
                    .expect("set replacement identity");
                batch.commit().expect("commit replacement");
            },
        );

        assert!(matches!(result, Err(ScanError::StaleRevision { .. })));
        let replacement = db
            .entry_for_path(relative)
            .expect("read replacement row")
            .expect("replacement row");
        assert_eq!(replacement.file_size, 64);
        assert_eq!(replacement.modified_ns, 99);
        assert_eq!(
            replacement.content_hash.as_deref(),
            Some("replacement-hash")
        );
        let replacement_manifest = db
            .list_manifest_entries()
            .expect("read replacement manifest")
            .into_iter()
            .find(|entry| entry.relative_path == relative)
            .expect("replacement manifest row");
        assert_eq!(
            replacement_manifest.file_identity.as_deref(),
            Some("replacement-identity")
        );
    }

    #[test]
    fn deep_hash_scan_retries_after_an_unrelated_manifest_revision() {
        let dir = tempfile::tempdir().expect("temp source");
        let target = Path::new("target.wav");
        let unrelated = Path::new("unrelated.wav");
        std::fs::write(dir.path().join(target), [3_u8; 32]).expect("write target");
        std::fs::write(dir.path().join(unrelated), [4_u8; 16]).expect("write unrelated");
        let db = SourceDatabase::open_for_source_write(dir.path()).expect("source db");
        db.upsert_file(target, 32, 1)
            .expect("insert pending target");

        let result = deep_hash_scan_with_post_hash_hook(
            &db,
            None,
            &HashSet::new(),
            DeferredHashScope::AllUnhashed,
            Some(1),
            Some(target),
            &UncoordinatedScanWriter,
            |_| {
                db.upsert_file(unrelated, 16, 2)
                    .expect("commit unrelated manifest row");
            },
        );

        assert!(matches!(result, Err(ScanError::StaleRevision { .. })));
        assert!(
            db.entry_for_path(target)
                .expect("read target row")
                .expect("target row")
                .content_hash
                .is_none(),
            "a revision mismatch must leave the planned hash pending for retry"
        );
        assert!(
            db.entry_for_path(unrelated)
                .expect("read unrelated row")
                .is_some(),
            "the concurrent committed manifest change must remain authoritative"
        );

        let retried = deep_hash_scan(
            &db,
            None,
            &HashSet::new(),
            DeferredHashScope::AllUnhashed,
            Some(1),
            Some(target),
        )
        .expect("retry deferred hash from the current manifest revision");
        assert_eq!(retried.hashes_computed, 1);
        assert!(
            db.entry_for_path(target)
                .expect("read retried target row")
                .expect("retried target row")
                .content_hash
                .is_some(),
            "the typed stale outcome must remain safely retryable"
        );
    }
}
