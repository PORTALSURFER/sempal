use std::fs::File;
use std::io::BufReader;
use std::path::Path;

use rodio::Decoder;
use symphonia::core::{
    audio::SampleBuffer,
    codecs::DecoderOptions,
    errors::Error,
    formats::FormatOptions,
    io::MediaSourceStream,
    meta::MetadataOptions,
    probe::Hint,
};

/// Raw decoded audio in interleaved `f32` samples.
pub(crate) struct DecodedAudio {
    pub(crate) samples: Vec<f32>,
    pub(crate) sample_rate: u32,
    pub(crate) channels: u16,
}

/// Decode audio into interleaved `f32` samples with sample rate and channel count.
///
/// Supported formats include wav/aiff/flac/mp3 via rodio, with a symphonia fallback.
pub(crate) fn decode_audio(path: &Path) -> Result<DecodedAudio, String> {
    let file = File::open(path).map_err(|err| format!("Failed to open {}: {err}", path.display()))?;
    let byte_len = file
        .metadata()
        .map(|meta| meta.len())
        .unwrap_or(0) as u64;
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
    let decoder = builder.build();
    match decoder {
        Ok(decoder) => {
            let sample_rate = decoder.sample_rate().max(1);
            let channels = decoder.channels().max(1);
            let samples: Vec<f32> = decoder.collect();
            Ok(DecodedAudio {
                samples,
                sample_rate,
                channels,
            })
        }
        Err(err) => match decode_with_symphonia(path) {
            Ok((samples, sample_rate, channels)) => Ok(DecodedAudio {
                samples,
                sample_rate: sample_rate.max(1),
                channels: channels.max(1),
            }),
            Err(fallback_err) => Err(format!(
                "Audio decode failed for {}: {err}. Symphonia fallback failed: {fallback_err}",
                path.display()
            )),
        },
    }
}

fn decode_with_symphonia(path: &Path) -> Result<(Vec<f32>, u32, u16), String> {
    let file = File::open(path).map_err(|err| format!("Open {}: {err}", path.display()))?;
    let mss = MediaSourceStream::new(Box::new(file), Default::default());
    let mut hint = Hint::new();
    if let Some(ext) = path.extension().and_then(|ext| ext.to_str()) {
        hint.with_extension(ext);
    }

    let probed = symphonia::default::get_probe()
        .format(&hint, mss, &FormatOptions::default(), &MetadataOptions::default())
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

    let mut decoder = symphonia::default::get_codecs()
        .make(codec_params, &DecoderOptions::default())
        .map_err(|err| format!("Symphonia decoder failed for {}: {err}", path.display()))?;

    let mut samples = Vec::new();
    loop {
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
    }

    if samples.is_empty() {
        return Err(format!("Symphonia decoded 0 samples for {}", path.display()));
    }

    Ok((samples, sample_rate, channels))
}
