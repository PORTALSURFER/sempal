use rand::{Rng, SeedableRng, seq::SliceRandom};
use rand::rngs::StdRng;

use super::{LogRegModel};
use crate::analysis::embedding::{EMBEDDING_DIM, EMBEDDING_MODEL_ID};
use crate::ml::gbdt_stump::softmax;

/// Training options for the embedding logistic regression head.
#[derive(Debug, Clone)]
pub struct TrainOptions {
    pub epochs: usize,
    pub learning_rate: f32,
    pub l2: f32,
    pub batch_size: usize,
    pub seed: u64,
    pub balance_classes: bool,
}

impl Default for TrainOptions {
    fn default() -> Self {
        Self {
            epochs: 20,
            learning_rate: 0.1,
            l2: 1e-4,
            batch_size: 128,
            seed: 42,
            balance_classes: false,
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

    let class_weights = if options.balance_classes {
        let mut counts = vec![0f32; classes];
        for &y in &dataset.y {
            if y < classes {
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
                    total / (classes as f32 * count)
                }
            })
            .collect()
    } else {
        vec![1.0; classes]
    };

    for _epoch in 0..options.epochs {
        indices.shuffle(&mut rng);
        for chunk in indices.chunks(batch_size) {
            let mut grad_w = vec![0.0f32; weights.len()];
            let mut grad_b = vec![0.0f32; bias.len()];
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
                let mut logits = vec![0.0f32; classes];
                for c in 0..classes {
                    let base = c * dim;
                    let mut sum = bias[c];
                    for i in 0..dim {
                        sum += weights[base + i] * x[i];
                    }
                    logits[c] = sum;
                }
                let probs = softmax(&logits);
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
    }

    let model = LogRegModel {
        model_version: 1,
        embedding_model_id: EMBEDDING_MODEL_ID.to_string(),
        embedding_dim: dim,
        classes: dataset.classes.clone(),
        weights,
        bias,
        temperature: 1.0,
    };
    model.validate()?;
    Ok(model)
}
