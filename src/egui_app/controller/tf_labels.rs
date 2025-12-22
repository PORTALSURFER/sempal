use super::*;
use rusqlite::{Connection, OptionalExtension, params};
use std::collections::{HashMap, HashSet};
use uuid::Uuid;

/// Anchor-based label metadata stored in the library database.
#[derive(Debug, Clone, PartialEq)]
pub struct TfLabel {
    pub label_id: String,
    pub name: String,
    pub threshold: f32,
    pub threshold_mode: TfLabelThresholdMode,
    pub adaptive_threshold: Option<f32>,
    pub adaptive_percentile: Option<f32>,
    pub adaptive_mean: Option<f32>,
    pub adaptive_std: Option<f32>,
    pub gap: f32,
    pub topk: i64,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum TfLabelThresholdMode {
    Manual,
    Percentile,
    ZScore,
}

impl TfLabelThresholdMode {
    fn from_db(value: &str) -> Self {
        match value {
            "percentile" => TfLabelThresholdMode::Percentile,
            "zscore" => TfLabelThresholdMode::ZScore,
            _ => TfLabelThresholdMode::Manual,
        }
    }

    fn as_db(self) -> &'static str {
        match self {
            TfLabelThresholdMode::Manual => "manual",
            TfLabelThresholdMode::Percentile => "percentile",
            TfLabelThresholdMode::ZScore => "zscore",
        }
    }
}

/// Anchor assignment for a label and sample.
#[derive(Debug, Clone, PartialEq)]
pub struct TfAnchor {
    pub anchor_id: String,
    pub label_id: String,
    pub sample_id: String,
    pub weight: f32,
}

/// Score output for a label match against a sample embedding.
#[derive(Debug, Clone, PartialEq)]
pub struct TfLabelMatch {
    pub label_id: String,
    pub name: String,
    pub score: f32,
    pub bucket: crate::analysis::anchor_scoring::ConfidenceBucket,
    pub gap: f32,
    pub second_best: Option<f32>,
    pub anchor_count: usize,
}

#[derive(Debug, Clone, PartialEq)]
pub struct TfLabelCandidateMatch {
    pub sample_id: String,
    pub score: f32,
    pub bucket: crate::analysis::anchor_scoring::ConfidenceBucket,
}

#[derive(Debug, Clone, PartialEq)]
pub struct TfLabelCoverageStats {
    pub total: usize,
    pub high: usize,
    pub medium: usize,
    pub low: usize,
}

impl EguiController {
    /// List all training-free labels.
    pub fn list_tf_labels(&mut self) -> Result<Vec<TfLabel>, String> {
        let conn = open_library_db()?;
        list_tf_labels_with_conn(&conn)
    }

    /// Create a training-free label definition.
    pub fn create_tf_label(
        &mut self,
        name: &str,
        threshold: f32,
        gap: f32,
        topk: i64,
    ) -> Result<TfLabel, String> {
        validate_tf_label_fields(name, threshold, gap, topk)?;
        let mut conn = open_library_db()?;
        let name = name.trim();
        create_tf_label_with_conn(&mut conn, name, threshold, gap, topk)
    }

    /// Update a training-free label definition.
    pub fn update_tf_label(
        &mut self,
        label_id: &str,
        name: &str,
        threshold: f32,
        gap: f32,
        topk: i64,
    ) -> Result<(), String> {
        validate_tf_label_fields(name, threshold, gap, topk)?;
        let mut conn = open_library_db()?;
        let name = name.trim();
        update_tf_label_with_conn(&mut conn, label_id, name, threshold, gap, topk)
    }

    pub fn update_tf_label_adaptive_settings(
        &mut self,
        label_id: &str,
        mode: TfLabelThresholdMode,
        adaptive_threshold: Option<f32>,
        adaptive_percentile: Option<f32>,
        adaptive_mean: Option<f32>,
        adaptive_std: Option<f32>,
    ) -> Result<(), String> {
        let mut conn = open_library_db()?;
        update_tf_label_adaptive_with_conn(
            &mut conn,
            label_id,
            mode,
            adaptive_threshold,
            adaptive_percentile,
            adaptive_mean,
            adaptive_std,
        )
    }

