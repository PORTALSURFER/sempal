use super::super::AudioPlayer;
use crate::waveform::WaveformRenderer;
use crate::audio::Source;
use std::{
    io::Cursor,
    path::PathBuf,
    time::{Duration, Instant},
};

pub(crate) fn test_player(
    stream: crate::audio::output::CpalAudioStream,
    track_duration: Option<f32>,
    started_at: Option<Instant>,
    play_span: Option<(f32, f32)>,
    looping: bool,
    loop_offset: Option<f32>,
    elapsed_override: Option<Duration>,
) -> AudioPlayer {
    AudioPlayer::test_with_state(
        stream,
        track_duration,
        started_at,
        play_span,
        looping,
        loop_offset,
        elapsed_override,
    )
}

pub(crate) mod fixtures {
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
        pub bits_per_sample: u16,
        pub sample_format: SampleFormat,
        pub pulses: Vec<TonePulse>,
    }

    impl ToneSpec {
        pub fn new(sample_rate: u32, channels: u16, duration_seconds: f32) -> Self {
            Self {
                sample_rate,
                channels,
                duration_seconds,
                bits_per_sample: 32,
                sample_format: SampleFormat::Float,
                pulses: Vec::new(),
            }
        }

        pub fn with_bit_depth(mut self, bits: u16, format: SampleFormat) -> Self {
            self.bits_per_sample = bits;
            self.sample_format = format;
            self
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
            let channels = self.spec.channels.max(1) as usize;
            if self.frames == 0 || channels == 0 {
                return 0;
            }
            let frame = (seconds * self.spec.sample_rate as f32).round() as usize;
            let clamped_frame = frame.min(self.frames.saturating_sub(1));
            let total_samples = self.frames.saturating_mul(channels);
            clamped_frame
                .saturating_mul(channels)
                .min(total_samples.saturating_sub(1))
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
            bits_per_sample: spec.bits_per_sample,
            sample_format: spec.sample_format,
        };
        let mut writer = WavWriter::create(&path, wav_spec).expect("create wav file");
        for frame in 0..frames {
            let time = frame as f32 / spec.sample_rate as f32;
            let clamped = pulse_amplitude(time, &spec.pulses);
            for _ in 0..spec.channels {
                match spec.sample_format {
                    SampleFormat::Float => writer.write_sample::<f32>(clamped).expect("write sample"),
                    SampleFormat::Int => {
                        match spec.bits_per_sample {
                            8 => writer.write_sample::<i8>((clamped * 127.0) as i8).expect("write sample"),
                            16 => writer.write_sample::<i16>((clamped * 32767.0) as i16).expect("write sample"),
                            24 => writer.write_sample::<i32>((clamped * 8388607.0) as i32).expect("write sample"),
                            32 => writer.write_sample::<i32>((clamped * 2147483647.0) as i32).expect("write sample"),
                            _ => panic!("Unsupported bit depth for tests"),
                        }
                    }
                }
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

pub(crate) fn silent_wav_bytes(duration_secs: f32, sample_rate: u32, channels: u16) -> Vec<u8> {
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

#[derive(Clone)]
pub(crate) struct ConstantSource {
    sample_rate: u32,
    channels: u16,
    total_frames: u32,
    emitted_samples: u32,
    value: f32,
}

impl ConstantSource {
    pub(crate) fn new(sample_rate: u32, channels: u16, duration_secs: f32, value: f32) -> Self {
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
    fn current_frame_len(&self) -> Option<usize> {
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

pub(crate) fn assert_fixture_decodes(renderer: &WaveformRenderer, fixture: fixtures::WavFixture) {
    assert!(fixture.path.is_file());
    let decoded = renderer
        .decode_from_bytes(&fixture.bytes)
        .expect("decode fixture");
    assert_eq!(decoded.sample_rate, fixture.spec.sample_rate);
    assert_eq!(decoded.channels, fixture.spec.channels);
    assert!((decoded.duration_seconds - fixture.spec.duration_seconds).abs() < 0.02);
    let expected_samples = fixture
        .frames
        .saturating_mul(fixture.spec.channels as usize);
    assert_eq!(decoded.samples.len(), expected_samples);

    let pulse = fixture.spec.pulses.first().expect("missing pulse");
    let sample_time = pulse.start_seconds + pulse.duration_seconds * 0.5;
    let idx = fixture.sample_index_at(sample_time);
    let expected = fixture.expected_amplitude_at(sample_time);
    let actual = decoded.samples[idx];
    assert!(
        (actual - expected).abs() < 1e-4,
        "Amplitude mismatch at time {}: expected {}, got {} (bits={}, format={:?})",
        sample_time, expected, actual, fixture.spec.bits_per_sample, fixture.spec.sample_format
    );

    let tail_time = (fixture.spec.duration_seconds - 0.01).max(0.0);
    let tail_idx = fixture.sample_index_at(tail_time);
    let tail_expected = fixture.expected_amplitude_at(tail_time);
    assert!((decoded.samples[tail_idx] - tail_expected).abs() < 1e-6);
}
