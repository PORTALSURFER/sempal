use std::fs::File;
use std::path::Path;

use symphonia::core::{
    audio::SampleBuffer, codecs::DecoderOptions, errors::Error, formats::FormatOptions,
    io::MediaSourceStream, meta::MetadataOptions, probe::Hint,
};

/// Raw decoded audio in interleaved `f32` samples.
pub(crate) struct DecodedAudio {
    pub(crate) samples: Vec<f32>,
    pub(crate) sample_rate: u32,
    pub(crate) channels: u16,
}

/// Decode audio into interleaved `f32` samples with sample rate and channel count.
pub(crate) fn decode_audio(path: &Path, max_seconds: Option<f32>) -> Result<DecodedAudio, String> {
    match decode_with_symphonia(path, max_seconds) {
        Ok((samples, sample_rate, channels)) => Ok(DecodedAudio {
            samples,
            sample_rate: sample_rate.max(1),
            channels: channels.max(1),
        }),
        Err(err) => Err(format!(
            "Audio decode failed for {}: {err}",
            path.display()
        )),
    }
}

fn decode_with_symphonia(
    path: &Path,
    max_seconds: Option<f32>,
) -> Result<(Vec<f32>, u32, u16), String> {
    let file = File::open(path).map_err(|err| format!("Open {}: {err}", path.display()))?;
    let mss = MediaSourceStream::new(Box::new(file), Default::default());
    let mut hint = Hint::new();
    if let Some(ext) = path.extension().and_then(|ext| ext.to_str()) {
        hint.with_extension(ext);
    }

    let probed = symphonia::default::get_probe()
        .format(
            &hint,
            mss,
            &FormatOptions::default(),
            &MetadataOptions::default(),
        )
        .map_err(|err| format!("Symphonia probe failed for {}: {err}", path.display()))?;
    let mut format = probed.format;
    let track = format
        .default_track()
        .ok_or_else(|| format!("No default track for {}", path.display()))?;
    let codec_params = &track.codec_params;
    let sample_rate = codec_params
        .sample_rate
        .ok_or_else(|| format!("Missing sample rate for {}", path.display()))?;
    let channels = codec_params
        .channels
        .ok_or_else(|| format!("Missing channel count for {}", path.display()))?
        .count() as u16;
    let max_samples = max_seconds.filter(|limit| *limit > 0.0).map(|limit| {
        let frames = (limit * sample_rate as f32).ceil().max(1.0);
        (frames as usize).saturating_mul(channels as usize).max(1)
    });

    let mut decoder = symphonia::default::get_codecs()
        .make(codec_params, &DecoderOptions::default())
        .map_err(|err| format!("Symphonia decoder failed for {}: {err}", path.display()))?;

    let mut samples = Vec::new();
    loop {
        if max_samples.is_some_and(|limit| samples.len() >= limit) {
            break;
        }
        let packet = match format.next_packet() {
            Ok(packet) => packet,
            Err(Error::IoError(_)) => break,
            Err(err) => {
                return Err(format!(
                    "Symphonia packet read failed for {}: {err}",
                    path.display()
                ));
            }
        };
        let audio_buf = match decoder.decode(&packet) {
            Ok(audio_buf) => audio_buf,
            Err(Error::DecodeError(_)) => continue,
            Err(err) => {
                return Err(format!(
                    "Symphonia decode failed for {}: {err}",
                    path.display()
                ));
            }
        };
        let spec = *audio_buf.spec();
        let mut sample_buf = SampleBuffer::<f32>::new(audio_buf.capacity() as u64, spec);
        sample_buf.copy_interleaved_ref(audio_buf);
        samples.extend_from_slice(sample_buf.samples());
        if let Some(limit) = max_samples {
            if samples.len() >= limit {
                samples.truncate(limit);
                break;
            }
        }
    }

    if samples.is_empty() {
        return Err(format!(
            "Symphonia decoded 0 samples for {}",
            path.display()
        ));
    }

    Ok((samples, sample_rate, channels))
}