    pub fn update_tf_label_adaptive_settings(
        &mut self,
        label_id: &str,
        mode: TfLabelThresholdMode,
        adaptive_threshold: Option<f32>,
        adaptive_percentile: Option<f32>,
        adaptive_mean: Option<f32>,
        adaptive_std: Option<f32>,
    ) -> Result<(), String> {
        let mut conn = open_library_db()?;
        update_tf_label_adaptive_with_conn(
            &mut conn,
            label_id,
            mode,
            adaptive_threshold,
            adaptive_percentile,
            adaptive_mean,
            adaptive_std,
        )
    }

    /// Remove a training-free label (anchors cascade).
    pub fn delete_tf_label(&mut self, label_id: &str) -> Result<(), String> {
        let mut conn = open_library_db()?;
        delete_tf_label_with_conn(&mut conn, label_id)
    }

    /// List anchors for a training-free label.
    pub fn list_tf_anchors(&mut self, label_id: &str) -> Result<Vec<TfAnchor>, String> {
        let conn = open_library_db()?;
        list_tf_anchors_with_conn(&conn, label_id)
    }

    /// Add (or update) an anchor for a label and sample.
    pub fn add_tf_anchor(
        &mut self,
        label_id: &str,
        sample_id: &str,
        weight: f32,
    ) -> Result<TfAnchor, String> {
        validate_tf_anchor_fields(weight)?;
        let mut conn = open_library_db()?;
        upsert_tf_anchor_with_conn(&mut conn, label_id, sample_id, weight)
    }

    /// Update an anchor weight.
    pub fn update_tf_anchor(&mut self, anchor_id: &str, weight: f32) -> Result<(), String> {
        validate_tf_anchor_fields(weight)?;
        let mut conn = open_library_db()?;
        update_tf_anchor_with_conn(&mut conn, anchor_id, weight)
    }

    /// Remove an anchor from a label.
    pub fn delete_tf_anchor(&mut self, anchor_id: &str) -> Result<(), String> {
        let mut conn = open_library_db()?;
        delete_tf_anchor_with_conn(&mut conn, anchor_id)
    }

    /// Score the training-free labels for a given sample_id.
    pub fn tf_label_matches_for_sample(
        &mut self,
        sample_id: &str,
        mode: crate::sample_sources::config::TfLabelAggregationMode,
    ) -> Result<Vec<TfLabelMatch>, String> {
        let conn = open_library_db()?;
        let embedding = load_tf_embedding(&conn, sample_id)?;
        let labels = list_tf_labels_with_conn(&conn)?;
        if labels.is_empty() {
            return Ok(Vec::new());
        }
        let (anchors, anchor_counts) = load_tf_anchor_embeddings(&conn)?;
        let label_specs: Vec<crate::analysis::anchor_match::LabelSpec> = labels
            .iter()
            .map(|label| crate::analysis::anchor_match::LabelSpec {
                label_id: label.label_id.clone(),
                name: label.name.clone(),
                threshold: effective_label_threshold(label),
                gap: label.gap,
                topk: label.topk.max(1) as usize,
            })
            .collect();
        let defaults = crate::analysis::embedding::tf_label_defaults();
        let scores = crate::analysis::anchor_match::score_labels_for_embedding(
            &label_specs,
            &anchors,
            &embedding,
            |label| match mode {
                crate::sample_sources::config::TfLabelAggregationMode::MeanTopK => {
                    crate::analysis::anchor_scoring::AnchorAggregation::MeanTopK(label.topk)
                }
                crate::sample_sources::config::TfLabelAggregationMode::Max => {
                    crate::analysis::anchor_scoring::AnchorAggregation::Max
                }
            },
            defaults.low_threshold_ratio,
        );
        let matches = scores
            .into_iter()
            .map(|score| TfLabelMatch {
                anchor_count: anchor_counts
                    .get(&score.label_id)
                    .copied()
                    .unwrap_or(0),
                label_id: score.label_id,
                name: score.name,
                score: score.score,
                bucket: score.bucket,
                gap: score.gap,
                second_best: score.second_best,
            })
            .collect();
        Ok(matches)
    }

    pub fn clear_tf_label_score_cache(&mut self) {
        self.ui.tf_labels.last_score_sample_id = None;
        self.ui.tf_labels.last_scores.clear();
        self.ui.tf_labels.last_candidate_label_id = None;
        self.ui.tf_labels.last_candidate_results.clear();
        self.ui.tf_labels.auto_tag_prompt = None;
        self.ui.tf_labels.coverage_stats.clear();
    }

