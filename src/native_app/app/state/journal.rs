//! Background ownership boundary for the profile-local operation journal.
//!
//! The UI owns only this bounded command/status handle.  The coordinator, profile lock,
//! startup scan, durable record writes, and lock release all stay on the owner thread.

use std::path::PathBuf;
use std::sync::mpsc::{self, Receiver, SyncSender, TrySendError};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{Duration, Instant};

use serde_json::Value;
use uuid::Uuid;

use crate::native_app::transaction_history::operation_journal::{
    JournalError, OperationDisposition, OperationIntent, OperationJournalCoordinator,
    OperationPhase,
};

const JOURNAL_COMMAND_CAPACITY: usize = 32;
const OWNER_POLL_INTERVAL: Duration = Duration::from_millis(50);
const OWNER_RETRY_INTERVAL: Duration = Duration::from_millis(25);
const OWNER_RETRY_MAX_INTERVAL: Duration = Duration::from_millis(250);
const OWNER_RETRY_TIMEOUT: Duration = Duration::from_secs(2);

#[repr(u8)]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum OwnerLifecycle {
    Initializing = 0,
    Available = 1,
    Unavailable = 2,
    Closing = 3,
    Closed = 4,
}

impl OwnerLifecycle {
    fn from_u8(value: u8) -> Self {
        match value {
            1 => Self::Available,
            2 => Self::Unavailable,
            3 => Self::Closing,
            4 => Self::Closed,
            _ => Self::Initializing,
        }
    }
}

/// Status emitted by the owner as profile journal ownership moves through startup.
///
/// The coordinator and recovery scan remain private to the owner thread; the UI receives only
/// this typed projection of the lifecycle.
#[derive(Clone, Debug, Eq, PartialEq)]
pub(in crate::native_app) enum OperationJournalStatus {
    Initializing,
    Available {
        summary: crate::native_app::transaction_history::operation_journal::RecoverySummary,
    },
    Unavailable {
        reason: OperationJournalUnavailable,
    },
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(in crate::native_app) enum OperationJournalUnavailable {
    OwnedByAnotherProcess { path: PathBuf },
    OpenFailed(String),
    OwnerStartFailed(String),
}

impl std::fmt::Display for OperationJournalUnavailable {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::OwnedByAnotherProcess { path } => {
                write!(formatter, "owned by another process: {}", path.display())
            }
            Self::OpenFailed(error) => formatter.write_str(error),
            Self::OwnerStartFailed(error) => formatter.write_str(error),
        }
    }
}

impl OperationJournalUnavailable {
    fn from_error(error: JournalError) -> Self {
        match error {
            JournalError::OwnedByAnotherProcess { path } => Self::OwnedByAnotherProcess { path },
            error => Self::OpenFailed(error.to_string()),
        }
    }
}

#[derive(Debug, thiserror::Error)]
pub(in crate::native_app) enum JournalOperationError {
    #[error(transparent)]
    Journal(#[from] JournalError),
    #[error("operation journal unavailable: {0}")]
    Unavailable(OperationJournalUnavailable),
    #[error("operation journal owner is shutting down")]
    Closed,
}

enum JournalCommand {
    Admit {
        intent: OperationIntent,
        payload: Value,
        result: SyncSender<Result<Uuid, JournalOperationError>>,
    },
    Update {
        operation_id: Uuid,
        phase: OperationPhase,
        disposition: OperationDisposition,
        result: SyncSender<Result<(), JournalOperationError>>,
    },
}

/// Handle retained by background application state.  It never contains the coordinator.
pub(in crate::native_app) struct OperationJournalOwner {
    commands: Option<SyncSender<JournalCommand>>,
    statuses: Receiver<OperationJournalStatus>,
    stop: std::sync::Arc<std::sync::atomic::AtomicBool>,
    lifecycle: Arc<std::sync::atomic::AtomicU8>,
    unavailable: Arc<Mutex<Option<OperationJournalUnavailable>>>,
    #[cfg(test)]
    blocked_receiver: Option<Receiver<JournalCommand>>,
}

impl OperationJournalOwner {
    /// Start the profile owner. Profile resolution and journal opening happen on the worker.
    #[cfg(not(test))]
    pub(in crate::native_app) fn start() -> Self {
        Self::start_with_optional_directory(None)
    }

    #[cfg(test)]
    pub(in crate::native_app) fn start_with_directory(directory: PathBuf) -> Self {
        Self::start_with_optional_directory(Some(directory))
    }

    fn start_with_optional_directory(directory: Option<PathBuf>) -> Self {
        Self::start_with_optional_directory_and_spawn_failure(directory, false)
    }

