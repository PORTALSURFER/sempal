// Regression tests for audio playback math using deterministic tone fixtures.
use super::*;
use crate::waveform::WaveformRenderer;
use std::{
    io::Cursor,
    path::PathBuf,
    sync::Arc,
    time::{Duration, Instant},
};

mod fixtures {
    use super::*;
    use hound::{SampleFormat, WavSpec, WavWriter};
    use tempfile::TempDir;

    #[derive(Clone)]
    pub struct TonePulse {
        pub start_seconds: f32,
        pub duration_seconds: f32,
        pub amplitude: f32,
    }

    #[derive(Clone)]
    pub struct ToneSpec {
        pub sample_rate: u32,
        pub channels: u16,
        pub duration_seconds: f32,
        pub pulses: Vec<TonePulse>,
    }

    impl ToneSpec {
        pub fn new(sample_rate: u32, channels: u16, duration_seconds: f32) -> Self {
            Self {
                sample_rate,
                channels,
                duration_seconds,
                pulses: Vec::new(),
            }
        }

        pub fn with_pulse(mut self, pulse: TonePulse) -> Self {
            self.pulses.push(pulse);
            self
        }
    }

    pub struct WavFixture {
        pub spec: ToneSpec,
        pub path: PathBuf,
        pub bytes: Vec<u8>,
        pub frames: usize,
        _tempdir: TempDir,
    }

    impl WavFixture {
        pub fn sample_index_at(&self, seconds: f32) -> usize {
            if self.frames == 0 {
                return 0;
            }
            let raw = (seconds * self.spec.sample_rate as f32).round() as usize;
            raw.min(self.frames.saturating_sub(1))
        }

        pub fn expected_amplitude_at(&self, seconds: f32) -> f32 {
            pulse_amplitude(seconds, &self.spec.pulses)
        }
    }

    pub fn build_fixture(spec: ToneSpec) -> WavFixture {
        let frames = (spec.duration_seconds * spec.sample_rate as f32).round() as usize;
        let tempdir = TempDir::new().expect("create tempdir");
        let path = tempdir.path().join("fixture.wav");
        let wav_spec = WavSpec {
            channels: spec.channels,
            sample_rate: spec.sample_rate,
            bits_per_sample: 32,
            sample_format: SampleFormat::Float,
        };
        let mut writer = WavWriter::create(&path, wav_spec).expect("create wav file");
        for frame in 0..frames {
            let time = frame as f32 / spec.sample_rate as f32;
            let clamped = pulse_amplitude(time, &spec.pulses);
            for _ in 0..spec.channels {
                writer.write_sample::<f32>(clamped).expect("write sample");
            }
        }
        writer.finalize().expect("finalize wav");
        let bytes = std::fs::read(&path).expect("read wav bytes");
        WavFixture {
            spec,
            path,
            bytes,
            frames,
            _tempdir: tempdir,
        }
    }

    fn pulse_amplitude(seconds: f32, pulses: &[TonePulse]) -> f32 {
        let mut amplitude = 0.0;
        for pulse in pulses {
            if seconds >= pulse.start_seconds
                && seconds < pulse.start_seconds + pulse.duration_seconds
            {
                amplitude += pulse.amplitude;
            }
        }
        amplitude.clamp(-1.0, 1.0)
    }
}

fn silent_wav_bytes(duration_secs: f32, sample_rate: u32, channels: u16) -> Vec<u8> {
    let spec = hound::WavSpec {
        channels,
        sample_rate,
        bits_per_sample: 16,
        sample_format: hound::SampleFormat::Int,
    };
    let mut cursor = Cursor::new(Vec::new());
    {
        let mut writer = hound::WavWriter::new(&mut cursor, spec).expect("create wav writer");
        let frames = (duration_secs * sample_rate as f32).round() as usize;
        for _ in 0..frames {
            for _ in 0..channels {
                writer.write_sample::<i16>(0).expect("write sample");
            }
        }
        writer.finalize().expect("finalize wav");
    }
    cursor.into_inner()
}

#[test]
fn normalized_progress_respects_span() {
    let progress = normalized_progress(Some((2.0, 4.0)), 10.0, 1.0, false);
    assert_eq!(progress, Some(0.3));
}

#[test]
fn normalized_progress_handles_elapsed_beyond_span() {
    let progress = normalized_progress(Some((2.0, 4.0)), 10.0, 3.5, false);
    assert_eq!(progress, Some(0.4));
}

#[test]
fn normalized_progress_loops_within_range() {
    let progress = normalized_progress(Some((2.0, 4.0)), 10.0, 5.5, true);
    assert!((progress.unwrap() - 0.35).abs() < 0.0001);
}

#[test]
fn normalized_progress_handles_full_track() {
    let progress = normalized_progress(None, 8.0, 3.0, false);
    assert_eq!(progress, Some(0.375));
}

