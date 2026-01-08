use super::*;
use crate::audio::{AudioRecorder, RecordingOutcome};
use std::time::Duration;

mod path;
mod recorder;

const RECORDING_FILE_PREFIX: &str = "recording_";
const RECORDING_FILE_EXT: &str = "wav";
const RECORDING_REFRESH_INTERVAL: Duration = Duration::from_millis(60);
const RECORDING_MAX_FULL_FRAMES: usize = 2_500_000;
const RECORDING_MAX_PEAK_BUCKETS: usize = 1_000_000;

impl EguiController {
    pub fn is_recording(&self) -> bool {
        recorder::is_recording(self)
    }

    pub fn start_recording(&mut self) -> Result<(), String> {
        recorder::start_recording(self)
    }

    pub(super) fn start_recording_in_current_source(&mut self) -> Result<(), String> {
        recorder::start_recording_in_current_source(self)
    }

    pub fn stop_recording(&mut self) -> Result<Option<RecordingOutcome>, String> {
        recorder::stop_recording(self)
    }

    pub fn stop_recording_and_load(&mut self) -> Result<(), String> {
        recorder::stop_recording_and_load(self)
    }

    fn refresh_output_after_recording(&mut self) {
        recorder::refresh_output_after_recording(self);
    }

    pub(crate) fn refresh_recording_waveform(&mut self) {
        recorder::refresh_recording_waveform(self);
    }

    pub(super) fn start_input_monitor(&mut self, recorder: &AudioRecorder) {
        recorder::start_input_monitor(self, recorder);
    }

    pub(super) fn stop_input_monitor(&mut self) {
        recorder::stop_input_monitor(self);
    }
}
