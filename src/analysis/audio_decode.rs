use std::fs::File;
use std::path::Path;

use symphonia::core::{
    audio::SampleBuffer,
    codecs::DecoderOptions,
    errors::Error,
    formats::FormatOptions,
    io::MediaSourceStream,
    meta::MetadataOptions,
    probe::Hint,
};

pub(crate) fn decode_with_symphonia(
    path: &Path,
) -> Result<(Vec<f32>, u32, u16), String> {
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
