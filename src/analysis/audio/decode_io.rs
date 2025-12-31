use std::cell::RefCell;
use std::fs::File;
use std::io::BufReader;
use std::path::Path;

use rodio::{Decoder, Source};

use super::analysis_prep::{downmix_to_mono_into, prepare_mono_for_analysis_from_slice};
use super::resample::resample_linear_into;
use super::{ANALYSIS_SAMPLE_RATE, AnalysisAudio, MAX_ANALYSIS_SECONDS, WINDOW_SECONDS};

pub(crate) fn decode_for_analysis(path: &Path) -> Result<AnalysisAudio, String> {
    decode_for_analysis_with_rate(path, ANALYSIS_SAMPLE_RATE)
}

pub(crate) struct AudioProbe {
    pub(crate) duration_seconds: Option<f32>,
    #[allow(dead_code)]
    pub(crate) sample_rate: Option<u32>,
    #[allow(dead_code)]
    pub(crate) channels: Option<u16>,
}

pub(crate) fn probe_metadata(path: &Path) -> Result<AudioProbe, String> {
    if path
        .extension()
        .and_then(|ext| ext.to_str())
        .is_some_and(|ext| ext.eq_ignore_ascii_case("wav"))
    {
        let reader = hound::WavReader::open(path)
            .map_err(|err| format!("WAV probe failed for {}: {err}", path.display()))?;
        let spec = reader.spec();
        let sample_rate = spec.sample_rate.max(1);
        let channels = spec.channels.max(1);
        let duration_seconds = (reader.duration() as f32
            / channels as f32
            / sample_rate as f32)
            .max(0.0);
        return Ok(AudioProbe {
            duration_seconds: Some(duration_seconds),
            sample_rate: Some(sample_rate),
            channels: Some(channels),
        });
    }

    let file =
        File::open(path).map_err(|err| format!("Failed to open {}: {err}", path.display()))?;
    let byte_len = file.metadata().map(|meta| meta.len()).unwrap_or(0) as u64;
    let hint = path
        .extension()
        .and_then(|ext| ext.to_str())
        .map(str::to_ascii_lowercase);
    let mut builder = Decoder::builder()
        .with_data(BufReader::new(file))
        .with_byte_len(byte_len)
        .with_seekable(false);
    if let Some(hint) = hint.as_deref() {
        builder = builder.with_hint(hint);
    }
    let decoder = builder
        .build()
        .map_err(|err| format!("Audio metadata probe failed for {}: {err}", path.display()))?;
    Ok(AudioProbe {
        duration_seconds: decoder.total_duration().map(|dur| dur.as_secs_f32()),
        sample_rate: Some(decoder.sample_rate().max(1)),
        channels: Some(decoder.channels().max(1)),
    })
}

pub(crate) fn decode_for_analysis_with_rate(
    path: &Path,
    sample_rate: u32,
) -> Result<AnalysisAudio, String> {
    decode_for_analysis_with_rate_limit(path, sample_rate, None)
}

pub(crate) fn decode_for_analysis_with_rate_limit(
    path: &Path,
    sample_rate: u32,
    max_seconds: Option<f32>,
) -> Result<AnalysisAudio, String> {
    let default_max = MAX_ANALYSIS_SECONDS + WINDOW_SECONDS;
    let max_decode_seconds = max_seconds
        .filter(|limit| limit.is_finite() && *limit > 0.0)
        .map(|limit| default_max.min(limit + WINDOW_SECONDS))
        .unwrap_or(default_max);
    let decoded = crate::analysis::audio_decode::decode_audio(path, Some(max_decode_seconds))?;
    DECODE_SCRATCH.with(|scratch| {
        let mut scratch = scratch.borrow_mut();
        downmix_to_mono_into(&mut scratch.mono, &decoded.samples, decoded.channels);
        let mono_len = scratch.mono.len();
        let mut resampled = Vec::new();
        resample_linear_into(
            &mut resampled,
            &scratch.mono[..mono_len],
            decoded.sample_rate,
            sample_rate,
        );
        Ok(prepare_mono_for_analysis_from_slice(&resampled, sample_rate))
    })
}

struct DecodeScratch {
    mono: Vec<f32>,
}

