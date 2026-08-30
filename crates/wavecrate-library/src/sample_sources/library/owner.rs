//! Profile-owned asynchronous ownership for the global library database.

use std::path::PathBuf;
use std::sync::mpsc::{self, Receiver, SyncSender, TrySendError};
use std::sync::{Arc, Mutex};
use std::thread::{self, JoinHandle};

use thiserror::Error;

use super::connection::LibraryDatabase;
use super::error::LibraryError;
use crate::app_dirs::{ProfileOwnershipError, WritableProfileGuard};
use crate::sample_sources::SampleSource;

/// Maximum number of global-library commands waiting for the owner thread.
pub const GLOBAL_LIBRARY_WRITER_COMMAND_CAPACITY: usize = 32;

const OWNER_POLL_INTERVAL: std::time::Duration = std::time::Duration::from_millis(25);

/// An immutable, owned snapshot of the configured source registry.
#[derive(Clone, Debug)]
pub struct SourceRegistrySnapshot {
    sources: Arc<[SampleSource]>,
}

impl SourceRegistrySnapshot {
    /// Own a source registry snapshot without retaining a caller-owned mutable collection.
    pub fn new(sources: Vec<SampleSource>) -> Self {
        Self {
            sources: Arc::from(sources.into_boxed_slice()),
        }
    }

    /// Borrow the sources in this immutable snapshot.
    pub fn as_slice(&self) -> &[SampleSource] {
        &self.sources
    }

    /// Return the number of sources in this snapshot.
    pub fn len(&self) -> usize {
        self.sources.len()
    }

    /// Return whether this snapshot contains no sources.
    pub fn is_empty(&self) -> bool {
        self.sources.is_empty()
    }

    pub(super) fn from_sources(sources: Vec<SampleSource>) -> Self {
        Self::new(sources)
    }
}

impl From<Vec<SampleSource>> for SourceRegistrySnapshot {
    fn from(sources: Vec<SampleSource>) -> Self {
        Self::new(sources)
    }
}

/// Lifecycle state visible at the global-library writer boundary.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum GlobalLibraryWriterStatus {
    /// The owner thread has not completed profile-owned database initialization.
    Initializing,
    /// The owner has one initialized writable library database connection.
    Available,
    /// Initialization failed and this participant will not admit commands.
    Unavailable {
        /// Reason that the participant is unavailable.
        reason: GlobalLibraryWriterUnavailable,
    },
    /// Shutdown has stopped new command admission and is draining accepted commands.
    Closing,
    /// The writer has closed its library database connection and worker thread.
    Closed,
}

/// Fail-closed reasons retained by the global-library owner.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum GlobalLibraryWriterUnavailable {
    /// The acquired profile root or lock entry no longer names the owned object.
    ProfileOwnershipChanged {
        /// Profile root whose ownership boundary changed.
        path: PathBuf,
        /// Stable profile-boundary failure description.
        reason: String,
    },
    /// The library database could not be initialized or became unusable.
    DatabaseUnavailable {
        /// Stable database failure description.
        reason: String,
    },
}

impl std::fmt::Display for GlobalLibraryWriterUnavailable {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::ProfileOwnershipChanged { path, reason } => write!(
                formatter,
                "profile ownership changed at {}: {reason}",
                path.display()
            ),
            Self::DatabaseUnavailable { reason } => {
                write!(formatter, "global library database unavailable: {reason}")
            }
        }
    }
}

/// Nonblocking command-admission result for the global-library owner.
#[derive(Clone, Debug, Eq, PartialEq, Error)]
pub enum GlobalLibraryWriterQueueError {
    /// The owner is still applying schema and migrations.
    #[error("global library writer is initializing")]
    Initializing,
    /// The fixed-capacity owner queue is full.
    #[error("global library writer queue is full")]
    Full,
    /// Initialization failed or the owner is otherwise unavailable.
    #[error("global library writer is unavailable: {0}")]
    Unavailable(GlobalLibraryWriterUnavailable),
    /// The owner is closing or has already closed.
    #[error("global library writer is closed")]
    Closed,
}