#[test]
fn remaining_loop_duration_reports_time_left_in_cycle() {
    let Ok(stream) = rodio::OutputStreamBuilder::open_default_stream() else {
        return;
    };
    let started_at = Instant::now() - Duration::from_secs_f32(0.75);
    let player = AudioPlayer {
        stream,
        sink: None,
        current_audio: None,
        track_duration: Some(8.0),
        started_at: Some(started_at),
        play_span: Some((1.0, 3.0)),
        looping: true,
        loop_offset: None,
        volume: 1.0,
        output: ResolvedOutput::default(),
    };

    let remaining = player.remaining_loop_duration().unwrap();
    assert!((remaining.as_secs_f32() - 1.25).abs() < 0.1);
}

#[test]
fn remaining_loop_duration_none_when_not_looping() {
    let Ok(stream) = rodio::OutputStreamBuilder::open_default_stream() else {
        return;
    };
    let player = AudioPlayer {
        stream,
        sink: None,
        current_audio: None,
        track_duration: Some(8.0),
        started_at: Some(Instant::now()),
        play_span: Some((1.0, 3.0)),
        looping: false,
        loop_offset: None,
        volume: 1.0,
        output: ResolvedOutput::default(),
    };

    assert!(player.remaining_loop_duration().is_none());
}

#[test]
fn normalized_progress_returns_none_when_invalid_duration() {
    assert_eq!(normalized_progress(None, 0.0, 1.0, false), None);
    assert_eq!(normalized_progress(None, -1.0, 1.0, false), None);
}

#[test]
fn progress_wraps_full_loop_from_offset() {
    let Ok(stream) = rodio::OutputStreamBuilder::open_default_stream() else {
        return;
    };
    let player = AudioPlayer {
        stream,
        sink: None,
        current_audio: None,
        track_duration: Some(10.0),
        started_at: Some(Instant::now() - Duration::from_secs_f32(2.0)),
        play_span: Some((0.0, 10.0)),
        looping: true,
        loop_offset: Some(7.0),
        volume: 1.0,
        output: ResolvedOutput::default(),
    };

    let progress = player.progress().unwrap();
    assert!((progress - 0.9).abs() < 0.05);
}

#[test]
fn set_audio_prefers_provided_duration() {
    let Ok(mut player) = AudioPlayer::new() else {
        return;
    };
    let bytes = silent_wav_bytes(2.0, 44_100, 2);
    player.set_audio(bytes, 2.0);
    let duration = player.track_duration.expect("duration set");
    assert!((duration - 2.0).abs() < 0.01);
}

#[test]
fn set_audio_falls_back_to_header() {
    let Ok(mut player) = AudioPlayer::new() else {
        return;
    };
    let bytes = silent_wav_bytes(2.0, 44_100, 2);
    player.set_audio(bytes, 0.0);
    let duration = player.track_duration.expect("duration set");
    assert!((duration - 2.0).abs() < 0.01);
}

#[test]
fn span_pipeline_preserves_sample_count() {
    let bytes = Arc::from(silent_wav_bytes(4.0, 1_000, 2));
    let (count, sample_rate, channels) =
        AudioPlayer::span_sample_count(bytes, 0.0, 4.0).expect("span count");
    let expected = (4.0 * sample_rate as f32 * channels as f32) as usize;
    let delta = (count as isize - expected as isize).abs();
    assert!(delta <= 2, "count {count}, expected {expected}");
}

#[test]
fn play_range_accepts_zero_width_request() {
    let Ok(stream) = rodio::OutputStreamBuilder::open_default_stream() else {
        return;
    };
    let mut player = AudioPlayer {
        stream,
        sink: None,
        current_audio: None,
        track_duration: None,
        started_at: None,
        play_span: None,
        looping: false,
        loop_offset: None,
        volume: 1.0,
        output: ResolvedOutput::default(),
    };
    let bytes = vec![
        0x52, 0x49, 0x46, 0x46, 0x24, 0x80, 0x00, 0x00, 0x57, 0x41, 0x56, 0x45, 0x66, 0x6D, 0x74,
        0x20, 0x10, 0x00, 0x00, 0x00, 0x01, 0x00, 0x01, 0x00, 0x44, 0xAC, 0x00, 0x00, 0x88, 0x58,
        0x01, 0x00, 0x02, 0x00, 0x10, 0x00, 0x64, 0x61, 0x74, 0x61, 0x00, 0x80, 0x00, 0x00, 0x00,
        0x00,
    ];
    player.set_audio(bytes, 1.0);
    assert!(player.play_range(0.5, 0.5, false).is_ok());
}

