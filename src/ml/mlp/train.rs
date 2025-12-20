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
            input_kind: MlpInputKind::FeaturesV1,
            seed: 42,
        }
    }
}

pub fn train_mlp(dataset: &TrainDataset, options: &TrainOptions) -> Result<MlpModel, String> {
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

    let class_weights = if options.balance_classes {
        let mut counts = vec![0f32; n_classes];
        for &y in &dataset.y {
            if y < n_classes {
                counts[y] += 1.0;
            }
        }
        let total: f32 = counts.iter().sum();
        counts
            .into_iter()
            .map(|count| {
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

    for _epoch in 0..options.epochs {
        indices.shuffle(&mut rng);
        for batch in indices.chunks(batch_size) {
            let mut d_w1 = vec![0.0f32; weights1.len()];
            let mut d_b1 = vec![0.0f32; bias1.len()];
            let mut d_w2 = vec![0.0f32; weights2.len()];
            let mut d_b2 = vec![0.0f32; bias2.len()];
            let mut batch_weight = 0.0f32;

            for &idx in batch {
                let x = &dataset.x[idx];
                let mut x_norm = vec![0.0f32; d];
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
                let mut d_hidden = vec![0.0f32; hidden];
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
