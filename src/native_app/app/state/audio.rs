use std::{
    collections::HashSet,
    time::{Duration, Instant},
};

use wavecrate::audio::{
    AudioDeviceSummary, AudioHostSummary, AudioOutputConfig, AudioPlayer, PlaybackRequestId,
    PlaybackRuntimeEventReceiver, PlaybackRuntimeHandle, PlaybackRuntimeProgress,
    PlaybackRuntimeStreamPolicy, ResolvedOutput,
};
use wavecrate::sample_sources::config::AppSettingsCore;

use crate::native_app::audio::{
    playback::PlaybackIntent,
    playback_history::{LastPlayedPersistRequest, PlaybackNavigationHistory},
};

mod span_retarget;
mod visual_progress;

pub(in crate::native_app) use span_retarget::PlaybackSpanRetargetRejection;
use span_retarget::PlaybackSpanRetargetState;
use visual_progress::PlaybackVisualProgress;

pub(in crate::native_app) struct AudioAppState {
    pub(in crate::native_app) player: Option<AudioPlayer>,
    pub(in crate::native_app) loop_playback: bool,
    pub(in crate::native_app) loop_playback_manual_override_path: Option<String>,
    pub(in crate::native_app) metronome_enabled: bool,
    pub(in crate::native_app) normalized_audition_enabled: bool,
    pub(in crate::native_app) volume: f32,
    pub(in crate::native_app) volume_persist_deadline: Option<Instant>,
    pub(in crate::native_app) volume_persist_inflight: bool,
    pub(in crate::native_app) last_played_persist_task: radiant::prelude::LatestTask,
    pub(in crate::native_app) pending_last_played_persist: Option<LastPlayedPersistRequest>,
    pub(in crate::native_app) output_config: AudioOutputConfig,
    pub(in crate::native_app) output_config_persist_pending: bool,
    pub(in crate::native_app) output_resolved: Option<ResolvedOutput>,
    pub(in crate::native_app) hosts: Vec<AudioHostSummary>,
    pub(in crate::native_app) devices: Vec<AudioDeviceSummary>,
    pub(in crate::native_app) sample_rates: Vec<u32>,
    pub(in crate::native_app) settings_error: Option<String>,
    pub(in crate::native_app) current_playback_span: Option<(f32, f32)>,
    pub(in crate::native_app) pending_playback_start: Option<PendingPlaybackStart>,
    pub(in crate::native_app) pending_sample_playback: Option<SamplePlaybackRequest>,
    pub(in crate::native_app) playback_history: PlaybackNavigationHistory,
    pub(in crate::native_app) sample_playback_session: Option<SamplePlaybackSession>,
    pub(in crate::native_app) next_sample_playback_generation: u64,
    pub(in crate::native_app) playback_runtime: Option<PlaybackRuntimeHandle>,
    pub(in crate::native_app) playback_events: Option<PlaybackRuntimeEventReceiver>,
    pub(in crate::native_app) playback_progress: PlaybackRuntimeProgress,
    pub(in crate::native_app) playback_visual_progress: Option<PlaybackVisualProgress>,
    pub(in crate::native_app) pending_playback_progress_polls: HashSet<u64>,
    pub(in crate::native_app) completed_transient_playback:
        Option<CompletedTransientSamplePlayback>,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub(in crate::native_app) struct AudioOptionsRefreshResult {
    pub(in crate::native_app) hosts: Vec<AudioHostSummary>,
    pub(in crate::native_app) devices: Vec<AudioDeviceSummary>,
    pub(in crate::native_app) sample_rates: Vec<u32>,
    pub(in crate::native_app) error: Option<String>,
}

#[derive(Clone, Copy, Debug, PartialEq)]
#[allow(dead_code)]
pub(in crate::native_app) enum SamplePlaybackIntent {
    TransientNavigation,
    SettledNavigation,
    ExplicitPlayback,
    WaveformSpan,
    StarmapDrag,
    RandomAudition,
    Playmark,
    NormalizedResume,
    HistoryReplay,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(in crate::native_app) enum SamplePlaybackVisibility {
    Transient,
    Waveform,
}

impl SamplePlaybackVisibility {
    pub(in crate::native_app) fn updates_waveform_playhead(self) -> bool {
        matches!(self, Self::Waveform)
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(in crate::native_app) enum SamplePlaybackHistory {
    Record,
    Skip,
}

impl SamplePlaybackHistory {
    #[allow(dead_code)]
    pub(in crate::native_app) fn records(self) -> bool {
        matches!(self, Self::Record)
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
#[allow(dead_code)]
pub(in crate::native_app) enum SamplePlaybackLoopMode {
    FollowApp { offset: Option<f32> },
    OneShot,
    Looped { offset: f32 },
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[allow(dead_code)]
pub(in crate::native_app) enum SamplePlaybackNormalization {
    FollowSetting,
    Required,
    Disabled,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[allow(dead_code)]
pub(in crate::native_app) enum SamplePlaybackSourceProbe {
    LoadedOnly,
    CachedOnly,
    AllowFileProbe,
}

#[derive(Clone, Debug, PartialEq)]
pub(in crate::native_app) struct SamplePlaybackRequest {
    pub(in crate::native_app) path: String,
    pub(in crate::native_app) span: (f32, f32),
    pub(in crate::native_app) intent: SamplePlaybackIntent,
    pub(in crate::native_app) visibility: SamplePlaybackVisibility,
    pub(in crate::native_app) loop_mode: SamplePlaybackLoopMode,
    pub(in crate::native_app) history: SamplePlaybackHistory,
    pub(in crate::native_app) normalization: SamplePlaybackNormalization,
    pub(in crate::native_app) origin: &'static str,
    pub(in crate::native_app) source_probe: SamplePlaybackSourceProbe,
    pub(in crate::native_app) random_units: Option<(f32, f32)>,
    pub(in crate::native_app) stream_policy: PlaybackRuntimeStreamPolicy,
    pub(in crate::native_app) show_start_marker: bool,
}

impl SamplePlaybackRequest {
    pub(in crate::native_app) fn transient(
        path: String,
        intent: SamplePlaybackIntent,
        origin: &'static str,
    ) -> Self {
        Self {
            path,
            span: (0.0, 1.0),
            intent,
            visibility: SamplePlaybackVisibility::Transient,
            loop_mode: SamplePlaybackLoopMode::FollowApp { offset: None },
            history: SamplePlaybackHistory::Record,
            normalization: SamplePlaybackNormalization::FollowSetting,
            origin,
            source_probe: SamplePlaybackSourceProbe::CachedOnly,
            random_units: None,
            stream_policy: PlaybackRuntimeStreamPolicy::transient_navigation(),
            show_start_marker: false,
        }
    }

    pub(in crate::native_app) fn waveform(
        path: String,
        span: (f32, f32),
        intent: SamplePlaybackIntent,
        origin: &'static str,
        history: SamplePlaybackHistory,
    ) -> Self {
        Self {
            path,
            span,
            intent,
            visibility: SamplePlaybackVisibility::Waveform,
            loop_mode: SamplePlaybackLoopMode::FollowApp {
                offset: Some(span.0),
            },
            history,
            normalization: SamplePlaybackNormalization::FollowSetting,
            origin,
            source_probe: SamplePlaybackSourceProbe::LoadedOnly,
            random_units: None,
            stream_policy: PlaybackRuntimeStreamPolicy::full(),
            show_start_marker: true,
        }
    }

    pub(in crate::native_app) fn with_start_marker(mut self, show_start_marker: bool) -> Self {
        self.show_start_marker = show_start_marker;
        self
    }

    #[allow(dead_code)]
    pub(in crate::native_app) fn with_source_probe(
        mut self,
        source_probe: SamplePlaybackSourceProbe,
    ) -> Self {
        self.source_probe = source_probe;
        self
    }

    pub(in crate::native_app) fn with_random_units(mut self, start: f32, length: f32) -> Self {
        self.random_units = Some((start, length));
        self
    }

    pub(in crate::native_app) fn with_normalization(
        mut self,
        normalization: SamplePlaybackNormalization,
    ) -> Self {
        self.normalization = normalization;
        self
    }
}

#[derive(Clone, Debug, PartialEq)]
#[allow(dead_code)]
pub(in crate::native_app) enum SamplePlaybackSessionState {
    ResolvingSource,
    RuntimePending,
    AudibleTransient,
    WaveformVisible,
    Failed(String),
}

#[derive(Clone, Debug, PartialEq)]
pub(in crate::native_app) struct SamplePlaybackSession {
    pub(in crate::native_app) generation: u64,
    pub(in crate::native_app) request: SamplePlaybackRequest,
    pub(in crate::native_app) runtime_request_id: Option<u64>,
    pub(in crate::native_app) source_kind: &'static str,
    pub(in crate::native_app) submitted_at: Instant,
    pub(in crate::native_app) audible_started_at: Option<Instant>,
    pub(in crate::native_app) state: SamplePlaybackSessionState,
    span_retarget: PlaybackSpanRetargetState,
}

#[derive(Clone, Debug, PartialEq)]
pub(in crate::native_app) struct CompletedTransientSamplePlayback {
    pub(in crate::native_app) path: String,
    pub(in crate::native_app) source_kind: &'static str,
    pub(in crate::native_app) progress: f32,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub(in crate::native_app) struct PendingPlaybackStart {
    pub(in crate::native_app) intent: PlaybackIntent,
    pub(in crate::native_app) record_history: bool,
}

impl PendingPlaybackStart {
    pub(in crate::native_app) fn record(intent: PlaybackIntent) -> Self {
        Self {
            intent,
            record_history: true,
        }
    }

    pub(in crate::native_app) fn skip_history(intent: PlaybackIntent) -> Self {
        Self {
            intent,
            record_history: false,
        }
    }
}

impl AudioAppState {
    pub(in crate::native_app) fn from_settings(settings: &AppSettingsCore) -> Self {
        Self {
            player: None,
            loop_playback: false,
            loop_playback_manual_override_path: None,
            metronome_enabled: false,
            normalized_audition_enabled: settings.controls.normalized_audition_enabled,
            volume: settings.volume.clamp(0.0, 1.0),
            volume_persist_deadline: None,
            volume_persist_inflight: false,
            last_played_persist_task: radiant::prelude::LatestTask::new(),
            pending_last_played_persist: None,
            output_config: settings.audio_output.clone(),
            output_config_persist_pending: false,
            output_resolved: None,
            hosts: Vec::new(),
            devices: Vec::new(),
            sample_rates: Vec::new(),
            settings_error: None,
            current_playback_span: None,
            pending_playback_start: None,
            pending_sample_playback: None,
            playback_history: PlaybackNavigationHistory::default(),
            sample_playback_session: None,
            next_sample_playback_generation: 1,
            playback_runtime: None,
            playback_events: None,
            playback_progress: PlaybackRuntimeProgress::default(),
            playback_visual_progress: None,
            pending_playback_progress_polls: HashSet::new(),
            completed_transient_playback: None,
        }
    }

    #[cfg(test)]
    pub(in crate::native_app) fn for_tests() -> Self {
        Self::from_settings(&AppSettingsCore::default())
    }

    pub(in crate::native_app) fn start_sample_playback_session(
        &mut self,
        request: SamplePlaybackRequest,
        runtime_request_id: PlaybackRequestId,
        source_kind: &'static str,
    ) -> u64 {
        self.clear_playback_progress();
        self.completed_transient_playback = None;
        let generation = self.next_sample_playback_generation;
        self.next_sample_playback_generation = self
            .next_sample_playback_generation
            .saturating_add(1)
            .max(1);
        let confirmed_span = request.span;
        self.sample_playback_session = Some(SamplePlaybackSession {
            generation,
            request,
            runtime_request_id: Some(runtime_request_id.get()),
            source_kind,
            submitted_at: Instant::now(),
            audible_started_at: None,
            state: SamplePlaybackSessionState::RuntimePending,
            span_retarget: PlaybackSpanRetargetState::new(confirmed_span),
        });
        generation
    }

    #[allow(dead_code)]
    pub(in crate::native_app) fn start_resolving_sample_playback_session(
        &mut self,
        request: SamplePlaybackRequest,
        source_kind: &'static str,
    ) -> u64 {
        self.clear_playback_progress();
        self.completed_transient_playback = None;
        let generation = self.next_sample_playback_generation;
        self.next_sample_playback_generation = self
            .next_sample_playback_generation
            .saturating_add(1)
            .max(1);
        let confirmed_span = request.span;
        self.sample_playback_session = Some(SamplePlaybackSession {
            generation,
            request,
            runtime_request_id: None,
            source_kind,
            submitted_at: Instant::now(),
            audible_started_at: None,
            state: SamplePlaybackSessionState::ResolvingSource,
            span_retarget: PlaybackSpanRetargetState::new(confirmed_span),
        });
        generation
    }

    pub(in crate::native_app) fn clear_sample_playback_session(&mut self) {
        self.sample_playback_session = None;
        self.completed_transient_playback = None;
    }

    pub(in crate::native_app) fn take_completed_streamable_sample_playback(
        &mut self,
        path: &str,
    ) -> Option<f32> {
        let matches = self
            .completed_transient_playback
            .as_ref()
            .is_some_and(|completed| {
                completed.path == path && completed.source_kind != "preview_samples"
            });
        matches.then(|| {
            self.completed_transient_playback
                .take()
                .expect("matching completed transient playback")
                .progress
        })
    }

    pub(in crate::native_app) fn active_sample_playback_path(&self) -> Option<&str> {
        self.sample_playback_session
            .as_ref()
            .map(|session| session.request.path.as_str())
    }

    pub(in crate::native_app) fn active_sample_playback_matches(&self, path: &str) -> bool {
        self.active_sample_playback_path() == Some(path)
    }

    pub(in crate::native_app) fn active_sample_playback_is_preview(&self, path: &str) -> bool {
        self.sample_playback_session
            .as_ref()
            .is_some_and(|session| {
                session.request.path == path && session.source_kind == "preview_samples"
            })
    }

    pub(in crate::native_app) fn active_sample_playback_is_streamable(&self, path: &str) -> bool {
        self.sample_playback_session
            .as_ref()
            .is_some_and(|session| {
                session.request.path == path && session.source_kind != "preview_samples"
            })
    }

    pub(in crate::native_app) fn active_sample_playback_progress(
        &self,
        path: &str,
    ) -> Option<&PlaybackRuntimeProgress> {
        self.sample_playback_session
            .as_ref()
            .filter(|session| {
                session.request.path == path
                    && matches!(
                        session.state,
                        SamplePlaybackSessionState::AudibleTransient
                            | SamplePlaybackSessionState::WaveformVisible
                    )
            })
            .map(|_| &self.playback_progress)
    }

    pub(in crate::native_app) fn active_sample_playback_audible_elapsed(
        &self,
        path: &str,
    ) -> Option<Duration> {
        self.sample_playback_session
            .as_ref()
            .filter(|session| {
                session.request.path == path
                    && matches!(
                        session.state,
                        SamplePlaybackSessionState::AudibleTransient
                            | SamplePlaybackSessionState::WaveformVisible
                    )
            })
            .and_then(|session| session.audible_started_at)
            .map(|started_at| started_at.elapsed())
    }

    pub(in crate::native_app) fn active_sample_playback_updates_waveform(
        &self,
        path: &str,
    ) -> bool {
        self.sample_playback_session
            .as_ref()
            .is_some_and(|session| {
                session.request.path == path
                    && session.request.visibility.updates_waveform_playhead()
            })
    }

    pub(in crate::native_app) fn active_sample_playback_pending_runtime(&self) -> bool {
        self.sample_playback_session
            .as_ref()
            .is_some_and(|session| {
                matches!(session.state, SamplePlaybackSessionState::RuntimePending)
            })
    }

    #[allow(dead_code)]
    pub(in crate::native_app) fn active_starmap_audition_path(&self) -> Option<String> {
        self.sample_playback_session.as_ref().and_then(|session| {
            (session.request.origin == "starmap_drag").then(|| session.request.path.clone())
        })
    }

    pub(in crate::native_app) fn set_active_sample_playback_span(&mut self, span: (f32, f32)) {
        if let Some(session) = self.sample_playback_session.as_mut() {
            session.set_confirmed_span(span);
        }
    }
}