    pub fn preview_sample_by_id(&mut self, sample_id: &str) -> Result<(), String> {
        let (source_id, relative_path) =
            crate::egui_app::controller::analysis_jobs::parse_sample_id(sample_id)?;
        let source_id = SourceId::from_string(source_id);
        self.select_source_internal(Some(source_id), Some(relative_path));
        Ok(())
    }

    pub fn set_tf_label_aggregation_mode(
        &mut self,
        mode: crate::sample_sources::config::TfLabelAggregationMode,
    ) {
        self.ui.tf_labels.aggregation_mode = mode;
        self.settings.analysis.tf_label_aggregation = mode;
        let _ = self.persist_config("Failed to save TF label settings");
        self.clear_tf_label_score_cache();
    }

    pub fn tf_label_candidate_matches_for_label(
        &mut self,
        label_id: &str,
        candidate_k: usize,
        top_k: usize,
    ) -> Result<Vec<TfLabelCandidateMatch>, String> {
        let conn = open_library_db()?;
        let label = load_tf_label_by_id(&conn, label_id)?
            .ok_or_else(|| "Label not found".to_string())?;
        let anchors = load_tf_anchor_embeddings_for_label(&conn, &label.label_id)?;
        if anchors.is_empty() {
            return Ok(Vec::new());
        }
        let label_spec = crate::analysis::anchor_match::LabelSpec {
            label_id: label.label_id.clone(),
            name: label.name.clone(),
            threshold: effective_label_threshold(&label),
            gap: label.gap,
            topk: label.topk.max(1) as usize,
        };
        let aggregation = match self.ui.tf_labels.aggregation_mode {
            crate::sample_sources::config::TfLabelAggregationMode::MeanTopK => {
                crate::analysis::anchor_scoring::AnchorAggregation::MeanTopK(label_spec.topk)
            }
            crate::sample_sources::config::TfLabelAggregationMode::Max => {
                crate::analysis::anchor_scoring::AnchorAggregation::Max
            }
        };
        let candidates = crate::analysis::label_match_ann::match_label_candidates_with_ann(
            &conn,
            &label_spec,
            &anchors,
            candidate_k,
            top_k,
            aggregation,
        )?;
        let defaults = crate::analysis::embedding::tf_label_defaults();
        let low =
            (label_spec.threshold * defaults.low_threshold_ratio).clamp(0.0, label_spec.threshold);
        let thresholds = crate::analysis::anchor_scoring::ConfidenceThresholds {
            high: label_spec.threshold,
            low,
            gap: label_spec.gap,
        };
        Ok(candidates
            .into_iter()
            .map(|entry| {
                let bucket = crate::analysis::anchor_scoring::classify_confidence(
                    entry.score,
                    None,
                    thresholds,
                );
                TfLabelCandidateMatch {
                    sample_id: entry.sample_id,
                    score: entry.score,
                    bucket,
                }
            })
            .collect())
    }

    pub fn tf_label_coverage_stats_for_label(
        &mut self,
        label_id: &str,
        candidate_k: usize,
        top_k: usize,
    ) -> Result<TfLabelCoverageStats, String> {
        let matches = self.tf_label_candidate_matches_for_label(label_id, candidate_k, top_k)?;
        let mut stats = TfLabelCoverageStats {
            total: matches.len(),
            high: 0,
            medium: 0,
            low: 0,
        };
        for entry in matches {
            match entry.bucket {
                crate::analysis::anchor_scoring::ConfidenceBucket::High => stats.high += 1,
                crate::analysis::anchor_scoring::ConfidenceBucket::Medium => stats.medium += 1,
                crate::analysis::anchor_scoring::ConfidenceBucket::Low => stats.low += 1,
            }
        }
        Ok(stats)
    }

    pub fn compute_tf_label_adaptive_percentile(
        &mut self,
        label_id: &str,
        percentile: f32,
        candidate_k: usize,
        top_k: usize,
    ) -> Result<f32, String> {
        if !(0.0..=1.0).contains(&percentile) {
            return Err("Percentile must be between 0.0 and 1.0".to_string());
        }
        let matches = self.tf_label_candidate_matches_for_label(label_id, candidate_k, top_k)?;
        let mut scores: Vec<f32> = matches.into_iter().map(|entry| entry.score).collect();
        if scores.is_empty() {
            return Err("No candidate scores available".to_string());
        }
        let threshold = percentile_from_scores(&mut scores, percentile);
        self.update_tf_label_adaptive_settings(
            label_id,
            TfLabelThresholdMode::Percentile,
            Some(threshold),
            Some(percentile),
            None,
            None,
        )?;
        Ok(threshold)
    }

