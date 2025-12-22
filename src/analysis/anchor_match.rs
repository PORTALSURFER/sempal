//! Label scoring for anchor-based training-free labels.

use std::collections::HashMap;

use crate::analysis::anchor_scoring::{
    classify_confidence, AnchorAggregation, AnchorSimilarity, ConfidenceBucket, ConfidenceThresholds,
};

#[derive(Debug, Clone, PartialEq)]
pub struct LabelSpec {
    pub label_id: String,
    pub name: String,
    pub threshold: f32,
    pub gap: f32,
    pub topk: usize,
}

#[derive(Debug, Clone, PartialEq)]
pub struct AnchorEmbedding {
    pub weight: f32,
    pub embedding: Vec<f32>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct LabelScore {
    pub label_id: String,
    pub name: String,
    pub score: f32,
    pub bucket: ConfidenceBucket,
    pub gap: f32,
    pub second_best: Option<f32>,
}

pub fn score_labels_for_embedding<F>(
    labels: &[LabelSpec],
    anchors: &HashMap<String, Vec<AnchorEmbedding>>,
    embedding: &[f32],
    aggregation_for_label: F,
    low_threshold_ratio: f32,
) -> Vec<LabelScore>
where
    F: Fn(&LabelSpec) -> AnchorAggregation,
{
    let mut scores: Vec<(usize, f32)> = Vec::new();
    for (idx, label) in labels.iter().enumerate() {
        let Some(anchor_list) = anchors.get(&label.label_id) else {
            continue;
        };
        if anchor_list.is_empty() {
            continue;
        }
        let mut similarities = Vec::with_capacity(anchor_list.len());
        for anchor in anchor_list {
            let similarity = dot_product(embedding, &anchor.embedding);
            similarities.push(AnchorSimilarity {
                similarity,
                weight: anchor.weight,
            });
        }
        let aggregator = aggregation_for_label(label);
        if let Some(score) = aggregator.score(&similarities) {
            scores.push((idx, score));
        }
    }

    let (top1, top2) = top_two_scores(&scores);
    let mut results = Vec::new();
    for (idx, score) in scores {
        let label = &labels[idx];
        let second_best = if Some(idx) == top1.map(|(i, _)| i) {
            top2.map(|(_, s)| s)
        } else {
            top1.map(|(_, s)| s)
        };
        let gap = match second_best {
            Some(other) => score - other,
            None => f32::INFINITY,
        };
        let low = (label.threshold * low_threshold_ratio).clamp(0.0, label.threshold);
        let thresholds = ConfidenceThresholds {
            high: label.threshold,
            low,
            gap: label.gap,
        };
        let bucket = classify_confidence(score, second_best, thresholds);
        results.push(LabelScore {
            label_id: label.label_id.clone(),
            name: label.name.clone(),
            score,
            bucket,
            gap,
            second_best,
        });
    }

    results.sort_by(|a, b| b.score.partial_cmp(&a.score).unwrap_or(std::cmp::Ordering::Equal));
    results
}

fn top_two_scores(scores: &[(usize, f32)]) -> (Option<(usize, f32)>, Option<(usize, f32)>) {
    let mut top1: Option<(usize, f32)> = None;
    let mut top2: Option<(usize, f32)> = None;
    for &(idx, score) in scores {
        match top1 {
            Some((_, best)) if score <= best => {
                if top2.map(|(_, second)| score > second).unwrap_or(true) {
                    top2 = Some((idx, score));
                }
            }
            _ => {
                top2 = top1;
                top1 = Some((idx, score));
            }
        }
    }
    (top1, top2)
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
    use crate::analysis::anchor_scoring::ConfidenceBucket;

    #[test]
    fn score_labels_for_embedding_orders_by_score_and_gap() {
        let labels = vec![
            LabelSpec {
                label_id: "a".to_string(),
                name: "A".to_string(),
                threshold: 0.7,
                gap: 0.1,
                topk: 2,
            },
            LabelSpec {
                label_id: "b".to_string(),
                name: "B".to_string(),
                threshold: 0.7,
                gap: 0.1,
                topk: 1,
            },
        ];
        let mut anchors = HashMap::new();
        anchors.insert(
            "a".to_string(),
            vec![AnchorEmbedding {
                weight: 1.0,
                embedding: vec![1.0, 0.0],
            }],
        );
        anchors.insert(
            "b".to_string(),
            vec![AnchorEmbedding {
                weight: 1.0,
                embedding: vec![0.0, 1.0],
            }],
        );
        let embedding = vec![0.9, 0.1];
        let results = score_labels_for_embedding(
            &labels,
            &anchors,
            &embedding,
            |label| {
                if label.topk == 1 {
                    AnchorAggregation::Max
                } else {
                    AnchorAggregation::MeanTopK(label.topk)
                }
            },
            0.85,
        );
        assert_eq!(results.len(), 2);
        assert_eq!(results[0].label_id, "a");
        assert_eq!(results[0].bucket, ConfidenceBucket::High);
        assert_eq!(results[1].label_id, "b");
    }
}
