//! Evaluation metrics for classification models.

use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

#[derive(Debug, Clone)]
/// Confusion matrix for a `K`-class classifier.
pub struct ConfusionMatrix {
    /// Number of classes.
    pub n_classes: usize,
    /// Row-major `KxK` counts (`truth * K + predicted`).
    pub counts: Vec<u32>,
}

impl ConfusionMatrix {
    /// Create an empty `KxK` confusion matrix.
    pub fn new(n_classes: usize) -> Self {
        Self {
            n_classes,
            counts: vec![0; n_classes * n_classes],
        }
    }

    pub fn add(&mut self, truth: usize, predicted: usize) {
        if truth >= self.n_classes || predicted >= self.n_classes {
            return;
        }
        let idx = truth * self.n_classes + predicted;
        self.counts[idx] = self.counts[idx].saturating_add(1);
    }

    pub fn get(&self, truth: usize, predicted: usize) -> u32 {
        self.counts[truth * self.n_classes + predicted]
    }
}

#[derive(Debug, Clone)]
/// Precision/recall statistics for a single class.
pub struct PerClassStats {
    /// `TP / (TP + FP)`.
    pub precision: f32,
    /// `TP / (TP + FN)`.
    pub recall: f32,
    /// Total number of true examples for the class.
    pub support: u32,
}

/// Serialized metrics snapshot for UI dashboards.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ModelMetrics {
    pub per_class: Vec<PerClassMetric>,
    pub hist_bins: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PerClassMetric {
    pub class_id: String,
    pub support: u32,
    pub precision: f32,
    pub recall: f32,
    pub f1: f32,
    pub confidence_hist: Vec<u32>,
}

/// Compute per-class precision and recall from a confusion matrix.
pub fn precision_recall_by_class(cm: &ConfusionMatrix) -> Vec<PerClassStats> {
    let k = cm.n_classes;
    let mut stats = Vec::with_capacity(k);
    for class_idx in 0..k {
        let tp = cm.get(class_idx, class_idx) as f32;
        let mut fp = 0f32;
        let mut fn_ = 0f32;
        let mut support = 0u32;
        for j in 0..k {
            let v = cm.get(class_idx, j);
            support = support.saturating_add(v);
            if j != class_idx {
                fn_ += v as f32;
            }
        }
        for i in 0..k {
            if i != class_idx {
                fp += cm.get(i, class_idx) as f32;
            }
        }
        let precision = if tp + fp == 0.0 { 0.0 } else { tp / (tp + fp) };
        let recall = if tp + fn_ == 0.0 { 0.0 } else { tp / (tp + fn_) };
        stats.push(PerClassStats {
            precision,
            recall,
            support,
        });
    }
    stats
}

/// Compute overall accuracy from a confusion matrix.
pub fn accuracy(cm: &ConfusionMatrix) -> f32 {
    let mut correct = 0u64;
    let mut total = 0u64;
    for truth in 0..cm.n_classes {
        for predicted in 0..cm.n_classes {
            let v = cm.get(truth, predicted) as u64;
            total += v;
            if truth == predicted {
                correct += v;
            }
        }
    }
    if total == 0 {
        0.0
    } else {
        (correct as f32) / (total as f32)
    }
}

/// Convenience mapping of class indices to names.
pub fn class_name_map(classes: &[String]) -> BTreeMap<usize, String> {
    classes
        .iter()
        .cloned()
        .enumerate()
        .map(|(i, name)| (i, name))
        .collect()
}
