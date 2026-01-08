use crate::selection::SelectionRange;
use hound::SampleFormat;
use std::fs;
use std::io::Cursor;
use std::path::Path;

/// Decoded WAV samples plus metadata derived from the file header.
#[derive(Clone, Debug)]
pub(super) struct DecodedSamples {
    pub(super) samples: Vec<f32>,
    pub(super) channels: u16,
    pub(super) sample_rate: u32,
}

/// Decode WAV bytes into interleaved f32 samples plus basic metadata.
pub(super) fn decode_samples_from_bytes(bytes: &[u8]) -> Result<DecodedSamples, String> {
    let mut reader =
        hound::WavReader::new(Cursor::new(bytes)).map_err(|err| format!("Invalid wav: {err}"))?;
    let spec = reader.spec();
    let channels = spec.channels.max(1);
    let samples = decode_samples(&mut reader, spec.sample_format, spec.bits_per_sample)?;
    Ok(DecodedSamples {
        samples,
        channels,
        sample_rate: spec.sample_rate.max(1),
    })
}

/// Crop interleaved samples to the provided normalized bounds.
pub(super) fn crop_samples(
    samples: &[f32],
    channels: u16,
    bounds: SelectionRange,
) -> Result<Vec<f32>, String> {
    let channels = channels.max(1) as usize;
    let total_frames = samples.len() / channels;
    if total_frames == 0 {
        return Err("No audio data to export".into());
    }
    let (start_frame, end_frame) = frame_bounds(total_frames, bounds);
    Ok(slice_frames(samples, channels, start_frame, end_frame))
}

/// Write interleaved f32 samples to a WAV file.
pub(super) fn write_wav(
    target: &Path,
    samples: &[f32],
    sample_rate: u32,
    channels: u16,
) -> Result<(), String> {
    if let Some(parent) = target.parent()
        && !parent.exists()
    {
        fs::create_dir_all(parent)
            .map_err(|err| format!("Failed to create folder {}: {err}", parent.display()))?;
    }
    let spec = hound::WavSpec {
        channels: channels.max(1),
        sample_rate: sample_rate.max(1),
        bits_per_sample: 32,
        sample_format: SampleFormat::Float,
    };
    let mut writer = hound::WavWriter::create(target, spec)
        .map_err(|err| format!("Failed to create clip: {err}"))?;
    for sample in samples {
        writer
            .write_sample(*sample)
            .map_err(|err| format!("Failed to write clip: {err}"))?;
    }
    writer
        .finalize()
        .map_err(|err| format!("Failed to finalize clip: {err}"))
}

fn decode_samples(
    reader: &mut hound::WavReader<Cursor<&[u8]>>,
    format: SampleFormat,
    bits_per_sample: u16,
) -> Result<Vec<f32>, String> {
    match format {
        SampleFormat::Float => reader
            .samples::<f32>()
            .map(|s| s.map_err(|err| format!("Sample error: {err}")))
            .collect::<Result<Vec<_>, _>>(),
        SampleFormat::Int => {
            let scale = (1i64 << bits_per_sample.saturating_sub(1)).max(1) as f32;
            reader
                .samples::<i32>()
                .map(|s| {
                    s.map(|v| v as f32 / scale)
                        .map_err(|err| format!("Sample error: {err}"))
                })
                .collect::<Result<Vec<_>, _>>()
        }
    }
}

fn frame_bounds(total_frames: usize, bounds: SelectionRange) -> (usize, usize) {
    let start_frame = ((bounds.start() * total_frames as f32).floor() as usize)
        .min(total_frames.saturating_sub(1));
    let mut end_frame = ((bounds.end() * total_frames as f32).ceil() as usize).min(total_frames);
    if end_frame <= start_frame {
        end_frame = (start_frame + 1).min(total_frames);
    }
    (start_frame, end_frame)
}

fn slice_frames(
    samples: &[f32],
    channels: usize,
    start_frame: usize,
    end_frame: usize,
) -> Vec<f32> {
    let mut cropped = Vec::with_capacity((end_frame - start_frame) * channels);
    for frame in start_frame..end_frame {
        let offset = frame * channels;
        cropped.extend_from_slice(&samples[offset..offset + channels]);
    }
    cropped
}
