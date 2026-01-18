use super::ScanJobMessage;
use super::library::analysis_jobs::AnalysisJobMessage;
use super::library::trash_move;
use super::playback::audio_loader::{AudioLoadJob, AudioLoadResult};
use super::source_watcher::{SourceWatchCommand, SourceWatchEntry, SourceWatchEvent};
use super::state::audio::{PendingAudio, PendingPlayback};
use super::state::runtime::{UpdateCheckResult, WavLoadJob, WavLoadResult};
use crate::sample_sources::SourceId;
use std::{
    collections::BTreeSet,
    path::PathBuf,
    sync::{
        Arc, Mutex,
        atomic::AtomicBool,
        mpsc::{Receiver, Sender},
    },
    thread,
};

type TryRecvError = std::sync::mpsc::TryRecvError;

#[cfg_attr(test, allow(dead_code))]
pub(crate) enum JobMessage {
    WavLoaded(WavLoadResult),
    AudioLoaded(AudioLoadResult),
    Scan(ScanJobMessage),
    SourceWatch(SourceWatchEvent),
    TrashMove(trash_move::TrashMoveMessage),
    CollectionMove(CollectionMoveResult),
    Analysis(AnalysisJobMessage),
    AnalysisFailuresLoaded(AnalysisFailuresResult),
    UmapBuilt(UmapBuildResult),
    UmapClustersBuilt(UmapClusterBuildResult),
    SimilarityPrepared(SimilarityPrepResult),
    UpdateChecked(UpdateCheckResult),
    IssueGatewayCreated(IssueGatewayCreateResult),
    IssueGatewayAuthed(IssueGatewayAuthResult),
    BrowserSearchFinished(SearchResult),
    Normalized(NormalizationResult),
}

#[derive(Debug)]
pub(crate) struct SearchJob {
    pub(super) source_id: SourceId,
    pub(super) source_root: PathBuf,
    pub(super) query: String,
    pub(super) filter: crate::egui_app::state::TriageFlagFilter,
    pub(super) sort: crate::egui_app::state::SampleBrowserSort,
    pub(super) similar_query: Option<crate::egui_app::state::SimilarQuery>,
    pub(super) folder_selection: Option<BTreeSet<PathBuf>>,
    pub(super) folder_negated: Option<BTreeSet<PathBuf>>,
}

#[derive(Debug)]
pub(crate) struct SearchResult {
    pub(crate) source_id: SourceId,
    pub(crate) query: String,
    pub(crate) visible: crate::egui_app::state::VisibleRows,
    pub(crate) trash: Vec<usize>,
    pub(crate) neutral: Vec<usize>,
    pub(crate) keep: Vec<usize>,
    pub(crate) scores: Vec<Option<i64>>,
}

#[derive(Debug)]
pub(crate) struct IssueGatewayJob {
    pub(crate) token: String,
    pub(crate) request: crate::issue_gateway::api::CreateIssueRequest,
}

#[derive(Debug)]
pub(crate) struct IssueGatewayPollJob {
    pub(crate) request_id: String,
}

#[derive(Debug)]
pub(crate) struct IssueGatewayCreateResult {
    pub(crate) result: Result<
        crate::issue_gateway::api::CreateIssueResponse,
        crate::issue_gateway::api::CreateIssueError,
    >,
}

#[derive(Debug)]
pub(crate) struct IssueGatewayAuthResult {
    pub(crate) result: Result<String, crate::issue_gateway::api::IssueAuthError>,
}

#[derive(Debug, Clone)]
pub(crate) struct UmapBuildJob {
    pub(super) model_id: String,
    pub(super) umap_version: String,
    pub(super) source_id: SourceId,
}

#[derive(Debug)]
pub(crate) struct UmapBuildResult {
    pub(super) umap_version: String,
    pub(super) result: Result<(), String>,
}

#[derive(Debug, Clone)]
pub(crate) struct UmapClusterBuildJob {
    pub(super) model_id: String,
    pub(super) umap_version: String,
    pub(super) source_id: Option<SourceId>,
}

