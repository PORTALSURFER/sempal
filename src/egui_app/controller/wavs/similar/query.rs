use super::*;
use crate::egui_app::state::SimilarQuery;
use crate::egui_app::view_model;
use rusqlite::params;
use super::resolve::{
    ResolvedSimilarity,
    open_source_db_for_id,
    rerank_with_dsp,
    load_embedding_for_sample,
    load_light_dsp_for_sample,
    normalize_l2,
    cosine_similarity,
};
use std::collections::HashMap;

pub(super) fn build_similar_query_for_sample_id(
    controller: &mut EguiController,
    sample_id: &str,
    score_cutoff: Option<f32>,
    label_builder: impl FnOnce(&Path) -> String,
    anchor_override: Option<usize>,
    empty_error: &str,
) -> Result<SimilarQuery, String> {
    let resolved =
        resolve::resolve_similarity_for_sample_id(controller, sample_id, score_cutoff)?;
    if resolved.indices.is_empty() {
        return Err(empty_error.to_string());
    }
    Ok(build_similar_query_from_resolved(
        controller,
        resolved,
        label_builder,
        anchor_override,
    ))
}

pub(super) fn build_similarity_query_for_loaded_sample(
    controller: &mut EguiController,
) -> Result<SimilarQuery, String> {
    let loaded_audio = controller
        .sample_view
        .wav
        .loaded_audio
        .as_ref()
        .ok_or_else(|| "Load a sample to sort by similarity".to_string())?;
    let source_id = loaded_audio.source_id.clone();
    if controller.selection_state.ctx.selected_source.as_ref() != Some(&source_id) {
        return Err("Select the loaded sample's source to sort by similarity".to_string());
    }
    let loaded_path = loaded_audio.relative_path.clone();
    let sample_id =
        super::super::analysis_jobs::build_sample_id(source_id.as_str(), &loaded_path);
    let conn = open_source_db_for_id(controller, &source_id)?;
    let query_embedding = load_embedding_for_sample(&conn, &sample_id)?
        .ok_or_else(|| "Similarity data missing for the loaded sample".to_string())?;
    let query_dsp = load_light_dsp_for_sample(&conn, &sample_id)?;
    let total = controller.wav_entries_len();
    let mut indices = Vec::with_capacity(total);
    let mut scores = Vec::with_capacity(total);
    let mut has_embedding = vec![false; total];
    let mut path_lookup = HashMap::new();
    controller.for_each_wav_entry(|index, entry| {
        path_lookup.insert(entry.relative_path.clone(), index);
    })?;
    let mut stmt = conn
        .prepare(
            "SELECT embeddings.sample_id, embeddings.vec, features.vec_blob
             FROM embeddings
             LEFT JOIN features ON features.sample_id = embeddings.sample_id
             WHERE embeddings.model_id = ?1",
        )
        .map_err(|err| format!("Load similarity embeddings failed: {err}"))?;
    let mut rows = stmt
        .query(params![crate::analysis::similarity::SIMILARITY_MODEL_ID])
        .map_err(|err| format!("Load similarity embeddings failed: {err}"))?;
    while let Some(row) = rows.next().map_err(|err| format!("Load embeddings failed: {err}"))? {
        let candidate_id: String = row
            .get(0)
            .map_err(|err| format!("Load embeddings failed: {err}"))?;
        let blob: Vec<u8> = row
            .get(1)
            .map_err(|err| format!("Load embeddings failed: {err}"))?;
        let features_blob: Option<Vec<u8>> = row
            .get(2)
            .map_err(|err| format!("Load embeddings failed: {err}"))?;
        let (candidate_source, relative_path) =
            super::super::analysis_jobs::parse_sample_id(&candidate_id)?;
        if candidate_source.as_str() != source_id.as_str() {
            continue;
        }
        let Some(index) = path_lookup.get(&relative_path).copied() else {
            continue;
        };
        let candidate =
            crate::analysis::decode_f32_le_blob(&blob).map_err(|err| err.to_string())?;
        let embed_sim = cosine_similarity(&query_embedding, &candidate).clamp(-1.0, 1.0);
        let dsp_sim = query_dsp.as_deref().and_then(|query_dsp| {
            features_blob
                .as_ref()
                .and_then(|blob| crate::analysis::decode_f32_le_blob(blob).ok())
                .and_then(|features| crate::analysis::light_dsp_from_features_v1(&features))
                .map(normalize_l2)
                .map(|candidate| cosine_similarity(query_dsp, &candidate))
        });
        let score = if let Some(dsp_sim) = dsp_sim {
            EMBED_WEIGHT * embed_sim + DSP_WEIGHT * dsp_sim
        } else {
            embed_sim
        };
        indices.push(index);
        scores.push(score);
        if index < has_embedding.len() {
            has_embedding[index] = true;
        }
    }
    for (index, has) in has_embedding.iter().enumerate() {
        if !*has {
            indices.push(index);
            scores.push(MISSING_SIMILARITY_SCORE);
        }
    }
    if indices.is_empty() {
        return Err("No similarity data available for the current source".to_string());
    }
    let label = view_model::sample_display_label(&loaded_path);
    let anchor_index = controller.wav_index_for_path(&loaded_path);
    Ok(SimilarQuery {
        sample_id,
        label: format!("Loaded: {label}"),
        indices,
        scores,
        anchor_index,
    })
}

