use std::sync::LazyLock;

use crate::analysis::{audio, embedding};

pub(crate) fn analysis_version() -> &'static str {
    &ANALYSIS_VERSION
}

pub(crate) fn analysis_version_for_sample_rate(sample_rate: u32) -> String {
    let payload = format!(
        "embedder={}|sr={}|max={}|window={}|hop={}|min={}|trim_on_db={}|trim_off_db={}|pre={}|post={}",
        embedding::EMBEDDING_MODEL_ID,
        sample_rate,
        audio::MAX_ANALYSIS_SECONDS,
        audio::WINDOW_SECONDS,
        audio::WINDOW_HOP_SECONDS,
        audio::MIN_ANALYSIS_SECONDS,
        audio::SILENCE_THRESHOLD_ON_DB,
        audio::SILENCE_THRESHOLD_OFF_DB,
        audio::SILENCE_PRE_ROLL_SECONDS,
        audio::SILENCE_POST_ROLL_SECONDS
    );
    let hash = blake3::hash(payload.as_bytes());
    format!("analysis_v1_{}", hash.to_hex())
}

static ANALYSIS_VERSION: LazyLock<String> =
    LazyLock::new(|| analysis_version_for_sample_rate(audio::ANALYSIS_SAMPLE_RATE));