#[derive(Debug)]
pub(crate) struct UmapClusterBuildResult {
    #[allow(dead_code)]
    pub(super) umap_version: String,
    pub(super) source_id: Option<SourceId>,
    pub(super) result: Result<crate::analysis::hdbscan::HdbscanStats, String>,
}

#[derive(Debug)]
pub(crate) struct SimilarityPrepOutcome {
    pub(crate) cluster_stats: crate::analysis::hdbscan::HdbscanStats,
    #[allow(dead_code)]
    pub(super) umap_version: String,
}

#[derive(Debug)]
pub(crate) struct SimilarityPrepResult {
    pub(crate) source_id: SourceId,
    pub(crate) result: Result<SimilarityPrepOutcome, String>,
}

#[derive(Debug)]
pub(crate) struct CollectionMoveSuccess {
    pub(crate) source_id: SourceId,
    pub(crate) relative_path: PathBuf,
    pub(crate) clip_root: PathBuf,
    pub(crate) clip_relative: PathBuf,
}

#[derive(Debug)]
pub(crate) struct CollectionMoveResult {
    pub(crate) collection_id: crate::sample_sources::CollectionId,
    pub(crate) moved: Vec<CollectionMoveSuccess>,
    pub(crate) errors: Vec<String>,
}

#[derive(Debug)]
pub(crate) struct AnalysisFailuresResult {
    pub(crate) source_id: SourceId,
    pub(crate) result: Result<std::collections::HashMap<PathBuf, String>, String>,
}

#[derive(Debug)]
pub(crate) struct NormalizationJob {
    pub(crate) source: crate::sample_sources::SampleSource,
    pub(crate) relative_path: PathBuf,
    pub(crate) absolute_path: PathBuf,
}

#[derive(Debug)]
pub(crate) struct NormalizationResult {
    pub(crate) source_id: crate::sample_sources::SourceId,
    pub(crate) relative_path: PathBuf,
    pub(crate) result: Result<(u64, i64, crate::sample_sources::Rating), String>,
}

pub(crate) struct ControllerJobs {
    pub(crate) wav_job_tx: Sender<WavLoadJob>,
    pub(crate) audio_job_tx: Sender<AudioLoadJob>,
    pub(crate) search_job_tx: Sender<SearchJob>,
    source_watch_tx: Sender<SourceWatchCommand>,
    message_tx: Sender<JobMessage>,
    message_rx: Receiver<JobMessage>,
    pub(super) pending_source: Option<SourceId>,
    pub(super) pending_select_path: Option<PathBuf>,
    pub(super) pending_audio: Option<PendingAudio>,
    pub(super) pending_playback: Option<PendingPlayback>,
    pub(super) next_audio_request_id: u64,
    pub(super) scan_in_progress: bool,
    pub(super) scan_cancel: Option<Arc<std::sync::atomic::AtomicBool>>,
    pub(super) trash_move_in_progress: bool,
    pub(super) trash_move_cancel: Option<Arc<std::sync::atomic::AtomicBool>>,
    pub(super) collection_move_in_progress: bool,
    pub(super) umap_build_in_progress: bool,
    pub(super) umap_cluster_build_in_progress: bool,
    pub(super) update_check_in_progress: bool,
    pub(super) issue_gateway_in_progress: bool,
    pub(super) issue_gateway_auth_in_progress: bool,
    pub(super) issue_gateway_poll_in_progress: bool,
    pub(super) issue_gateway_poll_cancel: Option<Arc<std::sync::atomic::AtomicBool>>,
    pub(super) repaint_signal: Arc<Mutex<Option<egui::Context>>>,
}