    #[cfg(test)]
    fn start_with_spawn_failure() -> Self {
        Self::start_with_optional_directory_and_spawn_failure(None, true)
    }

    fn start_with_optional_directory_and_spawn_failure(
        directory: Option<PathBuf>,
        force_spawn_failure: bool,
    ) -> Self {
        let (commands, command_rx) = mpsc::sync_channel(JOURNAL_COMMAND_CAPACITY);
        let (status_tx, statuses) = mpsc::channel();
        let stop = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
        let worker_stop = std::sync::Arc::clone(&stop);
        let lifecycle = Arc::new(std::sync::atomic::AtomicU8::new(
            OwnerLifecycle::Initializing as u8,
        ));
        let worker_lifecycle = Arc::clone(&lifecycle);
        let unavailable = Arc::new(Mutex::new(None));
        let worker_unavailable = Arc::clone(&unavailable);
        let worker_status_tx = status_tx.clone();
        let spawn_result = if force_spawn_failure {
            Err(std::io::Error::other("test owner thread spawn failure"))
        } else {
            thread::Builder::new()
                .name(String::from("wavecrate-operation-journal-owner"))
                .spawn(move || {
                    run_owner(
                        directory,
                        command_rx,
                        worker_status_tx,
                        worker_stop,
                        worker_lifecycle,
                        worker_unavailable,
                    );
                })
        };
        match spawn_result {
            Ok(_) => Self {
                commands: Some(commands),
                statuses,
                stop,
                lifecycle,
                unavailable,
                #[cfg(test)]
                blocked_receiver: None,
            },
            Err(error) => {
                let reason = OperationJournalUnavailable::OwnerStartFailed(error.to_string());
                *unavailable.lock().expect("journal unavailable state") = Some(reason.clone());
                lifecycle.store(
                    OwnerLifecycle::Unavailable as u8,
                    std::sync::atomic::Ordering::Release,
                );
                let _ = status_tx.send(OperationJournalStatus::Initializing);
                let _ = status_tx.send(OperationJournalStatus::Unavailable { reason });
                Self {
                    commands: None,
                    statuses,
                    stop,
                    lifecycle,
                    unavailable,
                    #[cfg(test)]
                    blocked_receiver: None,
                }
            }
        }
    }

    /// Test-only background state does not acquire the live profile lock.
    #[cfg(test)]
    pub(in crate::native_app) fn disabled() -> Self {
        let (commands, receiver) = mpsc::sync_channel(JOURNAL_COMMAND_CAPACITY);
        let (_status_tx, statuses) = mpsc::channel();
        let lifecycle = Arc::new(std::sync::atomic::AtomicU8::new(
            OwnerLifecycle::Available as u8,
        ));
        let unavailable = Arc::new(Mutex::new(None));
        Self {
            commands: Some(commands),
            statuses,
            stop: std::sync::Arc::new(std::sync::atomic::AtomicBool::new(true)),
            lifecycle,
            unavailable,
            blocked_receiver: Some(receiver),
        }
    }

    pub(in crate::native_app) fn take_status(&self) -> Option<OperationJournalStatus> {
        self.statuses.try_recv().ok()
    }

    /// Queue an admission without performing journal I/O on the caller thread.
    #[allow(dead_code)]
    pub(in crate::native_app) fn admit(
        &self,
        intent: OperationIntent,
        payload: Value,
    ) -> Result<Receiver<Result<Uuid, JournalOperationError>>, JournalOwnerQueueError> {
        self.ensure_available()?;
        let (result_tx, result_rx) = mpsc::sync_channel(1);
        self.commands
            .as_ref()
            .ok_or(JournalOwnerQueueError::Closed)?
            .try_send(JournalCommand::Admit {
                intent,
                payload,
                result: result_tx,
            })
            .map_err(|error| match error {
                TrySendError::Full(_) => JournalOwnerQueueError::Full,
                TrySendError::Disconnected(_) => JournalOwnerQueueError::Closed,
            })?;
        Ok(result_rx)
    }

    /// Queue a phase update without performing journal I/O on the caller thread.
    #[allow(dead_code)]
    pub(in crate::native_app) fn update(
        &self,
        operation_id: Uuid,
        phase: OperationPhase,
        disposition: OperationDisposition,
    ) -> Result<Receiver<Result<(), JournalOperationError>>, JournalOwnerQueueError> {
        self.ensure_available()?;
        let (result_tx, result_rx) = mpsc::sync_channel(1);
        self.commands
            .as_ref()
            .ok_or(JournalOwnerQueueError::Closed)?
            .try_send(JournalCommand::Update {
                operation_id,
                phase,
                disposition,
                result: result_tx,
            })
            .map_err(|error| match error {
                TrySendError::Full(_) => JournalOwnerQueueError::Full,
                TrySendError::Disconnected(_) => JournalOwnerQueueError::Closed,
            })?;
        Ok(result_rx)
    }

