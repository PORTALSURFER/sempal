//! Frequency-domain feature extraction (STFT + spectral statistics + MFCC).

mod mel;
mod stats;
mod stft;

use serde::{Deserialize, Serialize};

use mel::MelBank;

pub(crate) const STFT_FRAME_SIZE: usize = 1024;
pub(crate) const STFT_HOP_SIZE: usize = 512;

/// Mean and standard deviation for an aggregated metric.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub(crate) struct Stats {
    pub(crate) mean: f32,
    pub(crate) std: f32,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub(crate) struct SpectralAggregates {
    pub(crate) centroid_hz: Stats,
    pub(crate) rolloff_hz: Stats,
    pub(crate) flatness: Stats,
    pub(crate) bandwidth_hz: Stats,
    pub(crate) centroid_hz_early: Stats,
    pub(crate) rolloff_hz_early: Stats,
    pub(crate) flatness_early: Stats,
    pub(crate) bandwidth_hz_early: Stats,
    pub(crate) centroid_hz_late: Stats,
    pub(crate) rolloff_hz_late: Stats,
    pub(crate) flatness_late: Stats,
    pub(crate) bandwidth_hz_late: Stats,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub(crate) struct BandEnergyRatios {
    pub(crate) sub: Stats,
    pub(crate) low: Stats,
    pub(crate) mid: Stats,
    pub(crate) high: Stats,
    pub(crate) air: Stats,
    pub(crate) sub_early: Stats,
    pub(crate) low_early: Stats,
    pub(crate) mid_early: Stats,
    pub(crate) high_early: Stats,
    pub(crate) air_early: Stats,
    pub(crate) sub_late: Stats,
    pub(crate) low_late: Stats,
    pub(crate) mid_late: Stats,
    pub(crate) high_late: Stats,
    pub(crate) air_late: Stats,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub(crate) struct Mfcc20Stats {
    pub(crate) mean: Vec<f32>,
    pub(crate) std: Vec<f32>,
    pub(crate) mean_early: Vec<f32>,
    pub(crate) std_early: Vec<f32>,
    pub(crate) mean_late: Vec<f32>,
    pub(crate) std_late: Vec<f32>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub(crate) struct FrequencyDomainFeatures {
    pub(crate) sample_rate: u32,
    pub(crate) frame_size: usize,
    pub(crate) hop_size: usize,
    pub(crate) spectral: SpectralAggregates,
    pub(crate) band_energy_ratios: BandEnergyRatios,
    pub(crate) mfcc20: Mfcc20Stats,
}

/// Extract frequency-domain features from analysis-normalized mono audio.
pub(crate) fn extract_frequency_domain_features(
    samples: &[f32],
    sample_rate: u32,
) -> FrequencyDomainFeatures {
    let mel = MelBank::new(sample_rate, STFT_FRAME_SIZE, 40, 20, 20.0, 16_000.0);
    let frames = stft::compute_frames(samples, sample_rate, STFT_FRAME_SIZE, STFT_HOP_SIZE, &mel);
    let (early, late) = stats::early_late_ranges(frames.spectral.len());
    FrequencyDomainFeatures {
        sample_rate,
        frame_size: STFT_FRAME_SIZE,
        hop_size: STFT_HOP_SIZE,
        spectral: stats::spectral_aggregates(&frames.spectral, early.clone(), late.clone()),
        band_energy_ratios: stats::band_aggregates(&frames.bands, early.clone(), late.clone()),
        mfcc20: stats::mfcc_aggregates(&frames.mfcc, early, late),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sine_wave_centroid_tracks_frequency() {
        let sr = 22_050;
        let freq = 440.0_f32;
        let len = sr as usize / 2;
        let samples: Vec<f32> = (0..len)
            .map(|i| (2.0 * std::f32::consts::PI * freq * i as f32 / sr as f32).sin())
            .collect();
        let feats = extract_frequency_domain_features(&samples, sr);
        let centroid = feats.spectral.centroid_hz.mean;
        assert!(centroid > 200.0 && centroid < 800.0);
        assert!(feats.spectral.flatness.mean < 0.5);
    }

    #[test]
    fn mfcc_is_deterministic_for_same_input() {
        let sr = 22_050;
        let samples = vec![0.1_f32; sr as usize / 5];
        let a = extract_frequency_domain_features(&samples, sr);
        let b = extract_frequency_domain_features(&samples, sr);
        assert_eq!(a.mfcc20.mean, b.mfcc20.mean);
        assert_eq!(a.mfcc20.std, b.mfcc20.std);
    }
}
