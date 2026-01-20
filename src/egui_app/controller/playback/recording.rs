use super::*;
use crate::audio::{AudioRecorder, RecordingOutcome};
use std::time::Duration;

mod path;
mod recorder;
mod waveform_loader;

const RECORDING_FILE_PREFIX: &str = "recording_";
const RECORDING_FILE_EXT: &str = "wav";
const RECORDING_REFRESH_INTERVAL: Duration = Duration::from_millis(60);
const RECORDING_MAX_FULL_FRAMES: usize = 2_500_000;
const RECORDING_MAX_PEAK_BUCKETS: usize = 1_000_000;

impl EguiController {
    /// Return true if a recording session is active.
    pub fn is_recording(&self) -> bool {
        recorder::is_recording(self)
    }

    /// Begin recording using the current input configuration.
    pub fn start_recording(&mut self) -> Result<(), String> {
        recorder::start_recording(self)
    }


    /// Stop recording and return the outcome if a session existed.
    pub fn stop_recording(&mut self) -> Result<Option<RecordingOutcome>, String> {
        recorder::stop_recording(self)
    }

    /// Stop recording, then load the new file into the waveform view.
    pub fn stop_recording_and_load(&mut self) -> Result<(), String> {
        recorder::stop_recording_and_load(self)
    }


    pub(crate) fn refresh_recording_waveform(&mut self) {
        recorder::refresh_recording_waveform(self);
    }

    pub(crate) fn start_input_monitor(&mut self, recorder: &AudioRecorder) {
        recorder::start_input_monitor(self, recorder);
    }

    pub(crate) fn stop_input_monitor(&mut self) {
        recorder::stop_input_monitor(self);
    }
}