impl ControllerJobs {
    pub(super) fn new(
        wav_job_tx: Sender<WavLoadJob>,
        wav_job_rx: Receiver<WavLoadResult>,
        audio_job_tx: Sender<AudioLoadJob>,
        audio_job_rx: Receiver<AudioLoadResult>,
        search_job_tx: Sender<SearchJob>,
        search_job_rx: Receiver<SearchResult>,
    ) -> Self {
        let (message_tx, message_rx) = std::sync::mpsc::channel::<JobMessage>();
        let source_watch_tx =
            super::source_watcher::spawn_source_watcher(message_tx.clone());
        let jobs = Self {
            wav_job_tx,
            audio_job_tx,
            search_job_tx,
            source_watch_tx,
            message_tx,
            message_rx,
            pending_source: None,
            pending_select_path: None,
            pending_audio: None,
            pending_playback: None,
            next_audio_request_id: 1,
            scan_in_progress: false,
            scan_cancel: None,
            trash_move_in_progress: false,
            trash_move_cancel: None,
            collection_move_in_progress: false,
            umap_build_in_progress: false,
            umap_cluster_build_in_progress: false,
            update_check_in_progress: false,
            issue_gateway_in_progress: false,
            issue_gateway_auth_in_progress: false,
            issue_gateway_poll_in_progress: false,
            issue_gateway_poll_cancel: None,
            repaint_signal: Arc::new(Mutex::new(None)),
        };
        jobs.forward_wav_results(wav_job_rx);
        jobs.forward_audio_results(audio_job_rx);
        jobs.forward_search_results(search_job_rx);
        jobs
    }

    pub(super) fn try_recv_message(&self) -> Result<JobMessage, TryRecvError> {
        self.message_rx.try_recv()
    }

    pub(super) fn message_sender(&self) -> Sender<JobMessage> {
        self.message_tx.clone()
    }

    pub(crate) fn set_repaint_signal(&self, ctx: egui::Context) {
        if let Ok(mut signal) = self.repaint_signal.lock() {
            *signal = Some(ctx);
        }
    }


    /// Update the source roots watched for on-disk changes.
    pub(crate) fn update_source_watcher(&self, sources: Vec<SourceWatchEntry>) {
        let _ = self
            .source_watch_tx
            .send(SourceWatchCommand::ReplaceSources(sources));
    }

    pub(super) fn forward_wav_results(&self, rx: Receiver<WavLoadResult>) {
        let tx = self.message_tx.clone();
        let signal = self.repaint_signal.clone();
        thread::spawn(move || {
            while let Ok(message) = rx.recv() {
                let _ = tx.send(JobMessage::WavLoaded(message));
                if let Ok(lock) = signal.lock() {
                    if let Some(ctx) = lock.as_ref() {
                        ctx.request_repaint();
                    }
                }
            }
        });
    }

    pub(super) fn forward_audio_results(&self, rx: Receiver<AudioLoadResult>) {
        let tx = self.message_tx.clone();
        let signal = self.repaint_signal.clone();
        thread::spawn(move || {
            while let Ok(message) = rx.recv() {
                let _ = tx.send(JobMessage::AudioLoaded(message));
                if let Ok(lock) = signal.lock() {
                    if let Some(ctx) = lock.as_ref() {
                        ctx.request_repaint();
                    }
                }
            }
        });
    }

    pub(super) fn forward_search_results(&self, rx: Receiver<SearchResult>) {
        let tx = self.message_tx.clone();
        let signal = self.repaint_signal.clone();
        thread::spawn(move || {
            while let Ok(message) = rx.recv() {
                let _ = tx.send(JobMessage::BrowserSearchFinished(message));
                if let Ok(lock) = signal.lock() {
                    if let Some(ctx) = lock.as_ref() {
                        ctx.request_repaint();
                    }
                }
            }
        });
    }

    pub(super) fn wav_load_pending_for(&self, source_id: &SourceId) -> bool {
        self.pending_source.as_ref() == Some(source_id)
    }

    pub(super) fn mark_wav_load_pending(&mut self, source_id: SourceId) {
        self.pending_source = Some(source_id);
    }