    pub fn compute_tf_label_adaptive_zscore_stats(
        &mut self,
        label_id: &str,
        candidate_k: usize,
        top_k: usize,
    ) -> Result<(f32, f32), String> {
        let matches = self.tf_label_candidate_matches_for_label(label_id, candidate_k, top_k)?;
        let scores: Vec<f32> = matches.into_iter().map(|entry| entry.score).collect();
        if scores.is_empty() {
            return Err("No candidate scores available".to_string());
        }
        let (mean, std) = mean_std(&scores);
        self.update_tf_label_adaptive_settings(
            label_id,
            TfLabelThresholdMode::ZScore,
            None,
            None,
            Some(mean),
            Some(std),
        )?;
        Ok((mean, std))
    }
}

fn list_tf_labels_with_conn(conn: &Connection) -> Result<Vec<TfLabel>, String> {
    let mut stmt = conn
        .prepare(
            "SELECT label_id, name, threshold, threshold_mode, adaptive_threshold,
                    adaptive_percentile, adaptive_mean, adaptive_std, gap, topk
             FROM tf_labels
             ORDER BY name ASC",
        )
        .map_err(|err| format!("Prepare tf_labels query failed: {err}"))?;
    let rows = stmt
        .query_map([], row_to_tf_label)
        .map_err(|err| format!("Query tf_labels failed: {err}"))?;
    let mut labels = Vec::new();
    for row in rows {
        labels.push(row.map_err(|err| format!("Read tf_labels row failed: {err}"))?);
    }
    Ok(labels)
}

fn create_tf_label_with_conn(
    conn: &mut Connection,
    name: &str,
    threshold: f32,
    gap: f32,
    topk: i64,
) -> Result<TfLabel, String> {
    let label_id = Uuid::new_v4().to_string();
    let now = now_epoch_seconds();
    conn.execute(
        "INSERT INTO tf_labels (
            label_id,
            name,
            threshold,
            threshold_mode,
            adaptive_threshold,
            adaptive_percentile,
            adaptive_mean,
            adaptive_std,
            adaptive_updated_at,
            gap,
            topk,
            created_at,
            updated_at
         )
         VALUES (?1, ?2, ?3, ?4, NULL, NULL, NULL, NULL, NULL, ?5, ?6, ?7, ?7)",
        params![
            label_id,
            name,
            threshold,
            TfLabelThresholdMode::Manual.as_db(),
            gap,
            topk,
            now
        ],
    )
    .map_err(|err| format!("Insert tf_label failed: {err}"))?;
    Ok(TfLabel {
        label_id,
        name: name.to_string(),
        threshold,
        threshold_mode: TfLabelThresholdMode::Manual,
        adaptive_threshold: None,
        adaptive_percentile: None,
        adaptive_mean: None,
        adaptive_std: None,
        gap,
        topk,
    })
}

fn update_tf_label_with_conn(
    conn: &mut Connection,
    label_id: &str,
    name: &str,
    threshold: f32,
    gap: f32,
    topk: i64,
) -> Result<(), String> {
    let now = now_epoch_seconds();
    let updated = conn
        .execute(
            "UPDATE tf_labels
             SET name = ?2, threshold = ?3, gap = ?4, topk = ?5, updated_at = ?6
             WHERE label_id = ?1",
            params![label_id, name, threshold, gap, topk, now],
        )
        .map_err(|err| format!("Update tf_label failed: {err}"))?;
    if updated == 0 {
        return Err("No tf_label updated".to_string());
    }
    Ok(())
}

fn update_tf_label_adaptive_with_conn(
    conn: &mut Connection,
    label_id: &str,
    mode: TfLabelThresholdMode,
    adaptive_threshold: Option<f32>,
    adaptive_percentile: Option<f32>,
    adaptive_mean: Option<f32>,
    adaptive_std: Option<f32>,
) -> Result<(), String> {
    let now = now_epoch_seconds();
    let updated = conn
        .execute(
            "UPDATE tf_labels
             SET threshold_mode = ?2,
                 adaptive_threshold = ?3,
                 adaptive_percentile = ?4,
                 adaptive_mean = ?5,
                 adaptive_std = ?6,
                 adaptive_updated_at = ?7,
                 updated_at = ?7
             WHERE label_id = ?1",
            params![
                label_id,
                mode.as_db(),
                adaptive_threshold,
                adaptive_percentile,
                adaptive_mean,
                adaptive_std,
                now
            ],
        )
        .map_err(|err| format!("Update adaptive thresholds failed: {err}"))?;
    if updated == 0 {
        return Err("No tf_label updated".to_string());
    }
    Ok(())
}

