//! Compatibility exports for Wavecrate's reusable audio foundation.
//!
//! Generic audio infrastructure lives in the `reson` crate. This module keeps
//! existing Wavecrate imports stable and owns conversion from Wavecrate-specific
//! selection state into neutral realtime audio fade ranges.

use std::time::Duration;

mod normalized;

pub use reson::{
    AudioDeviceSummary, AudioHostSummary, AudioInputConfig, AudioInputError, AudioOutputConfig,
    AudioOutputError, AudioPlayer, AudioRecorder, EditFadeRange, FadeParams, InputMonitor,
    PlaybackMetronomeConfig, PlaybackRequestId, PlaybackRuntime, PlaybackRuntimeCancellation,
    PlaybackRuntimeConfig, PlaybackRuntimeEvent, PlaybackRuntimeEventReceiver,
    PlaybackRuntimeGainNormalization, PlaybackRuntimeHandle, PlaybackRuntimeMode,
    PlaybackRuntimeProgress, PlaybackRuntimeReplacePolicy, PlaybackRuntimeRequest,
    PlaybackRuntimeSource, PlaybackRuntimeSpanUpdate, PlaybackRuntimeStarted,
    PlaybackRuntimeStreamPolicy, PlaybackRuntimeSubmitError, RecordingHealth, RecordingOutcome,
    ResolvedInput, ResolvedInputConfig, ResolvedOutput, SamplesBuffer, Source, Wsola,
    available_devices, available_hosts, available_input_channel_count, available_input_devices,
    available_input_hosts, decoder, input, open_output_stream, output, recording,
    resolve_input_stream_config, supported_input_sample_rates, supported_sample_rates,
    wav_sanitize,
};

pub use normalized::{
    normalized_gain_for_interleaved_span, normalized_gain_from_peak,
    peak_for_interleaved_f32_reader_span, peak_for_interleaved_span,
};

use crate::selection::SelectionRange;

/// Default anti-click fade used at hard playback/export/extraction boundaries.
pub const DEFAULT_SHORT_EDGE_FADE: Duration = Duration::from_millis(2);

/// Convert a Wavecrate waveform selection into a reusable `reson` edit-fade range.
pub fn edit_fade_range_from_selection(range: Option<SelectionRange>) -> Option<EditFadeRange> {
    range.map(|range| {
        EditFadeRange::new(
            range.start(),
            range.end(),
            range.gain(),
            range.fade_in().map(|fade| {
                FadeParams::with_outer_gain(fade.length, fade.curve, fade.mute, fade.outer_gain)
            }),
            range.fade_out().map(|fade| {
                FadeParams::with_outer_gain(fade.length, fade.curve, fade.mute, fade.outer_gain)
            }),
        )
    })
}

/// Apply short fade-in/out ramps across an entire interleaved clip.
pub fn apply_short_edge_fades_to_clip(
    samples: &mut [f32],
    channels: usize,
    sample_rate: u32,
    fade_duration: Duration,
) -> bool {
    let channels = channels.max(1);
    let total_frames = samples.len() / channels;
    let fade_frames = short_edge_fade_frame_count(sample_rate, total_frames, fade_duration);
    if fade_frames == 0 {
        return false;
    }
    for frame in 0..total_frames {
        let gain = short_edge_fade_gain(frame, total_frames, fade_frames);
        if (gain - 1.0).abs() <= f32::EPSILON {
            continue;
        }
        let offset = frame.saturating_mul(channels);
        for sample in samples[offset..offset + channels].iter_mut() {
            *sample *= gain;
        }
    }
    true
}

/// Resolve the fade length in frames for a short anti-click clip edge fade.
pub fn short_edge_fade_frame_count(
    sample_rate: u32,
    total_frames: usize,
    fade_duration: Duration,
) -> usize {
    if total_frames < 2 || fade_duration.is_zero() {
        return 0;
    }
    let requested = (fade_duration.as_secs_f64() * f64::from(sample_rate.max(1))).round() as usize;
    requested.min(total_frames / 2)
}

/// Gain for a frame inside a short clip edge fade.
pub fn short_edge_fade_gain(frame: usize, total_frames: usize, fade_frames: usize) -> f32 {
    if total_frames == 0 || fade_frames == 0 {
        return 1.0;
    }
    let fade_frames = fade_frames.min(total_frames / 2).max(1);
    let fade_in = if frame < fade_frames {
        ramp_up_gain(frame, fade_frames)
    } else {
        1.0
    };
    let frames_from_end = total_frames.saturating_sub(frame + 1);
    let fade_out = if frames_from_end < fade_frames {
        ramp_up_gain(frames_from_end, fade_frames)
    } else {
        1.0
    };
    fade_in.min(fade_out)
}

