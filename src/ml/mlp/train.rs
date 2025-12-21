use rand::{Rng, SeedableRng, seq::SliceRandom};
use rand::rngs::StdRng;

use crate::ml::gbdt_stump::TrainDataset;
use super::{MlpInputKind, MlpModel};

#[derive(Debug, Clone)]
pub struct TrainOptions {
    pub hidden_size: usize,
    pub epochs: usize,
    pub batch_size: usize,
    pub learning_rate: f32,
    pub l2_penalty: f32,
    pub dropout: f32,
    pub label_smoothing: f32,
    pub balance_classes: bool,
    pub input_kind: MlpInputKind,
    pub seed: u64,
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
            hidden_size: 128,
            epochs: 20,
            batch_size: 128,
            learning_rate: 0.01,
            l2_penalty: 1e-4,
            dropout: 0.15,
            label_smoothing: 0.05,
            balance_classes: true,
            input_kind: MlpInputKind::EmbeddingV1,
            seed: 42,
            early_stop_target: Some(0.98),
            early_stop_patience: 8,
            early_stop_min_delta: 0.001,
            eval_every: 1,
        }
    }
}

pub fn train_mlp(
    dataset: &TrainDataset,
    options: &TrainOptions,
    validation: Option<&TrainDataset>,
) -> Result<MlpModel, String> {
    if dataset.x.len() != dataset.y.len() {
        return Err("Mismatched X/Y lengths".to_string());
    }
    if dataset.x.is_empty() {
        return Err("Empty dataset".to_string());
    }
    let n_classes = dataset.classes.len();
    if n_classes < 2 {
        return Err("Need at least 2 classes".to_string());
    }
    let n = dataset.x.len();
    let d = dataset.feature_len_f32;
    let hidden = options.hidden_size.max(1);
    let batch_size = options.batch_size.max(1);
    let dropout = options.dropout.clamp(0.0, 0.9);
    let label_smoothing = options.label_smoothing.clamp(0.0, 0.2);

    let (mean, std) = feature_mean_std(&dataset.x, d);
    let mut rng = StdRng::seed_from_u64(options.seed);

    let mut weights1 = vec![0.0f32; hidden * d];
    let mut bias1 = vec![0.0f32; hidden];
    let mut weights2 = vec![0.0f32; n_classes * hidden];
    let mut bias2 = vec![0.0f32; n_classes];

    for w in &mut weights1 {
        *w = (rng.random::<f32>() - 0.5) * 0.1;
    }
    for w in &mut weights2 {
        *w = (rng.random::<f32>() - 0.5) * 0.1;
    }

    let mut indices: Vec<usize> = (0..n).collect();
    let mut hidden_act = vec![0.0f32; hidden];
    let mut hidden_pre = vec![0.0f32; hidden];
    let mut logits = vec![0.0f32; n_classes];
    let mut probs = vec![0.0f32; n_classes];

    let mut class_counts = vec![0f32; n_classes];
    for &y in &dataset.y {
        if y < n_classes {
            class_counts[y] += 1.0;
        }
    }
    let total: f32 = class_counts.iter().sum();
    // Initialize output bias to log-priors for faster convergence on imbalanced data.
    for (idx, &count) in class_counts.iter().enumerate() {
        let prior = if total > 0.0 {
            count / total
        } else {
            1.0 / n_classes as f32
        };
        bias2[idx] = prior.max(1e-6).ln();
    }

    let class_weights = if options.balance_classes {
        class_counts
            .iter()
            .map(|&count| {
                if count == 0.0 {
                    0.0
                } else {
                    total / (n_classes as f32 * count)
                }
            })
            .collect()
    } else {
        vec![1.0; n_classes]
    };

    let mut d_w1 = vec![0.0f32; weights1.len()];
    let mut d_b1 = vec![0.0f32; bias1.len()];
    let mut d_w2 = vec![0.0f32; weights2.len()];
    let mut d_b2 = vec![0.0f32; bias2.len()];
    let mut d_hidden = vec![0.0f32; hidden];
    let mut x_norm = vec![0.0f32; d];
    let eval_every = options.eval_every.max(1);
    let mut best_acc = f32::NEG_INFINITY;
    let mut best_weights1 = None;
    let mut best_bias1 = None;
    let mut best_weights2 = None;
    let mut best_bias2 = None;
    let mut epochs_since_improve = 0usize;
    let early_stop_enabled = validation.is_some()
        && (options.early_stop_target.is_some() || options.early_stop_patience > 0);
    for epoch in 0..options.epochs {
        indices.shuffle(&mut rng);
        for batch in indices.chunks(batch_size) {
            d_w1.fill(0.0);
            d_b1.fill(0.0);
            d_w2.fill(0.0);
            d_b2.fill(0.0);
            let mut batch_weight = 0.0f32;

            for &idx in batch {
                let x = &dataset.x[idx];
                for i in 0..d {
                    let denom = std[i].max(1e-6);
                    x_norm[i] = (x[i] - mean[i]) / denom;
                }

                for h in 0..hidden {
                    let mut sum = bias1[h];
                    let base = h * d;
                    for i in 0..d {
                        sum += weights1[base + i] * x_norm[i];
                    }
                    hidden_pre[h] = sum;
                    let mut act = sum.max(0.0);
                    if dropout > 0.0 {
                        let keep = rng.random::<f32>() > dropout;
                        if keep {
                            act /= 1.0 - dropout;
                        } else {
                            act = 0.0;
                        }
                    }
                    hidden_act[h] = act;
                }

                for c in 0..n_classes {
                    let mut sum = bias2[c];
                    let base = c * hidden;
                    for h in 0..hidden {
                        sum += weights2[base + h] * hidden_act[h];
                    }
                    logits[c] = sum;
                }
                softmax_inplace(&logits, &mut probs);

                let y = dataset.y[idx];
                if y >= n_classes {
                    continue;
                }
                let weight = class_weights[y];
                if weight == 0.0 {
                    continue;
                }
                d_hidden.fill(0.0);
                for c in 0..n_classes {
                    let target = if label_smoothing > 0.0 {
                        if c == y {
                            1.0 - label_smoothing
                        } else {
                            label_smoothing / (n_classes as f32 - 1.0)
                        }
                    } else if c == y {
                        1.0
                    } else {
                        0.0
                    };
                    let dz2 = probs[c] - target;
                    d_b2[c] += dz2 * weight;
                    let base = c * hidden;
                    for h in 0..hidden {
                        d_w2[base + h] += dz2 * hidden_act[h] * weight;
                        d_hidden[h] += dz2 * weights2[base + h] * weight;
                    }
                }
                for h in 0..hidden {
                    if hidden_pre[h] <= 0.0 {
                        d_hidden[h] = 0.0;
                    }
                    d_b1[h] += d_hidden[h];
                    let base = h * d;
                    for i in 0..d {
                        d_w1[base + i] += d_hidden[h] * x_norm[i];
                    }
                }
                batch_weight += weight;
            }

            if batch_weight == 0.0 {
                continue;
            }
            let scale = options.learning_rate / batch_weight;
            let l2 = options.l2_penalty;
            for i in 0..weights1.len() {
                weights1[i] -= scale * (d_w1[i] + l2 * weights1[i]);
            }
            for i in 0..bias1.len() {
                bias1[i] -= scale * d_b1[i];
            }
            for i in 0..weights2.len() {
                weights2[i] -= scale * (d_w2[i] + l2 * weights2[i]);
            }
            for i in 0..bias2.len() {
                bias2[i] -= scale * d_b2[i];
            }
        }

        if early_stop_enabled && (epoch + 1) % eval_every == 0 {
            let val = validation.expect("validation present");
            let acc = mlp_accuracy(
                &weights1,
                &bias1,
                &weights2,
                &bias2,
                &mean,
                &std,
                d,
                hidden,
                val,
            );
            if acc > best_acc + options.early_stop_min_delta {
                best_acc = acc;
                best_weights1 = Some(weights1.clone());
                best_bias1 = Some(bias1.clone());
                best_weights2 = Some(weights2.clone());
                best_bias2 = Some(bias2.clone());
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

    if let (Some(w1), Some(b1), Some(w2), Some(b2)) =
        (best_weights1, best_bias1, best_weights2, best_bias2)
    {
        weights1 = w1;
        bias1 = b1;
        weights2 = w2;
        bias2 = b2;
    }

    Ok(MlpModel {
        model_version: 1,
        input_kind: options.input_kind,
        feat_version: dataset.feat_version,
        feature_len_f32: dataset.feature_len_f32,
        classes: dataset.classes.clone(),
        hidden_size: hidden,
        weights1,
        bias1,
        weights2,
        bias2,
        feature_mean: mean,
        feature_std: std,
        class_thresholds: None,
        top2_margin: None,
        metrics: None,
    })
}

fn feature_mean_std(rows: &[Vec<f32>], d: usize) -> (Vec<f32>, Vec<f32>) {
    let mut mean = vec![0.0f32; d];
    for row in rows {
        for i in 0..d {
            mean[i] += row[i];
        }
    }
    let n = rows.len().max(1) as f32;
    for v in &mut mean {
        *v /= n;
    }

    let mut var = vec![0.0f32; d];
    for row in rows {
        for i in 0..d {
            let diff = row[i] - mean[i];
            var[i] += diff * diff;
        }
    }
    for v in &mut var {
        *v = (*v / n).sqrt();
    }
    (mean, var)
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

fn mlp_accuracy(
    weights1: &[f32],
    bias1: &[f32],
    weights2: &[f32],
    bias2: &[f32],
    mean: &[f32],
    std: &[f32],
    input: usize,
    hidden: usize,
    dataset: &TrainDataset,
) -> f32 {
    let classes = bias2.len().max(1);
    let mut correct = 0usize;
    let mut total = 0usize;
    let mut normalized = vec![0.0f32; input];
    let mut hidden_act = vec![0.0f32; hidden];
    for (x, &y) in dataset.x.iter().zip(dataset.y.iter()) {
        if x.len() != input || y >= classes {
            continue;
        }
        for i in 0..input {
            let denom = std[i].max(1e-6);
            normalized[i] = (x[i] - mean[i]) / denom;
        }
        for h in 0..hidden {
            let mut sum = bias1[h];
            let base = h * input;
            for i in 0..input {
                sum += weights1[base + i] * normalized[i];
            }
            hidden_act[h] = sum.max(0.0);
        }
        let mut best_idx = 0usize;
        let mut best_val = f32::NEG_INFINITY;
        for c in 0..classes {
            let mut sum = bias2[c];
            let base = c * hidden;
            for h in 0..hidden {
                sum += weights2[base + h] * hidden_act[h];
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
