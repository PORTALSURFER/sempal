use super::*;
use rusqlite::OptionalExtension;
use crate::egui_app::view_model;

const DEFAULT_SIMILAR_COUNT: usize = 40;
const SIMILAR_RE_RANK_CANDIDATES: usize = 200;
const EMBED_WEIGHT: f32 = 0.8;
const DSP_WEIGHT: f32 = 0.2;

pub(super) fn find_similar_for_visible_row(
    controller: &mut EguiController,
    visible_row: usize,
) -> Result<(), String> {
    let source_id = controller
        .selection_state
        .ctx
        .selected_source
        .clone()
        .ok_or_else(|| "No active source selected".to_string())?;
    let entry_index = controller
        .ui
        .browser
        .visible
        .get(visible_row)
        .copied()
        .ok_or_else(|| "Selected row is out of range".to_string())?;
    let entry = controller
        .wav_entries
        .entries
        .get(entry_index)
        .ok_or_else(|| "Sample entry missing".to_string())?;
    let sample_id = super::super::analysis_jobs::build_sample_id(
        source_id.as_str(),
        &entry.relative_path,
    );
    let db_path = crate::app_dirs::app_root_dir()
        .map_err(|err| err.to_string())?
        .join(crate::sample_sources::library::LIBRARY_DB_FILE_NAME);
    let conn = super::super::analysis_jobs::open_library_db(&db_path)?;
    let neighbours = crate::analysis::ann_index::find_similar(
        &conn,
        &sample_id,
        SIMILAR_RE_RANK_CANDIDATES,
    )?;
    let query_embedding = load_embedding_for_sample(&conn, &sample_id)?;
    let query_dsp = load_light_dsp_for_sample(&conn, &sample_id)?;
    let ranked = rerank_with_dsp(&conn, neighbours, query_embedding.as_deref(), query_dsp.as_deref())?;

    let mut indices = Vec::new();
    for candidate_id in ranked {
        let (candidate_source, relative_path) =
            super::super::analysis_jobs::parse_sample_id(&candidate_id)?;
        if candidate_source.as_str() != source_id.as_str() {
            continue;
        }
        if let Some(index) = controller.wav_entries.lookup.get(&relative_path) {
            indices.push(*index);
            if indices.len() >= DEFAULT_SIMILAR_COUNT {
                break;
            }
        }
    }
    if indices.is_empty() {
        return Err("No similar samples found in the current source".to_string());
    }
    controller.ui.browser.similar_query = Some(crate::egui_app::state::SimilarQuery {
        sample_id,
        label: view_model::sample_display_label(&entry.relative_path),
        indices,
    });
    controller.ui.browser.search_query.clear();
    controller.ui.browser.search_focus_requested = false;
    controller.rebuild_browser_lists();
    Ok(())
}

pub(super) fn clear_similar_filter(controller: &mut EguiController) {
    if controller.ui.browser.similar_query.take().is_some() {
        controller.rebuild_browser_lists();
    }
}

