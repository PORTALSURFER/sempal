//! PANNs embedding inference and batching utilities.

mod backend;
mod infer;
mod logmel;
mod model;
mod query;

mod panns_burn {
    include!(concat!(env!("OUT_DIR"), "/burn_panns/panns_cnn14_16k.rs"));
}

mod panns_paths {
    include!(concat!(env!("OUT_DIR"), "/burn_panns/panns_paths.rs"));
}

/// PANNs embedding model identifier used for caching and lookup.
pub const EMBEDDING_MODEL_ID: &str =
    "panns_cnn14_16k__sr16k__nfft512__hop160__mel64__log10__chunk10__repeatpad_v1";
/// Output embedding dimension for the PANNs model.
pub const EMBEDDING_DIM: usize = 2048;
/// Data type label for stored embeddings.
pub const EMBEDDING_DTYPE_F32: &str = "f32";

#[allow(unused_imports)]
pub(crate) use backend::{
    embedding_batch_max, embedding_inflight_max, embedding_model_path, embedding_pipeline_enabled,
    panns_burnpack_path,
};
#[allow(unused_imports)]
pub(crate) use infer::{
    infer_embedding, infer_embedding_from_logmel, infer_embedding_query,
    infer_embeddings_batch, infer_embeddings_from_logmel_batch,
    infer_embeddings_from_logmel_batch_chunked, infer_embeddings_from_logmel_batch_pipelined,
    EmbeddingBatchInput,
};
#[allow(unused_imports)]
pub(crate) use logmel::{build_panns_logmel_into, PannsLogMelScratch, PANNS_LOGMEL_LEN};
#[allow(unused_imports)]
pub(crate) use model::{warmup_panns, PannsModel};

#[cfg(test)]
mod tests {
    use super::*;
    use serde::Deserialize;

    #[derive(Deserialize)]
    struct GoldenEmbedding {
        sample_rate: u32,
        tone_hz: f32,
        tone_amp: f32,
        tone_seconds: f32,
        target_seconds: f32,
        embedding: Vec<f32>,
    }

    #[test]
    fn golden_embedding_matches_python() {
        let path = std::env::var("SEMPAL_PANNS_EMBED_GOLDEN_PATH")
            .ok()
            .filter(|path| !path.trim().is_empty());
        let Some(path) = path else {
            return;
        };
        if !panns_burnpack_path().map(|p| p.exists()).unwrap_or(false) {
            return;
        }
        let payload = std::fs::read_to_string(path).expect("read golden json");
        let golden: GoldenEmbedding = serde_json::from_str(&payload).expect("parse golden json");
        assert_eq!(golden.embedding.len(), EMBEDDING_DIM);

        let tone_len = (golden.sample_rate as f32 * golden.tone_seconds).round() as usize;
        let mut tone = Vec::with_capacity(tone_len);
        for i in 0..tone_len {
            let t = i as f32 / golden.sample_rate.max(1) as f32;
            let sample = (2.0 * std::f32::consts::PI * golden.tone_hz * t).sin() * golden.tone_amp;
            tone.push(sample);
        }
        let target_len = (golden.sample_rate as f32 * golden.target_seconds).round() as usize;
        let mut padded = Vec::new();
        logmel::repeat_pad_into(&mut padded, &tone, target_len);

        let embedding = infer_embedding(&padded, golden.sample_rate).expect("rust embedding");
        assert_eq!(embedding.len(), golden.embedding.len());

        let mut max_diff = 0.0_f32;
        for (&a, &b) in embedding.iter().zip(golden.embedding.iter()) {
            max_diff = max_diff.max((a - b).abs());
        }
        const MAX_DIFF: f32 = 1e-3;
        assert!(
            max_diff <= MAX_DIFF,
            "max diff {max_diff} exceeds {MAX_DIFF}"
        );
    }
}