fn delete_tf_label_with_conn(conn: &mut Connection, label_id: &str) -> Result<(), String> {
    let deleted = conn
        .execute("DELETE FROM tf_labels WHERE label_id = ?1", params![label_id])
        .map_err(|err| format!("Delete tf_label failed: {err}"))?;
    if deleted == 0 {
        return Err("No tf_label deleted".to_string());
    }
    Ok(())
}

fn list_tf_anchors_with_conn(conn: &Connection, label_id: &str) -> Result<Vec<TfAnchor>, String> {
    let mut stmt = conn
        .prepare(
            "SELECT anchor_id, label_id, sample_id, weight
             FROM tf_anchors
             WHERE label_id = ?1
             ORDER BY created_at ASC",
        )
        .map_err(|err| format!("Prepare tf_anchors query failed: {err}"))?;
    let rows = stmt
        .query_map([label_id], row_to_tf_anchor)
        .map_err(|err| format!("Query tf_anchors failed: {err}"))?;
    let mut anchors = Vec::new();
    for row in rows {
        anchors.push(row.map_err(|err| format!("Read tf_anchors row failed: {err}"))?);
    }
    Ok(anchors)
}

fn upsert_tf_anchor_with_conn(
    conn: &mut Connection,
    label_id: &str,
    sample_id: &str,
    weight: f32,
) -> Result<TfAnchor, String> {
    if let Some(anchor) = find_tf_anchor_by_label_sample(conn, label_id, sample_id)? {
        update_tf_anchor_with_conn(conn, &anchor.anchor_id, weight)?;
        return Ok(TfAnchor { weight, ..anchor });
    }
    let anchor_id = Uuid::new_v4().to_string();
    let now = now_epoch_seconds();
    conn.execute(
        "INSERT INTO tf_anchors (anchor_id, label_id, sample_id, weight, created_at, updated_at)
         VALUES (?1, ?2, ?3, ?4, ?5, ?5)",
        params![anchor_id, label_id, sample_id, weight, now],
    )
    .map_err(|err| format!("Insert tf_anchor failed: {err}"))?;
    Ok(TfAnchor {
        anchor_id,
        label_id: label_id.to_string(),
        sample_id: sample_id.to_string(),
        weight,
    })
}

fn update_tf_anchor_with_conn(
    conn: &mut Connection,
    anchor_id: &str,
    weight: f32,
) -> Result<(), String> {
    let now = now_epoch_seconds();
    let updated = conn
        .execute(
            "UPDATE tf_anchors
             SET weight = ?2, updated_at = ?3
             WHERE anchor_id = ?1",
            params![anchor_id, weight, now],
        )
        .map_err(|err| format!("Update tf_anchor failed: {err}"))?;
    if updated == 0 {
        return Err("No tf_anchor updated".to_string());
    }
    Ok(())
}

fn delete_tf_anchor_with_conn(conn: &mut Connection, anchor_id: &str) -> Result<(), String> {
    let deleted = conn
        .execute("DELETE FROM tf_anchors WHERE anchor_id = ?1", params![anchor_id])
        .map_err(|err| format!("Delete tf_anchor failed: {err}"))?;
    if deleted == 0 {
        return Err("No tf_anchor deleted".to_string());
    }
    Ok(())
}

fn find_tf_anchor_by_label_sample(
    conn: &Connection,
    label_id: &str,
    sample_id: &str,
) -> Result<Option<TfAnchor>, String> {
    conn.query_row(
        "SELECT anchor_id, label_id, sample_id, weight
         FROM tf_anchors
         WHERE label_id = ?1 AND sample_id = ?2",
        params![label_id, sample_id],
        row_to_tf_anchor,
    )
    .optional()
    .map_err(|err| format!("Query tf_anchor failed: {err}"))
}

