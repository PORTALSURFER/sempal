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
    HarvestTouchedPersistRequest, HarvestTouchedPersistResult, SourceId,
    persist_harvest_touched_if_current,
};

use crate::native_app::app::GuiSourceProcessingEventSink;
use crate::native_app::app::{
    ExtractedFilePlaybackType, FileMoveProgress, GuiMessage, NormalizationProgress,
    NormalizationQueueItem, PendingWaveformDestructiveEdit, SourceProcessingHealth,
    SourceProcessingProgress,
};
use crate::native_app::source_processing::SourceProcessingSupervisor;
use crate::native_app::waveform::WaveformPreservedMarks;

pub(in crate::native_app) struct BackgroundTaskState {
    pub(in crate::native_app) worker_sender: Sender<GuiMessage>,
    pub(in crate::native_app) worker_receiver: Option<Receiver<GuiMessage>>,
    pub(in crate::native_app) next_task_id: u64,
    pub(in crate::native_app) sample_load_validation_task: ui::LatestTask,
    pub(in crate::native_app) deferred_sample_load_task: ui::LatestTask,
    pub(in crate::native_app) sample_load_tasks: ui::ResourceTasks,
    pub(in crate::native_app) harvest_touched_persist: HarvestTouchedPersistOwner,
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

impl BackgroundTaskState {
    pub(in crate::native_app) fn new(
        worker_sender: Sender<GuiMessage>,
        worker_receiver: Option<Receiver<GuiMessage>>,
        sources: Vec<wavecrate::sample_sources::SampleSource>,
    ) -> Self {
        #[cfg(not(test))]
        let source_processing = Self::start_source_processing(&worker_sender, sources);
        #[cfg(test)]
        let source_processing = {
            drop(sources);
            SourceProcessingSupervisor::dormant()
        };
        Self::with_source_processing(worker_sender, worker_receiver, source_processing)
    }

    #[cfg(any(test, feature = "legacy-controller"))]
    pub(in crate::native_app) fn new_runtime(
        worker_sender: Sender<GuiMessage>,
        worker_receiver: Option<Receiver<GuiMessage>>,
        sources: Vec<wavecrate::sample_sources::SampleSource>,
    ) -> Self {
        let source_processing = Self::start_source_processing(&worker_sender, sources);
        Self::with_source_processing(worker_sender, worker_receiver, source_processing)
    }

    fn start_source_processing(
        worker_sender: &Sender<GuiMessage>,
        sources: Vec<wavecrate::sample_sources::SampleSource>,
    ) -> SourceProcessingSupervisor {
        SourceProcessingSupervisor::start_with_event_sink(
            sources,
            GuiSourceProcessingEventSink::new(worker_sender.clone()),
        )
    }

    fn with_source_processing(
        worker_sender: Sender<GuiMessage>,
        worker_receiver: Option<Receiver<GuiMessage>>,
        source_processing: SourceProcessingSupervisor,
    ) -> Self {
        let source_lifecycle_generations = source_processing.lifecycle_generations();
        Self {
            worker_sender,
            worker_receiver,
            next_task_id: 1,
            sample_load_validation_task: ui::LatestTask::new(),
            deferred_sample_load_task: ui::LatestTask::new(),
            sample_load_tasks: ui::ResourceTasks::new(),
            harvest_touched_persist: HarvestTouchedPersistOwner::new(),
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
        Self::new(std::sync::mpsc::channel().0, None, Vec::new())
    }
}

const HARVEST_TOUCHED_QUEUE_CAPACITY: usize = 64;
const HARVEST_TOUCHED_BATCH_LIMIT: usize = 32;
const HARVEST_TOUCHED_ADMISSION_POLL_DELAY: Duration = Duration::from_millis(50);

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
