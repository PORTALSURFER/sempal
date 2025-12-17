//! Background analysis helpers (decoding, normalization, feature extraction).

pub(crate) mod audio;
pub(crate) mod features;
pub(crate) mod fft;
pub(crate) mod frequency_domain;
pub(crate) mod time_domain;
pub(crate) mod vector;

pub use vector::{FEATURE_VECTOR_LEN_V1, FEATURE_VERSION_V1};
