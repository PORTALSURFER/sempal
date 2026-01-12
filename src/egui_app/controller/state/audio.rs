//! Audio playback and loading state for the controller.

use crate::audio::AudioPlayer;
use crate::egui_app::controller::playback::audio_cache::AudioCache;
use crate::sample_sources::SourceId;
use crate::audio::{AudioRecorder, InputMonitor};
use std::cell::RefCell;
use std::path::PathBuf;
use std::rc::Rc;
use std::time::Instant;

pub(crate) struct ControllerAudioState {
    pub(crate) player: Option<Rc<RefCell<AudioPlayer>>>,
    pub(crate) cache: AudioCache,
    pub(crate) pending_loop_disable_at: Option<Instant>,
    pub(crate) recorder: Option<AudioRecorder>,
    pub(crate) recording_target: Option<RecordingTarget>,
    pub(crate) input_monitor: Option<InputMonitor>,
    pub(crate) pending_age_update: Option<PendingAgeUpdate>,
}

impl ControllerAudioState {
    pub(crate) fn new(
        player: Option<Rc<RefCell<AudioPlayer>>>,
        cache_capacity: usize,
        history_limit: usize,
    ) -> Self {
        Self {
            player,
            cache: AudioCache::new(cache_capacity, history_limit),
            pending_loop_disable_at: None,
            recorder: None,
            recording_target: None,
            input_monitor: None,
            pending_age_update: None,
        }
    }
}

#[derive(Clone)]
pub(crate) struct PendingAgeUpdate {
    pub(crate) source_id: SourceId,
    pub(crate) root: PathBuf,
    pub(crate) relative_path: PathBuf,
    pub(crate) played_at: i64,
}

#[derive(Clone)]
pub(crate) struct RecordingTarget {
    pub(crate) source_id: SourceId,
    pub(crate) relative_path: PathBuf,
    pub(crate) absolute_path: PathBuf,
    pub(crate) last_refresh_at: Option<Instant>,
    pub(crate) last_file_len: u64,
    pub(crate) loaded_once: bool,
}

#[derive(Clone)]
pub(crate) struct PendingAudio {
    pub(crate) request_id: u64,
    pub(crate) source_id: SourceId,
    pub(crate) root: PathBuf,
    pub(crate) relative_path: PathBuf,
    pub(crate) intent: AudioLoadIntent,
}

#[derive(Clone)]
pub(crate) struct PendingPlayback {
    pub(crate) source_id: SourceId,
    pub(crate) relative_path: PathBuf,
    pub(crate) looped: bool,
    pub(crate) start_override: Option<f32>,
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum AudioLoadIntent {
    Selection,
    CollectionPreview,
}

#[derive(Clone)]
pub(crate) struct LoadedAudio {
    pub(crate) source_id: SourceId,
    pub(crate) root: PathBuf,
    pub(crate) relative_path: PathBuf,
    pub(crate) bytes: Vec<u8>,
    pub(crate) duration_seconds: f32,
    pub(crate) sample_rate: u32,
}
