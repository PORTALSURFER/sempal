use rand::{Rng, SeedableRng, seq::SliceRandom};
use rand::rngs::StdRng;

use super::{LogRegModel};
use crate::analysis::embedding::{EMBEDDING_DIM, EMBEDDING_MODEL_ID};

/// Training options for the embedding logistic regression head.
#[derive(Debug, Clone)]
pub struct TrainOptions {
    pub epochs: usize,
    pub learning_rate: f32,
    pub l2: f32,
    pub batch_size: usize,
    pub seed: u64,
    pub balance_classes: bool,
    /// Target validation accuracy to stop training early.
    pub early_stop_target: Option<f32>,
    /// Number of evaluations without improvement before stopping.
    pub early_stop_patience: usize,
    /// Minimum accuracy delta that counts as an improvement.
    pub early_stop_min_delta: f32,
    /// Evaluate validation accuracy every N epochs.
    pub eval_every: usize,
}

impl Default for TrainOptions {
    fn default() -> Self {
        Self {
            epochs: 20,
            learning_rate: 0.1,
            l2: 1e-4,
            batch_size: 128,
            seed: 42,
            balance_classes: true,
            early_stop_target: Some(0.98),
            early_stop_patience: 8,
            early_stop_min_delta: 0.001,
            eval_every: 1,
        }
    }
}

/// In-memory training dataset for logreg models.
#[derive(Debug, Clone)]
pub struct TrainDataset {
    pub classes: Vec<String>,
    pub x: Vec<Vec<f32>>,
    pub y: Vec<usize>,
}

pub fn train_logreg(
    dataset: &TrainDataset,
    options: &TrainOptions,
    validation: Option<&TrainDataset>,
) -> Result<LogRegModel, String> {
    if dataset.x.is_empty() || dataset.y.is_empty() {
        return Err("Empty training set".to_string());
    }
    if dataset.x.len() != dataset.y.len() {
        return Err("Mismatched training inputs/labels".to_string());
    }
    let classes = dataset.classes.len();
    if classes == 0 {
        return Err("No classes available for training".to_string());
    }
    let dim = dataset.x[0].len();
    if dim != EMBEDDING_DIM {
        return Err(format!(
            "Unexpected embedding dimension {} (expected {})",
            dim, EMBEDDING_DIM
        ));
    }
    for row in &dataset.x {
        if row.len() != dim {
            return Err("Inconsistent embedding row length".to_string());
        }
    }

    let mut rng = StdRng::seed_from_u64(options.seed);
    let mut weights = vec![0.0f32; classes * dim];
    let mut bias = vec![0.0f32; classes];
    for w in &mut weights {
        *w = (rng.random::<f32>() - 0.5) * 0.01;
    }

    let mut indices: Vec<usize> = (0..dataset.x.len()).collect();
    let batch_size = options.batch_size.max(1);
    let lr = options.learning_rate;
    let l2 = options.l2.max(0.0);

    let mut class_counts = vec![0f32; classes];
    for &y in &dataset.y {
        if y < classes {
            class_counts[y] += 1.0;
        }
    }
    let total: f32 = class_counts.iter().sum();
    // Initialize biases to log-priors for faster convergence on imbalanced data.
    for (idx, &count) in class_counts.iter().enumerate() {
        let prior = if total > 0.0 {
            count / total
        } else {
            1.0 / classes as f32
        };
        bias[idx] = prior.max(1e-6).ln();
    }

    let class_weights = if options.balance_classes {
        class_counts
            .iter()
            .map(|&count| {
                if count == 0.0 {
                    0.0
                } else {
                    total / (classes as f32 * count)
                }
            })
            .collect()
    } else {
        vec![1.0; classes]
    };

    let mut grad_w = vec![0.0f32; weights.len()];
    let mut grad_b = vec![0.0f32; bias.len()];
    let mut logits = vec![0.0f32; classes];
    let mut probs = vec![0.0f32; classes];
    let eval_every = options.eval_every.max(1);
    let mut best_acc = f32::NEG_INFINITY;
    let mut best_weights = None;
    let mut best_bias = None;
    let mut epochs_since_improve = 0usize;
    let early_stop_enabled = validation.is_some()
        && (options.early_stop_target.is_some() || options.early_stop_patience > 0);
    for epoch in 0..options.epochs {
        indices.shuffle(&mut rng);
        for chunk in indices.chunks(batch_size) {
            grad_w.fill(0.0);
            grad_b.fill(0.0);
            let mut batch_weight = 0.0f32;
            for &idx in chunk {
                let x = &dataset.x[idx];
                let y = dataset.y[idx];
                if y >= classes {
                    continue;
                }
                let weight = class_weights[y];
                if weight == 0.0 {
                    continue;
                }
                for c in 0..classes {
                    let base = c * dim;
                    let mut sum = bias[c];
                    for i in 0..dim {
                        sum += weights[base + i] * x[i];
                    }
                    logits[c] = sum;
                }
                softmax_inplace(&logits, &mut probs);
                for c in 0..classes {
                    let diff = probs[c] - if c == y { 1.0 } else { 0.0 };
                    let base = c * dim;
                    for i in 0..dim {
                        grad_w[base + i] += diff * x[i] * weight;
                    }
                    grad_b[c] += diff * weight;
                }
                batch_weight += weight;
            }
            if batch_weight == 0.0 {
                continue;
            }
            let inv = 1.0 / batch_weight;
            for c in 0..classes {
                let base = c * dim;
                for i in 0..dim {
                    let idx = base + i;
                    let l2_term = l2 * weights[idx];
                    weights[idx] -= lr * (grad_w[idx] * inv + l2_term);
                }
                bias[c] -= lr * grad_b[c] * inv;
            }
        }

        if early_stop_enabled && (epoch + 1) % eval_every == 0 {
            let val = validation.expect("validation present");
            let acc = logreg_accuracy(&weights, &bias, dim, val);
            if acc > best_acc + options.early_stop_min_delta {
                best_acc = acc;
                best_weights = Some(weights.clone());
                best_bias = Some(bias.clone());
                epochs_since_improve = 0;
            } else {
                epochs_since_improve = epochs_since_improve.saturating_add(1);
            }

            if let Some(target) = options.early_stop_target {
                if acc >= target.clamp(0.0, 1.0) {
                    break;
                }
            }
            if options.early_stop_patience > 0 && epochs_since_improve >= options.early_stop_patience {
                break;
            }
        }
    }

    if let (Some(best_weights), Some(best_bias)) = (best_weights, best_bias) {
        weights = best_weights;
        bias = best_bias;
    }

    let model = LogRegModel {
        model_id: None,
        model_version: 1,
        embedding_model_id: EMBEDDING_MODEL_ID.to_string(),
        embedding_dim: dim,
        classes: dataset.classes.clone(),
        weights,
        bias,
        temperature: 1.0,
        class_thresholds: None,
        top2_margin: None,
        metrics: None,
    };
    model.validate()?;
    Ok(model)
}