/// Failure to start a profile-owned global-library worker.
#[derive(Debug, Error)]
pub enum GlobalLibraryWriterStartError {
    /// The supplied profile capability was no longer valid before handoff.
    #[error(transparent)]
    ProfileOwnership(#[from] ProfileOwnershipError),
    /// The operating system rejected creation of the owner thread.
    #[error("failed to start global library writer thread: {0}")]
    ThreadSpawn(#[source] std::io::Error),
}

enum WriterCommand {
    LoadSourceRegistry {
        result: SyncSender<Result<SourceRegistrySnapshot, LibraryError>>,
    },
    ReplaceSourceRegistry {
        snapshot: SourceRegistrySnapshot,
        result: SyncSender<Result<(), LibraryError>>,
    },
}

enum WriterReply {
    Load {
        result: SyncSender<Result<SourceRegistrySnapshot, LibraryError>>,
        value: Result<SourceRegistrySnapshot, LibraryError>,
    },
    Replace {
        result: SyncSender<Result<(), LibraryError>>,
        value: Result<(), LibraryError>,
    },
}

impl WriterReply {
    fn send(self) {
        match self {
            Self::Load { result, value } => {
                let _ = result.send(value);
            }
            Self::Replace { result, value } => {
                let _ = result.send(value);
            }
        }
    }

    fn send_error(self, error: LibraryError) {
        match self {
            Self::Load { result, .. } => {
                let _ = result.send(Err(error));
            }
            Self::Replace { result, .. } => {
                let _ = result.send(Err(error));
            }
        }
    }
}

struct OwnerHooks {
    initialization_gate: Option<Arc<std::sync::Barrier>>,
    #[cfg(test)]
    binding_open_gate: Option<(Arc<std::sync::Barrier>, Arc<std::sync::Barrier>)>,
    #[cfg(test)]
    post_command_gate: Option<(Arc<std::sync::Barrier>, Arc<std::sync::Barrier>)>,
}

/// Profile-owned asynchronous writer for `library.db`.
///
/// The handle contains only typed command and lifecycle state. The writable SQLite connection
/// remains on one owner thread for the handle's lifetime and is never exposed to callers.
pub struct GlobalLibraryWriter {
    commands: Mutex<Option<SyncSender<WriterCommand>>>,
    admission_gate: Arc<Mutex<()>>,
    lifecycle: Arc<std::sync::atomic::AtomicU8>,
    unavailable: Arc<Mutex<Option<GlobalLibraryWriterUnavailable>>>,
    worker: Option<JoinHandle<()>>,
    #[cfg(test)]
    blocked_receiver: Option<Receiver<WriterCommand>>,
}

impl GlobalLibraryWriter {
    /// Start an owner using a clone of an already-acquired profile capability.
    ///
    /// Cloning the capability duplicates its retained descriptors without acquiring a second
    /// profile lock. The supplied guard remains owned by its caller until this writer is shut
    /// down, which lets a profile owner coordinate release order explicitly.
    pub fn start(
        profile_guard: &WritableProfileGuard,
    ) -> Result<Self, GlobalLibraryWriterStartError> {
        Self::start_inner(profile_guard, None, None, None)
    }

    fn start_inner(
        profile_guard: &WritableProfileGuard,
        initialization_gate: Option<Arc<std::sync::Barrier>>,
        binding_open_gate: Option<(Arc<std::sync::Barrier>, Arc<std::sync::Barrier>)>,
        post_command_gate: Option<(Arc<std::sync::Barrier>, Arc<std::sync::Barrier>)>,
    ) -> Result<Self, GlobalLibraryWriterStartError> {
        let worker_profile_guard = profile_guard.try_clone()?;
        let (commands, command_rx) = mpsc::sync_channel(GLOBAL_LIBRARY_WRITER_COMMAND_CAPACITY);
        let lifecycle = Arc::new(std::sync::atomic::AtomicU8::new(
            WriterLifecycle::Initializing as u8,
        ));
        let worker_lifecycle = Arc::clone(&lifecycle);
        let unavailable = Arc::new(Mutex::new(None));
        let worker_unavailable = Arc::clone(&unavailable);
        let admission_gate = Arc::new(Mutex::new(()));
        let worker_admission_gate = Arc::clone(&admission_gate);
        #[cfg(not(test))]
        let _ = (binding_open_gate, post_command_gate);
        let worker = thread::Builder::new()
            .name(String::from("wavecrate-global-library-writer"))
            .spawn(move || {
                run_owner(
                    worker_profile_guard,
                    command_rx,
                    worker_lifecycle,
                    worker_unavailable,
                    OwnerHooks {
                        initialization_gate,
                        #[cfg(test)]
                        binding_open_gate,
                        #[cfg(test)]
                        post_command_gate,
                    },
                    worker_admission_gate,
                );
            })
            .map_err(GlobalLibraryWriterStartError::ThreadSpawn)?;

        Ok(Self {
            commands: Mutex::new(Some(commands)),
            admission_gate,
            lifecycle,
            unavailable,
            worker: Some(worker),
            #[cfg(test)]
            blocked_receiver: None,
        })
    }

    #[cfg(test)]
    fn start_with_initialization_gate_for_test(
        profile_guard: &WritableProfileGuard,
    ) -> (Self, Arc<std::sync::Barrier>) {
        let gate = Arc::new(std::sync::Barrier::new(2));
        let writer = Self::start_inner(profile_guard, Some(Arc::clone(&gate)), None, None)
            .expect("writer start");
        (writer, gate)
    }

    #[cfg(test)]
    fn start_with_binding_open_gate_for_test(
        profile_guard: &WritableProfileGuard,
    ) -> (Self, Arc<std::sync::Barrier>, Arc<std::sync::Barrier>) {
        let ready = Arc::new(std::sync::Barrier::new(2));
        let release = Arc::new(std::sync::Barrier::new(2));
        let writer = Self::start_inner(
            profile_guard,
            None,
            Some((Arc::clone(&ready), Arc::clone(&release))),
            None,
        )
        .expect("writer start");
        (writer, ready, release)
    }

    #[cfg(test)]
    fn start_with_post_command_gate_for_test(
        profile_guard: &WritableProfileGuard,
    ) -> (Self, Arc<std::sync::Barrier>, Arc<std::sync::Barrier>) {
        let ready = Arc::new(std::sync::Barrier::new(2));
        let release = Arc::new(std::sync::Barrier::new(2));
        let writer = Self::start_inner(
            profile_guard,
            None,
            None,
            Some((Arc::clone(&ready), Arc::clone(&release))),
        )
        .expect("writer start");
        (writer, ready, release)
    }

    #[cfg(test)]
    fn disabled() -> Self {
        let (commands, blocked_receiver) =
            mpsc::sync_channel(GLOBAL_LIBRARY_WRITER_COMMAND_CAPACITY);
        Self {
            commands: Mutex::new(Some(commands)),
            admission_gate: Arc::new(Mutex::new(())),
            lifecycle: Arc::new(std::sync::atomic::AtomicU8::new(
                WriterLifecycle::Available as u8,
            )),
            unavailable: Arc::new(Mutex::new(None)),
            worker: None,
            blocked_receiver: Some(blocked_receiver),
        }
    }

    /// Return the current owner lifecycle without waiting for initialization or I/O.
    pub fn status(&self) -> GlobalLibraryWriterStatus {
        match WriterLifecycle::from_u8(self.lifecycle.load(std::sync::atomic::Ordering::Acquire)) {
            WriterLifecycle::Initializing => GlobalLibraryWriterStatus::Initializing,
            WriterLifecycle::Available => GlobalLibraryWriterStatus::Available,
            WriterLifecycle::Unavailable => GlobalLibraryWriterStatus::Unavailable {
                reason: self
                    .unavailable
                    .lock()
                    .unwrap_or_else(|poisoned| poisoned.into_inner())
                    .clone()
                    .unwrap_or_else(|| GlobalLibraryWriterUnavailable::DatabaseUnavailable {
                        reason: String::from("global library writer is unavailable"),
                    }),
            },
            WriterLifecycle::Closing => GlobalLibraryWriterStatus::Closing,
            WriterLifecycle::Closed => GlobalLibraryWriterStatus::Closed,
        }
    }

    /// Return the retained reason for an initialization or profile-boundary failure, if any.
    pub fn unavailable_reason(&self) -> Option<GlobalLibraryWriterUnavailable> {
        self.unavailable
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .clone()
    }

    /// Queue a load of the immutable source-registry snapshot.
    pub fn load_source_registry_snapshot(
        &self,
    ) -> Result<Receiver<Result<SourceRegistrySnapshot, LibraryError>>, GlobalLibraryWriterQueueError>
    {
        self.ensure_available()?;
        let (result_tx, result_rx) = mpsc::sync_channel(1);
        self.send(WriterCommand::LoadSourceRegistry { result: result_tx })?;
        Ok(result_rx)
    }

    /// Queue an idempotent replacement of the source-registry snapshot.
    pub fn replace_source_registry(
        &self,
        snapshot: SourceRegistrySnapshot,
    ) -> Result<Receiver<Result<(), LibraryError>>, GlobalLibraryWriterQueueError> {
        self.ensure_available()?;
        let (result_tx, result_rx) = mpsc::sync_channel(1);
        self.send(WriterCommand::ReplaceSourceRegistry {
            snapshot,
            result: result_tx,
        })?;
        Ok(result_rx)
    }

    /// Stop new admission, drain accepted commands, close `library.db`, and join the owner.
    pub fn shutdown(&mut self) -> bool {
        let previous = self.lifecycle.swap(
            WriterLifecycle::Closing as u8,
            std::sync::atomic::Ordering::AcqRel,
        );
        let sender = {
            let _admission = self
                .admission_gate
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner());
            self.commands
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner())
                .take()
        };
        drop(sender);
        #[cfg(test)]
        self.blocked_receiver.take();
        if let Some(worker) = self.worker.take() {
            let _ = worker.join();
        }
        self.lifecycle.store(
            WriterLifecycle::Closed as u8,
            std::sync::atomic::Ordering::Release,
        );
        WriterLifecycle::from_u8(previous) != WriterLifecycle::Closed
    }

