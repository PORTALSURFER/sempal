//! ANN-backed candidate generation for training-free label matching.

use std::collections::{HashMap, HashSet};

use crate::analysis::anchor_match::{AnchorEmbedding, LabelSpec};
use crate::analysis::anchor_scoring::{AnchorAggregation, AnchorSimilarity};
use crate::analysis::ann_index;
use crate::analysis::decode_f32_le_blob;
use rusqlite::{Connection, OptionalExtension, params};

#[derive(Debug, Clone, PartialEq)]
pub struct LabelCandidateScore {
    pub sample_id: String,
    pub score: f32,
}

/// Fetch candidate samples via ANN per anchor, score them, and return the top matches.
pub fn match_label_candidates_with_ann(
    conn: &Connection,
    label: &LabelSpec,
    anchors: &[AnchorEmbedding],
    candidate_k: usize,
    top_k: usize,
) -> Result<Vec<LabelCandidateScore>, String> {
    if anchors.is_empty() || candidate_k == 0 || top_k == 0 {
        return Ok(Vec::new());
    }
    let candidate_ids = collect_ann_candidates(conn, anchors, candidate_k)?;
    if candidate_ids.is_empty() {
        return Ok(Vec::new());
    }
    let embeddings = load_embeddings_for_candidates(conn, &candidate_ids)?;
    let scores = score_label_candidates(label, anchors, &embeddings);
    Ok(scores.into_iter().take(top_k).collect())
}

fn collect_ann_candidates(
    conn: &Connection,
    anchors: &[AnchorEmbedding],
    candidate_k: usize,
) -> Result<Vec<String>, String> {
    let mut candidates = Vec::new();
    let mut seen = HashSet::new();
    for anchor in anchors {
        let neighbours = ann_index::find_similar_for_embedding(conn, &anchor.embedding, candidate_k)?;
        for neighbour in neighbours {
            if seen.insert(neighbour.sample_id.clone()) {
                candidates.push(neighbour.sample_id);
            }
        }
    }
    Ok(candidates)
}

fn load_embeddings_for_candidates(
    conn: &Connection,
    candidate_ids: &[String],
) -> Result<HashMap<String, Vec<f32>>, String> {
    let mut embeddings = HashMap::new();
    let mut stmt = conn
        .prepare("SELECT vec FROM embeddings WHERE sample_id = ?1 AND model_id = ?2")
        .map_err(|err| format!("Prepare embedding lookup failed: {err}"))?;
    for sample_id in candidate_ids {
        let blob: Option<Vec<u8>> = stmt
            .query_row(
                params![sample_id, crate::analysis::embedding::EMBEDDING_MODEL_ID],
                |row| row.get(0),
            )
            .optional()
            .map_err(|err| format!("Load embedding failed: {err}"))?;
        if let Some(blob) = blob {
            if let Ok(vec) = decode_f32_le_blob(&blob) {
                embeddings.insert(sample_id.clone(), vec);
            }
        }
    }
    Ok(embeddings)
}

fn score_label_candidates(
    label: &LabelSpec,
    anchors: &[AnchorEmbedding],
    candidates: &HashMap<String, Vec<f32>>,
) -> Vec<LabelCandidateScore> {
    let mut results = Vec::new();
    let aggregation = AnchorAggregation::MeanTopK(label.topk.max(1));
    for (sample_id, embedding) in candidates {
        let mut similarities = Vec::with_capacity(anchors.len());
        for anchor in anchors {
            let similarity = dot_product(embedding, &anchor.embedding);
            similarities.push(AnchorSimilarity {
                similarity,
                weight: anchor.weight,
            });
        }
        if let Some(score) = aggregation.score(&similarities) {
            results.push(LabelCandidateScore {
                sample_id: sample_id.clone(),
                score,
            });
        }
    }
    results.sort_by(|a, b| b.score.partial_cmp(&a.score).unwrap_or(std::cmp::Ordering::Equal));
    results
}

fn dot_product(a: &[f32], b: &[f32]) -> f32 {
    let len = a.len().min(b.len());
    let mut sum = 0.0_f32;
    for i in 0..len {
        sum += a[i] * b[i];
    }
    sum
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn score_label_candidates_orders_by_score() {
        let label = LabelSpec {
            label_id: "l".to_string(),
            name: "Label".to_string(),
            threshold: 0.7,
            gap: 0.1,
            topk: 2,
        };
        let anchors = vec![
            AnchorEmbedding {
                weight: 1.0,
                embedding: vec![1.0, 0.0],
            },
            AnchorEmbedding {
                weight: 1.0,
                embedding: vec![0.0, 1.0],
            },
        ];
        let mut candidates = HashMap::new();
        candidates.insert("a".to_string(), vec![0.9, 0.1]);
        candidates.insert("b".to_string(), vec![0.2, 0.8]);
        let scores = score_label_candidates(&label, &anchors, &candidates);
        assert_eq!(scores.len(), 2);
        assert_eq!(scores[0].sample_id, "b");
        assert!(scores[0].score > scores[1].score);
    }
}