fn ramp_up_gain(offset: usize, fade_frames: usize) -> f32 {
    if fade_frames <= 1 {
        return 0.0;
    }
    (offset as f32 / (fade_frames - 1) as f32).clamp(0.0, 1.0)
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use super::*;

    #[test]
    fn short_edge_fades_ramp_clip_boundaries() {
        let mut samples = vec![1.0_f32; 8];

        let applied =
            apply_short_edge_fades_to_clip(&mut samples, 1, 1_000, Duration::from_millis(2));

        assert!(applied);
        assert_eq!(samples, vec![0.0, 1.0, 1.0, 1.0, 1.0, 1.0, 1.0, 0.0]);
    }

    #[test]
    fn short_edge_fades_preserve_channel_pairs() {
        let mut samples = vec![1.0_f32; 12];

        apply_short_edge_fades_to_clip(&mut samples, 2, 1_000, Duration::from_millis(2));

        assert_eq!(samples[0], samples[1]);
        assert_eq!(samples[2], samples[3]);
        assert_eq!(samples[8], samples[9]);
        assert_eq!(samples[10], samples[11]);
        assert_eq!(samples[0], 0.0);
        assert_eq!(samples[10], 0.0);
    }

    #[test]
    fn playback_retarget_real_output_survives_rapid_span_and_metronome_updates() {
        let Ok(player) = AudioPlayer::new() else {
            return;
        };
        let runtime = PlaybackRuntime::spawn(player, PlaybackRuntimeConfig::default())
            .expect("playback runtime");
        let request = PlaybackRuntimeRequest {
            source: PlaybackRuntimeSource::DecodedSamples {
                audio_bytes: Arc::<[u8]>::from([]),
                samples: Arc::<[f32]>::from(vec![0.0; 48_000]),
                duration: 1.0,
                sample_rate: 48_000,
                channels: 1,
            },
            mode: PlaybackRuntimeMode::Looped {
                start: 0.0,
                end: 1.0,
                offset: 0.0,
            },
            stream_policy: PlaybackRuntimeStreamPolicy::full(),
            volume: 1.0,
            playback_gain: 1.0,
            playback_gain_normalization: None,
            replace_policy: PlaybackRuntimeReplacePolicy::ClearPrevious,
            edit_fade: None,
            metronome: Some(PlaybackMetronomeConfig::new(4).with_cycle(48_000, 0)),
        };
        let started_id = runtime.handle.try_play(request).expect("start playback");
        assert!(matches!(
            runtime
                .events
                .recv_timeout(Duration::from_secs(2))
                .expect("started event"),
            PlaybackRuntimeEvent::Started(PlaybackRuntimeStarted { id, .. }) if id == started_id
        ));

        let mut latest_retarget_id = started_id;
        for update in 0..128_u64 {
            let start = (update % 20) as f64 / 100.0;
            let end = (start + 0.5).min(1.0);
            latest_retarget_id = runtime
                .handle
                .try_retarget_span(PlaybackRuntimeSpanUpdate {
                    start,
                    end,
                    offset: start,
                    seek_to_offset: update % 3 == 0,
                    looped: true,
                    playback_gain: 1.0,
                    playback_gain_normalization: None,
                    metronome: (update % 5 != 0).then(|| {
                        PlaybackMetronomeConfig::new((update % 7 + 1) as u16)
                            .with_cycle(24_000 + update, update * 17)
                    }),
                })
                .expect("retarget playback");
        }
        let mut saw_latest_retarget = false;
        while !saw_latest_retarget {
            match runtime
                .events
                .recv_timeout(Duration::from_secs(2))
                .expect("retarget event")
            {
                PlaybackRuntimeEvent::Progress { id, progress } if id == latest_retarget_id => {
                    saw_latest_retarget = true;
                    assert!(progress.looping);
                    assert!(progress.error.is_none());
                }
                PlaybackRuntimeEvent::Progress { .. } => {}
                other => panic!("unexpected playback runtime event: {other:?}"),
            }
        }
        assert!(
            saw_latest_retarget,
            "rapid retargeting must converge on the newest complete request"
        );
        runtime.handle.try_shutdown().expect("shutdown runtime");
    }
}
