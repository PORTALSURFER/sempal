use std::{
    collections::{BTreeMap, HashSet, VecDeque},
    path::PathBuf,
    sync::{
        Arc, Mutex,
        atomic::AtomicU64,
        mpsc::{Receiver, Sender},
    },
    time::Duration,
};

use radiant::{gui::frame as frame_ui, prelude as ui};
use wavecrate::audio::AudioPlayer;
use wavecrate::sample_sources::{
    HarvestTouchedPersistRequest, HarvestTouchedPersistResult, SampleSource, SourceId,
    persist_harvest_touched_if_current,
};

use crate::native_app::app::GuiSourceProcessingEventSink;
use crate::native_app::app::{
    ExtractedFilePlaybackType, FileMoveProgress, GuiMessage, NormalizationProgress,
    NormalizationQueueItem, PendingWaveformDestructiveEdit, SourceProcessingHealth,
    SourceProcessingProgress,
};
use crate::native_app::sample_library::harvest_tracking::{
    HarvestSelectionDerivationRequest, execute_harvest_selection_derivation,
};
use crate::native_app::sample_library::sample_ratings::{
    RatingPersistRequest, persist_rating_requests,
};
use crate::native_app::source_processing::{
    SourceProcessingRegistration, SourceProcessingSupervisor,
};
use crate::native_app::waveform::WaveformPreservedMarks;

pub(in crate::native_app) struct BackgroundTaskState {
    pub(in crate::native_app) waveform_recovery_root:
        Option<crate::native_app::transaction_history::operation_journal::RecoveryRootCapability>,
    pub(in crate::native_app) worker_sender: Sender<GuiMessage>,
    pub(in crate::native_app) worker_receiver: Option<Receiver<GuiMessage>>,
    pub(in crate::native_app) next_task_id: u64,
    pub(in crate::native_app) sample_load_validation_task: ui::LatestTask,
    pub(in crate::native_app) deferred_sample_load_task: ui::LatestTask,
    pub(in crate::native_app) sample_load_tasks: ui::ResourceTasks,
    pub(in crate::native_app) harvest_touched_persist: HarvestTouchedPersistOwner,
    pub(in crate::native_app) harvest_selection_derivation: HarvestSelectionDerivationOwner,
    pub(in crate::native_app) rating_persist: RatingPersistOwner,
    pub(in crate::native_app) operation_journal: super::OperationJournalOwner,
    pub(in crate::native_app) active_sample_load_key: Option<ui::ResourceKey>,
    pub(in crate::native_app) sample_load_cancel: Option<ui::CancellationToken>,
    pub(in crate::native_app) settled_sample_promotion_task: ui::LatestTask,
    pub(in crate::native_app) preview_audition_task: ui::LatestTask,
    pub(in crate::native_app) preview_audition_warm_task: ui::LatestTask,
    pub(in crate::native_app) starmap_audition_advance_task: ui::LatestTask,
    pub(in crate::native_app) starmap_audition_promotion_task: ui::LatestTask,
    pub(in crate::native_app) audio_options_refresh_task: ui::LatestTask,
    pub(in crate::native_app) audio_options_refresh_cancel: Option<ui::CancellationToken>,
    pub(in crate::native_app) audio_output_persist_task: ui::LatestTask,
    pub(in crate::native_app) audio_output_persist_generation: Arc<AtomicU64>,
    pub(in crate::native_app) audio_output_persist_lock: Arc<Mutex<()>>,
    pub(in crate::native_app) audio_open: AudioOpenTaskOwner,
    pub(in crate::native_app) folder_tree_refresh_task: ui::LatestTask,
    pub(in crate::native_app) folder_verify_task: ui::LatestTask,
    pub(in crate::native_app) release_update_check_task: ui::LatestTask,
    pub(in crate::native_app) global_storage_usage_task: ui::LatestTask,
    pub(in crate::native_app) waveform_destructive_edit_task: ui::LatestTask,
    pub(in crate::native_app) waveform_destructive_edit_context:
        Option<WaveformDestructiveEditUiContext>,
    pub(in crate::native_app) history_file_io_gate: Arc<Mutex<()>>,
    pub(in crate::native_app) normalization_progress: Option<NormalizationProgress>,
    pub(in crate::native_app) normalization_active_paths: HashSet<PathBuf>,
    pub(in crate::native_app) normalization_queue: VecDeque<NormalizationQueueItem>,
    pub(in crate::native_app) file_move_progress: Option<FileMoveProgress>,
    pub(in crate::native_app) source_processing_progress: Option<SourceProcessingProgress>,
    pub(in crate::native_app) source_processing_health: BTreeMap<String, SourceProcessingHealth>,
    pub(in crate::native_app) source_lifecycle_generations: BTreeMap<String, u64>,
    pub(in crate::native_app) progress_tick: f32,
    pub(in crate::native_app) frame_cadence: frame_ui::FrameCadenceMonitor,
    pub(in crate::native_app) source_processing: SourceProcessingSupervisor,
}

#[cfg(test)]
mod rating_persist_owner_tests {
    use super::*;
    use wavecrate::sample_sources::SourceDatabase;

    fn owner() -> RatingPersistOwner {
        RatingPersistOwner::new_with_lifecycle_generations(BTreeMap::from([(
            String::from("one"),
            7,
        )]))
    }

    fn request(source: &str, path: &str, rating: i8) -> RatingPersistRequest {
        RatingPersistRequest {
            source_id: source.to_owned(),
            lifecycle_generation: Some(7),
            root: PathBuf::from(format!("/tmp/{source}")),
            database_root: PathBuf::from(format!("/tmp/{source}/.db")),
            relative_path: PathBuf::from(path),
            absolute_path: PathBuf::from(format!("/tmp/{source}/{path}")),
            rating: wavecrate::sample_sources::Rating::new(rating),
            locked: false,
        }
    }

    #[test]
    fn rapid_same_path_enqueue_keeps_latest_revision() {
        let mut owner = owner();
        owner.enqueue(request("one", "kick.wav", 1));
        owner.enqueue(request("one", "kick.wav", 2));
        let work = owner.queue.lock().expect("queue lock").claim_all();
        assert_eq!(work.len(), 1);
        assert_eq!(work[0].revision, 2);
        assert_eq!(work[0].request.rating.val(), 2);
    }

    #[test]
    fn replacement_makes_inflight_completion_stale() {
        let mut owner = owner();
        owner.enqueue(request("one", "kick.wav", 1));
        let first = owner.queue.lock().expect("queue lock").claim_all();
        owner.enqueue(request("one", "kick.wav", 2));
        let key = first[0].key.clone();
        let queue = owner.queue.lock().expect("queue lock");
        assert!(!queue.entries.get(&key).is_some_and(|entry| {
            entry.revision == first[0].revision && entry.state == RatingPersistEntryState::InFlight
        }));
        drop(queue);
        assert_eq!(owner.queue.lock().expect("queue lock").claim_all().len(), 1);
    }

    #[test]
    fn failed_completion_does_not_erase_newer_projection() {
        let mut owner = owner();
        owner.enqueue(request("one", "kick.wav", 1));
        let first = owner.queue.lock().expect("queue lock").claim_all();
        owner.enqueue(request("one", "kick.wav", 2));
        let item = RatingPersistBatchItem {
            key: first[0].key.clone(),
            revision: first[0].revision,
            absolute_path: first[0].request.absolute_path.clone(),
            result: Some(Err(String::from("disk"))),
        };
        owner
            .queue
            .lock()
            .expect("queue lock")
            .entries
            .get_mut(&item.key)
            .unwrap()
            .state = RatingPersistEntryState::Pending;
        assert_eq!(
            owner.queue.lock().expect("queue lock").entries[&item.key]
                .request
                .rating
                .val(),
            2
        );
    }

    #[test]
    fn same_source_rekey_after_claim_fences_old_completion() {
        let mut owner = owner();
        owner.enqueue(request("one", "old/kick.wav", 1));
        let work = owner.queue.lock().expect("queue lock").claim_all();
        let old_revision = work[0].revision;
        owner.defer_auto_trash(
            "one",
            PathBuf::from("old/kick.wav").as_path(),
            old_revision,
            PathBuf::from("/tmp/one/old/kick.wav"),
        );
        owner.rekey_prefix(
            "one",
            PathBuf::from("old").as_path(),
            PathBuf::from("new").as_path(),
            false,
        );
        let queue = owner.queue.lock().expect("queue lock");
        let old_key = RatingPersistKey {
            source_id: String::from("one"),
            relative_path: PathBuf::from("old/kick.wav"),
        };
        let new_key = RatingPersistKey {
            source_id: String::from("one"),
            relative_path: PathBuf::from("new/kick.wav"),
        };
        assert!(!queue.entries.contains_key(&old_key));
        assert!(queue.entries[&new_key].revision > old_revision);
        assert_eq!(
            queue.entries[&new_key].state,
            RatingPersistEntryState::Pending
        );
        assert_eq!(
            queue.deferred_auto_trash[&new_key].1,
            PathBuf::from("/tmp/one/new/kick.wav")
        );
    }

    #[test]
    fn cross_source_rekey_after_claim_fences_old_completion() {
        let mut owner = owner();
        owner
            .queue
            .lock()
            .expect("queue lock")
            .lifecycle_generations
            .insert(String::from("two"), 9);
        owner.enqueue(request("one", "kick.wav", 1));
        let work = owner.queue.lock().expect("queue lock").claim_all();
        let old_revision = work[0].revision;
        owner.defer_auto_trash(
            "one",
            PathBuf::from("kick.wav").as_path(),
            old_revision,
            PathBuf::from("/tmp/one/kick.wav"),
        );
        owner.rekey_cross_source(
            "one",
            PathBuf::from("kick.wav").as_path(),
            "two",
            PathBuf::from("moved/kick.wav").as_path(),
            PathBuf::from("/tmp/two").as_path(),
            PathBuf::from("/tmp/two/.db").as_path(),
        );
        let queue = owner.queue.lock().expect("queue lock");
        let old_key = RatingPersistKey {
            source_id: String::from("one"),
            relative_path: PathBuf::from("kick.wav"),
        };
        let new_key = RatingPersistKey {
            source_id: String::from("two"),
            relative_path: PathBuf::from("moved/kick.wav"),
        };
        assert!(!queue.entries.contains_key(&old_key));
        assert!(queue.entries[&new_key].revision > old_revision);
        assert_eq!(
            queue.entries[&new_key].state,
            RatingPersistEntryState::Pending
        );
        assert_eq!(
            queue.entries[&new_key].request.lifecycle_generation,
            Some(9)
        );
        assert_eq!(
            queue.deferred_auto_trash[&new_key].1,
            PathBuf::from("/tmp/two/moved/kick.wav")
        );
    }