#[test]
fn decode_handles_varied_sample_rates_and_channels() {
    use fixtures::{TonePulse, ToneSpec, build_fixture};

    let renderer = WaveformRenderer::new(24, 12);
    let specs = [
        ToneSpec::new(8_000, 1, 0.25).with_pulse(TonePulse {
            start_seconds: 0.0,
            duration_seconds: 0.05,
            amplitude: 0.9,
        }),
        ToneSpec::new(48_000, 2, 1.2).with_pulse(TonePulse {
            start_seconds: 0.9,
            duration_seconds: 0.1,
            amplitude: 0.6,
        }),
        ToneSpec::new(11_025, 2, 0.5).with_pulse(TonePulse {
            start_seconds: 0.4,
            duration_seconds: 0.05,
            amplitude: 0.75,
        }),
    ];

    for spec in specs {
        let fixture = build_fixture(spec);
        assert_fixture_decodes(&renderer, fixture);
    }
}

#[test]
fn span_sample_count_tracks_requested_window() {
    use fixtures::{TonePulse, ToneSpec, build_fixture};

    let spec = ToneSpec::new(22_050, 2, 0.8).with_pulse(TonePulse {
        start_seconds: 0.6,
        duration_seconds: 0.05,
        amplitude: 0.5,
    });
    let fixture = build_fixture(spec);
    let start = 0.25 * fixture.spec.duration_seconds;
    let end = 0.75 * fixture.spec.duration_seconds;
    let bytes = Arc::from(fixture.bytes.clone());

    let (count, sample_rate, channels) =
        AudioPlayer::span_sample_count(bytes, start, end).expect("span count");
    let expected_frames = ((end - start) * sample_rate as f32) as usize;
    let expected_samples = expected_frames * channels as usize;
    let delta = (count as isize - expected_samples as isize).abs();
    assert!(
        delta <= 2,
        "count {count}, expected {expected_samples} (frames {expected_frames})"
    );
}

#[test]
fn normalized_progress_wraps_partial_selection_when_looping() {
    let duration = 1.6;
    let span = (0.3, 1.1);
    let elapsed = (span.1 - span.0) * 1.4;

    let progress = normalized_progress(Some(span), duration, elapsed, true).unwrap();
    let expected = (span.0 + (elapsed % (span.1 - span.0))) / duration;
    assert!((progress - expected).abs() < 0.001);
}

#[test]
fn edge_fade_tracks_frames_for_multichannel() {
    let source = ConstantSource::new(1_000, 2, 1.0, 1.0);
    let mut faded = EdgeFade::new(source, Duration::from_millis(100));
    let samples: Vec<f32> = faded.by_ref().collect();
    assert_eq!(samples.len(), 2_000);
    // Halfway through the clip should still be fully audible (no early fade-out).
    assert!(samples[1_000] > 0.5);
}

#[derive(Clone)]
struct ConstantSource {
    sample_rate: u32,
    channels: u16,
    total_frames: u32,
    emitted_samples: u32,
    value: f32,
}

impl ConstantSource {
    fn new(sample_rate: u32, channels: u16, duration_secs: f32, value: f32) -> Self {
        let total_frames = (duration_secs * sample_rate as f32).round() as u32;
        Self {
            sample_rate,
            channels,
            total_frames,
            emitted_samples: 0,
            value,
        }
    }
}

impl Iterator for ConstantSource {
    type Item = f32;

    fn next(&mut self) -> Option<Self::Item> {
        let max_samples = self.total_frames.saturating_mul(self.channels as u32);
        if self.emitted_samples >= max_samples {
            return None;
        }
        self.emitted_samples = self.emitted_samples.saturating_add(1);
        Some(self.value)
    }
}

impl Source for ConstantSource {
    fn current_span_len(&self) -> Option<usize> {
        None
    }

    fn channels(&self) -> u16 {
        self.channels
    }

    fn sample_rate(&self) -> u32 {
        self.sample_rate
    }

    fn total_duration(&self) -> Option<Duration> {
        Some(Duration::from_secs_f32(
            self.total_frames as f32 / self.sample_rate as f32,
        ))
    }
}

fn assert_fixture_decodes(renderer: &WaveformRenderer, fixture: fixtures::WavFixture) {
    assert!(fixture.path.is_file());
    let decoded = renderer
        .decode_from_bytes(&fixture.bytes)
        .expect("decode fixture");
    assert_eq!(decoded.sample_rate, fixture.spec.sample_rate);
    assert_eq!(decoded.channels, fixture.spec.channels);
    assert!((decoded.duration_seconds - fixture.spec.duration_seconds).abs() < 0.02);
    assert_eq!(decoded.samples.len(), fixture.frames);

    let pulse = fixture.spec.pulses.first().expect("missing pulse");
    let sample_time = pulse.start_seconds + pulse.duration_seconds * 0.5;
    let idx = fixture.sample_index_at(sample_time);
    let expected = fixture.expected_amplitude_at(sample_time);
    assert!((decoded.samples[idx] - expected).abs() < 1e-6);

    let tail_time = (fixture.spec.duration_seconds - 0.01).max(0.0);
    let tail_idx = fixture.sample_index_at(tail_time);
    let tail_expected = fixture.expected_amplitude_at(tail_time);
    assert!((decoded.samples[tail_idx] - tail_expected).abs() < 1e-6);
}
