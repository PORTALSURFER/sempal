//! Audio playback and loading state for the controller.

use super::super::{AudioPlayer, SourceId, audio_cache::AudioCache};
use crate::audio::{AudioRecorder, InputMonitor};
use std::cell::RefCell;
use std::path::PathBuf;
use std::rc::Rc;
use std::time::Instant;

pub(in crate::egui_app::controller) struct ControllerAudioState {
    pub(in crate::egui_app::controller) player: Option<Rc<RefCell<AudioPlayer>>>,
    pub(in crate::egui_app::controller) cache: AudioCache,
    pub(in crate::egui_app::controller) pending_loop_disable_at: Option<Instant>,
    pub(in crate::egui_app::controller) recorder: Option<AudioRecorder>,
    pub(in crate::egui_app::controller) recording_target: Option<RecordingTarget>,
    pub(in crate::egui_app::controller) input_monitor: Option<InputMonitor>,
}

impl ControllerAudioState {
    pub(in crate::egui_app::controller) fn new(
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
        }
    }
}

#[derive(Clone)]
pub(in crate::egui_app::controller) struct RecordingTarget {
    pub(in crate::egui_app::controller) source_id: SourceId,
    pub(in crate::egui_app::controller) relative_path: PathBuf,
    pub(in crate::egui_app::controller) absolute_path: PathBuf,
    pub(in crate::egui_app::controller) last_refresh_at: Option<Instant>,
    pub(in crate::egui_app::controller) last_file_len: u64,
    pub(in crate::egui_app::controller) loaded_once: bool,
}

#[derive(Clone)]
pub(in crate::egui_app::controller) struct PendingAudio {
    pub(in crate::egui_app::controller) request_id: u64,
    pub(in crate::egui_app::controller) source_id: SourceId,
    pub(in crate::egui_app::controller) root: PathBuf,
    pub(in crate::egui_app::controller) relative_path: PathBuf,
    pub(in crate::egui_app::controller) intent: AudioLoadIntent,
}

#[derive(Clone)]
pub(in crate::egui_app::controller) struct PendingPlayback {
    pub(in crate::egui_app::controller) source_id: SourceId,
    pub(in crate::egui_app::controller) relative_path: PathBuf,
    pub(in crate::egui_app::controller) looped: bool,
    pub(in crate::egui_app::controller) start_override: Option<f32>,
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub(in crate::egui_app::controller) enum AudioLoadIntent {
    Selection,
    CollectionPreview,
}

#[derive(Clone)]
pub(in crate::egui_app::controller) struct LoadedAudio {
    pub(in crate::egui_app::controller) source_id: SourceId,
    pub(in crate::egui_app::controller) relative_path: PathBuf,
    pub(in crate::egui_app::controller) bytes: Vec<u8>,
    pub(in crate::egui_app::controller) duration_seconds: f32,
    pub(in crate::egui_app::controller) sample_rate: u32,
    pub(in crate::egui_app::controller) channels: u16,
}
