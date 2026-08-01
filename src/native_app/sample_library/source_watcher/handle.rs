#[cfg(test)]
use notify::EventKind;
use notify::{Config, Event, EventHandler, PollWatcher, Watcher};
use std::{
    collections::{HashMap, HashSet},
    path::PathBuf,
    sync::{
        Arc, Mutex, OnceLock,
        atomic::{AtomicBool, Ordering},
        mpsc::{Receiver, Sender, SyncSender, TryRecvError, TrySendError},
    },
    thread,
    time::{Duration, Instant},
};
use wavecrate::sample_sources::{SampleSource, SourceId};

use super::classification::retain_source_refresh_candidates;
use super::journal::{self, JournalRecovery};
use super::roots::{
    RootIdentityRecovery, RootWatchUpdate, WatchedRootIdentities, root_watch_status,
    update_watched_roots,
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

struct ActiveSourceWatcher {
    _watcher: Box<dyn Watcher + Send>,
    ingress_enabled: Arc<AtomicBool>,
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
    event_tx: SyncSender<notify::Result<Event>>,
    overflowed: Arc<AtomicBool>,
    enabled: Arc<AtomicBool>,
}

impl EventHandler for SourceWatcherIngress {
    fn handle_event(&mut self, event: notify::Result<Event>) {
        if !self.enabled.load(Ordering::Acquire) {
            return;
        }
        let event = match event {
            Ok(mut event) => {
                if !retain_source_refresh_candidates(&mut event) {
                    return;
                }
                Ok(event)
            }
            Err(error) => Err(error),
        };
        match self.event_tx.try_send(event) {
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
        sources: Vec<SampleSource>,
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
            Some(lifecycle_tx) => run_source_watcher(command_rx, message_tx, sources, lifecycle_tx),
            None => run_source_watcher_without_lifecycle(command_rx),
        });
        Self {
            command_tx,
            join_handle: Some(handle),
            lifecycle_tx,
        }
    }

    pub(in crate::native_app) fn replace_sources(&self, sources: Vec<SampleSource>) {
        let _ = self
            .command_tx
            .send(GuiSourceWatchCommand::ReplaceSources(sources));
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
    ReplaceSources(Vec<SampleSource>),
    #[cfg(test)]
    ReconcileAllSources,
    AcknowledgeCommittedPaths {
        source_id: SourceId,
        echoes: Vec<CommittedWatcherEcho>,
        cursor: RevisionFirstCursor,
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
    Shutdown,
}

fn run_source_watcher(
    command_rx: Receiver<GuiSourceWatchCommand>,
    message_tx: Sender<GuiMessage>,
    initial_sources: Vec<SampleSource>,
    lifecycle_tx: Sender<SourceWatcherLifecycle>,
) {
    let (event_tx, event_rx) = std::sync::mpsc::sync_channel(WATCHER_EVENT_QUEUE_CAPACITY);
    let ingress_overflowed = Arc::new(AtomicBool::new(false));
    let mut watcher = None;
    let mut pending_watcher = None;
    let mut retired_initializers = Vec::with_capacity(MAX_UNRESOLVED_INITIALIZERS);
    let mut teardown = SourceWatcherTeardown {
        workers: Vec::new(),
    };
    let mut state = GuiSourceWatchState::default();
    state.set_sources(initial_sources);
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
            Ok(GuiSourceWatchCommand::ReplaceSources(sources)) => {
                let roots_changed =
                    desired_watched_roots(&sources) != desired_watched_roots(&state.sources);
                state.set_sources(sources);
                if roots_changed {
                    retire_source_watcher(&mut watcher, &mut teardown);
                    cancel_pending_source_watcher(&mut pending_watcher, &mut retired_initializers);
                    state.reset_watches(Instant::now());
                    next_restart = Instant::now();
                    restart_delay = WATCHER_RESTART_MIN;
                }
                next_root_refresh = Instant::now();
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
            Ok(GuiSourceWatchCommand::FinishJournalBarrierAudit {
                source_id,
                lifecycle_generation,
                source_revision,
                complete,
            }) => {
                if complete {
                    if let Some(source_revision) = source_revision {
                        if let Some(barrier) = audit_barriers.remove(&source_id) {
                            let checkpoint = barrier.into_revision_bound(
                                source_id.clone(),
                                lifecycle_generation,
                                source_revision,
                            );
                            let _ = message_tx
                                .send(GuiMessage::SourceWatcherCheckpointReady(checkpoint));
                        }
                    } else {
                        tracing::warn!(
                            source_id,
                            lifecycle_generation,
                            "Completed source audit did not provide a committed revision for its watcher barrier"
                        );
                    }
                }
                if deferred_audit_barrier_sources.remove(&source_id) {
                    // The unavailable-watcher audit predates watcher recovery. Capture only now,
                    // after that audit has completed, then schedule a fresh audit tied to this
                    // barrier instead of letting the older completion advance a new cursor.
                    if let Some(barrier) =
                        journal::capture_audit_barrier(&state.sources, &source_id)
                    {
                        audit_barriers.insert(source_id.clone(), barrier);
                        let _ = message_tx.send(GuiMessage::SourceWatcherJournalGap {
                            source_id,
                            reason: "watcher_recovered_after_unavailable",
                        });
                    }
                }
            }
            #[cfg(test)]
            Ok(GuiSourceWatchCommand::ReconcileAllSources) => {
                state.mark_all_overflowed(Instant::now());
            }
            #[cfg(test)]
            Ok(GuiSourceWatchCommand::ForceRestart) => {
                retire_source_watcher(&mut watcher, &mut teardown);
                cancel_pending_source_watcher(&mut pending_watcher, &mut retired_initializers);
                state.reset_watches(Instant::now());
                next_restart = Instant::now();
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
                match event_tx.try_send(Ok(event)) {
                    Ok(()) => {}
                    Err(TrySendError::Full(_)) => {
                        ingress_overflowed.store(true, Ordering::Release);
                    }
                    Err(TrySendError::Disconnected(_)) => {}
                }
            }
            Ok(GuiSourceWatchCommand::Shutdown) => {
                cancel_pending_source_watcher(&mut pending_watcher, &mut retired_initializers);
                retire_source_watcher_on_shutdown(&mut watcher, &mut teardown);
                break;
            }
            Err(std::sync::mpsc::RecvTimeoutError::Timeout) => {}
            Err(std::sync::mpsc::RecvTimeoutError::Disconnected) => {
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
                            publish_closed_app_journal_recovery(
                                &message_tx,
                                &state.sources,
                                backend == SourceWatcherBackend::Native,
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

        let mut watcher_failed = false;
        let mut root_invalidated = false;
        while let Ok(event) = event_rx.try_recv() {
            let event: notify::Result<Event> = event;
            match event {
                Ok(event) => {
                    root_invalidated |= state.collect_event(&event, Instant::now());
                }
                Err(error) => {
                    tracing::warn!("GUI source watcher error: {error}");
                    watcher_failed = true;
                }
            }
        }

        if watcher_failed || root_invalidated {
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
                paths: event.paths,
                overflowed: event.overflowed,
                source_root_available: event.source_root_available,
                journal_checkpoint_event_id: None,
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

fn publish_closed_app_journal_recovery(
    message_tx: &Sender<GuiMessage>,
    sources: &[SampleSource],
    native_watcher: bool,
    audit_barriers: &mut HashMap<String, journal::AuditBarrier>,
    deferred_audit_barrier_sources: &mut HashSet<String>,
    defer_audit_barriers: bool,
) {
    for (source, recovery) in sources
        .iter()
        .zip(journal::recover_sources(sources, native_watcher))
    {
        match recovery {
            #[cfg(target_os = "macos")]
            JournalRecovery::Changes { paths, event_id } if paths.is_empty() => {
                tracing::info!(
                    source_id = source.id.as_str(),
                    event_id,
                    "Empty closed-application source watcher replay requires a bounded manifest audit"
                );
                if defer_audit_barriers {
                    deferred_audit_barrier_sources.insert(source.id.as_str().to_string());
                } else {
                    if let Some(barrier) =
                        journal::capture_audit_barrier(sources, source.id.as_str())
                    {
                        audit_barriers.insert(source.id.as_str().to_string(), barrier);
                    }
                    let _ = message_tx.send(GuiMessage::SourceWatcherJournalGap {
                        source_id: source.id.as_str().to_string(),
                        reason: "journal_replay_empty_paths",
                    });
                }
            }
            #[cfg(target_os = "macos")]
            JournalRecovery::Changes { paths, event_id } => {
                tracing::info!(
                    source_id = source.id.as_str(),
                    path_count = paths.len(),
                    "Replaying closed-application source watcher changes"
                );
                let _ = message_tx.send(GuiMessage::SourceFilesystemChanged {
                    source_id: source.id.as_str().to_string(),
                    paths,
                    overflowed: false,
                    source_root_available: true,
                    journal_checkpoint_event_id: Some(event_id),
                });
            }
            JournalRecovery::FullAudit { reason } => {
                tracing::info!(
                    source_id = source.id.as_str(),
                    reason,
                    "Durable source watcher coverage requires a bounded manifest audit"
                );
                if defer_audit_barriers {
                    // An unavailable-watcher fallback may already be auditing this source.
                    // Wait for that completion command before capturing the barrier and emitting
                    // the replacement audit request; otherwise the replacement could start
                    // before its fence exists.
                    deferred_audit_barrier_sources.insert(source.id.as_str().to_string());
                } else {
                    if let Some(barrier) =
                        journal::capture_audit_barrier(sources, source.id.as_str())
                    {
                        audit_barriers.insert(source.id.as_str().to_string(), barrier);
                    }
                    let _ = message_tx.send(GuiMessage::SourceWatcherJournalGap {
                        source_id: source.id.as_str().to_string(),
                        reason,
                    });
                }
            }
        }
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
    event_tx: SyncSender<notify::Result<Event>>,
    ingress_overflowed: Arc<AtomicBool>,
    backend: SourceWatcherBackend,
) -> Result<PendingSourceWatcher, String> {
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
    event_tx: SyncSender<notify::Result<Event>>,
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
    use super::*;
    use notify::{RecursiveMode, WatcherKind};
    use std::{
        collections::HashMap,
        path::Path,
        sync::{
            Mutex, OnceLock,
            mpsc::{Receiver, SyncSender},
        },
    };
    use wavecrate::sample_sources::{SampleSource, SourceId};

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
        }
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