pub(super) fn build_similarity_query_for_audio_path(
    controller: &mut EguiController,
    path: &Path,
) -> Result<SimilarQuery, String> {
    let source_id = controller
        .selection_state
        .ctx
        .selected_source
        .clone()
        .ok_or_else(|| "No active source selected".to_string())?;
    let features = crate::analysis::compute_feature_vector_v1_for_path(path)?;
    let embedding = crate::analysis::similarity::embedding_from_features(&features)?;
    let query_dsp = crate::analysis::light_dsp_from_features_v1(&features).map(normalize_l2);
    let conn = open_source_db_for_id(controller, &source_id)?;
    let neighbours = crate::analysis::ann_index::find_similar_for_embedding(
        &conn,
        &embedding,
        SIMILAR_RE_RANK_CANDIDATES,
    )?;
    let ranked = rerank_with_dsp(&conn, neighbours, Some(&embedding), query_dsp.as_deref())?;

    let mut indices = Vec::new();
    let mut scores = Vec::new();
    for (candidate_id, score) in ranked {
        let (candidate_source, relative_path) =
            super::super::analysis_jobs::parse_sample_id(&candidate_id)?;
        if candidate_source.as_str() != source_id.as_str() {
            continue;
        }
        if let Some(index) = controller.wav_index_for_path(&relative_path) {
            indices.push(index);
            scores.push(score);
            if indices.len() >= DEFAULT_SIMILAR_COUNT {
                break;
            }
        }
    }
    if indices.is_empty() {
        return Err("No similar samples found in the current source".to_string());
    }
    let label = path
        .file_name()
        .and_then(|name| name.to_str())
        .map(|name| format!("Clip: {name}"))
        .unwrap_or_else(|| "Clip".to_string());
    Ok(SimilarQuery {
        sample_id: format!("clip::{}", path.display()),
        label,
        indices,
        scores,
        anchor_index: None,
    })
}

fn build_similar_query_from_resolved(
    controller: &mut EguiController,
    resolved: ResolvedSimilarity,
    label_builder: impl FnOnce(&Path) -> String,
    anchor_override: Option<usize>,
) -> SimilarQuery {
    SimilarQuery {
        sample_id: resolved.sample_id,
        label: label_builder(&resolved.relative_path),
        indices: resolved.indices,
        scores: resolved.scores,
        anchor_index: resolve_anchor_index(controller, &resolved.relative_path, anchor_override),
    }
}

fn resolve_anchor_index(
    controller: &mut EguiController,
    relative_path: &Path,
    anchor_override: Option<usize>,
) -> Option<usize> {
    anchor_override.or_else(|| controller.wav_index_for_path(relative_path))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::egui_app::controller::test_support::{prepare_with_source_and_wav_entries, sample_entry};

    #[test]
    fn resolve_anchor_index_prefers_override() {
        let (mut controller, _source) = prepare_with_source_and_wav_entries(vec![
            sample_entry("a.wav", SampleTag::Neutral),
        ]);
        let anchor = resolve_anchor_index(&mut controller, Path::new("a.wav"), Some(7));
        assert_eq!(anchor, Some(7));
    }
}
