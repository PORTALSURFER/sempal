use serde::{Deserialize, Serialize};
use std::path::Path;

/// Single-node decision tree used as a weak learner.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Stump {
    /// Feature index used for the split.
    pub feature_index: u16,
    /// Threshold in feature units.
    pub threshold: f32,
    /// Prediction for `feature <= threshold`.
    pub left_value: f32,
    /// Prediction for `feature > threshold`.
    pub right_value: f32,
}

impl Stump {
    /// Predict the stump value for a feature vector.
    pub fn predict(&self, features: &[f32]) -> f32 {
        let idx = self.feature_index as usize;
        let value = features.get(idx).copied().unwrap_or(0.0);
        if value <= self.threshold {
            self.left_value
        } else {
            self.right_value
        }
    }
}

/// Gradient-boosted decision stump model for multi-class classification.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GbdtStumpModel {
    /// Model format version.
    pub model_version: i64,
    /// Feature vector version expected by this model.
    pub feat_version: i64,
    /// Number of `f32` values per feature vector.
    pub feature_len_f32: usize,
    /// Ordered list of class identifiers.
    pub classes: Vec<String>,
    /// Learning rate applied to each stump prediction.
    pub learning_rate: f32,
    /// Initial raw logits before boosting rounds.
    pub init_raw: Vec<f32>,
    /// Shape: `[n_rounds][n_classes]`.
    pub stumps: Vec<Vec<Stump>>,
}

impl GbdtStumpModel {
    /// Validate structural invariants of the model.
    pub fn validate(&self) -> Result<(), String> {
        if self.classes.len() < 2 {
            return Err("Model must contain at least 2 classes".to_string());
        }
        if self.init_raw.len() != self.classes.len() {
            return Err("init_raw length must match classes length".to_string());
        }
        for (round_idx, round) in self.stumps.iter().enumerate() {
            if round.len() != self.classes.len() {
                return Err(format!(
                    "Round {round_idx} has {} stumps but expected {}",
                    round.len(),
                    self.classes.len()
                ));
            }
        }
        Ok(())
    }

    /// Load a model from a JSON file.
    pub fn load_json(path: &Path) -> Result<Self, String> {
        let bytes = std::fs::read(path).map_err(|err| err.to_string())?;
        let model: Self = serde_json::from_slice(&bytes).map_err(|err| err.to_string())?;
        model.validate()?;
        Ok(model)
    }

    /// Predict raw logits for a feature vector.
    pub fn predict_raw(&self, features: &[f32]) -> Vec<f32> {
        let mut raw = self.init_raw.clone();
        for round in &self.stumps {
            for (class_idx, stump) in round.iter().enumerate() {
                raw[class_idx] += self.learning_rate * stump.predict(features);
            }
        }
        raw
    }

    /// Predict class probabilities for a feature vector.
    pub fn predict_proba(&self, features: &[f32]) -> Vec<f32> {
        softmax(&self.predict_raw(features))
    }

    /// Predict the best class index for a feature vector.
    pub fn predict_class_index(&self, features: &[f32]) -> usize {
        argmax(&self.predict_raw(features))
    }
}

/// Compute a numerically-stable softmax for a set of logits.
pub fn softmax(raw: &[f32]) -> Vec<f32> {
    if raw.is_empty() {
        return Vec::new();
    }
    let max = raw
        .iter()
        .copied()
        .fold(f32::NEG_INFINITY, |a, b| a.max(b));
    let mut exps = Vec::with_capacity(raw.len());
    let mut sum = 0.0f32;
    for &v in raw {
        let e = (v - max).exp();
        exps.push(e);
        sum += e;
    }
    if sum == 0.0 {
        return vec![1.0 / raw.len() as f32; raw.len()];
    }
    for v in &mut exps {
        *v /= sum;
    }
    exps
}

fn argmax(values: &[f32]) -> usize {
    let mut best_idx = 0usize;
    let mut best_val = f32::NEG_INFINITY;
    for (idx, &v) in values.iter().enumerate() {
        if v > best_val {
            best_val = v;
            best_idx = idx;
        }
    }
    best_idx
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn stump_predict_branches() {
        let stump = Stump {
            feature_index: 0,
            threshold: 0.5,
            left_value: -1.0,
            right_value: 2.0,
        };
        assert_eq!(stump.predict(&[0.0]), -1.0);
        assert_eq!(stump.predict(&[0.5]), -1.0);
        assert_eq!(stump.predict(&[0.6]), 2.0);
    }

    #[test]
    fn model_predicts_argmax() {
        let model = GbdtStumpModel {
            model_version: 1,
            feat_version: 1,
            feature_len_f32: 2,
            classes: vec!["a".into(), "b".into()],
            learning_rate: 1.0,
            init_raw: vec![0.0, 0.0],
            stumps: vec![vec![
                Stump {
                    feature_index: 0,
                    threshold: 0.0,
                    left_value: 1.0,
                    right_value: -1.0,
                },
                Stump {
                    feature_index: 0,
                    threshold: 0.0,
                    left_value: -1.0,
                    right_value: 1.0,
                },
            ]],
        };
        assert_eq!(model.predict_class_index(&[0.0, 0.0]), 0);
        assert_eq!(model.predict_class_index(&[1.0, 0.0]), 1);
    }
}

