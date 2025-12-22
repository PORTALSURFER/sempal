use super::features::AnalysisFeaturesV1;

/// Current persisted feature vector version.
pub const FEATURE_VERSION_V1: i64 = 1;
/// Number of `f32` values stored for `FEATURE_VERSION_V1`.
pub const FEATURE_VECTOR_LEN_V1: usize = 183;

pub(crate) fn to_f32_vector_v1(features: &AnalysisFeaturesV1) -> Vec<f32> {
    let mut out = Vec::with_capacity(FEATURE_VECTOR_LEN_V1);

    let t = &features.time_domain;
    out.push(t.duration_seconds);
    out.push(t.peak);
    out.push(t.rms);
    out.push(t.crest_factor);
    out.push(t.zero_crossing_rate);
    out.push(t.attack_seconds);
    out.push(t.decay_20db_seconds);
    out.push(t.decay_40db_seconds);
    out.push(t.onset_count as f32);

    let f = &features.frequency_domain;
    push_stats(&mut out, &f.spectral.centroid_hz);
    push_stats(&mut out, &f.spectral.rolloff_hz);
    push_stats(&mut out, &f.spectral.flatness);
    push_stats(&mut out, &f.spectral.bandwidth_hz);
    push_stats(&mut out, &f.spectral.centroid_hz_early);
    push_stats(&mut out, &f.spectral.rolloff_hz_early);
    push_stats(&mut out, &f.spectral.flatness_early);
    push_stats(&mut out, &f.spectral.bandwidth_hz_early);
    push_stats(&mut out, &f.spectral.centroid_hz_late);
    push_stats(&mut out, &f.spectral.rolloff_hz_late);
    push_stats(&mut out, &f.spectral.flatness_late);
    push_stats(&mut out, &f.spectral.bandwidth_hz_late);

    push_stats(&mut out, &f.band_energy_ratios.sub);
    push_stats(&mut out, &f.band_energy_ratios.low);
    push_stats(&mut out, &f.band_energy_ratios.mid);
    push_stats(&mut out, &f.band_energy_ratios.high);
    push_stats(&mut out, &f.band_energy_ratios.air);
    push_stats(&mut out, &f.band_energy_ratios.sub_early);
    push_stats(&mut out, &f.band_energy_ratios.low_early);
    push_stats(&mut out, &f.band_energy_ratios.mid_early);
    push_stats(&mut out, &f.band_energy_ratios.high_early);
    push_stats(&mut out, &f.band_energy_ratios.air_early);
    push_stats(&mut out, &f.band_energy_ratios.sub_late);
    push_stats(&mut out, &f.band_energy_ratios.low_late);
    push_stats(&mut out, &f.band_energy_ratios.mid_late);
    push_stats(&mut out, &f.band_energy_ratios.high_late);
    push_stats(&mut out, &f.band_energy_ratios.air_late);

    push_vec(&mut out, &f.mfcc20.mean);
    push_vec(&mut out, &f.mfcc20.std);
    push_vec(&mut out, &f.mfcc20.mean_early);
    push_vec(&mut out, &f.mfcc20.std_early);
    push_vec(&mut out, &f.mfcc20.mean_late);
    push_vec(&mut out, &f.mfcc20.std_late);

    debug_assert_eq!(out.len(), FEATURE_VECTOR_LEN_V1);
    out
}

pub fn encode_f32_le_blob(values: &[f32]) -> Vec<u8> {
    let mut out = Vec::with_capacity(values.len().saturating_mul(4));
    for &v in values {
        out.extend_from_slice(&v.to_le_bytes());
    }
    out
}

/// Decode a little-endian `f32` blob (as stored in SQLite) into a `Vec<f32>`.
pub fn decode_f32_le_blob(blob: &[u8]) -> Result<Vec<f32>, String> {
    if blob.len() % 4 != 0 {
        return Err("Feature blob length is not a multiple of 4 bytes".to_string());
    }
    let mut out = Vec::with_capacity(blob.len() / 4);
    for chunk in blob.chunks_exact(4) {
        out.push(f32::from_le_bytes(
            chunk.try_into().expect("chunk size verified"),
        ));
    }
    Ok(out)
}

