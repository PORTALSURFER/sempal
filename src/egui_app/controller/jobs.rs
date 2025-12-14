use super::{
    AudioLoadJob, AudioLoadResult, PendingAudio, PendingPlayback, ScanResult, SourceId,
    UpdateCheckResult, WavLoadJob, WavLoadResult, trash_move,
};
use std::{
    path::PathBuf,
    sync::{
        Arc,
        atomic::AtomicBool,
        mpsc::{Receiver, Sender},
    },
    thread,
};

type TryRecvError = std::sync::mpsc::TryRecvError;

#[cfg_attr(test, allow(dead_code))]
pub(in super) enum JobMessage {
    WavLoaded(WavLoadResult),
    AudioLoaded(AudioLoadResult),
    ScanFinished(ScanResult),
    TrashMove(trash_move::TrashMoveMessage),
    UpdateChecked(UpdateCheckResult),
}

pub(in super) struct ControllerJobs {
    pub(in super) wav_job_tx: Sender<WavLoadJob>,
    pub(in super) audio_job_tx: Sender<AudioLoadJob>,
    message_tx: Sender<JobMessage>,
    message_rx: Receiver<JobMessage>,
    pub(in super) pending_source: Option<SourceId>,
    pub(in super) pending_select_path: Option<PathBuf>,
    pub(in super) pending_audio: Option<PendingAudio>,
    pub(in super) pending_playback: Option<PendingPlayback>,
    pub(in super) next_audio_request_id: u64,
    pub(in super) scan_in_progress: bool,
    pub(in super) trash_move_in_progress: bool,
    pub(in super) trash_move_cancel: Option<Arc<std::sync::atomic::AtomicBool>>,
    pub(in super) update_check_in_progress: bool,
}

impl ControllerJobs {
    pub(in super) fn new(
        wav_job_tx: Sender<WavLoadJob>,
        wav_job_rx: Receiver<WavLoadResult>,
        audio_job_tx: Sender<AudioLoadJob>,
        audio_job_rx: Receiver<AudioLoadResult>,
    ) -> Self {
        let (message_tx, message_rx) = std::sync::mpsc::channel::<JobMessage>();
        let jobs = Self {
            wav_job_tx,
            audio_job_tx,
            message_tx,
            message_rx,
            pending_source: None,
            pending_select_path: None,
            pending_audio: None,
            pending_playback: None,
            next_audio_request_id: 1,
            scan_in_progress: false,
            trash_move_in_progress: false,
            trash_move_cancel: None,
            update_check_in_progress: false,
        };
        jobs.forward_wav_results(wav_job_rx);
        jobs.forward_audio_results(audio_job_rx);
        jobs
    }

    pub(in super) fn try_recv_message(&self) -> Result<JobMessage, TryRecvError> {
        self.message_rx.try_recv()
    }

    pub(in super) fn forward_wav_results(&self, rx: Receiver<WavLoadResult>) {
        let tx = self.message_tx.clone();
        thread::spawn(move || {
            while let Ok(message) = rx.recv() {
                let _ = tx.send(JobMessage::WavLoaded(message));
            }
        });
    }

    pub(in super) fn forward_audio_results(&self, rx: Receiver<AudioLoadResult>) {
        let tx = self.message_tx.clone();
        thread::spawn(move || {
            while let Ok(message) = rx.recv() {
                let _ = tx.send(JobMessage::AudioLoaded(message));
            }
        });
    }

    pub(in super) fn wav_load_pending_for(&self, source_id: &SourceId) -> bool {
        self.pending_source.as_ref() == Some(source_id)
    }

    pub(in super) fn mark_wav_load_pending(&mut self, source_id: SourceId) {
        self.pending_source = Some(source_id);
    }

    pub(in super) fn clear_wav_load_pending(&mut self) {
        self.pending_source = None;
    }

    pub(in super) fn send_wav_job(&self, job: WavLoadJob) {
        let _ = self.wav_job_tx.send(job);
    }

    pub(in super) fn set_pending_select_path(&mut self, path: Option<PathBuf>) {
        self.pending_select_path = path;
    }

    pub(in super) fn pending_select_path(&self) -> Option<PathBuf> {
        self.pending_select_path.clone()
    }

    pub(in super) fn take_pending_select_path(&mut self) -> Option<PathBuf> {
        self.pending_select_path.take()
    }

    pub(in super) fn pending_audio(&self) -> Option<PendingAudio> {
        self.pending_audio.clone()
    }

    pub(in super) fn set_pending_audio(&mut self, pending: Option<PendingAudio>) {
        self.pending_audio = pending;
    }

    pub(in super) fn pending_playback(&self) -> Option<PendingPlayback> {
        self.pending_playback.clone()
    }

    pub(in super) fn set_pending_playback(&mut self, pending: Option<PendingPlayback>) {
        self.pending_playback = pending;
    }

    pub(in super) fn next_audio_request_id(&mut self) -> u64 {
        let request_id = self.next_audio_request_id;
        self.next_audio_request_id = self
            .next_audio_request_id
            .wrapping_add(1)
            .max(1);
        request_id
    }

    pub(in super) fn send_audio_job(&self, job: AudioLoadJob) -> Result<(), ()> {
        self.audio_job_tx.send(job).map_err(|_| ())
    }

    pub(in super) fn scan_in_progress(&self) -> bool {
        self.scan_in_progress
    }

    pub(in super) fn begin_scan(&mut self, rx: Receiver<ScanResult>) {
        self.scan_in_progress = true;
        let tx = self.message_tx.clone();
        thread::spawn(move || {
            if let Ok(result) = rx.recv() {
                let _ = tx.send(JobMessage::ScanFinished(result));
            }
        });
    }

    pub(in super) fn trash_move_in_progress(&self) -> bool {
        self.trash_move_in_progress
    }

    #[cfg_attr(test, allow(dead_code))]
    pub(in super) fn start_trash_move(
        &mut self,
        rx: Receiver<trash_move::TrashMoveMessage>,
        cancel: Arc<AtomicBool>,
    ) {
        self.trash_move_in_progress = true;
        self.trash_move_cancel = Some(cancel);
        let tx = self.message_tx.clone();
        thread::spawn(move || {
            while let Ok(message) = rx.recv() {
                let is_finished = matches!(message, trash_move::TrashMoveMessage::Finished(_));
                let _ = tx.send(JobMessage::TrashMove(message));
                if is_finished {
                    break;
                }
            }
        });
    }

    pub(in super) fn trash_move_cancel(&self) -> Option<Arc<AtomicBool>> {
        self.trash_move_cancel.clone()
    }

    pub(in super) fn clear_trash_move(&mut self) {
        self.trash_move_in_progress = false;
        self.trash_move_cancel = None;
    }

    pub(in super) fn update_check_in_progress(&self) -> bool {
        self.update_check_in_progress
    }

    pub(in super) fn begin_update_check(&mut self, request: crate::updater::UpdateCheckRequest) {
        if self.update_check_in_progress {
            return;
        }
        self.update_check_in_progress = true;
        let tx = self.message_tx.clone();
        thread::spawn(move || {
            let result = super::updates::run_update_check(request);
            let _ = tx.send(JobMessage::UpdateChecked(UpdateCheckResult { result }));
        });
    }

    pub(in super) fn clear_update_check(&mut self) {
        self.update_check_in_progress = false;
    }

}