    #[test]
    fn close_flushes_latest_inflight_supersession_including_lock() {
        let source_root = tempfile::tempdir().expect("source root");
        let database_root = tempfile::tempdir().expect("database root");
        let relative_path = PathBuf::from("kick.wav");
        let absolute_path = source_root.path().join(&relative_path);
        std::fs::write(&absolute_path, b"fixture").expect("sample fixture");
        let source_id = String::from("one");
        let mut owner = owner();

        owner.enqueue(RatingPersistRequest {
            source_id: source_id.clone(),
            lifecycle_generation: Some(7),
            root: source_root.path().to_path_buf(),
            database_root: database_root.path().to_path_buf(),
            relative_path: relative_path.clone(),
            absolute_path: absolute_path.clone(),
            rating: wavecrate::sample_sources::Rating::new(1),
            locked: false,
        });
        let _inflight = owner.queue.lock().expect("queue lock").claim_all();
        owner.enqueue(RatingPersistRequest {
            source_id,
            lifecycle_generation: Some(7),
            root: source_root.path().to_path_buf(),
            database_root: database_root.path().to_path_buf(),
            relative_path: relative_path.clone(),
            absolute_path,
            rating: wavecrate::sample_sources::Rating::KEEP_3,
            locked: true,
        });

        assert_eq!(owner.close(), 0);
        let database = SourceDatabase::open_for_user_metadata_write_with_database_root(
            source_root.path(),
            database_root.path(),
        )
        .expect("reopen source database");
        assert_eq!(
            database.tag_for_path(&relative_path).expect("read rating"),
            Some(wavecrate::sample_sources::Rating::KEEP_3)
        );
        assert_eq!(
            database.locked_for_path(&relative_path).expect("read lock"),
            Some(true)
        );
    }

    #[test]
    fn close_reports_failed_requests_and_clears_closed_queue() {
        let source_root = tempfile::tempdir().expect("source root");
        let database_root = tempfile::tempdir().expect("database root");
        let mut owner = owner();
        owner.enqueue(RatingPersistRequest {
            source_id: String::from("one"),
            lifecycle_generation: Some(7),
            root: source_root.path().to_path_buf(),
            database_root: database_root.path().to_path_buf(),
            relative_path: PathBuf::from("missing.wav"),
            absolute_path: source_root.path().join("missing.wav"),
            rating: wavecrate::sample_sources::Rating::KEEP_3,
            locked: true,
        });

        assert_eq!(owner.close(), 1);
        let queue = owner.queue.lock().expect("queue lock");
        assert!(queue.closed);
        assert!(queue.entries.is_empty());
        assert!(queue.desired.is_empty());
    }

    #[test]
    fn worker_persists_current_generation_rating_and_lock() {
        let source_root = tempfile::tempdir().expect("source root");
        let database_root = tempfile::tempdir().expect("database root");
        let relative_path = PathBuf::from("kick.wav");
        let absolute_path = source_root.path().join(&relative_path);
        std::fs::write(&absolute_path, b"fixture").expect("sample fixture");
        let mut owner = owner();
        owner.enqueue(RatingPersistRequest {
            source_id: String::from("one"),
            lifecycle_generation: Some(7),
            root: source_root.path().to_path_buf(),
            database_root: database_root.path().to_path_buf(),
            relative_path: relative_path.clone(),
            absolute_path: absolute_path.clone(),
            rating: wavecrate::sample_sources::Rating::KEEP_3,
            locked: true,
        });

        let result =
            persist_rating_queue(Arc::clone(&owner.queue), Arc::clone(&owner.persist_gate));
        assert_eq!(result.results.len(), 1);
        assert_eq!(result.results[0].result, Some(Ok(())));

        let database = SourceDatabase::open_for_user_metadata_write_with_database_root(
            source_root.path(),
            database_root.path(),
        )
        .expect("reopen source database");
        assert_eq!(
            database.tag_for_path(&relative_path).expect("read rating"),
            Some(wavecrate::sample_sources::Rating::KEEP_3)
        );
        assert_eq!(
            database.locked_for_path(&relative_path).expect("read lock"),
            Some(true)
        );
    }

    #[test]
    fn worker_rejects_generation_mismatch_without_persisting() {
        let source_root = tempfile::tempdir().expect("source root");
        let database_root = tempfile::tempdir().expect("database root");
        let relative_path = PathBuf::from("kick.wav");
        let absolute_path = source_root.path().join(&relative_path);
        std::fs::write(&absolute_path, b"fixture").expect("sample fixture");
        let mut owner = owner();
        owner.enqueue(RatingPersistRequest {
            source_id: String::from("one"),
            lifecycle_generation: Some(8),
            root: source_root.path().to_path_buf(),
            database_root: database_root.path().to_path_buf(),
            relative_path: relative_path.clone(),
            absolute_path,
            rating: wavecrate::sample_sources::Rating::KEEP_3,
            locked: true,
        });

        let result =
            persist_rating_queue(Arc::clone(&owner.queue), Arc::clone(&owner.persist_gate));
        assert_eq!(result.results.len(), 1);
        assert_eq!(result.results[0].result, None);

        let database = SourceDatabase::open_for_user_metadata_write_with_database_root(
            source_root.path(),
            database_root.path(),
        )
        .expect("reopen source database");
        assert_eq!(
            database.tag_for_path(&relative_path).expect("read rating"),
            None
        );
        assert_eq!(
            database.locked_for_path(&relative_path).expect("read lock"),
            None
        );
    }
}

impl BackgroundTaskState {
    pub(in crate::native_app) fn new_with_registrations(
        worker_sender: Sender<GuiMessage>,
        worker_receiver: Option<Receiver<GuiMessage>>,
        sources: Vec<wavecrate::sample_sources::SampleSource>,
    ) -> (Self, Vec<SourceProcessingRegistration>) {
        #[cfg(test)]
        let recovery_root = sources.first().map(|source| source.root.join(".wavecrate"));
        #[cfg(not(test))]
        let recovery_root = None;
        #[cfg(not(test))]
        let (source_processing, registrations) =
            Self::start_source_processing(&worker_sender, sources);
        #[cfg(test)]
        let (source_processing, registrations) =
            SourceProcessingSupervisor::dormant_with_sources(sources);
        let state = Self::with_source_processing(
            worker_sender,
            worker_receiver,
            source_processing,
            registrations.clone(),
            recovery_root,
        );
        (state, registrations)
    }

    #[cfg(any(test, feature = "legacy-controller"))]
    pub(in crate::native_app) fn new_runtime_with_registrations(
        worker_sender: Sender<GuiMessage>,
        worker_receiver: Option<Receiver<GuiMessage>>,
        sources: Vec<wavecrate::sample_sources::SampleSource>,
    ) -> (Self, Vec<SourceProcessingRegistration>) {
        let (source_processing, registrations) =
            Self::start_source_processing(&worker_sender, sources);
        let state = Self::with_source_processing(
            worker_sender,
            worker_receiver,
            source_processing,
            registrations.clone(),
            None,
        );
        (state, registrations)
    }

    fn start_source_processing(
        worker_sender: &Sender<GuiMessage>,
        sources: Vec<wavecrate::sample_sources::SampleSource>,
    ) -> (
        SourceProcessingSupervisor,
        Vec<SourceProcessingRegistration>,
    ) {
        SourceProcessingSupervisor::start_with_event_sink(
            sources,
            GuiSourceProcessingEventSink::new(worker_sender.clone()),
        )
    }

    fn with_source_processing(
        worker_sender: Sender<GuiMessage>,
        worker_receiver: Option<Receiver<GuiMessage>>,
        source_processing: SourceProcessingSupervisor,
        registrations: Vec<SourceProcessingRegistration>,
        recovery_root: Option<PathBuf>,
    ) -> Self {
        let source_lifecycle_generations: BTreeMap<String, u64> = registrations
            .iter()
            .map(|registration| {
                (
                    registration.source.id.to_string(),
                    registration.lifecycle_generation,
                )
            })
            .collect();
        let waveform_recovery_root = recovery_root.map(|path| {
            let file = std::fs::File::open(&path).expect("test recovery root");
            let identity =
                wavecrate_library::filesystem_identity::stable_filesystem_identity_from_open_file(
                    &file,
                )
                .expect("test recovery root identity");
            crate::native_app::transaction_history::operation_journal::RecoveryRootCapability {
                path,
                file: Arc::new(file),
                identity,
            }
        });
        #[cfg(not(test))]
        let operation_journal = super::OperationJournalOwner::start();
        #[cfg(test)]
        let operation_journal = super::OperationJournalOwner::disabled();
        Self {
            waveform_recovery_root,
            worker_sender,
            worker_receiver,
            next_task_id: 1,
            sample_load_validation_task: ui::LatestTask::new(),
            deferred_sample_load_task: ui::LatestTask::new(),
            sample_load_tasks: ui::ResourceTasks::new(),
            harvest_touched_persist: HarvestTouchedPersistOwner::new(),
            harvest_selection_derivation: HarvestSelectionDerivationOwner::new(),
            rating_persist: RatingPersistOwner::new_with_lifecycle_generations(
                source_lifecycle_generations.clone(),
            ),
            operation_journal,
            active_sample_load_key: None,
            sample_load_cancel: None,
            settled_sample_promotion_task: ui::LatestTask::new(),
            preview_audition_task: ui::LatestTask::new(),
            preview_audition_warm_task: ui::LatestTask::new(),
            starmap_audition_advance_task: ui::LatestTask::new(),
            starmap_audition_promotion_task: ui::LatestTask::new(),
            audio_options_refresh_task: ui::LatestTask::new(),
            audio_options_refresh_cancel: None,
            audio_output_persist_task: ui::LatestTask::new(),
            audio_output_persist_generation: Arc::new(AtomicU64::new(0)),
            audio_output_persist_lock: Arc::new(Mutex::new(())),
            audio_open: AudioOpenTaskOwner::new(),
            folder_tree_refresh_task: ui::LatestTask::new(),
            folder_verify_task: ui::LatestTask::new(),
            release_update_check_task: ui::LatestTask::new(),
            global_storage_usage_task: ui::LatestTask::new(),
            waveform_destructive_edit_task: ui::LatestTask::new(),
            waveform_destructive_edit_context: None,
            history_file_io_gate: Arc::new(Mutex::new(())),
            normalization_progress: None,
            normalization_active_paths: HashSet::new(),
            normalization_queue: VecDeque::new(),
            file_move_progress: None,
            source_processing_progress: None,
            source_processing_health: BTreeMap::new(),
            source_lifecycle_generations,
            progress_tick: 0.0,
            frame_cadence: frame_ui::FrameCadenceMonitor::new(),
            source_processing,
        }
    }

    pub(in crate::native_app) fn next_task_id(&mut self) -> u64 {
        let task_id = self.next_task_id;
        self.next_task_id = self.next_task_id.saturating_add(1);
        task_id
    }

    #[cfg(test)]
    pub(in crate::native_app) fn for_tests() -> Self {
        Self::new_with_registrations(std::sync::mpsc::channel().0, None, Vec::new()).0
    }

    pub(in crate::native_app) fn take_operation_journal_status(
        &mut self,
    ) -> Option<super::journal::OperationJournalStatus> {
        let status = self.operation_journal.take_status()?;
        if let super::journal::OperationJournalStatus::Available { recovery_root, .. } = &status {
            self.waveform_recovery_root = Some(recovery_root.clone());
        } else if matches!(
            status,
            super::journal::OperationJournalStatus::Initializing
                | super::journal::OperationJournalStatus::Unavailable { .. }
        ) {
            self.waveform_recovery_root = None;
        }
        Some(status)
    }
}

