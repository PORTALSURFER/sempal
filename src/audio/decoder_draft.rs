use std::io::{Cursor, Read, Seek, SeekFrom};
use std::sync::Arc;
use symphonia::core::audio::AudioBufferRef;
use symphonia::core::audio::Signal;
use symphonia::core::codecs::{Decoder, DecoderOptions};
use symphonia::core::errors::Error;
use symphonia::core::formats::{FormatOptions, FormatReader};
use symphonia::core::io::ReadOnlySource;
use symphonia::core::io::MediaSourceStream;
use symphonia::core::probe::Hint;
use std::time::Duration;

use super::Source;

pub struct SymphoniaDecoder {
    reader: Box<dyn FormatReader>,
    decoder: Box<dyn Decoder>,
    current_buffer: Option<AudioBufferRef<'static>>,
    buffer_pos: usize,
    sample_rate: u32,
    channels: u16,
    total_duration: Option<Duration>,
}

impl SymphoniaDecoder {
    pub fn new(data: Arc<[u8]>) -> Result<Self, String> {
        let cursor = Cursor::new(data);
        let mss = MediaSourceStream::new(Box::new(cursor), Default::default());
        
        let mut hint = Hint::new();
        hint.with_extension("wav");

        let probed = symphonia::default::get_probe()
            .format(&hint, mss, &FormatOptions::default(), &Default::default())
            .map_err(|e| format!("Symphonia probe failed: {}", e))?;

        let mut reader = probed.format;
        let track = reader.default_track().ok_or("No default track found")?;
        
        let decoder = symphonia::default::get_codecs()
            .make(&track.codec_params, &DecoderOptions::default())
            .map_err(|e| format!("Symphonia decoder creation failed: {}", e))?;

        let sample_rate = track.codec_params.sample_rate.unwrap_or(44100);
        let channels = track.codec_params.channels.map(|c| c.count() as u16).unwrap_or(2);
        
        let total_duration = track.codec_params.n_frames.map(|frames| {
            Duration::from_nanos((frames * 1_000_000_000) / sample_rate as u64)
        });

        Ok(Self {
            reader,
            decoder,
            current_buffer: None,
            buffer_pos: 0,
            sample_rate,
            channels,
            total_duration,
        })
    }

    pub fn try_seek(&mut self, duration: Duration) -> Result<(), String> {
        let timestamp = (duration.as_secs_f64() * self.sample_rate as f64) as u64;
        self.reader.seek(symphonia::core::formats::SeekMode::Accurate, symphonia::core::formats::SeekTo::Time {
            time: symphonia::core::units::Time::new(duration.as_secs(), duration.subsec_nanos() as f64 / 1_000_000_000.0),
            track_id: None,
        }).map_err(|e| format!("Seek failed: {}", e))?;
        
        self.current_buffer = None;
        self.buffer_pos = 0;
        Ok(())
    }
}

impl Iterator for SymphoniaDecoder {
    type Item = f32;

    fn next(&mut self) -> Option<Self::Item> {
        loop {
            if let Some(buf) = &self.current_buffer {
                let spec = buf.spec();
                let planes = buf.planes();
                let samples_per_plane = buf.frames();
                let total_samples = samples_per_plane * spec.channels.count();

                if self.buffer_pos < total_samples {
                    // Interleave samples
                    let chan_idx = self.buffer_pos % spec.channels.count();
                    let frame_idx = self.buffer_pos / spec.channels.count();
                    let sample = match buf {
                        AudioBufferRef::F32(b) => b.chan(chan_idx)[frame_idx],
                        AudioBufferRef::U8(b) => b.chan(chan_idx)[frame_idx] as f32 / 128.0 - 1.0,
                        AudioBufferRef::U16(b) => b.chan(chan_idx)[frame_idx] as f32 / 32768.0 - 1.0,
                        AudioBufferRef::U24(b) => b.chan(chan_idx)[frame_idx] as f32 / 8388608.0 - 1.0,
                        AudioBufferRef::U32(b) => b.chan(chan_idx)[frame_idx] as f32 / 2147483648.0 - 1.0,
                        AudioBufferRef::S8(b) => b.chan(chan_idx)[frame_idx] as f32 / 128.0,
                        AudioBufferRef::S16(b) => b.chan(chan_idx)[frame_idx] as f32 / 32768.0,
                        AudioBufferRef::S24(b) => b.chan(chan_idx)[frame_idx] as f32 / 8388608.0,
                        AudioBufferRef::S32(b) => b.chan(chan_idx)[frame_idx] as f32 / 2147483648.0,
                        _ => 0.0,
                    };
                    self.buffer_pos += 1;
                    return Some(sample);
                }
            }

            // Need more data
            let packet = match self.reader.next_packet() {
                Ok(p) => p,
                Err(Error::IoError(ref e)) if e.kind() == std::io::ErrorKind::UnexpectedEof => return None,
                Err(e) => {
                    tracing::error!("Symphonia error: {}", e);
                    return None;
                }
            };

            let decoded = match self.decoder.decode(&packet) {
                Ok(d) => d,
                Err(Error::DecodeError(e)) => {
                    tracing::warn!("Symphonia decode error: {}", e);
                    continue;
                }
                Err(e) => {
                    tracing::error!("Symphonia error: {}", e);
                    return None;
                }
            };

            // Convert to static lifetime by cloning the buffer
            // This is a bit expensive but symphonia 0.5.x doesn't make it easy to keep the buffer.
            // Actually, we can just store the decoded buffer and use it.
            // But next() needs to return a sample, so we need to hold onto it.
            // To avoid 'static issues, we can use a local buffer.
            
            // Wait, I'll use a simpler approach: store a Vec<f32> for the interleaved samples of the current packet.
            // This avoids complex lifetime issues with AudioBufferRef.
            
            self.current_buffer = Some(decoded);
            self.buffer_pos = 0;
        }
    }
}

// Wait, the 'static lifetime in AudioBufferRef<'static> is a problem.
// Let's refactor to store the interleaved samples.
