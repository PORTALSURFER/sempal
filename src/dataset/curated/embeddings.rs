use crate::sample_sources::config::TrainingAugmentation;

/// Embedding + lightweight feature vector derived from an audio sample.
#[derive(Debug, Clone)]
pub(super) struct EmbeddingVariant {
    pub(super) embedding: Vec<f32>,
    pub(super) light_features: Vec<f32>,
}

/// Build a deterministic augmentation RNG for curated dataset ingestion.
pub(super) fn augment_rng(
    augmentation: &TrainingAugmentation,
    seed: u64,
) -> rand::rngs::StdRng {
    crate::analysis::augment::AugmentOptions {
        enabled: augmentation.enabled,
        copies_per_sample: augmentation.copies_per_sample,
        gain_jitter_db: augmentation.gain_jitter_db,
        noise_std: augmentation.noise_std,
        pitch_semitones: augmentation.pitch_semitones,
        time_stretch_pct: augmentation.time_stretch_pct,
        seed,
    }
    .rng()
}

/// Build embedding variants (base + optional augmentations) for a decoded sample.
pub(super) fn build_embedding_variants(
    decoded: &crate::analysis::audio::AnalysisAudio,
    augmentation: &TrainingAugmentation,
    rng: &mut rand::rngs::StdRng,
) -> Result<Vec<EmbeddingVariant>, String> {
    let mut variants = Vec::new();
    let base_samples = if augmentation.preprocess {
        crate::analysis::audio::preprocess_mono_for_embedding(
            &decoded.mono,
            decoded.sample_rate_used,
        )
    } else {
        decoded.mono.clone()
    };
    let base =
        crate::analysis::embedding::infer_embedding(&base_samples, decoded.sample_rate_used)?;
    let base_light = time_domain_vector(&base_samples, decoded.sample_rate_used);
    variants.push(EmbeddingVariant {
        embedding: base,
        light_features: base_light,
    });

    if augmentation.enabled && augmentation.copies_per_sample > 0 {
        let options = crate::analysis::augment::AugmentOptions {
            enabled: true,
            copies_per_sample: augmentation.copies_per_sample,
            gain_jitter_db: augmentation.gain_jitter_db,
            noise_std: augmentation.noise_std,
            pitch_semitones: augmentation.pitch_semitones,
            time_stretch_pct: augmentation.time_stretch_pct,
            seed: 0,
        };
        for _ in 0..augmentation.copies_per_sample {
            let augmented = crate::analysis::augment::augment_waveform(
                &decoded.mono,
                rng,
                &options,
            );
            let processed = if augmentation.preprocess {
                crate::analysis::audio::preprocess_mono_for_embedding(
                    &augmented,
                    decoded.sample_rate_used,
                )
            } else {
                augmented
            };
            let embedding = crate::analysis::embedding::infer_embedding(
                &processed,
                decoded.sample_rate_used,
            )?;
            let light = time_domain_vector(&processed, decoded.sample_rate_used);
            variants.push(EmbeddingVariant {
                embedding,
                light_features: light,
            });
        }
    }

    Ok(variants)
}

fn time_domain_vector(samples: &[f32], sample_rate: u32) -> Vec<f32> {
    let feats = crate::analysis::time_domain::extract_time_domain_features(samples, sample_rate);
    vec![
        feats.duration_seconds,
        feats.peak,
        feats.rms,
        feats.crest_factor,
        feats.zero_crossing_rate,
        feats.attack_seconds,
        feats.decay_20db_seconds,
        feats.decay_40db_seconds,
        feats.onset_count as f32,
    ]
}
