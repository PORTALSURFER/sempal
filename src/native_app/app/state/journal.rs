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

use crate::native_app::transaction_history::operation_journal::{
    BoundedAdmissionError, FilesystemStageOutcome, JournalError, OperationDisposition, OperationId,
    OperationIntent, OperationJournalCoordinator, OperationPhase, PreparedOperationOutcome,
    RecoveryRootCapability,
};
use crate::native_app::transaction_history::{
    HistoryFileAction, HistoryFileIoDirection, RejectedBeforeIntent,
};
use crate::native_app::transaction_history::{
    acquire_absent_final_publication_guard, acquire_expected_identity_publication,
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
        recovery_root: RecoveryRootCapability,
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
    #[error("operation rejected before durable intent: {0}")]
    RejectedBeforeIntent(RejectedBeforeIntent),
    #[error(transparent)]
    Journal(#[from] JournalError),
    #[error("operation journal unavailable: {0}")]
    Unavailable(OperationJournalUnavailable),
    #[error("operation journal owner is shutting down")]
    Closed,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(in crate::native_app) enum OperationJournalRestoreError {
    RejectedBeforeIntent(RejectedBeforeIntent),
    Journal(String),
    Unavailable(String),
    Closed,
}

impl From<JournalOperationError> for OperationJournalRestoreError {
    fn from(error: JournalOperationError) -> Self {
        match error {
            JournalOperationError::RejectedBeforeIntent(error) => Self::RejectedBeforeIntent(error),
            JournalOperationError::Journal(error) => Self::Journal(error.to_string()),
            JournalOperationError::Unavailable(error) => Self::Unavailable(error.to_string()),
            JournalOperationError::Closed => Self::Closed,
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(in crate::native_app) struct OperationJournalRestoreCompletion {
    pub(in crate::native_app) execution_id: u64,
    pub(in crate::native_app) transaction_id: u64,
    pub(in crate::native_app) direction: HistoryFileIoDirection,
    pub(in crate::native_app) through_target: Option<u64>,
    pub(in crate::native_app) label: String,
    pub(in crate::native_app) result: Result<FilesystemStageOutcome, OperationJournalRestoreError>,
}

enum JournalCommand {
    BoundedWaveformRestore {
        intent: OperationIntent,
        payload: Value,
        direction: HistoryFileIoDirection,
        actions: Vec<HistoryFileAction>,
        result: SyncSender<Result<OperationId, JournalOperationError>>,
    },
    PrepareBoundedWaveformRestore {
        intent: OperationIntent,
        payload: Value,
        direction: HistoryFileIoDirection,
        actions: Vec<HistoryFileAction>,
        result: SyncSender<Result<PreparedOperationOutcome, JournalOperationError>>,
    },
    PrepareAndStageBoundedWaveformRestore {
        intent: OperationIntent,
        payload: Value,
        direction: HistoryFileIoDirection,
        actions: Vec<HistoryFileAction>,
        result: SyncSender<Result<FilesystemStageOutcome, JournalOperationError>>,
    },
    #[cfg(test)]
    Admit {
        intent: OperationIntent,
        payload: Value,
        result: SyncSender<Result<OperationId, JournalOperationError>>,
    },
    Update {
        operation_id: OperationId,
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
        // macOS tempfile paths commonly live below `/var`, which is a symlink to
        // `/private/var`. Resolve that test-only harness path before the owner
        // applies strict no-follow traversal to its capability root.
        let directory = std::fs::canonicalize(&directory).unwrap_or(directory);
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
    #[cfg(test)]
    #[allow(dead_code)]
    pub(in crate::native_app) fn admit(
        &self,
        intent: OperationIntent,
        payload: Value,
    ) -> Result<Receiver<Result<OperationId, JournalOperationError>>, JournalOwnerQueueError> {
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

    /// Queue the bounded history capacity gate.  Shape and filesystem facts are resolved only
    /// by the owner thread before the durable intent is written.
    #[allow(dead_code)]
    pub(in crate::native_app) fn admit_bounded_waveform_restore(
        &self,
        intent: OperationIntent,
        payload: Value,
        direction: HistoryFileIoDirection,
        actions: Vec<HistoryFileAction>,
    ) -> Result<Receiver<Result<OperationId, JournalOperationError>>, JournalOwnerQueueError> {
        self.ensure_available()?;
        let (result_tx, result_rx) = mpsc::sync_channel(1);
        self.commands
            .as_ref()
            .ok_or(JournalOwnerQueueError::Closed)?
            .try_send(JournalCommand::BoundedWaveformRestore {
                intent,
                payload,
                direction,
                actions,
                result: result_tx,
            })
            .map_err(|error| match error {
                TrySendError::Full(_) => JournalOwnerQueueError::Full,
                TrySendError::Disconnected(_) => JournalOwnerQueueError::Closed,
            })?;
        Ok(result_rx)
    }

    /// Queue owner-thread admission plus typed preparation for one waveform restore.
    #[allow(dead_code)]
    pub(in crate::native_app) fn prepare_bounded_waveform_restore(
        &self,
        intent: OperationIntent,
        payload: Value,
        direction: HistoryFileIoDirection,
        actions: Vec<HistoryFileAction>,
    ) -> Result<
        Receiver<Result<PreparedOperationOutcome, JournalOperationError>>,
        JournalOwnerQueueError,
    > {
        self.ensure_available()?;
        let (result_tx, result_rx) = mpsc::sync_channel(1);
        self.commands
            .as_ref()
            .ok_or(JournalOwnerQueueError::Closed)?
            .try_send(JournalCommand::PrepareBoundedWaveformRestore {
                intent,
                payload,
                direction,
                actions,
                result: result_tx,
            })
            .map_err(|error| match error {
                TrySendError::Full(_) => JournalOwnerQueueError::Full,
                TrySendError::Disconnected(_) => JournalOwnerQueueError::Closed,
            })?;
        Ok(result_rx)
    }

    /// Queue owner-thread admission, preparation, and destination-local staging for one
    /// waveform restore. Final target publication remains outside this bounded command.
    #[allow(dead_code)]
    pub(in crate::native_app) fn prepare_and_stage_bounded_waveform_restore(
        &self,
        intent: OperationIntent,
        payload: Value,
        direction: HistoryFileIoDirection,
        actions: Vec<HistoryFileAction>,
    ) -> Result<
        Receiver<Result<FilesystemStageOutcome, JournalOperationError>>,
        JournalOwnerQueueError,
    > {
        self.ensure_available()?;
        let (result_tx, result_rx) = mpsc::sync_channel(1);
        self.commands
            .as_ref()
            .ok_or(JournalOwnerQueueError::Closed)?
            .try_send(JournalCommand::PrepareAndStageBoundedWaveformRestore {
                intent,
                payload,
                direction,
                actions,
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
        operation_id: OperationId,
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
                // The explicit journal directory is also the recovery root for
                // test/runtime overrides. Production callers pass `None`, so
                // the capability helper resolves the profile app root here on
                // the owner thread.
                let recovery_path = directory.clone();
                let recovery_root = match crate::native_app::transaction_history::operation_journal::open_recovery_root_capability(recovery_path) {
                    Ok(capability) => capability,
                    Err(error) => {
                        let reason = OperationJournalUnavailable::from_error(error);
                        *unavailable.lock().expect("journal unavailable state") = Some(reason.clone());
                        lifecycle.store(OwnerLifecycle::Unavailable as u8, std::sync::atomic::Ordering::Release);
                        let _ = status_tx.send(OperationJournalStatus::Unavailable { reason });
                        break None;
                    }
                };
                lifecycle.store(
                    OwnerLifecycle::Available as u8,
                    std::sync::atomic::Ordering::Release,
                );
                let _ = status_tx.send(OperationJournalStatus::Available {
                    summary,
                    recovery_root,
                });
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
            Ok(JournalCommand::BoundedWaveformRestore {
                intent,
                payload,
                direction,
                actions,
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
                            .admit_bounded_waveform_restore(intent, payload, direction, &actions)
                            .map_err(|error| match error {
                                BoundedAdmissionError::Rejected(rejection) => {
                                    JournalOperationError::RejectedBeforeIntent(rejection)
                                }
                                BoundedAdmissionError::Journal(error) => {
                                    JournalOperationError::Journal(error)
                                }
                            })
                    });
                let _ = result.send(outcome);
            }
            Ok(JournalCommand::PrepareBoundedWaveformRestore {
                intent,
                payload,
                direction,
                actions,
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
                            .prepare_bounded_waveform_restore(intent, payload, direction, &actions)
                            .map_err(|error| match error {
                                BoundedAdmissionError::Rejected(rejection) => {
                                    JournalOperationError::RejectedBeforeIntent(rejection)
                                }
                                BoundedAdmissionError::Journal(error) => {
                                    JournalOperationError::Journal(error)
                                }
                            })
                    });
                let _ = result.send(outcome);
            }
            Ok(JournalCommand::PrepareAndStageBoundedWaveformRestore {
                intent,
                payload,
                direction,
                actions,
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
                        match coordinator
                            .prepare_bounded_waveform_restore(intent, payload, direction, &actions)
                        {
                            Ok(PreparedOperationOutcome::Prepared(operation_id)) => {
                                match coordinator
                                    .stage_admitted_bounded_waveform_restore(operation_id)
                                    .map_err(JournalOperationError::Journal)?
                                {
                                    FilesystemStageOutcome::FilesystemStaged(operation_id) => {
                                        if let Some(mut context) = coordinator
                                            .prepare_absent_final_publication_attempt_if_needed(
                                                operation_id,
                                            )
                                            .map_err(JournalOperationError::Journal)?
                                        {
                                            let request = context
                                                .take_guard_request()
                                                .map_err(JournalOperationError::Journal)?;
                                            let owner_result =
                                                acquire_absent_final_publication_guard(
                                                    request,
                                                    operation_id,
                                                );
                                            coordinator
                                                .commit_absent_final_publication_attempt(
                                                    context,
                                                    owner_result,
                                                )
                                                .map_err(JournalOperationError::Journal)
                                        } else if let Some(mut context) = coordinator
                                            .prepare_expected_identity_publication_attempt_if_needed(
                                                operation_id,
                                            )
                                            .map_err(JournalOperationError::Journal)?
                                        {
                                            let request = context
                                                .take_owner_request()
                                                .map_err(JournalOperationError::Journal)?;
                                            let owner_result =
                                                acquire_expected_identity_publication(request);
                                            coordinator
                                                .commit_expected_identity_publication_attempt(
                                                    context,
                                                    owner_result,
                                                )
                                                .map_err(JournalOperationError::Journal)
                                        } else {
                                            Err(JournalOperationError::Journal(
                                                JournalError::InvalidPublicationEvidence {
                                                    operation_id,
                                                    reason: String::from(
                                                        "filesystem-staged restore has no publication owner contract",
                                                    ),
                                                },
                                            ))
                                        }
                                    }
                                    outcome => Ok(outcome),
                                }
                            }
                            Ok(PreparedOperationOutcome::RetryPending {
                                operation_id,
                                reason,
                            }) => Ok(FilesystemStageOutcome::RetryPending {
                                operation_id,
                                reason,
                            }),
                            Ok(PreparedOperationOutcome::JournalWriteFailed {
                                operation_id,
                                reason,
                            }) => Ok(FilesystemStageOutcome::JournalWriteFailed {
                                operation_id,
                                reason,
                            }),
                            Err(BoundedAdmissionError::Rejected(rejection)) => {
                                Err(JournalOperationError::RejectedBeforeIntent(rejection))
                            }
                            Err(BoundedAdmissionError::Journal(error)) => {
                                Err(JournalOperationError::Journal(error))
                            }
                        }
                    });
                let _ = result.send(outcome);
            }
            #[cfg(test)]
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
    use crate::native_app::transaction_history::HistoryFileIoDirection;
    use crate::native_app::transaction_history::operation_journal::{
        FilesystemStagedParticipant, OperationActor, OperationKind,
    };
    use crate::native_app::waveform_edits::waveform_restore_action_for_capacity_tests;

    fn fixture_directory() -> tempfile::TempDir {
        #[cfg(target_os = "macos")]
        {
            return tempfile::tempdir_in("/private/tmp").expect("fixture directory");
        }
        #[cfg(not(target_os = "macos"))]
        tempfile::tempdir().expect("fixture directory")
    }

    fn intent() -> OperationIntent {
        OperationIntent {
            actor: OperationActor::User,
            kind: OperationKind::FileHistory,
            label: String::from("owner test"),
        }
    }

    #[test]
    fn owner_admits_bounded_restore_only_after_owner_thread_gate() {
        let directory = tempfile::tempdir().expect("journal directory");
        let files = fixture_directory();
        let backup = files.path().join("before.wav");
        let target = files.path().join("target.wav");
        std::fs::write(&backup, vec![7_u8; 4097]).expect("backup");
        std::fs::write(&target, vec![0_u8; 4097]).expect("target");
        let action = waveform_restore_action_for_capacity_tests(backup, target, false);
        let owner = OperationJournalOwner::start_with_directory(directory.path().to_path_buf());
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
                .expect("available status"),
            OperationJournalStatus::Available { .. }
        ));
        let rejected = owner
            .admit_bounded_waveform_restore(
                intent(),
                Value::Null,
                HistoryFileIoDirection::Undo,
                Vec::new(),
            )
            .expect("queue invalid shape")
            .recv_timeout(Duration::from_secs(2))
            .expect("invalid-shape result");
        assert!(matches!(
            rejected,
            Err(JournalOperationError::RejectedBeforeIntent(
                RejectedBeforeIntent::InvalidShape
            ))
        ));
        let missing_target = files.path().join("missing-target.wav");
        let missing_action = waveform_restore_action_for_capacity_tests(
            files.path().join("before.wav"),
            missing_target,
            false,
        );
        let missing = owner
            .admit_bounded_waveform_restore(
                intent(),
                Value::Null,
                HistoryFileIoDirection::Undo,
                vec![missing_action],
            )
            .expect("queue missing target")
            .recv_timeout(Duration::from_secs(2))
            .expect("missing-target result");
        assert!(matches!(
            missing,
            Err(JournalOperationError::RejectedBeforeIntent(
                RejectedBeforeIntent::MissingTarget(_)
            ))
        ));
        assert_eq!(
            std::fs::read_dir(directory.path())
                .expect("journal entries")
                .filter_map(Result::ok)
                .filter(
                    |entry| entry.path().extension().and_then(|ext| ext.to_str()) == Some("json")
                )
                .count(),
            0
        );
        let result = owner
            .admit_bounded_waveform_restore(
                intent(),
                Value::Null,
                HistoryFileIoDirection::Undo,
                vec![action],
            )
            .expect("queue bounded admission")
            .recv_timeout(Duration::from_secs(2))
            .expect("admission result")
            .expect("bounded admission");
        drop(owner);
        let record = std::fs::read_dir(directory.path())
            .expect("journal entries")
            .filter_map(Result::ok)
            .find(|entry| entry.path().extension().and_then(|ext| ext.to_str()) == Some("json"))
            .map(|entry| std::fs::read(entry.path()).expect("record bytes"))
            .expect("durable record");
        let value: Value = serde_json::from_slice(&record).expect("record json");
        assert_eq!(value["operation_id"], result.to_string());
        assert!(value["capacity_plan"]["volumes"].is_array());
    }

    #[test]
    fn owner_prepares_stages_and_fail_closed_publication_without_target_mutation() {
        let directory = tempfile::tempdir().expect("journal directory");
        let files = fixture_directory();
        let backup = files.path().join("before.wav");
        let target = files.path().join("target.wav");
        std::fs::write(&backup, vec![7_u8; 4097]).expect("backup");
        std::fs::write(&target, vec![0_u8; 4097]).expect("target");
        let target_before = std::fs::read(&target).expect("target before staging");
        let action =
            waveform_restore_action_for_capacity_tests(backup.clone(), target.clone(), false);
        let owner = OperationJournalOwner::start_with_directory(directory.path().to_path_buf());
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
                .expect("available status"),
            OperationJournalStatus::Available { .. }
        ));

        let rejected = owner
            .prepare_and_stage_bounded_waveform_restore(
                intent(),
                Value::Null,
                HistoryFileIoDirection::Undo,
                Vec::new(),
            )
            .expect("queue invalid shape")
            .recv_timeout(Duration::from_secs(2))
            .expect("invalid-shape result");
        assert!(matches!(
            rejected,
            Err(JournalOperationError::RejectedBeforeIntent(
                RejectedBeforeIntent::InvalidShape
            ))
        ));

        let payload = serde_json::json!({
            "execution_id": 41,
            "transaction_id": 1,
            "direction": "undo",
            "through_target": 7,
        });
        let operation_id = match owner
            .prepare_and_stage_bounded_waveform_restore(
                intent(),
                payload.clone(),
                HistoryFileIoDirection::Undo,
                vec![action],
            )
            .expect("queue prepare and stage")
            .recv_timeout(Duration::from_secs(2))
            .expect("prepare and stage result")
            .expect("prepare and stage")
        {
            FilesystemStageOutcome::PlatformQualificationRequired { operation_id, .. } => {
                operation_id
            }
            other => panic!("expected platform-qualification outcome, got {other:?}"),
        };
        assert_eq!(
            std::fs::read(&target).expect("target after staging"),
            target_before
        );

        let staging = files
            .path()
            .join(format!(".wavecrate-restore-{operation_id}.stage"));
        drop(owner);
        let deadline = Instant::now() + Duration::from_secs(2);
        let reopened = loop {
            match OperationJournalCoordinator::open(directory.path().to_path_buf()) {
                Ok(coordinator) => break coordinator,
                Err(error) if Instant::now() < deadline => {
                    let _ = error;
                    std::thread::sleep(Duration::from_millis(10));
                }
                Err(error) => panic!("owner did not release journal lock: {error}"),
            }
        };
        let record = reopened.record(operation_id).expect("staged record");
        assert_eq!(record.payload, payload);
        assert_eq!(record.phase, OperationPhase::FilesystemStaged);
        assert_eq!(record.disposition, OperationDisposition::RetryPending);
        assert!(matches!(
            record.staged.as_ref().map(|staged| &staged.participant),
            Some(FilesystemStagedParticipant::CopyValidated { .. })
        ));
        assert_eq!(
            std::fs::read(&staging).expect("staging bytes"),
            std::fs::read(backup).unwrap()
        );
        assert_eq!(
            std::fs::read(target).expect("target after reopen"),
            target_before
        );
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
            OperationJournalStatus::Available { summary, .. } => {
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
