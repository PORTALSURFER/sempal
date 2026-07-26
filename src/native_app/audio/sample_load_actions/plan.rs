use radiant::prelude as ui;
use std::time::{Duration, Instant};

use crate::native_app::{
    app::{GuiMessage, NativeAppState, emit_gui_action, sample_path_label},
    audio::sample_load_actions::{
        foreground_sample_load_priority, log_sample_load_timing, sample_resource_key,
        types::{SampleLoadRequest, SampleLoadStrategy},
        worker::{SampleLoadWorker, SampleLoadWorkerEvent},
    },
};

pub(in crate::native_app::audio) const NORMALIZATION_SAMPLE_LOAD_RETRY_DELAY: Duration =
    Duration::from_secs(2);

impl NativeAppState {
    pub(super) fn schedule_deferred_sample_load(
        &mut self,
        path: String,
        autoplay: bool,
        check_cache: bool,
        delay: Duration,
        input_method: &'static str,
        context: &mut ui::UiUpdateContext<GuiMessage>,
    ) {
        tracing::info!(
            target: "wavecrate::debug::sample_load",
            event = "browser.sample_load.deferred_scheduled",
            path = %path,
            input_method,
            cache_state = "uncached",
            autoplay,
            delay_ms = delay.as_secs_f64() * 1000.0,
            "Sample load scheduled"
        );
        context.after_latest(
            &mut self.background.deferred_sample_load_task,
            delay,
            move |ticket| GuiMessage::DeferredSampleLoad {
                ticket,
                path,
                autoplay,
                check_cache,
                scheduled_at: Instant::now(),
            },
        );
    }

    pub(in crate::native_app) fn start_deferred_sample_load(
        &mut self,
        ticket: ui::TaskTicket,
        path: String,
        autoplay: bool,
        _check_cache: bool,
        scheduled_at: Instant,
        context: &mut ui::UiUpdateContext<GuiMessage>,
    ) {
        let started_at = Instant::now();
        log_sample_load_timing(
            "browser.sample_load.deferred_wait",
            path.as_str(),
            started_at.saturating_duration_since(scheduled_at),
            true,
        );
        if !self.background.deferred_sample_load_task.finish(ticket)
            || self.library.folder_browser.selected_file_id() != Some(path.as_str())
        {
            self.audio.pending_sample_playback = None;
            emit_gui_action(
                "browser.select_sample",
                Some("browser"),
                Some(&sample_path_label(path.as_str())),
                "load_deferred_stale",
                started_at,
                None,
            );
            return;
        }
        if self.sample_load_blocked_by_normalization(&path) {
            self.ui.status.sample = format!(
                "Selected {} | waiting for normalization",
                sample_path_label(path.as_str())
            );
            self.schedule_deferred_sample_load(
                path,
                autoplay,
                _check_cache,
                NORMALIZATION_SAMPLE_LOAD_RETRY_DELAY,
                "normalization",
                context,
            );
            return;
        }
        self.start_sample_load(
            path,
            autoplay,
            context,
            SampleLoadStrategy::Decode,
            started_at,
        );
    }

    pub(super) fn prepare_uncached_sample_load(
        &mut self,
        path: &str,
        outcome: &'static str,
        started_at: Instant,
    ) {
        let early_playback_matches_load = self.audio.active_sample_playback_matches(path);
        let keep_audible_waveform_visible = self.waveform.current.is_playing()
            || self.audio.current_playback_span.is_some()
            || self.audio.sample_playback_session.is_some();
        if !early_playback_matches_load {
            self.stop_current_sample_playback_for_load();
        }
        if !keep_audible_waveform_visible {
            self.replace_waveform_deferred(crate::native_app::app::WaveformState::empty());
        }
        self.ui.status.sample = format!("Loading {}", sample_path_label(path));
        let label = sample_path_label(path);
        self.waveform.load.label = Some(label.clone());
        self.waveform.load.progress = 0.0;
        self.waveform.load.target_progress = 0.0;
        self.waveform.load.selection.start_uncached(path);
        emit_gui_action(
            "browser.select_sample",
            Some("browser"),
            Some(&label),
            outcome,
            started_at,
            None,
        );
    }

    pub(super) fn start_sample_load(
        &mut self,
        path: String,
        autoplay: bool,
        context: &mut ui::UiUpdateContext<GuiMessage>,
        strategy: SampleLoadStrategy,
        started_at: Instant,
    ) {
        if self.sample_load_blocked_by_normalization(&path) {
            self.ui.status.sample = format!(
                "Selected {} | waiting for normalization",
                sample_path_label(path.as_str())
            );
            self.schedule_deferred_sample_load(
                path,
                autoplay,
                false,
                NORMALIZATION_SAMPLE_LOAD_RETRY_DELAY,
                "normalization",
                context,
            );
            return;
        }
        emit_gui_action(
            "browser.select_sample",
            Some("browser"),
            Some(&sample_path_label(path.as_str())),
            "load_queued",
            started_at,
            None,
        );
        self.start_sample_load_with_priority(
            path,
            autoplay,
            context,
            foreground_sample_load_priority(),
            strategy,
        );
    }

    pub(super) fn start_sample_load_with_priority(
        &mut self,
        path: String,
        autoplay: bool,
        context: &mut ui::UiUpdateContext<GuiMessage>,
        priority: ui::TaskPriority,
        strategy: SampleLoadStrategy,
    ) {
        let require_decoded_playback = autoplay && self.loop_playback_for_path_after_policy(&path);
        self.log_sample_identity_checkpoint(
            "browser.sample_load.worker_queued",
            "start_sample_load_with_priority",
            Some(std::path::Path::new(&path)),
            Some(match strategy {
                SampleLoadStrategy::CacheThenDecode => "cache_then_decode",
                SampleLoadStrategy::DisplayAfterInstantAudition => "display_after_instant_audition",
                SampleLoadStrategy::Decode => "decode",
            }),
        );
        self.cancel_active_sample_load_worker();
        let request =
            SampleLoadRequest::new(path, autoplay, priority, strategy, require_decoded_playback);
        let key = sample_resource_key(request.path());
        self.background.active_sample_load_key = Some(key.clone());
        let load = context
            .business()
            .priority("gui-sample-load", request.priority())
            .cancellable()
            .latest_for_resource(&mut self.background.sample_load_tasks, key);
        // Keep this stream fully ordered: progress and playback-ready are
        // distinct state transitions.
        self.background.sample_load_cancel = Some(load.stream(
            move |worker_context, events| {
                SampleLoadWorker::new(request).run(worker_context, events)
            },
            |event| match event.output {
                SampleLoadWorkerEvent::Progress(progress) => {
                    GuiMessage::SampleLoadProgress(event.key, event.ticket, progress)
                }
                SampleLoadWorkerEvent::PlaybackReady(ready) => {
                    GuiMessage::SamplePlaybackReady(ui::KeyedTaskCompletion {
                        key: event.key,
                        ticket: event.ticket,
                        output: ready,
                    })
                }
            },
            GuiMessage::SampleLoadFinished,
        ));
    }
}
