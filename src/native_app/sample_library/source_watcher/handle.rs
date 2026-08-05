use notify::{Config, Event, EventHandler, EventKind, PollWatcher, Watcher};
use std::{
    collections::{HashMap, HashSet},
    path::PathBuf,
    sync::{
        Arc, Mutex, OnceLock,
        atomic::{AtomicBool, AtomicU64, Ordering},
        mpsc::{Receiver, Sender, SyncSender, TryRecvError, TrySendError},
    },
    thread,
    time::{Duration, Instant},
};
use wavecrate::sample_sources::{SampleSource, SourceDatabase, SourceId};
use wavecrate_library::sample_sources::reconciliation::{
    AdapterDisposition, AdmissionOutcome, BackendStreamIdentity, CaptureBoundary, DispatchTicket,
    LiveAuditCorrelation, ReconciliationLifecycle, ReconciliationScopeKind, RootIdentity,
    SourceAuditReceipt, WatcherGeneration, coalesce_scopes,
};

use super::admission_lifecycle::{AdmissionLifecycle, FseventsReplayEvidence};
use super::capture::{
    SourceWatcherCapture, capture_event, capture_to_observation_batch, fsevents_replay_observations,
};
use super::classification::retain_source_refresh_candidates;
use super::journal::{self, JournalRecovery};
use super::roots::{
    RootIdentityRecovery, RootWatchUpdate, WatchedRootIdentities, registered_root_identity,
    root_watch_status, update_watched_roots,
};
use super::state::GuiSourceWatchState;
use super::{
    ROOT_REFRESH_AVAILABLE, ROOT_REFRESH_UNAVAILABLE, SOURCE_CHANGE_DEBOUNCE,
    WATCHER_EVENT_QUEUE_CAPACITY, WATCHER_POLL_INTERVAL, WATCHER_RESTART_MAX, WATCHER_RESTART_MIN,
    WATCHER_START_TIMEOUT,
};
use crate::native_app::app::GuiMessage;
use crate::native_app::sample_library::committed_file_mutations::{
    CommittedWatcherEcho, RevisionFirstCursor,
};
use crate::native_app::sample_library::source_watcher::{
    RevisionBoundCheckpoint, WatcherContinuityProof,
};
use crate::native_app::source_processing::SourceProcessingRegistration;

struct ActiveSourceWatcher {
    _watcher: Box<dyn Watcher + Send>,
    ingress_enabled: Arc<AtomicBool>,
    stream_id: u64,
}

static NEXT_STREAM_ID: AtomicU64 = AtomicU64::new(1);

fn next_stream_id() -> u64 {
    NEXT_STREAM_ID.fetch_add(1, Ordering::Relaxed)
}

static NEXT_CAPTURE_BOUNDARY: AtomicU64 = AtomicU64::new(1);

fn next_capture_boundary() -> CaptureBoundary {
    let captured_at = NEXT_CAPTURE_BOUNDARY.fetch_add(1, Ordering::Relaxed);
    // This process-local monotonic value is an opaque callback marker only. It is deliberately
    // not copied into the optional backend sequence fields, which remain absent unless notify
    // supplies real sequence or cookie evidence.
    CaptureBoundary::try_new(captured_at, None, None).expect("capture boundary")
}

#[derive(Debug)]
struct CapturedSourceWatcherCapture {
    capture: SourceWatcherCapture,
    boundary: CaptureBoundary,
}

impl CapturedSourceWatcherCapture {
    fn from_capture(capture: SourceWatcherCapture) -> Self {
        Self {
            capture,
            boundary: next_capture_boundary(),
        }
    }

