use super::{
    AudioLoadJob, AudioLoadResult, PendingAudio, PendingPlayback, ScanResult, SourceId, WavLoadJob,
    WavLoadResult, trash_move,
};
use std::{
    path::PathBuf,
    sync::{
        Arc,
        mpsc::{Receiver, Sender},
    },
};

pub(in super) struct ControllerJobs {
    pub(in super) wav_job_tx: Sender<WavLoadJob>,
    pub(in super) wav_job_rx: Receiver<WavLoadResult>,
    pub(in super) audio_job_tx: Sender<AudioLoadJob>,
    pub(in super) audio_job_rx: Receiver<AudioLoadResult>,
    pub(in super) pending_source: Option<SourceId>,
    pub(in super) pending_select_path: Option<PathBuf>,
    pub(in super) pending_audio: Option<PendingAudio>,
    pub(in super) pending_playback: Option<PendingPlayback>,
    pub(in super) next_audio_request_id: u64,
    pub(in super) scan_rx: Option<Receiver<ScanResult>>,
    pub(in super) scan_in_progress: bool,
    pub(in super) trash_move_rx: Option<Receiver<trash_move::TrashMoveMessage>>,
    pub(in super) trash_move_cancel: Option<Arc<std::sync::atomic::AtomicBool>>,
}