thread_local! {
    static DECODE_SCRATCH: RefCell<DecodeScratch> = RefCell::new(DecodeScratch {
        mono: Vec::new(),
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::analysis::audio::ANALYSIS_SAMPLE_RATE;
    use hound::{SampleFormat, WavSpec, WavWriter};
    use tempfile::TempDir;

    #[test]
    fn wav_probe_reads_duration_without_full_decode() {
        let dir = TempDir::new().unwrap();
        let path = dir.path().join("probe.wav");
        let spec = WavSpec {
            channels: 1,
            sample_rate: 48_000,
            bits_per_sample: 16,
            sample_format: SampleFormat::Int,
        };
        let mut writer = WavWriter::create(&path, spec).unwrap();
        for _ in 0..48_000 {
            writer.write_sample::<i16>(0).unwrap();
        }
        writer.finalize().unwrap();
        let probe = probe_metadata(&path).unwrap();
        let duration = probe.duration_seconds.unwrap();
        assert!((duration - 1.0).abs() < 1e-3);
        assert_eq!(probe.sample_rate, Some(48_000));
        assert_eq!(probe.channels, Some(1));
    }

    #[test]
    fn decode_for_analysis_decodes_wav_to_target_rate() {
        let dir = TempDir::new().unwrap();
        let path = dir.path().join("fixture.wav");
        let spec = WavSpec {
            channels: 2,
            sample_rate: 44_100,
            bits_per_sample: 32,
            sample_format: SampleFormat::Float,
        };
        let mut writer = WavWriter::create(&path, spec).unwrap();
        for _ in 0..(44_100 / 10) {
            writer.write_sample::<f32>(0.25).unwrap();
            writer.write_sample::<f32>(0.25).unwrap();
        }
        writer.finalize().unwrap();

        let decoded = decode_for_analysis(&path).unwrap();
        assert_eq!(decoded.sample_rate_used, ANALYSIS_SAMPLE_RATE);
        assert!((decoded.duration_seconds - 0.1).abs() < 0.02);
        let peak = decoded
            .mono
            .iter()
            .copied()
            .map(|v| v.abs())
            .fold(0.0, f32::max);
        assert!((peak - 1.0).abs() < 1e-6);
    }

    #[test]
    fn decode_for_analysis_trims_leading_and_trailing_silence() {
        let dir = TempDir::new().unwrap();
        let path = dir.path().join("trim.wav");
        let sample_rate = ANALYSIS_SAMPLE_RATE;
        let spec = WavSpec {
            channels: 1,
            sample_rate,
            bits_per_sample: 32,
            sample_format: SampleFormat::Float,
        };
        let mut writer = WavWriter::create(&path, spec).unwrap();
        let silence_frames = (0.1 * sample_rate as f32).round() as usize;
        let tone_frames = (0.1 * sample_rate as f32).round() as usize;
        let tail_silence_frames = (0.2 * sample_rate as f32).round() as usize;
        for _ in 0..silence_frames {
            writer.write_sample::<f32>(0.0).unwrap();
        }
        for _ in 0..tone_frames {
            writer.write_sample::<f32>(0.25).unwrap();
        }
        for _ in 0..tail_silence_frames {
            writer.write_sample::<f32>(0.0).unwrap();
        }
        writer.finalize().unwrap();

        let decoded = decode_for_analysis(&path).unwrap();
        assert!(decoded.duration_seconds < 0.25);
        assert!(decoded.duration_seconds > 0.08);
    }

    #[test]
    fn quiet_samples_are_not_trimmed_to_empty() {
        let dir = TempDir::new().unwrap();
        let path = dir.path().join("quiet.wav");
        let sample_rate = ANALYSIS_SAMPLE_RATE;
        let spec = WavSpec {
            channels: 1,
            sample_rate,
            bits_per_sample: 32,
            sample_format: SampleFormat::Float,
        };
        let mut writer = WavWriter::create(&path, spec).unwrap();
        let frames = (0.1 * sample_rate as f32).round() as usize;
        for _ in 0..frames {
            writer.write_sample::<f32>(0.001).unwrap();
        }
        writer.finalize().unwrap();

        let decoded = decode_for_analysis(&path).unwrap();
        assert!(!decoded.mono.is_empty());
        let peak = decoded
            .mono
            .iter()
            .copied()
            .map(|v| v.abs())
            .fold(0.0, f32::max);
        assert!(peak > 0.5);
    }
}
