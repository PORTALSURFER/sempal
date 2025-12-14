use std::path::PathBuf;
use thiserror::Error;

#[derive(Debug, Error)]
pub enum WaveformDecodeError {
    #[error("Invalid wav: {message}")]
    Invalid { message: String },
    #[error("Sample error: {source}")]
    Sample { source: hound::Error },
}

#[derive(Debug, Error)]
pub enum WaveformLoadError {
    #[error("Failed to read {path}: {source}")]
    Read {
        path: PathBuf,
        source: std::io::Error,
    },
    #[error(transparent)]
    Decode(#[from] WaveformDecodeError),
}
