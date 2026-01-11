use std::io::Cursor;
use std::sync::Arc;

use crate::audio::decoder::SymphoniaDecoder;
use crate::audio::Source;

pub(crate) fn decoder_from_bytes(bytes: Arc<[u8]>) -> Result<SymphoniaDecoder, String> {
    SymphoniaDecoder::from_bytes(bytes)
}

pub(crate) fn decoder_duration(bytes: &Arc<[u8]>) -> Option<f32> {
    decoder_from_bytes(bytes.clone())
        .ok()
        .and_then(|decoder| decoder.total_duration())
        .map(|duration| duration.as_secs_f32())
}

pub(crate) fn wav_header_duration(bytes: &Arc<[u8]>) -> Option<f32> {
    wav_spec_from_bytes(bytes).map(|(duration, _)| duration)
}

pub(crate) fn wav_spec_from_bytes(bytes: &Arc<[u8]>) -> Option<(f32, u32)> {
    let reader = hound::WavReader::new(Cursor::new(bytes.clone())).ok()?;
    let spec = reader.spec();
    let sample_rate = spec.sample_rate as f32;
    let channels = spec.channels.max(1) as f32;
    if sample_rate <= 0.0 {
        return None;
    }
    let duration = reader.duration() as f32 / (sample_rate * channels);
    Some((duration, spec.sample_rate))
}

pub(crate) fn map_seek_error(error: String) -> String {
    format!("Audio seek failed: {error}")
}