fn softmax_inplace(raw: &[f32], out: &mut [f32]) {
    if raw.is_empty() || out.is_empty() {
        return;
    }
    let max = raw
        .iter()
        .copied()
        .fold(f32::NEG_INFINITY, |a, b| a.max(b));
    let mut sum = 0.0f32;
    for (i, &v) in raw.iter().enumerate() {
        let e = (v - max).exp();
        out[i] = e;
        sum += e;
    }
    if sum == 0.0 {
        let uniform = 1.0 / (raw.len() as f32);
        for v in out.iter_mut() {
            *v = uniform;
        }
        return;
    }
    for v in out.iter_mut() {
        *v /= sum;
    }
}

fn logreg_accuracy(
    weights: &[f32],
    bias: &[f32],
    dim: usize,
    dataset: &TrainDataset,
) -> f32 {
    let classes = bias.len().max(1);
    let mut correct = 0usize;
    let mut total = 0usize;
    for (x, &y) in dataset.x.iter().zip(dataset.y.iter()) {
        if x.len() != dim || y >= classes {
            continue;
        }
        let mut best_idx = 0usize;
        let mut best_val = f32::NEG_INFINITY;
        for c in 0..classes {
            let base = c * dim;
            let mut sum = bias[c];
            for i in 0..dim {
                sum += weights[base + i] * x[i];
            }
            if sum > best_val {
                best_val = sum;
                best_idx = c;
            }
        }
        if best_idx == y {
            correct += 1;
        }
        total += 1;
    }
    if total == 0 {
        0.0
    } else {
        correct as f32 / total as f32
    }
}