    fn ensure_available(&self) -> Result<(), GlobalLibraryWriterQueueError> {
        match WriterLifecycle::from_u8(self.lifecycle.load(std::sync::atomic::Ordering::Acquire)) {
            WriterLifecycle::Initializing => Err(GlobalLibraryWriterQueueError::Initializing),
            WriterLifecycle::Available => Ok(()),
            WriterLifecycle::Unavailable => Err(GlobalLibraryWriterQueueError::Unavailable(
                self.unavailable
                    .lock()
                    .unwrap_or_else(|poisoned| poisoned.into_inner())
                    .clone()
                    .unwrap_or_else(|| GlobalLibraryWriterUnavailable::DatabaseUnavailable {
                        reason: String::from("global library writer is unavailable"),
                    }),
            )),
            WriterLifecycle::Closing | WriterLifecycle::Closed => {
                Err(GlobalLibraryWriterQueueError::Closed)
            }
        }
    }

    fn send(&self, command: WriterCommand) -> Result<(), GlobalLibraryWriterQueueError> {
        let _admission = self
            .admission_gate
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        self.ensure_available()?;
        let sender = self
            .commands
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .as_ref()
            .cloned()
            .ok_or(GlobalLibraryWriterQueueError::Closed)?;
        sender.try_send(command).map_err(|error| match error {
            TrySendError::Full(_) => GlobalLibraryWriterQueueError::Full,
            TrySendError::Disconnected(_) => GlobalLibraryWriterQueueError::Closed,
        })
    }
}

impl Drop for GlobalLibraryWriter {
    fn drop(&mut self) {
        let _ = self.shutdown();
    }
}

#[repr(u8)]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum WriterLifecycle {
    Initializing = 0,
    Available = 1,
    Unavailable = 2,
    Closing = 3,
    Closed = 4,
}

impl WriterLifecycle {
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

fn run_owner(
    profile_guard: WritableProfileGuard,
    command_rx: Receiver<WriterCommand>,
    lifecycle: Arc<std::sync::atomic::AtomicU8>,
    unavailable: Arc<Mutex<Option<GlobalLibraryWriterUnavailable>>>,
    hooks: OwnerHooks,
    admission_gate: Arc<Mutex<()>>,
) {
    if let Some(initialization_gate) = hooks.initialization_gate {
        initialization_gate.wait();
    }
    #[cfg(test)]
    let open_result =
        LibraryDatabase::open_for_profile_guard(&profile_guard, hooks.binding_open_gate);
    #[cfg(not(test))]
    let open_result = { LibraryDatabase::open_for_profile_guard(&profile_guard) };
    let mut database = match open_result {
        Ok(database) => database,
        Err(error) => {
            let profile_error = profile_guard.validate_current().err();
            let reason = profile_error
                .as_ref()
                .map(unavailable_from_profile_error)
                .unwrap_or_else(|| unavailable_from_library_error(&error));
            *unavailable
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner()) = Some(reason);
            lifecycle.store(
                if profile_error.is_some() || matches!(error, LibraryError::ProfileOwnership(_)) {
                    WriterLifecycle::Closed as u8
                } else {
                    WriterLifecycle::Unavailable as u8
                },
                std::sync::atomic::Ordering::Release,
            );
            return;
        }
    };