    pub(super) fn clear_wav_load_pending(&mut self) {
        self.pending_source = None;
    }

    pub(super) fn send_wav_job(&self, job: WavLoadJob) {
        let _ = self.wav_job_tx.send(job);
    }

    pub(super) fn set_pending_select_path(&mut self, path: Option<PathBuf>) {
        self.pending_select_path = path;
    }

    pub(super) fn pending_select_path(&self) -> Option<PathBuf> {
        self.pending_select_path.clone()
    }

    pub(super) fn take_pending_select_path(&mut self) -> Option<PathBuf> {
        self.pending_select_path.take()
    }

    pub(super) fn pending_audio(&self) -> Option<PendingAudio> {
        self.pending_audio.clone()
    }

    pub(super) fn set_pending_audio(&mut self, pending: Option<PendingAudio>) {
        self.pending_audio = pending;
    }

    pub(super) fn pending_playback(&self) -> Option<PendingPlayback> {
        self.pending_playback.clone()
    }

    pub(super) fn set_pending_playback(&mut self, pending: Option<PendingPlayback>) {
        self.pending_playback = pending;
    }

    pub(super) fn next_audio_request_id(&mut self) -> u64 {
        let request_id = self.next_audio_request_id;
        self.next_audio_request_id = self.next_audio_request_id.wrapping_add(1).max(1);
        request_id
    }

    pub(super) fn send_audio_job(&self, job: AudioLoadJob) -> Result<(), ()> {
        self.audio_job_tx.send(job).map_err(|_| ())
    }

    pub(super) fn send_search_job(&self, job: SearchJob) {
        let _ = self.search_job_tx.send(job);
    }

    pub(super) fn scan_in_progress(&self) -> bool {
        self.scan_in_progress
    }

    pub(super) fn start_scan(&mut self, rx: Receiver<ScanJobMessage>, cancel: Arc<AtomicBool>) {
        self.scan_in_progress = true;
        self.scan_cancel = Some(cancel);
        let tx = self.message_tx.clone();
        let signal = self.repaint_signal.clone();
        thread::spawn(move || {
            while let Ok(message) = rx.recv() {
                let is_finished = matches!(message, ScanJobMessage::Finished(_));
                let _ = tx.send(JobMessage::Scan(message));
                if let Ok(lock) = signal.lock() {
                    if let Some(ctx) = lock.as_ref() {
                        ctx.request_repaint();
                    }
                }
                if is_finished {
                    break;
                }
            }
        });
    }

    pub(super) fn scan_cancel(&self) -> Option<Arc<AtomicBool>> {
        self.scan_cancel.clone()
    }

    pub(super) fn clear_scan(&mut self) {
        self.scan_in_progress = false;
        self.scan_cancel = None;
    }

    pub(super) fn trash_move_in_progress(&self) -> bool {
        self.trash_move_in_progress
    }

    #[cfg_attr(test, allow(dead_code))]
    pub(super) fn start_trash_move(
        &mut self,
        rx: Receiver<trash_move::TrashMoveMessage>,
        cancel: Arc<AtomicBool>,
    ) {
        self.trash_move_in_progress = true;
        self.trash_move_cancel = Some(cancel);
        let tx = self.message_tx.clone();
        let signal = self.repaint_signal.clone();
        thread::spawn(move || {
            while let Ok(message) = rx.recv() {
                let is_finished = matches!(message, trash_move::TrashMoveMessage::Finished(_));
                let _ = tx.send(JobMessage::TrashMove(message));
                if let Ok(lock) = signal.lock() {
                    if let Some(ctx) = lock.as_ref() {
                        ctx.request_repaint();
                    }
                }
                if is_finished {
                    break;
                }
            }
        });
    }

    pub(super) fn trash_move_cancel(&self) -> Option<Arc<AtomicBool>> {
        self.trash_move_cancel.clone()
    }

    pub(super) fn clear_trash_move(&mut self) {
        self.trash_move_in_progress = false;
        self.trash_move_cancel = None;
    }

