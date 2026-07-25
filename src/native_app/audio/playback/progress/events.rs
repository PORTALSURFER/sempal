use std::{path::Path, time::Duration};

use super::{
    state::playback_error_indicates_output_unavailable, telemetry::log_runtime_playback_event,
};
use crate::native_app::app::{
    NativeAppState, PlaybackSpanRetargetRejection, SamplePlaybackSessionState, sample_path_label,
};
use crate::native_app::starmap_audition_telemetry::StarmapAuditionCounter;
use wavecrate::audio::{
    PlaybackRuntimeCancellation, PlaybackRuntimeEvent, PlaybackRuntimeProgress,
    PlaybackRuntimeStarted, ResolvedOutput,
};

const MAX_PLAYBACK_RUNTIME_EVENTS_PER_FRAME: usize = 32;

impl NativeAppState {
    pub(in crate::native_app) fn runtime_playback_origin_for_path(
        &self,
        path: &str,
    ) -> &'static str {
        if self.ui.chrome.starmap_audition_drag.is_some()
            || self
                .ui
                .chrome
                .starmap_audition_queue
                .active_file_id
                .as_deref()
                == Some(path)
        {
            "starmap_drag"
        } else if self.audio.active_sample_playback_matches(path) {
            "instant_audition"
        } else {
            "browser"
        }
    }

    pub(in crate::native_app) fn current_waveform_runtime_source_kind(&self) -> &'static str {
        if self.waveform.current.playback_samples().is_some() {
            "decoded_samples"
        } else if self.waveform.current.playback_cache_file().is_some() {
            "interleaved_f32_file"
        } else if self.waveform.current.playback_source_file().is_some() {
            "audio_file"
        } else {
            "audio_bytes"
        }
    }

    pub(in crate::native_app) fn drain_playback_runtime_events(&mut self) {
        let Some(events) = self.audio.playback_events.take() else {
            return;
        };
        for event in events
            .try_iter()
            .take(MAX_PLAYBACK_RUNTIME_EVENTS_PER_FRAME)
        {
            self.apply_playback_runtime_event(event);
        }
        if self.audio.playback_runtime.is_some() {
            self.audio.playback_events = Some(events);
        }
    }

    fn apply_playback_runtime_event(&mut self, event: PlaybackRuntimeEvent) {
        match event {
            PlaybackRuntimeEvent::Started(started) => self.finish_runtime_playback_started(started),
            PlaybackRuntimeEvent::Failed { id, error } => {
                self.finish_runtime_playback_failed(id, error)
            }
            PlaybackRuntimeEvent::Cancelled { id, reason } => {
                self.finish_runtime_playback_cancelled(id, reason)
            }
            PlaybackRuntimeEvent::Stopped { .. } => {}
            PlaybackRuntimeEvent::Progress { id, progress } => {
                self.apply_runtime_playback_progress_for_request(id.get(), progress);
            }
        }
    }

    fn apply_runtime_playback_progress_for_request(
        &mut self,
        request_id: u64,
        progress: PlaybackRuntimeProgress,
    ) {
        if self
            .audio
            .pending_playback_progress_polls
            .remove(&request_id)
        {
            self.apply_runtime_playback_progress(progress);
            return;
        }
        let confirmed_retarget =
            self.audio
                .sample_playback_session
                .as_mut()
                .is_some_and(|session| {
                    if !session.confirm_span_retarget_id(request_id) {
                        return false;
                    }
                    session.runtime_request_id = Some(request_id);
                    true
                });
        if confirmed_retarget {
            self.apply_authoritative_runtime_playback_progress(progress);
            return;
        }
        if self
            .audio
            .sample_playback_session
            .as_ref()
            .is_some_and(|session| session.runtime_request_id != Some(request_id))
        {
            return;
        }
        self.apply_runtime_playback_progress(progress);
    }

    pub(super) fn apply_runtime_playback_progress(&mut self, progress: PlaybackRuntimeProgress) {
        if self.audio.active_sample_playback_pending_runtime() {
            return;
        }
        self.audio.set_playback_progress(progress);
    }

    fn apply_authoritative_runtime_playback_progress(&mut self, progress: PlaybackRuntimeProgress) {
        if self.audio.active_sample_playback_pending_runtime() {
            return;
        }
        self.audio.set_authoritative_playback_progress(progress);
    }

    fn finish_runtime_playback_started(&mut self, started: PlaybackRuntimeStarted) {
        self.finish_runtime_playback_started_parts(
            started.id.get(),
            started.output,
            started.playback_start,
        );
    }

    pub(in crate::native_app) fn finish_runtime_playback_started_parts(
        &mut self,
        request_id: u64,
        output: ResolvedOutput,
        runtime_playback_start: f32,
    ) {
        let Some(session) = self.audio.sample_playback_session.as_mut() else {
            return;
        };
        if session.runtime_request_id != Some(request_id) {
            log_runtime_playback_event(
                "runtime.started",
                "id_mismatch",
                Some(StarmapAuditionCounter::RuntimeStale),
                session,
                None,
                None,
            );
            return;
        }
        let submit_elapsed = session.submitted_at.elapsed();
        log_runtime_playback_event(
            "runtime.started",
            "started",
            Some(StarmapAuditionCounter::RuntimeStarted),
            session,
            Some(submit_elapsed),
            None,
        );
        let source_updates_waveform = session.request.visibility.updates_waveform_playhead();
        session.state = if source_updates_waveform {
            SamplePlaybackSessionState::WaveformVisible
        } else {
            SamplePlaybackSessionState::AudibleTransient
        };
        session.audible_started_at = Some(std::time::Instant::now());
        let path = session.request.path.clone();
        let span = session.request.span;
        let show_start_marker = session.request.show_start_marker;
        let preserves_live_retarget = session.has_pending_span_retarget();
        let playback_start = if preserves_live_retarget {
            self.audio
                .playback_visual_progress
                .map_or(runtime_playback_start, |progress| progress.anchor_ratio)
        } else {
            runtime_playback_start
        };
        self.audio.output_resolved = Some(output);
        self.audio.current_playback_span = source_updates_waveform.then_some(span);
        self.audio
            .set_started_playback_progress(wavecrate::audio::PlaybackRuntimeProgress {
                active: true,
                elapsed: Some(Duration::ZERO),
                looping: self.audio.loop_playback,
                progress: Some(playback_start),
                error: None,
            });
        if source_updates_waveform
            && !preserves_live_retarget
            && self.waveform.current.path() == Path::new(&path)
        {
            let preserves_existing_marker = self.waveform.current.is_playing()
                && self
                    .waveform
                    .current
                    .playhead_ratio()
                    .is_some_and(|ratio| (ratio - playback_start).abs() <= 0.000_1);
            if preserves_existing_marker {
                self.waveform
                    .current
                    .restart_playback_preserving_marker(playback_start);
            } else if show_start_marker {
                self.waveform.current.start_playback(playback_start);
            } else {
                self.waveform
                    .current
                    .start_playback_without_marker(playback_start);
            }
        }
        self.ui.status.sample = format!("Playing {}", sample_path_label(&path));
    }

    fn finish_runtime_playback_failed(
        &mut self,
        id: wavecrate::audio::PlaybackRequestId,
        error: String,
    ) {
        let span_retarget_failure =
            self.audio
                .sample_playback_session
                .as_mut()
                .and_then(|session| {
                    let rejection = session.reject_span_retarget(id)?;
                    let (outcome, counter) = match rejection {
                        PlaybackSpanRetargetRejection::Restore(_) => (
                            "retarget_failed",
                            Some(StarmapAuditionCounter::RuntimeFailed),
                        ),
                        PlaybackSpanRetargetRejection::Superseded => (
                            "retarget_failure_superseded",
                            Some(StarmapAuditionCounter::RuntimeStale),
                        ),
                    };
                    log_runtime_playback_event(
                        "runtime.failed",
                        outcome,
                        counter,
                        session,
                        None,
                        Some(&error),
                    );
                    Some((session.request.path.clone(), rejection))
                });
        if let Some((path, rejection)) = span_retarget_failure {
            if playback_error_indicates_output_unavailable(&error) {
                self.mark_audio_output_unavailable(error);
                return;
            }
            let PlaybackSpanRetargetRejection::Restore(span) = rejection else {
                return;
            };
            let progress = self.audio.playback_progress.progress.unwrap_or(span.0);
            self.audio.current_playback_span = Some(span);
            self.audio
                .reset_playback_visual_progress(progress, self.audio.loop_playback);
            self.waveform.current.set_playhead_ratio(progress);
            self.ui.status.sample = format!(
                "Playing {} | live range update unavailable: {error}",
                sample_path_label(&path)
            );
            return;
        }

        let Some(session) = self.audio.sample_playback_session.as_mut() else {
            return;
        };
        if session.runtime_request_id != Some(id.get()) {
            log_runtime_playback_event(
                "runtime.failed",
                "id_mismatch",
                Some(StarmapAuditionCounter::RuntimeStale),
                session,
                None,
                Some(&error),
            );
            return;
        }
        let submit_elapsed = session.submitted_at.elapsed();
        log_runtime_playback_event(
            "runtime.failed",
            "failed",
            Some(StarmapAuditionCounter::RuntimeFailed),
            session,
            Some(submit_elapsed),
            Some(&error),
        );
        if playback_error_indicates_output_unavailable(&error) {
            self.mark_audio_output_unavailable(error);
            return;
        }
        let path = session.request.path.clone();
        session.state = SamplePlaybackSessionState::Failed(error.clone());
        self.audio.clear_sample_playback_session();
        self.audio.current_playback_span = None;
        self.waveform.current.stop_playback();
        self.ui.status.sample = format!(
            "Loaded {} | playback unavailable: {error}",
            sample_path_label(&path)
        );
    }

    fn finish_runtime_playback_cancelled(
        &mut self,
        id: wavecrate::audio::PlaybackRequestId,
        reason: PlaybackRuntimeCancellation,
    ) {
        let Some(session) = self.audio.sample_playback_session.as_ref() else {
            return;
        };
        if session.runtime_request_id != Some(id.get()) {
            log_runtime_playback_event(
                "runtime.cancelled",
                "id_mismatch",
                Some(StarmapAuditionCounter::RuntimeStale),
                session,
                None,
                None,
            );
            return;
        }
        let submit_elapsed = session.submitted_at.elapsed();
        log_runtime_playback_event(
            "runtime.cancelled",
            match reason {
                PlaybackRuntimeCancellation::Superseded => "superseded",
                PlaybackRuntimeCancellation::Stopped => "stopped",
                PlaybackRuntimeCancellation::Shutdown => "shutdown",
            },
            Some(StarmapAuditionCounter::RuntimeCancelled),
            session,
            Some(submit_elapsed),
            None,
        );
        if reason != PlaybackRuntimeCancellation::Superseded {
            self.audio.clear_sample_playback_session();
            self.audio.current_playback_span = None;
            self.waveform.current.stop_playback();
        }
    }
}

#[cfg(test)]
#[path = "events_tests.rs"]
mod tests;
