use std::io::Cursor;
use std::sync::Arc;

use rodio::Decoder;

pub(crate) fn decoder_from_bytes(bytes: Arc<[u8]>) -> Result<Decoder<Cursor<Arc<[u8]>>>, String> {
    let byte_len = bytes.len() as u64;
    Decoder::builder()
        .with_data(Cursor::new(bytes))
        .with_byte_len(byte_len)
        .with_seekable(true)
        .with_hint("wav")
        .build()
        .map_err(|error| format!("Audio decode failed: {error}"))
}

pub(crate) fn decoder_duration(bytes: &Arc<[u8]>) -> Option<f32> {
    decoder_from_bytes(bytes.clone())
        .ok()
        .and_then(|decoder| decoder.total_duration())
        .map(|duration| duration.as_secs_f32())
}

pub(crate) fn wav_header_duration(bytes: &Arc<[u8]>) -> Option<f32> {
    let reader = hound::WavReader::new(Cursor::new(bytes.clone())).ok()?;
    let spec = reader.spec();
    let sample_rate = spec.sample_rate as f32;
    let channels = spec.channels.max(1) as f32;
    if sample_rate <= 0.0 {
        return None;
    }
    Some(reader.duration() as f32 / (sample_rate * channels))
}

pub(crate) fn map_seek_error(error: rodio::source::SeekError) -> String {
    match error {
        rodio::source::SeekError::NotSupported { .. } => {
            "Seeking not supported for this audio source".into()
        }
        _ => format!("Audio seek failed: {error}"),
    }
}