    /// Signal the owner to stop and close the command sender without joining its thread.
    ///
    /// The owner thread observes the closed channel, drops its coordinator, and releases the
    /// profile lock itself. This call never waits for owner-thread I/O or synchronization.
    pub(in crate::native_app) fn shutdown(&mut self) -> bool {
        let previous = self.lifecycle.swap(
            OwnerLifecycle::Closing as u8,
            std::sync::atomic::Ordering::AcqRel,
        );
        self.stop.store(true, std::sync::atomic::Ordering::Release);
        let closed = self.commands.take().is_some();
        #[cfg(test)]
        self.blocked_receiver.take();
        closed && OwnerLifecycle::from_u8(previous) != OwnerLifecycle::Closed
    }

    fn ensure_available(&self) -> Result<(), JournalOwnerQueueError> {
        match OwnerLifecycle::from_u8(self.lifecycle.load(std::sync::atomic::Ordering::Acquire)) {
            OwnerLifecycle::Initializing => Err(JournalOwnerQueueError::Initializing),
            OwnerLifecycle::Available => Ok(()),
            OwnerLifecycle::Unavailable => Err(JournalOwnerQueueError::Unavailable(
                self.unavailable
                    .lock()
                    .expect("journal unavailable state")
                    .clone()
                    .unwrap_or(OperationJournalUnavailable::OpenFailed(String::from(
                        "operation journal coordinator is unavailable",
                    ))),
            )),
            OwnerLifecycle::Closing | OwnerLifecycle::Closed => Err(JournalOwnerQueueError::Closed),
        }
    }
}

impl Drop for OperationJournalOwner {
    fn drop(&mut self) {
        let _ = self.shutdown();
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(in crate::native_app) enum JournalOwnerQueueError {
    Initializing,
    Unavailable(OperationJournalUnavailable),
    Full,
    Closed,
}

fn run_owner(
    directory: Option<PathBuf>,
    command_rx: Receiver<JournalCommand>,
    status_tx: mpsc::Sender<OperationJournalStatus>,
    stop: std::sync::Arc<std::sync::atomic::AtomicBool>,
    lifecycle: Arc<std::sync::atomic::AtomicU8>,
    unavailable: Arc<Mutex<Option<OperationJournalUnavailable>>>,
) {
    let _ = status_tx.send(OperationJournalStatus::Initializing);
    let mut retry_interval = OWNER_RETRY_INTERVAL;
    let retry_started = Instant::now();
    let mut coordinator = loop {
        if stop.load(std::sync::atomic::Ordering::Acquire) {
            lifecycle.store(
                OwnerLifecycle::Closed as u8,
                std::sync::atomic::Ordering::Release,
            );
            return;
        }
        let opened = match directory.as_ref() {
            Some(directory) => OperationJournalCoordinator::open(directory.clone()),
            None => OperationJournalCoordinator::open_current_profile(),
        };
        match opened {
            Ok(coordinator) => {
                let summary = coordinator.recovery_summary();
                lifecycle.store(
                    OwnerLifecycle::Available as u8,
                    std::sync::atomic::Ordering::Release,
                );
                let _ = status_tx.send(OperationJournalStatus::Available { summary });
                break Some(coordinator);
            }
            Err(JournalError::OwnedByAnotherProcess { path }) => {
                // Ownership replacement is retried here, never admitted while initializing.
                if retry_started.elapsed() >= OWNER_RETRY_TIMEOUT {
                    let reason = OperationJournalUnavailable::OwnedByAnotherProcess { path };
                    *unavailable.lock().expect("journal unavailable state") = Some(reason.clone());
                    lifecycle.store(
                        OwnerLifecycle::Unavailable as u8,
                        std::sync::atomic::Ordering::Release,
                    );
                    let _ = status_tx.send(OperationJournalStatus::Unavailable { reason });
                    break None;
                }
                thread::sleep(retry_interval);
                retry_interval = (retry_interval * 2).min(OWNER_RETRY_MAX_INTERVAL);
            }
            Err(error) => {
                let reason = OperationJournalUnavailable::from_error(error);
                *unavailable.lock().expect("journal unavailable state") = Some(reason.clone());
                lifecycle.store(
                    OwnerLifecycle::Unavailable as u8,
                    std::sync::atomic::Ordering::Release,
                );
                let _ = status_tx.send(OperationJournalStatus::Unavailable { reason });
                break None;
            }
        }
    };

    while !stop.load(std::sync::atomic::Ordering::Acquire) {
        match command_rx.recv_timeout(OWNER_POLL_INTERVAL) {
            Ok(JournalCommand::Admit {
                intent,
                payload,
                result,
            }) => {
                if stop.load(std::sync::atomic::Ordering::Acquire) {
                    let _ = result.send(Err(JournalOperationError::Closed));
                    continue;
                }
                let outcome = coordinator
                    .as_mut()
                    .ok_or_else(|| {
                        JournalOperationError::Unavailable(
                            unavailable
                                .lock()
                                .expect("journal unavailable state")
                                .clone()
                                .unwrap_or(OperationJournalUnavailable::OpenFailed(String::from(
                                    "operation journal coordinator is unavailable",
                                ))),
                        )
                    })
                    .and_then(|coordinator| {
                        coordinator
                            .admit(intent, payload)
                            .map_err(JournalOperationError::Journal)
                    });
                let _ = result.send(outcome);
            }
            Ok(JournalCommand::Update {
                operation_id,
                phase,
                disposition,
                result,
            }) => {
                if stop.load(std::sync::atomic::Ordering::Acquire) {
                    let _ = result.send(Err(JournalOperationError::Closed));
                    continue;
                }
                let outcome = coordinator
                    .as_mut()
                    .ok_or_else(|| {
                        JournalOperationError::Unavailable(
                            unavailable
                                .lock()
                                .expect("journal unavailable state")
                                .clone()
                                .unwrap_or(OperationJournalUnavailable::OpenFailed(String::from(
                                    "operation journal coordinator is unavailable",
                                ))),
                        )
                    })
                    .and_then(|coordinator| {
                        coordinator
                            .update(operation_id, phase, disposition)
                            .map_err(JournalOperationError::Journal)
                    });
                let _ = result.send(outcome);
            }
            Err(mpsc::RecvTimeoutError::Timeout) => {}
            Err(mpsc::RecvTimeoutError::Disconnected) => break,
        }
    }
    drop(coordinator);
    lifecycle.store(
        OwnerLifecycle::Closed as u8,
        std::sync::atomic::Ordering::Release,
    );
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::native_app::transaction_history::operation_journal::{
        OperationActor, OperationKind,
    };

    fn intent() -> OperationIntent {
        OperationIntent {
            actor: OperationActor::User,
            kind: OperationKind::FileHistory,
            label: String::from("owner test"),
        }
    }

    #[test]
    fn startup_scan_and_diagnostic_run_on_owner_thread() {
        let directory = tempfile::tempdir().expect("journal directory");
        let owner = OperationJournalOwner::start_with_directory(directory.path().to_path_buf());
        assert_eq!(
            owner
                .statuses
                .recv_timeout(Duration::from_secs(2))
                .expect("initializing status"),
            OperationJournalStatus::Initializing
        );
        let status = owner
            .statuses
            .recv_timeout(Duration::from_secs(2))
            .expect("startup status");
        assert!(matches!(status, OperationJournalStatus::Available { .. }));
    }

    #[test]
    fn unresolved_recovery_summary_is_published_without_rewriting_record() {
        let directory = tempfile::tempdir().expect("journal directory");
        let path = directory.path().to_path_buf();
        let operation_id = {
            let mut coordinator =
                OperationJournalCoordinator::open(path.clone()).expect("open journal");
            coordinator
                .admit(intent(), Value::Null)
                .expect("admit unresolved record")
        };
        let record_path = path.join(format!("{operation_id}.json"));
        let before = std::fs::read(&record_path).expect("read record before startup");

        let owner = OperationJournalOwner::start_with_directory(path);
        assert_eq!(
            owner
                .statuses
                .recv_timeout(Duration::from_secs(2))
                .expect("initializing status"),
            OperationJournalStatus::Initializing
        );
        let status = owner
            .statuses
            .recv_timeout(Duration::from_secs(2))
            .expect("available status");
        match status {
            OperationJournalStatus::Available { summary } => {
                assert_eq!(summary.record_count, 1);
                assert_eq!(summary.unresolved_count, 1);
                assert!(summary.attention_required);
            }
            other => panic!("expected available status, got {other:?}"),
        }
        assert_eq!(
            std::fs::read(record_path).expect("read record after startup"),
            before
        );
    }

    #[test]
    fn owner_thread_spawn_failure_closes_admission_and_reports_typed_unavailable() {
        let owner = OperationJournalOwner::start_with_spawn_failure();
        assert_eq!(
            owner
                .statuses
                .recv_timeout(Duration::from_secs(2))
                .expect("initializing status"),
            OperationJournalStatus::Initializing
        );
        assert!(matches!(
            owner
                .statuses
                .recv_timeout(Duration::from_secs(2))
                .expect("unavailable status"),
            OperationJournalStatus::Unavailable {
                reason: OperationJournalUnavailable::OwnerStartFailed(_),
            }
        ));
        assert!(matches!(
            owner.admit(intent(), Value::Null),
            Err(JournalOwnerQueueError::Unavailable(
                OperationJournalUnavailable::OwnerStartFailed(_)
            ))
        ));
        assert_eq!(
            OwnerLifecycle::from_u8(owner.lifecycle.load(std::sync::atomic::Ordering::Acquire)),
            OwnerLifecycle::Unavailable
        );
    }

    #[test]
    fn durable_admission_is_queued_and_releases_on_shutdown() {
        let directory = tempfile::tempdir().expect("journal directory");
        let path = directory.path().to_path_buf();
        let mut owner = OperationJournalOwner::start_with_directory(path.clone());
        assert_eq!(
            owner
                .statuses
                .recv_timeout(Duration::from_secs(2))
                .expect("initializing status"),
            OperationJournalStatus::Initializing
        );
        let status = owner
            .statuses
            .recv_timeout(Duration::from_secs(2))
            .expect("startup status");
        assert!(matches!(status, OperationJournalStatus::Available { .. }));
        let result = owner
            .admit(intent(), Value::Null)
            .expect("queue admission")
            .recv_timeout(Duration::from_secs(2))
            .expect("admission result")
            .expect("durable admission");
        assert!(path.join(format!("{result}.json")).exists());
        assert!(owner.shutdown());
        let deadline = std::time::Instant::now() + Duration::from_secs(2);
        loop {
            if OperationJournalCoordinator::open(path.clone()).is_ok() {
                break;
            }
            assert!(
                std::time::Instant::now() < deadline,
                "owner did not release lock"
            );
            std::thread::sleep(Duration::from_millis(10));
        }
    }

    #[test]
    fn bounded_commands_use_try_send_and_fail_when_full() {
        let owner = OperationJournalOwner::disabled();
        for _ in 0..JOURNAL_COMMAND_CAPACITY {
            owner
                .admit(intent(), Value::Null)
                .expect("bounded queue admission");
        }
        assert!(matches!(
            owner.admit(intent(), Value::Null),
            Err(JournalOwnerQueueError::Full)
        ));
    }

    #[test]
    fn open_conflict_waits_for_replacement_and_gates_admission() {
        let directory = tempfile::tempdir().expect("journal directory");
        let path = directory.path().to_path_buf();
        let holder = OperationJournalCoordinator::open(path.clone()).expect("open holder");
        let owner = OperationJournalOwner::start_with_directory(path);
        assert_eq!(
            owner
                .statuses
                .recv_timeout(Duration::from_secs(2))
                .expect("initializing status"),
            OperationJournalStatus::Initializing
        );
        assert!(matches!(
            owner.admit(intent(), Value::Null),
            Err(JournalOwnerQueueError::Initializing)
        ));
        drop(holder);
        let status = owner
            .statuses
            .recv_timeout(Duration::from_secs(2))
            .expect("available status after replacement");
        assert!(matches!(status, OperationJournalStatus::Available { .. }));
        let result = owner
            .admit(intent(), Value::Null)
            .expect("queue admission")
            .recv_timeout(Duration::from_secs(2))
            .expect("admission result")
            .expect("durable admission");
        assert!(directory.path().join(format!("{result}.json")).exists());
    }

    #[test]
    fn open_failure_reports_unavailable() {
        let directory = tempfile::tempdir().expect("journal directory");
        let path = directory.path().join("not-a-directory");
        std::fs::write(&path, b"fixture").expect("fixture file");
        let owner = OperationJournalOwner::start_with_directory(path);
        assert_eq!(
            owner
                .statuses
                .recv_timeout(Duration::from_secs(2))
                .expect("initializing status"),
            OperationJournalStatus::Initializing
        );
        let status = owner
            .statuses
            .recv_timeout(Duration::from_secs(2))
            .expect("unavailable status");
        assert!(matches!(status, OperationJournalStatus::Unavailable { .. }));
    }
}