const HARVEST_TOUCHED_QUEUE_CAPACITY: usize = 64;
const HARVEST_TOUCHED_BATCH_LIMIT: usize = 32;
const HARVEST_TOUCHED_ADMISSION_POLL_DELAY: Duration = Duration::from_millis(50);

#[derive(Clone)]
pub(in crate::native_app) struct RatingPersistOwner {
    task: ui::LatestTask,
    queue: Arc<Mutex<RatingPersistQueue>>,
    /// Serializes source-database writes between the background worker and the
    /// synchronous shutdown flush.  The UI path never takes this lock.
    persist_gate: Arc<Mutex<()>>,
}

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub(in crate::native_app) struct RatingPersistKey {
    source_id: String,
    relative_path: PathBuf,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(in crate::native_app) struct RatingPersistBatchResult {
    pub(in crate::native_app) results: Vec<RatingPersistBatchItem>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(in crate::native_app) struct RatingPersistBatchItem {
    pub(in crate::native_app) key: RatingPersistKey,
    pub(in crate::native_app) revision: u64,
    pub(in crate::native_app) absolute_path: PathBuf,
    pub(in crate::native_app) result: Option<Result<(), String>>,
}

struct RatingPersistQueue {
    entries: std::collections::HashMap<RatingPersistKey, RatingPersistEntry>,
    desired: std::collections::HashMap<RatingPersistKey, RatingPersistRequest>,
    deferred_auto_trash: std::collections::HashMap<RatingPersistKey, (u64, PathBuf)>,
    lifecycle_generations: BTreeMap<String, u64>,
    order: VecDeque<RatingPersistKey>,
    next_revision: u64,
    closed: bool,
}

struct RatingPersistEntry {
    request: RatingPersistRequest,
    revision: u64,
    state: RatingPersistEntryState,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum RatingPersistEntryState {
    Pending,
    InFlight,
    Failed,
}

struct RatingPersistWork {
    key: RatingPersistKey,
    request: RatingPersistRequest,
    revision: u64,
}

impl RatingPersistOwner {
    fn new_with_lifecycle_generations(lifecycle_generations: BTreeMap<String, u64>) -> Self {
        Self {
            task: ui::LatestTask::new(),
            queue: Arc::new(Mutex::new(RatingPersistQueue {
                entries: std::collections::HashMap::new(),
                desired: std::collections::HashMap::new(),
                deferred_auto_trash: std::collections::HashMap::new(),
                lifecycle_generations,
                order: VecDeque::new(),
                next_revision: 1,
                closed: false,
            })),
            persist_gate: Arc::new(Mutex::new(())),
        }
    }

    pub(in crate::native_app) fn enqueue(&mut self, request: RatingPersistRequest) -> u64 {
        let key = RatingPersistKey {
            source_id: request.source_id.clone(),
            relative_path: request.relative_path.clone(),
        };
        let Ok(mut queue) = self.queue.lock() else {
            tracing::warn!("rating persistence queue lock poisoned; dropping request");
            return 0;
        };
        if queue.closed {
            return 0;
        }
        let revision = queue.next_revision;
        queue.next_revision = queue.next_revision.saturating_add(1);
        queue.desired.insert(key.clone(), request.clone());
        queue.deferred_auto_trash.remove(&key);
        if let Some(entry) = queue.entries.get_mut(&key) {
            entry.request = request;
            entry.revision = revision;
            if entry.state != RatingPersistEntryState::Pending {
                entry.state = RatingPersistEntryState::Pending;
                queue.order.push_back(key);
            }
            return revision;
        }
        queue.order.push_back(key.clone());
        queue.entries.insert(
            key,
            RatingPersistEntry {
                request,
                revision,
                state: RatingPersistEntryState::Pending,
            },
        );
        revision
    }

    pub(in crate::native_app) fn revision_for(
        &self,
        source_id: &str,
        relative_path: &std::path::Path,
    ) -> Option<u64> {
        let key = RatingPersistKey {
            source_id: source_id.to_owned(),
            relative_path: relative_path.to_path_buf(),
        };
        self.queue
            .lock()
            .ok()
            .and_then(|queue| queue.entries.get(&key).map(|entry| entry.revision))
    }

    pub(in crate::native_app) fn defer_auto_trash(
        &mut self,
        source_id: &str,
        relative_path: &std::path::Path,
        revision: u64,
        absolute_path: PathBuf,
    ) {
        let key = RatingPersistKey {
            source_id: source_id.to_owned(),
            relative_path: relative_path.to_path_buf(),
        };
        if let Ok(mut queue) = self.queue.lock() {
            queue
                .deferred_auto_trash
                .insert(key, (revision, absolute_path));
        }
    }

    pub(in crate::native_app) fn take_committed_auto_trash(&mut self) -> Vec<PathBuf> {
        self.queue
            .lock()
            .map(|mut queue| {
                let mut paths = Vec::new();
                let keys = queue
                    .deferred_auto_trash
                    .iter()
                    .filter_map(|(key, (revision, _))| {
                        queue
                            .entries
                            .get(key)
                            .is_none()
                            .then_some((key.clone(), *revision))
                    })
                    .collect::<Vec<_>>();
                for (key, _) in keys {
                    if let Some((_, path)) = queue.deferred_auto_trash.remove(&key) {
                        paths.push(path);
                    }
                }
                paths
            })
            .unwrap_or_default()
    }

    pub(in crate::native_app) fn desired_snapshot(&self) -> Vec<RatingPersistRequest> {
        self.queue
            .lock()
            .map(|queue| queue.desired.values().cloned().collect())
            .unwrap_or_default()
    }

    pub(in crate::native_app) fn retain_current_lifecycles(
        &mut self,
        generations: &BTreeMap<String, u64>,
    ) {
        if let Ok(mut queue) = self.queue.lock() {
            queue.lifecycle_generations = generations.clone();
            queue.entries.retain(|key, entry| {
                entry
                    .request
                    .lifecycle_generation
                    .is_none_or(|generation| generations.get(&key.source_id) == Some(&generation))
            });
            queue.desired.retain(|key, request| {
                request
                    .lifecycle_generation
                    .is_none_or(|generation| generations.get(&key.source_id) == Some(&generation))
            });
            let stale_auto_trash = queue
                .deferred_auto_trash
                .keys()
                .filter(|key| {
                    queue.entries.get(*key).is_none_or(|entry| {
                        entry
                            .request
                            .lifecycle_generation
                            .is_some_and(|generation| {
                                generations.get(&key.source_id) != Some(&generation)
                            })
                    })
                })
                .cloned()
                .collect::<Vec<_>>();
            for key in stale_auto_trash {
                queue.deferred_auto_trash.remove(&key);
            }
        }
    }

    pub(in crate::native_app) fn rekey_exact(
        &mut self,
        source_id: &str,
        from: &std::path::Path,
        to: &std::path::Path,
    ) {
        self.rekey_prefix(source_id, from, to, true);
    }

    pub(in crate::native_app) fn rekey_prefix(
        &mut self,
        source_id: &str,
        from: &std::path::Path,
        to: &std::path::Path,
        exact: bool,
    ) {
        let Ok(mut queue) = self.queue.lock() else {
            return;
        };
        let keys = queue
            .desired
            .keys()
            .filter(|key| {
                key.source_id == source_id
                    && (key.relative_path == from
                        || (!exact && key.relative_path.starts_with(from)))
            })
            .cloned()
            .collect::<Vec<_>>();
        for key in keys {
            let Some(mut request) = queue.desired.remove(&key) else {
                continue;
            };
            let Ok(suffix) = key.relative_path.strip_prefix(from) else {
                continue;
            };
            let new_relative = if exact {
                to.to_path_buf()
            } else {
                to.join(suffix)
            };
            request.relative_path = new_relative.clone();
            request.absolute_path = request.root.join(&new_relative);
            let new_key = RatingPersistKey {
                source_id: source_id.to_owned(),
                relative_path: new_relative,
            };
            queue.desired.insert(new_key, request);
        }
        let entry_keys = queue
            .entries
            .keys()
            .filter(|key| {
                key.source_id == source_id
                    && (key.relative_path == from
                        || (!exact && key.relative_path.starts_with(from)))
            })
            .cloned()
            .collect::<Vec<_>>();
        for key in entry_keys {
            let Ok(suffix) = key.relative_path.strip_prefix(from) else {
                continue;
            };
            let new_relative = if exact {
                to.to_path_buf()
            } else {
                to.join(suffix)
            };
            let new_key = RatingPersistKey {
                source_id: source_id.to_owned(),
                relative_path: new_relative.clone(),
            };
            if let Some(mut entry) = queue.entries.remove(&key) {
                entry.request.relative_path = new_relative.clone();
                entry.request.absolute_path = entry.request.root.join(&new_relative);
                if entry.state == RatingPersistEntryState::InFlight {
                    let revision = queue.next_revision;
                    queue.next_revision = queue.next_revision.saturating_add(1);
                    entry.revision = revision;
                    entry.state = RatingPersistEntryState::Pending;
                    entry.request.lifecycle_generation = queue
                        .lifecycle_generations
                        .get(source_id)
                        .copied()
                        .or(entry.request.lifecycle_generation);
                    queue.order.push_back(new_key.clone());
                } else {
                    for queued_key in &mut queue.order {
                        if *queued_key == key {
                            *queued_key = new_key.clone();
                        }
                    }
                }
                queue.entries.insert(new_key.clone(), entry);
            }
            if let Some(deferred) = queue.deferred_auto_trash.remove(&key) {
                let revision = queue.entries.get(&new_key).map(|entry| entry.revision);
                if let Some(revision) = revision {
                    let absolute_path = queue
                        .entries
                        .get(&new_key)
                        .map(|entry| entry.request.absolute_path.clone())
                        .unwrap_or(deferred.1);
                    queue
                        .deferred_auto_trash
                        .insert(new_key.clone(), (revision, absolute_path));
                }
            }
        }
    }

    pub(in crate::native_app) fn rekey_cross_source(
        &mut self,
        from_source_id: &str,
        from: &std::path::Path,
        to_source_id: &str,
        to: &std::path::Path,
        to_root: &std::path::Path,
        to_database_root: &std::path::Path,
    ) {
        let Ok(mut queue) = self.queue.lock() else {
            return;
        };
        let keys = queue
            .desired
            .keys()
            .filter(|key| key.source_id == from_source_id && key.relative_path == from)
            .cloned()
            .collect::<Vec<_>>();
        for key in keys {
            let Some(mut request) = queue.desired.remove(&key) else {
                continue;
            };
            request.source_id = to_source_id.to_owned();
            request.root = to_root.to_path_buf();
            request.database_root = to_database_root.to_path_buf();
            request.relative_path = to.to_path_buf();
            request.absolute_path = request.root.join(to);
            request.lifecycle_generation = queue.lifecycle_generations.get(to_source_id).copied();
            queue.desired.insert(
                RatingPersistKey {
                    source_id: to_source_id.to_owned(),
                    relative_path: to.to_path_buf(),
                },
                request,
            );
        }
        let old_key = RatingPersistKey {
            source_id: from_source_id.to_owned(),
            relative_path: from.to_path_buf(),
        };
        let new_key = RatingPersistKey {
            source_id: to_source_id.to_owned(),
            relative_path: to.to_path_buf(),
        };
        if let Some(mut entry) = queue.entries.remove(&old_key) {
            entry.request.source_id = to_source_id.to_owned();
            entry.request.root = to_root.to_path_buf();
            entry.request.database_root = to_database_root.to_path_buf();
            entry.request.relative_path = to.to_path_buf();
            entry.request.absolute_path = entry.request.root.join(to);
            let destination_generation = queue.lifecycle_generations.get(to_source_id).copied();
            if entry.state == RatingPersistEntryState::InFlight {
                let revision = queue.next_revision;
                queue.next_revision = queue.next_revision.saturating_add(1);
                entry.revision = revision;
                entry.state = RatingPersistEntryState::Pending;
                entry.request.lifecycle_generation = queue
                    .lifecycle_generations
                    .get(to_source_id)
                    .copied()
                    .or(destination_generation);
                queue.order.push_back(new_key.clone());
            } else {
                entry.request.lifecycle_generation = destination_generation;
                for queued_key in &mut queue.order {
                    if *queued_key == old_key {
                        *queued_key = new_key.clone();
                    }
                }
            }
            queue.entries.insert(new_key.clone(), entry);
        }
        if let Some(deferred) = queue.deferred_auto_trash.remove(&old_key) {
            if let Some(revision) = queue.entries.get(&new_key).map(|entry| entry.revision) {
                let absolute_path = queue
                    .entries
                    .get(&new_key)
                    .map(|entry| entry.request.absolute_path.clone())
                    .unwrap_or(deferred.1);
                queue
                    .deferred_auto_trash
                    .insert(new_key.clone(), (revision, absolute_path));
            }
        }
    }

    pub(in crate::native_app) fn invalidate_prefix(
        &mut self,
        source_id: &str,
        prefix: &std::path::Path,
    ) {
        if let Ok(mut queue) = self.queue.lock() {
            queue.desired.retain(|key, _| {
                !(key.source_id == source_id && key.relative_path.starts_with(prefix))
            });
            queue.entries.retain(|key, _| {
                !(key.source_id == source_id && key.relative_path.starts_with(prefix))
            });
            queue.order.retain(|key| {
                !(key.source_id == source_id && key.relative_path.starts_with(prefix))
            });
            queue.deferred_auto_trash.retain(|key, _| {
                !(key.source_id == source_id && key.relative_path.starts_with(prefix))
            });
        }
    }

    pub(in crate::native_app) fn schedule_if_idle(
        &mut self,
        context: &mut ui::UiUpdateContext<GuiMessage>,
    ) {
        let pending = self.queue.lock().ok().is_some_and(|queue| {
            queue
                .entries
                .values()
                .any(|entry| entry.state == RatingPersistEntryState::Pending)
        });
        if !pending || self.task.active().is_some() {
            return;
        }
        let queue = Arc::clone(&self.queue);
        let persist_gate = Arc::clone(&self.persist_gate);
        context
            .business()
            .background("gui-rating-persist")
            .latest(&mut self.task)
            .run(
                move |_| persist_rating_queue(queue, persist_gate),
                GuiMessage::RatingPersisted,
            );
    }

    pub(in crate::native_app) fn finish(
        &mut self,
        completion: ui::TaskCompletion<RatingPersistBatchResult>,
    ) -> Option<RatingPersistBatchResult> {
        let result = self.task.finish_completion(completion)?;
        if let Ok(mut queue) = self.queue.lock() {
            for item in &result.results {
                let Some(entry) = queue.entries.get(&item.key) else {
                    continue;
                };
                if entry.revision != item.revision
                    || entry.state != RatingPersistEntryState::InFlight
                    || entry
                        .request
                        .lifecycle_generation
                        .is_some_and(|generation| {
                            queue.lifecycle_generations.get(&item.key.source_id)
                                != Some(&generation)
                        })
                {
                    continue;
                }
                if item.result.as_ref().is_some_and(|result| result.is_ok()) {
                    queue.entries.remove(&item.key);
                } else {
                    // Keep failures visible but do not retry until a newer UI projection arrives.
                    if let Some(entry) = queue.entries.get_mut(&item.key) {
                        entry.state = RatingPersistEntryState::Failed;
                    }
                    queue.deferred_auto_trash.remove(&item.key);
                }
            }
        }
        Some(result)
    }

    pub(in crate::native_app) fn close(&mut self) -> usize {
        self.task.cancel();
        let requests = {
            let Ok(mut queue) = self.queue.lock() else {
                tracing::error!("rating persistence queue lock poisoned during shutdown flush");
                return 0;
            };
            if queue.closed {
                return 0;
            }
            // Fence new UI requests before waiting for a worker that may still
            // be finishing an older batch.  The worker's final current check
            // observes this flag while holding the same persistence gate.
            queue.closed = true;
            queue
                .entries
                .values()
                .filter(|entry| {
                    entry.request.lifecycle_generation.is_none_or(|generation| {
                        queue.lifecycle_generations.get(&entry.request.source_id)
                            == Some(&generation)
                    })
                })
                .map(|entry| entry.request.clone())
                .collect::<Vec<_>>()
        };

        if requests.is_empty() {
            if let Ok(mut queue) = self.queue.lock() {
                queue.entries.clear();
                queue.order.clear();
                queue.desired.clear();
                queue.deferred_auto_trash.clear();
            }
            return 0;
        }

        // Shutdown is the one path allowed to synchronously wait for source
        // database I/O.  This gate also ensures a worker that was already in
        // flight cannot run after this flush and overwrite its latest state.
        let Ok(_persist_gate) = self.persist_gate.lock() else {
            tracing::error!(
                count = requests.len(),
                "rating persistence gate poisoned during shutdown flush"
            );
            return requests.len();
        };
        let persisted = persist_rating_requests(&requests, |_| true);
        let mut unflushed = 0;
        for (request, result) in requests.iter().zip(persisted) {
            match result {
                Some(Ok(())) => {}
                Some(Err(error)) => {
                    unflushed += 1;
                    tracing::error!(
                        path = %request.absolute_path.display(),
                        error = %error,
                        "rating persistence shutdown flush failed"
                    );
                }
                None => {
                    unflushed += 1;
                    tracing::error!(
                        path = %request.absolute_path.display(),
                        "rating persistence shutdown flush did not attempt request"
                    );
                }
            }
        }
        if let Ok(mut queue) = self.queue.lock() {
            queue.entries.clear();
            queue.order.clear();
            queue.desired.clear();
            queue.deferred_auto_trash.clear();
        }
        unflushed
    }
}

fn persist_rating_queue(
    queue: Arc<Mutex<RatingPersistQueue>>,
    persist_gate: Arc<Mutex<()>>,
) -> RatingPersistBatchResult {
    let work = queue
        .lock()
        .map(|mut queue| queue.claim_all())
        .unwrap_or_default();
    let requests = work
        .iter()
        .map(|item| item.request.clone())
        .collect::<Vec<_>>();
    let Ok(_persist_gate) = persist_gate.lock() else {
        let results = work
            .into_iter()
            .map(|item| RatingPersistBatchItem {
                key: item.key,
                revision: item.revision,
                absolute_path: item.request.absolute_path,
                result: Some(Err(String::from(
                    "rating persistence gate poisoned before database write",
                ))),
            })
            .collect();
        return RatingPersistBatchResult { results };
    };
    let persisted = persist_rating_requests(&requests, |request| {
        let Some(item) = work.iter().find(|item| {
            item.request.source_id == request.source_id
                && item.request.relative_path == request.relative_path
        }) else {
            return false;
        };
        queue.lock().ok().is_some_and(|queue| {
            !queue.closed
                && queue.entries.get(&item.key).is_some_and(|entry| {
                    entry.revision == item.revision
                        && entry.state == RatingPersistEntryState::InFlight
                        && entry.request.lifecycle_generation.is_none_or(|generation| {
                            queue.lifecycle_generations.get(&item.key.source_id)
                                == Some(&generation)
                        })
                })
        })
    });
    let mut results = Vec::with_capacity(work.len());
    for (item, result) in work.into_iter().zip(persisted) {
        results.push(RatingPersistBatchItem {
            key: item.key,
            revision: item.revision,
            absolute_path: item.request.absolute_path,
            result,
        });
    }
    RatingPersistBatchResult { results }
}

impl RatingPersistQueue {
    fn claim_all(&mut self) -> Vec<RatingPersistWork> {
        let mut work = Vec::with_capacity(self.order.len());
        while let Some(key) = self.order.pop_front() {
            let Some(entry) = self.entries.get_mut(&key) else {
                continue;
            };
            if entry.state != RatingPersistEntryState::Pending {
                continue;
            }
            entry.state = RatingPersistEntryState::InFlight;
            work.push(RatingPersistWork {
                key,
                request: entry.request.clone(),
                revision: entry.revision,
            });
        }
        work
    }
}

#[derive(Clone)]
pub(in crate::native_app) struct HarvestTouchedPersistOwner {
    task: ui::LatestTask,
    admission_receipt: Option<HarvestTouchedPersistAdmissionReceipt>,
    admission_poll_task: ui::LatestTask,
    admission_retry_used: bool,
    queue: Arc<Mutex<HarvestTouchedPersistQueue>>,
}

#[derive(Clone)]
struct HarvestTouchedPersistAdmissionReceipt {
    ticket: ui::TaskTicket,
    receipt: ui::BusinessTaskAdmissionReceipt,
}

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub(in crate::native_app) struct HarvestTouchedPersistKey {
    source_id: SourceId,
    relative_path: PathBuf,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(in crate::native_app) enum HarvestTouchedPersistAdmission {
    Queued,
    Replaced,
    Saturated,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(in crate::native_app) struct HarvestSelectionDerivationBatchResult {
    pub(in crate::native_app) results: Vec<HarvestSelectionDerivationBatchItem>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(in crate::native_app) struct HarvestSelectionDerivationBatchItem {
    pub(in crate::native_app) id: u64,
    pub(in crate::native_app) revision: u64,
    pub(in crate::native_app) source_path: PathBuf,
    pub(in crate::native_app) child_path: PathBuf,
    pub(in crate::native_app) result: Result<(), String>,
}

#[derive(Clone)]
pub(in crate::native_app) struct HarvestSelectionDerivationOwner {
    task: ui::LatestTask,
    queue: Arc<Mutex<HarvestSelectionDerivationQueue>>,
    persist_gate: Arc<Mutex<()>>,
}

struct HarvestSelectionDerivationQueue {
    entries: std::collections::HashMap<u64, HarvestSelectionDerivationEntry>,
    order: VecDeque<u64>,
    next_id: u64,
    next_revision: u64,
    closed: bool,
}

struct HarvestSelectionDerivationEntry {
    request: HarvestSelectionDerivationRequest,
    revision: u64,
    state: HarvestSelectionDerivationEntryState,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum HarvestSelectionDerivationEntryState {
    Pending,
    InFlight,
    Failed,
}

struct HarvestSelectionDerivationWork {
    id: u64,
    revision: u64,
}

impl HarvestSelectionDerivationOwner {
    fn new() -> Self {
        Self {
            task: ui::LatestTask::new(),
            queue: Arc::new(Mutex::new(HarvestSelectionDerivationQueue {
                entries: std::collections::HashMap::new(),
                order: VecDeque::new(),
                next_id: 1,
                next_revision: 1,
                closed: false,
            })),
            persist_gate: Arc::new(Mutex::new(())),
        }
    }

    pub(in crate::native_app) fn enqueue(&mut self, request: HarvestSelectionDerivationRequest) {
        let Ok(mut queue) = self.queue.lock() else {
            tracing::error!("harvest selection derivation queue lock poisoned; dropping request");
            return;
        };
        if queue.closed {
            tracing::error!("harvest selection derivation queue closed; dropping request");
            return;
        }
        let id = queue.next_id;
        queue.next_id = queue.next_id.saturating_add(1);
        let revision = queue.next_revision;
        queue.next_revision = queue.next_revision.saturating_add(1);
        queue.order.push_back(id);
        queue.entries.insert(
            id,
            HarvestSelectionDerivationEntry {
                request,
                revision,
                state: HarvestSelectionDerivationEntryState::Pending,
            },
        );
    }

    pub(in crate::native_app) fn schedule_if_idle(
        &mut self,
        context: &mut ui::UiUpdateContext<GuiMessage>,
    ) {
        let pending = self.queue.lock().ok().is_some_and(|queue| {
            queue
                .entries
                .values()
                .any(|entry| entry.state == HarvestSelectionDerivationEntryState::Pending)
        });
        if !pending || self.task.active().is_some() {
            return;
        }
        let queue = Arc::clone(&self.queue);
        let persist_gate = Arc::clone(&self.persist_gate);
        context
            .business()
            .background("gui-harvest-selection-derivation")
            .latest(&mut self.task)
            .run(
                move |_| persist_harvest_selection_derivation_queue(queue, persist_gate),
                GuiMessage::HarvestSelectionDerivationPersisted,
            );
    }

    pub(in crate::native_app) fn finish(
        &mut self,
        completion: ui::TaskCompletion<HarvestSelectionDerivationBatchResult>,
    ) -> Option<HarvestSelectionDerivationBatchResult> {
        let result = self.task.finish_completion(completion)?;
        for item in &result.results {
            if let Err(error) = &item.result {
                tracing::warn!(
                    source = %item.source_path.display(),
                    child = %item.child_path.display(),
                    "failed to record harvest derivation in background: {error}"
                );
            }
        }
        Some(result)
    }

    pub(in crate::native_app) fn rekey_file(
        &self,
        old_path: &std::path::Path,
        new_path: &std::path::Path,
    ) {
        self.rekey_paths(|request| {
            let mut changed = false;
            if request.source_path == old_path {
                request.source_path = new_path.to_path_buf();
                changed = true;
            }
            if request.child_path == old_path {
                request.child_path = new_path.to_path_buf();
                changed = true;
            }
            changed
        });
    }

    pub(in crate::native_app) fn rekey_file_cross_source(
        &self,
        old_path: &std::path::Path,
        new_path: &std::path::Path,
        destination_source: SampleSource,
    ) {
        self.rekey_paths(|request| {
            let mut changed = false;
            if request.source_path == old_path {
                request.source_path = new_path.to_path_buf();
                request.source = destination_source.clone();
                changed = true;
            }
            if request.child_path == old_path {
                request.child_path = new_path.to_path_buf();
                request.child_source = destination_source.clone();
                changed = true;
            }
            changed
        });
    }

    pub(in crate::native_app) fn rekey_prefix(
        &self,
        old_prefix: &std::path::Path,
        new_prefix: &std::path::Path,
    ) {
        self.rekey_paths(|request| {
            let mut changed = false;
            if let Ok(suffix) = request.source_path.strip_prefix(old_prefix) {
                request.source_path = new_prefix.join(suffix);
                changed = true;
            }
            if let Ok(suffix) = request.child_path.strip_prefix(old_prefix) {
                request.child_path = new_prefix.join(suffix);
                changed = true;
            }
            changed
        });
    }

    fn rekey_paths<F>(&self, mut rekey: F)
    where
        F: FnMut(&mut HarvestSelectionDerivationRequest) -> bool,
    {
        // Move completion and the worker must observe one serialized ordering:
        // persistence gate, then queue.  This prevents a move rekey from
        // changing an in-flight request after the worker has validated it.
        let Ok(_persist_gate) = self.persist_gate.lock() else {
            tracing::error!("harvest selection derivation gate lock poisoned during rekey");
            return;
        };
        let Ok(mut queue) = self.queue.lock() else {
            tracing::error!("harvest selection derivation queue lock poisoned during rekey");
            return;
        };
        let ids = queue.entries.keys().copied().collect::<Vec<_>>();
        for id in ids {
            let changed = {
                let Some(entry) = queue.entries.get_mut(&id) else {
                    continue;
                };
                rekey(&mut entry.request)
            };
            if changed {
                let revision = queue.next_revision;
                queue.next_revision = queue.next_revision.saturating_add(1);
                let was_pending = {
                    let Some(entry) = queue.entries.get_mut(&id) else {
                        continue;
                    };
                    let was_pending = entry.state == HarvestSelectionDerivationEntryState::Pending;
                    entry.revision = revision;
                    if !was_pending {
                        entry.state = HarvestSelectionDerivationEntryState::Pending;
                    }
                    was_pending
                };
                if !was_pending {
                    queue.order.push_back(id);
                }
            }
        }
    }

    pub(in crate::native_app) fn close(&mut self) -> usize {
        self.task.cancel();
        let Ok(_persist_gate) = self.persist_gate.lock() else {
            tracing::error!("harvest selection derivation gate poisoned during shutdown flush");
            return self
                .queue
                .lock()
                .map(|queue| queue.entries.len())
                .unwrap_or(0);
        };
        let requests = {
            let Ok(mut queue) = self.queue.lock() else {
                tracing::error!(
                    "harvest selection derivation queue lock poisoned during shutdown flush"
                );
                return 0;
            };
            if queue.closed {
                return 0;
            }
            queue.closed = true;
            queue
                .entries
                .values()
                .map(|entry| (entry.request.clone(), entry.revision))
                .collect::<Vec<_>>()
        };
        let mut unflushed = 0;
        for (request, _) in &requests {
            if let Err(error) = execute_harvest_selection_derivation(request.clone()) {
                unflushed += 1;
                tracing::error!(
                    source = %request.source_path.display(),
                    child = %request.child_path.display(),
                    "harvest derivation shutdown flush failed: {error}"
                );
            }
        }
        if let Ok(mut queue) = self.queue.lock() {
            queue.entries.clear();
            queue.order.clear();
        }
        unflushed
    }
}

fn persist_harvest_selection_derivation_queue(
    queue: Arc<Mutex<HarvestSelectionDerivationQueue>>,
    persist_gate: Arc<Mutex<()>>,
) -> HarvestSelectionDerivationBatchResult {
    let Ok(_persist_gate) = persist_gate.lock() else {
        tracing::error!("harvest selection derivation gate poisoned in background worker");
        return HarvestSelectionDerivationBatchResult {
            results: Vec::new(),
        };
    };
    let work = queue
        .lock()
        .map(|mut queue| queue.claim_all())
        .unwrap_or_default();
    let mut results = Vec::with_capacity(work.len());
    for item in work {
        // Validate the exact claimed revision immediately before executing the
        // database mutation.  Rekeys take the same gate first, so they either
        // happen before this check (and supersede the old work) or after the
        // old mutation has committed.
        let request = queue.lock().ok().and_then(|queue| {
            queue.entries.get(&item.id).and_then(|entry| {
                (entry.revision == item.revision
                    && entry.state == HarvestSelectionDerivationEntryState::InFlight
                    && !queue.closed)
                    .then(|| entry.request.clone())
            })
        });
        let Some(request) = request else {
            continue;
        };
        // Test-only rendezvous for the exact validation/execute interleaving.
        // Production behavior has no hook or additional synchronization here.
        #[cfg(test)]
        harvest_selection_derivation_test_boundary();
        let result = execute_harvest_selection_derivation(request.clone());
        if let Ok(mut queue) = queue.lock() {
            queue.acknowledge(item.id, item.revision, result.is_ok());
        }
        results.push(HarvestSelectionDerivationBatchItem {
            id: item.id,
            revision: item.revision,
            source_path: request.source_path,
            child_path: request.child_path,
            result,
        });
    }
    HarvestSelectionDerivationBatchResult { results }
}

impl HarvestSelectionDerivationQueue {
    fn claim_all(&mut self) -> Vec<HarvestSelectionDerivationWork> {
        let ids = std::mem::take(&mut self.order);
        ids.into_iter()
            .filter_map(|id| {
                let entry = self.entries.get_mut(&id)?;
                if entry.state != HarvestSelectionDerivationEntryState::Pending {
                    return None;
                }
                entry.state = HarvestSelectionDerivationEntryState::InFlight;
                Some(HarvestSelectionDerivationWork {
                    id,
                    revision: entry.revision,
                })
            })
            .collect()
    }

    fn acknowledge(&mut self, id: u64, revision: u64, successful: bool) {
        let Some(entry) = self.entries.get_mut(&id) else {
            return;
        };
        if entry.revision != revision
            || entry.state != HarvestSelectionDerivationEntryState::InFlight
        {
            return;
        }
        if successful {
            self.entries.remove(&id);
        } else {
            entry.state = HarvestSelectionDerivationEntryState::Failed;
        }
    }
}

#[cfg(test)]
type HarvestSelectionDerivationTestBoundaryHook = Arc<dyn Fn() + Send + Sync + 'static>;

#[cfg(test)]
static HARVEST_SELECTION_DERIVATION_TEST_BOUNDARY_HOOK: Mutex<
    Option<HarvestSelectionDerivationTestBoundaryHook>,
> = Mutex::new(None);

#[cfg(test)]
fn harvest_selection_derivation_test_boundary() {
    let hook = HARVEST_SELECTION_DERIVATION_TEST_BOUNDARY_HOOK
        .lock()
        .ok()
        .and_then(|hook| hook.clone());
    if let Some(hook) = hook {
        hook();
    }
}

#[cfg(test)]
mod harvest_selection_derivation_owner_tests {
    use super::*;
    use wavecrate::sample_sources::{HarvestDerivationOperation, HarvestFileKey, SampleSource};
    use wavecrate::selection::SelectionRange;

    fn request(root: &std::path::Path) -> HarvestSelectionDerivationRequest {
        let source = SampleSource::new_with_id(SourceId::new(), root.to_path_buf());
        HarvestSelectionDerivationRequest {
            source_path: root.join("old/source.wav"),
            child_path: root.join("old/child.wav"),
            source: source.clone(),
            child_source: source,
            selection: SelectionRange::new(0.25, 0.75),
            source_duration_seconds: 4.0,
            operation: HarvestDerivationOperation::Extract,
            inherited_tags: Vec::new(),
        }
    }

    #[test]
    fn move_rekeys_pending_parent_and_child_paths() {
        let root = tempfile::tempdir().expect("root");
        let mut owner = HarvestSelectionDerivationOwner::new();
        owner.enqueue(request(root.path()));
        owner.rekey_prefix(&root.path().join("old"), &root.path().join("new"));
        let queue = owner.queue.lock().expect("queue lock");
        let entry = queue.entries.values().next().expect("queued request");
        assert_eq!(
            entry.request.source_path,
            root.path().join("new/source.wav")
        );
        assert_eq!(entry.request.child_path, root.path().join("new/child.wav"));
    }

    #[test]
    fn in_flight_move_rekeys_to_one_pending_latest_edge() {
        let root = tempfile::tempdir().expect("root");
        let mut owner = HarvestSelectionDerivationOwner::new();
        owner.enqueue(request(root.path()));
        let claimed = owner.queue.lock().expect("queue lock").claim_all();
        assert_eq!(claimed.len(), 1);

        owner.rekey_prefix(&root.path().join("old"), &root.path().join("new"));

        let queue = owner.queue.lock().expect("queue lock");
        assert_eq!(queue.entries.len(), 1);
        let entry = queue.entries.values().next().expect("rekeyed request");
        assert_eq!(entry.state, HarvestSelectionDerivationEntryState::Pending);
        assert!(entry.revision > claimed[0].revision);
        assert_eq!(
            entry.request.source_path,
            root.path().join("new/source.wav")
        );
        assert_eq!(entry.request.child_path, root.path().join("new/child.wav"));
        assert!(!queue.order.is_empty(), "rekeyed edge must be rescheduled");
    }

    #[test]
    fn cross_source_in_flight_move_rekeys_endpoint_source_context() {
        let source_root = tempfile::tempdir().expect("source root");
        let destination_root = tempfile::tempdir().expect("destination root");
        let source = SampleSource::new_with_id(SourceId::new(), source_root.path().to_path_buf());
        let destination =
            SampleSource::new_with_id(SourceId::new(), destination_root.path().to_path_buf());
        let mut owner = HarvestSelectionDerivationOwner::new();
        let mut queued = request(source_root.path());
        queued.source = source.clone();
        queued.child_source = source;
        owner.enqueue(queued);
        let claimed = owner.queue.lock().expect("queue lock").claim_all();
        assert_eq!(claimed.len(), 1);

        let old_path = source_root.path().join("old/source.wav");
        let new_path = destination_root.path().join("new/source.wav");
        owner.rekey_file_cross_source(&old_path, &new_path, destination.clone());

        let queue = owner.queue.lock().expect("queue lock");
        let entry = queue.entries.values().next().expect("rekeyed request");
        assert_eq!(entry.state, HarvestSelectionDerivationEntryState::Pending);
        assert!(entry.revision > claimed[0].revision);
        assert_eq!(entry.request.source_path, new_path);
        assert_eq!(entry.request.source.id, destination.id);
        assert_eq!(entry.request.source.root, destination.root);
        assert_ne!(entry.request.child_source.id, destination.id);
    }

    #[test]
    fn shutdown_flush_attempts_admitted_request_and_closes_queue() {
        let root = tempfile::tempdir().expect("root");
        let mut owner = HarvestSelectionDerivationOwner::new();
        owner.enqueue(request(root.path()));
        let unflushed = owner.close();
        assert_eq!(unflushed, 0);
        assert!(owner.queue.lock().expect("queue lock").closed);
        assert!(owner.queue.lock().expect("queue lock").entries.is_empty());
    }

    #[test]
    fn worker_move_boundary_persists_only_remapped_harvest_edge() {
        let config_base = tempfile::tempdir().expect("config base");
        let _config_guard =
            wavecrate::app_dirs::ConfigBaseGuard::set(config_base.path().to_path_buf());
        let source_root = tempfile::tempdir().expect("source root");
        let source = SampleSource::new_with_id(SourceId::new(), source_root.path().to_path_buf());
        let old_parent_path = source_root.path().join("old/source.wav");
        let old_child_path = source_root.path().join("old/child.wav");
        let derivation_request = HarvestSelectionDerivationRequest {
            source_path: old_parent_path.clone(),
            child_path: old_child_path.clone(),
            source: source.clone(),
            child_source: source.clone(),
            selection: SelectionRange::new(0.25, 0.75),
            source_duration_seconds: 4.0,
            operation: HarvestDerivationOperation::Extract,
            inherited_tags: Vec::new(),
        };

        let (validated_tx, validated_rx) = std::sync::mpsc::channel();
        let (release_tx, release_channel_rx) = std::sync::mpsc::channel();
        let release_rx = Arc::new(Mutex::new(release_channel_rx));
        let hook = Arc::new(move || {
            validated_tx.send(()).expect("worker validation rendezvous");
            release_rx
                .lock()
                .expect("worker release lock")
                .recv()
                .expect("worker release rendezvous");
        });
        *HARVEST_SELECTION_DERIVATION_TEST_BOUNDARY_HOOK
            .lock()
            .expect("install test hook") = Some(hook);
        struct BoundaryHookGuard;
        impl Drop for BoundaryHookGuard {
            fn drop(&mut self) {
                *HARVEST_SELECTION_DERIVATION_TEST_BOUNDARY_HOOK
                    .lock()
                    .expect("clear test hook") = None;
            }
        }
        let _hook_guard = BoundaryHookGuard;

        let mut owner = HarvestSelectionDerivationOwner::new();
        owner.enqueue(derivation_request);
        let worker_queue = Arc::clone(&owner.queue);
        let worker_gate = Arc::clone(&owner.persist_gate);
        let worker_config_base = config_base.path().to_path_buf();
        let worker = std::thread::spawn(move || {
            let _worker_config_guard =
                wavecrate::app_dirs::ConfigBaseGuard::set(worker_config_base);
            persist_harvest_selection_derivation_queue(worker_queue, worker_gate)
        });

        validated_rx
            .recv_timeout(std::time::Duration::from_secs(2))
            .expect("worker must hold old validated request");

        let (move_started_tx, move_started_rx) = std::sync::mpsc::channel();
        let move_owner = owner.clone();
        let move_source_id = source.id.clone();
        let move_config_base = config_base.path().to_path_buf();
        let old_abs_prefix = source_root.path().join("old");
        let new_abs_prefix = source_root.path().join("new");
        let old_prefix = std::path::PathBuf::from("old");
        let new_prefix = std::path::PathBuf::from("new");
        let mover = std::thread::spawn(move || {
            let _move_config_guard = wavecrate::app_dirs::ConfigBaseGuard::set(move_config_base);
            move_started_tx.send(()).expect("move rendezvous");
            move_owner.rekey_prefix(&old_abs_prefix, &new_abs_prefix);
            wavecrate::sample_sources::library::remap_harvest_file_prefix(
                &move_source_id,
                &old_prefix,
                &new_prefix,
            )
            .expect("remap harvest graph");
        });
        move_started_rx
            .recv_timeout(std::time::Duration::from_secs(2))
            .expect("move must reach rekey boundary");
        release_tx.send(()).expect("release worker");

        let worker_result = worker.join().expect("worker join");
        assert_eq!(worker_result.results.len(), 1);
        assert!(worker_result.results[0].result.is_ok());
        mover.join().expect("move join");

        let old_parent_key =
            HarvestFileKey::new(source.id.clone(), PathBuf::from("old/source.wav"));
        let old_child_key = HarvestFileKey::new(source.id.clone(), PathBuf::from("old/child.wav"));
        let new_parent_key =
            HarvestFileKey::new(source.id.clone(), PathBuf::from("new/source.wav"));
        let new_child_key = HarvestFileKey::new(source.id.clone(), PathBuf::from("new/child.wav"));
        let new_parent_edges =
            wavecrate::sample_sources::library::harvest_derivations_for_parent(&new_parent_key)
                .expect("load remapped parent edges");
        assert_eq!(new_parent_edges.len(), 1);
        assert_eq!(new_parent_edges[0].parent.key, new_parent_key);
        assert_eq!(new_parent_edges[0].child.key, new_child_key);
        assert!(
            wavecrate::sample_sources::library::harvest_derivations_for_parent(&old_parent_key,)
                .expect("load stale parent edges")
                .is_empty()
        );
        assert!(
            wavecrate::sample_sources::library::harvest_parents_for_child(&old_child_key)
                .expect("load stale child parents")
                .is_empty()
        );
        let new_child_parents =
            wavecrate::sample_sources::library::harvest_parents_for_child(&new_child_key)
                .expect("load remapped child parents");
        assert_eq!(new_child_parents.len(), 1);
        assert_eq!(new_child_parents[0].parent.key, new_parent_key);
    }
}

struct HarvestTouchedPersistQueue {
    entries: std::collections::HashMap<HarvestTouchedPersistKey, HarvestTouchedPersistEntry>,
    order: VecDeque<HarvestTouchedPersistKey>,
    next_revision: u64,
    closed: bool,
}

struct HarvestTouchedPersistEntry {
    request: HarvestTouchedPersistRequest,
    revision: u64,
    state: HarvestTouchedPersistEntryState,
    retry_blocked: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum HarvestTouchedPersistEntryState {
    Pending,
    InFlight,
}

struct HarvestTouchedPersistWork {
    key: HarvestTouchedPersistKey,
    request: HarvestTouchedPersistRequest,
    revision: u64,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(in crate::native_app) struct HarvestTouchedPersistBatchResult {
    pub(in crate::native_app) results: Vec<HarvestTouchedPersistBatchItem>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(in crate::native_app) struct HarvestTouchedPersistBatchItem {
    pub(in crate::native_app) key: HarvestTouchedPersistKey,
    pub(in crate::native_app) revision: u64,
    pub(in crate::native_app) result: Option<HarvestTouchedPersistResult>,
}

impl HarvestTouchedPersistOwner {
    fn new() -> Self {
        Self {
            task: ui::LatestTask::new(),
            admission_receipt: None,
            admission_poll_task: ui::LatestTask::new(),
            admission_retry_used: false,
            queue: Arc::new(Mutex::new(HarvestTouchedPersistQueue {
                entries: std::collections::HashMap::new(),
                order: VecDeque::new(),
                next_revision: 1,
                closed: false,
            })),
        }
    }

    pub(in crate::native_app) fn enqueue(
        &mut self,
        request: HarvestTouchedPersistRequest,
    ) -> HarvestTouchedPersistAdmission {
        let key = HarvestTouchedPersistKey::from_request(&request);
        let Ok(mut queue) = self.queue.lock() else {
            tracing::warn!("harvest touched queue lock poisoned; rejecting request");
            return HarvestTouchedPersistAdmission::Saturated;
        };
        if queue.closed {
            return HarvestTouchedPersistAdmission::Saturated;
        }
        let revision = next_harvest_touched_revision(&mut queue);
        if let Some(entry) = queue.entries.get_mut(&key) {
            // A coalesced enqueue explicitly re-arms one admission recovery attempt.
            self.admission_retry_used = false;
            let was_retry_blocked = entry.retry_blocked;
            entry.request = request;
            entry.revision = revision;
            entry.retry_blocked = false;
            if entry.state == HarvestTouchedPersistEntryState::InFlight {
                entry.state = HarvestTouchedPersistEntryState::Pending;
                queue.order.push_back(key);
            } else if was_retry_blocked {
                queue.order.push_back(key);
            }
            return HarvestTouchedPersistAdmission::Replaced;
        }
        if queue.entries.len() >= HARVEST_TOUCHED_QUEUE_CAPACITY {
            return HarvestTouchedPersistAdmission::Saturated;
        }
        // A fresh enqueue explicitly re-arms one admission recovery attempt.
        self.admission_retry_used = false;
        queue.order.push_back(key.clone());
        queue.entries.insert(
            key,
            HarvestTouchedPersistEntry {
                request,
                revision,
                state: HarvestTouchedPersistEntryState::Pending,
                retry_blocked: false,
            },
        );
        HarvestTouchedPersistAdmission::Queued
    }

    pub(in crate::native_app) fn schedule_if_idle(
        &mut self,
        context: &mut ui::UiUpdateContext<GuiMessage>,
    ) {
        if self.task.active().is_some()
            || self
                .queue
                .lock()
                .ok()
                .is_none_or(|queue| !queue.has_pending())
        {
            return;
        }
        let queue = Arc::clone(&self.queue);
        let request = context
            .business()
            .blocking_io("gui-harvest-touched-persist")
            .latest(&mut self.task);
        let ticket = request.ticket();
        let receipt = request.run_with_receipt(
            move |_| persist_harvest_touched_queue(queue),
            GuiMessage::HarvestTouchedPersisted,
        );
        self.admission_receipt = Some(HarvestTouchedPersistAdmissionReceipt { ticket, receipt });
        self.schedule_admission_poll(context);
    }

    fn schedule_admission_poll(&mut self, context: &mut ui::UiUpdateContext<GuiMessage>) {
        if self.admission_poll_task.active().is_some() {
            return;
        }
        context.after_latest(
            &mut self.admission_poll_task,
            HARVEST_TOUCHED_ADMISSION_POLL_DELAY,
            GuiMessage::HarvestTouchedPersistAdmissionPoll,
        );
    }

    pub(in crate::native_app) fn poll_admission(
        &mut self,
        ticket: ui::TaskTicket,
        context: &mut ui::UiUpdateContext<GuiMessage>,
    ) {
        if !self.admission_poll_task.finish(ticket) {
            return;
        }
        let Some(admission) = self.admission_receipt.as_ref() else {
            // This is the one delayed recovery turn after a rejected admission.
            self.schedule_if_idle(context);
            return;
        };
        let state = admission.receipt.poll();
        match state {
            ui::BusinessTaskAdmission::Pending => self.schedule_admission_poll(context),
            ui::BusinessTaskAdmission::Accepted => {}
            ui::BusinessTaskAdmission::Rejected | ui::BusinessTaskAdmission::Closed => {
                let rejected = state == ui::BusinessTaskAdmission::Rejected;
                self.admission_receipt = None;
                if rejected {
                    if self.admission_retry_used {
                        self.block_admission_retry_queue();
                    } else {
                        // The worker closure claims entries only after host admission. A
                        // rejected receipt therefore leaves the queue untouched; arm exactly
                        // one delayed recovery turn without recursively submitting a worker.
                        self.admission_retry_used = true;
                        self.schedule_admission_poll(context);
                    }
                }
            }
        }
    }

    pub(in crate::native_app) fn finish(
        &mut self,
        completion: ui::TaskCompletion<HarvestTouchedPersistBatchResult>,
    ) -> Option<HarvestTouchedPersistBatchResult> {
        let completion_ticket = completion.ticket;
        let result = self.task.finish_completion(completion)?;
        if self
            .admission_receipt
            .as_ref()
            .is_some_and(|admission| admission.ticket == completion_ticket)
        {
            self.admission_receipt = None;
            self.admission_poll_task.cancel();
            self.admission_retry_used = false;
        }
        if let Ok(mut queue) = self.queue.lock() {
            for item in &result.results {
                queue.acknowledge(item);
            }
        }
        Some(result)
    }

    pub(in crate::native_app) fn invalidate(&mut self, request: &HarvestTouchedPersistRequest) {
        let key = HarvestTouchedPersistKey::from_request(request);
        if let Ok(mut queue) = self.queue.lock() {
            queue.entries.remove(&key);
            queue.order.retain(|candidate| candidate != &key);
        }
    }

    fn block_admission_retry_queue(&mut self) {
        let Ok(mut queue) = self.queue.lock() else {
            return;
        };
        for entry in queue.entries.values_mut() {
            entry.retry_blocked = true;
            entry.state = HarvestTouchedPersistEntryState::Pending;
        }
    }

    pub(in crate::native_app) fn close(&mut self) -> usize {
        self.task.cancel();
        self.admission_poll_task.cancel();
        self.admission_receipt = None;
        let Ok(mut queue) = self.queue.lock() else {
            return 0;
        };
        queue.closed = true;
        let count = queue.entries.len();
        queue.entries.clear();
        queue.order.clear();
        count
    }
}

fn persist_harvest_touched_queue(
    queue: Arc<Mutex<HarvestTouchedPersistQueue>>,
) -> HarvestTouchedPersistBatchResult {
    let work = queue
        .lock()
        .map(|mut queue| queue.claim_batch())
        .unwrap_or_default();
    let mut results = Vec::with_capacity(work.len());
    for item in work {
        let key = item.key.clone();
        let revision = item.revision;
        let current_queue = Arc::clone(&queue);
        let result = persist_harvest_touched_if_current(item.request, move || {
            current_queue
                .lock()
                .ok()
                .is_some_and(|queue| queue.is_current(&key, revision))
        });
        results.push(HarvestTouchedPersistBatchItem {
            key: item.key,
            revision,
            result,
        });
    }
    HarvestTouchedPersistBatchResult { results }
}

fn next_harvest_touched_revision(queue: &mut HarvestTouchedPersistQueue) -> u64 {
    let revision = queue.next_revision;
    queue.next_revision = queue.next_revision.saturating_add(1);
    revision
}

impl HarvestTouchedPersistKey {
    fn from_request(request: &HarvestTouchedPersistRequest) -> Self {
        Self {
            source_id: request.source.id.clone(),
            relative_path: request.relative_path.clone(),
        }
    }
}

impl HarvestTouchedPersistQueue {
    fn has_pending(&self) -> bool {
        self.entries.values().any(|entry| {
            entry.state == HarvestTouchedPersistEntryState::Pending && !entry.retry_blocked
        })
    }

    fn claim_batch(&mut self) -> Vec<HarvestTouchedPersistWork> {
        let mut claimed = Vec::with_capacity(HARVEST_TOUCHED_BATCH_LIMIT);
        while claimed.len() < HARVEST_TOUCHED_BATCH_LIMIT {
            let Some(key) = self.order.pop_front() else {
                break;
            };
            let Some(entry) = self.entries.get_mut(&key) else {
                continue;
            };
            if entry.state != HarvestTouchedPersistEntryState::Pending || entry.retry_blocked {
                continue;
            }
            entry.state = HarvestTouchedPersistEntryState::InFlight;
            claimed.push(HarvestTouchedPersistWork {
                key,
                request: entry.request.clone(),
                revision: entry.revision,
            });
        }
        claimed
    }

    fn is_current(&self, key: &HarvestTouchedPersistKey, revision: u64) -> bool {
        self.entries.get(key).is_some_and(|entry| {
            entry.revision == revision && entry.state == HarvestTouchedPersistEntryState::InFlight
        })
    }

    fn acknowledge(&mut self, item: &HarvestTouchedPersistBatchItem) {
        let Some(entry) = self.entries.get_mut(&item.key) else {
            return;
        };
        if entry.revision != item.revision
            || entry.state != HarvestTouchedPersistEntryState::InFlight
        {
            return;
        }
        if item
            .result
            .as_ref()
            .is_some_and(|result| result.result.is_ok())
        {
            self.entries.remove(&item.key);
        } else {
            entry.state = HarvestTouchedPersistEntryState::Pending;
            entry.retry_blocked = true;
            self.order.push_back(item.key.clone());
        }
    }
}

#[cfg(test)]
mod harvest_touched_owner_tests {
    use super::*;
    use radiant::prelude::IntoView;

    struct RejectingWorkerBridge;

    impl radiant::runtime::RuntimeBridge<GuiMessage> for RejectingWorkerBridge {
        fn project_surface(&mut self) -> std::sync::Arc<radiant::runtime::UiSurface<GuiMessage>> {
            std::sync::Arc::new(radiant::prelude::empty::<GuiMessage>().into_surface())
        }

        fn host_capabilities(&self) -> radiant::runtime::RuntimeHostCapabilities<Self, GuiMessage> {
            radiant::runtime::RuntimeHostCapabilities::new().with_tasks()
        }
    }

    impl radiant::runtime::RuntimeTaskHost<GuiMessage> for RejectingWorkerBridge {
        fn schedule_timer(
            &mut self,
            _delay: Duration,
            _wake: radiant::runtime::RuntimeTimerWake,
        ) -> bool {
            true
        }

        fn spawn_worker_task(
            &mut self,
            _name: &'static str,
            _priority: radiant::prelude::TaskPriority,
            _is_cancelled: Option<Box<dyn Fn() -> bool + Send + Sync + 'static>>,
            _work: Box<dyn FnOnce() + Send + 'static>,
        ) -> bool {
            false
        }
    }

    fn request(source_id: &str, path: &str) -> HarvestTouchedPersistRequest {
        HarvestTouchedPersistRequest {
            file_id: format!("/tmp/{source_id}/{path}"),
            source: wavecrate::sample_sources::SampleSource::new_with_id(
                SourceId::from_string(source_id),
                PathBuf::from(format!("/tmp/{source_id}")),
            ),
            relative_path: PathBuf::from(path),
        }
    }

    fn successful(item: &HarvestTouchedPersistWork) -> HarvestTouchedPersistBatchItem {
        HarvestTouchedPersistBatchItem {
            key: item.key.clone(),
            revision: item.revision,
            result: Some(HarvestTouchedPersistResult {
                file_id: item.request.file_id.clone(),
                result: Ok(()),
            }),
        }
    }

    fn failed(item: &HarvestTouchedPersistWork) -> HarvestTouchedPersistBatchItem {
        HarvestTouchedPersistBatchItem {
            key: item.key.clone(),
            revision: item.revision,
            result: Some(HarvestTouchedPersistResult {
                file_id: item.request.file_id.clone(),
                result: Err(String::from("transient")),
            }),
        }
    }

    #[test]
    fn pending_enqueue_coalesces_to_latest_revision() {
        let mut owner = HarvestTouchedPersistOwner::new();
        assert_eq!(
            owner.enqueue(request("source", "kick.wav")),
            HarvestTouchedPersistAdmission::Queued
        );
        assert_eq!(
            owner.enqueue(request("source", "kick.wav")),
            HarvestTouchedPersistAdmission::Replaced
        );

        let work = owner.queue.lock().expect("queue lock").claim_batch();
        assert_eq!(work.len(), 1);
        assert_eq!(work[0].request.file_id, "/tmp/source/kick.wav");
        assert_eq!(work[0].revision, 2);
    }

    #[test]
    fn inflight_enqueue_supersedes_without_losing_capacity() {
        let mut owner = HarvestTouchedPersistOwner::new();
        owner.enqueue(request("source", "kick.wav"));
        let first = owner.queue.lock().expect("queue lock").claim_batch();
        assert_eq!(first.len(), 1);

        assert_eq!(
            owner.enqueue(request("source", "kick.wav")),
            HarvestTouchedPersistAdmission::Replaced
        );
        let queue = owner.queue.lock().expect("queue lock");
        assert_eq!(queue.entries.len(), 1);
        assert!(!queue.is_current(&first[0].key, first[0].revision));
        drop(queue);
        assert_eq!(
            owner.queue.lock().expect("queue lock").claim_batch().len(),
            1
        );
    }

    #[test]
    fn stale_completion_cannot_remove_replacement() {
        let mut owner = HarvestTouchedPersistOwner::new();
        owner.enqueue(request("source", "kick.wav"));
        let first = owner.queue.lock().expect("queue lock").claim_batch();
        owner.enqueue(request("source", "kick.wav"));
        owner
            .queue
            .lock()
            .expect("queue lock")
            .acknowledge(&successful(&first[0]));
        assert_eq!(owner.queue.lock().expect("queue lock").entries.len(), 1);
    }

    #[test]
    fn capacity_counts_inflight_and_pending_entries() {
        let mut owner = HarvestTouchedPersistOwner::new();
        for index in 0..HARVEST_TOUCHED_QUEUE_CAPACITY {
            assert_eq!(
                owner.enqueue(request("source", &format!("{index}.wav"))),
                HarvestTouchedPersistAdmission::Queued
            );
        }
        let _ = owner.queue.lock().expect("queue lock").claim_batch();
        assert_eq!(
            owner.enqueue(request("source", "overflow.wav")),
            HarvestTouchedPersistAdmission::Saturated
        );
        assert_eq!(
            owner.enqueue(request("source", "0.wav")),
            HarvestTouchedPersistAdmission::Replaced
        );
    }

    #[test]
    fn finite_batches_preserve_fifo_and_limit() {
        let mut owner = HarvestTouchedPersistOwner::new();
        for index in 0..(HARVEST_TOUCHED_BATCH_LIMIT + 1) {
            owner.enqueue(request("source", &format!("{index}.wav")));
        }
        let mut queue = owner.queue.lock().expect("queue lock");
        let first = queue.claim_batch();
        assert_eq!(first.len(), HARVEST_TOUCHED_BATCH_LIMIT);
        assert_eq!(first[0].request.relative_path, PathBuf::from("0.wav"));
        assert_eq!(queue.claim_batch().len(), 1);
    }

    #[test]
    fn close_rejects_late_enqueue_and_reports_admitted_work() {
        let mut owner = HarvestTouchedPersistOwner::new();
        owner.enqueue(request("source", "kick.wav"));
        assert_eq!(owner.close(), 1);
        assert_eq!(
            owner.enqueue(request("source", "snare.wav")),
            HarvestTouchedPersistAdmission::Saturated
        );
    }

    #[test]
    fn failed_revision_is_retained_without_immediate_retry_loop() {
        let mut owner = HarvestTouchedPersistOwner::new();
        owner.enqueue(request("source", "kick.wav"));
        let work = owner.queue.lock().expect("queue lock").claim_batch();
        owner
            .queue
            .lock()
            .expect("queue lock")
            .acknowledge(&failed(&work[0]));
        let queue = owner.queue.lock().expect("queue lock");
        assert_eq!(queue.entries.len(), 1);
        assert!(!queue.has_pending());
    }

    #[test]
    fn harvest_persist_uses_blocking_io_lane() {
        let mut owner = HarvestTouchedPersistOwner::new();
        owner.enqueue(request("source", "kick.wav"));
        let mut context = ui::UiUpdateContext::default();
        owner.schedule_if_idle(&mut context);
        assert_eq!(
            context
                .into_command()
                .business_task_priority("gui-harvest-touched-persist"),
            Some(ui::TaskPriority::BlockingIo)
        );
    }

    #[test]
    fn rejected_receipt_retries_later_without_reclaiming_before_admission() {
        let mut owner = HarvestTouchedPersistOwner::new();
        owner.enqueue(request("source", "kick.wav"));
        let mut context = ui::UiUpdateContext::default();
        owner.schedule_if_idle(&mut context);
        let command = context.into_command();

        let mut runtime = radiant::runtime::SurfaceRuntime::new(
            RejectingWorkerBridge,
            radiant::gui::types::Vector2::new(80.0, 40.0),
        );
        runtime.execute_command(command);
        let poll_ticket = owner
            .admission_poll_task
            .active()
            .expect("admission poll timer should be retained");
        assert!(owner.queue.lock().expect("queue lock").has_pending());

        let mut delayed_retry_context = ui::UiUpdateContext::default();
        owner.poll_admission(poll_ticket, &mut delayed_retry_context);
        let delayed_retry = delayed_retry_context.into_command();
        assert_eq!(
            delayed_retry.business_task_priority("gui-harvest-touched-persist"),
            None
        );

        let retry_poll_ticket = owner
            .admission_poll_task
            .active()
            .expect("one delayed recovery poll should be retained");
        let mut retry_context = ui::UiUpdateContext::default();
        owner.poll_admission(retry_poll_ticket, &mut retry_context);
        let retry_command = retry_context.into_command();
        assert_eq!(
            retry_command.business_task_priority("gui-harvest-touched-persist"),
            Some(ui::TaskPriority::BlockingIo)
        );
        assert!(owner.task.active().is_some());
        assert!(owner.queue.lock().expect("queue lock").has_pending());

        runtime.execute_command(retry_command);
        let second_reject_poll = owner
            .admission_poll_task
            .active()
            .expect("second admission should arm one poll");
        let mut blocked_context = ui::UiUpdateContext::default();
        owner.poll_admission(second_reject_poll, &mut blocked_context);
        assert_eq!(
            blocked_context
                .into_command()
                .business_task_priority("gui-harvest-touched-persist"),
            None
        );
        assert!(owner.task.active().is_none());
        assert!(!owner.queue.lock().expect("queue lock").has_pending());

        owner.enqueue(request("source", "snare.wav"));
        let mut rearm_context = ui::UiUpdateContext::default();
        owner.schedule_if_idle(&mut rearm_context);
        assert_eq!(
            rearm_context
                .into_command()
                .business_task_priority("gui-harvest-touched-persist"),
            Some(ui::TaskPriority::BlockingIo)
        );
    }

    #[test]
    fn stale_receipt_poll_and_completion_cannot_replace_latest_work() {
        let mut owner = HarvestTouchedPersistOwner::new();
        owner.enqueue(request("source", "kick.wav"));
        let first = owner.queue.lock().expect("queue lock").claim_batch();
        owner.enqueue(request("source", "kick.wav"));
        let stale = HarvestTouchedPersistBatchResult {
            results: vec![successful(&first[0])],
        };
        let stale_ticket = owner.task.begin();
        let _latest_ticket = owner.task.begin();
        let stale_completion = ui::TaskCompletion {
            ticket: stale_ticket,
            output: stale,
        };
        assert!(owner.finish(stale_completion).is_none());
        let stale_poll_ticket = owner.admission_poll_task.begin();
        let _latest_poll_ticket = owner.admission_poll_task.begin();
        owner.poll_admission(stale_poll_ticket, &mut ui::UiUpdateContext::default());
        assert!(owner.admission_poll_task.active().is_some());
        assert_eq!(owner.queue.lock().expect("queue lock").entries.len(), 1);
    }
}

#[derive(Clone, Debug)]
pub(in crate::native_app) struct WaveformDestructiveEditUiContext {
    pub(in crate::native_app) request: PendingWaveformDestructiveEdit,
    pub(in crate::native_app) playback_was_active: bool,
    pub(in crate::native_app) source_duration_seconds: Option<f64>,
    pub(in crate::native_app) extracted_playback_type: ExtractedFilePlaybackType,
    pub(in crate::native_app) preserved_marks: Option<WaveformPreservedMarks>,
    pub(in crate::native_app) output_focus_path: Option<std::path::PathBuf>,
    pub(in crate::native_app) harvest_whole_file_derivation: Option<(
        std::path::PathBuf,
        wavecrate::sample_sources::HarvestDerivationOperation,
    )>,
}

/// Owns audio-output open task identity and stale-completion policy.
pub(in crate::native_app) struct AudioOpenTaskOwner {
    task: ui::LatestTask,
}

impl AudioOpenTaskOwner {
    fn new() -> Self {
        Self {
            task: ui::LatestTask::new(),
        }
    }

    pub(in crate::native_app) fn active(&self) -> Option<ui::TaskTicket> {
        self.task.active()
    }

    pub(in crate::native_app) fn begin(&mut self) -> AudioOpenTaskRequest {
        AudioOpenTaskRequest {
            ticket: self.task.begin(),
        }
    }

    pub(in crate::native_app) fn cancel(&mut self) {
        self.task.cancel();
    }

    pub(in crate::native_app) fn finish(
        &mut self,
        completion: AudioOpenTaskCompletion,
    ) -> AudioOpenCompletion {
        if !self.task.finish(completion.ticket()) {
            return AudioOpenCompletion::Stale;
        }
        AudioOpenCompletion::Current(Box::new(
            completion
                .take_result()
                .unwrap_or_else(|| Err(String::from("audio output worker did not report"))),
        ))
    }
}

/// Cloneable message payload for a non-cloneable audio-open result.
#[derive(Clone)]
pub(in crate::native_app) struct AudioOpenTaskCompletion {
    ticket: ui::TaskTicket,
    result: Arc<Mutex<Option<Result<AudioPlayer, String>>>>,
}

impl AudioOpenTaskCompletion {
    pub(in crate::native_app) fn ticket(&self) -> ui::TaskTicket {
        self.ticket
    }

    fn take_result(self) -> Option<Result<AudioPlayer, String>> {
        self.result.lock().ok().and_then(|mut result| result.take())
    }
}

impl std::fmt::Debug for AudioOpenTaskCompletion {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("AudioOpenTaskCompletion")
            .field("ticket", &self.ticket)
            .finish_non_exhaustive()
    }
}

impl PartialEq for AudioOpenTaskCompletion {
    fn eq(&self, other: &Self) -> bool {
        self.ticket == other.ticket
    }
}

/// Worker-owned request token that can produce exactly one completion result.
pub(in crate::native_app) struct AudioOpenTaskRequest {
    ticket: ui::TaskTicket,
}

impl AudioOpenTaskRequest {
    pub(in crate::native_app) fn complete(
        self,
        result: Result<AudioPlayer, String>,
    ) -> AudioOpenTaskCompletion {
        AudioOpenTaskCompletion {
            ticket: self.ticket,
            result: Arc::new(Mutex::new(Some(result))),
        }
    }
}

pub(in crate::native_app) enum AudioOpenCompletion {
    Current(Box<Result<AudioPlayer, String>>),
    Stale,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn audio_open_task_owner_ignores_stale_ticket_results() {
        let mut owner = AudioOpenTaskOwner::new();
        let stale_completion = owner.begin().complete(Err(String::from("stale")));
        let current_completion = owner.begin().complete(Err(String::from("current")));

        assert!(matches!(
            owner.finish(stale_completion),
            AudioOpenCompletion::Stale
        ));
        assert!(
            matches!(owner.finish(current_completion), AudioOpenCompletion::Current(result) if result.as_ref().is_err())
        );
    }

    #[test]
    fn audio_open_task_completion_reports_missing_result_after_consumption() {
        let completion = AudioOpenTaskOwner::new()
            .begin()
            .complete(Err(String::from("reported")));
        let clone = completion.clone();

        assert!(matches!(
            completion.take_result(),
            Some(Err(error)) if error == "reported"
        ));
        assert!(clone.take_result().is_none());
    }
}