pub(super) fn find_similar_for_audio_path(
    controller: &mut EguiController,
    path: &Path,
) -> Result<(), String> {
    let source_id = controller
        .selection_state
        .ctx
        .selected_source
        .clone()
        .ok_or_else(|| "No active source selected".to_string())?;
    let decoded = crate::analysis::audio::decode_for_analysis(path)?;
    let processed = crate::analysis::audio::preprocess_mono_for_embedding(
        &decoded.mono,
        decoded.sample_rate_used,
    );
    let embedding =
        crate::analysis::embedding::infer_embedding_query(&processed, decoded.sample_rate_used)?;
    let query_dsp = crate::analysis::compute_feature_vector_v1_for_path(path)
        .ok()
        .and_then(|features| crate::analysis::light_dsp_from_features_v1(&features))
        .map(normalize_l2);
    let db_path = crate::app_dirs::app_root_dir()
        .map_err(|err| err.to_string())?
        .join(crate::sample_sources::library::LIBRARY_DB_FILE_NAME);
    let conn = super::super::analysis_jobs::open_library_db(&db_path)?;
    let neighbours = crate::analysis::ann_index::find_similar_for_embedding(
        &conn,
        &embedding,
        SIMILAR_RE_RANK_CANDIDATES,
    )?;
    let ranked = rerank_with_dsp(&conn, neighbours, Some(&embedding), query_dsp.as_deref())?;

    let mut indices = Vec::new();
    for candidate_id in ranked {
        let (candidate_source, relative_path) =
            super::super::analysis_jobs::parse_sample_id(&candidate_id)?;
        if candidate_source.as_str() != source_id.as_str() {
            continue;
        }
        if let Some(index) = controller.wav_entries.lookup.get(&relative_path) {
            indices.push(*index);
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
    controller.ui.browser.similar_query = Some(crate::egui_app::state::SimilarQuery {
        sample_id: format!("clip::{}", path.display()),
        label,
        indices,
    });
    controller.ui.browser.search_query.clear();
    controller.ui.browser.search_focus_requested = false;
    controller.rebuild_browser_lists();
    Ok(())
}

fn rerank_with_dsp(
    conn: &rusqlite::Connection,
    neighbours: Vec<crate::analysis::ann_index::SimilarNeighbor>,
    query_embedding: Option<&[f32]>,
    query_dsp: Option<&[f32]>,
) -> Result<Vec<String>, String> {
    let mut scored = Vec::with_capacity(neighbours.len());
    for neighbour in neighbours {
        if neighbour.sample_id.is_empty() {
            continue;
        }
        let embed_sim = if let Some(query_embedding) = query_embedding {
            match load_embedding_for_sample(conn, &neighbour.sample_id)? {
                Some(candidate) => cosine_similarity(query_embedding, &candidate).clamp(-1.0, 1.0),
                None => (1.0 - neighbour.distance).clamp(-1.0, 1.0),
            }
        } else {
            (1.0 - neighbour.distance).clamp(-1.0, 1.0)
        };
        let dsp_sim = if let Some(query_dsp) = query_dsp {
            load_light_dsp_for_sample(conn, &neighbour.sample_id)?
                .as_deref()
                .map(|candidate| cosine_similarity(query_dsp, candidate))
        } else {
            None
        };
        let score = if let Some(dsp_sim) = dsp_sim {
            EMBED_WEIGHT * embed_sim + DSP_WEIGHT * dsp_sim
        } else {
            embed_sim
        };
        scored.push((neighbour.sample_id, score));
    }
    scored.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));
    Ok(scored.into_iter().map(|(id, _)| id).collect())
}

fn load_light_dsp_for_sample(
    conn: &rusqlite::Connection,
    sample_id: &str,
) -> Result<Option<Vec<f32>>, String> {
    let blob: Option<Vec<u8>> = conn
        .query_row(
            "SELECT vec_blob FROM features WHERE sample_id = ?1",
            [sample_id],
            |row| row.get(0),
        )
        .optional()
        .map_err(|err| format!("Load features failed: {err}"))?;
    let Some(blob) = blob else {
        return Ok(None);
    };
    let features = crate::analysis::decode_f32_le_blob(&blob)?;
    let light = crate::analysis::light_dsp_from_features_v1(&features);
    Ok(light.map(normalize_l2))
}

fn load_embedding_for_sample(
    conn: &rusqlite::Connection,
    sample_id: &str,
) -> Result<Option<Vec<f32>>, String> {
    let blob: Option<Vec<u8>> = conn
        .query_row(
            "SELECT vec FROM embeddings WHERE sample_id = ?1 AND model_id = ?2",
            params![sample_id, crate::analysis::embedding::EMBEDDING_MODEL_ID],
            |row| row.get(0),
        )
        .optional()
        .map_err(|err| format!("Load embedding failed: {err}"))?;
    let Some(blob) = blob else {
        return Ok(None);
    };
    crate::analysis::decode_f32_le_blob(&blob).map(Some)
}

fn normalize_l2(mut values: Vec<f32>) -> Vec<f32> {
    let mut sum = 0.0_f32;
    for value in &values {
        sum += value * value;
    }
    let norm = sum.sqrt();
    if norm.is_finite() && norm > 0.0 {
        for value in &mut values {
            *value /= norm;
        }
    }
    values
}

fn cosine_similarity(a: &[f32], b: &[f32]) -> f32 {
    let len = a.len().min(b.len());
    if len == 0 {
        return 0.0;
    }
    let mut sum = 0.0_f32;
    for i in 0..len {
        sum += a[i] * b[i];
    }
    sum
}
