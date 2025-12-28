use super::*;
use crate::egui_app::view_model;
use rusqlite::{OptionalExtension, params};

const DEFAULT_SIMILAR_COUNT: usize = 40;
const SIMILAR_RE_RANK_CANDIDATES: usize = 200;
const EMBED_WEIGHT: f32 = 0.8;
const DSP_WEIGHT: f32 = 0.2;
const DUPLICATE_SCORE_THRESHOLD: f32 = 0.995;
const DUPLICATE_RMS_MIN: f32 = 1.0e-4;
const FEATURE_RMS_INDEX: usize = 2;

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
        .ok_or_else(|| "Selected row is out of range".to_string())?;
    let entry = controller
        .wav_entry(entry_index)
        .ok_or_else(|| "Sample entry missing".to_string())?;
    let entry_path = entry.relative_path.clone();
    let sample_id =
        super::super::analysis_jobs::build_sample_id(source_id.as_str(), &entry_path);
    let query = build_similar_query_for_sample_id(
        controller,
        &sample_id,
        None,
        |path| view_model::sample_display_label(path),
        Some(entry_index),
        "No similar samples found in the current source",
    )?;
    controller.ui.browser.similar_query = Some(query);
    controller.ui.browser.search_query.clear();
    controller.ui.browser.search_focus_requested = false;
    controller.rebuild_browser_lists();
    Ok(())
}

pub(super) fn find_duplicates_for_visible_row(
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
        .ok_or_else(|| "Selected row is out of range".to_string())?;
    let entry = controller
        .wav_entry(entry_index)
        .ok_or_else(|| "Sample entry missing".to_string())?;
    let entry_path = entry.relative_path.clone();
    let sample_id =
        super::super::analysis_jobs::build_sample_id(source_id.as_str(), &entry_path);
    let query = build_similar_query_for_sample_id(
        controller,
        &sample_id,
        Some(DUPLICATE_SCORE_THRESHOLD),
        |path| format!("Duplicates of {}", view_model::sample_display_label(path)),
        Some(entry_index),
        "No duplicates found in the current source",
    )?;
    controller.ui.browser.similar_query = Some(query);
    controller.ui.browser.search_query.clear();
    controller.ui.browser.search_focus_requested = false;
    controller.rebuild_browser_lists();
    Ok(())
}