fn push_stats(out: &mut Vec<f32>, stats: &super::frequency_domain::Stats) {
    out.push(stats.mean);
    out.push(stats.std);
}

fn push_vec(out: &mut Vec<f32>, values: &[f32]) {
    out.extend_from_slice(values);
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::analysis::frequency_domain::{FrequencyDomainFeatures, Mfcc20Stats, Stats};
    use crate::analysis::time_domain::TimeDomainFeatures;
    use crate::analysis::audio::ANALYSIS_SAMPLE_RATE;

    #[test]
    fn vector_v1_has_stable_length() {
        let time_domain = TimeDomainFeatures {
            duration_seconds: 1.0,
            peak: 1.0,
            rms: 0.5,
            crest_factor: 2.0,
            zero_crossing_rate: 0.0,
            attack_seconds: 0.01,
            decay_20db_seconds: 0.1,
            decay_40db_seconds: 0.2,
            onset_count: 3,
        };
        let s = Stats { mean: 0.0, std: 0.0 };
        let frequency_domain = FrequencyDomainFeatures {
            sample_rate: ANALYSIS_SAMPLE_RATE,
            frame_size: 1024,
            hop_size: 512,
            spectral: crate::analysis::frequency_domain::SpectralAggregates {
                centroid_hz: s.clone(),
                rolloff_hz: s.clone(),
                flatness: s.clone(),
                bandwidth_hz: s.clone(),
                centroid_hz_early: s.clone(),
                rolloff_hz_early: s.clone(),
                flatness_early: s.clone(),
                bandwidth_hz_early: s.clone(),
                centroid_hz_late: s.clone(),
                rolloff_hz_late: s.clone(),
                flatness_late: s.clone(),
                bandwidth_hz_late: s.clone(),
            },
            band_energy_ratios: crate::analysis::frequency_domain::BandEnergyRatios {
                sub: s.clone(),
                low: s.clone(),
                mid: s.clone(),
                high: s.clone(),
                air: s.clone(),
                sub_early: s.clone(),
                low_early: s.clone(),
                mid_early: s.clone(),
                high_early: s.clone(),
                air_early: s.clone(),
                sub_late: s.clone(),
                low_late: s.clone(),
                mid_late: s.clone(),
                high_late: s.clone(),
                air_late: s.clone(),
            },
            mfcc20: Mfcc20Stats {
                mean: vec![0.0; 20],
                std: vec![0.0; 20],
                mean_early: vec![0.0; 20],
                std_early: vec![0.0; 20],
                mean_late: vec![0.0; 20],
                std_late: vec![0.0; 20],
            },
        };
        let features = AnalysisFeaturesV1::new(time_domain, frequency_domain);
        let vec = to_f32_vector_v1(&features);
        assert_eq!(vec.len(), FEATURE_VECTOR_LEN_V1);
    }

    #[test]
    fn encode_blob_is_little_endian() {
        let values = [1.0_f32, -2.5_f32];
        let blob = encode_f32_le_blob(&values);
        assert_eq!(blob.len(), 8);
        assert_eq!(&blob[0..4], &1.0_f32.to_le_bytes());
        assert_eq!(&blob[4..8], &(-2.5_f32).to_le_bytes());
    }

    #[test]
    fn decode_blob_round_trips() {
        let values = [1.0_f32, -2.5_f32, 0.125_f32];
        let blob = encode_f32_le_blob(&values);
        let decoded = decode_f32_le_blob(&blob).unwrap();
        assert_eq!(decoded, values);
    }

    #[test]
    fn decode_blob_rejects_non_multiple_of_4() {
        let err = decode_f32_le_blob(&[1, 2, 3]).unwrap_err();
        assert!(err.to_ascii_lowercase().contains("multiple of 4"));
    }
}
