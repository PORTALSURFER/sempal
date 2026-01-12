use std::path::PathBuf;
use thiserror::Error;

/// Errors reported while decoding waveform audio data.
#[derive(Debug, Error)]
pub enum WaveformDecodeError {
    /// The WAV header or payload is malformed.
    #[error("Invalid wav: {message}")]
    Invalid {
        /// Human-readable validation error.
        message: String,
    },
    /// Failed while reading WAV samples.
    #[error("Sample error: {source}")]
    Sample {
        /// Underlying WAV decode error.
        source: hound::Error,
    },
}

/// Errors reported while loading waveform data from disk.
#[derive(Debug, Error)]
pub enum WaveformLoadError {
    /// The file exceeds the configured size limit.
    #[error("Waveform file {path} is too large ({size_bytes} bytes, limit {limit_bytes} bytes)")]
    TooLarge {
        /// Path to the oversized file.
        path: PathBuf,
        /// Size of the file in bytes.
        size_bytes: u64,
        /// Maximum allowed size in bytes.
        limit_bytes: u64,
    },
    /// Failed to read file metadata.
    #[error("Failed to read metadata for {path}: {source}")]
    Metadata {
        /// Path whose metadata was requested.
        path: PathBuf,
        /// Underlying IO error.
        source: std::io::Error,
    },
    /// Failed to read file contents.
    #[error("Failed to read {path}: {source}")]
    Read {
        /// Path that could not be read.
        path: PathBuf,
        /// Underlying IO error.
        source: std::io::Error,
    },
    /// Failed to decode the waveform payload.
    #[error(transparent)]
    Decode(#[from] WaveformDecodeError),
}
