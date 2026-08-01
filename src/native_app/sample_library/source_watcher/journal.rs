//! Durable closed-application coverage for source roots.
//!
//! The live `notify` watcher deliberately starts before this module replays a persisted FSEvents
//! cursor. That ordering closes the handoff window: replay covers the time Wavecrate was not
//! running, while the live watcher owns changes made during replay. A missing cursor, changed
//! filesystem identity, or any FSEvents history-loss flag fails closed to the existing bounded
//! manifest audit.

#[cfg(target_os = "macos")]
use std::path::{Path, PathBuf};

#[cfg(target_os = "macos")]
use notify::EventKind;
use serde::{
    Deserialize, Serialize,
    de::{self, Deserializer, MapAccess, Visitor},
};
use std::fmt;
use wavecrate::sample_sources::{
    SampleSource,
    db::{SourceDatabase, SourceWriteBatch},
};
use wavecrate_library::{
    filesystem_identity::stable_filesystem_identity,
    sample_sources::db::META_SOURCE_WATCHER_CHECKPOINT,
};

const CHECKPOINT_FORMAT_VERSION: u32 = 2;
const LEGACY_CHECKPOINT_FIELDS: [&str; 2] = ["root_identity", "event_id"];
const V2_CHECKPOINT_FIELDS: [&str; 7] = [
    "root_identity",
    "event_id",
    "format_version",
    "source_id",
    "lifecycle_generation",
    "source_revision",
    "cause",
];

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub(in crate::native_app) enum CheckpointCause {
    TargetedReplay,
    CompletedFallbackAudit,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub(in crate::native_app) struct RevisionBoundCheckpoint {
    pub(in crate::native_app) source_id: String,
    pub(in crate::native_app) lifecycle_generation: u64,
    pub(in crate::native_app) source_revision: u64,
    pub(in crate::native_app) root_identity: String,
    pub(in crate::native_app) event_id: u64,
    pub(in crate::native_app) cause: CheckpointCause,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(in crate::native_app) enum CheckpointAdvanceOutcome {
    Applied,
    AlreadyApplied,
    Superseded,
    Retryable,
    AuditRequired,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
struct SourceWatcherCheckpoint {
    root_identity: String,
    event_id: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    format_version: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    source_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    lifecycle_generation: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    source_revision: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    cause: Option<CheckpointCause>,
}

impl SourceWatcherCheckpoint {
    fn legacy(root_identity: String, event_id: u64) -> Self {
        Self {
            root_identity,
            event_id,
            format_version: None,
            source_id: None,
            lifecycle_generation: None,
            source_revision: None,
            cause: None,
        }
    }

    fn from_revision_bound(checkpoint: &RevisionBoundCheckpoint) -> Self {
        Self {
            root_identity: checkpoint.root_identity.clone(),
            event_id: checkpoint.event_id,
            format_version: Some(CHECKPOINT_FORMAT_VERSION),
            source_id: Some(checkpoint.source_id.clone()),
            lifecycle_generation: Some(checkpoint.lifecycle_generation),
            source_revision: Some(checkpoint.source_revision),
            cause: Some(checkpoint.cause),
        }
    }

    fn revision_bound(&self) -> Option<RevisionBoundCheckpoint> {
        let (
            Some(CHECKPOINT_FORMAT_VERSION),
            Some(source_id),
            Some(lifecycle_generation),
            Some(source_revision),
            Some(cause),
        ) = (
            self.format_version,
            self.source_id.clone(),
            self.lifecycle_generation,
            self.source_revision,
            self.cause,
        )
        else {
            return None;
        };
        Some(RevisionBoundCheckpoint {
            source_id,
            lifecycle_generation,
            source_revision,
            root_identity: self.root_identity.clone(),
            event_id: self.event_id,
            cause,
        })
    }
}

fn has_exact_checkpoint_fields(
    object: &serde_json::Map<String, serde_json::Value>,
    expected_fields: &[&str],
) -> bool {
    object.len() == expected_fields.len()
        && expected_fields
            .iter()
            .all(|field| object.contains_key(*field))
}

fn checkpoint_string(
    object: &serde_json::Map<String, serde_json::Value>,
    field: &str,
) -> Result<String, String> {
    object
        .get(field)
        .and_then(serde_json::Value::as_str)
        .map(str::to_owned)
        .ok_or_else(|| format!("checkpoint field {field} must be a string"))
}

fn checkpoint_u64(
    object: &serde_json::Map<String, serde_json::Value>,
    field: &str,
) -> Result<u64, String> {
    object
        .get(field)
        .and_then(serde_json::Value::as_u64)
        .ok_or_else(|| format!("checkpoint field {field} must be an unsigned integer"))
}

fn parse_checkpoint_object(
    value: &str,
) -> Result<serde_json::Map<String, serde_json::Value>, String> {
    struct CheckpointObjectVisitor;

    impl<'de> Visitor<'de> for CheckpointObjectVisitor {
        type Value = serde_json::Map<String, serde_json::Value>;

        fn expecting(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
            formatter.write_str("a JSON object")
        }

        fn visit_map<M>(self, mut map: M) -> Result<Self::Value, M::Error>
        where
            M: MapAccess<'de>,
        {
            let mut object = serde_json::Map::new();
            while let Some(key) = map.next_key::<String>()? {
                if object.contains_key(&key) {
                    return Err(de::Error::custom(format!(
                        "duplicate checkpoint field: {key}"
                    )));
                }
                object.insert(key, map.next_value::<serde_json::Value>()?);
            }
            Ok(object)
        }
    }

    let mut deserializer = serde_json::Deserializer::from_str(value);
    let object = deserializer
        .deserialize_map(CheckpointObjectVisitor)
        .map_err(|error| error.to_string())?;
    deserializer.end().map_err(|error| error.to_string())?;
    Ok(object)
}

fn parse_checkpoint(value: &str) -> Result<SourceWatcherCheckpoint, String> {
    let object = parse_checkpoint_object(value)?;

    if has_exact_checkpoint_fields(&object, &LEGACY_CHECKPOINT_FIELDS) {
        return Ok(SourceWatcherCheckpoint::legacy(
            checkpoint_string(&object, "root_identity")?,
            checkpoint_u64(&object, "event_id")?,
        ));
    }

    if !has_exact_checkpoint_fields(&object, &V2_CHECKPOINT_FIELDS) {
        return Err("watcher checkpoint has an unsupported field shape".to_string());
    }
    if checkpoint_u64(&object, "format_version")? != u64::from(CHECKPOINT_FORMAT_VERSION) {
        return Err("watcher checkpoint has an unsupported format version".to_string());
    }

    let cause = serde_json::from_value(
        object
            .get("cause")
            .cloned()
            .ok_or_else(|| "watcher checkpoint is missing cause".to_string())?,
    )
    .map_err(|error| format!("checkpoint cause is invalid: {error}"))?;
    Ok(SourceWatcherCheckpoint {
        root_identity: checkpoint_string(&object, "root_identity")?,
        event_id: checkpoint_u64(&object, "event_id")?,
        format_version: Some(CHECKPOINT_FORMAT_VERSION),
        source_id: Some(checkpoint_string(&object, "source_id")?),
        lifecycle_generation: Some(checkpoint_u64(&object, "lifecycle_generation")?),
        source_revision: Some(checkpoint_u64(&object, "source_revision")?),
        cause: Some(cause),
    })
}

fn decide_checkpoint_advance(
    current: Option<&SourceWatcherCheckpoint>,
    requested: &RevisionBoundCheckpoint,
    expected_source_id: &str,
    current_lifecycle_generation: u64,
    current_source_revision: u64,
    current_root_identity: &str,
) -> CheckpointAdvanceOutcome {
    if requested.source_id != expected_source_id
        || requested.lifecycle_generation != current_lifecycle_generation
        || requested.source_revision != current_source_revision
        || requested.root_identity != current_root_identity
    {
        return CheckpointAdvanceOutcome::AuditRequired;
    }

    if requested.cause == CheckpointCause::CompletedFallbackAudit {
        let Some(current) = current else {
            // A completed source-wide audit is the authority that establishes the first
            // revision-bound cursor after a missing checkpoint.
            return CheckpointAdvanceOutcome::Applied;
        };
        if current.revision_bound().is_none() {
            if current.root_identity != requested.root_identity
                || current.event_id > requested.event_id
            {
                return CheckpointAdvanceOutcome::AuditRequired;
            }
            // A valid legacy cursor can be upgraded by the completed audit. Malformed and
            // unknown bytes never reach this branch because read_checkpoint_from_batch fails
            // closed before the pure decision.
            return CheckpointAdvanceOutcome::Applied;
        }
    }

    let Some(current) = current else {
        return CheckpointAdvanceOutcome::AuditRequired;
    };
    let Some(current) = current.revision_bound() else {
        return CheckpointAdvanceOutcome::AuditRequired;
    };

    if current.source_id != requested.source_id
        || current.source_revision > current_source_revision
        || current.root_identity != requested.root_identity
    {
        return CheckpointAdvanceOutcome::AuditRequired;
    }
    if current == *requested {
        return CheckpointAdvanceOutcome::AlreadyApplied;
    }
    if requested.event_id < current.event_id {
        return CheckpointAdvanceOutcome::AuditRequired;
    }
    if requested.event_id > current.event_id {
        return CheckpointAdvanceOutcome::Applied;
    }
    CheckpointAdvanceOutcome::Superseded
}

/// Apply a revision-bound watcher checkpoint from the source-processing owner.
///
/// The caller owns lifecycle and live-root validation. This helper only performs bounded source
/// database work: it opens the configured source database, holds one immediate write batch while
/// reading the current revision and checkpoint evidence, and commits the new checkpoint only when
/// the pure advance decision is `Applied`. Malformed, unknown, stale, and superseded evidence is
/// never rewritten; a completed authoritative source audit may upgrade a valid missing or legacy
/// cursor to the revision-bound format.
pub(in crate::native_app) fn write_revision_bound_checkpoint(
    source: &SampleSource,
    requested: &RevisionBoundCheckpoint,
    current_lifecycle_generation: u64,
    current_root_identity: &str,
) -> CheckpointAdvanceOutcome {
    let database = match source_database(source) {
        Ok(database) => database,
        Err(error) => {
            tracing::debug!(
                source_id = source.id.as_str(),
                ?error,
                "Could not open source database for watcher checkpoint"
            );
            return CheckpointAdvanceOutcome::Retryable;
        }
    };
    let mut batch = match database.write_batch() {
        Ok(batch) => batch,
        Err(error) => {
            tracing::debug!(
                source_id = source.id.as_str(),
                ?error,
                "Could not start source transaction for watcher checkpoint"
            );
            return CheckpointAdvanceOutcome::Retryable;
        }
    };
    let current_source_revision = match batch.get_revision() {
        Ok(revision) => revision,
        Err(error) => {
            tracing::debug!(
                source_id = source.id.as_str(),
                ?error,
                "Could not read source revision for watcher checkpoint"
            );
            return CheckpointAdvanceOutcome::Retryable;
        }
    };
    let current = match read_checkpoint_from_batch(&batch) {
        Ok(current) => current,
        Err(outcome) => return outcome,
    };
    let outcome = decide_checkpoint_advance(
        current.as_ref(),
        requested,
        source.id.as_str(),
        current_lifecycle_generation,
        current_source_revision,
        current_root_identity,
    );
    if outcome != CheckpointAdvanceOutcome::Applied {
        return outcome;
    }

    let value =
        match serde_json::to_string(&SourceWatcherCheckpoint::from_revision_bound(requested)) {
            Ok(value) => value,
            Err(error) => {
                tracing::debug!(
                    source_id = source.id.as_str(),
                    ?error,
                    "Could not serialize watcher checkpoint"
                );
                return CheckpointAdvanceOutcome::Retryable;
            }
        };
    if let Err(error) = batch.set_metadata(META_SOURCE_WATCHER_CHECKPOINT, &value) {
        tracing::debug!(
            source_id = source.id.as_str(),
            ?error,
            "Could not write watcher checkpoint metadata"
        );
        return CheckpointAdvanceOutcome::Retryable;
    }
    if let Err(error) = batch.commit_auxiliary_state() {
        tracing::debug!(
            source_id = source.id.as_str(),
            ?error,
            "Could not commit watcher checkpoint metadata"
        );
        return CheckpointAdvanceOutcome::Retryable;
    }
    CheckpointAdvanceOutcome::Applied
}

fn read_checkpoint_from_batch(
    batch: &SourceWriteBatch<'_>,
) -> Result<Option<SourceWatcherCheckpoint>, CheckpointAdvanceOutcome> {
    let Some(value) = batch
        .get_metadata(META_SOURCE_WATCHER_CHECKPOINT)
        .map_err(|error| {
            tracing::debug!(?error, "Could not read watcher checkpoint metadata");
            CheckpointAdvanceOutcome::Retryable
        })?
    else {
        return Ok(None);
    };
    match parse_checkpoint(&value) {
        Ok(checkpoint) => Ok(Some(checkpoint)),
        Err(error) => {
            tracing::debug!(?error, "Stored watcher checkpoint is malformed");
            Err(CheckpointAdvanceOutcome::AuditRequired)
        }
    }
}

#[derive(Clone, Debug)]
pub(super) struct AuditBarrier(SourceWatcherCheckpoint);

impl AuditBarrier {
    pub(super) fn into_revision_bound(
        self,
        source_id: String,
        lifecycle_generation: u64,
        source_revision: u64,
    ) -> RevisionBoundCheckpoint {
        RevisionBoundCheckpoint {
            source_id,
            lifecycle_generation,
            source_revision,
            root_identity: self.0.root_identity,
            event_id: self.0.event_id,
            cause: CheckpointCause::CompletedFallbackAudit,
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(super) enum JournalRecovery {
    #[cfg(target_os = "macos")]
    Changes {
        paths: Vec<PathBuf>,
        event_id: u64,
    },
    FullAudit {
        reason: &'static str,
    },
}

/// Recover the changes made while the process was not observing a source root.
///
/// This returns paths relative to `source.root`; callers feed them through the normal debounced
/// source-sync path. The fallback is intentionally per source so a mounted volume or a single
/// unavailable database cannot make healthy sources traverse too.
pub(super) fn recover_sources(
    sources: &[SampleSource],
    native_watcher: bool,
) -> Vec<JournalRecovery> {
    sources
        .iter()
        .map(|source| recover_source(source, native_watcher))
        .collect()
}

#[cfg(target_os = "macos")]
fn classify_replayed_paths(
    source: &SampleSource,
    paths: Vec<PathBuf>,
    event_id: u64,
) -> JournalRecovery {
    let paths = paths
        .into_iter()
        .filter(|path| {
            super::classification::path_is_source_refresh_candidate(path, EventKind::Any)
        })
        .filter_map(|path| path.strip_prefix(&source.root).ok().map(PathBuf::from))
        .filter(|path| !path.as_os_str().is_empty())
        .collect::<Vec<_>>();
    if paths.is_empty() {
        JournalRecovery::FullAudit {
            reason: "journal_replay_empty_paths",
        }
    } else {
        JournalRecovery::Changes { paths, event_id }
    }
}

fn recover_source(source: &SampleSource, native_watcher: bool) -> JournalRecovery {
    if !native_watcher {
        return JournalRecovery::FullAudit {
            reason: "watcher_backend_has_no_durable_journal",
        };
    }
    let Some(root_identity) = std::fs::metadata(&source.root)
        .ok()
        .and_then(|metadata| stable_filesystem_identity(&source.root, &metadata))
    else {
        return JournalRecovery::FullAudit {
            reason: "source_root_identity_unavailable",
        };
    };
    let checkpoint = match load_checkpoint(source) {
        Ok(Some(checkpoint)) if checkpoint.root_identity == root_identity => checkpoint,
        Ok(Some(_)) => {
            return JournalRecovery::FullAudit {
                reason: "source_root_identity_changed",
            };
        }
        Ok(None) => {
            // Do not persist a cursor yet. The caller must first capture a barrier before the
            // fallback audit and commit that exact barrier after it completes; writing "now"
            // here could skip a mutation the audit had already passed.
            let _ = root_identity;
            return JournalRecovery::FullAudit {
                reason: "watcher_checkpoint_missing",
            };
        }
        Err(error) => {
            tracing::warn!(
                source_id = source.id.as_str(),
                "Could not read durable source watcher checkpoint: {error}"
            );
            return JournalRecovery::FullAudit {
                reason: "watcher_checkpoint_unavailable",
            };
        }
    };

    #[cfg(target_os = "macos")]
    {
        match replay_fsevents(&source.root, checkpoint.event_id) {
            Ok((paths, event_id)) => classify_replayed_paths(source, paths, event_id),
            Err(reason) => JournalRecovery::FullAudit { reason },
        }
    }
    #[cfg(not(target_os = "macos"))]
    {
        let _ = checkpoint;
        JournalRecovery::FullAudit {
            reason: "durable_journal_unsupported",
        }
    }
}

/// Advance a replay cursor only after the target filesystem reconciliation has committed.
#[cfg(test)]
pub(super) fn advance_after_reconciliation(
    sources: &[SampleSource],
    source_id: &str,
    event_id: u64,
) {
    let Some(source) = sources
        .iter()
        .find(|source| source.id.as_str() == source_id)
    else {
        return;
    };
    let Some(root_identity) = std::fs::metadata(&source.root)
        .ok()
        .and_then(|metadata| stable_filesystem_identity(&source.root, &metadata))
    else {
        return;
    };
    let Ok(Some(mut checkpoint)) = load_checkpoint(source) else {
        return;
    };
    if checkpoint.root_identity != root_identity || checkpoint.event_id > event_id {
        return;
    }
    checkpoint.event_id = event_id;
    if let Err(error) = store_checkpoint(source, &checkpoint) {
        tracing::warn!(
            source_id,
            "Could not advance durable source watcher checkpoint: {error}"
        );
    }
}

/// Capture a journal barrier before a fallback audit starts. It remains in watcher memory until a
/// successful completion, so a crash or incomplete audit keeps the older cursor and replays safe
/// overlap on the next launch.
pub(super) fn capture_audit_barrier(
    sources: &[SampleSource],
    source_id: &str,
) -> Option<AuditBarrier> {
    #[cfg(target_os = "macos")]
    let event_id = unsafe { fsevent_sys::FSEventsGetCurrentEventId() };
    #[cfg(not(target_os = "macos"))]
    let event_id = 0;
    let source = sources
        .iter()
        .find(|source| source.id.as_str() == source_id)?;
    let root_identity = std::fs::metadata(&source.root)
        .ok()
        .and_then(|metadata| stable_filesystem_identity(&source.root, &metadata))?;
    Some(AuditBarrier(SourceWatcherCheckpoint::legacy(
        root_identity,
        event_id,
    )))
}

fn source_database(source: &SampleSource) -> Result<SourceDatabase, String> {
    let database_root = source.database_root().map_err(|error| error.to_string())?;
    SourceDatabase::open_for_background_job_with_database_root(&source.root, database_root)
        .map_err(|error| error.to_string())
}

fn load_checkpoint(source: &SampleSource) -> Result<Option<SourceWatcherCheckpoint>, String> {
    let database = source_database(source)?;
    database
        .get_metadata(META_SOURCE_WATCHER_CHECKPOINT)
        .map_err(|error| error.to_string())?
        .map(|value| parse_checkpoint(&value))
        .transpose()
}

#[cfg(test)]
fn store_checkpoint(
    source: &SampleSource,
    checkpoint: &SourceWatcherCheckpoint,
) -> Result<(), String> {
    let database = source_database(source)?;
    let value = serde_json::to_string(checkpoint).map_err(|error| error.to_string())?;
    database
        .set_metadata(META_SOURCE_WATCHER_CHECKPOINT, &value)
        .map_err(|error| error.to_string())
}

#[cfg(target_os = "macos")]
fn replay_fsevents(root: &Path, event_id: u64) -> Result<(Vec<PathBuf>, u64), &'static str> {
    macos::replay(root, event_id).map(|replay| (replay.paths, replay.event_id))
}

#[cfg(target_os = "macos")]
mod macos {
    use super::*;
    use fsevent_sys::{self as fs, core_foundation as cf};
    use std::{
        ffi::{CStr, c_void},
        ptr,
        sync::{Mutex, mpsc},
        time::Duration,
    };

    const HISTORY_TIMEOUT: Duration = Duration::from_secs(10);
    const HISTORY_LOSS_FLAGS: fs::FSEventStreamEventFlags =
        fs::kFSEventStreamEventFlagMustScanSubDirs
            | fs::kFSEventStreamEventFlagUserDropped
            | fs::kFSEventStreamEventFlagKernelDropped
            | fs::kFSEventStreamEventFlagEventIdsWrapped
            | fs::kFSEventStreamEventFlagRootChanged
            | fs::kFSEventStreamEventFlagMount
            | fs::kFSEventStreamEventFlagUnmount;

    #[derive(Default)]
    struct HistoryState {
        paths: Vec<PathBuf>,
        history_done: bool,
        history_lost: bool,
        latest_event_id: u64,
    }

    pub(super) struct HistoryReplay {
        pub(super) paths: Vec<PathBuf>,
        pub(super) event_id: u64,
    }

    struct HistoryContext {
        root: PathBuf,
        state: Mutex<HistoryState>,
        ready_tx: mpsc::Sender<()>,
    }

    /// CoreFoundation run-loop references can be stopped from another thread. The handle remains
    /// owned by the history worker; the caller uses it only to request a bounded shutdown before
    /// joining that worker.
    struct RunLoopHandle(cf::CFRunLoopRef);

    // Safety: CoreFoundation documents run-loop stop as cross-thread safe. The caller never
    // dereferences or releases this handle; stream teardown and context destruction stay on the
    // history worker that created them.
    unsafe impl Send for RunLoopHandle {}

    #[link(name = "CoreFoundation", kind = "framework")]
    unsafe extern "C" {
        fn CFRetain(cf: cf::CFRef) -> cf::CFRef;
    }

    pub(super) fn replay(root: &Path, event_id: u64) -> Result<HistoryReplay, &'static str> {
        let (result_tx, result_rx) = mpsc::sync_channel(1);
        let (run_loop_tx, run_loop_rx) = mpsc::sync_channel(1);
        let root = root.to_path_buf();
        let worker = std::thread::Builder::new()
            .name("wavecrate-fsevents-history".to_string())
            .spawn(move || {
                let _ = result_tx.send(replay_on_run_loop(&root, event_id, run_loop_tx));
            })
            .map_err(|_| "watcher_history_thread_unavailable")?;
        let run_loop = match run_loop_rx.recv_timeout(HISTORY_TIMEOUT) {
            Ok(run_loop) => run_loop,
            Err(mpsc::RecvTimeoutError::Disconnected) => {
                let _ = worker.join();
                return Err("watcher_history_start_failed");
            }
            Err(mpsc::RecvTimeoutError::Timeout) => {
                // The worker has not entered a callback-capable run loop. Dropping the receiver
                // makes its eventual run-loop handoff fail closed and it tears down on its owner
                // thread. Retain the join handle for bounded asynchronous reaping rather than
                // wedging the watcher coordinator on a stalled CoreServices constructor.
                super::super::handle::retain_shutdown_lifecycle_worker(worker);
                return Err("watcher_history_start_timeout");
            }
        };
        let result = match result_rx.recv_timeout(HISTORY_TIMEOUT) {
            Ok(result) => result,
            Err(_) => {
                unsafe { cf::CFRunLoopStop(run_loop.0) };
                let _ = worker.join();
                unsafe { cf::CFRelease(run_loop.0) };
                return Err("watcher_history_timeout");
            }
        };
        let _ = worker.join();
        unsafe { cf::CFRelease(run_loop.0) };
        result
    }

    fn replay_on_run_loop(
        root: &Path,
        event_id: u64,
        run_loop_tx: mpsc::SyncSender<RunLoopHandle>,
    ) -> Result<HistoryReplay, &'static str> {
        let (ready_tx, ready_rx) = mpsc::channel();
        let context = Box::new(HistoryContext {
            root: root.to_path_buf(),
            state: Mutex::new(HistoryState::default()),
            ready_tx,
        });
        let context = Box::into_raw(context);
        let stream = match unsafe { create_stream(root, event_id, context) } {
            Ok(stream) => stream,
            Err(error) => {
                unsafe { drop(Box::from_raw(context)) };
                return Err(error);
            }
        };
        let run_loop = unsafe { cf::CFRunLoopGetCurrent() };
        unsafe {
            fs::FSEventStreamScheduleWithRunLoop(stream, run_loop, cf::kCFRunLoopDefaultMode);
            if fs::FSEventStreamStart(stream) == 0 {
                fs::FSEventStreamInvalidate(stream);
                fs::FSEventStreamRelease(stream);
                drop(Box::from_raw(context));
                return Err("watcher_history_start_failed");
            }
        }
        let retained_run_loop = unsafe { CFRetain(run_loop) as cf::CFRunLoopRef };
        if run_loop_tx.send(RunLoopHandle(retained_run_loop)).is_err() {
            unsafe {
                cf::CFRelease(retained_run_loop);
                fs::FSEventStreamStop(stream);
                fs::FSEventStreamInvalidate(stream);
                fs::FSEventStreamRelease(stream);
                drop(Box::from_raw(context));
            }
            return Err("watcher_history_start_timeout");
        }
        // `HistoryDone` is delivered on this run loop and stops it in the callback. The outer
        // receiver timeout in `replay` bounds a wedged CoreServices stream without ever blocking
        // the watcher coordinator or the UI thread.
        unsafe { cf::CFRunLoopRun() };
        let completed = ready_rx.try_recv().is_ok();
        unsafe {
            fs::FSEventStreamStop(stream);
            fs::FSEventStreamInvalidate(stream);
            fs::FSEventStreamRelease(stream);
        }
        let context = unsafe { Box::from_raw(context) };
        if !completed || !context.state.lock().expect("history state").history_done {
            return Err("watcher_history_timeout");
        }
        let mut state = context.state.into_inner().expect("history state");
        if state.history_lost {
            return Err("watcher_history_gap");
        }
        state.paths.sort();
        state.paths.dedup();
        Ok(HistoryReplay {
            paths: state.paths,
            event_id: state.latest_event_id.max(event_id),
        })
    }

    unsafe fn create_stream(
        root: &Path,
        event_id: u64,
        context: *mut HistoryContext,
    ) -> Result<fs::FSEventStreamRef, &'static str> {
        let root = root.to_str().ok_or("watcher_root_not_utf8")?;
        let mut error = ptr::null_mut();
        let path = unsafe { cf::str_path_to_cfstring_ref(root, &mut error) };
        if path.is_null() {
            return Err("watcher_history_path_unavailable");
        }
        let paths = unsafe {
            cf::CFArrayCreateMutable(cf::kCFAllocatorDefault, 1, &cf::kCFTypeArrayCallBacks)
        };
        if paths.is_null() {
            unsafe { cf::CFRelease(path) };
            return Err("watcher_history_path_unavailable");
        }
        unsafe {
            cf::CFArrayAppendValue(paths, path);
            cf::CFRelease(path);
        }
        let stream_context = fs::FSEventStreamContext {
            version: 0,
            info: context.cast::<c_void>(),
            retain: None,
            release: None,
            copy_description: None,
        };
        let stream = unsafe {
            fs::FSEventStreamCreate(
                cf::kCFAllocatorDefault,
                history_callback,
                &stream_context,
                paths,
                event_id,
                0.0,
                fs::kFSEventStreamCreateFlagFileEvents | fs::kFSEventStreamCreateFlagNoDefer,
            )
        };
        unsafe { cf::CFRelease(paths) };
        if stream.is_null() {
            return Err("watcher_history_create_failed");
        }
        Ok(stream)
    }

    extern "C" fn history_callback(
        _stream: fs::FSEventStreamRef,
        info: *mut c_void,
        count: usize,
        event_paths: *mut c_void,
        event_flags: *const fs::FSEventStreamEventFlags,
        event_ids: *const fs::FSEventStreamEventId,
    ) {
        // The FSEvents callback owns only this short mutex and never reaches the GUI or SQLite;
        // a stalled history stream is therefore bounded by the outer timeout.
        let context = unsafe { &*(info.cast::<HistoryContext>()) };
        let paths = event_paths.cast::<*const std::ffi::c_char>();
        let mut state = context.state.lock().expect("history state");
        for index in 0..count {
            let flags = unsafe { *event_flags.add(index) };
            state.latest_event_id = state.latest_event_id.max(unsafe { *event_ids.add(index) });
            if flags & HISTORY_LOSS_FLAGS != 0 {
                state.history_lost = true;
            }
            if flags & fs::kFSEventStreamEventFlagHistoryDone != 0 {
                state.history_done = true;
                let _ = context.ready_tx.send(());
                unsafe { cf::CFRunLoopStop(cf::CFRunLoopGetCurrent()) };
                continue;
            }
            let path = unsafe { CStr::from_ptr(*paths.add(index)) };
            let path = PathBuf::from(path.to_string_lossy().into_owned());
            if path.starts_with(&context.root) {
                state.paths.push(path);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use wavecrate::sample_sources::SourceId;
    use wavecrate_library::sample_sources::SourceDatabase;

    fn revision_bound_checkpoint(event_id: u64) -> RevisionBoundCheckpoint {
        RevisionBoundCheckpoint {
            source_id: "source-a".to_string(),
            lifecycle_generation: 4,
            source_revision: 9,
            root_identity: "root-a".to_string(),
            event_id,
            cause: CheckpointCause::TargetedReplay,
        }
    }

    fn decide(
        current: Option<&SourceWatcherCheckpoint>,
        requested: &RevisionBoundCheckpoint,
    ) -> CheckpointAdvanceOutcome {
        decide_checkpoint_advance(current, requested, "source-a", 4, 9, "root-a")
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn empty_replayed_paths_require_a_conservative_audit() {
        let directory = tempfile::tempdir().expect("source root");
        let source = SampleSource::new_with_id(
            SourceId::from_string("empty-replay"),
            directory.path().to_path_buf(),
        );

        assert_eq!(
            classify_replayed_paths(&source, Vec::new(), 11),
            JournalRecovery::FullAudit {
                reason: "journal_replay_empty_paths",
            }
        );
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn nonempty_replayed_paths_remain_targeted_changes() {
        let directory = tempfile::tempdir().expect("source root");
        let source = SampleSource::new_with_id(
            SourceId::from_string("nonempty-replay"),
            directory.path().to_path_buf(),
        );

        assert_eq!(
            classify_replayed_paths(&source, vec![source.root.join("kick.wav")], 11,),
            JournalRecovery::Changes {
                paths: vec![PathBuf::from("kick.wav")],
                event_id: 11,
            }
        );
    }

    #[test]
    fn legacy_checkpoint_json_remains_deserializable() {
        let checkpoint = parse_checkpoint(r#"{"root_identity":"root-a","event_id":7}"#)
            .expect("legacy checkpoint JSON");

        assert_eq!(checkpoint.root_identity, "root-a");
        assert_eq!(checkpoint.event_id, 7);
        assert_eq!(checkpoint.format_version, None);
        assert_eq!(checkpoint.revision_bound(), None);
        assert_eq!(
            serde_json::to_string(&checkpoint).expect("legacy checkpoint JSON"),
            r#"{"root_identity":"root-a","event_id":7}"#
        );
    }

    #[test]
    fn complete_extension_checkpoint_without_v2_format_requires_an_audit() {
        let requested = revision_bound_checkpoint(8);
        let checkpoints = [
            r#"{"root_identity":"root-a","event_id":7,"source_id":"source-a","lifecycle_generation":4,"source_revision":9,"cause":"targeted_replay"}"#,
            r#"{"root_identity":"root-a","event_id":7,"format_version":null,"source_id":"source-a","lifecycle_generation":4,"source_revision":9,"cause":"targeted_replay"}"#,
        ];

        for value in checkpoints {
            assert!(parse_checkpoint(value).is_err());
            let checkpoint: SourceWatcherCheckpoint =
                serde_json::from_str(value).expect("extension checkpoint JSON");
            assert_eq!(checkpoint.revision_bound(), None);
            assert_eq!(
                decide(Some(&checkpoint), &requested),
                CheckpointAdvanceOutcome::AuditRequired
            );
        }
    }

    #[test]
    fn unknown_checkpoint_fields_fail_closed() {
        let json = r#"{
            "root_identity":"root-a",
            "event_id":7,
            "format_version":2,
            "source_id":"source-a",
            "lifecycle_generation":4,
            "source_revision":9,
            "cause":"targeted_replay",
            "unexpected":"evidence"
        }"#;

        assert!(parse_checkpoint(json).is_err());
    }

    #[test]
    fn checkpoint_parser_rejects_missing_required_fields_and_invalid_json() {
        for value in [
            "{not-json",
            "null",
            "[]",
            "{}",
            r#"{"root_identity":"root-a"}"#,
            r#"{"event_id":7}"#,
            r#"{"root_identity":null,"event_id":7}"#,
            r#"{"root_identity":"root-a","event_id":null}"#,
            r#"{"root_identity":"root-a","event_id":7} trailing-data"#,
        ] {
            assert!(
                parse_checkpoint(value).is_err(),
                "accepted invalid JSON: {value}"
            );
        }
    }

    #[test]
    fn duplicate_checkpoint_members_fail_closed_without_rewriting_bytes() {
        let cases = [
            (
                "duplicate-legacy",
                r#"{"root_identity":"root-a","event_id":7,"event_id":7}"#,
            ),
            (
                "duplicate-v2",
                r#"{"root_identity":"root-a","event_id":7,"format_version":2,"source_id":"duplicate-v2","lifecycle_generation":4,"source_revision":0,"cause":"targeted_replay","format_version":2}"#,
            ),
        ];

        for (source_id, bytes) in cases {
            let directory = tempfile::tempdir().expect("duplicate-evidence source");
            let source = SampleSource::new_with_id(
                SourceId::from_string(source_id),
                directory.path().to_path_buf(),
            );
            let requested = owner_checkpoint(source_id, 4, 0, "root-a", 8);

            assert!(
                parse_checkpoint(bytes).is_err(),
                "accepted {source_id} evidence"
            );
            seed_owner_checkpoint(&source, bytes);
            assert!(
                load_checkpoint(&source).is_err(),
                "loaded {source_id} evidence"
            );
            assert_eq!(
                recover_source(&source, true),
                JournalRecovery::FullAudit {
                    reason: "watcher_checkpoint_unavailable",
                }
            );
            assert_eq!(
                write_revision_bound_checkpoint(&source, &requested, 4, "root-a"),
                CheckpointAdvanceOutcome::AuditRequired
            );
            assert_eq!(owner_checkpoint_bytes(&source).as_deref(), Some(bytes));
        }
    }

    #[test]
    fn checkpoint_parser_rejects_null_v2_field_values() {
        let valid = serde_json::json!({
            "root_identity": "root-a",
            "event_id": 7,
            "format_version": 2,
            "source_id": "source-a",
            "lifecycle_generation": 4,
            "source_revision": 9,
            "cause": "targeted_replay"
        });
        for field in [
            "root_identity",
            "event_id",
            "format_version",
            "source_id",
            "lifecycle_generation",
            "source_revision",
            "cause",
        ] {
            let mut invalid = valid.clone();
            invalid[field] = serde_json::Value::Null;
            assert!(
                parse_checkpoint(&invalid.to_string()).is_err(),
                "accepted null v2 field: {field}"
            );
        }
    }

    #[test]
    fn revision_bound_checkpoint_round_trips_with_version_and_cause() {
        let expected = revision_bound_checkpoint(7);
        let checkpoint = SourceWatcherCheckpoint::from_revision_bound(&expected);
        let json = serde_json::to_string(&checkpoint).expect("revision-bound checkpoint JSON");

        assert!(json.contains(r#""format_version":2"#));
        assert!(json.contains(r#""cause":"targeted_replay""#));
        assert_eq!(
            parse_checkpoint(&json)
                .expect("revision-bound checkpoint JSON")
                .revision_bound(),
            Some(expected)
        );
        assert_eq!(
            serde_json::to_value(CheckpointCause::CompletedFallbackAudit)
                .expect("checkpoint cause JSON"),
            serde_json::json!("completed_fallback_audit")
        );
    }

    #[test]
    fn historical_lifecycle_advances_but_invalid_requests_require_an_audit() {
        let mut historical_request = revision_bound_checkpoint(7);
        historical_request.lifecycle_generation = 3;
        let current = SourceWatcherCheckpoint::from_revision_bound(&historical_request);

        assert_eq!(
            decide(Some(&current), &revision_bound_checkpoint(8)),
            CheckpointAdvanceOutcome::Applied
        );

        let mut mismatched_revision = revision_bound_checkpoint(8);
        mismatched_revision.source_revision = 8;
        assert_eq!(
            decide(Some(&current), &mismatched_revision),
            CheckpointAdvanceOutcome::AuditRequired
        );

        let mut mismatched_root = revision_bound_checkpoint(8);
        mismatched_root.root_identity = "root-b".to_string();
        assert_eq!(
            decide(Some(&current), &mismatched_root),
            CheckpointAdvanceOutcome::AuditRequired
        );

        let regressed_cursor = revision_bound_checkpoint(6);
        assert_eq!(
            decide(Some(&current), &regressed_cursor),
            CheckpointAdvanceOutcome::AuditRequired
        );
    }

    #[test]
    fn configured_source_id_mismatch_requires_an_audit() {
        let requested = RevisionBoundCheckpoint {
            source_id: "source-b".to_string(),
            ..revision_bound_checkpoint(8)
        };
        let current = SourceWatcherCheckpoint::from_revision_bound(&requested);

        assert_eq!(
            decide(Some(&current), &requested),
            CheckpointAdvanceOutcome::AuditRequired
        );
    }

    #[test]
    fn missing_current_checkpoint_requires_an_audit() {
        assert_eq!(
            decide(None, &revision_bound_checkpoint(8)),
            CheckpointAdvanceOutcome::AuditRequired
        );
    }

    #[test]
    fn duplicate_checkpoint_is_idempotent_and_newer_checkpoint_applies() {
        let current_request = revision_bound_checkpoint(7);
        let current = SourceWatcherCheckpoint::from_revision_bound(&current_request);

        assert_eq!(
            decide(Some(&current), &current_request),
            CheckpointAdvanceOutcome::AlreadyApplied
        );
        assert_eq!(
            decide(Some(&current), &revision_bound_checkpoint(8)),
            CheckpointAdvanceOutcome::Applied
        );
    }

    #[test]
    fn historical_v2_baseline_allows_revision_and_lifecycle_advance() {
        let mut historical = revision_bound_checkpoint(7);
        historical.lifecycle_generation = 3;
        historical.source_revision = 8;
        let historical = SourceWatcherCheckpoint::from_revision_bound(&historical);

        assert_eq!(
            decide(Some(&historical), &revision_bound_checkpoint(8)),
            CheckpointAdvanceOutcome::Applied
        );

        let mut equal_event_request = revision_bound_checkpoint(7);
        equal_event_request.cause = CheckpointCause::CompletedFallbackAudit;
        assert_eq!(
            decide(Some(&historical), &equal_event_request),
            CheckpointAdvanceOutcome::Superseded
        );
    }

    #[test]
    fn stored_revision_ahead_of_current_truth_requires_an_audit() {
        let mut historical = revision_bound_checkpoint(7);
        historical.source_revision = 10;
        let historical = SourceWatcherCheckpoint::from_revision_bound(&historical);

        assert_eq!(
            decide(Some(&historical), &revision_bound_checkpoint(8)),
            CheckpointAdvanceOutcome::AuditRequired
        );
    }

    #[test]
    fn legacy_checkpoint_without_revision_bound_evidence_requires_an_audit() {
        let legacy = SourceWatcherCheckpoint::legacy("root-a".to_string(), 7);

        assert_eq!(
            decide(Some(&legacy), &revision_bound_checkpoint(8)),
            CheckpointAdvanceOutcome::AuditRequired
        );
    }

    #[test]
    fn completed_fallback_audit_establishes_or_upgrades_a_valid_cursor() {
        let mut requested = revision_bound_checkpoint(8);
        requested.cause = CheckpointCause::CompletedFallbackAudit;
        let legacy = SourceWatcherCheckpoint::legacy("root-a".to_string(), 7);

        assert_eq!(decide(None, &requested), CheckpointAdvanceOutcome::Applied);
        assert_eq!(
            decide(Some(&legacy), &requested),
            CheckpointAdvanceOutcome::Applied
        );

        let mut current = requested.clone();
        current.event_id = 8;
        assert_eq!(
            decide(
                Some(&SourceWatcherCheckpoint::from_revision_bound(&current)),
                &requested,
            ),
            CheckpointAdvanceOutcome::AlreadyApplied
        );
    }

    #[test]
    fn completed_fallback_audit_never_regresses_or_crosses_root_identity() {
        let mut requested = revision_bound_checkpoint(8);
        requested.cause = CheckpointCause::CompletedFallbackAudit;

        let mut newer = SourceWatcherCheckpoint::legacy("root-a".to_string(), 9);
        assert_eq!(
            decide(Some(&newer), &requested),
            CheckpointAdvanceOutcome::AuditRequired
        );

        newer = SourceWatcherCheckpoint::legacy("root-b".to_string(), 7);
        assert_eq!(
            decide(Some(&newer), &requested),
            CheckpointAdvanceOutcome::AuditRequired
        );
    }

    fn owner_checkpoint(
        source_id: &str,
        lifecycle_generation: u64,
        source_revision: u64,
        root_identity: &str,
        event_id: u64,
    ) -> RevisionBoundCheckpoint {
        RevisionBoundCheckpoint {
            source_id: source_id.to_string(),
            lifecycle_generation,
            source_revision,
            root_identity: root_identity.to_string(),
            event_id,
            cause: CheckpointCause::TargetedReplay,
        }
    }

    fn seed_owner_checkpoint(source: &SampleSource, value: &str) {
        let database = SourceDatabase::open_for_test_fixture_source_write(&source.root)
            .expect("source database");
        let mut batch = database.write_batch().expect("checkpoint seed transaction");
        batch
            .set_metadata(META_SOURCE_WATCHER_CHECKPOINT, value)
            .expect("seed checkpoint metadata");
        batch
            .commit_auxiliary_state()
            .expect("commit checkpoint seed");
    }

    fn owner_checkpoint_bytes(source: &SampleSource) -> Option<String> {
        SourceDatabase::open_for_test_fixture_source_write(&source.root)
            .expect("source database")
            .get_metadata(META_SOURCE_WATCHER_CHECKPOINT)
            .expect("read checkpoint metadata")
    }

    #[test]
    fn owner_commits_valid_checkpoint_idempotently_without_advancing_source_revision() {
        let directory = tempfile::tempdir().expect("source directory");
        let source = SampleSource::new_with_id(
            SourceId::from_string("owner-valid"),
            directory.path().to_path_buf(),
        );
        let current = owner_checkpoint("owner-valid", 4, 0, "root-a", 7);
        let requested = owner_checkpoint("owner-valid", 4, 0, "root-a", 8);
        seed_owner_checkpoint(
            &source,
            &serde_json::to_string(&SourceWatcherCheckpoint::from_revision_bound(&current))
                .expect("serialize checkpoint seed"),
        );

        assert_eq!(
            write_revision_bound_checkpoint(&source, &requested, 4, "root-a"),
            CheckpointAdvanceOutcome::Applied
        );
        let database = SourceDatabase::open_for_test_fixture_source_write(&source.root)
            .expect("source database");
        assert_eq!(database.get_revision().expect("source revision"), 0);
        assert_eq!(
            owner_checkpoint_bytes(&source)
                .and_then(|value| parse_checkpoint(&value).ok())
                .and_then(|checkpoint| checkpoint.revision_bound()),
            Some(requested.clone())
        );
        assert_eq!(
            write_revision_bound_checkpoint(&source, &requested, 4, "root-a"),
            CheckpointAdvanceOutcome::AlreadyApplied
        );
        assert_eq!(
            SourceDatabase::open_for_test_fixture_source_write(&source.root)
                .expect("source database")
                .get_revision()
                .expect("source revision"),
            0
        );
    }

    #[test]
    fn owner_advances_historical_checkpoint_after_manifest_revision_and_lifecycle_advance() {
        let directory = tempfile::tempdir().expect("source directory");
        let source = SampleSource::new_with_id(
            SourceId::from_string("owner-historical-baseline"),
            directory.path().to_path_buf(),
        );
        let historical = owner_checkpoint("owner-historical-baseline", 4, 0, "root-a", 7);
        seed_owner_checkpoint(
            &source,
            &serde_json::to_string(&SourceWatcherCheckpoint::from_revision_bound(&historical))
                .expect("serialize historical checkpoint"),
        );

        let database = SourceDatabase::open_for_test_fixture_source_write(&source.root)
            .expect("source database");
        let initial_revision = database.get_revision().expect("initial source revision");
        let mut batch = database.write_batch().expect("manifest transaction");
        batch
            .upsert_file(std::path::Path::new("fixture.wav"), 1, 1)
            .expect("fixture manifest row");
        let (manifest_revision, _) = batch
            .commit_with_manifest_snapshot()
            .expect("fixture manifest commit");
        assert_eq!(manifest_revision, initial_revision + 1);

        let requested = owner_checkpoint(
            "owner-historical-baseline",
            5,
            manifest_revision,
            "root-a",
            8,
        );
        assert_eq!(
            write_revision_bound_checkpoint(&source, &requested, 5, "root-a"),
            CheckpointAdvanceOutcome::Applied
        );
        assert_eq!(
            owner_checkpoint_bytes(&source)
                .and_then(|value| parse_checkpoint(&value).ok())
                .and_then(|checkpoint| checkpoint.revision_bound()),
            Some(requested)
        );
        assert_eq!(
            SourceDatabase::open_for_test_fixture_source_write(&source.root)
                .expect("reopen source database")
                .get_revision()
                .expect("source revision"),
            manifest_revision
        );
    }

    #[test]
    fn owner_preserves_checkpoint_when_stored_revision_is_ahead_of_current_truth() {
        let directory = tempfile::tempdir().expect("source directory");
        let source = SampleSource::new_with_id(
            SourceId::from_string("owner-future-revision"),
            directory.path().to_path_buf(),
        );
        let historical = owner_checkpoint("owner-future-revision", 4, 1, "root-a", 7);
        let historical_bytes =
            serde_json::to_string(&SourceWatcherCheckpoint::from_revision_bound(&historical))
                .expect("serialize future checkpoint");
        seed_owner_checkpoint(&source, &historical_bytes);

        let requested = owner_checkpoint("owner-future-revision", 5, 0, "root-a", 8);
        assert_eq!(
            write_revision_bound_checkpoint(&source, &requested, 5, "root-a"),
            CheckpointAdvanceOutcome::AuditRequired
        );
        assert_eq!(
            owner_checkpoint_bytes(&source).as_deref(),
            Some(historical_bytes.as_str())
        );
    }

    #[test]
    fn owner_preserves_checkpoint_when_event_regresses() {
        let directory = tempfile::tempdir().expect("source directory");
        let source = SampleSource::new_with_id(
            SourceId::from_string("owner-event-regression"),
            directory.path().to_path_buf(),
        );
        let historical = owner_checkpoint("owner-event-regression", 4, 0, "root-a", 8);
        let historical_bytes =
            serde_json::to_string(&SourceWatcherCheckpoint::from_revision_bound(&historical))
                .expect("serialize historical checkpoint");
        seed_owner_checkpoint(&source, &historical_bytes);

        let requested = owner_checkpoint("owner-event-regression", 5, 0, "root-a", 7);
        assert_eq!(
            write_revision_bound_checkpoint(&source, &requested, 5, "root-a"),
            CheckpointAdvanceOutcome::AuditRequired
        );
        assert_eq!(
            owner_checkpoint_bytes(&source).as_deref(),
            Some(historical_bytes.as_str())
        );
    }

    #[test]
    fn owner_preserves_historical_checkpoint_at_equal_event_boundary() {
        let directory = tempfile::tempdir().expect("source directory");
        let source = SampleSource::new_with_id(
            SourceId::from_string("owner-equal-event"),
            directory.path().to_path_buf(),
        );
        let historical = owner_checkpoint("owner-equal-event", 4, 0, "root-a", 8);
        let historical_bytes =
            serde_json::to_string(&SourceWatcherCheckpoint::from_revision_bound(&historical))
                .expect("serialize historical checkpoint");
        seed_owner_checkpoint(&source, &historical_bytes);

        let database = SourceDatabase::open_for_test_fixture_source_write(&source.root)
            .expect("source database");
        let mut batch = database.write_batch().expect("manifest transaction");
        batch
            .upsert_file(std::path::Path::new("fixture.wav"), 1, 1)
            .expect("fixture manifest row");
        let (manifest_revision, _) = batch
            .commit_with_manifest_snapshot()
            .expect("fixture manifest commit");
        let requested = owner_checkpoint("owner-equal-event", 5, manifest_revision, "root-a", 8);

        assert_eq!(
            write_revision_bound_checkpoint(&source, &requested, 5, "root-a"),
            CheckpointAdvanceOutcome::Superseded
        );
        assert_eq!(
            owner_checkpoint_bytes(&source).as_deref(),
            Some(historical_bytes.as_str())
        );
    }

    #[test]
    fn owner_preserves_missing_legacy_unknown_and_malformed_evidence() {
        let requested = owner_checkpoint("owner-evidence", 4, 0, "root-a", 8);

        let missing_directory = tempfile::tempdir().expect("missing-evidence source");
        let missing_source = SampleSource::new_with_id(
            SourceId::from_string("owner-evidence"),
            missing_directory.path().to_path_buf(),
        );
        assert_eq!(
            write_revision_bound_checkpoint(&missing_source, &requested, 4, "root-a"),
            CheckpointAdvanceOutcome::AuditRequired
        );
        assert_eq!(owner_checkpoint_bytes(&missing_source), None);

        let legacy_directory = tempfile::tempdir().expect("legacy-evidence source");
        let legacy_source = SampleSource::new_with_id(
            SourceId::from_string("owner-evidence"),
            legacy_directory.path().to_path_buf(),
        );
        let legacy_bytes = r#"{"root_identity":"root-a","event_id":7}"#;
        seed_owner_checkpoint(&legacy_source, legacy_bytes);
        assert_eq!(
            write_revision_bound_checkpoint(&legacy_source, &requested, 4, "root-a"),
            CheckpointAdvanceOutcome::AuditRequired
        );
        assert_eq!(
            owner_checkpoint_bytes(&legacy_source).as_deref(),
            Some(legacy_bytes)
        );

        let unknown_directory = tempfile::tempdir().expect("unknown-evidence source");
        let unknown_source = SampleSource::new_with_id(
            SourceId::from_string("owner-evidence"),
            unknown_directory.path().to_path_buf(),
        );
        let unknown_bytes = r#"{"root_identity":"root-a","event_id":7,"format_version":2,"source_id":"owner-evidence","lifecycle_generation":4,"source_revision":0,"cause":"targeted_replay","unexpected":"evidence"}"#;
        seed_owner_checkpoint(&unknown_source, unknown_bytes);
        assert_eq!(
            write_revision_bound_checkpoint(&unknown_source, &requested, 4, "root-a"),
            CheckpointAdvanceOutcome::AuditRequired
        );
        assert_eq!(
            owner_checkpoint_bytes(&unknown_source).as_deref(),
            Some(unknown_bytes)
        );

        let malformed_directory = tempfile::tempdir().expect("malformed-evidence source");
        let malformed_source = SampleSource::new_with_id(
            SourceId::from_string("owner-evidence"),
            malformed_directory.path().to_path_buf(),
        );
        let malformed_bytes = "{not-json";
        seed_owner_checkpoint(&malformed_source, malformed_bytes);
        assert_eq!(
            write_revision_bound_checkpoint(&malformed_source, &requested, 4, "root-a"),
            CheckpointAdvanceOutcome::AuditRequired
        );
        assert_eq!(
            owner_checkpoint_bytes(&malformed_source).as_deref(),
            Some(malformed_bytes)
        );
    }

    #[test]
    fn owner_commits_completed_audit_barrier_over_missing_or_legacy_cursor() {
        for (source_id, initial_bytes) in [
            ("owner-audit-missing", None),
            (
                "owner-audit-legacy",
                Some(r#"{"root_identity":"root-a","event_id":7}"#),
            ),
        ] {
            let directory = tempfile::tempdir().expect("audit-barrier source");
            let source = SampleSource::new_with_id(
                SourceId::from_string(source_id),
                directory.path().to_path_buf(),
            );
            if let Some(bytes) = initial_bytes {
                seed_owner_checkpoint(&source, bytes);
            }
            let mut requested = owner_checkpoint(source_id, 4, 0, "root-a", 8);
            requested.cause = CheckpointCause::CompletedFallbackAudit;

            assert_eq!(
                write_revision_bound_checkpoint(&source, &requested, 4, "root-a"),
                CheckpointAdvanceOutcome::Applied
            );
            assert_eq!(
                owner_checkpoint_bytes(&source)
                    .and_then(|value| parse_checkpoint(&value).ok())
                    .and_then(|checkpoint| checkpoint.revision_bound()),
                Some(requested)
            );
        }
    }

    #[test]
    fn startup_rejects_partial_v2_evidence_and_owner_preserves_bytes() {
        let cases = [
            (
                "omitted",
                r#"{"root_identity":"root-a","event_id":7,"source_id":"owner-v2-omitted","lifecycle_generation":4,"source_revision":0,"cause":"targeted_replay"}"#,
            ),
            (
                "null",
                r#"{"root_identity":"root-a","event_id":7,"format_version":null,"source_id":"owner-v2-null","lifecycle_generation":4,"source_revision":0,"cause":"targeted_replay"}"#,
            ),
            (
                "wrong-version",
                r#"{"root_identity":"root-a","event_id":7,"format_version":1,"source_id":"owner-v2-wrong-version","lifecycle_generation":4,"source_revision":0,"cause":"targeted_replay"}"#,
            ),
            (
                "partial",
                r#"{"root_identity":"root-a","event_id":7,"format_version":2,"source_id":"owner-v2-partial","lifecycle_generation":4,"source_revision":0}"#,
            ),
        ];

        for (label, bytes) in cases {
            let directory = tempfile::tempdir().expect("version-evidence source");
            let source_id = format!("owner-v2-{label}");
            let source = SampleSource::new_with_id(
                SourceId::from_string(source_id.clone()),
                directory.path().to_path_buf(),
            );
            let requested = owner_checkpoint(&source_id, 4, 0, "root-a", 8);

            assert!(
                parse_checkpoint(bytes).is_err(),
                "accepted {label} evidence"
            );
            seed_owner_checkpoint(&source, &bytes);

            assert!(load_checkpoint(&source).is_err(), "loaded {label} evidence");
            assert_eq!(
                recover_source(&source, true),
                JournalRecovery::FullAudit {
                    reason: "watcher_checkpoint_unavailable",
                }
            );
            assert_eq!(
                write_revision_bound_checkpoint(&source, &requested, 4, "root-a"),
                CheckpointAdvanceOutcome::AuditRequired
            );
            assert_eq!(owner_checkpoint_bytes(&source).as_deref(), Some(bytes));
        }
    }

    #[test]
    fn owner_preserves_checkpoint_when_requested_revision_is_not_current() {
        let directory = tempfile::tempdir().expect("source directory");
        let source = SampleSource::new_with_id(
            SourceId::from_string("owner-revision"),
            directory.path().to_path_buf(),
        );
        let current = owner_checkpoint("owner-revision", 4, 0, "root-a", 7);
        let requested = owner_checkpoint("owner-revision", 4, 1, "root-a", 8);
        let current_bytes =
            serde_json::to_string(&SourceWatcherCheckpoint::from_revision_bound(&current))
                .expect("serialize checkpoint seed");
        seed_owner_checkpoint(&source, &current_bytes);

        assert_eq!(
            write_revision_bound_checkpoint(&source, &requested, 4, "root-a"),
            CheckpointAdvanceOutcome::AuditRequired
        );
        assert_eq!(
            owner_checkpoint_bytes(&source).as_deref(),
            Some(current_bytes.as_str())
        );
    }

    #[test]
    fn committed_reconciliation_advances_but_never_regresses_the_cursor() {
        let directory = tempfile::tempdir().expect("source directory");
        let source = SampleSource::new_with_id(
            SourceId::from_string("journal-cursor-advance"),
            directory.path().to_path_buf(),
        );
        let metadata = std::fs::metadata(&source.root).expect("source metadata");
        let root_identity = stable_filesystem_identity(&source.root, &metadata)
            .expect("stable source root identity");
        store_checkpoint(&source, &SourceWatcherCheckpoint::legacy(root_identity, 7))
            .expect("store checkpoint");

        advance_after_reconciliation(std::slice::from_ref(&source), source.id.as_str(), 11);
        advance_after_reconciliation(std::slice::from_ref(&source), source.id.as_str(), 9);

        assert_eq!(
            load_checkpoint(&source)
                .expect("load checkpoint")
                .expect("checkpoint")
                .event_id,
            11
        );
    }
}