fn row_to_tf_label(row: &rusqlite::Row<'_>) -> rusqlite::Result<TfLabel> {
    Ok(TfLabel {
        label_id: row.get(0)?,
        name: row.get(1)?,
        threshold: row.get(2)?,
        threshold_mode: TfLabelThresholdMode::from_db(&row.get::<_, String>(3)?),
        adaptive_threshold: row.get(4)?,
        adaptive_percentile: row.get(5)?,
        adaptive_mean: row.get(6)?,
        adaptive_std: row.get(7)?,
        gap: row.get(8)?,
        topk: row.get(9)?,
    })
}

fn row_to_tf_anchor(row: &rusqlite::Row<'_>) -> rusqlite::Result<TfAnchor> {
    Ok(TfAnchor {
        anchor_id: row.get(0)?,
        label_id: row.get(1)?,
        sample_id: row.get(2)?,
        weight: row.get(3)?,
    })
}

fn open_library_db() -> Result<Connection, String> {
    crate::sample_sources::library::open_connection()
        .map_err(|err| format!("Open library DB failed: {err}"))
}

fn now_epoch_seconds() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_else(|_| Duration::from_secs(0))
        .as_secs() as i64
}

fn load_tf_embedding(conn: &Connection, sample_id: &str) -> Result<Vec<f32>, String> {
    let blob: Vec<u8> = conn
        .query_row(
            "SELECT vec FROM embeddings WHERE sample_id = ?1 AND model_id = ?2",
            params![sample_id, crate::analysis::embedding::EMBEDDING_MODEL_ID],
            |row| row.get(0),
        )
        .map_err(|err| format!("Failed to load embedding for {sample_id}: {err}"))?;
    crate::analysis::decode_f32_le_blob(&blob)
}

fn load_tf_label_by_id(conn: &Connection, label_id: &str) -> Result<Option<TfLabel>, String> {
    conn.query_row(
        "SELECT label_id, name, threshold, threshold_mode, adaptive_threshold,
                adaptive_percentile, adaptive_mean, adaptive_std, gap, topk
         FROM tf_labels
         WHERE label_id = ?1",
        params![label_id],
        row_to_tf_label,
    )
    .optional()
    .map_err(|err| format!("Query tf_label failed: {err}"))
}

fn effective_label_threshold(label: &TfLabel) -> f32 {
    match label.threshold_mode {
        TfLabelThresholdMode::Manual => label.threshold,
        TfLabelThresholdMode::Percentile => label.adaptive_threshold.unwrap_or(label.threshold),
        TfLabelThresholdMode::ZScore => {
            if let (Some(mean), Some(std)) = (label.adaptive_mean, label.adaptive_std) {
                if std.is_finite() && std > 0.0 {
                    return (mean + label.threshold * std).clamp(0.0, 1.0);
                }
            }
            label.threshold
        }
    }
}

fn percentile_from_scores(scores: &mut [f32], percentile: f32) -> f32 {
    scores.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
    let len = scores.len().max(1);
    let idx = ((len - 1) as f32 * percentile.clamp(0.0, 1.0)).round() as usize;
    scores[idx.min(len - 1)].clamp(0.0, 1.0)
}

fn mean_std(scores: &[f32]) -> (f32, f32) {
    let len = scores.len().max(1) as f32;
    let mean = scores.iter().copied().sum::<f32>() / len;
    let mut var = 0.0_f32;
    for &score in scores {
        let diff = score - mean;
        var += diff * diff;
    }
    let std = (var / len).sqrt();
    (mean, std)
}

fn load_tf_anchor_embeddings(
    conn: &Connection,
) -> Result<
    (
        HashMap<String, Vec<crate::analysis::anchor_match::AnchorEmbedding>>,
        HashMap<String, usize>,
    ),
    String,
