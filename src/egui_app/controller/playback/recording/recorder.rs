use super::state::audio::{PendingRecordingWaveform, RecordingTarget};
use super::*;
use crate::audio::{AudioRecorder, InputMonitor, RecordingOutcome};
use std::time::Instant;
use super::waveform_loader::RecordingWaveformJob;

pub(crate) fn is_recording(controller: &EguiController) -> bool {
    controller.audio.recorder.is_some()
}

pub(crate) fn start_recording(controller: &mut EguiController) -> Result<(), String> {
    start_recording_in_current_source(controller)
}

pub(crate) fn start_recording_in_current_source(
    controller: &mut EguiController,
) -> Result<(), String> {
    if is_recording(controller) {
        return Ok(());
    }
    if controller.is_playing() {
        controller.stop_playback_if_active();
    }
    let (source, relative_path, output_path) =
        super::path::next_recording_path_in_source(controller)?;
    if controller.settings.controls.input_monitoring_enabled && controller.audio.player.is_none() {
        if let Err(err) = controller.ensure_player() {
            controller.set_status(err, StatusTone::Warning);
        }
    }
    controller.sample_view.wav.selected_wav = Some(relative_path.clone());
    controller.audio.recording_target = Some(RecordingTarget {
        source_id: source.id.clone(),
        relative_path,
        absolute_path: output_path.clone(),
        last_refresh_at: None,
        last_file_len: 0,
        loaded_once: false,
    });
    let recorder = AudioRecorder::start(&controller.settings.audio_input, output_path.clone())
        .map_err(|err| err.to_string())?;
    controller.update_audio_input_status(recorder.resolved());
    start_input_monitor(controller, &recorder);
    controller.audio.recorder = Some(recorder);
    controller.set_status(
        format!("Recording to {}", output_path.display()),
        StatusTone::Busy,
    );
    Ok(())
}

pub(crate) fn stop_recording(
    controller: &mut EguiController,
) -> Result<Option<RecordingOutcome>, String> {
    let target = controller.audio.recording_target.clone();
    stop_input_monitor(controller);
    controller
        .runtime
        .jobs
        .set_pending_recording_waveform(None);
    let Some(recorder) = controller.audio.recorder.take() else {
        return Ok(None);
    };
    let outcome = recorder.stop().map_err(|err| err.to_string())?;
    controller.audio.recording_target = None;
    controller.set_status(
        format!(
            "Recorded {:.2}s to {}",
            outcome.duration_seconds,
            outcome.path.display()
        ),
        StatusTone::Info,
    );
    if let Err(err) =
        super::path::register_recording_in_browser(controller, target.as_ref(), &outcome.path)
    {
        controller.set_status(
            format!(
                "Recorded {:.2}s to {} (indexing failed: {err})",
                outcome.duration_seconds,
                outcome.path.display()
            ),
            StatusTone::Warning,
        );
    }
    if let Ok((source, relative_path)) =
        super::path::resolve_recording_target(controller, target.as_ref(), &outcome.path)
    {
        controller.invalidate_cached_audio(&source.id, &relative_path);
        controller.sample_view.wav.loaded_audio = None;
        controller.sample_view.wav.loaded_wav = None;
        controller.ui.loaded_wav = None;
        if let Err(err) = controller.load_waveform_for_selection(&source, &relative_path) {
            controller.set_status(
                format!("Recorded {} (load failed: {err})", relative_path.display()),
                StatusTone::Warning,
            );
        }
    }
    refresh_output_after_recording(controller);
    Ok(Some(outcome))
}

pub(crate) fn stop_recording_and_load(controller: &mut EguiController) -> Result<(), String> {
    let _ = stop_recording(controller)?;
    Ok(())
}

pub(crate) fn refresh_output_after_recording(controller: &mut EguiController) {
    if !output_host_is_asio(controller) {
        return;
    }
    if let Err(err) = controller.rebuild_audio_player() {
        controller.set_status(
            format!("Audio output restart failed after recording: {err}"),
            StatusTone::Warning,
        );
    }
}

fn output_host_is_asio(controller: &EguiController) -> bool {
    let host_id = controller
        .audio
        .player
        .as_ref()
        .map(|player| player.borrow().output_details().host_id.clone())
        .or_else(|| controller.settings.audio_output.host.clone());
    host_id
        .as_deref()
        .is_some_and(|host| host.eq_ignore_ascii_case("asio"))
}

pub(crate) fn refresh_recording_waveform(controller: &mut EguiController) {
    if !is_recording(controller) {
        controller.audio.recording_target = None;
        controller
            .runtime
            .jobs
            .set_pending_recording_waveform(None);
        return;
    }
    let (source_id, relative_path, absolute_path, last_refresh_at, last_file_len, loaded_once) =
        match controller.audio.recording_target.as_ref() {
            Some(target) => (
                target.source_id.clone(),
                target.relative_path.clone(),
                target.absolute_path.clone(),
                target.last_refresh_at,
                target.last_file_len,
                target.loaded_once,
            ),
            None => return,
        };
    let now = Instant::now();
    if last_refresh_at.is_some_and(|last| now.duration_since(last) < RECORDING_REFRESH_INTERVAL) {
        return;
    }
    let recorder = match controller.audio.recorder.as_ref() {
        Some(recorder) => recorder,
        None => return,
    };
    let request_id = controller.runtime.jobs.next_recording_waveform_request_id();
    let job = RecordingWaveformJob {
        request_id,
        source_id: source_id.clone(),
        relative_path: relative_path.clone(),
        absolute_path: absolute_path.clone(),
        last_file_len,
        loaded_once,
        sample_rate: recorder.resolved().sample_rate,
        channels: recorder.resolved().channel_count,
    };
    controller.runtime.jobs.set_pending_recording_waveform(Some(
        PendingRecordingWaveform {
            request_id,
            source_id,
            relative_path,
            absolute_path,
        },
    ));
    controller.runtime.jobs.send_recording_waveform_job(job);
    if let Some(target) = controller.audio.recording_target.as_mut() {
        target.last_refresh_at = Some(now);
    }
}

pub(crate) fn start_input_monitor(controller: &mut EguiController, recorder: &AudioRecorder) {
    if !controller.settings.controls.input_monitoring_enabled {
        return;
    }
    if controller.audio.input_monitor.is_some() {
        return;
    }
    let Some(player_rc) = controller.audio.player.as_ref() else {
        controller.set_status(
            "Audio output unavailable for monitoring",
            StatusTone::Warning,
        );
        return;
    };
    let sink = player_rc.borrow().create_monitor_sink(controller.ui.volume);
    let monitor = InputMonitor::start(
        sink,
        recorder.resolved().channel_count,
        recorder.resolved().sample_rate,
    );
    recorder.attach_monitor(&monitor);
    controller.audio.input_monitor = Some(monitor);
}

pub(crate) fn stop_input_monitor(controller: &mut EguiController) {
    if let Some(recorder) = controller.audio.recorder.as_ref() {
        recorder.detach_monitor();
    }
    if let Some(monitor) = controller.audio.input_monitor.take() {
        monitor.stop();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::egui_app::controller::test_support::dummy_controller;

    #[test]
    fn output_host_is_asio_handles_settings_host() {
        let (mut controller, _source) = dummy_controller();
        controller.settings.audio_output.host = Some("asio".to_string());
        assert!(output_host_is_asio(&controller));
        controller.settings.audio_output.host = Some("wasapi".to_string());
        assert!(!output_host_is_asio(&controller));
    }
}
