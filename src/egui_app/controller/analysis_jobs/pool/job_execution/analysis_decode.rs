use crate::egui_app::controller::analysis_jobs::db;

use super::analysis::AnalysisContext;

pub(super) enum DecodeOutcome {
    Decoded(crate::analysis::audio::AnalysisAudio),
    Skipped {
        duration_seconds: f32,
        sample_rate: u32,
    },
}

pub(super) fn decode_for_analysis(
    job: &db::ClaimedJob,
    context: &AnalysisContext<'_>,
) -> Result<DecodeOutcome, String> {
    let (_source_id, relative_path) = db::parse_sample_id(&job.sample_id)?;
    let absolute = job.source_root.join(&relative_path);
    if context.max_analysis_duration_seconds.is_finite()
        && context.max_analysis_duration_seconds > 0.0
    {
        if let Ok(probe) = crate::analysis::audio::probe_metadata(&absolute) {
            if let Some(duration_seconds) = probe.duration_seconds {
                if duration_seconds > context.max_analysis_duration_seconds {
                    let sample_rate = probe
                        .sample_rate
                        .unwrap_or(crate::analysis::audio::ANALYSIS_SAMPLE_RATE);
                    return Ok(DecodeOutcome::Skipped {
                        duration_seconds,
                        sample_rate,
                    });
                }
            }
        }
    }
    let decode_limit_seconds =
        if context.max_analysis_duration_seconds.is_finite()
            && context.max_analysis_duration_seconds > 0.0
        {
            Some(context.max_analysis_duration_seconds)
        } else {
            None
        };
    let decoded = crate::analysis::audio::decode_for_analysis_with_rate_limit(
        &absolute,
        context.analysis_sample_rate,
        decode_limit_seconds,
    )?;
    Ok(DecodeOutcome::Decoded(decoded))
}

pub(super) fn build_logmel_for_embedding(
    mono: &[f32],
    sample_rate: u32,
    scratch: &mut crate::analysis::embedding::PannsLogMelScratch,
) -> Result<Vec<f32>, String> {
    let processed = crate::analysis::audio::preprocess_mono_for_embedding(mono, sample_rate);
    let mut logmel = vec![0.0_f32; crate::analysis::embedding::PANNS_LOGMEL_LEN];
    crate::analysis::embedding::build_panns_logmel_into(
        &processed,
        sample_rate,
        &mut logmel,
        scratch,
    )?;
    Ok(logmel)
}

pub(super) fn infer_embedding_from_logmel(logmel: &[f32]) -> Result<Vec<f32>, String> {
    crate::analysis::embedding::infer_embedding_from_logmel(logmel)
}

pub(super) fn infer_embedding_from_audio(
    decoded: &crate::analysis::audio::AnalysisAudio,
) -> Result<Vec<f32>, String> {
    let mut scratch = crate::analysis::embedding::PannsLogMelScratch::default();
    let logmel = build_logmel_for_embedding(&decoded.mono, decoded.sample_rate_used, &mut scratch)?;
    infer_embedding_from_logmel(&logmel)
}
