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
use serde::{Deserialize, Serialize};
use wavecrate::sample_sources::{SampleSource, db::SourceDatabase};
use wavecrate_library::{
    filesystem_identity::stable_filesystem_identity,
    sample_sources::db::META_SOURCE_WATCHER_CHECKPOINT,
};

#[derive(Clone, Debug, Deserialize, Serialize)]
struct SourceWatcherCheckpoint {
    root_identity: String,
    event_id: u64,
}

#[derive(Clone, Debug)]
pub(super) struct AuditBarrier(SourceWatcherCheckpoint);

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
            Ok((paths, event_id)) => JournalRecovery::Changes {
                paths: paths
                    .into_iter()
                    .filter(|path| {
                        super::classification::path_is_source_refresh_candidate(
                            path,
                            EventKind::Any,
                        )
                    })
                    .filter_map(|path| path.strip_prefix(&source.root).ok().map(PathBuf::from))
                    .filter(|path| !path.as_os_str().is_empty())
                    .collect(),
                event_id,
            },
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
    Some(AuditBarrier(SourceWatcherCheckpoint {
        root_identity,
        event_id,
    }))
}

/// Commit a pre-audit barrier only after a complete audit. Never sample the current global ID at
/// completion: a live event after the barrier may not yet have committed and must stay replayable.
pub(super) fn commit_audit_barrier(
    sources: &[SampleSource],
    source_id: &str,
    barrier: AuditBarrier,
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
    if root_identity != barrier.0.root_identity {
        return;
    }
    if let Err(error) = store_checkpoint(source, &barrier.0) {
        tracing::warn!(
            source_id,
            "Could not establish post-audit source watcher checkpoint: {error}"
        );
    }
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
        .map(|value| serde_json::from_str(&value).map_err(|error| error.to_string()))
        .transpose()
}

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
        store_checkpoint(
            &source,
            &SourceWatcherCheckpoint {
                root_identity,
                event_id: 7,
            },
        )
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