    #[cfg(test)]
    fn with_boundary(capture: SourceWatcherCapture, boundary: CaptureBoundary) -> Self {
        Self { capture, boundary }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum SourceWatcherBackend {
    Native,
    Polling,
}

struct PendingSourceWatcher {
    result_rx:
        Receiver<Result<(ActiveSourceWatcher, RootWatchUpdate, WatchedRootIdentities), String>>,
    ingress_enabled: Arc<AtomicBool>,
    join_handle: thread::JoinHandle<()>,
    completed_result:
        Option<Result<(ActiveSourceWatcher, RootWatchUpdate, WatchedRootIdentities), String>>,
    started_at: Instant,
    backend: SourceWatcherBackend,
}

/// At most one unresolved constructor is retained per backend.  Constructors cannot be cancelled
/// safely, so a timed-out constructor keeps its slot until it exits and its join handle is reaped.
const MAX_UNRESOLVED_INITIALIZERS: usize = 2;

/// A notify backend may block forever in `Drop` on macOS.  This is the hard ceiling for the
/// dedicated teardown workers; once it is reached the coordinator stays responsive but reports a
/// degraded, fenced watcher instead of creating another thread.
const MAX_UNRESOLVED_TEARDOWNS: usize = 3;

/// Process-lifetime ownership for shutdown handoffs.  These workers only exist while bounded
/// lifecycle work remains; completed handles are joined when the next handoff is registered.
static SHUTDOWN_LIFECYCLE_WORKERS: OnceLock<Mutex<Vec<thread::JoinHandle<()>>>> = OnceLock::new();

pub(super) fn retain_shutdown_lifecycle_worker(worker: thread::JoinHandle<()>) {
    let workers = SHUTDOWN_LIFECYCLE_WORKERS.get_or_init(|| Mutex::new(Vec::new()));
    let mut workers = workers.lock().expect("shutdown lifecycle worker registry");
    let mut index = 0;
    while index < workers.len() {
        if workers[index].is_finished() {
            let finished = workers.swap_remove(index);
            let _ = finished.join();
        } else {
            index += 1;
        }
    }
    workers.push(worker);
}

struct SourceWatcherTeardown {
    workers: Vec<thread::JoinHandle<()>>,
}

struct SourceWatcherLifecycle {
    retired_initializers: Vec<PendingSourceWatcher>,
    teardown: SourceWatcherTeardown,
    retained_watcher: Option<ActiveSourceWatcher>,
}

impl SourceWatcherLifecycle {
    fn reap_until_quiescent(mut self) {
        while !self.is_quiescent() {
            self.teardown.reap_finished();
            reap_retired_initializers(&mut self.retired_initializers, &mut self.teardown);
            self.retry_retained_watcher(false);
            if !self.is_quiescent() {
                thread::sleep(WATCHER_POLL_INTERVAL);
            }
        }
    }

    fn retry_retained_watcher(&mut self, allow_shutdown_reserve: bool) {
        let Some(watcher) = self.retained_watcher.take() else {
            return;
        };
        let result = if allow_shutdown_reserve {
            self.teardown.retire_on_shutdown(watcher)
        } else {
            self.teardown.retire(watcher)
        };
        if let Err(watcher) = result {
            self.retained_watcher = Some(watcher);
        }
    }

    fn is_quiescent(&self) -> bool {
        self.retired_initializers.is_empty()
            && self.teardown.unresolved_count() == 0
            && self.retained_watcher.is_none()
    }
}

fn start_source_watcher_lifecycle_service() -> Result<Sender<SourceWatcherLifecycle>, String> {
    let (lifecycle_tx, lifecycle_rx) = std::sync::mpsc::channel::<SourceWatcherLifecycle>();
    let worker = thread::Builder::new()
        .name("wavecrate-source-watcher-lifecycle".to_string())
        .spawn(move || {
            while let Ok(lifecycle) = lifecycle_rx.recv() {
                lifecycle.reap_until_quiescent();
            }
        })
        .map_err(|error| error.to_string())?;
    retain_shutdown_lifecycle_worker(worker);
    Ok(lifecycle_tx)
}

impl SourceWatcherTeardown {
    fn reap_finished(&mut self) {
        let mut index = 0;
        while index < self.workers.len() {
            if self.workers[index].is_finished() {
                let worker = self.workers.swap_remove(index);
                let _ = worker.join();
            } else {
                index += 1;
            }
        }
    }

    fn retire(&mut self, watcher: ActiveSourceWatcher) -> Result<(), ActiveSourceWatcher> {
        self.retire_with_limit(watcher, MAX_UNRESOLVED_TEARDOWNS)
    }

    fn retire_on_shutdown(
        &mut self,
        watcher: ActiveSourceWatcher,
    ) -> Result<(), ActiveSourceWatcher> {
        // Normal recovery reserves one fixed slot for the coordinator's final active watcher, so
        // shutdown can remain non-blocking even when all regular teardown workers are wedged.
        self.retire_with_limit(watcher, MAX_UNRESOLVED_TEARDOWNS + 1)
    }

    fn retire_with_limit(
        &mut self,
        watcher: ActiveSourceWatcher,
        limit: usize,
    ) -> Result<(), ActiveSourceWatcher> {
        watcher.ingress_enabled.store(false, Ordering::Release);
        self.reap_finished();
        if self.workers.len() >= limit {
            return Err(watcher);
        }
        let pending = Arc::new(std::sync::Mutex::new(Some(watcher)));
        let worker_value = Arc::clone(&pending);
        match thread::Builder::new()
            .name("wavecrate-source-watcher-teardown".to_string())
            .spawn(move || drop(worker_value.lock().expect("teardown watcher lock").take()))
        {
            Ok(worker) => {
                self.workers.push(worker);
                Ok(())
            }
            Err(error) => {
                tracing::warn!("Could not start GUI source watcher teardown worker: {error}");
                Err(pending
                    .lock()
                    .expect("teardown watcher lock")
                    .take()
                    .expect("teardown worker must not take watcher when spawn fails"))
            }
        }
    }

    fn unresolved_count(&self) -> usize {
        self.workers.len()
    }
}

struct SourceWatcherIngress {
    event_tx: SyncSender<CapturedSourceWatcherCapture>,
    overflowed: Arc<AtomicBool>,
    enabled: Arc<AtomicBool>,
    stream_id: u64,
}

impl EventHandler for SourceWatcherIngress {
    fn handle_event(&mut self, event: notify::Result<Event>) {
        let capture = match capture_event(event) {
            SourceWatcherCapture::Notify { event, .. } => SourceWatcherCapture::Notify {
                stream_id: self.stream_id,
                event,
            },
            SourceWatcherCapture::Error { .. } => SourceWatcherCapture::Error {
                stream_id: self.stream_id,
            },
            SourceWatcherCapture::Overflow { .. } => SourceWatcherCapture::Overflow {
                stream_id: self.stream_id,
            },
        };
        if !self.enabled.load(Ordering::Acquire) {
            return;
        }
        match self
            .event_tx
            .try_send(CapturedSourceWatcherCapture::from_capture(capture))
        {
            Ok(()) => {}
            Err(TrySendError::Full(_)) => {
                self.overflowed.store(true, Ordering::Release);
            }
            Err(TrySendError::Disconnected(_)) => {}
        }
    }
}

#[derive(Debug)]
pub(in crate::native_app) struct GuiSourceWatcherHandle {
    command_tx: Sender<GuiSourceWatchCommand>,
    join_handle: Option<thread::JoinHandle<()>>,
    lifecycle_tx: Option<Sender<SourceWatcherLifecycle>>,
}

impl GuiSourceWatcherHandle {
    pub(in crate::native_app) fn spawn(
        registrations: Vec<SourceProcessingRegistration>,
        message_tx: Sender<GuiMessage>,
    ) -> Self {
        let (command_tx, command_rx) = std::sync::mpsc::channel();
        let lifecycle_tx = match start_source_watcher_lifecycle_service() {
            Ok(lifecycle_tx) => Some(lifecycle_tx),
            Err(error) => {
                tracing::error!(
                    "Could not start GUI source watcher lifecycle service; watcher is disabled: {error}"
                );
                None
            }
        };
        let coordinator_lifecycle_tx = lifecycle_tx.clone();
        let handle = thread::spawn(move || match coordinator_lifecycle_tx {
            Some(lifecycle_tx) => {
                run_source_watcher(command_rx, message_tx, registrations, lifecycle_tx)
            }
            None => run_source_watcher_without_lifecycle(command_rx),
        });
        Self {
            command_tx,
            join_handle: Some(handle),
            lifecycle_tx,
        }
    }

    pub(in crate::native_app) fn replace_sources(
        &self,
        registrations: Vec<SourceProcessingRegistration>,
    ) {
        let _ = self
            .command_tx
            .send(GuiSourceWatchCommand::ReplaceSources(registrations));
    }

    pub(in crate::native_app) fn acknowledge_committed_paths(
        &self,
        source_id: SourceId,
        echoes: Vec<CommittedWatcherEcho>,
        cursor: RevisionFirstCursor,
    ) {
        let _ = self
            .command_tx
            .send(GuiSourceWatchCommand::AcknowledgeCommittedPaths {
                source_id,
                echoes,
                cursor,
            });
    }

    pub(in crate::native_app) fn finish_journal_barrier_audit(
        &self,
        source_id: String,
        lifecycle_generation: u64,
        source_revision: Option<u64>,
        complete: bool,
    ) {
        let _ = self
            .command_tx
            .send(GuiSourceWatchCommand::FinishJournalBarrierAudit {
                source_id,
                lifecycle_generation,
                source_revision,
                complete,
            });
    }

    pub(in crate::native_app) fn acknowledge_source_audit_receipt(
        &self,
        receipt: SourceAuditReceipt,
    ) {
        let _ = self
            .command_tx
            .send(GuiSourceWatchCommand::AcknowledgeSourceAuditReceipt { receipt });
    }

    pub(in crate::native_app) fn acknowledge_replay_checkpoint(
        &self,
        request: RevisionBoundCheckpoint,
    ) {
        let _ = self
            .command_tx
            .send(GuiSourceWatchCommand::AcknowledgeReplayCheckpoint { request });
    }

    #[cfg(test)]
    pub(in crate::native_app) fn request_full_reconciliation(&self) {
        let _ = self
            .command_tx
            .send(GuiSourceWatchCommand::ReconcileAllSources);
    }

    #[cfg(test)]
    pub(in crate::native_app) fn force_overflow_for_tests(&self) {
        self.request_full_reconciliation();
    }

    #[cfg(test)]
    pub(in crate::native_app) fn force_restart_for_tests(&self) {
        let _ = self.command_tx.send(GuiSourceWatchCommand::ForceRestart);
    }

    #[cfg(test)]
    pub(in crate::native_app) fn force_root_refresh_for_tests(&self) {
        let _ = self
            .command_tx
            .send(GuiSourceWatchCommand::ForceRootRefresh);
    }

    #[cfg(test)]
    pub(in crate::native_app) fn inject_paths_for_tests(&self, paths: Vec<std::path::PathBuf>) {
        let _ = self
            .command_tx
            .send(GuiSourceWatchCommand::InjectPaths(paths));
    }

    #[cfg(test)]
    pub(super) fn inject_capture_for_tests(&self, capture: SourceWatcherCapture) {
        let _ = self
            .command_tx
            .send(GuiSourceWatchCommand::InjectCapture(capture));
    }

    #[cfg(test)]
    pub(super) fn watcher_is_live_for_tests(&self) -> bool {
        let (status_tx, status_rx) = std::sync::mpsc::channel();
        self.command_tx
            .send(GuiSourceWatchCommand::ReportWatcherLive(status_tx))
            .expect("source watcher should accept a status query");
        status_rx
            .recv_timeout(WATCHER_START_TIMEOUT)
            .expect("source watcher should report its live status")
    }

    #[cfg(any(test, feature = "legacy-controller"))]
    pub(in crate::native_app) fn wait_until_ready(&self, timeout: Duration) -> Result<(), String> {
        let (ready_tx, ready_rx) = std::sync::mpsc::channel();
        self.command_tx
            .send(GuiSourceWatchCommand::AwaitReady(ready_tx))
            .map_err(|_| String::from("request source watcher readiness"))?;
        ready_rx
            .recv_timeout(timeout)
            .map_err(|_| String::from("source watcher did not become ready"))
    }

    #[cfg(test)]
    pub(in crate::native_app) fn wait_until_ready_for_tests(&self) {
        self.wait_until_ready(Duration::from_secs(30))
            .expect("source watcher should become ready");
    }
}

impl Drop for GuiSourceWatcherHandle {
    fn drop(&mut self) {
        let _ = self.command_tx.send(GuiSourceWatchCommand::Shutdown);
        if let Some(handle) = self.join_handle.take() {
            let _ = handle.join();
        }
        self.lifecycle_tx.take();
    }
}

#[derive(Debug)]
enum GuiSourceWatchCommand {
    ReplaceSources(Vec<SourceProcessingRegistration>),
    #[cfg(test)]
    ReconcileAllSources,
    AcknowledgeCommittedPaths {
        source_id: SourceId,
        echoes: Vec<CommittedWatcherEcho>,
        cursor: RevisionFirstCursor,
    },
    AcknowledgeSourceAuditReceipt {
        receipt: SourceAuditReceipt,
    },
    AcknowledgeReplayCheckpoint {
        request: RevisionBoundCheckpoint,
    },
    FinishJournalBarrierAudit {
        source_id: String,
        lifecycle_generation: u64,
        source_revision: Option<u64>,
        complete: bool,
    },
    #[cfg(test)]
    ForceRestart,
    #[cfg(test)]
    ForceRootRefresh,
    #[cfg(any(test, feature = "legacy-controller"))]
    AwaitReady(Sender<()>),
    #[cfg(test)]
    InjectPaths(Vec<std::path::PathBuf>),
    #[cfg(test)]
    InjectCapture(SourceWatcherCapture),
    #[cfg(test)]
    ReportWatcherLive(Sender<bool>),
    Shutdown,
}

fn run_source_watcher(
    command_rx: Receiver<GuiSourceWatchCommand>,
    message_tx: Sender<GuiMessage>,
    initial_registrations: Vec<SourceProcessingRegistration>,
    lifecycle_tx: Sender<SourceWatcherLifecycle>,
) {
    let (event_tx, event_rx) =
        std::sync::mpsc::sync_channel::<CapturedSourceWatcherCapture>(WATCHER_EVENT_QUEUE_CAPACITY);
    let ingress_overflowed = Arc::new(AtomicBool::new(false));
    let mut watcher = None;
    let mut pending_watcher = None;
    let mut retired_initializers = Vec::with_capacity(MAX_UNRESOLVED_INITIALIZERS);
    let mut teardown = SourceWatcherTeardown {
        workers: Vec::new(),
    };
    let mut state = GuiSourceWatchState::default();
    state.set_registrations(initial_registrations);
    let mut admission_lifecycle = AdmissionLifecycle::new();
    state.set_audit_request_capacity(admission_lifecycle.max_source_audit_request_entries());
    let mut pending_capture_contexts = HashMap::<DispatchTicket, PendingCaptureContext>::new();
    let mut pending_replay_checkpoints = HashMap::<DispatchTicket, PendingReplayCheckpoint>::new();
    let mut next_restart = Instant::now();
    let mut restart_delay = WATCHER_RESTART_MIN;
    let mut next_root_refresh = Instant::now();
    let mut root_identity_recovery = RootIdentityRecovery::default();
    let mut audit_barriers = HashMap::<String, journal::AuditBarrier>::new();
    let mut deferred_audit_barrier_sources = HashSet::new();
    let mut watcher_has_been_ready = false;
    let mut watcher_unavailable_reported = false;
    #[cfg(any(test, feature = "legacy-controller"))]
    let mut readiness_waiters = Vec::<Sender<()>>::new();

    loop {
        match command_rx.recv_timeout(WATCHER_POLL_INTERVAL) {
            Ok(GuiSourceWatchCommand::ReplaceSources(registrations)) => {
                let now = Instant::now();
                let sources = registrations
                    .iter()
                    .map(|registration| registration.source.clone())
                    .collect::<Vec<_>>();
                let roots_changed =
                    desired_watched_roots(&sources) != desired_watched_roots(&state.sources);
                state.set_registrations(registrations);
                if !reconcile_watcher_admission(
                    &mut state,
                    &mut admission_lifecycle,
                    "source-list replacement",
                ) {
                    retire_source_watcher(&mut watcher, &mut teardown);
                    cancel_pending_source_watcher(&mut pending_watcher, &mut retired_initializers);
                    if watcher_has_been_ready {
                        state.reset_watches(now);
                    } else {
                        state.clear_watches();
                    }
                    next_restart = now + restart_delay;
                    restart_delay = doubled_backoff(restart_delay);
                } else if roots_changed {
                    fence_watcher_admission(
                        &mut state,
                        &mut admission_lifecycle,
                        "source-list watcher reset",
                        now,
                    );
                    retire_source_watcher(&mut watcher, &mut teardown);
                    cancel_pending_source_watcher(&mut pending_watcher, &mut retired_initializers);
                    state.reset_watches(now);
                    next_restart = now;
                    restart_delay = WATCHER_RESTART_MIN;
                }
                next_root_refresh = now;
            }
            Ok(GuiSourceWatchCommand::AcknowledgeCommittedPaths {
                source_id,
                echoes,
                cursor,
            }) => {
                state.acknowledge_committed_paths(
                    source_id.as_str(),
                    &echoes,
                    cursor,
                    Instant::now(),
                );
            }
            Ok(GuiSourceWatchCommand::AcknowledgeSourceAuditReceipt { receipt }) => {
                let outcome = admission_lifecycle.acknowledge_source_audit_receipt(&receipt);
                tracing::debug!(
                    cleared_markers = outcome.cleared_markers(),
                    remaining_markers = outcome.remaining_markers(),
                    complete = receipt.is_complete(),
                    "Applied source manifest audit receipt to retained watcher uncertainty"
                );
            }
            Ok(GuiSourceWatchCommand::AcknowledgeReplayCheckpoint { request }) => {
                let sources = state.sources.clone();
                acknowledge_replay_checkpoint(
                    &mut state,
                    &mut admission_lifecycle,
                    &mut pending_replay_checkpoints,
                    &sources,
                    request,
                    Instant::now(),
                );
            }
            Ok(GuiSourceWatchCommand::FinishJournalBarrierAudit {
                source_id,
                lifecycle_generation,
                source_revision,
                complete,
            }) => {
                finish_journal_barrier_audit(
                    &message_tx,
                    &state.sources,
                    &mut audit_barriers,
                    &mut deferred_audit_barrier_sources,
                    source_id,
                    lifecycle_generation,
                    source_revision,
                    complete,
                );
            }
            #[cfg(test)]
            Ok(GuiSourceWatchCommand::ReconcileAllSources) => {
                state.mark_all_overflowed(Instant::now());
            }
            #[cfg(test)]
            Ok(GuiSourceWatchCommand::ForceRestart) => {
                let now = Instant::now();
                fence_watcher_admission(
                    &mut state,
                    &mut admission_lifecycle,
                    "forced watcher restart",
                    now,
                );
                retire_source_watcher(&mut watcher, &mut teardown);
                cancel_pending_source_watcher(&mut pending_watcher, &mut retired_initializers);
                state.reset_watches(now);
                next_restart = now;
                restart_delay = WATCHER_RESTART_MIN;
            }
            #[cfg(test)]
            Ok(GuiSourceWatchCommand::ForceRootRefresh) => {
                next_root_refresh = Instant::now();
            }
            #[cfg(any(test, feature = "legacy-controller"))]
            Ok(GuiSourceWatchCommand::AwaitReady(ready_tx)) => {
                readiness_waiters.push(ready_tx);
            }
            #[cfg(test)]
            Ok(GuiSourceWatchCommand::InjectPaths(paths)) => {
                let event = paths
                    .into_iter()
                    .fold(Event::new(EventKind::Any), Event::add_path);
                let stream_id = watcher
                    .as_ref()
                    .map(|watcher| watcher.stream_id)
                    .unwrap_or(0);
                let capture = match capture_event(Ok(event)) {
                    SourceWatcherCapture::Notify { event, .. } => {
                        SourceWatcherCapture::Notify { stream_id, event }
                    }
                    SourceWatcherCapture::Error { .. } => SourceWatcherCapture::Error { stream_id },
                    SourceWatcherCapture::Overflow { .. } => {
                        SourceWatcherCapture::Overflow { stream_id }
                    }
                };
                match event_tx.try_send(CapturedSourceWatcherCapture::from_capture(capture)) {
                    Ok(()) => {}
                    Err(TrySendError::Full(_)) => {
                        ingress_overflowed.store(true, Ordering::Release);
                    }
                    Err(TrySendError::Disconnected(_)) => {}
                }
            }
            #[cfg(test)]
            Ok(GuiSourceWatchCommand::InjectCapture(capture)) => {
                match event_tx.try_send(CapturedSourceWatcherCapture::from_capture(capture)) {
                    Ok(()) => {}
                    Err(TrySendError::Full(_)) => {
                        ingress_overflowed.store(true, Ordering::Release);
                    }
                    Err(TrySendError::Disconnected(_)) => {}
                }
            }
            #[cfg(test)]
            Ok(GuiSourceWatchCommand::ReportWatcherLive(status_tx)) => {
                let is_live = watcher
                    .as_ref()
                    .is_some_and(|watcher| watcher.ingress_enabled.load(Ordering::Acquire));
                let _ = status_tx.send(is_live);
            }
            Ok(GuiSourceWatchCommand::Shutdown) => {
                let now = Instant::now();
                fence_watcher_admission(
                    &mut state,
                    &mut admission_lifecycle,
                    "watcher shutdown",
                    now,
                );
                cancel_pending_source_watcher(&mut pending_watcher, &mut retired_initializers);
                retire_source_watcher_on_shutdown(&mut watcher, &mut teardown);
                break;
            }
            Err(std::sync::mpsc::RecvTimeoutError::Timeout) => {}
            Err(std::sync::mpsc::RecvTimeoutError::Disconnected) => {
                let now = Instant::now();
                fence_watcher_admission(
                    &mut state,
                    &mut admission_lifecycle,
                    "watcher command disconnect",
                    now,
                );
                cancel_pending_source_watcher(&mut pending_watcher, &mut retired_initializers);
                retire_source_watcher_on_shutdown(&mut watcher, &mut teardown);
                break;
            }
        }

        let now = Instant::now();
        teardown.reap_finished();
        reap_retired_initializers(&mut retired_initializers, &mut teardown);
        if watcher
            .as_ref()
            .is_some_and(|watcher| !watcher.ingress_enabled.load(Ordering::Acquire))
        {
            // A saturated teardown lane retains a fenced watcher in this slot.  Retry the
            // handoff as capacity is reclaimed; it must never be mistaken for a live watcher.
            retire_source_watcher(&mut watcher, &mut teardown);
        }
        if watcher.is_none() && pending_watcher.is_none() && now >= next_restart {
            if let Some(backend) = next_available_backend(&pending_watcher, &retired_initializers) {
                if !start_pending_source_watcher(
                    &mut pending_watcher,
                    &retired_initializers,
                    state.sources.clone(),
                    event_tx.clone(),
                    Arc::clone(&ingress_overflowed),
                    backend,
                ) {
                    if !watcher_has_been_ready {
                        publish_watcher_unavailable_fallback(
                            &message_tx,
                            &state.sources,
                            &mut watcher_unavailable_reported,
                        );
                    }
                    next_restart = now + restart_delay;
                    restart_delay = doubled_backoff(restart_delay);
                }
            } else {
                tracing::warn!(
                    unresolved_initializers = retired_initializers.len(),
                    max_unresolved_initializers = MAX_UNRESOLVED_INITIALIZERS,
                    "All GUI source watcher initializer slots are unresolved; backing off recovery"
                );
                if !watcher_has_been_ready {
                    publish_watcher_unavailable_fallback(
                        &message_tx,
                        &state.sources,
                        &mut watcher_unavailable_reported,
                    );
                }
                next_restart = now + restart_delay;
                restart_delay = doubled_backoff(restart_delay);
            }
        }
        if let Some(pending) = pending_watcher.take() {
            let backend = pending.backend;
            match pending.result_rx.try_recv() {
                Ok(Ok((restarted, update, watched_roots))) => {
                    let _ = pending.join_handle.join();
                    state.watched_roots = watched_roots;
                    let (unavailable, watch_failed) =
                        state.apply_root_watch_update(update, now, false);
                    if watch_failed {
                        fence_watcher_admission(
                            &mut state,
                            &mut admission_lifecycle,
                            "partial watcher installation",
                            now,
                        );
                        if let Err(restarted) =
                            retire_source_watcher_value(restarted, &mut teardown)
                        {
                            watcher = Some(restarted);
                        }
                        if watcher_has_been_ready {
                            state.reset_watches(now);
                        }
                        if backend == SourceWatcherBackend::Native && watcher.is_none() {
                            tracing::warn!(
                                "Native GUI source watcher could not register every root; \
                                 falling back to polling"
                            );
                            if !start_pending_source_watcher(
                                &mut pending_watcher,
                                &retired_initializers,
                                state.sources.clone(),
                                event_tx.clone(),
                                Arc::clone(&ingress_overflowed),
                                SourceWatcherBackend::Polling,
                            ) {
                                next_restart = now + restart_delay;
                                restart_delay = doubled_backoff(restart_delay);
                            }
                        } else {
                            next_restart = now + restart_delay;
                            restart_delay = doubled_backoff(restart_delay);
                        }
                    } else {
                        let lifecycle_ready = reconcile_watcher_admission(
                            &mut state,
                            &mut admission_lifecycle,
                            "successful watcher installation",
                        );
                        if !lifecycle_ready {
                            if let Err(restarted) =
                                retire_source_watcher_value(restarted, &mut teardown)
                            {
                                watcher = Some(restarted);
                            }
                            if watcher_has_been_ready {
                                state.reset_watches(now);
                            } else {
                                state.clear_watches();
                            }
                            if !watcher_has_been_ready {
                                publish_watcher_unavailable_fallback(
                                    &message_tx,
                                    &state.sources,
                                    &mut watcher_unavailable_reported,
                                );
                            }
                            next_restart = now + restart_delay;
                            restart_delay = doubled_backoff(restart_delay);
                            continue;
                        }
                        restarted.ingress_enabled.store(true, Ordering::Release);
                        let first_ready = !watcher_has_been_ready;
                        let recovered_after_unavailability = watcher_unavailable_reported;
                        watcher_has_been_ready = true;
                        watcher = Some(restarted);
                        if first_ready {
                            // Registration callbacks were fenced while every root was installed.
                            // Now that ingress is live, replay the durable macOS journal before
                            // admitting the lifecycle probe. A history gap is scoped to the one
                            // affected source and deliberately falls back to its bounded audit.
                            let recovery_sources = state.sources.clone();
                            publish_closed_app_journal_recovery(
                                &message_tx,
                                &mut state,
                                &recovery_sources,
                                backend == SourceWatcherBackend::Native,
                                &mut admission_lifecycle,
                                &mut pending_capture_contexts,
                                &mut audit_barriers,
                                &mut deferred_audit_barrier_sources,
                                recovered_after_unavailability,
                            );
                            let _ = message_tx.send(GuiMessage::SourceWatcherReady {
                                deferred_audit_sources: deferred_audit_barrier_sources
                                    .iter()
                                    .cloned()
                                    .collect(),
                            });
                        }
                        watcher_unavailable_reported = false;
                        restart_delay = WATCHER_RESTART_MIN;
                        next_root_refresh = now
                            + if unavailable {
                                ROOT_REFRESH_UNAVAILABLE
                            } else {
                                ROOT_REFRESH_AVAILABLE
                            };
                    }
                }
                Ok(Err(error)) => {
                    let _ = pending.join_handle.join();
                    fence_watcher_admission(
                        &mut state,
                        &mut admission_lifecycle,
                        "watcher initialization failure",
                        now,
                    );
                    tracing::warn!(
                        ?backend,
                        retry_ms = restart_delay.as_millis(),
                        "Failed to initialize GUI source watcher: {error}"
                    );
                    if watcher_has_been_ready {
                        state.mark_all_overflowed(now);
                    }
                    if backend == SourceWatcherBackend::Native {
                        if !start_pending_source_watcher(
                            &mut pending_watcher,
                            &retired_initializers,
                            state.sources.clone(),
                            event_tx.clone(),
                            Arc::clone(&ingress_overflowed),
                            SourceWatcherBackend::Polling,
                        ) {
                            next_restart = now + restart_delay;
                            restart_delay = doubled_backoff(restart_delay);
                        }
                    } else {
                        if !watcher_has_been_ready {
                            publish_watcher_unavailable_fallback(
                                &message_tx,
                                &state.sources,
                                &mut watcher_unavailable_reported,
                            );
                        }
                        next_restart = now + restart_delay;
                        restart_delay = doubled_backoff(restart_delay);
                    }
                }
                Err(TryRecvError::Empty)
                    if now.saturating_duration_since(pending.started_at)
                        < WATCHER_START_TIMEOUT =>
                {
                    pending_watcher = Some(pending);
                }
                Err(TryRecvError::Empty) => {
                    fence_watcher_admission(
                        &mut state,
                        &mut admission_lifecycle,
                        "watcher initialization timeout",
                        now,
                    );
                    pending.ingress_enabled.store(false, Ordering::Release);
                    debug_assert!(
                        retired_initializers
                            .iter()
                            .all(|retired| retired.backend != pending.backend)
                    );
                    retired_initializers.push(pending);
                    debug_assert!(retired_initializers.len() <= MAX_UNRESOLVED_INITIALIZERS);
                    tracing::warn!(
                        backend = ?retired_initializers.last().expect("timed-out initializer").backend,
                        timeout_ms = WATCHER_START_TIMEOUT.as_millis(),
                        retry_ms = restart_delay.as_millis(),
                        "Timed out initializing GUI source watcher"
                    );
                    if watcher_has_been_ready {
                        state.mark_all_overflowed(now);
                    }
                    if retired_initializers
                        .last()
                        .expect("timed-out initializer")
                        .backend
                        == SourceWatcherBackend::Native
                    {
                        if !start_pending_source_watcher(
                            &mut pending_watcher,
                            &retired_initializers,
                            state.sources.clone(),
                            event_tx.clone(),
                            Arc::clone(&ingress_overflowed),
                            SourceWatcherBackend::Polling,
                        ) {
                            next_restart = now + restart_delay;
                            restart_delay = doubled_backoff(restart_delay);
                        }
                    } else {
                        if !watcher_has_been_ready {
                            publish_watcher_unavailable_fallback(
                                &message_tx,
                                &state.sources,
                                &mut watcher_unavailable_reported,
                            );
                        }
                        next_restart = now + restart_delay;
                        restart_delay = doubled_backoff(restart_delay);
                    }
                }
                Err(TryRecvError::Disconnected) => {
                    let _ = pending.join_handle.join();
                    fence_watcher_admission(
                        &mut state,
                        &mut admission_lifecycle,
                        "watcher initializer disconnect",
                        now,
                    );
                    tracing::warn!(
                        ?backend,
                        retry_ms = restart_delay.as_millis(),
                        "GUI source watcher initializer exited without a result"
                    );
                    if watcher_has_been_ready {
                        state.mark_all_overflowed(now);
                    }
                    if backend == SourceWatcherBackend::Native {
                        if !start_pending_source_watcher(
                            &mut pending_watcher,
                            &retired_initializers,
                            state.sources.clone(),
                            event_tx.clone(),
                            Arc::clone(&ingress_overflowed),
                            SourceWatcherBackend::Polling,
                        ) {
                            next_restart = now + restart_delay;
                            restart_delay = doubled_backoff(restart_delay);
                        }
                    } else {
                        if !watcher_has_been_ready {
                            publish_watcher_unavailable_fallback(
                                &message_tx,
                                &state.sources,
                                &mut watcher_unavailable_reported,
                            );
                        }
                        next_restart = now + restart_delay;
                        restart_delay = doubled_backoff(restart_delay);
                    }
                }
            }
        }
        #[cfg(any(test, feature = "legacy-controller"))]
        if watcher.is_some() {
            for ready in readiness_waiters.drain(..) {
                let _ = ready.send(());
            }
        }

        if now >= next_root_refresh && watcher.is_some() && pending_watcher.is_none() {
            let status = root_watch_status(&state.watched_roots, &state.sources);
            let mut invalidated_roots = status.changed_roots;
            invalidated_roots
                .extend(root_identity_recovery.due_roots(&status.uncertain_roots, now));
            invalidated_roots.sort();
            invalidated_roots.dedup();
            if !invalidated_roots.is_empty() {
                tracing::warn!(
                    roots = ?invalidated_roots,
                    "Source root availability or filesystem identity changed; restarting watcher"
                );
                state.mark_roots_overflowed(&invalidated_roots, now);
                fence_watcher_admission(
                    &mut state,
                    &mut admission_lifecycle,
                    "root identity refresh",
                    now,
                );
                retire_source_watcher(&mut watcher, &mut teardown);
                cancel_pending_source_watcher(&mut pending_watcher, &mut retired_initializers);
                state.clear_watches();
                next_restart = now;
                restart_delay = WATCHER_RESTART_MIN;
            }
            next_root_refresh = now
                + if status.has_unavailable_roots {
                    ROOT_REFRESH_UNAVAILABLE
                } else {
                    ROOT_REFRESH_AVAILABLE
                };
        }

        if ingress_overflowed.swap(false, Ordering::AcqRel) {
            tracing::warn!("GUI source watcher event queue overflowed; reconciling every source");
            state.mark_all_overflowed(now);
        }

        let (watcher_failed, root_invalidated) = drain_watcher_captures_with_replay(
            &mut state,
            &mut admission_lifecycle,
            watcher.as_ref(),
            &event_rx,
            &mut pending_capture_contexts,
            &mut pending_replay_checkpoints,
            &message_tx,
            now,
        );

        if watcher_failed || root_invalidated {
            fence_watcher_admission(
                &mut state,
                &mut admission_lifecycle,
                "watcher capture retirement",
                now,
            );
            retire_source_watcher(&mut watcher, &mut teardown);
            cancel_pending_source_watcher(&mut pending_watcher, &mut retired_initializers);
            if watcher_failed {
                state.reset_watches(now);
                next_restart = now + restart_delay;
                restart_delay = doubled_backoff(restart_delay);
            } else {
                state.clear_watches();
                next_restart = now;
                restart_delay = WATCHER_RESTART_MIN;
            }
        }

        flush_pending_audit_requests(&mut state, &admission_lifecycle, &message_tx, now);

        for event in state.drain_ready_sources(now, SOURCE_CHANGE_DEBOUNCE) {
            tracing::debug!(
                source_id = %event.source_id,
                overflowed = event.overflowed,
                source_root_available = event.source_root_available,
                paths = ?event.paths,
                "Publishing debounced GUI source watcher event"
            );
            let _ = message_tx.send(GuiMessage::SourceFilesystemChanged {
                source_id: event.source_id,
                // Debounced native events are the explicit compatibility lane: no typed
                // normalized scope is available after the live event collector coalesces them.
                scopes: None,
                paths: event.paths,
                overflowed: event.overflowed,
                source_root_available: event.source_root_available,
                source_root_identity: None,
                lifecycle_generation: None,
                journal_checkpoint_event_id: None,
                watcher_continuity_proof: None,
            });
        }
    }

    let mut lifecycle = SourceWatcherLifecycle {
        retired_initializers,
        teardown,
        retained_watcher: watcher,
    };
    lifecycle.retry_retained_watcher(true);
    if !lifecycle.is_quiescent() {
        tracing::warn!(
            unresolved_initializers = lifecycle.retired_initializers.len(),
            unresolved_teardowns = lifecycle.teardown.unresolved_count(),
            retained_watcher = lifecycle.retained_watcher.is_some(),
            "GUI source watcher coordinator stopped with bounded lifecycle work still in flight"
        );
    }
    if !lifecycle.is_quiescent() {
        lifecycle_tx
            .send(lifecycle)
            .expect("source watcher lifecycle service must outlive its coordinator");
    }
}

fn fence_watcher_admission(
    state: &mut GuiSourceWatchState,
    admission: &mut AdmissionLifecycle,
    boundary: &'static str,
    now: Instant,
) {
    if let Err(error) = admission.fence_all() {
        tracing::error!(
            boundary,
            ?error,
            "Could not fully fence native watcher admission; keeping backend ingress closed"
        );
    }
    purge_stale_audit_requests(state, admission, now);
}

fn reconcile_watcher_admission(
    state: &mut GuiSourceWatchState,
    admission: &mut AdmissionLifecycle,
    boundary: &'static str,
) -> bool {
    let result = admission.reconcile(&state.sources, &state.watched_roots);
    purge_stale_audit_requests(state, admission, Instant::now());
    match result {
        Ok(()) => true,
        Err(error) => {
            tracing::error!(
                boundary,
                ?error,
                "Native watcher admission lifecycle failed closed"
            );
            fence_watcher_admission(state, admission, boundary, Instant::now());
            false
        }
    }
}

fn purge_stale_audit_requests(
    state: &mut GuiSourceWatchState,
    admission: &AdmissionLifecycle,
    now: Instant,
) {
    let displaced = state.purge_non_matching_audit_requests(|request| {
        admission.request_matches_current_capturing_lane(request)
    });
    if !displaced.is_empty() {
        tracing::warn!(
            request_count = displaced.len(),
            "Purged watcher audit requests from a stopped or replaced admission lane"
        );
        state.recover_displaced_audit_requests(&displaced, now);
    }
}

struct SourceCaptureTarget {
    source_id: SourceId,
    source_root: PathBuf,
    paths: Vec<PathBuf>,
    conservative: bool,
}

struct PendingCaptureContext {
    source_id: SourceId,
    source_root: PathBuf,
    root_identity: RootIdentity,
    stream_id: u64,
    watcher_generation: WatcherGeneration,
    capture_boundary: CaptureBoundary,
    original_event: Option<Event>,
    compatibility_event: Option<Event>,
    conservative: bool,
    correlation: Option<LiveAuditCorrelation>,
    replay: Option<PendingReplayHandoff>,
}

struct PendingReplayHandoff {
    paths: Vec<PathBuf>,
    proof: WatcherContinuityProof,
    continuity_proven: bool,
    source_processing_lifecycle_generation: u64,
}

#[derive(Clone)]
struct PendingReplayCheckpoint {
    source_id: SourceId,
    source_root: PathBuf,
    root_identity: RootIdentity,
    owner_watcher_generation: WatcherGeneration,
    source_processing_lifecycle_generation: u64,
    proof: WatcherContinuityProof,
}

fn replay_registration_matches_current(
    state: &GuiSourceWatchState,
    source_id: &SourceId,
    source_root: &PathBuf,
    lifecycle_generation: u64,
) -> bool {
    state
        .registration_for_source(source_id.as_str())
        .is_some_and(|registration| {
            registration.source.id == *source_id
                && registration.source.root == *source_root
                && registration.lifecycle_generation == lifecycle_generation
        })
}

fn capture_stream_id(capture: &SourceWatcherCapture) -> u64 {
    match capture {
        SourceWatcherCapture::Notify { stream_id, .. }
        | SourceWatcherCapture::Error { stream_id }
        | SourceWatcherCapture::Overflow { stream_id } => *stream_id,
    }
}

fn source_capture_targets(
    capture: &SourceWatcherCapture,
    sources: &[SampleSource],
) -> Vec<SourceCaptureTarget> {
    let Some(event) = (match capture {
        SourceWatcherCapture::Notify { event, .. } => Some(event),
        SourceWatcherCapture::Error { .. } | SourceWatcherCapture::Overflow { .. } => None,
    }) else {
        return sources
            .iter()
            .map(|source| SourceCaptureTarget {
                source_id: source.id.clone(),
                source_root: source.root.clone(),
                paths: Vec::new(),
                conservative: true,
            })
            .collect();
    };

    if event.paths.is_empty() {
        return sources
            .iter()
            .map(|source| SourceCaptureTarget {
                source_id: source.id.clone(),
                source_root: source.root.clone(),
                paths: Vec::new(),
                conservative: true,
            })
            .collect();
    }

    let mut targets = Vec::<SourceCaptureTarget>::new();
    let mut ambiguous = false;
    for path in &event.paths {
        let mut matches = sources
            .iter()
            .filter(|source| path.starts_with(&source.root))
            .collect::<Vec<_>>();
        let deepest_root = matches
            .iter()
            .map(|source| source.root.components().count())
            .max();
        matches.retain(|source| Some(source.root.components().count()) == deepest_root);
        if matches.is_empty() || matches.len() > 1 {
            ambiguous = true;
        }
        for source in matches {
            if let Some(target) = targets
                .iter_mut()
                .find(|target| target.source_id == source.id)
            {
                target.paths.push(path.clone());
            } else {
                targets.push(SourceCaptureTarget {
                    source_id: source.id.clone(),
                    source_root: source.root.clone(),
                    paths: vec![path.clone()],
                    conservative: false,
                });
            }
        }
    }

    if ambiguous {
        return sources
            .iter()
            .map(|source| SourceCaptureTarget {
                source_id: source.id.clone(),
                source_root: source.root.clone(),
                paths: Vec::new(),
                conservative: true,
            })
            .collect();
    }
    if targets.len() > 1 {
        return targets
            .into_iter()
            .map(|target| SourceCaptureTarget {
                source_id: target.source_id,
                source_root: target.source_root,
                paths: Vec::new(),
                conservative: true,
            })
            .collect();
    }
    targets
}

fn capture_for_target(
    capture: &SourceWatcherCapture,
    target: &SourceCaptureTarget,
) -> SourceWatcherCapture {
    match capture {
        SourceWatcherCapture::Notify { stream_id, event } => {
            let event = if target.conservative {
                let mut event = event.clone();
                // Keep bounded callback attributes (including Rescan/tracker) while replacing
                // path-bearing evidence with an explicit pathless observation. The normalizer
                // widens this to SourceAudit, but the non-marker raw kind still receives a ticket
                // so the exact original Event remains retained through handoff.
                event.kind = EventKind::Create(notify::event::CreateKind::Any);
                event.paths.clear();
                event
            } else {
                let mut event = event.clone();
                event.paths = target.paths.clone();
                event
            };
            SourceWatcherCapture::Notify {
                stream_id: *stream_id,
                event,
            }
        }
        SourceWatcherCapture::Error { stream_id } => SourceWatcherCapture::Error {
            stream_id: *stream_id,
        },
        SourceWatcherCapture::Overflow { stream_id } => SourceWatcherCapture::Overflow {
            stream_id: *stream_id,
        },
    }
}

fn widen_source(state: &mut GuiSourceWatchState, source_root: &PathBuf, now: Instant) {
    state.mark_roots_overflowed(std::slice::from_ref(source_root), now);
}

fn admit_capture_target(
    state: &mut GuiSourceWatchState,
    admission: &mut AdmissionLifecycle,
    capture: &SourceWatcherCapture,
    boundary: CaptureBoundary,
    target: SourceCaptureTarget,
    pending_contexts: &mut HashMap<DispatchTicket, PendingCaptureContext>,
    now: Instant,
) {
    let Some(lane) = admission.lane_for_capture(&target.source_id) else {
        tracing::warn!(
            source_id = target.source_id.as_str(),
            "Native watcher capture had no identity-qualified admission lane"
        );
        widen_source(state, &target.source_root, now);
        return;
    };
    if lane.lifecycle() != ReconciliationLifecycle::Capturing {
        tracing::warn!(
            source_id = target.source_id.as_str(),
            generation = lane.generation().get(),
            "Native watcher capture arrived outside a capturing admission lane"
        );
        widen_source(state, &target.source_root, now);
        return;
    }

    let context_limit = admission
        .max_in_flight()
        .min(super::MAX_PENDING_CAPTURE_CONTEXTS);
    if pending_contexts.len() >= context_limit {
        if let Some((request, marker_backed)) =
            admission.source_audit_request_for_current_lane(&target.source_id)
        {
            state.enqueue_audit_request(request, marker_backed);
        }
        tracing::warn!(
            source_id = target.source_id.as_str(),
            context_limit,
            "Native watcher admission context pressure widened source"
        );
        widen_source(state, &target.source_root, now);
        return;
    }

    let original_event = match capture {
        SourceWatcherCapture::Notify { event, .. } => Some(event.clone()),
        _ => None,
    };
    let compatibility_event = match capture {
        SourceWatcherCapture::Notify { event, .. } if !target.conservative => {
            let mut event = event.clone();
            event.paths = target.paths.clone();
            Some(event)
        }
        _ => None,
    };
    let stream_id = capture_stream_id(capture);
    let capture = capture_for_target(capture, &target);
    let batch = match capture_to_observation_batch(
        capture,
        &target.source_root,
        target.source_id.clone(),
        lane.root_identity().clone(),
        lane.generation(),
        boundary,
    ) {
        Ok(batch) => batch,
        Err(error) => {
            tracing::warn!(
                source_id = target.source_id.as_str(),
                ?error,
                "Native watcher capture could not be mapped to a root-relative observation"
            );
            widen_source(state, &target.source_root, now);
            return;
        }
    };

    let live = match admission.admit_live_with_correlation(batch) {
        Ok(live) => live,
        Err(error) => {
            tracing::warn!(
                source_id = target.source_id.as_str(),
                ?error,
                "Native watcher live admission failed closed"
            );
            widen_source(state, &target.source_root, now);
            return;
        }
    };
    let outcome = live.admission().outcome().clone();
    let audit_request_to_queue = match &outcome {
        AdmissionOutcome::Accepted(_) if live.correlation().is_some() => None,
        _ => live.audit_request().cloned().map(|request| {
            let marker_backed = admission.source_audit_request_is_marker_backed(&request);
            (request, marker_backed)
        }),
    };
    if let Some((request, marker_backed)) = audit_request_to_queue {
        state.enqueue_audit_request(request, marker_backed);
    }
    match outcome {
        AdmissionOutcome::Accepted(ticket) => {
            let source_id = target.source_id.clone();
            let context = PendingCaptureContext {
                source_id: target.source_id,
                source_root: target.source_root,
                root_identity: lane.root_identity().clone(),
                stream_id,
                watcher_generation: lane.generation(),
                capture_boundary: boundary,
                original_event,
                compatibility_event,
                conservative: target.conservative,
                correlation: live.correlation().cloned(),
                replay: None,
            };
            if let Some(previous) = pending_contexts.insert(ticket, context) {
                tracing::error!(
                    ticket = ticket.id(),
                    source_id = previous.source_id.as_str(),
                    "Native watcher admission ticket unexpectedly replaced its handoff context"
                );
                state.mark_all_overflowed(now);
            }
            if live.correlation().is_none() {
                tracing::error!(
                    ticket = ticket.id(),
                    source_id = source_id.as_str(),
                    "Accepted live admission did not return its retained audit correlation"
                );
            }
        }
        AdmissionOutcome::DuplicateSuppressed(_) => {
            tracing::debug!(
                source_id = target.source_id.as_str(),
                "Suppressing duplicate native watcher observation"
            );
        }
        AdmissionOutcome::Rejected(reason) => {
            tracing::warn!(
                source_id = target.source_id.as_str(),
                ?reason,
                "Native watcher evidence was retained as conservative uncertainty"
            );
            widen_source(state, &target.source_root, now);
        }
        AdmissionOutcome::UncertaintyCapacityExhausted(_) => {
            tracing::error!(
                source_id = target.source_id.as_str(),
                "Native watcher uncertainty capacity was exhausted"
            );
            widen_source(state, &target.source_root, now);
        }
    }
}

fn handoff_dispatched_capture(
    state: &mut GuiSourceWatchState,
    message_tx: &Sender<GuiMessage>,
    context: &PendingCaptureContext,
    dispatched: &wavecrate_library::sample_sources::reconciliation::DispatchedObservation,
    now: Instant,
) -> (bool, bool) {
    let provenance = dispatched.normalized().envelope().provenance();
    let expected_stream = context
        .replay
        .as_ref()
        .and_then(|replay| BackendStreamIdentity::from_fsevents_device(replay.proof.backend_device))
        .unwrap_or_else(|| {
            BackendStreamIdentity::from_bytes(context.stream_id.to_be_bytes().to_vec())
        });
    let correlation_matches = context.replay.is_some()
        || context
            .correlation
            .as_ref()
            .is_some_and(|correlation| correlation.ticket() == dispatched.ticket());
    let provenance_matches = provenance.source_id() == &context.source_id
        && provenance.root_identity() == Some(&context.root_identity)
        && provenance.backend_stream_identity() == Some(&expected_stream)
        && provenance.watcher_generation() == context.watcher_generation
        && provenance.capture_boundary() == context.capture_boundary;
    if !correlation_matches || !provenance_matches {
        tracing::error!(
            source_id = context.source_id.as_str(),
            ticket = dispatched.ticket().id(),
            correlation_matches,
            provenance_matches,
            "Native watcher dispatch provenance did not match its retained capture context"
        );
        state.mark_all_overflowed(now);
        return (false, false);
    }
    let scopes = coalesce_scopes(dispatched.normalized().scopes().iter().cloned());
    let source_audit = context.conservative
        || scopes
            .iter()
            .any(|scope| scope.kind() == ReconciliationScopeKind::SourceAudit);
    if let Some(correlation) = context.correlation.as_ref() {
        state.enqueue_audit_request(correlation.audit_request(), true);
    }
    if let Some(replay) = context.replay.as_ref()
        && !replay.continuity_proven
    {
        tracing::warn!(
            source_id = context.source_id.as_str(),
            ticket = dispatched.ticket().id(),
            "Closed-application replay lacked durable continuity; retaining source-scoped audit fallback"
        );
        state.mark_source_overflowed(context.source_id.as_str(), now);
        return (true, false);
    }
    if let Some(replay) = context.replay.as_ref()
        && !replay_registration_matches_current(
            state,
            &context.source_id,
            &context.source_root,
            replay.source_processing_lifecycle_generation,
        )
    {
        tracing::warn!(
            source_id = context.source_id.as_str(),
            ticket = dispatched.ticket().id(),
            lifecycle_generation = replay.source_processing_lifecycle_generation,
            "Closed-application replay registration was replaced before typed handoff; retaining source audit fallback"
        );
        state.mark_source_overflowed(context.source_id.as_str(), now);
        return (false, false);
    }
    if source_audit && context.replay.is_none() {
        widen_source(state, &context.source_root, now);
        return (true, false);
    }

    if let Some(replay) = context.replay.as_ref() {
        if message_tx
            .send(GuiMessage::SourceFilesystemChanged {
                source_id: context.source_id.as_str().to_string(),
                scopes: Some(scopes),
                paths: replay.paths.clone(),
                overflowed: false,
                source_root_available: true,
                source_root_identity: Some(replay.proof.root_identity.clone()),
                lifecycle_generation: Some(replay.source_processing_lifecycle_generation),
                journal_checkpoint_event_id: Some(replay.proof.acknowledged_end_event_id),
                watcher_continuity_proof: Some(replay.proof.clone()),
            })
            .is_err()
        {
            tracing::warn!(
                source_id = context.source_id.as_str(),
                "Source-processing channel closed before replay handoff"
            );
            return (false, true);
        }
        return (true, false);
    }

    let Some(compatibility_event) = context.compatibility_event.as_ref() else {
        tracing::error!(
            source_id = context.source_id.as_str(),
            original_event_retained = context.original_event.is_some(),
            "Native watcher dispatch has no compatible source-refresh event"
        );
        state.mark_all_overflowed(now);
        return (false, false);
    };
    let mut event = compatibility_event.clone();
    if !retain_source_refresh_candidates(&mut event) {
        return (true, false);
    }
    (true, state.collect_event(&event, now))
}

fn dispatch_pending_capture_contexts_with_replay(
    state: &mut GuiSourceWatchState,
    admission: &mut AdmissionLifecycle,
    pending_contexts: &mut HashMap<DispatchTicket, PendingCaptureContext>,
    pending_replay_checkpoints: &mut HashMap<DispatchTicket, PendingReplayCheckpoint>,
    message_tx: &Sender<GuiMessage>,
    now: Instant,
) -> bool {
    prune_stale_replay_checkpoints(admission, pending_replay_checkpoints);
    let mut root_invalidated = false;
    if admission.in_flight() == 0 && !pending_contexts.is_empty() {
        tracing::error!(
            context_count = pending_contexts.len(),
            "Native watcher admission fence retired pending capture contexts before handoff"
        );
        for (_, context) in pending_contexts.drain() {
            widen_source(state, &context.source_root, now);
        }
    }
    if admission.in_flight() == 0 && !pending_replay_checkpoints.is_empty() {
        tracing::error!(
            checkpoint_count = pending_replay_checkpoints.len(),
            "Native watcher admission fence retired replay checkpoint contexts"
        );
        pending_replay_checkpoints.clear();
    }
    while let Some(dispatched) = admission.dispatch_next() {
        let ticket = dispatched.ticket();
        let Some(context) = pending_contexts.remove(&ticket) else {
            tracing::error!(
                ticket = ticket.id(),
                "Native watcher dispatch ticket had no retained handoff context"
            );
            state.mark_all_overflowed(now);
            root_invalidated = true;
            if let Err(error) = admission.mark_dispatched(ticket) {
                tracing::error!(
                    ticket = ticket.id(),
                    ?error,
                    "Could not mark orphaned watcher dispatch"
                );
                break;
            }
            if let Err(error) = admission.mark_applied(ticket) {
                tracing::error!(
                    ticket = ticket.id(),
                    ?error,
                    "Could not mark orphaned watcher application"
                );
                break;
            }
            continue;
        };

        if let Err(error) = admission.mark_dispatched(ticket) {
            tracing::error!(
                ticket = ticket.id(),
                ?error,
                "Could not mark native watcher dispatch"
            );
            widen_source(state, &context.source_root, now);
            pending_contexts.insert(ticket, context);
            break;
        }
        let (handoff_succeeded, invalidated) =
            handoff_dispatched_capture(state, message_tx, &context, &dispatched, now);
        root_invalidated |= invalidated;
        if !handoff_succeeded {
            tracing::error!(
                ticket = ticket.id(),
                source_id = context.source_id.as_str(),
                "Native watcher handoff failed closed"
            );
        }
        if let Err(error) = admission.mark_applied(ticket) {
            tracing::error!(
                ticket = ticket.id(),
                ?error,
                "Could not mark native watcher application"
            );
            widen_source(state, &context.source_root, now);
            pending_contexts.insert(ticket, context);
            break;
        }
        if let Some(replay) = context.replay.as_ref()
            && replay.continuity_proven
            && handoff_succeeded
            && !invalidated
        {
            pending_replay_checkpoints.insert(
                ticket,
                PendingReplayCheckpoint {
                    source_id: context.source_id.clone(),
                    source_root: context.source_root.clone(),
                    root_identity: context.root_identity.clone(),
                    owner_watcher_generation: context.watcher_generation,
                    source_processing_lifecycle_generation: replay
                        .source_processing_lifecycle_generation,
                    proof: replay.proof.clone(),
                },
            );
            continue;
        }
        if context.replay.is_some() && !handoff_succeeded {
            root_invalidated = true;
            continue;
        }
        if let Err(error) = admission.mark_unproven_audit_handed_off(ticket) {
            tracing::error!(
                ticket = ticket.id(),
                ?error,
                "Could not retire native watcher proofless dispatch"
            );
            widen_source(state, &context.source_root, now);
            pending_contexts.insert(ticket, context);
            break;
        }
    }
    root_invalidated
}

fn prune_stale_replay_checkpoints(
    admission: &AdmissionLifecycle,
    pending_replay_checkpoints: &mut HashMap<DispatchTicket, PendingReplayCheckpoint>,
) {
    pending_replay_checkpoints.retain(|ticket, pending| {
        let current_lane = admission.lane_for_capture(&pending.source_id);
        let current = current_lane.as_ref();
        let current_matches = current.is_some_and(|lane| {
            lane.lifecycle() == ReconciliationLifecycle::Capturing
                && lane.root_identity() == &pending.root_identity
                && lane.generation() == pending.owner_watcher_generation
        });
        if !current_matches {
            tracing::debug!(
                ticket = ticket.id(),
                source_id = pending.source_id.as_str(),
                "Pruning replay checkpoint from a retired watcher lane"
            );
        }
        current_matches
    });
}

#[cfg(test)]
fn dispatch_pending_capture_contexts(
    state: &mut GuiSourceWatchState,
    admission: &mut AdmissionLifecycle,
    pending_contexts: &mut HashMap<DispatchTicket, PendingCaptureContext>,
    now: Instant,
) -> bool {
    let mut pending_replay_checkpoints = HashMap::new();
    let (message_tx, _message_rx) = std::sync::mpsc::channel();
    dispatch_pending_capture_contexts_with_replay(
        state,
        admission,
        pending_contexts,
        &mut pending_replay_checkpoints,
        &message_tx,
        now,
    )
}

fn flush_pending_audit_requests(
    state: &mut GuiSourceWatchState,
    admission: &AdmissionLifecycle,
    message_tx: &Sender<GuiMessage>,
    now: Instant,
) {
    purge_stale_audit_requests(state, admission, now);
    if state.pending_audit_requests.conservative_recovery_latched() {
        state.mark_all_overflowed(now);
        state
            .pending_audit_requests
            .clear_conservative_recovery_latch();
    }
    while state.pending_audit_requests.front().is_some() {
        // Revalidate immediately before each send. A lifecycle boundary can leave a retained
        // request in the queue until this exact transport point; stale identities must never be
        // emitted, while a failed send below deliberately leaves the request at the front.
        purge_stale_audit_requests(state, admission, now);
        let Some(request) = state.pending_audit_requests.front().cloned() else {
            break;
        };
        if !admission.request_matches_current_capturing_lane(&request) {
            continue;
        }
        if message_tx
            .send(GuiMessage::SourceWatcherManifestAuditRequested { request })
            .is_err()
        {
            tracing::warn!("Source-processing request channel closed before live audit handoff");
            break;
        }
        state.pending_audit_requests.pop_front();
    }
}

fn drain_watcher_captures_with_replay(
    state: &mut GuiSourceWatchState,
    admission: &mut AdmissionLifecycle,
    watcher: Option<&ActiveSourceWatcher>,
    event_rx: &Receiver<CapturedSourceWatcherCapture>,
    pending_contexts: &mut HashMap<DispatchTicket, PendingCaptureContext>,
    pending_replay_checkpoints: &mut HashMap<DispatchTicket, PendingReplayCheckpoint>,
    message_tx: &Sender<GuiMessage>,
    now: Instant,
) -> (bool, bool) {
    let mut watcher_failed = false;
    let mut root_invalidated = dispatch_pending_capture_contexts_with_replay(
        state,
        admission,
        pending_contexts,
        pending_replay_checkpoints,
        message_tx,
        now,
    );
    while let Ok(captured) = event_rx.try_recv() {
        let stream_id = capture_stream_id(&captured.capture);
        let current_stream = watcher.is_some_and(|watcher| {
            watcher.stream_id == stream_id && watcher.ingress_enabled.load(Ordering::Acquire)
        });
        if !current_stream {
            state.mark_all_overflowed(now);
            continue;
        }
        if matches!(&captured.capture, SourceWatcherCapture::Error { .. }) {
            tracing::warn!("GUI source watcher error");
            watcher_failed = true;
        }
        if matches!(&captured.capture, SourceWatcherCapture::Overflow { .. }) {
            tracing::warn!("GUI source watcher overflow marker");
        }
        for target in source_capture_targets(&captured.capture, &state.sources) {
            admit_capture_target(
                state,
                admission,
                &captured.capture,
                captured.boundary,
                target,
                pending_contexts,
                now,
            );
        }
    }
    root_invalidated |= dispatch_pending_capture_contexts_with_replay(
        state,
        admission,
        pending_contexts,
        pending_replay_checkpoints,
        message_tx,
        now,
    );
    (watcher_failed, root_invalidated)
}

#[cfg(test)]
fn drain_watcher_captures(
    state: &mut GuiSourceWatchState,
    admission: &mut AdmissionLifecycle,
    watcher: Option<&ActiveSourceWatcher>,
    event_rx: &Receiver<CapturedSourceWatcherCapture>,
    pending_contexts: &mut HashMap<DispatchTicket, PendingCaptureContext>,
    now: Instant,
) -> (bool, bool) {
    let mut pending_replay_checkpoints = HashMap::new();
    let (message_tx, _message_rx) = std::sync::mpsc::channel();
    drain_watcher_captures_with_replay(
        state,
        admission,
        watcher,
        event_rx,
        pending_contexts,
        &mut pending_replay_checkpoints,
        &message_tx,
        now,
    )
}

fn finish_journal_barrier_audit(
    message_tx: &Sender<GuiMessage>,
    sources: &[SampleSource],
    audit_barriers: &mut HashMap<String, journal::AuditBarrier>,
    deferred_audit_barrier_sources: &mut HashSet<String>,
    source_id: String,
    lifecycle_generation: u64,
    source_revision: Option<u64>,
    complete: bool,
) {
    if !complete {
        return;
    }
    let Some(source_revision) = source_revision else {
        tracing::warn!(
            source_id,
            lifecycle_generation,
            "Completed source audit did not provide a committed revision for its watcher barrier"
        );
        return;
    };

    if let Some(barrier) = audit_barriers.remove(&source_id) {
        let checkpoint =
            barrier.into_revision_bound(source_id.clone(), lifecycle_generation, source_revision);
        let _ = message_tx.send(GuiMessage::SourceWatcherCheckpointReady(checkpoint));
    }
    if deferred_audit_barrier_sources.remove(&source_id) {
        // The unavailable-watcher audit predates watcher recovery. Capture only now,
        // after that audit has completed, then schedule a fresh audit tied to this
        // barrier instead of letting the older completion advance a new cursor.
        if let Some(barrier) = journal::capture_audit_barrier(sources, &source_id) {
            audit_barriers.insert(source_id.clone(), barrier);
            let _ = message_tx.send(GuiMessage::SourceWatcherJournalGap {
                source_id,
                reason: "watcher_recovered_after_unavailable",
            });
        }
    }
}

fn request_closed_app_journal_audit(
    message_tx: &Sender<GuiMessage>,
    sources: &[SampleSource],
    audit_barriers: &mut HashMap<String, journal::AuditBarrier>,
    deferred_audit_barrier_sources: &mut HashSet<String>,
    defer_audit_barriers: bool,
    source_id: &SourceId,
    reason: &'static str,
) {
    if defer_audit_barriers {
        deferred_audit_barrier_sources.insert(source_id.as_str().to_string());
        return;
    }
    if let Some(barrier) = journal::capture_audit_barrier(sources, source_id.as_str()) {
        audit_barriers.insert(source_id.as_str().to_string(), barrier);
    }
    let _ = message_tx.send(GuiMessage::SourceWatcherJournalGap {
        source_id: source_id.as_str().to_string(),
        reason,
    });
}

#[cfg(target_os = "macos")]
fn handle_closed_app_replay_fallback(
    state: &mut GuiSourceWatchState,
    source: &SampleSource,
    outcome: &AdmissionOutcome,
    now: Instant,
) {
    match outcome {
        AdmissionOutcome::Rejected(reason) => {
            tracing::warn!(
                source_id = source.id.as_str(),
                ?reason,
                "Closed-application replay remained conservative uncertainty"
            );
        }
        AdmissionOutcome::UncertaintyCapacityExhausted(envelope) => {
            tracing::warn!(
                source_id = source.id.as_str(),
                observations = envelope.observations().len(),
                "Closed-application replay exceeded uncertainty capacity"
            );
        }
        outcome => unreachable!("unexpected closed-application replay fallback: {outcome:?}"),
    }
    state.mark_source_overflowed(source.id.as_str(), now);
}

fn publish_closed_app_journal_recovery(
    message_tx: &Sender<GuiMessage>,
    state: &mut GuiSourceWatchState,
    sources: &[SampleSource],
    native_watcher: bool,
    admission: &mut AdmissionLifecycle,
    pending_contexts: &mut HashMap<DispatchTicket, PendingCaptureContext>,
    audit_barriers: &mut HashMap<String, journal::AuditBarrier>,
    deferred_audit_barrier_sources: &mut HashSet<String>,
    defer_audit_barriers: bool,
) {
    let now = Instant::now();
    for (source, recovery) in sources
        .iter()
        .zip(journal::recover_sources(sources, native_watcher))
    {
        match recovery {
            #[cfg(target_os = "macos")]
            JournalRecovery::Changes { paths, proof, .. } if paths.is_empty() => {
                tracing::info!(
                    source_id = source.id.as_str(),
                    replay_end_event_id = proof.replay_coverage_end_event_id,
                    "Empty closed-application source watcher replay requires a bounded manifest audit"
                );
                request_closed_app_journal_audit(
                    message_tx,
                    sources,
                    audit_barriers,
                    deferred_audit_barrier_sources,
                    defer_audit_barriers,
                    &source.id,
                    "journal_replay_empty_paths",
                );
            }
            #[cfg(target_os = "macos")]
            JournalRecovery::Changes {
                paths,
                proof,
                historical_source_lifecycle_generation,
            } => {
                let Some(registration) = state.registration_for_source(source.id.as_str()).cloned()
                else {
                    tracing::warn!(
                        source_id = source.id.as_str(),
                        "Closed-application replay has no current source-processing registration"
                    );
                    request_closed_app_journal_audit(
                        message_tx,
                        sources,
                        audit_barriers,
                        deferred_audit_barrier_sources,
                        defer_audit_barriers,
                        &source.id,
                        "journal_replay_source_registration_unavailable",
                    );
                    continue;
                };
                let source = &registration.source;
                tracing::info!(
                    source_id = source.id.as_str(),
                    path_count = paths.len(),
                    replay_start_event_id = proof.replay_coverage_start_event_id,
                    replay_end_event_id = proof.replay_coverage_end_event_id,
                    backend_device = proof.backend_device,
                    watcher_generation = proof.watcher_generation,
                    "Replaying closed-application source watcher changes"
                );
                let (observations, limits) = match fsevents_replay_observations(paths.clone()) {
                    Ok(evidence) => evidence,
                    Err(error) => {
                        tracing::warn!(
                            source_id = source.id.as_str(),
                            ?error,
                            "Closed-application replay evidence exceeded native bounds"
                        );
                        request_closed_app_journal_audit(
                            message_tx,
                            sources,
                            audit_barriers,
                            deferred_audit_barrier_sources,
                            defer_audit_barriers,
                            &source.id,
                            "journal_replay_raw_evidence_unbounded",
                        );
                        continue;
                    }
                };
                let Some(lane) = admission.lane_for_capture(&source.id) else {
                    tracing::warn!(
                        source_id = source.id.as_str(),
                        "Closed-application replay has no current owner lane"
                    );
                    request_closed_app_journal_audit(
                        message_tx,
                        sources,
                        audit_barriers,
                        deferred_audit_barrier_sources,
                        defer_audit_barriers,
                        &source.id,
                        "journal_replay_owner_lane_unavailable",
                    );
                    continue;
                };
                let database_root = match source.database_root() {
                    Ok(root) => root,
                    Err(error) => {
                        tracing::warn!(
                            source_id = source.id.as_str(),
                            ?error,
                            "Closed-application replay authority database root unavailable"
                        );
                        request_closed_app_journal_audit(
                            message_tx,
                            sources,
                            audit_barriers,
                            deferred_audit_barrier_sources,
                            defer_audit_barriers,
                            &source.id,
                            "journal_replay_authority_unavailable",
                        );
                        continue;
                    }
                };
                let database = match SourceDatabase::open_for_background_job_with_database_root(
                    &source.root,
                    database_root,
                ) {
                    Ok(database) => database,
                    Err(error) => {
                        tracing::warn!(
                            source_id = source.id.as_str(),
                            ?error,
                            "Closed-application replay authority database unavailable"
                        );
                        request_closed_app_journal_audit(
                            message_tx,
                            sources,
                            audit_barriers,
                            deferred_audit_barrier_sources,
                            defer_audit_barriers,
                            &source.id,
                            "journal_replay_authority_unavailable",
                        );
                        continue;
                    }
                };
                let first_sequence = proof
                    .replay_coverage_start_event_id
                    .checked_add(1)
                    .filter(|first| *first <= proof.replay_coverage_end_event_id);
                let capture_boundary = CaptureBoundary::try_new(
                    proof.replay_coverage_end_event_id,
                    first_sequence,
                    first_sequence.map(|_| proof.replay_coverage_end_event_id),
                )
                .expect("closed-application replay boundary remains ordered");
                let replay_admission = admission.admit_fsevents_replay(
                    source,
                    &database,
                    proof.watcher_generation,
                    FseventsReplayEvidence {
                        source_lifecycle_generation: WatcherGeneration::new(
                            historical_source_lifecycle_generation,
                        ),
                        replay_stream_generation: proof.watcher_generation,
                        backend_device: proof.backend_device,
                        replay_start_event_id: proof.replay_coverage_start_event_id,
                        replay_end_event_id: proof.replay_coverage_end_event_id,
                        observations,
                        limits,
                    },
                );
                let replay_admission = match replay_admission {
                    Ok(admission) => admission,
                    Err(error) => {
                        tracing::warn!(
                            source_id = source.id.as_str(),
                            ?error,
                            "Closed-application replay admission failed closed"
                        );
                        request_closed_app_journal_audit(
                            message_tx,
                            sources,
                            audit_barriers,
                            deferred_audit_barrier_sources,
                            defer_audit_barriers,
                            &source.id,
                            "journal_replay_admission_unavailable",
                        );
                        continue;
                    }
                };
                if let Some(request) = replay_admission.audit_request().cloned() {
                    let marker_backed = admission.source_audit_request_is_marker_backed(&request);
                    state.enqueue_audit_request(request, marker_backed);
                }
                let continuity_proven = replay_admission.admission().disposition()
                    == AdapterDisposition::AdmittedWithContinuity;
                match replay_admission.admission().outcome() {
                    AdmissionOutcome::Accepted(ticket) => {
                        let context = PendingCaptureContext {
                            source_id: source.id.clone(),
                            source_root: source.root.clone(),
                            root_identity: lane.root_identity().clone(),
                            stream_id: 0,
                            watcher_generation: lane.generation(),
                            capture_boundary,
                            original_event: None,
                            compatibility_event: None,
                            conservative: false,
                            correlation: None,
                            replay: Some(PendingReplayHandoff {
                                paths,
                                proof,
                                continuity_proven,
                                source_processing_lifecycle_generation: registration
                                    .lifecycle_generation,
                            }),
                        };
                        pending_contexts.insert(*ticket, context);
                    }
                    AdmissionOutcome::DuplicateSuppressed(_) => {
                        tracing::debug!(
                            source_id = source.id.as_str(),
                            "Suppressing duplicate closed-application replay"
                        );
                    }
                    AdmissionOutcome::Rejected(_)
                    | AdmissionOutcome::UncertaintyCapacityExhausted(_) => {
                        handle_closed_app_replay_fallback(
                            state,
                            source,
                            replay_admission.admission().outcome(),
                            now,
                        );
                    }
                }
            }
            JournalRecovery::FullAudit { reason } => {
                tracing::info!(
                    source_id = source.id.as_str(),
                    reason,
                    "Durable source watcher coverage requires a bounded manifest audit"
                );
                request_closed_app_journal_audit(
                    message_tx,
                    sources,
                    audit_barriers,
                    deferred_audit_barrier_sources,
                    defer_audit_barriers,
                    &source.id,
                    reason,
                );
            }
        }
    }
}

fn acknowledge_replay_checkpoint(
    state: &mut GuiSourceWatchState,
    admission: &mut AdmissionLifecycle,
    pending_replay_checkpoints: &mut HashMap<DispatchTicket, PendingReplayCheckpoint>,
    _sources: &[SampleSource],
    request: RevisionBoundCheckpoint,
    now: Instant,
) {
    let Some(proof) = request.continuity_proof.as_ref() else {
        tracing::warn!(
            source_id = request.source_id.as_str(),
            "Replay checkpoint acknowledgement had no continuity proof"
        );
        return;
    };
    if !journal::targeted_replay_request_has_valid_proof(&request) {
        tracing::warn!(
            source_id = request.source_id.as_str(),
            event_id = request.event_id,
            "Replay checkpoint acknowledgement proof was malformed"
        );
        return;
    }
    let request_root_identity = RootIdentity::from_bytes(request.root_identity.as_bytes().to_vec());
    let Some(ticket) = pending_replay_checkpoints
        .iter()
        .find_map(|(ticket, pending)| {
            (pending.source_id.as_str() == request.source_id
                && pending.root_identity == request_root_identity
                && pending.proof == *proof)
                .then_some(*ticket)
        })
    else {
        tracing::debug!(
            source_id = request.source_id.as_str(),
            event_id = request.event_id,
            "Ignoring replay checkpoint acknowledgement without a matching applied ticket"
        );
        return;
    };
    let Some(pending) = pending_replay_checkpoints.get(&ticket).cloned() else {
        return;
    };
    let source_id = pending.source_id.clone();
    let Some(registration) = state.registration_for_source(source_id.as_str()).cloned() else {
        tracing::warn!(
            source_id = source_id.as_str(),
            "Replay checkpoint acknowledgement has no current source-processing registration"
        );
        pending_replay_checkpoints.remove(&ticket);
        state.mark_source_overflowed(source_id.as_str(), now);
        return;
    };
    if registration.source.root != pending.source_root
        || registration.lifecycle_generation != pending.source_processing_lifecycle_generation
        || request.lifecycle_generation != registration.lifecycle_generation
    {
        tracing::warn!(
            source_id = source_id.as_str(),
            ticket = ticket.id(),
            captured_lifecycle_generation = pending.source_processing_lifecycle_generation,
            current_lifecycle_generation = registration.lifecycle_generation,
            requested_lifecycle_generation = request.lifecycle_generation,
            "Replay checkpoint acknowledgement crossed a source-processing lifecycle boundary; retaining source audit fallback"
        );
        pending_replay_checkpoints.remove(&ticket);
        state.mark_source_overflowed(source_id.as_str(), now);
        return;
    }
    let source = &registration.source;
    let current_source_lifecycle_generation =
        WatcherGeneration::new(registration.lifecycle_generation);
    let database_root = match source.database_root() {
        Ok(root) => root,
        Err(error) => {
            tracing::warn!(
                source_id = source_id.as_str(),
                ?error,
                "Replay checkpoint authority database root was unavailable"
            );
            fail_closed_replay_checkpoint(
                state,
                admission,
                pending_replay_checkpoints,
                source,
                ticket,
                &pending,
                now,
            );
            return;
        }
    };
    let database = match SourceDatabase::open_for_background_job_with_database_root(
        &source.root,
        database_root,
    ) {
        Ok(database) => database,
        Err(error) => {
            tracing::warn!(
                source_id = source_id.as_str(),
                ?error,
                "Replay checkpoint authority database was unavailable"
            );
            fail_closed_replay_checkpoint(
                state,
                admission,
                pending_replay_checkpoints,
                source,
                ticket,
                &pending,
                now,
            );
            return;
        }
    };
    match admission.mark_fsevents_replay_checkpointed(
        &source_id,
        &database,
        current_source_lifecycle_generation,
        ticket,
    ) {
        Ok(()) => {
            pending_replay_checkpoints.remove(&ticket);
            tracing::debug!(
                source_id = source_id.as_str(),
                ticket = ticket.id(),
                event_id = request.event_id,
                "Retired replay admission after durable watcher checkpoint"
            );
        }
        Err(error) => {
            tracing::warn!(
                source_id = source_id.as_str(),
                ticket = ticket.id(),
                category = error.category(),
                ?error,
                "Replay checkpoint authority did not match the applied admission"
            );
            fail_closed_replay_checkpoint(
                state,
                admission,
                pending_replay_checkpoints,
                source,
                ticket,
                &pending,
                now,
            );
        }
    }
}

fn fail_closed_replay_checkpoint(
    state: &mut GuiSourceWatchState,
    admission: &mut AdmissionLifecycle,
    pending_replay_checkpoints: &mut HashMap<DispatchTicket, PendingReplayCheckpoint>,
    source: &SampleSource,
    ticket: DispatchTicket,
    pending: &PendingReplayCheckpoint,
    now: Instant,
) {
    if source.id != pending.source_id || source.root != pending.source_root {
        pending_replay_checkpoints.remove(&ticket);
        tracing::debug!(
            source_id = pending.source_id.as_str(),
            ticket = ticket.id(),
            "Ignoring replay checkpoint failure from a stale source identity"
        );
        return;
    }
    let fenced = match admission.fence_source_if_current(
        &pending.source_id,
        &pending.root_identity,
        pending.owner_watcher_generation,
    ) {
        Ok(fenced) => fenced,
        Err(error) => {
            tracing::error!(
                source_id = pending.source_id.as_str(),
                ticket = ticket.id(),
                ?error,
                "Could not safely fence source-local replay failure; using degraded global fail-closed fallback"
            );
            return global_fail_closed_replay_checkpoint(
                state,
                admission,
                pending_replay_checkpoints,
                &pending.source_id,
                &pending.source_root,
                now,
            );
        }
    };
    if !fenced {
        pending_replay_checkpoints.remove(&ticket);
        tracing::debug!(
            source_id = pending.source_id.as_str(),
            ticket = ticket.id(),
            "Ignoring replay checkpoint failure from a stale source lane"
        );
        return;
    }

    state.mark_source_overflowed(pending.source_id.as_str(), now);
    let Some(root_identity) = registered_root_identity(&state.watched_roots, &source.root) else {
        tracing::warn!(
            source_id = pending.source_id.as_str(),
            ticket = ticket.id(),
            "Source-local replay recovery is waiting for a currently installed watched-root identity"
        );
        pending_replay_checkpoints.remove(&ticket);
        return;
    };
    if let Err(error) = admission.reconcile_source(source, root_identity) {
        tracing::error!(
            source_id = pending.source_id.as_str(),
            ticket = ticket.id(),
            ?error,
            "Could not restart source-local replay lane; using degraded global fail-closed fallback"
        );
        return global_fail_closed_replay_checkpoint(
            state,
            admission,
            pending_replay_checkpoints,
            &pending.source_id,
            &pending.source_root,
            now,
        );
    }

    let Some((request, marker_backed)) =
        admission.source_audit_request_for_current_lane(&pending.source_id)
    else {
        tracing::error!(
            source_id = pending.source_id.as_str(),
            ticket = ticket.id(),
            "Source-local replay recovery restarted without a current typed audit request; using degraded global fail-closed fallback"
        );
        return global_fail_closed_replay_checkpoint(
            state,
            admission,
            pending_replay_checkpoints,
            &pending.source_id,
            &pending.source_root,
            now,
        );
    };
    pending_replay_checkpoints.remove(&ticket);
    state.enqueue_audit_request(request, marker_backed);
}

fn global_fail_closed_replay_checkpoint(
    state: &mut GuiSourceWatchState,
    admission: &mut AdmissionLifecycle,
    pending_replay_checkpoints: &mut HashMap<DispatchTicket, PendingReplayCheckpoint>,
    source_id: &SourceId,
    source_root: &PathBuf,
    now: Instant,
) {
    widen_source(state, source_root, now);
    fence_watcher_admission(state, admission, "degraded replay checkpoint failure", now);
    pending_replay_checkpoints.clear();
    if reconcile_watcher_admission(state, admission, "degraded replay checkpoint recovery")
        && let Some((request, marker_backed)) =
            admission.source_audit_request_for_current_lane(source_id)
    {
        state.enqueue_audit_request(request, marker_backed);
    }
}

/// A watcher that cannot initialize must not hold startup reconciliation hostage. This fallback
/// deliberately has no audit barrier: without a live ingress stream, advancing a durable cursor
/// would risk hiding mutations made during the audit. The old cursor remains replayable when a
/// watcher eventually recovers, while the supervisor gets a bounded, retryable source audit now.
fn publish_watcher_unavailable_fallback(
    message_tx: &Sender<GuiMessage>,
    sources: &[SampleSource],
    already_reported: &mut bool,
) {
    if std::mem::replace(already_reported, true) {
        return;
    }
    tracing::warn!(
        source_count = sources.len(),
        "Source watcher unavailable at startup; admitting bounded source audit fallback"
    );
    for source in sources {
        let _ = message_tx.send(GuiMessage::SourceWatcherJournalGap {
            source_id: source.id.as_str().to_string(),
            reason: "watcher_unavailable",
        });
    }
}

fn run_source_watcher_without_lifecycle(command_rx: Receiver<GuiSourceWatchCommand>) {
    while !matches!(
        command_rx.recv(),
        Ok(GuiSourceWatchCommand::Shutdown) | Err(std::sync::mpsc::RecvError)
    ) {}
}

fn spawn_source_watcher(
    sources: Vec<SampleSource>,
    event_tx: SyncSender<CapturedSourceWatcherCapture>,
    ingress_overflowed: Arc<AtomicBool>,
    backend: SourceWatcherBackend,
) -> Result<PendingSourceWatcher, String> {
    let stream_id = next_stream_id();
    let (result_tx, result_rx) = std::sync::mpsc::channel();
    // Native backends can emit callbacks while roots are being registered.
    // Fence those callbacks and open ingress only once the complete watcher is
    // installed; the watcher-ready audit covers the construction interval.
    let ingress_enabled = Arc::new(AtomicBool::new(false));
    let watcher_enabled = Arc::clone(&ingress_enabled);
    let join_handle = thread::Builder::new()
        .name("wavecrate-source-watcher-start".to_string())
        .spawn(move || {
            let ingress = SourceWatcherIngress {
                event_tx,
                overflowed: ingress_overflowed,
                enabled: Arc::clone(&watcher_enabled),
                stream_id,
            };
            let watcher: Result<Box<dyn Watcher + Send>, String> = match backend {
                SourceWatcherBackend::Native => notify::recommended_watcher(ingress)
                    .map(|watcher| Box::new(watcher) as Box<dyn Watcher + Send>)
                    .map_err(|error| error.to_string()),
                SourceWatcherBackend::Polling => PollWatcher::new(
                    ingress,
                    Config::default().with_poll_interval(Duration::from_secs(1)),
                )
                .map(|watcher| Box::new(watcher) as Box<dyn Watcher + Send>)
                .map_err(|error| error.to_string()),
            };
            let result = watcher.map(|mut watcher| {
                let mut watched_roots = HashMap::new();
                let update = update_watched_roots(watcher.as_mut(), &mut watched_roots, &sources);
                (
                    ActiveSourceWatcher {
                        _watcher: watcher,
                        ingress_enabled: watcher_enabled,
                        stream_id,
                    },
                    update,
                    watched_roots,
                )
            });
            let _ = result_tx.send(result);
        })
        .map_err(|error| error.to_string())?;
    Ok(PendingSourceWatcher {
        result_rx,
        ingress_enabled,
        join_handle,
        completed_result: None,
        started_at: Instant::now(),
        backend,
    })
}

fn has_initializer_for_backend(
    pending: &Option<PendingSourceWatcher>,
    retired_initializers: &[PendingSourceWatcher],
    backend: SourceWatcherBackend,
) -> bool {
    pending
        .as_ref()
        .is_some_and(|pending| pending.backend == backend)
        || retired_initializers
            .iter()
            .any(|pending| pending.backend == backend)
}

fn next_available_backend(
    pending: &Option<PendingSourceWatcher>,
    retired_initializers: &[PendingSourceWatcher],
) -> Option<SourceWatcherBackend> {
    [SourceWatcherBackend::Native, SourceWatcherBackend::Polling]
        .into_iter()
        .find(|backend| !has_initializer_for_backend(pending, retired_initializers, *backend))
}

fn start_pending_source_watcher(
    pending: &mut Option<PendingSourceWatcher>,
    retired_initializers: &[PendingSourceWatcher],
    sources: Vec<SampleSource>,
    event_tx: SyncSender<CapturedSourceWatcherCapture>,
    ingress_overflowed: Arc<AtomicBool>,
    backend: SourceWatcherBackend,
) -> bool {
    if has_initializer_for_backend(pending, retired_initializers, backend) {
        tracing::warn!(
            ?backend,
            unresolved_initializers = retired_initializers.len(),
            max_unresolved_initializers = MAX_UNRESOLVED_INITIALIZERS,
            "GUI source watcher initializer is still unresolved; keeping watcher recovery bounded"
        );
        return false;
    }
    match spawn_source_watcher(sources, event_tx, ingress_overflowed, backend) {
        Ok(watcher) => {
            *pending = Some(watcher);
            true
        }
        Err(error) => {
            tracing::warn!(
                ?backend,
                "Could not start GUI source watcher initializer: {error}"
            );
            false
        }
    }
}

fn reap_retired_initializers(
    retired_initializers: &mut Vec<PendingSourceWatcher>,
    teardown: &mut SourceWatcherTeardown,
) {
    let mut index = 0;
    while index < retired_initializers.len() {
        let result = match retired_initializers[index].completed_result.take() {
            Some(result) => Some(result),
            None => match retired_initializers[index].result_rx.try_recv() {
                Ok(result) => Some(result),
                Err(TryRecvError::Disconnected) => Some(Err(
                    "GUI source watcher initializer exited without a result".to_string(),
                )),
                Err(TryRecvError::Empty) => None,
            },
        };
        let Some(result) = result else {
            index += 1;
            continue;
        };
        match result {
            Ok((watcher, update, watched_roots)) => match teardown.retire(watcher) {
                Ok(()) => {
                    let initializer = retired_initializers.swap_remove(index);
                    let _ = initializer.join_handle.join();
                }
                Err(watcher) => {
                    // Keep the initializer slot occupied until its stale watcher can be handed to
                    // the bounded teardown lane.  This deliberately stops further recovery
                    // attempts instead of dropping on the coordinator or making another reaper.
                    retired_initializers[index]
                        .ingress_enabled
                        .store(false, Ordering::Release);
                    retired_initializers[index].completed_result =
                        Some(Ok((watcher, update, watched_roots)));
                    tracing::warn!(
                        unresolved_teardowns = teardown.unresolved_count(),
                        max_unresolved_teardowns = MAX_UNRESOLVED_TEARDOWNS,
                        "Stale GUI source watcher is waiting for bounded teardown capacity"
                    );
                    index += 1;
                }
            },
            Err(error) => {
                tracing::debug!("Retired GUI source watcher initializer completed: {error}");
                let initializer = retired_initializers.swap_remove(index);
                let _ = initializer.join_handle.join();
            }
        }
    }
}

fn desired_watched_roots(sources: &[SampleSource]) -> HashSet<PathBuf> {
    sources
        .iter()
        .map(|source| source.root.clone())
        .filter(|root| root.is_dir())
        .collect()
}

fn cancel_pending_source_watcher(
    watcher: &mut Option<PendingSourceWatcher>,
    retired_initializers: &mut Vec<PendingSourceWatcher>,
) {
    if let Some(watcher) = watcher.take() {
        watcher.ingress_enabled.store(false, Ordering::Release);
        debug_assert!(
            retired_initializers
                .iter()
                .all(|retired| retired.backend != watcher.backend)
        );
        retired_initializers.push(watcher);
        debug_assert!(retired_initializers.len() <= MAX_UNRESOLVED_INITIALIZERS);
    }
}

/// Stop accepting callbacks before dropping the macOS FSEvents watcher off the coordinator.
///
/// `notify` waits for the Core Foundation run loop to become idle during `Drop`. A busy source can
/// keep that wait inside the watcher coordinator for an unbounded interval, preventing restart,
/// recovery events, and shutdown. Fencing callback ingress makes the old stream quiescent while a
/// short-lived reaper performs the backend-specific blocking teardown.
fn retire_source_watcher(
    watcher_slot: &mut Option<ActiveSourceWatcher>,
    teardown: &mut SourceWatcherTeardown,
) {
    if let Some(watcher) = watcher_slot.take() {
        if let Err(watcher) = teardown.retire(watcher) {
            tracing::warn!(
                unresolved_teardowns = teardown.unresolved_count(),
                max_unresolved_teardowns = MAX_UNRESOLVED_TEARDOWNS,
                "GUI source watcher teardown is saturated; retaining the fenced watcher"
            );
            *watcher_slot = Some(watcher);
        }
    }
}

fn retire_source_watcher_on_shutdown(
    watcher_slot: &mut Option<ActiveSourceWatcher>,
    teardown: &mut SourceWatcherTeardown,
) {
    if let Some(watcher) = watcher_slot.take() {
        if let Err(watcher) = teardown.retire_on_shutdown(watcher) {
            // This can only happen if a regular failure path already saturated every slot and a
            // final reserved shutdown slot.  Retaining the fenced watcher is safer than blocking
            // the coordinator; the diagnostic makes the bounded degradation visible.
            tracing::error!(
                unresolved_teardowns = teardown.unresolved_count(),
                max_unresolved_teardowns = MAX_UNRESOLVED_TEARDOWNS + 1,
                "GUI source watcher shutdown teardown capacity is saturated"
            );
            *watcher_slot = Some(watcher);
        }
    }
}

fn retire_source_watcher_value(
    watcher: ActiveSourceWatcher,
    teardown: &mut SourceWatcherTeardown,
) -> Result<(), ActiveSourceWatcher> {
    teardown.retire(watcher)
}

pub(super) fn doubled_backoff(current: Duration) -> Duration {
    super::doubled_duration(current, WATCHER_RESTART_MAX)
}

#[cfg(test)]
mod lifecycle_tests {
    use super::super::CheckpointCause;
    use super::super::MAX_PENDING_CAPTURE_CONTEXTS;
    use super::super::capture::MAX_CAPTURE_PATHS;
    use super::*;
    use notify::{Event, EventKind, RecursiveMode, WatcherKind};
    use std::{
        collections::{HashMap, HashSet},
        path::Path,
        sync::{
            Mutex, OnceLock,
            mpsc::{Receiver, SyncSender},
        },
    };
    use wavecrate::sample_sources::{SampleSource, SourceId};
    use wavecrate_library::sample_sources::db::META_SOURCE_WATCHER_CHECKPOINT;
    use wavecrate_library::sample_sources::reconciliation::{
        BackendStreamIdentity, CaptureBoundary, RawEventKind, RawObservation,
        RawObservationEnvelope, RawObservationLimits, RawObservationProvenance, RawObservedPath,
        RawPathRole, ReconciliationAdmissionLimits, ReconciliationScopeKind, UncertaintyReason,
    };

    static LIFECYCLE_TEST_LOCK: OnceLock<Mutex<()>> = OnceLock::new();

    fn lock_lifecycle_tests() -> std::sync::MutexGuard<'static, ()> {
        LIFECYCLE_TEST_LOCK
            .get_or_init(|| Mutex::new(()))
            .lock()
            .expect("lifecycle test lock")
    }

    #[test]
    fn startup_watcher_unavailability_admits_one_scoped_fallback_per_source() {
        let source = SampleSource::new_with_id(
            SourceId::from_string("watcher-unavailable"),
            PathBuf::from("/tmp/watcher-unavailable"),
        );
        let (message_tx, message_rx) = std::sync::mpsc::channel();
        let mut reported = false;

        publish_watcher_unavailable_fallback(&message_tx, &[source], &mut reported);
        publish_watcher_unavailable_fallback(&message_tx, &[], &mut reported);

        assert!(matches!(
            message_rx.recv().expect("watcher fallback message"),
            GuiMessage::SourceWatcherJournalGap { source_id, reason }
                if source_id == "watcher-unavailable" && reason == "watcher_unavailable"
        ));
        assert!(matches!(
            message_rx.try_recv(),
            Err(std::sync::mpsc::TryRecvError::Empty)
        ));
    }

    #[test]
    fn incomplete_normal_barrier_remains_pending() {
        let directory = tempfile::tempdir().expect("source root");
        let source = SampleSource::new_with_id(
            SourceId::from_string("incomplete-normal-barrier"),
            directory.path().to_path_buf(),
        );
        let source_id = source.id.as_str().to_string();
        let sources = vec![source];
        let mut audit_barriers = HashMap::new();
        audit_barriers.insert(
            source_id.clone(),
            journal::capture_audit_barrier(&sources, &source_id).expect("audit barrier"),
        );
        let mut deferred_audit_barrier_sources = HashSet::new();
        let (message_tx, message_rx) = std::sync::mpsc::channel();

        finish_journal_barrier_audit(
            &message_tx,
            &sources,
            &mut audit_barriers,
            &mut deferred_audit_barrier_sources,
            source_id.clone(),
            3,
            Some(7),
            false,
        );
        finish_journal_barrier_audit(
            &message_tx,
            &sources,
            &mut audit_barriers,
            &mut deferred_audit_barrier_sources,
            source_id.clone(),
            3,
            None,
            true,
        );

        assert!(audit_barriers.contains_key(&source_id));
        assert!(matches!(
            message_rx.try_recv(),
            Err(std::sync::mpsc::TryRecvError::Empty)
        ));
    }

    #[test]
    fn incomplete_deferred_marker_remains_without_recapture() {
        let directory = tempfile::tempdir().expect("source root");
        let source = SampleSource::new_with_id(
            SourceId::from_string("incomplete-deferred-barrier"),
            directory.path().to_path_buf(),
        );
        let source_id = source.id.as_str().to_string();
        let sources = vec![source];
        let mut audit_barriers = HashMap::new();
        let mut deferred_audit_barrier_sources = HashSet::from([source_id.clone()]);
        let (message_tx, message_rx) = std::sync::mpsc::channel();

        finish_journal_barrier_audit(
            &message_tx,
            &sources,
            &mut audit_barriers,
            &mut deferred_audit_barrier_sources,
            source_id.clone(),
            4,
            Some(8),
            false,
        );
        finish_journal_barrier_audit(
            &message_tx,
            &sources,
            &mut audit_barriers,
            &mut deferred_audit_barrier_sources,
            source_id.clone(),
            4,
            None,
            true,
        );

        assert!(audit_barriers.is_empty());
        assert!(deferred_audit_barrier_sources.contains(&source_id));
        assert!(matches!(
            message_rx.try_recv(),
            Err(std::sync::mpsc::TryRecvError::Empty)
        ));
    }

    #[test]
    fn successful_completion_forwards_and_recaptures_barriers() {
        let directory = tempfile::tempdir().expect("source root");
        let source = SampleSource::new_with_id(
            SourceId::from_string("completed-barrier"),
            directory.path().to_path_buf(),
        );
        let source_id = source.id.as_str().to_string();
        let sources = vec![source];
        let mut audit_barriers = HashMap::new();
        audit_barriers.insert(
            source_id.clone(),
            journal::capture_audit_barrier(&sources, &source_id).expect("audit barrier"),
        );
        let mut deferred_audit_barrier_sources = HashSet::from([source_id.clone()]);
        let (message_tx, message_rx) = std::sync::mpsc::channel();

        finish_journal_barrier_audit(
            &message_tx,
            &sources,
            &mut audit_barriers,
            &mut deferred_audit_barrier_sources,
            source_id.clone(),
            5,
            Some(9),
            true,
        );

        let checkpoint = match message_rx.recv().expect("forwarded audit barrier") {
            GuiMessage::SourceWatcherCheckpointReady(checkpoint) => checkpoint,
            message => panic!("expected watcher checkpoint, got {message:?}"),
        };
        assert_eq!(checkpoint.source_id, source_id);
        assert_eq!(checkpoint.lifecycle_generation, 5);
        assert_eq!(checkpoint.source_revision, 9);
        assert!(matches!(
            message_rx.recv().expect("deferred audit gap"),
            GuiMessage::SourceWatcherJournalGap {
                source_id: gap_source_id,
                reason: "watcher_recovered_after_unavailable",
            } if gap_source_id == source_id
        ));
        assert!(audit_barriers.contains_key(&source_id));
        assert!(deferred_audit_barrier_sources.is_empty());
        assert!(matches!(
            message_rx.try_recv(),
            Err(std::sync::mpsc::TryRecvError::Empty)
        ));
    }

    fn blocking_initializer(
        backend: SourceWatcherBackend,
        release_rx: Receiver<()>,
    ) -> PendingSourceWatcher {
        let (result_tx, result_rx) = std::sync::mpsc::channel();
        let ingress_enabled = Arc::new(AtomicBool::new(true));
        let join_handle = thread::spawn(move || {
            release_rx.recv().expect("release blocking initializer");
            let _ = result_tx.send(Err("test initializer released".to_string()));
        });
        PendingSourceWatcher {
            result_rx,
            ingress_enabled,
            join_handle,
            completed_result: None,
            started_at: Instant::now(),
            backend,
        }
    }

    #[test]
    fn blocking_initializers_keep_one_owned_slot_per_backend_across_recovery_cycles() {
        let _guard = lock_lifecycle_tests();
        let (native_release_tx, native_release_rx) = std::sync::mpsc::channel();
        let (polling_release_tx, polling_release_rx) = std::sync::mpsc::channel();
        let mut pending = Some(blocking_initializer(
            SourceWatcherBackend::Native,
            native_release_rx,
        ));
        let mut retired = Vec::with_capacity(MAX_UNRESOLVED_INITIALIZERS);
        cancel_pending_source_watcher(&mut pending, &mut retired);

        assert!(has_initializer_for_backend(
            &pending,
            &retired,
            SourceWatcherBackend::Native
        ));
        assert!(!has_initializer_for_backend(
            &pending,
            &retired,
            SourceWatcherBackend::Polling
        ));

        pending = Some(blocking_initializer(
            SourceWatcherBackend::Polling,
            polling_release_rx,
        ));
        cancel_pending_source_watcher(&mut pending, &mut retired);

        assert_eq!(retired.len(), MAX_UNRESOLVED_INITIALIZERS);
        assert!(has_initializer_for_backend(
            &pending,
            &retired,
            SourceWatcherBackend::Native
        ));
        assert!(has_initializer_for_backend(
            &pending,
            &retired,
            SourceWatcherBackend::Polling
        ));
        assert_eq!(next_available_backend(&pending, &retired), None);

        let (event_tx, _event_rx) = std::sync::mpsc::sync_channel(1);
        let overflowed = Arc::new(AtomicBool::new(false));
        for _ in 0..8 {
            assert!(
                !start_pending_source_watcher(
                    &mut pending,
                    &retired,
                    Vec::new(),
                    event_tx.clone(),
                    Arc::clone(&overflowed),
                    SourceWatcherBackend::Native,
                ),
                "a blocked native initializer must keep its only slot across retries"
            );
            assert!(
                !start_pending_source_watcher(
                    &mut pending,
                    &retired,
                    Vec::new(),
                    event_tx.clone(),
                    Arc::clone(&overflowed),
                    SourceWatcherBackend::Polling,
                ),
                "a blocked polling initializer must keep its only slot across retries"
            );
        }

        native_release_tx
            .send(())
            .expect("release native initializer");
        polling_release_tx
            .send(())
            .expect("release polling initializer");
        let mut teardown = SourceWatcherTeardown {
            workers: Vec::new(),
        };
        let deadline = Instant::now() + Duration::from_secs(2);
        while !retired.is_empty() && Instant::now() < deadline {
            reap_retired_initializers(&mut retired, &mut teardown);
            thread::yield_now();
        }
        assert!(
            retired.is_empty(),
            "released initializer slots must be joined"
        );
    }

    #[test]
    fn polling_recovery_remains_available_while_native_initializer_is_unresolved() {
        let _guard = lock_lifecycle_tests();
        let (native_release_tx, native_release_rx) = std::sync::mpsc::channel();
        let mut pending = Some(blocking_initializer(
            SourceWatcherBackend::Native,
            native_release_rx,
        ));
        let mut retired = Vec::with_capacity(MAX_UNRESOLVED_INITIALIZERS);
        cancel_pending_source_watcher(&mut pending, &mut retired);

        assert_eq!(
            next_available_backend(&pending, &retired),
            Some(SourceWatcherBackend::Polling),
            "a failed polling watcher must restart through its free polling slot"
        );

        native_release_tx
            .send(())
            .expect("release native initializer");
        let mut teardown = SourceWatcherTeardown {
            workers: Vec::new(),
        };
        let deadline = Instant::now() + Duration::from_secs(2);
        while !retired.is_empty() && Instant::now() < deadline {
            reap_retired_initializers(&mut retired, &mut teardown);
            thread::yield_now();
        }
        assert!(retired.is_empty());
    }

    #[test]
    fn ingress_attaches_its_stream_id_to_all_captured_variants() {
        let (event_tx, event_rx) = std::sync::mpsc::sync_channel(3);
        let mut ingress = SourceWatcherIngress {
            event_tx,
            overflowed: Arc::new(AtomicBool::new(false)),
            enabled: Arc::new(AtomicBool::new(true)),
            stream_id: 41,
        };

        ingress.handle_event(Ok(Event {
            kind: EventKind::Any,
            paths: Vec::new(),
            attrs: notify::event::EventAttributes::default(),
        }));
        ingress.handle_event(Err(notify::Error::generic("capture-test")));
        ingress.handle_event(Ok(Event {
            kind: EventKind::Any,
            paths: (0..=MAX_CAPTURE_PATHS)
                .map(|index| PathBuf::from(format!("{index}.wav")))
                .collect(),
            attrs: notify::event::EventAttributes::default(),
        }));

        let notification = event_rx.recv().expect("captured notification");
        assert!(matches!(
            notification.capture,
            SourceWatcherCapture::Notify { stream_id: 41, .. }
        ));
        assert!(notification.boundary.captured_at() > 0);
        let error = event_rx.recv().expect("captured error");
        assert!(matches!(
            error.capture,
            SourceWatcherCapture::Error { stream_id: 41 }
        ));
        assert!(error.boundary.captured_at() > 0);
        let overflow = event_rx.recv().expect("captured overflow");
        assert!(matches!(
            overflow.capture,
            SourceWatcherCapture::Overflow { stream_id: 41 }
        ));
        assert!(overflow.boundary.captured_at() > 0);
    }

    struct ImmediateDropWatcher;

    impl Watcher for ImmediateDropWatcher {
        fn new<F: EventHandler>(_event_handler: F, _config: Config) -> notify::Result<Self>
        where
            Self: Sized,
        {
            unreachable!("test watcher is constructed directly")
        }

        fn watch(&mut self, _path: &Path, _recursive_mode: RecursiveMode) -> notify::Result<()> {
            Ok(())
        }

        fn unwatch(&mut self, _path: &Path) -> notify::Result<()> {
            Ok(())
        }

        fn kind() -> WatcherKind
        where
            Self: Sized,
        {
            WatcherKind::NullWatcher
        }
    }

    fn source_with_root(id: &str, root: &Path) -> SampleSource {
        SampleSource::new_with_id(SourceId::from_string(id), root.to_path_buf())
    }

    fn configured_admission(sources: &[SampleSource]) -> AdmissionLifecycle {
        configured_admission_with_limits(sources, None)
    }

    fn configured_admission_with_limits(
        sources: &[SampleSource],
        limits: Option<ReconciliationAdmissionLimits>,
    ) -> AdmissionLifecycle {
        let watched_roots = sources
            .iter()
            .enumerate()
            .map(|(index, source)| (source.root.clone(), Some(format!("identity-{index}"))))
            .collect::<WatchedRootIdentities>();
        let mut admission = limits
            .map(AdmissionLifecycle::with_limits)
            .unwrap_or_else(AdmissionLifecycle::new);
        admission
            .reconcile(sources, &watched_roots)
            .expect("configured source admission");
        admission
    }

    fn limited_admission(sources: &[SampleSource], max_in_flight: usize) -> AdmissionLifecycle {
        let per_lane = RawObservationLimits::new(8, usize::MAX, usize::MAX)
            .expect("native test per-lane limits");
        let global = RawObservationLimits::new(32, usize::MAX, usize::MAX)
            .expect("native test global limits");
        let limits = ReconciliationAdmissionLimits::new_with_per_lane_capacity(
            sources.len().max(1),
            per_lane,
            global,
            max_in_flight,
            1,
            8,
            64,
        )
        .expect("native test admission limits");
        configured_admission_with_limits(sources, Some(limits))
    }

    fn source_watcher_state(sources: Vec<SampleSource>) -> GuiSourceWatchState {
        let registrations = sources
            .iter()
            .cloned()
            .map(|source| SourceProcessingRegistration::new(source, 1))
            .collect();
        GuiSourceWatchState {
            registrations,
            sources,
            ..Default::default()
        }
    }

    #[test]
    fn replay_registration_revalidation_rejects_replaced_source_generation() {
        let first_root = tempfile::tempdir().expect("first source root");
        let replacement_root = tempfile::tempdir().expect("replacement source root");
        let first = source_with_root("replay-registration-race", first_root.path());
        let replacement = source_with_root("replay-registration-race", replacement_root.path());
        let mut state = source_watcher_state(vec![first.clone()]);

        assert!(replay_registration_matches_current(
            &state,
            &first.id,
            &first.root,
            1,
        ));
        state.set_registrations(vec![SourceProcessingRegistration::new(
            replacement.clone(),
            2,
        )]);
        assert!(!replay_registration_matches_current(
            &state,
            &first.id,
            &first.root,
            1,
        ));
        assert!(!replay_registration_matches_current(
            &state,
            &replacement.id,
            &replacement.root,
            1,
        ));
        assert!(replay_registration_matches_current(
            &state,
            &replacement.id,
            &replacement.root,
            2,
        ));
    }

    fn notify_capture(root: &Path, stream_id: u64, paths: &[&str]) -> SourceWatcherCapture {
        SourceWatcherCapture::Notify {
            stream_id,
            event: Event {
                kind: EventKind::Create(notify::event::CreateKind::File),
                paths: paths.iter().map(|path| root.join(path)).collect(),
                attrs: notify::event::EventAttributes::default(),
            },
        }
    }

    fn exact_boundary(sequence: u64) -> CaptureBoundary {
        CaptureBoundary::try_new(sequence, Some(sequence), Some(sequence))
            .expect("native test capture boundary")
    }

    fn live_test_watcher(stream_id: u64) -> ActiveSourceWatcher {
        ActiveSourceWatcher {
            _watcher: Box::new(ImmediateDropWatcher),
            ingress_enabled: Arc::new(AtomicBool::new(true)),
            stream_id,
        }
    }

    #[test]
    fn admitted_capture_retains_identity_and_correlation_until_proofless_handoff() {
        let directory = tempfile::tempdir().expect("source root");
        let source = source_with_root("identity-bound", directory.path());
        let mut state = source_watcher_state(vec![source.clone()]);
        let mut admission = configured_admission(&state.sources);
        state.set_audit_request_capacity(admission.max_source_audit_request_entries());
        let capture = notify_capture(directory.path(), 17, &["sample.wav"]);
        let boundary = exact_boundary(77);
        let mut targets = source_capture_targets(&capture, &state.sources);
        assert_eq!(targets.len(), 1);
        let target = targets.pop().expect("source target");
        let mut pending_contexts = HashMap::new();

        admit_capture_target(
            &mut state,
            &mut admission,
            &capture,
            boundary,
            target,
            &mut pending_contexts,
            Instant::now(),
        );

        let ticket = *pending_contexts
            .keys()
            .next()
            .expect("accepted capture ticket");
        let lane = admission
            .lane_for_capture(&source.id)
            .expect("identity-qualified lane");
        let context = pending_contexts
            .get(&ticket)
            .expect("pending capture context");
        assert_eq!(context.source_id, source.id);
        assert_eq!(context.source_root, source.root);
        assert_eq!(
            context.root_identity,
            RootIdentity::from_bytes(b"identity-0".to_vec())
        );
        assert_eq!(context.stream_id, 17);
        assert_eq!(context.watcher_generation, lane.generation());
        assert_eq!(context.capture_boundary, boundary);
        assert_eq!(
            context.correlation.as_ref().map(|value| value.ticket()),
            Some(ticket)
        );
        let expected_root_identity = context.root_identity.clone();
        let expected_generation = context.watcher_generation;
        assert_eq!(admission.in_flight(), 1);

        assert!(!dispatch_pending_capture_contexts(
            &mut state,
            &mut admission,
            &mut pending_contexts,
            Instant::now(),
        ));

        assert!(pending_contexts.is_empty());
        assert_eq!(admission.in_flight(), 0);
        let marker = admission
            .retained_uncertainties()
            .iter()
            .find(|marker| marker.source_id() == Some(&source.id))
            .expect("live uncertainty marker");
        assert_eq!(marker.root_identity(), Some(&expected_root_identity));
        assert_eq!(marker.generation(), Some(expected_generation));
        assert_eq!(marker.capture_boundary(), Some(boundary));
        assert_eq!(marker.scope(), ReconciliationScopeKind::SourceAudit);
        assert!(marker.reasons().contains(&UncertaintyReason::LiveUnproven));
        let request = state
            .pending_audit_requests
            .pop_front()
            .expect("proofless handoff audit request");
        assert_eq!(request.source_id(), &source.id);
        assert_eq!(request.root_identity(), &expected_root_identity);
        assert_eq!(request.generation(), expected_generation);
        assert_eq!(request.boundary(), marker.boundary());
        assert_eq!(admission.retained_uncertainties().len(), 1);
        assert_eq!(
            state
                .pending
                .get(source.id.as_str())
                .expect("compatibility refresh handoff")
                .paths,
            [Path::new("sample.wav").to_path_buf()]
                .into_iter()
                .collect()
        );
        assert!(
            !state
                .pending
                .get(source.id.as_str())
                .expect("compatibility refresh handoff")
                .overflowed
        );
    }

    #[test]
    fn audit_request_queue_coalesces_identity_and_latches_on_fallback_overflow() {
        let first_directory = tempfile::tempdir().expect("first source root");
        let second_directory = tempfile::tempdir().expect("second source root");
        let first = source_with_root("queue-first", first_directory.path());
        let second = source_with_root("queue-second", second_directory.path());
        let sources = vec![first.clone(), second.clone()];
        let mut state = source_watcher_state(sources.clone());
        let mut admission = limited_admission(&sources, 1);
        state.set_audit_request_capacity(1);
        let mut pending_contexts = HashMap::new();

        let capture = notify_capture(&first.root, 301, &["first.wav"]);
        let target = source_capture_targets(&capture, &state.sources)
            .pop()
            .expect("first source target");
        admit_capture_target(
            &mut state,
            &mut admission,
            &capture,
            exact_boundary(301),
            target,
            &mut pending_contexts,
            Instant::now(),
        );
        dispatch_pending_capture_contexts(
            &mut state,
            &mut admission,
            &mut pending_contexts,
            Instant::now(),
        );
        state.pending_audit_requests.clear();

        let (first_request, marker_backed) = admission
            .source_audit_request_for_current_lane(&first.id)
            .expect("first source audit request");
        assert!(marker_backed);
        state.enqueue_audit_request(first_request.clone(), true);
        assert_eq!(state.pending_audit_requests.len(), 1);
        assert_eq!(
            state
                .pending_audit_requests
                .front()
                .expect("first queued request")
                .identity(),
            first_request.identity()
        );

        let capture = notify_capture(&first.root, 301, &["second.wav"]);
        let target = source_capture_targets(&capture, &state.sources)
            .pop()
            .expect("second source target");
        admit_capture_target(
            &mut state,
            &mut admission,
            &capture,
            exact_boundary(302),
            target,
            &mut pending_contexts,
            Instant::now(),
        );
        let (covering_request, marker_backed) = admission
            .source_audit_request_for_current_lane(&first.id)
            .expect("covering source audit request");
        assert!(marker_backed);
        assert_eq!(covering_request.identity(), first_request.identity());
        assert!(covering_request.boundary().covers(first_request.boundary()));
        state.enqueue_audit_request(covering_request.clone(), true);
        assert_eq!(state.pending_audit_requests.len(), 1);
        let queued = state
            .pending_audit_requests
            .front()
            .expect("coalesced request");
        assert_eq!(queued.identity(), first_request.identity());
        assert_eq!(queued.boundary(), covering_request.boundary());

        let (fallback_request, marker_backed) = admission
            .source_audit_request_for_current_lane(&second.id)
            .expect("second source audit request");
        assert!(!marker_backed);
        assert_ne!(fallback_request.identity(), first_request.identity());
        state.enqueue_audit_request(fallback_request, false);
        assert!(state.pending_audit_requests.len() <= 1);
        assert!(state.pending_audit_requests.conservative_recovery_latched());
        assert_eq!(
            state
                .pending_audit_requests
                .front()
                .expect("marker-backed request retained")
                .identity(),
            first_request.identity()
        );
    }

    #[test]
    fn root_rebind_drops_old_typed_requests_and_widens_the_replacement_source() {
        let old_directory = tempfile::tempdir().expect("old source root");
        let new_directory = tempfile::tempdir().expect("replacement source root");
        let old_source = source_with_root("root-rebind-request", old_directory.path());
        let replacement = source_with_root("root-rebind-request", new_directory.path());
        let mut state = source_watcher_state(vec![old_source.clone()]);
        let mut admission = configured_admission(&state.sources);
        state.set_audit_request_capacity(admission.max_source_audit_request_entries());

        let old_request = admission
            .source_audit_request_for_current_lane(&old_source.id)
            .expect("old typed request")
            .0;
        state.enqueue_audit_request(old_request.clone(), true);

        state.set_sources(vec![replacement.clone()]);
        state.watched_roots = HashMap::from([(
            replacement.root.clone(),
            Some(String::from("replacement-identity")),
        )]);
        admission
            .reconcile(&state.sources, &state.watched_roots)
            .expect("replacement admission lane");

        let (message_tx, message_rx) = std::sync::mpsc::channel();
        purge_stale_audit_requests(&mut state, &admission, Instant::now());
        flush_pending_audit_requests(&mut state, &admission, &message_tx, Instant::now());

        assert!(matches!(
            message_rx.try_recv(),
            Err(std::sync::mpsc::TryRecvError::Empty)
        ));
        assert!(
            state
                .pending
                .get(replacement.id.as_str())
                .is_some_and(|pending| pending.overflowed)
        );
        assert!(
            !state
                .pending_audit_requests
                .front()
                .is_some_and(|request| request == &old_request)
        );
    }

    #[test]
    fn stop_restart_drops_old_typed_requests_then_emits_only_restarted_identity() {
        let directory = tempfile::tempdir().expect("source root");
        let source = source_with_root("stop-restart-request", directory.path());
        let mut state = source_watcher_state(vec![source.clone()]);
        let mut admission = configured_admission(&state.sources);
        state.watched_roots =
            HashMap::from([(source.root.clone(), Some(String::from("identity-0")))]);
        state.set_audit_request_capacity(admission.max_source_audit_request_entries());

        let old_request = admission
            .source_audit_request_for_current_lane(&source.id)
            .expect("old typed request")
            .0;
        state.enqueue_audit_request(old_request.clone(), true);
        admission.fence_all().expect("stop watcher lane");

        let (message_tx, message_rx) = std::sync::mpsc::channel();
        purge_stale_audit_requests(&mut state, &admission, Instant::now());
        flush_pending_audit_requests(&mut state, &admission, &message_tx, Instant::now());
        assert!(matches!(
            message_rx.try_recv(),
            Err(std::sync::mpsc::TryRecvError::Empty)
        ));
        assert!(
            state
                .pending
                .get(source.id.as_str())
                .is_some_and(|pending| pending.overflowed)
        );

        admission
            .reconcile(&state.sources, &state.watched_roots)
            .expect("restart watcher lane");
        let restarted_request = admission
            .source_audit_request_for_current_lane(&source.id)
            .expect("restarted typed request")
            .0;
        assert!(restarted_request.generation() > old_request.generation());
        state.enqueue_audit_request(restarted_request.clone(), true);
        flush_pending_audit_requests(&mut state, &admission, &message_tx, Instant::now());

        let emitted = match message_rx.recv().expect("restarted audit request") {
            GuiMessage::SourceWatcherManifestAuditRequested { request } => request,
            message => panic!("expected restarted audit request, got {message:?}"),
        };
        assert_eq!(emitted, restarted_request);
        assert_ne!(emitted, old_request);
    }

    #[test]
    fn failed_audit_handoff_retains_the_current_request_for_retry() {
        let directory = tempfile::tempdir().expect("source root");
        let source = source_with_root("failed-audit-handoff", directory.path());
        let mut state = source_watcher_state(vec![source.clone()]);
        let admission = configured_admission(&state.sources);
        state.set_audit_request_capacity(admission.max_source_audit_request_entries());
        let request = admission
            .source_audit_request_for_current_lane(&source.id)
            .expect("current typed request")
            .0;
        state.enqueue_audit_request(request.clone(), true);

        let (message_tx, message_rx) = std::sync::mpsc::channel();
        drop(message_rx);
        flush_pending_audit_requests(&mut state, &admission, &message_tx, Instant::now());

        assert_eq!(state.pending_audit_requests.front(), Some(&request));
    }

    #[test]
    fn exact_callback_boundary_duplicate_is_suppressed_without_new_handoff() {
        let directory = tempfile::tempdir().expect("source root");
        let source = source_with_root("duplicate", directory.path());
        let mut state = source_watcher_state(vec![source.clone()]);
        let mut admission = configured_admission(&state.sources);
        let boundary = exact_boundary(88);
        let mut pending_contexts = HashMap::new();

        for capture in [
            notify_capture(directory.path(), 21, &["duplicate.wav"]),
            notify_capture(directory.path(), 21, &["duplicate.wav"]),
        ] {
            let target = source_capture_targets(&capture, &state.sources)
                .pop()
                .expect("source target");
            admit_capture_target(
                &mut state,
                &mut admission,
                &capture,
                boundary,
                target,
                &mut pending_contexts,
                Instant::now(),
            );
            if admission.in_flight() != 0 {
                dispatch_pending_capture_contexts(
                    &mut state,
                    &mut admission,
                    &mut pending_contexts,
                    Instant::now(),
                );
            }
        }

        assert!(pending_contexts.is_empty());
        assert_eq!(admission.in_flight(), 0);
        assert_eq!(
            admission
                .retained_uncertainties()
                .iter()
                .filter(|marker| marker.source_id() == Some(&source.id))
                .count(),
            1,
            "duplicate suppression must not retain a second marker"
        );
    }

    #[test]
    fn stale_stream_is_fenced_before_native_admission() {
        let directory = tempfile::tempdir().expect("source root");
        let source = source_with_root("stale-stream", directory.path());
        let mut state = source_watcher_state(vec![source]);
        let mut admission = configured_admission(&state.sources);
        let watcher = live_test_watcher(41);
        let (event_tx, event_rx) = std::sync::mpsc::sync_channel(1);
        event_tx
            .send(CapturedSourceWatcherCapture::with_boundary(
                notify_capture(directory.path(), 40, &["stale.wav"]),
                exact_boundary(91),
            ))
            .expect("stale capture");
        let mut pending_contexts = HashMap::new();

        let (watcher_failed, root_invalidated) = drain_watcher_captures(
            &mut state,
            &mut admission,
            Some(&watcher),
            &event_rx,
            &mut pending_contexts,
            Instant::now(),
        );

        assert!(!watcher_failed);
        assert!(!root_invalidated);
        assert_eq!(admission.in_flight(), 0);
        assert!(pending_contexts.is_empty());
        assert!(
            state
                .pending
                .values()
                .next()
                .expect("stale stream widening")
                .overflowed
        );
    }

    #[test]
    fn queue_pressure_and_handoff_failure_widen_without_dropping_ticket_state() {
        let directory = tempfile::tempdir().expect("source root");
        let source = source_with_root("pressure", directory.path());
        let mut state = source_watcher_state(vec![source.clone()]);
        let mut admission = limited_admission(&state.sources, 1);
        let mut pending_contexts = HashMap::new();
        let now = Instant::now();

        for path in ["first.wav", "second.wav"] {
            let capture = notify_capture(directory.path(), 31, &[path]);
            let target = source_capture_targets(&capture, &state.sources)
                .pop()
                .expect("source target");
            admit_capture_target(
                &mut state,
                &mut admission,
                &capture,
                exact_boundary(if path == "first.wav" { 101 } else { 102 }),
                target,
                &mut pending_contexts,
                now,
            );
        }
        assert_eq!(admission.in_flight(), 1);
        assert_eq!(pending_contexts.len(), 1);
        assert!(
            state
                .pending
                .get(source.id.as_str())
                .is_some_and(|pending| pending.overflowed)
        );

        dispatch_pending_capture_contexts(&mut state, &mut admission, &mut pending_contexts, now);
        assert_eq!(admission.in_flight(), 0);
        assert!(pending_contexts.is_empty());

        let capture = notify_capture(directory.path(), 31, &["handoff.wav"]);
        let target = source_capture_targets(&capture, &state.sources)
            .pop()
            .expect("source target");
        admit_capture_target(
            &mut state,
            &mut admission,
            &capture,
            exact_boundary(103),
            target,
            &mut pending_contexts,
            now,
        );
        pending_contexts
            .values_mut()
            .next()
            .expect("handoff context")
            .correlation = None;
        dispatch_pending_capture_contexts(&mut state, &mut admission, &mut pending_contexts, now);
        assert_eq!(admission.in_flight(), 0);
        assert!(pending_contexts.is_empty());
        assert!(
            state
                .pending
                .get(source.id.as_str())
                .is_some_and(|pending| pending.overflowed)
        );
    }

    #[test]
    fn full_context_capacity_queues_current_lane_audit_without_extra_ticket_or_context() {
        let directory = tempfile::tempdir().expect("source root");
        let sources = (0..=MAX_PENDING_CAPTURE_CONTEXTS)
            .map(|index| {
                source_with_root(
                    &format!("full-context-{index}"),
                    &directory.path().join(format!("source-{index}")),
                )
            })
            .collect::<Vec<_>>();
        let overflow_source = sources.last().cloned().expect("overflow source");
        let mut state = source_watcher_state(sources.clone());
        let mut admission = configured_admission(&sources);
        state.set_audit_request_capacity(admission.max_source_audit_request_entries());
        let mut pending_contexts = HashMap::new();

        for (index, source) in sources
            .iter()
            .take(MAX_PENDING_CAPTURE_CONTEXTS)
            .enumerate()
        {
            let capture = notify_capture(&source.root, 101, &["sample.wav"]);
            let target = SourceCaptureTarget {
                source_id: source.id.clone(),
                source_root: source.root.clone(),
                paths: vec![source.root.join("sample.wav")],
                conservative: false,
            };
            admit_capture_target(
                &mut state,
                &mut admission,
                &capture,
                exact_boundary(index as u64 + 1),
                target,
                &mut pending_contexts,
                Instant::now(),
            );
        }

        assert_eq!(pending_contexts.len(), MAX_PENDING_CAPTURE_CONTEXTS);
        assert_eq!(admission.in_flight(), MAX_PENDING_CAPTURE_CONTEXTS);
        assert_eq!(
            admission.retained_uncertainties().len(),
            MAX_PENDING_CAPTURE_CONTEXTS
        );

        let lane = admission
            .lane_for_capture(&overflow_source.id)
            .expect("overflow source lane");
        state.pending_audit_requests.clear();
        let capture = notify_capture(&overflow_source.root, 101, &["sample.wav"]);
        let target = SourceCaptureTarget {
            source_id: overflow_source.id.clone(),
            source_root: overflow_source.root.clone(),
            paths: vec![overflow_source.root.join("sample.wav")],
            conservative: false,
        };
        admit_capture_target(
            &mut state,
            &mut admission,
            &capture,
            exact_boundary(258),
            target,
            &mut pending_contexts,
            Instant::now(),
        );

        assert_eq!(pending_contexts.len(), MAX_PENDING_CAPTURE_CONTEXTS);
        assert_eq!(admission.in_flight(), MAX_PENDING_CAPTURE_CONTEXTS);
        assert_eq!(
            admission.retained_uncertainties().len(),
            MAX_PENDING_CAPTURE_CONTEXTS,
            "full-context widening must not retain another marker"
        );
        let request = state
            .pending_audit_requests
            .front()
            .expect("full-context source audit request");
        assert_eq!(request.source_id(), &overflow_source.id);
        assert_eq!(request.root_identity(), lane.root_identity());
        assert_eq!(request.generation(), lane.generation());
        assert!(request.boundary().first() > 0);
        assert_eq!(request.boundary().first(), request.boundary().through());
        assert!(
            state
                .pending
                .get(overflow_source.id.as_str())
                .is_some_and(|pending| pending.overflowed)
        );
    }

    #[test]
    fn full_context_stopped_lane_widens_without_typed_request() {
        let directory = tempfile::tempdir().expect("source root");
        let source = source_with_root("full-context-stopped", directory.path());
        let mut state = source_watcher_state(vec![source.clone()]);
        let mut admission = limited_admission(&state.sources, 1);
        let capture = notify_capture(directory.path(), 102, &["first.wav"]);
        let target = SourceCaptureTarget {
            source_id: source.id.clone(),
            source_root: source.root.clone(),
            paths: vec![source.root.join("first.wav")],
            conservative: false,
        };
        let mut pending_contexts = HashMap::new();
        admit_capture_target(
            &mut state,
            &mut admission,
            &capture,
            exact_boundary(259),
            target,
            &mut pending_contexts,
            Instant::now(),
        );
        assert_eq!(pending_contexts.len(), 1);
        admission.fence_all().expect("stop capturing lane");
        state.pending_audit_requests.clear();

        let capture = notify_capture(directory.path(), 102, &["second.wav"]);
        let target = SourceCaptureTarget {
            source_id: source.id.clone(),
            source_root: source.root.clone(),
            paths: vec![source.root.join("second.wav")],
            conservative: false,
        };
        admit_capture_target(
            &mut state,
            &mut admission,
            &capture,
            exact_boundary(260),
            target,
            &mut pending_contexts,
            Instant::now(),
        );

        assert!(state.pending_audit_requests.is_empty());
        assert_eq!(pending_contexts.len(), 1);
        assert!(
            state
                .pending
                .get(source.id.as_str())
                .is_some_and(|pending| pending.overflowed)
        );
    }

    #[test]
    fn multi_source_and_ambiguous_paths_widen_all_configured_sources() {
        let first_directory = tempfile::tempdir().expect("first source root");
        let second_directory = tempfile::tempdir().expect("second source root");
        let first = source_with_root("multi-first", first_directory.path());
        let second = source_with_root("multi-second", second_directory.path());
        let sources = vec![first.clone(), second.clone()];
        let mut state = source_watcher_state(sources.clone());
        let mut admission = configured_admission(&sources);
        let capture = SourceWatcherCapture::Notify {
            stream_id: 51,
            event: Event {
                kind: EventKind::Create(notify::event::CreateKind::File),
                paths: vec![
                    first_directory.path().join("first.wav"),
                    second_directory.path().join("second.wav"),
                ],
                attrs: notify::event::EventAttributes::default(),
            },
        };
        let targets = source_capture_targets(&capture, &sources);
        assert_eq!(targets.len(), 2);
        assert!(targets.iter().all(|target| target.conservative));
        let (event_tx, event_rx) = std::sync::mpsc::sync_channel(1);
        event_tx
            .send(CapturedSourceWatcherCapture::with_boundary(
                capture,
                exact_boundary(111),
            ))
            .expect("multi-source capture");
        let watcher = live_test_watcher(51);
        let mut pending_contexts = HashMap::new();
        drain_watcher_captures(
            &mut state,
            &mut admission,
            Some(&watcher),
            &event_rx,
            &mut pending_contexts,
            Instant::now(),
        );
        assert!(sources.iter().all(|source| {
            state
                .pending
                .get(source.id.as_str())
                .is_some_and(|pending| pending.overflowed)
        }));
        assert!(pending_contexts.is_empty());

        let shared_directory = tempfile::tempdir().expect("ambiguous root");
        let ambiguous_sources = vec![
            source_with_root("ambiguous-first", shared_directory.path()),
            source_with_root("ambiguous-second", shared_directory.path()),
        ];
        let mut ambiguous_state = source_watcher_state(ambiguous_sources.clone());
        let mut ambiguous_admission = configured_admission(&ambiguous_sources);
        let ambiguous_capture = notify_capture(shared_directory.path(), 61, &["ambiguous.wav"]);
        let ambiguous_targets = source_capture_targets(&ambiguous_capture, &ambiguous_sources);
        assert_eq!(ambiguous_targets.len(), 2);
        assert!(ambiguous_targets.iter().all(|target| target.conservative));
        let (event_tx, event_rx) = std::sync::mpsc::sync_channel(1);
        event_tx
            .send(CapturedSourceWatcherCapture::with_boundary(
                ambiguous_capture,
                exact_boundary(112),
            ))
            .expect("ambiguous capture");
        let watcher = live_test_watcher(61);
        let mut pending_contexts = HashMap::new();
        drain_watcher_captures(
            &mut ambiguous_state,
            &mut ambiguous_admission,
            Some(&watcher),
            &event_rx,
            &mut pending_contexts,
            Instant::now(),
        );
        assert!(ambiguous_sources.iter().all(|source| {
            ambiguous_state
                .pending
                .get(source.id.as_str())
                .is_some_and(|pending| pending.overflowed)
        }));
    }

    #[test]
    fn lifecycle_fence_cleans_pending_contexts_and_widens_retired_work() {
        let directory = tempfile::tempdir().expect("source root");
        let source = source_with_root("fenced-context", directory.path());
        let mut state = source_watcher_state(vec![source.clone()]);
        let mut admission = configured_admission(&state.sources);
        let capture = notify_capture(directory.path(), 71, &["retired.wav"]);
        let target = source_capture_targets(&capture, &state.sources)
            .pop()
            .expect("source target");
        let mut pending_contexts = HashMap::new();
        admit_capture_target(
            &mut state,
            &mut admission,
            &capture,
            exact_boundary(121),
            target,
            &mut pending_contexts,
            Instant::now(),
        );
        assert_eq!(admission.in_flight(), 1);
        admission.fence_all().expect("fence captured work");
        assert_eq!(admission.in_flight(), 0);

        dispatch_pending_capture_contexts(
            &mut state,
            &mut admission,
            &mut pending_contexts,
            Instant::now(),
        );
        assert!(pending_contexts.is_empty());
        assert!(
            state
                .pending
                .get(source.id.as_str())
                .is_some_and(|pending| pending.overflowed)
        );
    }

    #[test]
    fn source_rebind_prunes_stale_replay_checkpoint_while_other_lane_remains_in_flight() {
        let first_directory = tempfile::tempdir().expect("first source root");
        let replacement_directory = tempfile::tempdir().expect("replacement source root");
        let second_directory = tempfile::tempdir().expect("second source root");
        let first = source_with_root("replay-prune-first", first_directory.path());
        let replacement = source_with_root("replay-prune-first", replacement_directory.path());
        let second = source_with_root("replay-prune-second", second_directory.path());
        let sources = vec![first.clone(), second.clone()];
        let mut state = source_watcher_state(sources.clone());
        let mut admission = configured_admission(&sources);
        let first_lane = admission
            .lane_for_capture(&first.id)
            .expect("first source lane");
        let mut pending_contexts = HashMap::new();

        let first_capture = notify_capture(&first.root, 401, &["first.wav"]);
        let first_target = source_capture_targets(&first_capture, &sources)
            .pop()
            .expect("first source target");
        admit_capture_target(
            &mut state,
            &mut admission,
            &first_capture,
            exact_boundary(401),
            first_target,
            &mut pending_contexts,
            Instant::now(),
        );
        let first_ticket = *pending_contexts.keys().next().expect("first source ticket");
        let dispatched = admission.dispatch_next().expect("first source dispatch");
        assert_eq!(dispatched.ticket(), first_ticket);
        admission
            .mark_dispatched(first_ticket)
            .expect("first source dispatched");
        admission
            .mark_applied(first_ticket)
            .expect("first source applied");
        pending_contexts
            .remove(&first_ticket)
            .expect("first source handoff context");

        let second_capture = notify_capture(&second.root, 402, &["second.wav"]);
        let second_target = source_capture_targets(&second_capture, &sources)
            .pop()
            .expect("second source target");
        admit_capture_target(
            &mut state,
            &mut admission,
            &second_capture,
            exact_boundary(402),
            second_target,
            &mut pending_contexts,
            Instant::now(),
        );
        assert_eq!(admission.in_flight(), 2);

        let mut pending_replay_checkpoints = HashMap::from([(
            first_ticket,
            PendingReplayCheckpoint {
                source_id: first.id.clone(),
                source_root: first.root.clone(),
                root_identity: first_lane.root_identity().clone(),
                owner_watcher_generation: first_lane.generation(),
                source_processing_lifecycle_generation: 1,
                proof: journal::WatcherContinuityProof {
                    root_identity: "identity-0".to_string(),
                    backend: journal::WatcherBackend::Fsevents,
                    backend_device: 1,
                    watcher_generation: 1,
                    replay_coverage_start_event_id: 1,
                    replay_coverage_end_event_id: 2,
                    acknowledged_end_event_id: 2,
                },
            },
        )]);

        admission
            .reconcile(
                &[replacement.clone(), second.clone()],
                &HashMap::from([
                    (
                        replacement.root.clone(),
                        Some("identity-rebound".to_string()),
                    ),
                    (second.root.clone(), Some("identity-1".to_string())),
                ]),
            )
            .expect("rebind first source while second remains active");
        assert_eq!(admission.in_flight(), 1);
        assert!(admission.lane_for_capture(&second.id).is_some());

        let stale_root_pending = pending_replay_checkpoints
            .get(&first_ticket)
            .cloned()
            .expect("stale root checkpoint context");
        fail_closed_replay_checkpoint(
            &mut state,
            &mut admission,
            &mut pending_replay_checkpoints,
            &replacement,
            first_ticket,
            &stale_root_pending,
            Instant::now(),
        );
        assert!(
            !pending_replay_checkpoints.contains_key(&first_ticket),
            "a stale root context is removed immediately"
        );
        assert_eq!(admission.in_flight(), 1);
        assert!(admission.lane_for_capture(&second.id).is_some());

        let rebound_root_identity = RootIdentity::from_bytes(b"identity-rebound".to_vec());
        admission
            .reconcile_source(&replacement, rebound_root_identity.clone())
            .expect("start rebound source lane");
        let rebound_lane = admission
            .lane_for_capture(&replacement.id)
            .expect("rebound source lane");
        let stopped_generation = rebound_lane.generation();
        assert!(
            admission
                .fence_source_if_current(
                    &replacement.id,
                    rebound_lane.root_identity(),
                    stopped_generation,
                )
                .expect("stop rebound source lane")
        );
        admission
            .reconcile_source(&replacement, rebound_root_identity.clone())
            .expect("restart rebound source lane");
        let restarted_lane = admission
            .lane_for_capture(&replacement.id)
            .expect("restarted rebound source lane");
        assert!(restarted_lane.generation() > stopped_generation);

        let stale_lane_pending = PendingReplayCheckpoint {
            source_id: replacement.id.clone(),
            source_root: replacement.root.clone(),
            root_identity: rebound_root_identity,
            owner_watcher_generation: stopped_generation,
            source_processing_lifecycle_generation: 1,
            proof: stale_root_pending.proof.clone(),
        };
        pending_replay_checkpoints.insert(first_ticket, stale_lane_pending.clone());
        fail_closed_replay_checkpoint(
            &mut state,
            &mut admission,
            &mut pending_replay_checkpoints,
            &replacement,
            first_ticket,
            &stale_lane_pending,
            Instant::now(),
        );
        assert!(
            !pending_replay_checkpoints.contains_key(&first_ticket),
            "a stale lane context is removed immediately"
        );
        assert_eq!(admission.in_flight(), 1);
        assert!(admission.lane_for_capture(&second.id).is_some());
        assert_eq!(
            admission
                .lane_for_capture(&replacement.id)
                .expect("current rebound lane")
                .generation(),
            restarted_lane.generation()
        );

        prune_stale_replay_checkpoints(&admission, &mut pending_replay_checkpoints);

        assert!(pending_replay_checkpoints.is_empty());
        assert_eq!(admission.in_flight(), 1);
        assert!(admission.lane_for_capture(&second.id).is_some());
        admission.fence_all().expect("clean up active test lanes");
    }

    #[test]
    fn replay_checkpoint_ack_uses_current_terminal_lifecycle_generation() {
        let directory = tempfile::tempdir().expect("source root");
        let source = source_with_root("replay-generation", directory.path());
        let source_id = source.id.clone();
        let root_identity = "identity-0";
        let replay_stream_generation = 42;
        let backend_device = 99;
        let replay_start_event_id = 17;
        let replay_end_event_id = 18;
        let checkpoint_value = |lifecycle_generation: u64, event_id: u64| {
            serde_json::json!({
                "root_identity": root_identity,
                "event_id": event_id,
                "format_version": 3,
                "source_id": source_id.as_str(),
                "lifecycle_generation": lifecycle_generation,
                "source_revision": 1,
                "cause": "targeted_replay",
                "continuity_proof": {
                    "root_identity": root_identity,
                    "backend": "fsevents",
                    "backend_device": backend_device,
                    "watcher_generation": replay_stream_generation,
                    "replay_coverage_start_event_id": event_id - 1,
                    "replay_coverage_end_event_id": event_id,
                    "acknowledged_end_event_id": event_id
                }
            })
            .to_string()
        };

        {
            let database =
                SourceDatabase::open_for_source_write(&source.root).expect("source database");
            database
                .set_metadata(
                    META_SOURCE_WATCHER_CHECKPOINT,
                    &checkpoint_value(1, replay_start_event_id),
                )
                .expect("historical replay checkpoint");
        }

        let mut state = source_watcher_state(vec![source.clone()]);
        state.set_registrations(vec![SourceProcessingRegistration::new(source.clone(), 2)]);
        let mut admission = configured_admission(&state.sources);
        let lane = admission
            .lane_for_capture(&source_id)
            .expect("replay source lane");
        let (observations, limits) =
            fsevents_replay_observations(vec![source.root.join("replayed.wav")])
                .expect("bounded replay observations");
        let replay_admission = {
            let database = SourceDatabase::open_for_background_job(&source.root)
                .expect("replay authority database");
            admission
                .admit_fsevents_replay(
                    &source,
                    &database,
                    replay_stream_generation,
                    FseventsReplayEvidence {
                        source_lifecycle_generation: WatcherGeneration::new(1),
                        replay_stream_generation,
                        backend_device,
                        replay_start_event_id,
                        replay_end_event_id,
                        observations,
                        limits,
                    },
                )
                .expect("replay admission")
        };
        assert_eq!(
            replay_admission.admission().disposition(),
            AdapterDisposition::AdmittedWithContinuity
        );
        let ticket = match replay_admission.admission().outcome() {
            AdmissionOutcome::Accepted(ticket) => *ticket,
            outcome => panic!("expected accepted replay, got {outcome:?}"),
        };
        let dispatched = admission.dispatch_next().expect("replay dispatch");
        assert_eq!(dispatched.ticket(), ticket);
        admission
            .mark_dispatched(ticket)
            .expect("replay dispatched");
        admission.mark_applied(ticket).expect("replay applied");

        let proof = journal::WatcherContinuityProof {
            root_identity: root_identity.to_string(),
            backend: journal::WatcherBackend::Fsevents,
            backend_device,
            watcher_generation: replay_stream_generation,
            replay_coverage_start_event_id: replay_start_event_id,
            replay_coverage_end_event_id: replay_end_event_id,
            acknowledged_end_event_id: replay_end_event_id,
        };
        let mut pending_replay_checkpoints = HashMap::from([(
            ticket,
            PendingReplayCheckpoint {
                source_id: source_id.clone(),
                source_root: source.root.clone(),
                root_identity: lane.root_identity().clone(),
                owner_watcher_generation: lane.generation(),
                source_processing_lifecycle_generation: 2,
                proof: proof.clone(),
            },
        )]);

        {
            let database = SourceDatabase::open_for_source_write(&source.root)
                .expect("terminal checkpoint database");
            database
                .set_metadata(
                    META_SOURCE_WATCHER_CHECKPOINT,
                    &checkpoint_value(2, replay_end_event_id),
                )
                .expect("current terminal replay checkpoint");
        }

        acknowledge_replay_checkpoint(
            &mut state,
            &mut admission,
            &mut pending_replay_checkpoints,
            std::slice::from_ref(&source),
            RevisionBoundCheckpoint {
                source_id: source_id.as_str().to_string(),
                lifecycle_generation: 2,
                source_revision: 1,
                root_identity: root_identity.to_string(),
                event_id: replay_end_event_id,
                cause: CheckpointCause::TargetedReplay,
                continuity_proof: Some(proof),
            },
            Instant::now(),
        );

        assert!(pending_replay_checkpoints.is_empty());
        assert_eq!(admission.in_flight(), 0);
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn closed_app_replay_fallback_is_source_scoped_for_shared_root() {
        let directory = tempfile::tempdir().expect("shared source root");
        let first = source_with_root("closed-replay-fallback-first", directory.path());
        let second = source_with_root("closed-replay-fallback-second", directory.path());
        let sources = vec![first.clone(), second.clone()];
        let mut state = source_watcher_state(sources.clone());
        state.set_registrations(vec![
            SourceProcessingRegistration::new(first.clone(), 1),
            SourceProcessingRegistration::new(second.clone(), 2),
        ]);
        state.watched_roots =
            HashMap::from([(first.root.clone(), Some(String::from("identity-1")))]);
        let mut admission = limited_admission(&sources, 1);
        state.set_audit_request_capacity(admission.max_source_audit_request_entries());

        let checkpoint_value = |source: &SampleSource,
                                lifecycle_generation: u64,
                                event_id: u64,
                                backend_device: u64,
                                watcher_generation: u64,
                                replay_start_event_id: u64,
                                replay_end_event_id: u64| {
            serde_json::json!({
                "root_identity": "identity-1",
                "event_id": event_id,
                "format_version": 3,
                "source_id": source.id.as_str(),
                "lifecycle_generation": lifecycle_generation,
                "source_revision": 1,
                "cause": "targeted_replay",
                "continuity_proof": {
                    "root_identity": "identity-1",
                    "backend": "fsevents",
                    "backend_device": backend_device,
                    "watcher_generation": watcher_generation,
                    "replay_coverage_start_event_id": replay_start_event_id,
                    "replay_coverage_end_event_id": replay_end_event_id,
                    "acknowledged_end_event_id": replay_end_event_id
                }
            })
            .to_string()
        };
        let write_checkpoint = |source: &SampleSource, value: String| {
            let database =
                SourceDatabase::open_for_source_write(&source.root).expect("source database");
            database
                .set_metadata(META_SOURCE_WATCHER_CHECKPOINT, &value)
                .expect("watcher checkpoint");
        };

        write_checkpoint(&second, checkpoint_value(&second, 1, 20, 202, 42, 19, 20));
        let second_lane = admission.lane_for_capture(&second.id).expect("second lane");
        let (second_observations, second_limits) =
            fsevents_replay_observations(vec![second.root.join("second.wav")])
                .expect("second replay observations");
        let second_replay = {
            let database = SourceDatabase::open_for_background_job(&second.root)
                .expect("second replay authority database");
            admission
                .admit_fsevents_replay(
                    &second,
                    &database,
                    42,
                    FseventsReplayEvidence {
                        source_lifecycle_generation: WatcherGeneration::new(1),
                        replay_stream_generation: 42,
                        backend_device: 202,
                        replay_start_event_id: 20,
                        replay_end_event_id: 21,
                        observations: second_observations,
                        limits: second_limits,
                    },
                )
                .expect("second replay admission")
        };
        let second_ticket = match second_replay.admission().outcome() {
            AdmissionOutcome::Accepted(ticket) => *ticket,
            outcome => panic!("unexpected second replay outcome: {outcome:?}"),
        };

        let (first_observations, first_limits) =
            fsevents_replay_observations(vec![first.root.join("first.wav")])
                .expect("first replay observations");
        let first_replay = {
            let database = SourceDatabase::open_for_background_job(&first.root)
                .expect("first replay authority database");
            admission
                .admit_fsevents_replay(
                    &first,
                    &database,
                    41,
                    FseventsReplayEvidence {
                        source_lifecycle_generation: WatcherGeneration::new(1),
                        replay_stream_generation: 41,
                        backend_device: 101,
                        replay_start_event_id: 10,
                        replay_end_event_id: 11,
                        observations: first_observations,
                        limits: first_limits,
                    },
                )
                .expect("first replay admission")
        };
        assert!(matches!(
            first_replay.admission().outcome(),
            AdmissionOutcome::Rejected(_)
        ));
        assert_eq!(admission.in_flight(), 1);

        let dispatched = admission.dispatch_next().expect("second replay dispatch");
        assert_eq!(dispatched.ticket(), second_ticket);
        admission
            .mark_dispatched(second_ticket)
            .expect("second replay dispatched");
        admission
            .mark_applied(second_ticket)
            .expect("second replay applied");

        let second_proof = WatcherContinuityProof {
            root_identity: String::from("identity-1"),
            backend: journal::WatcherBackend::Fsevents,
            backend_device: 202,
            watcher_generation: 42,
            replay_coverage_start_event_id: 20,
            replay_coverage_end_event_id: 21,
            acknowledged_end_event_id: 21,
        };
        let mut pending_replay_checkpoints = HashMap::from([(
            second_ticket,
            PendingReplayCheckpoint {
                source_id: second.id.clone(),
                source_root: second.root.clone(),
                root_identity: second_lane.root_identity().clone(),
                owner_watcher_generation: second_lane.generation(),
                source_processing_lifecycle_generation: 2,
                proof: second_proof.clone(),
            },
        )]);

        handle_closed_app_replay_fallback(
            &mut state,
            &first,
            first_replay.admission().outcome(),
            Instant::now(),
        );
        assert!(
            state
                .pending
                .get(first.id.as_str())
                .is_some_and(|pending| pending.overflowed)
        );
        assert!(state.pending.get(second.id.as_str()).is_none());
        assert_eq!(admission.in_flight(), 1);
        assert_eq!(
            admission.lane_for_capture(&second.id),
            Some(second_lane.clone())
        );
        assert!(pending_replay_checkpoints.contains_key(&second_ticket));

        let first_lane = admission.lane_for_capture(&first.id).expect("first lane");
        let capacity_envelope = RawObservationEnvelope::try_new(
            RawObservationProvenance::new(
                first.id.clone(),
                Some(first_lane.root_identity().clone()),
                BackendStreamIdentity::from_fsevents_device(101),
                first_lane.generation(),
                CaptureBoundary::try_new(12, None, None).expect("capacity boundary"),
            ),
            vec![RawObservation::new(
                RawEventKind::Create,
                vec![RawObservedPath::new(
                    PathBuf::from("first.wav"),
                    RawPathRole::Subject,
                )],
            )],
            RawObservationLimits::new(8, usize::MAX, usize::MAX).expect("capacity limits"),
        )
        .expect("capacity envelope");
        let capacity_outcome =
            AdmissionOutcome::UncertaintyCapacityExhausted(Box::new(capacity_envelope));
        handle_closed_app_replay_fallback(&mut state, &first, &capacity_outcome, Instant::now());
        assert!(
            state
                .pending
                .get(first.id.as_str())
                .is_some_and(|pending| pending.overflowed)
        );
        assert!(state.pending.get(second.id.as_str()).is_none());
        assert_eq!(admission.in_flight(), 1);
        assert!(pending_replay_checkpoints.contains_key(&second_ticket));

        write_checkpoint(&second, checkpoint_value(&second, 2, 21, 202, 42, 20, 21));
        acknowledge_replay_checkpoint(
            &mut state,
            &mut admission,
            &mut pending_replay_checkpoints,
            &sources,
            RevisionBoundCheckpoint {
                source_id: second.id.as_str().to_string(),
                lifecycle_generation: 2,
                source_revision: 1,
                root_identity: String::from("identity-1"),
                event_id: 21,
                cause: CheckpointCause::TargetedReplay,
                continuity_proof: Some(second_proof),
            },
            Instant::now(),
        );

        assert!(pending_replay_checkpoints.is_empty());
        assert_eq!(admission.in_flight(), 0);
        assert_eq!(admission.lane_for_capture(&second.id), Some(second_lane));
        assert!(
            state
                .pending
                .get(first.id.as_str())
                .is_some_and(|pending| pending.overflowed)
        );
        assert!(state.pending.get(second.id.as_str()).is_none());
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn accepted_unproven_replay_uses_source_local_audit_handoff() {
        let directory = tempfile::tempdir().expect("shared replay root");
        let first = source_with_root("unproven-replay-first", directory.path());
        let second = source_with_root("unproven-replay-second", directory.path());
        let sources = vec![first.clone(), second.clone()];
        let mut state = source_watcher_state(sources.clone());
        state.set_registrations(vec![
            SourceProcessingRegistration::new(first.clone(), 1),
            SourceProcessingRegistration::new(second.clone(), 2),
        ]);
        state.watched_roots =
            HashMap::from([(first.root.clone(), Some(String::from("identity-1")))]);
        let mut admission = configured_admission(&sources);
        state.set_audit_request_capacity(admission.max_source_audit_request_entries());

        let checkpoint_value = |source: &SampleSource,
                                lifecycle_generation: u64,
                                event_id: u64,
                                backend_device: u64,
                                watcher_generation: u64,
                                replay_start_event_id: u64,
                                replay_end_event_id: u64| {
            serde_json::json!({
                "root_identity": "identity-1",
                "event_id": event_id,
                "format_version": 3,
                "source_id": source.id.as_str(),
                "lifecycle_generation": lifecycle_generation,
                "source_revision": 1,
                "cause": "targeted_replay",
                "continuity_proof": {
                    "root_identity": "identity-1",
                    "backend": "fsevents",
                    "backend_device": backend_device,
                    "watcher_generation": watcher_generation,
                    "replay_coverage_start_event_id": replay_start_event_id,
                    "replay_coverage_end_event_id": replay_end_event_id,
                    "acknowledged_end_event_id": replay_end_event_id
                }
            })
            .to_string()
        };
        let write_checkpoint = |source: &SampleSource, value: String| {
            let database =
                SourceDatabase::open_for_source_write(&source.root).expect("source database");
            database
                .set_metadata(META_SOURCE_WATCHER_CHECKPOINT, &value)
                .expect("watcher checkpoint");
        };

        write_checkpoint(&second, checkpoint_value(&second, 1, 20, 202, 42, 19, 20));
        let first_lane = admission.lane_for_capture(&first.id).expect("first lane");
        let second_lane = admission.lane_for_capture(&second.id).expect("second lane");
        let first_boundary =
            CaptureBoundary::try_new(11, Some(11), Some(11)).expect("first replay boundary");
        let second_boundary =
            CaptureBoundary::try_new(21, Some(21), Some(21)).expect("second replay boundary");
        let first_proof = WatcherContinuityProof {
            root_identity: String::from("identity-1"),
            backend: journal::WatcherBackend::Fsevents,
            backend_device: 101,
            watcher_generation: 41,
            replay_coverage_start_event_id: 10,
            replay_coverage_end_event_id: 11,
            acknowledged_end_event_id: 11,
        };
        let second_proof = WatcherContinuityProof {
            root_identity: String::from("identity-1"),
            backend: journal::WatcherBackend::Fsevents,
            backend_device: 202,
            watcher_generation: 42,
            replay_coverage_start_event_id: 20,
            replay_coverage_end_event_id: 21,
            acknowledged_end_event_id: 21,
        };

        let (first_observations, first_limits) =
            fsevents_replay_observations(vec![PathBuf::from("first.wav")])
                .expect("first replay observations");
        let first_replay = {
            let database = SourceDatabase::open_for_background_job(&first.root)
                .expect("first replay authority database");
            admission
                .admit_fsevents_replay(
                    &first,
                    &database,
                    41,
                    FseventsReplayEvidence {
                        source_lifecycle_generation: WatcherGeneration::new(1),
                        replay_stream_generation: 41,
                        backend_device: 101,
                        replay_start_event_id: 10,
                        replay_end_event_id: 11,
                        observations: first_observations,
                        limits: first_limits,
                    },
                )
                .expect("first replay admission")
        };
        assert_eq!(
            first_replay.admission().disposition(),
            AdapterDisposition::SourceAuditRequired
        );
        assert!(matches!(
            first_replay.admission().outcome(),
            AdmissionOutcome::Accepted(_)
        ));
        let first_ticket = match first_replay.admission().outcome() {
            AdmissionOutcome::Accepted(ticket) => *ticket,
            outcome => panic!("unexpected first replay outcome: {outcome:?}"),
        };
        let first_request = first_replay
            .audit_request()
            .cloned()
            .expect("first source typed audit request");
        assert_eq!(first_request.source_id(), &first.id);
        assert_eq!(first_request.root_identity(), first_lane.root_identity());
        assert_eq!(first_request.generation(), first_lane.generation());
        let first_marker_backed = admission.source_audit_request_is_marker_backed(&first_request);
        assert!(first_marker_backed);
        state.enqueue_audit_request(first_request.clone(), first_marker_backed);

        let (second_observations, second_limits) =
            fsevents_replay_observations(vec![PathBuf::from("second.wav")])
                .expect("second replay observations");
        let second_replay = {
            let database = SourceDatabase::open_for_background_job(&second.root)
                .expect("second replay authority database");
            admission
                .admit_fsevents_replay(
                    &second,
                    &database,
                    42,
                    FseventsReplayEvidence {
                        source_lifecycle_generation: WatcherGeneration::new(1),
                        replay_stream_generation: 42,
                        backend_device: 202,
                        replay_start_event_id: 20,
                        replay_end_event_id: 21,
                        observations: second_observations,
                        limits: second_limits,
                    },
                )
                .expect("second replay admission")
        };
        assert_eq!(
            second_replay.admission().disposition(),
            AdapterDisposition::AdmittedWithContinuity
        );
        let second_ticket = match second_replay.admission().outcome() {
            AdmissionOutcome::Accepted(ticket) => *ticket,
            outcome => panic!("unexpected second replay outcome: {outcome:?}"),
        };

        let mut pending_contexts = HashMap::from([
            (
                first_ticket,
                PendingCaptureContext {
                    source_id: first.id.clone(),
                    source_root: first.root.clone(),
                    root_identity: first_lane.root_identity().clone(),
                    stream_id: 0,
                    watcher_generation: first_lane.generation(),
                    capture_boundary: first_boundary,
                    original_event: None,
                    compatibility_event: None,
                    conservative: false,
                    correlation: None,
                    replay: Some(PendingReplayHandoff {
                        paths: vec![PathBuf::from("first.wav")],
                        proof: first_proof,
                        continuity_proven: false,
                        source_processing_lifecycle_generation: 1,
                    }),
                },
            ),
            (
                second_ticket,
                PendingCaptureContext {
                    source_id: second.id.clone(),
                    source_root: second.root.clone(),
                    root_identity: second_lane.root_identity().clone(),
                    stream_id: 0,
                    watcher_generation: second_lane.generation(),
                    capture_boundary: second_boundary,
                    original_event: None,
                    compatibility_event: None,
                    conservative: false,
                    correlation: None,
                    replay: Some(PendingReplayHandoff {
                        paths: vec![PathBuf::from("second.wav")],
                        proof: second_proof.clone(),
                        continuity_proven: true,
                        source_processing_lifecycle_generation: 2,
                    }),
                },
            ),
        ]);
        let mut pending_replay_checkpoints = HashMap::new();
        let (message_tx, message_rx) = std::sync::mpsc::channel();
        let root_invalidated = dispatch_pending_capture_contexts_with_replay(
            &mut state,
            &mut admission,
            &mut pending_contexts,
            &mut pending_replay_checkpoints,
            &message_tx,
            Instant::now(),
        );

        assert!(!root_invalidated);
        assert!(pending_contexts.is_empty());
        assert_eq!(admission.in_flight(), 1);
        assert!(!pending_replay_checkpoints.contains_key(&first_ticket));
        let second_checkpoint = pending_replay_checkpoints
            .get(&second_ticket)
            .expect("continuity-proven replay checkpoint");
        assert_eq!(second_checkpoint.proof, second_proof);
        assert_eq!(state.pending_audit_requests.front(), Some(&first_request));
        assert!(
            state
                .pending
                .get(first.id.as_str())
                .is_some_and(|pending| pending.overflowed && pending.paths.is_empty())
        );
        assert!(
            state.pending.get(second.id.as_str()).is_none(),
            "unexpected source pending state: {:?}",
            state.pending
        );
        assert!(admission.retained_uncertainties().iter().any(|marker| {
            marker.source_id() == Some(&first.id)
                && marker.root_identity() == Some(first_lane.root_identity())
                && marker.generation() == Some(first_lane.generation())
        }));

        match message_rx
            .recv_timeout(Duration::from_secs(1))
            .expect("continuity-proven replay message")
        {
            GuiMessage::SourceFilesystemChanged {
                source_id,
                scopes,
                paths,
                overflowed,
                source_root_available,
                source_root_identity,
                lifecycle_generation,
                journal_checkpoint_event_id,
                watcher_continuity_proof,
            } => {
                assert_eq!(source_id, second.id.as_str());
                assert!(scopes.is_some());
                assert_eq!(paths, vec![PathBuf::from("second.wav")]);
                assert!(!overflowed);
                assert!(source_root_available);
                assert_eq!(
                    source_root_identity,
                    Some(second_proof.root_identity.clone())
                );
                assert_eq!(lifecycle_generation, Some(2));
                assert_eq!(journal_checkpoint_event_id, Some(21));
                assert_eq!(watcher_continuity_proof, Some(second_proof.clone()));
            }
            message => panic!("unexpected replay handoff message: {message:?}"),
        }
        assert!(matches!(
            message_rx.try_recv(),
            Err(std::sync::mpsc::TryRecvError::Empty)
        ));

        write_checkpoint(&second, checkpoint_value(&second, 2, 21, 202, 42, 20, 21));
        acknowledge_replay_checkpoint(
            &mut state,
            &mut admission,
            &mut pending_replay_checkpoints,
            &sources,
            RevisionBoundCheckpoint {
                source_id: second.id.as_str().to_string(),
                lifecycle_generation: 2,
                source_revision: 1,
                root_identity: String::from("identity-1"),
                event_id: 21,
                cause: CheckpointCause::TargetedReplay,
                continuity_proof: Some(second_proof),
            },
            Instant::now(),
        );

        assert!(pending_replay_checkpoints.is_empty());
        assert_eq!(admission.in_flight(), 0);
        assert_eq!(admission.lane_for_capture(&second.id), Some(second_lane));
        assert_eq!(state.pending_audit_requests.front(), Some(&first_request));
    }

    #[test]
    fn replay_checkpoint_failure_is_source_scoped_and_other_source_retires() {
        let first_directory = tempfile::tempdir().expect("first source root");
        let second_directory = tempfile::tempdir().expect("second source root");
        let first = source_with_root("replay-failure-first", first_directory.path());
        let second = source_with_root("replay-failure-second", second_directory.path());
        let sources = vec![first.clone(), second.clone()];
        let mut state = source_watcher_state(sources.clone());
        state.set_registrations(vec![
            SourceProcessingRegistration::new(first.clone(), 2),
            SourceProcessingRegistration::new(second.clone(), 2),
        ]);
        state.watched_roots = HashMap::from([
            (first.root.clone(), Some(String::from("identity-0"))),
            (second.root.clone(), Some(String::from("identity-1"))),
        ]);
        let mut admission = configured_admission(&sources);
        state.set_audit_request_capacity(admission.max_source_audit_request_entries());

        let checkpoint_value = |source: &SampleSource,
                                root_identity: &str,
                                backend_device: u64,
                                watcher_generation: u64,
                                lifecycle_generation: u64,
                                event_id: u64| {
            serde_json::json!({
                "root_identity": root_identity,
                "event_id": event_id,
                "format_version": 3,
                "source_id": source.id.as_str(),
                "lifecycle_generation": lifecycle_generation,
                "source_revision": 1,
                "cause": "targeted_replay",
                "continuity_proof": {
                    "root_identity": root_identity,
                    "backend": "fsevents",
                    "backend_device": backend_device,
                    "watcher_generation": watcher_generation,
                    "replay_coverage_start_event_id": event_id - 1,
                    "replay_coverage_end_event_id": event_id,
                    "acknowledged_end_event_id": event_id
                }
            })
            .to_string()
        };
        let write_checkpoint = |source: &SampleSource, value: String| {
            let database =
                SourceDatabase::open_for_source_write(&source.root).expect("source database");
            database
                .set_metadata(META_SOURCE_WATCHER_CHECKPOINT, &value)
                .expect("watcher checkpoint");
        };

        write_checkpoint(
            &first,
            checkpoint_value(&first, "identity-0", 101, 41, 1, 10),
        );
        write_checkpoint(
            &second,
            checkpoint_value(&second, "identity-1", 202, 42, 1, 20),
        );

        let first_lane = admission.lane_for_capture(&first.id).expect("first lane");
        let second_lane = admission.lane_for_capture(&second.id).expect("second lane");
        let (first_observations, first_limits) =
            fsevents_replay_observations(vec![first.root.join("first.wav")])
                .expect("first replay observations");
        let first_ticket = {
            let database = SourceDatabase::open_for_background_job(&first.root)
                .expect("first replay authority database");
            let replay = admission
                .admit_fsevents_replay(
                    &first,
                    &database,
                    41,
                    FseventsReplayEvidence {
                        source_lifecycle_generation: WatcherGeneration::new(1),
                        replay_stream_generation: 41,
                        backend_device: 101,
                        replay_start_event_id: 10,
                        replay_end_event_id: 11,
                        observations: first_observations,
                        limits: first_limits,
                    },
                )
                .expect("first replay admission");
            match replay.admission().outcome() {
                AdmissionOutcome::Accepted(ticket) => *ticket,
                outcome => panic!("unexpected first replay outcome: {outcome:?}"),
            }
        };
        let (second_observations, second_limits) =
            fsevents_replay_observations(vec![second.root.join("second.wav")])
                .expect("second replay observations");
        let second_ticket = {
            let database = SourceDatabase::open_for_background_job(&second.root)
                .expect("second replay authority database");
            let replay = admission
                .admit_fsevents_replay(
                    &second,
                    &database,
                    42,
                    FseventsReplayEvidence {
                        source_lifecycle_generation: WatcherGeneration::new(1),
                        replay_stream_generation: 42,
                        backend_device: 202,
                        replay_start_event_id: 20,
                        replay_end_event_id: 21,
                        observations: second_observations,
                        limits: second_limits,
                    },
                )
                .expect("second replay admission");
            match replay.admission().outcome() {
                AdmissionOutcome::Accepted(ticket) => *ticket,
                outcome => panic!("unexpected second replay outcome: {outcome:?}"),
            }
        };

        for _ in 0..2 {
            let dispatched = admission.dispatch_next().expect("replay dispatch");
            assert!(dispatched.ticket() == first_ticket || dispatched.ticket() == second_ticket);
            admission
                .mark_dispatched(dispatched.ticket())
                .expect("replay dispatched");
            admission
                .mark_applied(dispatched.ticket())
                .expect("replay applied");
        }
        assert_eq!(admission.in_flight(), 2);

        let first_proof = WatcherContinuityProof {
            root_identity: String::from("identity-0"),
            backend: journal::WatcherBackend::Fsevents,
            backend_device: 101,
            watcher_generation: 41,
            replay_coverage_start_event_id: 10,
            replay_coverage_end_event_id: 11,
            acknowledged_end_event_id: 11,
        };
        let second_proof = WatcherContinuityProof {
            root_identity: String::from("identity-1"),
            backend: journal::WatcherBackend::Fsevents,
            backend_device: 202,
            watcher_generation: 42,
            replay_coverage_start_event_id: 20,
            replay_coverage_end_event_id: 21,
            acknowledged_end_event_id: 21,
        };
        let mut pending_replay_checkpoints = HashMap::from([
            (
                first_ticket,
                PendingReplayCheckpoint {
                    source_id: first.id.clone(),
                    source_root: first.root.clone(),
                    root_identity: first_lane.root_identity().clone(),
                    owner_watcher_generation: first_lane.generation(),
                    source_processing_lifecycle_generation: 2,
                    proof: first_proof.clone(),
                },
            ),
            (
                second_ticket,
                PendingReplayCheckpoint {
                    source_id: second.id.clone(),
                    source_root: second.root.clone(),
                    root_identity: second_lane.root_identity().clone(),
                    owner_watcher_generation: second_lane.generation(),
                    source_processing_lifecycle_generation: 2,
                    proof: second_proof.clone(),
                },
            ),
        ]);

        // First source has only its pre-terminal authority, while the second source has reached
        // the replay terminal event. The failure must not retire the second applied ticket.
        write_checkpoint(
            &first,
            checkpoint_value(&first, "identity-0", 101, 41, 2, 10),
        );
        write_checkpoint(
            &second,
            checkpoint_value(&second, "identity-1", 202, 42, 2, 21),
        );

        acknowledge_replay_checkpoint(
            &mut state,
            &mut admission,
            &mut pending_replay_checkpoints,
            &sources,
            RevisionBoundCheckpoint {
                source_id: first.id.as_str().to_string(),
                lifecycle_generation: 2,
                source_revision: 1,
                root_identity: String::from("identity-0"),
                event_id: 11,
                cause: CheckpointCause::TargetedReplay,
                continuity_proof: Some(first_proof.clone()),
            },
            Instant::now(),
        );

        let restarted_first = admission
            .lane_for_capture(&first.id)
            .expect("restarted first lane");
        assert_eq!(restarted_first.root_identity(), first_lane.root_identity());
        assert!(restarted_first.generation() > first_lane.generation());
        assert_eq!(
            restarted_first.lifecycle(),
            ReconciliationLifecycle::Capturing
        );
        assert_eq!(
            admission.lane_for_capture(&second.id),
            Some(second_lane.clone())
        );
        assert_eq!(admission.in_flight(), 1);
        assert!(!pending_replay_checkpoints.contains_key(&first_ticket));
        assert!(pending_replay_checkpoints.contains_key(&second_ticket));
        assert!(
            state
                .pending
                .get(first.id.as_str())
                .is_some_and(|pending| pending.overflowed)
        );
        assert!(state.pending.get(second.id.as_str()).is_none());
        let first_request = state
            .pending_audit_requests
            .front()
            .expect("first source typed audit request");
        assert_eq!(first_request.source_id(), &first.id);
        assert_eq!(first_request.generation(), restarted_first.generation());

        let stopped_first_pending = PendingReplayCheckpoint {
            source_id: first.id.clone(),
            source_root: first.root.clone(),
            root_identity: restarted_first.root_identity().clone(),
            owner_watcher_generation: restarted_first.generation(),
            source_processing_lifecycle_generation: 2,
            proof: first_proof.clone(),
        };
        pending_replay_checkpoints.insert(first_ticket, stopped_first_pending.clone());
        state.watched_roots.remove(&first.root);
        fail_closed_replay_checkpoint(
            &mut state,
            &mut admission,
            &mut pending_replay_checkpoints,
            &first,
            first_ticket,
            &stopped_first_pending,
            Instant::now(),
        );

        assert!(!pending_replay_checkpoints.contains_key(&first_ticket));
        let stopped_first = admission
            .lane_for_capture(&first.id)
            .expect("first lane remains owned while root is unavailable");
        assert_eq!(stopped_first.lifecycle(), ReconciliationLifecycle::Stopped);
        assert_eq!(admission.in_flight(), 1);
        assert_eq!(
            admission.lane_for_capture(&second.id),
            Some(second_lane.clone())
        );
        let second_pending = pending_replay_checkpoints
            .get(&second_ticket)
            .expect("second checkpoint context remains pending");
        assert_eq!(
            second_pending.owner_watcher_generation,
            second_lane.generation()
        );
        assert_eq!(second_pending.root_identity, *second_lane.root_identity());

        state
            .watched_roots
            .insert(first.root.clone(), Some(String::from("identity-0")));
        admission
            .reconcile_source(&first, RootIdentity::from_bytes(b"identity-0".to_vec()))
            .expect("restart first lane after root installation");
        let installed_first = admission
            .lane_for_capture(&first.id)
            .expect("reinstalled first lane");
        assert!(installed_first.generation() > stopped_first.generation());
        assert_eq!(
            installed_first.lifecycle(),
            ReconciliationLifecycle::Capturing
        );
        let (request, _) = admission
            .source_audit_request_for_current_lane(&first.id)
            .expect("typed audit request after root installation");
        assert_eq!(request.source_id(), &first.id);
        assert_eq!(request.generation(), installed_first.generation());

        acknowledge_replay_checkpoint(
            &mut state,
            &mut admission,
            &mut pending_replay_checkpoints,
            &sources,
            RevisionBoundCheckpoint {
                source_id: second.id.as_str().to_string(),
                lifecycle_generation: 2,
                source_revision: 1,
                root_identity: String::from("identity-1"),
                event_id: 21,
                cause: CheckpointCause::TargetedReplay,
                continuity_proof: Some(second_proof),
            },
            Instant::now(),
        );

        assert!(pending_replay_checkpoints.is_empty());
        assert_eq!(admission.in_flight(), 0);
        assert_eq!(admission.lane_for_capture(&second.id), Some(second_lane));
        assert!(admission.lane_for_capture(&first.id).is_some());
    }

    #[test]
    fn missing_identity_and_invalid_root_mapping_widen_conservatively() {
        let directory = tempfile::tempdir().expect("source root");
        let source = source_with_root("missing-identity", directory.path());
        let mut state = source_watcher_state(vec![source.clone()]);
        let mut admission = AdmissionLifecycle::new();
        let capture = notify_capture(directory.path(), 81, &["missing.wav"]);
        let target = source_capture_targets(&capture, &state.sources)
            .pop()
            .expect("source target");
        let mut pending_contexts = HashMap::new();
        admit_capture_target(
            &mut state,
            &mut admission,
            &capture,
            exact_boundary(131),
            target,
            &mut pending_contexts,
            Instant::now(),
        );
        assert!(
            state
                .pending
                .get(source.id.as_str())
                .is_some_and(|pending| pending.overflowed)
        );

        let mut configured = configured_admission(&state.sources);
        let invalid_target = SourceCaptureTarget {
            source_id: source.id.clone(),
            source_root: source.root.clone(),
            paths: vec![source.root.join("../escape.wav")],
            conservative: false,
        };
        let mut invalid_state = source_watcher_state(vec![source.clone()]);
        admit_capture_target(
            &mut invalid_state,
            &mut configured,
            &notify_capture(directory.path(), 82, &["ignored.wav"]),
            exact_boundary(132),
            invalid_target,
            &mut pending_contexts,
            Instant::now(),
        );
        assert!(
            invalid_state
                .pending
                .get(source.id.as_str())
                .is_some_and(|pending| pending.overflowed)
        );
    }

    struct BlockingDropWatcher {
        release_rx: Receiver<()>,
    }

    impl Drop for BlockingDropWatcher {
        fn drop(&mut self) {
            self.release_rx
                .recv()
                .expect("release blocking watcher drop");
        }
    }

    impl Watcher for BlockingDropWatcher {
        fn new<F: EventHandler>(_event_handler: F, _config: Config) -> notify::Result<Self>
        where
            Self: Sized,
        {
            unreachable!("test watcher is constructed directly")
        }

        fn watch(&mut self, _path: &Path, _recursive_mode: RecursiveMode) -> notify::Result<()> {
            Ok(())
        }

        fn unwatch(&mut self, _path: &Path) -> notify::Result<()> {
            Ok(())
        }

        fn kind() -> WatcherKind
        where
            Self: Sized,
        {
            WatcherKind::NullWatcher
        }
    }

    fn blocking_watcher(release_rx: Receiver<()>) -> ActiveSourceWatcher {
        ActiveSourceWatcher {
            _watcher: Box::new(BlockingDropWatcher { release_rx }),
            ingress_enabled: Arc::new(AtomicBool::new(true)),
            stream_id: next_stream_id(),
        }
    }

    #[test]
    fn fenced_retained_watcher_capture_widens_without_requesting_teardown() {
        let _guard = lock_lifecycle_tests();
        let mut teardown = SourceWatcherTeardown {
            workers: Vec::new(),
        };
        let mut teardown_releases = Vec::new();
        for _ in 0..MAX_UNRESOLVED_TEARDOWNS {
            let (release_tx, release_rx) = std::sync::mpsc::sync_channel(1);
            if teardown.retire(blocking_watcher(release_rx)).is_err() {
                panic!("teardown slot should be available");
            }
            teardown_releases.push(release_tx);
        }

        let (retained_release_tx, retained_release_rx) = std::sync::mpsc::sync_channel(1);
        let retained = blocking_watcher(retained_release_rx);
        let stream_id = retained.stream_id;
        let mut watcher = Some(retained);
        retire_source_watcher(&mut watcher, &mut teardown);

        let fenced = watcher
            .as_ref()
            .expect("saturated teardown retains watcher");
        assert!(!fenced.ingress_enabled.load(Ordering::Acquire));
        assert_eq!(teardown.unresolved_count(), MAX_UNRESOLVED_TEARDOWNS);

        let source = SampleSource::new_with_id(
            SourceId::from_string("fenced-retained-capture"),
            PathBuf::from("/tmp/fenced-retained-capture"),
        );
        let source_id = source.id.as_str().to_string();
        let mut state = GuiSourceWatchState {
            sources: vec![source],
            ..Default::default()
        };
        let (event_tx, event_rx) = std::sync::mpsc::sync_channel(1);
        event_tx
            .send(CapturedSourceWatcherCapture::from_capture(
                SourceWatcherCapture::Error { stream_id },
            ))
            .expect("inject same-stream fenced capture");
        let mut admission = AdmissionLifecycle::new();
        let mut pending_contexts = HashMap::new();

        let (watcher_failed, root_invalidated) = drain_watcher_captures(
            &mut state,
            &mut admission,
            watcher.as_ref(),
            &event_rx,
            &mut pending_contexts,
            Instant::now(),
        );

        assert!(
            !watcher_failed,
            "a fenced stream must not request current watcher teardown"
        );
        assert!(!root_invalidated);
        let pending = state
            .pending
            .get(&source_id)
            .expect("fenced capture must widen the source");
        assert!(pending.paths.is_empty());
        assert!(pending.overflowed);
        assert!(watcher.is_some(), "fenced watcher must remain retained");
        assert_eq!(teardown.unresolved_count(), MAX_UNRESOLVED_TEARDOWNS);

        for release in teardown_releases {
            release.send(()).expect("release teardown worker");
        }
        let deadline = Instant::now() + Duration::from_secs(2);
        while teardown.unresolved_count() != 0 && Instant::now() < deadline {
            teardown.reap_finished();
            thread::yield_now();
        }
        assert_eq!(teardown.unresolved_count(), 0);
        retained_release_tx
            .send(())
            .expect("release retained watcher");
        drop(watcher);
    }

    fn completed_initializer(watcher: ActiveSourceWatcher) -> PendingSourceWatcher {
        let (_result_tx, result_rx) = std::sync::mpsc::channel();
        let join_handle = thread::spawn(|| {});
        while !join_handle.is_finished() {
            thread::yield_now();
        }
        PendingSourceWatcher {
            result_rx,
            ingress_enabled: Arc::new(AtomicBool::new(false)),
            join_handle,
            completed_result: Some(Ok((
                watcher,
                RootWatchUpdate {
                    changed_roots: Vec::new(),
                    has_unavailable_roots: false,
                    watch_failed: false,
                },
                HashMap::new(),
            ))),
            started_at: Instant::now(),
            backend: SourceWatcherBackend::Native,
        }
    }

    #[test]
    fn blocking_watcher_drops_have_a_fixed_tracked_worker_ceiling() {
        let _guard = lock_lifecycle_tests();
        let mut teardown = SourceWatcherTeardown {
            workers: Vec::new(),
        };
        let mut releases: Vec<SyncSender<()>> = Vec::new();
        for _ in 0..MAX_UNRESOLVED_TEARDOWNS {
            let (release_tx, release_rx) = std::sync::mpsc::sync_channel(1);
            if teardown.retire(blocking_watcher(release_rx)).is_err() {
                panic!("teardown slot should be available");
            }
            releases.push(release_tx);
        }
        let (extra_release_tx, extra_release_rx) = std::sync::mpsc::sync_channel(1);
        let retained = match teardown.retire(blocking_watcher(extra_release_rx)) {
            Err(watcher) => watcher,
            Ok(()) => panic!("a blocking drop must not spawn an unbounded reaper"),
        };
        assert_eq!(teardown.unresolved_count(), MAX_UNRESOLVED_TEARDOWNS);

        for release in releases {
            release.send(()).expect("release teardown worker");
        }
        let deadline = Instant::now() + Duration::from_secs(2);
        while teardown.unresolved_count() != 0 && Instant::now() < deadline {
            teardown.reap_finished();
            thread::yield_now();
        }
        assert_eq!(teardown.unresolved_count(), 0);
        extra_release_tx.send(()).expect("release retained watcher");
        drop(retained);
    }

    #[test]
    fn shutdown_hands_blocked_initializer_and_saturated_stale_drop_to_lifecycle_owner() {
        let _guard = lock_lifecycle_tests();
        let (initializer_release_tx, initializer_release_rx) = std::sync::mpsc::channel();
        let (stale_drop_release_tx, stale_drop_release_rx) = std::sync::mpsc::sync_channel(1);
        let mut teardown = SourceWatcherTeardown {
            workers: Vec::new(),
        };
        let mut teardown_releases = Vec::new();
        for _ in 0..MAX_UNRESOLVED_TEARDOWNS {
            let (release_tx, release_rx) = std::sync::mpsc::sync_channel(1);
            if teardown.retire(blocking_watcher(release_rx)).is_err() {
                panic!("occupy teardown slot");
            }
            teardown_releases.push(release_tx);
        }
        let lifecycle = SourceWatcherLifecycle {
            retired_initializers: vec![
                blocking_initializer(SourceWatcherBackend::Polling, initializer_release_rx),
                completed_initializer(blocking_watcher(stale_drop_release_rx)),
            ],
            teardown,
            retained_watcher: None,
        };
        let lifecycle_tx = start_source_watcher_lifecycle_service()
            .expect("start lifecycle service before shutdown handoff");
        let (handoff_tx, handoff_rx) = std::sync::mpsc::channel();
        thread::spawn(move || {
            lifecycle_tx
                .send(lifecycle)
                .expect("lifecycle service must accept shutdown handoff");
            let _ = handoff_tx.send(());
        });
        handoff_rx
            .recv_timeout(Duration::from_millis(500))
            .expect("shutdown must hand off blocking lifecycle work without dropping it");

        initializer_release_tx
            .send(())
            .expect("release initializer after shutdown handoff");
        stale_drop_release_tx
            .send(())
            .expect("release stale watcher after shutdown handoff");
        for release in teardown_releases {
            release.send(()).expect("release teardown worker");
        }
    }
}