    if let Err(error) = database.validate_profile_guard(&profile_guard) {
        close_after_profile_change(
            &command_rx,
            &lifecycle,
            &unavailable,
            &admission_gate,
            error,
        );
        return;
    }
    lifecycle.store(
        WriterLifecycle::Available as u8,
        std::sync::atomic::Ordering::Release,
    );

    loop {
        if let Err(error) = database.validate_profile_guard(&profile_guard) {
            close_after_profile_change(
                &command_rx,
                &lifecycle,
                &unavailable,
                &admission_gate,
                error,
            );
            break;
        }
        let command = match command_rx.recv_timeout(OWNER_POLL_INTERVAL) {
            Ok(command) => command,
            Err(mpsc::RecvTimeoutError::Timeout) => continue,
            Err(mpsc::RecvTimeoutError::Disconnected) => break,
        };
        if let Err(error) = database.validate_profile_guard(&profile_guard) {
            reject_after_profile_change(command, &error);
            close_after_profile_change(
                &command_rx,
                &lifecycle,
                &unavailable,
                &admission_gate,
                error,
            );
            break;
        }

        let reply = execute_command(command, &mut database);

        #[cfg(test)]
        if let Some((ready, release)) = hooks.post_command_gate.as_ref() {
            ready.wait();
            release.wait();
        }

        if let Err(error) = database.validate_profile_guard(&profile_guard) {
            reply.send_error(profile_change_library_error(&error, true));
            close_after_profile_change(
                &command_rx,
                &lifecycle,
                &unavailable,
                &admission_gate,
                error,
            );
            break;
        }
        reply.send();
    }

    drop(database);
    lifecycle.store(
        WriterLifecycle::Closed as u8,
        std::sync::atomic::Ordering::Release,
    );
}

fn execute_command(command: WriterCommand, database: &mut LibraryDatabase) -> WriterReply {
    match command {
        WriterCommand::LoadSourceRegistry { result } => WriterReply::Load {
            value: database.load_source_registry(),
            result,
        },
        WriterCommand::ReplaceSourceRegistry { snapshot, result } => WriterReply::Replace {
            value: database.replace_source_registry(&snapshot),
            result,
        },
    }
}

fn close_after_profile_change(
    command_rx: &Receiver<WriterCommand>,
    lifecycle: &Arc<std::sync::atomic::AtomicU8>,
    unavailable: &Arc<Mutex<Option<GlobalLibraryWriterUnavailable>>>,
    admission_gate: &Arc<Mutex<()>>,
    error: ProfileOwnershipError,
) {
    let _admission = admission_gate
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    let reason = unavailable_from_profile_error(&error);
    *unavailable
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner()) = Some(reason.clone());
    lifecycle.store(
        WriterLifecycle::Closed as u8,
        std::sync::atomic::Ordering::Release,
    );
    reject_remaining_profile_change(command_rx, &error);
}

fn reject_after_profile_change(command: WriterCommand, error: &ProfileOwnershipError) {
    send_command_error(command, profile_change_library_error(error, false));
}

fn reject_remaining_profile_change(
    command_rx: &Receiver<WriterCommand>,
    error: &ProfileOwnershipError,
) {
    while let Ok(command) = command_rx.try_recv() {
        reject_after_profile_change(command, error);
    }
}

fn send_command_error(command: WriterCommand, error: LibraryError) {
    match command {
        WriterCommand::LoadSourceRegistry { result } => {
            let _ = result.send(Err(error));
        }
        WriterCommand::ReplaceSourceRegistry { result, .. } => {
            let _ = result.send(Err(error));
        }
    }
}

fn profile_change_library_error(
    error: &ProfileOwnershipError,
    completion_not_confirmable: bool,
) -> LibraryError {
    let reason = if completion_not_confirmable {
        format!("completion not confirmable: {error}")
    } else {
        error.to_string()
    };
    LibraryError::ProfileOwnershipChanged {
        path: profile_error_path(error),
        reason,
    }
}

fn profile_error_path(error: &ProfileOwnershipError) -> PathBuf {
    match error {
        ProfileOwnershipError::ProfileRootReplaced { path }
        | ProfileOwnershipError::ProfileOwnerLockReplaced { path }
        | ProfileOwnershipError::ProfileOwnedByAnotherProcess { path }
        | ProfileOwnershipError::Io { path, .. }
        | ProfileOwnershipError::NotRegularFile { path }
        | ProfileOwnershipError::IdentityUnavailable { path }
        | ProfileOwnershipError::Unsupported { path } => path.clone(),
        ProfileOwnershipError::AppDirectory(_) => PathBuf::new(),
    }
}

fn unavailable_from_profile_error(error: &ProfileOwnershipError) -> GlobalLibraryWriterUnavailable {
    GlobalLibraryWriterUnavailable::ProfileOwnershipChanged {
        path: profile_error_path(error),
        reason: error.to_string(),
    }
}

