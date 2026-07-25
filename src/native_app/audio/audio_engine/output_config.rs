use std::{
    sync::{
        Arc, Mutex,
        atomic::{AtomicU64, Ordering},
    },
    time::Instant,
};

use radiant::prelude as ui;

use crate::native_app::app::{
    AudioOutputPersistResult, GuiMessage, NativeAppState, PendingPlaybackStart, emit_gui_action,
};
use crate::native_app::audio::playback::PlaybackIntent;
use wavecrate::sample_sources::config::AppConfig;

impl NativeAppState {
    pub(in crate::native_app) fn set_audio_output_host(
        &mut self,
        host: Option<String>,
        context: &mut ui::UiUpdateContext<GuiMessage>,
    ) {
        let started_at = Instant::now();
        self.close_audio_settings_dropdowns();
        self.audio.output_config.host = host;
        self.audio.output_config.device = None;
        self.audio.output_config.sample_rate = None;
        self.apply_audio_output_config_change(started_at, "audio.output.host.set", context);
    }

    pub(in crate::native_app) fn set_audio_output_device(
        &mut self,
        device: Option<String>,
        context: &mut ui::UiUpdateContext<GuiMessage>,
    ) {
        let started_at = Instant::now();
        self.close_audio_settings_dropdowns();
        self.audio.output_config.device = device;
        self.audio.output_config.sample_rate = None;
        self.apply_audio_output_config_change(started_at, "audio.output.device.set", context);
    }

    pub(in crate::native_app) fn set_audio_output_sample_rate(
        &mut self,
        sample_rate: Option<u32>,
        context: &mut ui::UiUpdateContext<GuiMessage>,
    ) {
        let started_at = Instant::now();
        self.close_audio_settings_dropdowns();
        self.audio.output_config.sample_rate = sample_rate;
        self.apply_audio_output_config_change(started_at, "audio.output.sample_rate.set", context);
    }

    pub(in crate::native_app) fn apply_audio_output_config_change(
        &mut self,
        started_at: Instant,
        action: &'static str,
        context: &mut ui::UiUpdateContext<GuiMessage>,
    ) {
        let restart_span = self
            .waveform
            .current
            .is_playing()
            .then_some(self.audio.current_playback_span)
            .flatten();
        self.stop_audio_output_playback();
        if let Some(runtime) = self.audio.playback_runtime.take() {
            let _ = runtime.try_shutdown();
        }
        self.background.audio_open.cancel();
        self.audio.player = None;
        self.audio.playback_events = None;
        self.audio.clear_sample_playback_session();
        self.audio.output_resolved = None;
        self.audio.pending_playback_start = restart_span.map(|(start, end)| {
            PendingPlaybackStart::skip_history(PlaybackIntent::new(start, end))
        });
        self.audio.settings_error = None;
        self.ui.status.sample = String::from("Audio output change pending");
        self.queue_audio_options_refresh(context);
        self.queue_configured_audio_player_open(context, true);
        emit_gui_action(
            action,
            Some("audio_settings"),
            None,
            "queued",
            started_at,
            None,
        );
    }

    pub(in crate::native_app) fn queue_audio_output_persist(
        &mut self,
        context: &mut ui::UiUpdateContext<GuiMessage>,
    ) {
        let mut persisted = self.current_settings_core();
        persisted.audio_output = self.audio.output_config.clone();
        let sources = self.library.folder_browser.configured_sample_sources();
        let generation = self
            .background
            .audio_output_persist_generation
            .load(Ordering::Acquire);
        let latest_generation = Arc::clone(&self.background.audio_output_persist_generation);
        let persist_lock = Arc::clone(&self.background.audio_output_persist_lock);
        context
            .business()
            .blocking_io("gui-audio-output-persist")
            .latest(&mut self.background.audio_output_persist_task)
            .run(
                move |_| {
                    persist_audio_output_settings(
                        sources,
                        persisted,
                        generation,
                        latest_generation,
                        persist_lock,
                    )
                },
                GuiMessage::AudioOutputPersisted,
            );
    }

    pub(in crate::native_app) fn finish_audio_output_persist(
        &mut self,
        completion: ui::TaskCompletion<AudioOutputPersistResult>,
    ) {
        let Some(result) = self
            .background
            .audio_output_persist_task
            .finish_completion(completion)
        else {
            return;
        };
        match result.result {
            Ok(()) if result.persisted.audio_output == self.audio.output_config => {
                self.ui.settings.persisted.audio_output = result.persisted.audio_output;
                self.audio.output_config_persist_pending = false;
            }
            Ok(()) => {}
            Err(error) => {
                self.ui.status.sample = format!("Settings not saved: {error}");
                emit_gui_action(
                    "audio.output.persist",
                    Some("settings"),
                    None,
                    "persist_error",
                    Instant::now(),
                    Some(&error),
                );
            }
        }
    }
}

fn persist_audio_output_settings(
    sources: Vec<wavecrate::sample_sources::SampleSource>,
    persisted: wavecrate::sample_sources::config::AppSettingsCore,
    generation: u64,
    latest_generation: Arc<AtomicU64>,
    persist_lock: Arc<Mutex<()>>,
) -> AudioOutputPersistResult {
    let result = persist_audio_output_settings_if_current(
        generation,
        &latest_generation,
        &persist_lock,
        || {
            wavecrate::sample_sources::config::save(&AppConfig {
                sources,
                core: persisted.clone(),
            })
            .map_err(|err| err.to_string())
        },
    );
    AudioOutputPersistResult { persisted, result }
}

fn persist_audio_output_settings_if_current(
    generation: u64,
    latest_generation: &AtomicU64,
    persist_lock: &Mutex<()>,
    persist: impl FnOnce() -> Result<(), String>,
) -> Result<(), String> {
    let _guard = persist_lock
        .lock()
        .map_err(|_| String::from("audio settings persistence lock poisoned"))?;
    if latest_generation.load(Ordering::Acquire) != generation {
        return Ok(());
    }
    persist()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::{
        Arc,
        atomic::{AtomicBool, AtomicU64, Ordering},
    };

    #[test]
    fn stale_audio_persist_worker_cannot_overwrite_newer_confirmed_output() {
        let latest_generation = AtomicU64::new(1);
        let persist_lock = Mutex::new(());
        let durable_output = Arc::new(Mutex::new(String::from("old-output")));

        let output_a = Arc::clone(&durable_output);
        persist_audio_output_settings_if_current(1, &latest_generation, &persist_lock, || {
            *output_a.lock().expect("durable output lock") = String::from("output-a");
            Ok(())
        })
        .expect("output A should persist while current");

        latest_generation.store(2, Ordering::Release);
        let output_b = Arc::clone(&durable_output);
        persist_audio_output_settings_if_current(2, &latest_generation, &persist_lock, || {
            *output_b.lock().expect("durable output lock") = String::from("output-b");
            Ok(())
        })
        .expect("output B should persist while current");

        let stale_save_attempted = Arc::new(AtomicBool::new(false));
        let stale_save_attempted_for_worker = Arc::clone(&stale_save_attempted);
        persist_audio_output_settings_if_current(1, &latest_generation, &persist_lock, || {
            stale_save_attempted_for_worker.store(true, Ordering::Release);
            Ok(())
        })
        .expect("stale completion should be harmless");

        assert_eq!(
            durable_output.lock().expect("durable output lock").as_str(),
            "output-b"
        );
        assert!(!stale_save_attempted.load(Ordering::Acquire));
    }
}
