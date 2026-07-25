use std::sync::atomic::Ordering;
use std::time::Instant;

use radiant::prelude as ui;
use wavecrate::audio::{AudioPlayer, PlaybackRuntime, PlaybackRuntimeConfig};

use crate::native_app::app::{AudioOpenCompletion, GuiMessage, NativeAppState, emit_gui_action};

use super::timing::log_audio_open_timing;

impl NativeAppState {
    pub(in crate::native_app) fn maybe_open_audio_player(
        &mut self,
        context: &mut ui::UiUpdateContext<GuiMessage>,
    ) {
        if self.audio.playback_runtime.is_some()
            || self.audio.player.is_some()
            || self.background.audio_open.active().is_some()
            || self.audio.settings_error.is_some()
        {
            return;
        }
        self.queue_configured_audio_player_open(context, false);
    }

    pub(in crate::native_app) fn queue_configured_audio_player_open(
        &mut self,
        context: &mut ui::UiUpdateContext<GuiMessage>,
        persist_on_success: bool,
    ) {
        if self.background.audio_open.active().is_some() {
            return;
        }
        let started_at = Instant::now();
        let request = self.background.audio_open.begin();
        let config = self.audio.output_config.clone();
        let volume = self.audio.volume;
        if persist_on_success {
            self.audio.output_config_persist_pending = true;
        }
        self.audio.settings_error = None;
        context.business().blocking_io("gui-audio-open").run(
            move |_| {
                log_audio_open_timing("audio.output.open.queue_wait", started_at.elapsed(), true);
                let open_started_at = Instant::now();
                let result = AudioPlayer::from_config(&config).map(|mut player| {
                    player.set_volume(volume);
                    player
                });
                log_audio_open_timing(
                    "audio.output.open.worker_open",
                    open_started_at.elapsed(),
                    true,
                );
                request.complete(result)
            },
            GuiMessage::AudioPlayerOpenFinished,
        );
        emit_gui_action(
            "audio.output.open",
            Some("audio"),
            None,
            "queued",
            started_at,
            None,
        );
    }

    pub(in crate::native_app) fn finish_audio_player_open(
        &mut self,
        completion: crate::native_app::app::AudioOpenTaskCompletion,
        context: &mut ui::UiUpdateContext<GuiMessage>,
    ) {
        let started_at = Instant::now();
        let AudioOpenCompletion::Current(result) = self.background.audio_open.finish(completion)
        else {
            emit_gui_action(
                "audio.output.open",
                Some("audio"),
                None,
                "stale",
                started_at,
                None,
            );
            return;
        };
        match *result {
            Ok(player) => self.finish_current_audio_player_open(player, started_at, context),
            Err(err) => self.finish_failed_audio_player_open(err, started_at),
        }
    }

    fn finish_current_audio_player_open(
        &mut self,
        player: AudioPlayer,
        started_at: Instant,
        context: &mut ui::UiUpdateContext<GuiMessage>,
    ) {
        log_audio_open_timing("audio.output.open.finish", started_at.elapsed(), false);
        self.audio.output_resolved = Some(player.output_details().clone());
        self.audio.settings_error = None;
        self.audio.player = None;
        match PlaybackRuntime::spawn(player, PlaybackRuntimeConfig::default()) {
            Ok(runtime) => {
                self.audio.playback_runtime = Some(runtime.handle);
                self.audio.playback_events = Some(runtime.events);
            }
            Err(err) => {
                self.finish_failed_audio_player_open(
                    format!("start playback runtime: {err}"),
                    started_at,
                );
                return;
            }
        }
        if self.audio.output_config_persist_pending {
            self.background
                .audio_output_persist_generation
                .fetch_add(1, Ordering::AcqRel);
            self.queue_audio_output_persist(context);
        }
        let pending = self.audio.pending_playback_start.take();
        if let Some(pending) = pending {
            match self.start_playback_intent_with_history(pending.intent, pending.record_history) {
                Ok(()) => {
                    let file_name = self.waveform.current.file_name();
                    self.ui.status.sample = format!("Playing {file_name}");
                }
                Err(err) => {
                    self.ui.status.sample = format!("Playback unavailable: {err}");
                    self.audio.settings_error = Some(err);
                }
            }
        }
        emit_gui_action(
            "audio.output.open",
            Some("audio"),
            None,
            "success",
            started_at,
            None,
        );
    }

    fn finish_failed_audio_player_open(&mut self, err: String, started_at: Instant) {
        log_audio_open_timing("audio.output.open.finish", started_at.elapsed(), false);
        self.audio.settings_error = Some(err.clone());
        self.audio.player = None;
        if let Some(runtime) = self.audio.playback_runtime.take() {
            let _ = runtime.try_shutdown();
        }
        self.audio.playback_runtime = None;
        self.audio.playback_events = None;
        self.audio.output_resolved = None;
        self.audio.pending_playback_start = None;
        emit_gui_action(
            "audio.output.open",
            Some("audio"),
            None,
            "error",
            started_at,
            Some(&err),
        );
    }
}
