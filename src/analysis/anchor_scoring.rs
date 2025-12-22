//! Scoring helpers for training-free anchor labels.

/// Aggregation strategy for anchor similarity scores.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AnchorAggregation {
    Max,
    MeanTopK(usize),
}

/// Similarity score paired with an anchor weight.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct AnchorSimilarity {
    pub similarity: f32,
    pub weight: f32,
}

/// Confidence buckets for label assignment.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ConfidenceBucket {
    High,
    Medium,
    Low,
}

/// Threshold configuration for confidence bucketing.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ConfidenceThresholds {
    pub high: f32,
    pub low: f32,
    pub gap: f32,
}

impl AnchorAggregation {
    pub fn score(&self, anchors: &[AnchorSimilarity]) -> Option<f32> {
        if anchors.is_empty() {
            return None;
        }
        let mut weighted: Vec<f32> = anchors
            .iter()
            .map(|anchor| (anchor.similarity * anchor.weight).max(0.0))
            .collect();
        if weighted.is_empty() {
            return None;
        }
        match *self {
            AnchorAggregation::Max => weighted.into_iter().reduce(f32::max),
            AnchorAggregation::MeanTopK(topk) => {
                let k = topk.max(1).min(weighted.len());
                weighted.sort_by(|a, b| b.partial_cmp(a).unwrap_or(std::cmp::Ordering::Equal));
                let sum: f32 = weighted.into_iter().take(k).sum();
                Some(sum / k as f32)
            }
        }
    }
}

/// Apply gap-aware thresholds to classify a label score.
pub fn classify_confidence(
    score: f32,
    second_best: Option<f32>,
    thresholds: ConfidenceThresholds,
) -> ConfidenceBucket {
    let gap = match second_best {
        Some(other) => score - other,
        None => f32::INFINITY,
    };
    if score >= thresholds.high && gap >= thresholds.gap {
        ConfidenceBucket::High
    } else if score >= thresholds.low {
        ConfidenceBucket::Medium
    } else {
        ConfidenceBucket::Low
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn aggregation_max_uses_weighted_similarity() {
        let anchors = vec![
            AnchorSimilarity {
                similarity: 0.4,
                weight: 1.0,
            },
            AnchorSimilarity {
                similarity: 0.6,
                weight: 0.5,
            },
        ];
        let score = AnchorAggregation::Max.score(&anchors).unwrap();
        assert!((score - 0.4).abs() < f32::EPSILON);
    }

    #[test]
    fn aggregation_mean_topk_averages_best_values() {
        let anchors = vec![
            AnchorSimilarity {
                similarity: 0.9,
                weight: 1.0,
            },
            AnchorSimilarity {
                similarity: 0.6,
                weight: 1.0,
            },
            AnchorSimilarity {
                similarity: 0.2,
                weight: 1.0,
            },
        ];
        let score = AnchorAggregation::MeanTopK(2).score(&anchors).unwrap();
        assert!((score - 0.75).abs() < 0.0001);
    }

    #[test]
    fn aggregation_handles_empty_inputs() {
        assert_eq!(AnchorAggregation::Max.score(&[]), None);
    }

    #[test]
    fn classify_confidence_applies_gap_logic() {
        let thresholds = ConfidenceThresholds {
            high: 0.8,
            low: 0.6,
            gap: 0.1,
        };
        assert_eq!(
            classify_confidence(0.85, Some(0.6), thresholds),
            ConfidenceBucket::High
        );
        assert_eq!(
            classify_confidence(0.85, Some(0.8), thresholds),
            ConfidenceBucket::Medium
        );
        assert_eq!(
            classify_confidence(0.65, Some(0.1), thresholds),
            ConfidenceBucket::Medium
        );
        assert_eq!(
            classify_confidence(0.4, Some(0.1), thresholds),
            ConfidenceBucket::Low
        );
    }

    #[test]
    fn classify_confidence_accepts_missing_second_best() {
        let thresholds = ConfidenceThresholds {
            high: 0.8,
            low: 0.6,
            gap: 0.2,
        };
        assert_eq!(
            classify_confidence(0.9, None, thresholds),
            ConfidenceBucket::High
        );
    }
}