> {
    let mut stmt = conn
        .prepare("SELECT label_id, sample_id, weight FROM tf_anchors")
        .map_err(|err| format!("Prepare tf_anchors query failed: {err}"))?;
    let rows = stmt
        .query_map([], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, f32>(2)?,
            ))
        })
        .map_err(|err| format!("Query tf_anchors failed: {err}"))?;

    let mut anchors_by_label: HashMap<String, Vec<(String, f32)>> = HashMap::new();
    let mut anchor_counts: HashMap<String, usize> = HashMap::new();
    let mut sample_ids = HashSet::new();
    for row in rows {
        let (label_id, sample_id, weight) =
            row.map_err(|err| format!("Read tf_anchors row failed: {err}"))?;
        anchors_by_label
            .entry(label_id.clone())
            .or_default()
            .push((sample_id.clone(), weight));
        *anchor_counts.entry(label_id).or_insert(0) += 1;
        sample_ids.insert(sample_id);
    }

    if sample_ids.is_empty() {
        return Ok((HashMap::new(), anchor_counts));
    }

    let mut embedding_stmt = conn
        .prepare("SELECT vec FROM embeddings WHERE sample_id = ?1 AND model_id = ?2")
        .map_err(|err| format!("Prepare embedding lookup failed: {err}"))?;
    let mut embeddings: HashMap<String, Vec<f32>> = HashMap::new();
    for sample_id in sample_ids {
        let blob: Option<Vec<u8>> = embedding_stmt
            .query_row(
                params![sample_id, crate::analysis::embedding::EMBEDDING_MODEL_ID],
                |row| row.get(0),
            )
            .optional()
            .map_err(|err| format!("Load embedding failed: {err}"))?;
        if let Some(blob) = blob {
            if let Ok(vec) = crate::analysis::decode_f32_le_blob(&blob) {
                embeddings.insert(sample_id, vec);
            }
        }
    }

    let mut anchors: HashMap<String, Vec<crate::analysis::anchor_match::AnchorEmbedding>> =
        HashMap::new();
    for (label_id, items) in anchors_by_label {
        let mut vec = Vec::new();
        for (sample_id, weight) in items {
            if let Some(embedding) = embeddings.get(&sample_id) {
                vec.push(crate::analysis::anchor_match::AnchorEmbedding {
                    weight,
                    embedding: embedding.clone(),
                });
            }
        }
        if !vec.is_empty() {
            anchors.insert(label_id, vec);
        }
    }

    Ok((anchors, anchor_counts))
}

fn load_tf_anchor_embeddings_for_label(
    conn: &Connection,
    label_id: &str,
) -> Result<Vec<crate::analysis::anchor_match::AnchorEmbedding>, String> {
    let mut stmt = conn
        .prepare(
            "SELECT sample_id, weight FROM tf_anchors WHERE label_id = ?1 ORDER BY created_at ASC",
        )
        .map_err(|err| format!("Prepare tf_anchors lookup failed: {err}"))?;
    let rows = stmt
        .query_map([label_id], |row| Ok((row.get::<_, String>(0)?, row.get::<_, f32>(1)?)))
        .map_err(|err| format!("Query tf_anchors failed: {err}"))?;

    let mut embedding_stmt = conn
        .prepare("SELECT vec FROM embeddings WHERE sample_id = ?1 AND model_id = ?2")
        .map_err(|err| format!("Prepare embedding lookup failed: {err}"))?;
    let mut anchors = Vec::new();
    for row in rows {
        let (sample_id, weight) =
            row.map_err(|err| format!("Read tf_anchors row failed: {err}"))?;
        let blob: Option<Vec<u8>> = embedding_stmt
            .query_row(
                params![sample_id, crate::analysis::embedding::EMBEDDING_MODEL_ID],
                |row| row.get(0),
            )
            .optional()
            .map_err(|err| format!("Load embedding failed: {err}"))?;
        if let Some(blob) = blob {
            if let Ok(vec) = crate::analysis::decode_f32_le_blob(&blob) {
                anchors.push(crate::analysis::anchor_match::AnchorEmbedding { weight, embedding: vec });
            }
        }
    }
    Ok(anchors)
}

fn validate_tf_label_fields(
    name: &str,
    threshold: f32,
    gap: f32,
    topk: i64,
) -> Result<(), String> {
    if name.trim().is_empty() {
        return Err("Label name cannot be empty".to_string());
    }
    if !(0.0..=1.0).contains(&threshold) {
        return Err("Label threshold must be between 0.0 and 1.0".to_string());
    }
    if gap < 0.0 || gap > 2.0 {
        return Err("Label gap must be between 0.0 and 2.0".to_string());
    }
    if topk < 1 {
        return Err("Label topk must be at least 1".to_string());
    }
    Ok(())
}

fn validate_tf_anchor_fields(weight: f32) -> Result<(), String> {
    if weight <= 0.0 {
        return Err("Anchor weight must be greater than 0.0".to_string());
    }
    Ok(())
}
