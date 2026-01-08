use super::state::audio::RecordingTarget;
use super::*;
use crate::audio::{AudioRecorder, InputMonitor, RecordingOutcome};
use crate::waveform::{DecodedWaveform, WaveformPeaks};
use std::sync::Arc;
use std::time::Instant;

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
    let metadata = match std::fs::metadata(&absolute_path) {
        Ok(metadata) => metadata,
        Err(_) => return,
    };
    let len = metadata.len();
    if len == 0 || len == last_file_len {
        if let Some(target) = controller.audio.recording_target.as_mut() {
            target.last_refresh_at = Some(now);
        }
        return;
    }
    let bytes = match std::fs::read(&absolute_path) {
        Ok(bytes) => bytes,
        Err(_) => return,
    };
    let recorder = controller.audio.recorder.as_ref();
    let decoded = recorder.and_then(|recorder| {
        decode_recording_waveform(
            &bytes,
            recorder.resolved().sample_rate,
            recorder.resolved().channel_count,
        )
    });
    let Some(decoded) = decoded else {
        if let Some(target) = controller.audio.recording_target.as_mut() {
            target.last_refresh_at = Some(now);
        }
        return;
    };
    if let Some(source) = controller
        .library
        .sources
        .iter()
        .find(|source| source.id == source_id)
        .cloned()
    {
        if loaded_once {
            controller.apply_waveform_image(decoded);
        } else {
            let _ = controller.finish_waveform_load(
                &source,
                &relative_path,
                decoded,
                bytes,
                AudioLoadIntent::Selection,
            );
            if let Some(target) = controller.audio.recording_target.as_mut() {
                target.loaded_once = true;
            }
        }
    }
    if let Some(target) = controller.audio.recording_target.as_mut() {
        target.last_file_len = len;
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

fn decode_recording_waveform(
    bytes: &[u8],
    sample_rate: u32,
    channels: u16,
) -> Option<DecodedWaveform> {
    let data_offset = find_wav_data_chunk(bytes)?;
    if data_offset >= bytes.len() {
        return None;
    }
    let data = &bytes[data_offset..];
    let total_samples = data.len() / 4;
    if total_samples == 0 {
        return None;
    }
    let channels = channels.max(1) as usize;
    let frames = total_samples / channels.max(1);
    if frames == 0 {
        return None;
    }
    let duration_seconds = frames as f32 / sample_rate.max(1) as f32;
    if frames <= RECORDING_MAX_FULL_FRAMES {
        let mut samples = Vec::with_capacity(total_samples);
        for chunk in data.chunks_exact(4) {
            samples.push(f32::from_le_bytes(chunk.try_into().ok()?));
        }
        return Some(DecodedWaveform {
            cache_token: next_recording_cache_token(),
            samples: Arc::from(samples),
            peaks: None,
            duration_seconds,
            sample_rate,
            channels: channels as u16,
        });
    }

    let bucket_size_frames = peak_bucket_size(frames);
    let bucket_count = frames.div_ceil(bucket_size_frames).max(1);
    let mut mono = vec![(1.0_f32, -1.0_f32); bucket_count];
    let mut left = if channels >= 2 {
        Some(vec![(1.0_f32, -1.0_f32); bucket_count])
    } else {
        None
    };
    let mut right = if channels >= 2 {
        Some(vec![(1.0_f32, -1.0_f32); bucket_count])
    } else {
        None
    };

    let mut sample_index = 0usize;
    for frame in 0..frames {
        let bucket = frame / bucket_size_frames;
        let mut frame_sum = 0.0_f32;
        for ch in 0..channels {
            let offset = sample_index.saturating_mul(4);
            let sample = if offset + 4 <= data.len() {
                let raw = f32::from_le_bytes(data[offset..offset + 4].try_into().ok()?);
                clamp_sample(raw)
            } else {
                0.0
            };
            sample_index = sample_index.saturating_add(1);
            frame_sum += sample;
            if ch == 0 {
                if let Some(left_peaks) = left.as_mut() {
                    let (min, max) = &mut left_peaks[bucket];
                    *min = (*min).min(sample);
                    *max = (*max).max(sample);
                }
            } else if ch == 1 {
                if let Some(right_peaks) = right.as_mut() {
                    let (min, max) = &mut right_peaks[bucket];
                    *min = (*min).min(sample);
                    *max = (*max).max(sample);
                }
            }
        }
        let frame_avg = frame_sum / channels as f32;
        let (min, max) = &mut mono[bucket];
        *min = (*min).min(frame_avg);
        *max = (*max).max(frame_avg);
    }

    Some(DecodedWaveform {
        cache_token: next_recording_cache_token(),
        samples: Arc::from(Vec::new()),
        peaks: Some(Arc::new(WaveformPeaks {
            total_frames: frames,
            channels: channels as u16,
            bucket_size_frames,
            mono,
            left,
            right,
        })),
        duration_seconds,
        sample_rate,
        channels: channels as u16,
    })
}

fn find_wav_data_chunk(bytes: &[u8]) -> Option<usize> {
    if bytes.len() < 12 {
        return None;
    }
    if &bytes[0..4] != b"RIFF" || &bytes[8..12] != b"WAVE" {
        return None;
    }
    let mut offset = 12usize;
    while offset + 8 <= bytes.len() {
        let id = &bytes[offset..offset + 4];
        let chunk_size = u32::from_le_bytes(bytes[offset + 4..offset + 8].try_into().ok()?);
        let data_start = offset + 8;
        if id == b"data" {
            return Some(data_start);
        }
        let mut next = data_start.saturating_add(chunk_size as usize);
        if chunk_size % 2 == 1 {
            next = next.saturating_add(1);
        }
        if next <= offset {
            break;
        }
        offset = next;
    }
    None
}

fn peak_bucket_size(frames: usize) -> usize {
    frames.div_ceil(RECORDING_MAX_PEAK_BUCKETS).max(1)
}

fn clamp_sample(sample: f32) -> f32 {
    sample.clamp(-1.0, 1.0)
}

fn next_recording_cache_token() -> u64 {
    use std::sync::atomic::{AtomicU64, Ordering};

    static NEXT_CACHE_TOKEN: AtomicU64 = AtomicU64::new(1);
    NEXT_CACHE_TOKEN.fetch_add(1, Ordering::Relaxed)
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
