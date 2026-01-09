use std::path::PathBuf;
use thiserror::Error;

#[derive(Debug, Error)]
/// Errors reported while decoding waveform audio data.
pub enum WaveformDecodeError {
    #[error("Invalid wav: {message}")]
    Invalid { message: String },
    #[error("Sample error: {source}")]
    Sample { source: hound::Error },
}

#[derive(Debug, Error)]
/// Errors reported while loading waveform data from disk.
pub enum WaveformLoadError {
    #[error("Waveform file {path} is too large ({size_bytes} bytes, limit {limit_bytes} bytes)")]
    TooLarge {
        path: PathBuf,
        size_bytes: u64,
        limit_bytes: u64,
    },
    #[error("Failed to read metadata for {path}: {source}")]
    Metadata {
        path: PathBuf,
        source: std::io::Error,
    },
    #[error("Failed to read {path}: {source}")]
    Read {
        path: PathBuf,
        source: std::io::Error,
    },
    #[error(transparent)]
    Decode(#[from] WaveformDecodeError),
}