pub(super) fn find_similar_for_sample_id(
    controller: &mut EguiController,
    sample_id: &str,
) -> Result<(), String> {
    let query = build_similar_query_for_sample_id(
        controller,
        sample_id,
        None,
        |path| view_model::sample_display_label(path),
        None,
        "No similar samples found in the current source",
    )?;
    controller.ui.browser.similar_query = Some(query);
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

fn open_source_db_for_id(
    controller: &EguiController,
    source_id: &SourceId,
) -> Result<rusqlite::Connection, String> {
    let source = controller
        .library
        .sources
        .iter()
        .find(|source| &source.id == source_id)
        .ok_or_else(|| "Source not found".to_string())?;
    super::super::analysis_jobs::open_source_db(&source.root)
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
    controller.ui.browser.similar_query = Some(crate::egui_app::state::SimilarQuery {
        sample_id: format!("clip::{}", path.display()),
        label,
        indices,
        scores,
        anchor_index: None,
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
) -> Result<Vec<(String, f32)>, String> {
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
    Ok(scored)
}

fn build_similar_query_for_sample_id(
    controller: &mut EguiController,
    sample_id: &str,
    score_cutoff: Option<f32>,
    label_builder: impl FnOnce(&Path) -> String,
    anchor_override: Option<usize>,
    empty_error: &str,
) -> Result<crate::egui_app::state::SimilarQuery, String> {
    let (source_id, relative_path) = super::super::analysis_jobs::parse_sample_id(sample_id)?;
    let source_id = SourceId::from_string(source_id);
    if controller.selection_state.ctx.selected_source.as_ref() != Some(&source_id) {
        controller.select_source(Some(source_id.clone()));
    }
    let mut conn = open_source_db_for_id(controller, &source_id)?;
    if let Err(err) = maybe_enqueue_full_analysis(controller, &mut conn, sample_id) {
        tracing::debug!("Fast prep refine enqueue failed: {err}");
    }
    if score_cutoff.is_some() {
        if let Some(rms) = load_rms_for_sample(&conn, sample_id)? {
            if is_effectively_silent(rms) {
                return Err("Selected sample is effectively silent".to_string());
            }
        }
    }
    let neighbours =
        crate::analysis::ann_index::find_similar(&conn, sample_id, SIMILAR_RE_RANK_CANDIDATES)?;
    let query_embedding = load_embedding_for_sample(&conn, sample_id)?;
    let query_dsp = load_light_dsp_for_sample(&conn, sample_id)?;
    let ranked = rerank_with_dsp(
        &conn,
        neighbours,
        query_embedding.as_deref(),
        query_dsp.as_deref(),
    )?;
    let (indices, scores) = filter_ranked_candidates(
        &conn,
        ranked,
        &source_id,
        score_cutoff,
        |path| controller.wav_index_for_path(path),
    )?;
    if indices.is_empty() {
        return Err(empty_error.to_string());
    }
    Ok(crate::egui_app::state::SimilarQuery {
        sample_id: sample_id.to_string(),
        label: label_builder(&relative_path),
        indices,
        scores,
        anchor_index: anchor_override.or_else(|| controller.wav_index_for_path(&relative_path)),
    })
}

fn filter_ranked_candidates(
    conn: &rusqlite::Connection,
    ranked: impl IntoIterator<Item = (String, f32)>,
    source_id: &SourceId,
    score_cutoff: Option<f32>,
    mut resolve_index: impl FnMut(&Path) -> Option<usize>,
) -> Result<(Vec<usize>, Vec<f32>), String> {
    let mut indices = Vec::new();
    let mut scores = Vec::new();
    let apply_duplicate_filters = score_cutoff.is_some();
    for (candidate_id, score) in ranked {
        if let Some(cutoff) = score_cutoff {
            if score < cutoff {
                break;
            }
        }
        let (candidate_source, relative_path) =
            super::super::analysis_jobs::parse_sample_id(&candidate_id)?;
        if candidate_source.as_str() != source_id.as_str() {
            continue;
        }
        if apply_duplicate_filters {
            if let Some(rms) = load_rms_for_sample(conn, &candidate_id)? {
                if is_effectively_silent(rms) {
                    continue;
                }
            }
        }
        if let Some(index) = resolve_index(&relative_path) {
            indices.push(index);
            scores.push(score);
            if indices.len() >= DEFAULT_SIMILAR_COUNT {
                break;
            }
        }
    }
    Ok((indices, scores))
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

fn load_rms_for_sample(
    conn: &rusqlite::Connection,
    sample_id: &str,
) -> Result<Option<f32>, String> {
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
    if features.len() <= FEATURE_RMS_INDEX {
        return Ok(None);
    }
    Ok(Some(features[FEATURE_RMS_INDEX]))
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

fn is_effectively_silent(rms: f32) -> bool {
    !rms.is_finite() || rms <= DUPLICATE_RMS_MIN
}

fn maybe_enqueue_full_analysis(
    controller: &EguiController,
    conn: &mut rusqlite::Connection,
    sample_id: &str,
) -> Result<(), String> {
    if !controller.similarity_prep_fast_mode_enabled() {
        return Ok(());
    }
    let row: Option<(String, Option<String>)> = conn
        .query_row(
            "SELECT content_hash, analysis_version FROM samples WHERE sample_id = ?1",
            params![sample_id],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .optional()
        .map_err(|err| format!("Load analysis version failed: {err}"))?;
    let Some((content_hash, analysis_version)) = row else {
        return Ok(());
    };
    if content_hash.trim().is_empty() {
        return Ok(());
    }
    let fast_version = crate::analysis::version::analysis_version_for_sample_rate(
        controller.similarity_prep_fast_sample_rate(),
    );
    if analysis_version.as_deref() != Some(fast_version.as_str()) {
        return Ok(());
    }
    let active: i64 = conn
        .query_row(
            "SELECT COUNT(*)
             FROM analysis_jobs
             WHERE sample_id = ?1 AND job_type = ?2 AND status IN ('pending','running')",
            params![sample_id, "wav_metadata_v1"],
            |row| row.get(0),
        )
        .unwrap_or(0);
    if active > 0 {
        return Ok(());
    }
    let created_at = now_epoch_seconds();
    conn.execute(
        "INSERT INTO analysis_jobs (sample_id, job_type, content_hash, status, attempts, created_at)
         VALUES (?1, ?2, ?3, 'pending', 0, ?4)
         ON CONFLICT(sample_id, job_type) DO UPDATE SET
            content_hash = excluded.content_hash,
            status = 'pending',
            attempts = 0,
            created_at = excluded.created_at,
            last_error = NULL",
        params![sample_id, "wav_metadata_v1", content_hash, created_at],
    )
    .map_err(|err| format!("Enqueue analysis job failed: {err}"))?;
    Ok(())
}

fn now_epoch_seconds() -> i64 {
    use std::time::{SystemTime, UNIX_EPOCH};
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs() as i64
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::analysis::vector::encode_f32_le_blob;
    use rusqlite::{Connection, params};
    use std::collections::HashMap;
    use std::path::PathBuf;

    fn in_memory_conn() -> Connection {
        let conn = Connection::open_in_memory().unwrap();
        conn.execute_batch(
            "CREATE TABLE features (
                sample_id TEXT PRIMARY KEY,
                feat_version INTEGER NOT NULL,
                vec_blob BLOB NOT NULL,
                computed_at INTEGER NOT NULL
             ) WITHOUT ROWID;",
        )
        .unwrap();
        conn
    }

    fn insert_rms(conn: &Connection, sample_id: &str, rms: f32) {
        let mut values = vec![0.0_f32; FEATURE_RMS_INDEX + 1];
        values[FEATURE_RMS_INDEX] = rms;
        let blob = encode_f32_le_blob(&values);
        conn.execute(
            "INSERT INTO features (sample_id, feat_version, vec_blob, computed_at)
             VALUES (?1, 1, ?2, 0)",
            params![sample_id, blob],
        )
        .unwrap();
    }

    #[test]
    fn duplicate_filter_respects_score_cutoff() {
        let conn = in_memory_conn();
        let source_id = SourceId::from_string("source-a");
        let sample_id = super::super::analysis_jobs::build_sample_id(
            source_id.as_str(),
            Path::new("a.wav"),
        );
        let lower_id = super::super::analysis_jobs::build_sample_id(
            source_id.as_str(),
            Path::new("b.wav"),
        );
        let ranked = vec![
            (sample_id.clone(), DUPLICATE_SCORE_THRESHOLD + 0.002),
            (lower_id.clone(), DUPLICATE_SCORE_THRESHOLD - 0.001),
        ];
        let mut lookup = HashMap::new();
        lookup.insert(PathBuf::from("a.wav"), 0);
        lookup.insert(PathBuf::from("b.wav"), 1);
        let (indices, scores) = filter_ranked_candidates(
            &conn,
            ranked,
            &source_id,
            Some(DUPLICATE_SCORE_THRESHOLD),
            |path| lookup.get(path).copied(),
        )
        .unwrap();
        assert_eq!(indices, vec![0]);
        assert_eq!(scores.len(), 1);
    }

    #[test]
    fn duplicate_filter_skips_silent_rms_candidates() {
        let conn = in_memory_conn();
        let source_id = SourceId::from_string("source-a");
        let silent_id = super::super::analysis_jobs::build_sample_id(
            source_id.as_str(),
            Path::new("silent.wav"),
        );
        let loud_id = super::super::analysis_jobs::build_sample_id(
            source_id.as_str(),
            Path::new("loud.wav"),
        );
        insert_rms(&conn, &silent_id, DUPLICATE_RMS_MIN * 0.5);
        insert_rms(&conn, &loud_id, DUPLICATE_RMS_MIN * 10.0);
        let ranked = vec![
            (silent_id.clone(), DUPLICATE_SCORE_THRESHOLD + 0.01),
            (loud_id.clone(), DUPLICATE_SCORE_THRESHOLD + 0.01),
        ];
        let mut lookup = HashMap::new();
        lookup.insert(PathBuf::from("silent.wav"), 0);
        lookup.insert(PathBuf::from("loud.wav"), 1);
        let (indices, scores) = filter_ranked_candidates(
            &conn,
            ranked,
            &source_id,
            Some(DUPLICATE_SCORE_THRESHOLD),
            |path| lookup.get(path).copied(),
        )
        .unwrap();
        assert_eq!(indices, vec![1]);
        assert_eq!(scores.len(), 1);
    }

    #[test]
    fn duplicate_filter_skips_cross_source_candidates() {
        let conn = in_memory_conn();
        let source_id = SourceId::from_string("source-a");
        let other_source = SourceId::from_string("source-b");
        let own_id = super::super::analysis_jobs::build_sample_id(
            source_id.as_str(),
            Path::new("keep.wav"),
        );
        let other_id = super::super::analysis_jobs::build_sample_id(
            other_source.as_str(),
            Path::new("skip.wav"),
        );
        insert_rms(&conn, &own_id, DUPLICATE_RMS_MIN * 10.0);
        insert_rms(&conn, &other_id, DUPLICATE_RMS_MIN * 10.0);
        let ranked = vec![
            (other_id.clone(), DUPLICATE_SCORE_THRESHOLD + 0.01),
            (own_id.clone(), DUPLICATE_SCORE_THRESHOLD + 0.01),
        ];
        let mut lookup = HashMap::new();
        lookup.insert(PathBuf::from("keep.wav"), 0);
        lookup.insert(PathBuf::from("skip.wav"), 1);
        let (indices, scores) = filter_ranked_candidates(
            &conn,
            ranked,
            &source_id,
            Some(DUPLICATE_SCORE_THRESHOLD),
            |path| lookup.get(path).copied(),
        )
        .unwrap();
        assert_eq!(indices, vec![0]);
        assert_eq!(scores.len(), 1);
    }
}