    pub(super) fn collection_move_in_progress(&self) -> bool {
        self.collection_move_in_progress
    }

    pub(super) fn start_collection_move(&mut self, rx: Receiver<CollectionMoveResult>) {
        self.collection_move_in_progress = true;
        let tx = self.message_tx.clone();
        let signal = self.repaint_signal.clone();
        thread::spawn(move || {
            while let Ok(message) = rx.recv() {
                let _ = tx.send(JobMessage::CollectionMove(message));
                if let Ok(lock) = signal.lock() {
                    if let Some(ctx) = lock.as_ref() {
                        ctx.request_repaint();
                    }
                }
                break;
            }
        });
    }

    pub(super) fn clear_collection_move(&mut self) {
        self.collection_move_in_progress = false;
    }

    pub(super) fn update_check_in_progress(&self) -> bool {
        self.update_check_in_progress
    }

    pub(super) fn umap_build_in_progress(&self) -> bool {
        self.umap_build_in_progress
    }

    pub(super) fn umap_cluster_build_in_progress(&self) -> bool {
        self.umap_cluster_build_in_progress
    }

    pub(super) fn begin_umap_build(&mut self, job: UmapBuildJob) {
        if self.umap_build_in_progress {
            return;
        }
        self.umap_build_in_progress = true;
        let tx = self.message_tx.clone();
        let signal = self.repaint_signal.clone();
        thread::spawn(move || {
            let result =
                super::ui::map_view::run_umap_build(&job.model_id, &job.umap_version, &job.source_id);
            let _ = tx.send(JobMessage::UmapBuilt(UmapBuildResult {
                umap_version: job.umap_version,
                result,
            }));
            if let Ok(lock) = signal.lock() {
                if let Some(ctx) = lock.as_ref() {
                    ctx.request_repaint();
                }
            }
        });
    }

    pub(super) fn clear_umap_build(&mut self) {
        self.umap_build_in_progress = false;
    }

    pub(super) fn begin_umap_cluster_build(&mut self, job: UmapClusterBuildJob) {
        if self.umap_cluster_build_in_progress {
            return;
        }
        self.umap_cluster_build_in_progress = true;
        let tx = self.message_tx.clone();
        let signal = self.repaint_signal.clone();
        thread::spawn(move || {
            let result = super::ui::map_view::run_umap_cluster_build(
                &job.model_id,
                &job.umap_version,
                job.source_id.as_ref(),
            );
            let _ = tx.send(JobMessage::UmapClustersBuilt(UmapClusterBuildResult {
                umap_version: job.umap_version,
                source_id: job.source_id,
                result,
            }));
            if let Ok(lock) = signal.lock() {
                if let Some(ctx) = lock.as_ref() {
                    ctx.request_repaint();
                }
            }
        });
    }

    pub(super) fn clear_umap_cluster_build(&mut self) {
        self.umap_cluster_build_in_progress = false;
    }

    pub(super) fn begin_update_check(&mut self, request: crate::updater::UpdateCheckRequest) {
        if self.update_check_in_progress {
            return;
        }
        self.update_check_in_progress = true;
        let tx = self.message_tx.clone();
        let signal = self.repaint_signal.clone();
        thread::spawn(move || {
            let result = super::updates::run_update_check(request);
            let _ = tx.send(JobMessage::UpdateChecked(UpdateCheckResult { result }));
            if let Ok(lock) = signal.lock() {
                if let Some(ctx) = lock.as_ref() {
                    ctx.request_repaint();
                }
            }
        });
    }

    pub(super) fn clear_update_check(&mut self) {
        self.update_check_in_progress = false;
    }

    pub(super) fn begin_issue_gateway_create(&mut self, job: IssueGatewayJob) {
        if self.issue_gateway_in_progress {
            return;
        }
        self.issue_gateway_in_progress = true;
        let tx = self.message_tx.clone();
        thread::spawn(move || {
            let result = crate::issue_gateway::api::create_issue(&job.token, &job.request);
            let _ = tx.send(JobMessage::IssueGatewayCreated(IssueGatewayCreateResult {
                result,
            }));
        });
    }

