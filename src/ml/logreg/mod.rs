//! Logistic regression classifier for embedding vectors.

use serde::{Deserialize, Serialize};

use crate::analysis::embedding::{EMBEDDING_DIM, EMBEDDING_MODEL_ID};
use crate::ml::gbdt_stump::softmax;

mod train;
pub use train::{TrainDataset, TrainOptions, train_logreg};

/// Default bundled classifier model id.
pub const DEFAULT_CLASSIFIER_MODEL_ID: &str = "yamnet_logreg_v1";

/// Default class list used by the bundled classifier.
pub const DEFAULT_CLASSIFIER_CLASSES: &[&str] = &[
    "kick",
    "snare",
    "clap",
    "hihat_open",
    "hihat_closed",
    "hihat",
    "tom",
    "rimshot",
    "shaker",
    "perc",
    "crash",
    "ride",
    "cymbal",
    "loop",
    "one_shot",
    "vocal",
    "bass",
    "fx",
];

/// Versioned logistic regression model for embedding vectors.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LogRegModel {
    #[serde(default)]
    pub model_id: Option<String>,
    pub model_version: i64,
    pub embedding_model_id: String,
    pub embedding_dim: usize,
    pub classes: Vec<String>,
    pub weights: Vec<f32>,
    pub bias: Vec<f32>,
    pub temperature: f32,
}

impl LogRegModel {
    /// Construct the bundled default classifier with zero-initialized weights.
    pub fn bundled() -> Self {
        let classes: Vec<String> = DEFAULT_CLASSIFIER_CLASSES
            .iter()
            .map(|class_id| (*class_id).to_string())
            .collect();
        let dim = EMBEDDING_DIM;
        let weights = vec![0.0; dim * classes.len()];
        let bias = vec![0.0; classes.len()];
        Self {
            model_id: None,
            model_version: 1,
            embedding_model_id: EMBEDDING_MODEL_ID.to_string(),
            embedding_dim: dim,
            classes,
            weights,
            bias,
            temperature: 1.0,
        }
    }

    /// Validate the model dimensions and embedding compatibility.
    pub fn validate(&self) -> Result<(), String> {
        if self.embedding_model_id != EMBEDDING_MODEL_ID {
            return Err(format!(
                "Unsupported embedding_model_id {} (expected {})",
                self.embedding_model_id, EMBEDDING_MODEL_ID
            ));
        }
        if self.embedding_dim != EMBEDDING_DIM {
            return Err(format!(
                "Unsupported embedding_dim {} (expected {})",
                self.embedding_dim, EMBEDDING_DIM
            ));
        }
        let classes = self.classes.len();
        if classes == 0 {
            return Err("No classes defined".to_string());
        }
        if self.weights.len() != classes * self.embedding_dim {
            return Err("weights length mismatch".to_string());
        }
        if self.bias.len() != classes {
            return Err("bias length mismatch".to_string());
        }
        if !self.temperature.is_finite() || self.temperature <= 0.0 {
            return Err("temperature must be > 0".to_string());
        }
        Ok(())
    }

    /// Compute class probabilities for a single embedding.
    pub fn predict_proba(&self, embedding: &[f32]) -> Vec<f32> {
        if embedding.len() != self.embedding_dim {
            return Vec::new();
        }
        let classes = self.classes.len();
        if classes == 0 {
            return Vec::new();
        }
        let mut logits = vec![0.0f32; classes];
        let temp = self.temperature.max(1e-6);
        for c in 0..classes {
            let mut sum = self.bias[c];
            let base = c * self.embedding_dim;
            for i in 0..self.embedding_dim {
                sum += self.weights[base + i] * embedding[i];
            }
            logits[c] = sum / temp;
        }
        softmax(&logits)
    }

    /// Return the argmax class index for the given embedding.
    pub fn predict_class_index(&self, embedding: &[f32]) -> usize {
        let proba = self.predict_proba(embedding);
        let mut best = 0usize;
        let mut best_val = f32::NEG_INFINITY;
        for (idx, &p) in proba.iter().enumerate() {
            if p > best_val {
                best_val = p;
                best = idx;
            }
        }
        best
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bundled_model_validates() {
        let model = LogRegModel::bundled();
        model.validate().unwrap();
        let out = model.predict_proba(&vec![0.0; EMBEDDING_DIM]);
        let sum: f32 = out.iter().sum();
        assert!((sum - 1.0).abs() < 1e-6);
    }
}
