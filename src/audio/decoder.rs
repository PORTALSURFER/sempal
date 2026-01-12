//! Symphonia-based audio decoder implementing the `Source` trait.

use std::io::Cursor;
use std::sync::Arc;
use symphonia::core::audio::{AudioBufferRef, Signal};
use symphonia::core::codecs::{Decoder, DecoderOptions};
use symphonia::core::errors::Error;
use symphonia::core::formats::{FormatOptions, FormatReader};
use symphonia::core::io::MediaSourceStream;
use symphonia::core::probe::Hint;
use std::time::Duration;

use super::Source;

/// Streaming decoder that yields interleaved `f32` samples.
pub struct SymphoniaDecoder {
    reader: Box<dyn FormatReader>,
    decoder: Box<dyn Decoder>,
    buffer: Vec<f32>,
    buffer_pos: usize,
    sample_rate: u32,
    channels: u16,
    total_duration: Option<Duration>,
    last_error: Option<String>,
}

impl SymphoniaDecoder {
    /// Create a decoder for a media source stream using a WAV hint.
    pub fn new(mss: MediaSourceStream) -> Result<Self, String> {
        Self::new_with_hint(mss, "wav")
    }

    /// Create a decoder for a media source stream with an explicit extension hint.
    pub fn new_with_hint(mss: MediaSourceStream, hint_str: &str) -> Result<Self, String> {
        let mut hint = Hint::new();
        hint.with_extension(hint_str);

        let probed = symphonia::default::get_probe()
            .format(&hint, mss, &FormatOptions::default(), &Default::default())
            .map_err(|e| format!("Symphonia probe failed: {}", e))?;

        let reader = probed.format;
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
            buffer: Vec::new(),
            buffer_pos: 0,
            sample_rate,
            channels,
            total_duration,
            last_error: None,
        })
    }

    /// Create a decoder from an in-memory byte buffer.
    pub fn from_bytes(data: Arc<[u8]>) -> Result<Self, String> {
        let cursor = Cursor::new(data);
        let mss = MediaSourceStream::new(Box::new(cursor), Default::default());
        Self::new(mss)
    }

    /// Record a hint for the decoder (currently a no-op).
    pub fn set_hint(&mut self, _hint: &str) {
        // Hint must be provided at probe time in Symphonia.
        // For now we just ignore this or we'd have to re-probe.
    }

    /// Attempt to seek to an absolute playback timestamp.
    pub fn try_seek(&mut self, duration: Duration) -> Result<(), String> {
        self.reader.seek(symphonia::core::formats::SeekMode::Coarse, symphonia::core::formats::SeekTo::Time {
            time: symphonia::core::units::Time::new(duration.as_secs(), duration.subsec_nanos() as f64 / 1_000_000_000.0),
            track_id: None,
        }).map_err(|e| format!("Seek failed: {}", e))?;
        
        self.buffer.clear();
        self.buffer_pos = 0;
        Ok(())
    }
}

impl Iterator for SymphoniaDecoder {
    type Item = f32;

    fn next(&mut self) -> Option<Self::Item> {
        loop {
            if self.buffer_pos < self.buffer.len() {
                let sample = self.buffer[self.buffer_pos];
                self.buffer_pos += 1;
                return Some(sample);
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

            // Interleave samples into the buffer
            self.buffer.clear();
            self.buffer_pos = 0;
            
            match decoded {
                AudioBufferRef::F32(buf) => {
                    let channels = buf.spec().channels.count();
                    let frames = buf.frames();
                    for frame in 0..frames {
                        for chan in 0..channels {
                            self.buffer.push(buf.chan(chan)[frame]);
                        }
                    }
                }
                AudioBufferRef::U8(buf) => {
                    let channels = buf.spec().channels.count();
                    let frames = buf.frames();
                    for frame in 0..frames {
                        for chan in 0..channels {
                            self.buffer.push(buf.chan(chan)[frame] as f32 / 128.0 - 1.0);
                        }
                    }
                }
                AudioBufferRef::U16(buf) => {
                    let channels = buf.spec().channels.count();
                    let frames = buf.frames();
                    for frame in 0..frames {
                        for chan in 0..channels {
                            self.buffer.push(buf.chan(chan)[frame] as f32 / 32768.0 - 1.0);
                        }
                    }
                }
                AudioBufferRef::S16(buf) => {
                    let channels = buf.spec().channels.count();
                    let frames = buf.frames();
                    for frame in 0..frames {
                        for chan in 0..channels {
                            self.buffer.push(buf.chan(chan)[frame] as f32 / 32768.0);
                        }
                    }
                }
                AudioBufferRef::S32(buf) => {
                    let channels = buf.spec().channels.count();
                    let frames = buf.frames();
                    for frame in 0..frames {
                        for chan in 0..channels {
                            self.buffer.push(buf.chan(chan)[frame] as f32 / 2147483648.0);
                        }
                    }
                }
                AudioBufferRef::S24(buf) => {
                    let channels = buf.spec().channels.count();
                    let frames = buf.frames();
                    for frame in 0..frames {
                        for chan in 0..channels {
                            self.buffer.push(buf.chan(chan)[frame].0 as f32 / 8388608.0);
                        }
                    }
                }
                AudioBufferRef::U24(buf) => {
                    let channels = buf.spec().channels.count();
                    let frames = buf.frames();
                    for frame in 0..frames {
                        for chan in 0..channels {
                            self.buffer.push(buf.chan(chan)[frame].0 as f32 / 8388608.0 - 1.0);
                        }
                    }
                }
                AudioBufferRef::U32(buf) => {
                    let channels = buf.spec().channels.count();
                    let frames = buf.frames();
                    for frame in 0..frames {
                        for chan in 0..channels {
                            self.buffer.push(buf.chan(chan)[frame] as f32 / 2147483648.0 - 1.0);
                        }
                    }
                }
                AudioBufferRef::S8(buf) => {
                    let channels = buf.spec().channels.count();
                    let frames = buf.frames();
                    for frame in 0..frames {
                        for chan in 0..channels {
                            self.buffer.push(buf.chan(chan)[frame] as f32 / 128.0);
                        }
                    }
                }
                _ => {
                    let msg = "Unsupported audio format in symphonia decoder".to_string();
                    tracing::warn!("{}", msg);
                    self.last_error = Some(msg);
                    return None;
                }
            }
        }
    }
}

impl Source for SymphoniaDecoder {
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
        self.total_duration
    }

    fn last_error(&self) -> Option<String> {
        self.last_error.clone()
    }
}
