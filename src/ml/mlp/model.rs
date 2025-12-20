use serde::{Deserialize, Serialize};

use crate::analysis::embedding::EMBEDDING_DIM;
use crate::analysis::{FEATURE_VECTOR_LEN_V1, FEATURE_VERSION_V1, LIGHT_DSP_VECTOR_LEN};
use crate::ml::metrics::ModelMetrics;
use crate::ml::gbdt_stump::softmax;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MlpModel {
    pub model_version: i64,
    #[serde(default = "default_input_kind")]
    pub input_kind: MlpInputKind,
    pub feat_version: i64,
    pub feature_len_f32: usize,
    pub classes: Vec<String>,
    pub hidden_size: usize,
    pub weights1: Vec<f32>,
    pub bias1: Vec<f32>,
    pub weights2: Vec<f32>,
    pub bias2: Vec<f32>,
    pub feature_mean: Vec<f32>,
    pub feature_std: Vec<f32>,
    #[serde(default)]
    pub class_thresholds: Option<Vec<f32>>,
    #[serde(default)]
    pub top2_margin: Option<f32>,
    #[serde(default)]
    pub metrics: Option<ModelMetrics>,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum MlpInputKind {
    FeaturesV1,
    EmbeddingV1,
    HybridV1,
}

fn default_input_kind() -> MlpInputKind {
    MlpInputKind::FeaturesV1
}

impl MlpModel {
    pub fn validate(&self) -> Result<(), String> {
        match self.input_kind {
            MlpInputKind::FeaturesV1 => {
                if self.feat_version != FEATURE_VERSION_V1 {
                    return Err(format!(
                        "Unsupported feat_version {} (expected {})",
                        self.feat_version, FEATURE_VERSION_V1
                    ));
                }
                if self.feature_len_f32 != FEATURE_VECTOR_LEN_V1 {
                    return Err(format!(
                        "Unsupported feature_len_f32 {} (expected {})",
                        self.feature_len_f32, FEATURE_VECTOR_LEN_V1
                    ));
                }
            }
            MlpInputKind::EmbeddingV1 => {
                if self.feature_len_f32 != EMBEDDING_DIM {
                    return Err(format!(
                        "Unsupported embedding_len {} (expected {})",
                        self.feature_len_f32, EMBEDDING_DIM
                    ));
                }
            }
            MlpInputKind::HybridV1 => {
                let expected = EMBEDDING_DIM + LIGHT_DSP_VECTOR_LEN;
                if self.feature_len_f32 != expected {
                    return Err(format!(
                        "Unsupported hybrid_len {} (expected {})",
                        self.feature_len_f32, expected
                    ));
                }
            }
        }
        let input = self.feature_len_f32;
        let hidden = self.hidden_size;
        let classes = self.classes.len();
        if self.weights1.len() != input * hidden {
            return Err("weights1 length mismatch".to_string());
        }
        if self.bias1.len() != hidden {
            return Err("bias1 length mismatch".to_string());
        }
        if self.weights2.len() != classes * hidden {
            return Err("weights2 length mismatch".to_string());
        }
        if self.bias2.len() != classes {
            return Err("bias2 length mismatch".to_string());
        }
        if self.feature_mean.len() != input {
            return Err("feature_mean length mismatch".to_string());
        }
        if self.feature_std.len() != input {
            return Err("feature_std length mismatch".to_string());
        }
        if let Some(thresholds) = &self.class_thresholds {
            if thresholds.len() != classes {
                return Err("class_thresholds length mismatch".to_string());
            }
        }
        Ok(())
    }

    pub fn predict_proba(&self, features: &[f32]) -> Vec<f32> {
        if features.len() != self.feature_len_f32 {
            return Vec::new();
        }
        let input = self.feature_len_f32;
        let hidden = self.hidden_size;
        let classes = self.classes.len();
        if classes == 0 || hidden == 0 {
            return Vec::new();
        }

        let mut normalized = vec![0.0f32; input];
        for i in 0..input {
            let std = self.feature_std[i].max(1e-6);
            normalized[i] = (features[i] - self.feature_mean[i]) / std;
        }

        let mut hidden_act = vec![0.0f32; hidden];
        for h in 0..hidden {
            let mut sum = self.bias1[h];
            let base = h * input;
            for i in 0..input {
                sum += self.weights1[base + i] * normalized[i];
            }
            hidden_act[h] = sum.max(0.0);
        }

        let mut logits = vec![0.0f32; classes];
        for c in 0..classes {
            let mut sum = self.bias2[c];
            let base = c * hidden;
            for h in 0..hidden {
                sum += self.weights2[base + h] * hidden_act[h];
            }
            logits[c] = sum;
        }

        softmax(&logits)
    }

    pub fn predict_class_index(&self, features: &[f32]) -> usize {
        let proba = self.predict_proba(features);
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
    fn softmax_output_sums_to_one() {
        let model = MlpModel {
            model_version: 1,
            input_kind: MlpInputKind::FeaturesV1,
            feat_version: FEATURE_VERSION_V1,
            feature_len_f32: FEATURE_VECTOR_LEN_V1,
            classes: vec!["kick".into(), "snare".into()],
            hidden_size: 2,
            weights1: vec![0.0; FEATURE_VECTOR_LEN_V1 * 2],
            bias1: vec![0.0; 2],
            weights2: vec![0.0; 2 * 2],
            bias2: vec![0.0; 2],
            feature_mean: vec![0.0; FEATURE_VECTOR_LEN_V1],
            feature_std: vec![1.0; FEATURE_VECTOR_LEN_V1],
            class_thresholds: None,
            top2_margin: None,
            metrics: None,
        };
        let out = model.predict_proba(&vec![0.0; FEATURE_VECTOR_LEN_V1]);
        let sum: f32 = out.iter().sum();
        assert!((sum - 1.0).abs() < 1e-6);
    }
}
