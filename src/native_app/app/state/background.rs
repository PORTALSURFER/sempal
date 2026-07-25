use std::{
    collections::{BTreeMap, HashSet, VecDeque},
    path::PathBuf,
    sync::{
        Arc, Mutex,
        atomic::AtomicU64,
        mpsc::{Receiver, Sender},
    },
};

use radiant::{gui::frame as frame_ui, prelude as ui};
use wavecrate::audio::AudioPlayer;

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