fn unavailable_from_library_error(error: &LibraryError) -> GlobalLibraryWriterUnavailable {
    match error {
        LibraryError::ProfileOwnership(error) => unavailable_from_profile_error(error),
        LibraryError::ProfileOwnershipChanged { path, reason } => {
            GlobalLibraryWriterUnavailable::ProfileOwnershipChanged {
                path: path.clone(),
                reason: reason.clone(),
            }
        }
        error => GlobalLibraryWriterUnavailable::DatabaseUnavailable {
            reason: error.to_string(),
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::app_dirs::{ConfigBaseGuard, PersistenceProfileGuard};
    use crate::sample_sources::{SourceId, SourceRole};
    use crate::test_runtime::TestRuntimeGuard;
    use rusqlite::Connection;
    use std::fs;
    use std::path::Path;
    use std::thread;
    use std::time::{Duration, Instant};
    use tempfile::tempdir;

    fn wait_until_available(writer: &GlobalLibraryWriter) {
        let deadline = Instant::now() + Duration::from_secs(2);
        loop {
            match writer.status() {
                GlobalLibraryWriterStatus::Available => return,
                GlobalLibraryWriterStatus::Unavailable { reason } => {
                    panic!("writer unavailable: {reason}")
                }
                status if Instant::now() >= deadline => panic!("writer status: {status:?}"),
                _ => thread::sleep(Duration::from_millis(5)),
            }
        }
    }

    fn wait_until_closed(writer: &GlobalLibraryWriter) {
        let deadline = Instant::now() + Duration::from_secs(2);
        while !matches!(writer.status(), GlobalLibraryWriterStatus::Closed) {
            assert!(Instant::now() < deadline, "writer did not close");
            thread::sleep(Duration::from_millis(5));
        }
    }

    fn source(root: &Path, id: &str) -> SampleSource {
        SampleSource {
            id: SourceId::from_string(id),
            root: root.to_path_buf(),
            role: SourceRole::Normal,
            metadata_storage: crate::sample_sources::SourceMetadataStorage::SourceFolder,
            primary_import_folder: crate::sample_sources::default_primary_import_folder(),
        }
    }

    fn start_writer(
        base: &tempfile::TempDir,
        name: &str,
    ) -> (
        ConfigBaseGuard,
        PersistenceProfileGuard,
        WritableProfileGuard,
        GlobalLibraryWriter,
    ) {
        let base_path = std::fs::canonicalize(base.path()).expect("canonical test base");
        let base_guard = ConfigBaseGuard::set(base_path);
        let profile_guard = PersistenceProfileGuard::named(name);
        let writable_guard = WritableProfileGuard::acquire_current().expect("profile ownership");
        let writer = GlobalLibraryWriter::start(&writable_guard).expect("writer start");
        (base_guard, profile_guard, writable_guard, writer)
    }

    #[test]
    fn queue_admission_distinguishes_initializing_and_full() {
        let temp = tempdir().expect("base");
        let _runtime = TestRuntimeGuard::acquire();
        let base_guard =
            ConfigBaseGuard::set(std::fs::canonicalize(temp.path()).expect("canonical test base"));
        let profile_guard = PersistenceProfileGuard::named("queue");
        let writable_guard = WritableProfileGuard::acquire_current().expect("profile ownership");
        let (writer, gate) =
            GlobalLibraryWriter::start_with_initialization_gate_for_test(&writable_guard);
        assert_eq!(writer.status(), GlobalLibraryWriterStatus::Initializing);
        assert!(matches!(
            writer.load_source_registry_snapshot(),
            Err(GlobalLibraryWriterQueueError::Initializing)
        ));
        gate.wait();
        wait_until_available(&writer);
        drop(writer);
        drop(writable_guard);
        drop(profile_guard);
        drop(base_guard);
    }

    #[test]
    fn queue_admission_is_nonblocking_and_reports_full() {
        let writer = GlobalLibraryWriter::disabled();
        for _ in 0..GLOBAL_LIBRARY_WRITER_COMMAND_CAPACITY {
            writer
                .load_source_registry_snapshot()
                .expect("bounded queue admission");
        }
        assert!(matches!(
            writer.load_source_registry_snapshot(),
            Err(GlobalLibraryWriterQueueError::Full)
        ));
    }

    #[test]
    fn source_registry_snapshot_owns_an_immutable_copy() {
        let temp = tempdir().expect("base");
        let mut original = source(&temp.path().join("original"), "source");
        let snapshot = SourceRegistrySnapshot::new(vec![original.clone()]);
        original.root = temp.path().join("mutated");

        assert_eq!(snapshot.len(), 1);
        assert_eq!(snapshot.as_slice()[0].root, temp.path().join("original"));
    }

    #[test]
    fn writer_keeps_shared_profile_ownership_after_original_guard_is_dropped() {
        let temp = tempdir().expect("base");
        let _runtime = TestRuntimeGuard::acquire();
        let base_path = std::fs::canonicalize(temp.path()).expect("canonical test base");
        let base_guard = ConfigBaseGuard::set(base_path);
        let profile_guard = PersistenceProfileGuard::named("shared-ownership");
        let writable_guard = WritableProfileGuard::acquire_current().expect("profile ownership");
        let mut writer = GlobalLibraryWriter::start(&writable_guard).expect("writer start");
        wait_until_available(&writer);
        drop(writable_guard);

        assert!(matches!(
            WritableProfileGuard::acquire_current(),
            Err(crate::app_dirs::ProfileOwnershipError::ProfileOwnedByAnotherProcess { .. })
        ));
        assert!(writer.shutdown());
        let replacement = WritableProfileGuard::acquire_current().expect("released ownership");
        drop(replacement);
        drop(profile_guard);
        drop(base_guard);
    }

    #[test]
    fn one_owner_connection_serves_repeated_commands_and_shutdown_drains() {
        let temp = tempdir().expect("base");
        let _runtime = TestRuntimeGuard::acquire();
        let (base_guard, profile_guard, writable_guard, mut writer) =
            start_writer(&temp, "one-connection");
        let _ = (&base_guard, &profile_guard);
        wait_until_available(&writer);
        let root = temp.path().join("source");
        let snapshot = SourceRegistrySnapshot::new(vec![source(&root, "source-1")]);
        for _ in 0..3 {
            writer
                .replace_source_registry(snapshot.clone())
                .expect("replace admission")
                .recv_timeout(Duration::from_secs(2))
                .expect("replace result")
                .expect("replace success");
        }
        let loaded = writer
            .load_source_registry_snapshot()
            .expect("load admission")
            .recv_timeout(Duration::from_secs(2))
            .expect("load result")
            .expect("load success");
        assert_eq!(loaded.len(), 1);
        assert_eq!(loaded.as_slice()[0].id.as_str(), "source-1");
        assert_eq!(
            super::super::connection::test_open_count(
                &writable_guard
                    .profile_root()
                    .join(super::super::LIBRARY_DB_FILE_NAME)
            ),
            1
        );
        assert!(writer.shutdown());
        assert!(matches!(writer.status(), GlobalLibraryWriterStatus::Closed));
    }

    #[test]
    fn failed_replacement_rolls_back_active_sources() {
        let temp = tempdir().expect("base");
        let _runtime = TestRuntimeGuard::acquire();
        let base_path = std::fs::canonicalize(temp.path()).expect("canonical test base");
        let base_guard = ConfigBaseGuard::set(base_path);
        let profile_guard = PersistenceProfileGuard::named("rollback");
        let original = source(&temp.path().join("original"), "original");
        crate::sample_sources::library::save(&crate::sample_sources::library::LibraryState {
            sources: vec![original.clone()],
        })
        .expect("initial source");
        let connection = crate::sample_sources::library::open_connection().expect("fixture DB");
        connection
            .execute_batch(
                "CREATE TRIGGER fail_known_source_update
                 BEFORE UPDATE OF value ON metadata
                 WHEN OLD.key = 'known_sources_v1'
                 BEGIN SELECT RAISE(FAIL, 'rollback fixture'); END;",
            )
            .expect("rollback trigger");
        drop(connection);
        let writable_guard = WritableProfileGuard::acquire_current().expect("profile ownership");
        let mut writer = GlobalLibraryWriter::start(&writable_guard).expect("writer start");
        wait_until_available(&writer);
        let replacement = SourceRegistrySnapshot::new(vec![source(
            &temp.path().join("replacement"),
            "replacement",
        )]);
        assert!(matches!(
            writer
                .replace_source_registry(replacement)
                .expect("replace admission")
                .recv_timeout(Duration::from_secs(2))
                .expect("replace result"),
            Err(LibraryError::Sql(_))
        ));
        let loaded = writer
            .load_source_registry_snapshot()
            .expect("load admission")
            .recv_timeout(Duration::from_secs(2))
            .expect("load result")
            .expect("load success");
        assert_eq!(loaded.as_slice()[0].id.as_str(), original.id.as_str());
        assert!(writer.shutdown());
        drop(writable_guard);
        drop(profile_guard);
        drop(base_guard);
    }

    #[test]
    fn source_registry_and_known_sources_survive_writer_reopen() {
        let temp = tempdir().expect("base");
        let _runtime = TestRuntimeGuard::acquire();
        let base_path = std::fs::canonicalize(temp.path()).expect("canonical test base");
        let base_guard = ConfigBaseGuard::set(base_path);
        let profile_guard = PersistenceProfileGuard::named("reopen");
        let root = temp.path().join("persistent-source");
        let retained = source(&root, "persistent-id");
        let writable_guard = WritableProfileGuard::acquire_current().expect("profile ownership");
        let mut writer = GlobalLibraryWriter::start(&writable_guard).expect("writer start");
        wait_until_available(&writer);
        writer
            .replace_source_registry(SourceRegistrySnapshot::new(vec![retained.clone()]))
            .expect("initial replace admission")
            .recv_timeout(Duration::from_secs(2))
            .expect("initial replace result")
            .expect("initial replace");
        assert!(writer.shutdown());
        drop(writable_guard);

        let writable_guard = WritableProfileGuard::acquire_current().expect("reopen ownership");
        let mut writer = GlobalLibraryWriter::start(&writable_guard).expect("reopen writer");
        wait_until_available(&writer);
        let loaded = writer
            .load_source_registry_snapshot()
            .expect("reopen load admission")
            .recv_timeout(Duration::from_secs(2))
            .expect("reopen load result")
            .expect("reopen load");
        assert_eq!(loaded.as_slice()[0].id.as_str(), retained.id.as_str());
        writer
            .replace_source_registry(SourceRegistrySnapshot::new(Vec::new()))
            .expect("remove admission")
            .recv_timeout(Duration::from_secs(2))
            .expect("remove result")
            .expect("remove");
        assert!(writer.shutdown());
        drop(writable_guard);

        assert_eq!(
            crate::sample_sources::library::lookup_retained_source_for_root(&root)
                .expect("retained source lookup")
                .expect("retained source")
                .id
                .as_str(),
            retained.id.as_str()
        );
        drop(profile_guard);
        drop(base_guard);
    }

    #[test]
    fn corrupt_database_is_unavailable_without_repeated_open_attempts() {
        let temp = tempdir().expect("base");
        let _runtime = TestRuntimeGuard::acquire();
        let base_guard =
            ConfigBaseGuard::set(std::fs::canonicalize(temp.path()).expect("canonical test base"));
        let profile_guard = PersistenceProfileGuard::named("corrupt");
        let writable_guard = WritableProfileGuard::acquire_current().expect("profile ownership");
        let path = writable_guard
            .profile_root()
            .join(super::super::LIBRARY_DB_FILE_NAME);
        fs::write(&path, b"not sqlite").expect("corrupt fixture");
        let writer = GlobalLibraryWriter::start(&writable_guard).expect("writer start");
        let deadline = Instant::now() + Duration::from_secs(2);
        while !matches!(
            writer.status(),
            GlobalLibraryWriterStatus::Unavailable { .. }
        ) {
            assert!(
                Instant::now() < deadline,
                "writer did not become unavailable"
            );
            thread::sleep(Duration::from_millis(5));
        }
        assert!(matches!(
            writer.load_source_registry_snapshot(),
            Err(GlobalLibraryWriterQueueError::Unavailable(
                GlobalLibraryWriterUnavailable::DatabaseUnavailable { .. }
            ))
        ));
        assert_eq!(super::super::connection::test_open_count(&path), 1);
        drop(writer);
        drop(writable_guard);
        drop(profile_guard);
        drop(base_guard);
    }

    #[test]
    fn profile_owned_open_rejects_nonregular_sidecar_before_sqlite() {
        let temp = tempdir().expect("base");
        let _runtime = TestRuntimeGuard::acquire();
        let base_path = std::fs::canonicalize(temp.path()).expect("canonical test base");
        let base_guard = ConfigBaseGuard::set(base_path);
        let profile_guard = PersistenceProfileGuard::named("sidecar-entry");
        let writable_guard = WritableProfileGuard::acquire_current().expect("profile ownership");
        let root = writable_guard.profile_root().to_path_buf();
        fs::create_dir(root.join("library.db-wal")).expect("nonregular WAL fixture");
        let db_path = root.join(super::super::LIBRARY_DB_FILE_NAME);
        let opens_before = super::super::connection::test_open_count(&db_path);
        let writer = GlobalLibraryWriter::start(&writable_guard).expect("writer start");
        let deadline = Instant::now() + Duration::from_secs(2);
        while !matches!(
            writer.status(),
            GlobalLibraryWriterStatus::Unavailable { .. }
        ) {
            assert!(
                Instant::now() < deadline,
                "writer did not reject the nonregular sidecar"
            );
            thread::sleep(Duration::from_millis(5));
        }
        assert!(matches!(
            writer.status(),
            GlobalLibraryWriterStatus::Unavailable {
                reason: GlobalLibraryWriterUnavailable::DatabaseUnavailable { reason }
            } if reason.contains("library.db-wal")
        ));
        assert_eq!(
            super::super::connection::test_open_count(&db_path),
            opens_before
        );
        drop(writer);
        drop(writable_guard);
        drop(profile_guard);
        drop(base_guard);
    }

    #[cfg(unix)]
    #[test]
    fn profile_owned_open_rejects_database_symlink_without_following_it() {
        let temp = tempdir().expect("base");
        let _runtime = TestRuntimeGuard::acquire();
        let base_path = std::fs::canonicalize(temp.path()).expect("canonical test base");
        let base_guard = ConfigBaseGuard::set(base_path);
        let profile_guard = PersistenceProfileGuard::named("database-symlink");
        let writable_guard = WritableProfileGuard::acquire_current().expect("profile ownership");
        let root = writable_guard.profile_root().to_path_buf();
        let outside = temp.path().join("outside.db");
        fs::write(&outside, b"outside database").expect("outside database");
        std::os::unix::fs::symlink(&outside, root.join(super::super::LIBRARY_DB_FILE_NAME))
            .expect("database symlink fixture");
        let db_path = root.join(super::super::LIBRARY_DB_FILE_NAME);
        let opens_before = super::super::connection::test_open_count(&db_path);
        let writer = GlobalLibraryWriter::start(&writable_guard).expect("writer start");
        let deadline = Instant::now() + Duration::from_secs(2);
        while !matches!(
            writer.status(),
            GlobalLibraryWriterStatus::Unavailable { .. }
        ) {
            assert!(
                Instant::now() < deadline,
                "writer did not reject the database symlink"
            );
            thread::sleep(Duration::from_millis(5));
        }
        assert!(matches!(
            writer.status(),
            GlobalLibraryWriterStatus::Unavailable {
                reason: GlobalLibraryWriterUnavailable::DatabaseUnavailable { reason }
            } if reason.contains("library.db")
        ));
        assert_eq!(
            super::super::connection::test_open_count(&db_path),
            opens_before
        );
        drop(writer);
        drop(writable_guard);
        drop(profile_guard);
        drop(base_guard);
    }

    #[cfg(unix)]
    #[test]
    fn profile_root_replacement_during_initialization_does_not_redirect_database() {
        let temp = tempdir().expect("base");
        let _runtime = TestRuntimeGuard::acquire();
        let base_path = std::fs::canonicalize(temp.path()).expect("canonical test base");
        let base_guard = ConfigBaseGuard::set(base_path);
        let profile_guard = PersistenceProfileGuard::named("initialization-replacement");
        let writable_guard = WritableProfileGuard::acquire_current().expect("profile ownership");
        let root = writable_guard.profile_root().to_path_buf();
        let (mut writer, ready, release) =
            GlobalLibraryWriter::start_with_binding_open_gate_for_test(&writable_guard);

        ready.wait();
        let displaced = temp.path().join("initialization-replacement-old");
        fs::rename(&root, &displaced).expect("displace original profile root");
        fs::create_dir(&root).expect("replacement profile root");
        release.wait();

        wait_until_closed(&writer);
        assert!(!root.join(super::super::LIBRARY_DB_FILE_NAME).exists());
        assert!(displaced.join(super::super::LIBRARY_DB_FILE_NAME).is_file());
        assert!(matches!(
            writer.unavailable_reason(),
            Some(GlobalLibraryWriterUnavailable::ProfileOwnershipChanged { path, .. })
                if path == root
        ));
        assert!(!writer.shutdown());
        drop(writable_guard);
        drop(profile_guard);
        drop(base_guard);
    }

    #[cfg(unix)]
    #[test]
    fn profile_replacement_before_reply_returns_completion_not_confirmable() {
        let temp = tempdir().expect("base");
        let _runtime = TestRuntimeGuard::acquire();
        let base_path = std::fs::canonicalize(temp.path()).expect("canonical test base");
        let base_guard = ConfigBaseGuard::set(base_path);
        let profile_guard = PersistenceProfileGuard::named("inflight-replacement");
        let writable_guard = WritableProfileGuard::acquire_current().expect("profile ownership");
        let (mut writer, ready, release) =
            GlobalLibraryWriter::start_with_post_command_gate_for_test(&writable_guard);
        wait_until_available(&writer);
        let root = writable_guard.profile_root().to_path_buf();
        let result = writer
            .replace_source_registry(SourceRegistrySnapshot::new(Vec::new()))
            .expect("replace admission");

        ready.wait();
        let queued_result = writer
            .load_source_registry_snapshot()
            .expect("queued load admission");
        let displaced = temp.path().join("inflight-replacement-old");
        fs::rename(&root, &displaced).expect("displace original profile root");
        fs::create_dir(&root).expect("replacement profile root");
        release.wait();

        let result = result
            .recv_timeout(Duration::from_secs(2))
            .expect("in-flight command result");
        match result {
            Err(LibraryError::ProfileOwnershipChanged { path, reason }) => {
                assert_eq!(path, root);
                assert!(reason.contains("completion not confirmable"));
            }
            other => panic!("profile replacement returned {other:?}"),
        }
        assert!(matches!(
            queued_result
                .recv_timeout(Duration::from_secs(2))
                .expect("queued command result"),
            Err(LibraryError::ProfileOwnershipChanged { .. })
        ));
        wait_until_closed(&writer);
        assert!(matches!(
            writer.load_source_registry_snapshot(),
            Err(GlobalLibraryWriterQueueError::Closed)
        ));
        assert!(displaced.join(super::super::LIBRARY_DB_FILE_NAME).is_file());
        assert!(!writer.shutdown());
        drop(writable_guard);
        drop(profile_guard);
        drop(base_guard);
    }

    #[test]
    #[cfg(unix)]
    fn profile_replacement_closes_owner_and_rejects_later_commands() {
        let temp = tempdir().expect("base");
        let _runtime = TestRuntimeGuard::acquire();
        let (base_guard, profile_guard, writable_guard, mut writer) =
            start_writer(&temp, "replacement");
        let root = writable_guard.profile_root().to_path_buf();
        wait_until_available(&writer);
        let displaced = temp.path().join("displaced-profile");
        fs::rename(&root, &displaced).expect("displace profile");
        fs::create_dir(&root).expect("replacement profile");
        wait_until_closed(&writer);
        assert!(matches!(
            writer.load_source_registry_snapshot(),
            Err(GlobalLibraryWriterQueueError::Closed)
        ));
        assert!(matches!(
            writer.unavailable_reason(),
            Some(GlobalLibraryWriterUnavailable::ProfileOwnershipChanged { .. })
        ));
        let _ = writer.shutdown();
        drop(writable_guard);
        drop(profile_guard);
        drop(base_guard);
    }

    #[cfg(any(unix, windows))]
    #[test]
    fn profile_lock_replacement_closes_owner_and_rejects_later_commands() {
        let temp = tempdir().expect("base");
        let _runtime = TestRuntimeGuard::acquire();
        let (base_guard, profile_guard, writable_guard, mut writer) =
            start_writer(&temp, "lock-replacement");
        let root = writable_guard.profile_root().to_path_buf();
        wait_until_available(&writer);
        let lock_path = root.join("profile-owner.lock");
        let displaced = root.join("profile-owner.lock.old");
        fs::rename(&lock_path, &displaced).expect("displace profile lock");
        fs::write(&lock_path, b"replacement").expect("replacement profile lock");
        wait_until_closed(&writer);
        assert!(matches!(
            writer.load_source_registry_snapshot(),
            Err(GlobalLibraryWriterQueueError::Closed)
        ));
        assert!(matches!(
            writer.unavailable_reason(),
            Some(GlobalLibraryWriterUnavailable::ProfileOwnershipChanged { path, .. })
                if path == lock_path
        ));
        let _ = writer.shutdown();
        drop(writable_guard);
        drop(profile_guard);
        drop(base_guard);
    }

    #[cfg(windows)]
    #[test]
    fn profile_root_rename_is_blocked_by_retained_database_binding() {
        let temp = tempdir().expect("base");
        let _runtime = TestRuntimeGuard::acquire();
        let (base_guard, profile_guard, writable_guard, mut writer) =
            start_writer(&temp, "root-rename-barrier");
        let root = writable_guard.profile_root().to_path_buf();
        wait_until_available(&writer);
        let displaced = temp.path().join("root-rename-barrier-old");
        assert!(fs::rename(&root, &displaced).is_err());
        assert!(matches!(
            writer.status(),
            GlobalLibraryWriterStatus::Available
        ));
        assert!(writer.shutdown());
        drop(writable_guard);
        drop(profile_guard);
        drop(base_guard);
    }

    #[test]
    fn shutdown_closes_database_before_guard_can_be_released() {
        let temp = tempdir().expect("base");
        let _runtime = TestRuntimeGuard::acquire();
        let (base_guard, profile_guard, writable_guard, mut writer) =
            start_writer(&temp, "shutdown");
        wait_until_available(&writer);
        let snapshot = SourceRegistrySnapshot::new(vec![source(&temp.path().join("source"), "id")]);
        let result = writer
            .replace_source_registry(snapshot)
            .expect("replace admission");
        assert!(writer.shutdown());
        assert!(
            result
                .recv_timeout(Duration::from_secs(2))
                .expect("drained command")
                .is_ok()
        );
        assert!(matches!(writer.status(), GlobalLibraryWriterStatus::Closed));
        drop(writable_guard);
        drop(profile_guard);
        drop(base_guard);
        let _ = Connection::open(
            temp.path()
                .join(".wavecrate")
                .join("profiles")
                .join("shutdown")
                .join("library.db"),
        )
        .expect("closed database is reopenable");
    }
}