    pub(super) fn clear_issue_gateway_create(&mut self) {
        self.issue_gateway_in_progress = false;
    }


    pub(super) fn clear_issue_gateway_auth(&mut self) {
        self.issue_gateway_auth_in_progress = false;
    }

    pub(super) fn begin_issue_gateway_poll(&mut self, job: IssueGatewayPollJob) {
        if self.issue_gateway_poll_in_progress {
            return;
        }
        self.issue_gateway_poll_in_progress = true;
        let cancel = Arc::new(std::sync::atomic::AtomicBool::new(false));
        self.issue_gateway_poll_cancel = Some(cancel.clone());
        let tx = self.message_tx.clone();
        thread::spawn(move || {
            loop {
                if cancel.load(std::sync::atomic::Ordering::Relaxed) {
                    break;
                }
                match crate::issue_gateway::api::poll_issue_token(&job.request_id) {
                    Ok(Some(token)) => {
                        let _ = tx.send(JobMessage::IssueGatewayAuthed(IssueGatewayAuthResult {
                            result: Ok(token),
                        }));
                        break;
                    }
                    Ok(None) => {
                        // Keep polling
                    }
                    Err(err) => {
                        let _ = tx.send(JobMessage::IssueGatewayAuthed(IssueGatewayAuthResult {
                            result: Err(err),
                        }));
                        break;
                    }
                }
                thread::sleep(std::time::Duration::from_secs(3));
            }
        });
    }

    pub(super) fn clear_issue_gateway_poll(&mut self) {
        self.issue_gateway_poll_in_progress = false;
        if let Some(cancel) = self.issue_gateway_poll_cancel.take() {
            cancel.store(true, std::sync::atomic::Ordering::Relaxed);
        }
    }

    pub(super) fn begin_normalization(&mut self, job: NormalizationJob) {
        let tx = self.message_tx.clone();
        let signal = self.repaint_signal.clone();
        thread::spawn(move || {
            // We need a way to call the normalization logic without the EguiController instance
            // since that's not thread-safe. The core logic is in analysis::audio.
            // But we also need database access for tags.
            // I'll refer to the implementation in collection_items_helpers/normalize.rs
            
            let source_id = job.source.id.clone();
            let relative_path = job.relative_path.clone();
            
            let result = (|| {
                let (mut samples, spec) = super::library::collection_items_helpers::io::read_samples_for_normalization(&job.absolute_path)?;
                if samples.is_empty() {
                    return Err("No audio data to normalize".to_string());
                }
                
                crate::analysis::audio::normalize_peak_in_place(&mut samples);

                let target_spec = hound::WavSpec {
                    channels: spec.channels.max(1),
                    sample_rate: spec.sample_rate.max(1),
                    bits_per_sample: 32,
                    sample_format: hound::SampleFormat::Float,
                };
                super::library::collection_items_helpers::io::write_normalized_wav(&job.absolute_path, &samples, target_spec)?;

                let (file_size, modified_ns) = super::library::collection_items_helpers::io::file_metadata(&job.absolute_path)?;
                
                // For the tag, we'll need to open the DB again since we don't have EguiController.
                let db = job.source.open_db()
                    .map_err(|err| format!("Database unavailable: {err}"))?;
                let tag = db.tag_for_path(&job.relative_path)
                    .map_err(|err| format!("Failed to read database: {err}"))?
                    .ok_or_else(|| "Sample not found in database".to_string())?;

                Ok((file_size, modified_ns, tag))
            })();

            let _ = tx.send(JobMessage::Normalized(NormalizationResult {
                source_id,
                relative_path,
                result,
            }));
            if let Ok(lock) = signal.lock() {
                if let Some(ctx) = lock.as_ref() {
                    ctx.request_repaint();
                }
            }
        });
    }
}
