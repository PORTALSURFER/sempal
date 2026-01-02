pub(crate) const PANNS_STFT_N_FFT: usize = 512;
pub(crate) const PANNS_STFT_HOP: usize = 160;
pub(crate) const PANNS_MEL_BANDS: usize = 64;
pub(crate) const PANNS_MEL_FMIN_HZ: f32 = 50.0;
pub(crate) const PANNS_MEL_FMAX_HZ: f32 = 8_000.0;

mod mel;
mod preprocessor;
mod stft;

pub(crate) use mel::PannsMelBank;
pub(crate) use preprocessor::{log_mel_frames, PannsPreprocessor};
pub(crate) use stft::{stft_power_frames, stft_power_frames_into_flat};

#[cfg(test)]
mod tests;
