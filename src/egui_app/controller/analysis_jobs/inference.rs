use rusqlite::{Connection, OptionalExtension, params};
use log::warn;
use std::borrow::Cow;

use super::types::TopKProbability;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

#[derive(Debug, Clone)]
pub(super) enum CachedModelKind {
    Gbdt(crate::ml::gbdt_stump::GbdtStumpModel),
    Mlp(crate::ml::mlp::MlpModel),
    LogReg(crate::ml::logreg::LogRegModel),
}

#[derive(Debug, Clone)]
pub(super) struct CachedModel {
    pub(super) model_id: String,
    pub(super) kind: String,
    pub(super) model: CachedModelKind,
}

#[derive(Debug, Clone)]
pub(super) struct CachedHead {
    pub(super) head_id: String,
    pub(super) model_id: String,
    pub(super) dim: usize,
    pub(super) num_classes: usize,
    pub(super) temperature: f32,
    pub(super) weights: Vec<f32>,
    pub(super) bias: Vec<f32>,
    pub(super) classes: Vec<String>,
}

pub(super) struct InferenceInputs<'a> {
    pub(super) features: Option<&'a [f32]>,
    pub(super) embedding: Option<&'a [f32]>,
}

pub(super) fn refresh_latest_model(
    conn: &Connection,
    cache: &mut Option<CachedModel>,
    preferred_model_id: Option<&str>,
) -> Result<(), String> {
    ensure_bundled_model(conn)?;
    let row: Option<(String, String, String)> = if let Some(model_id) = preferred_model_id {
        conn.query_row(
            "SELECT model_id, kind, model_json
             FROM models
             WHERE model_id = ?1",
            params![model_id],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
        )
        .optional()
        .map_err(|err| format!("Failed to query preferred model: {err}"))?
    } else {
        None
    };
    let row = if row.is_some() {
        row
    } else {
        conn.query_row(
            "SELECT model_id, kind, model_json
             FROM models
             ORDER BY created_at DESC, model_id DESC
             LIMIT 1",
            [],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
        )
        .optional()
        .map_err(|err| format!("Failed to query latest model: {err}"))?
    };

    let Some((model_id, kind, model_json)) = row else {
        *cache = None;
        return Ok(());
    };
    if cache.as_ref().is_some_and(|cached| cached.model_id == model_id) {
        return Ok(());
    }

    let model = match kind.as_str() {
        "gbdt_stump_v1" | "gbdt" => {
            let model: crate::ml::gbdt_stump::GbdtStumpModel =
                serde_json::from_str(&model_json)
                    .map_err(|err| format!("Failed to parse model_json: {err}"))?;
            if let Err(err) = model.validate() {
                warn!("Skipping model {model_id}: {err}");
                *cache = None;
                return Ok(());
            }
            CachedModelKind::Gbdt(model)
        }
        "mlp_v1" => {
            let model: crate::ml::mlp::MlpModel = serde_json::from_str(&model_json)
                .map_err(|err| format!("Failed to parse model_json: {err}"))?;
            if let Err(err) = model.validate() {
                warn!("Skipping model {model_id}: {err}");
                *cache = None;
                return Ok(());
            }
            CachedModelKind::Mlp(model)
        }
        "logreg_v1" => {
            let model: crate::ml::logreg::LogRegModel =
                serde_json::from_str(&model_json)
                    .map_err(|err| format!("Failed to parse model_json: {err}"))?;
            if let Err(err) = model.validate() {
                warn!("Skipping model {model_id}: {err}");
                *cache = None;
                return Ok(());
            }
            CachedModelKind::LogReg(model)
        }
        _ => {
            *cache = None;
            return Ok(());
        }
    };
    *cache = Some(CachedModel {
        model_id,
        kind,
        model,
    });
    Ok(())
}

pub(super) fn infer_and_upsert_prediction(
    conn: &Connection,
    cache: &mut Option<CachedModel>,
    head_cache: &mut Option<CachedHead>,
    preferred_model_id: Option<&str>,
    sample_id: &str,
    content_hash: &str,
    inputs: InferenceInputs<'_>,
    computed_at: i64,
    unknown_confidence_threshold: f32,
) -> Result<(), String> {
    infer_and_upsert_head_prediction(
        conn,
        head_cache,
        sample_id,
        inputs.embedding,
        unknown_confidence_threshold,
    )?;
    refresh_latest_model(conn, cache, preferred_model_id)?;
    let Some(cached) = cache.as_ref() else {
        return Ok(());
    };
    let user_label: Option<String> = conn
        .query_row(
            "SELECT class_id FROM labels_user WHERE sample_id = ?1",
            params![sample_id],
            |row| row.get(0),
        )
        .optional()
        .map_err(|err| format!("Failed to query labels_user: {err}"))?;
    if let Some(class_id) = user_label {
        let topk = vec![TopKProbability {
            class_id: class_id.clone(),
            probability: 1.0,
        }];
        let topk_json = serde_json::to_string(&topk).map_err(|err| err.to_string())?;
        conn.execute(
            "INSERT INTO predictions (sample_id, model_id, content_hash, top_class, confidence, topk_json, computed_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)
             ON CONFLICT(sample_id, model_id) DO UPDATE SET
                content_hash = excluded.content_hash,
                top_class = excluded.top_class,
                confidence = excluded.confidence,
                topk_json = excluded.topk_json,
                computed_at = excluded.computed_at",
            params![
                sample_id,
                cached.model_id,
                content_hash,
                class_id,
                1.0_f64,
                topk_json,
                computed_at
            ],
        )
        .map_err(|err| format!("Failed to upsert prediction: {err}"))?;
        return Ok(());
    }
    let (classes, proba) = match &cached.model {
        CachedModelKind::Gbdt(model) => {
            if model.feat_version != crate::analysis::FEATURE_VERSION_V1
                || model.feature_len_f32 != crate::analysis::FEATURE_VECTOR_LEN_V1
            {
                return Ok(());
            }
            let Some(features) = inputs.features else {
                return Ok(());
            };
            if features.len() != model.feature_len_f32 {
                return Ok(());
            }
            let proba = model.predict_proba(features);
            (model.classes.clone(), proba)
        }
        CachedModelKind::Mlp(model) => {
            let input: Cow<'_, [f32]> = match model.input_kind {
                crate::ml::mlp::MlpInputKind::FeaturesV1 => {
                    let Some(features) = inputs.features else {
                        return Ok(());
                    };
                    if features.len() != model.feature_len_f32 {
                        return Ok(());
                    }
                    Cow::Borrowed(features)
                }
                crate::ml::mlp::MlpInputKind::EmbeddingV1 => {
                    let Some(embedding) = inputs.embedding else {
                        return Ok(());
                    };
                    if embedding.len() != model.feature_len_f32 {
                        return Ok(());
                    }
                    Cow::Borrowed(embedding)
                }
                crate::ml::mlp::MlpInputKind::HybridV1 => {
                    let Some(embedding) = inputs.embedding else {
                        return Ok(());
                    };
                    let Some(features) = inputs.features else {
                        return Ok(());
                    };
                    let Some(light) = crate::analysis::light_dsp_from_features_v1(features) else {
                        return Ok(());
                    };
                    let mut combined =
                        Vec::with_capacity(embedding.len() + light.len());
                    combined.extend_from_slice(embedding);
                    combined.extend_from_slice(&light);
                    if combined.len() != model.feature_len_f32 {
                        return Ok(());
                    }
                    Cow::Owned(combined)
                }
            };
            let proba = model.predict_proba(input.as_ref());
            (model.classes.clone(), proba)
        }
        CachedModelKind::LogReg(model) => {
            let Some(embedding) = inputs.embedding else {
                return Ok(());
            };
            if embedding.len() != model.embedding_dim {
                return Ok(());
            }
            let proba = model.predict_proba(embedding);
            (model.classes.clone(), proba)
        }
    };
    if proba.is_empty() || proba.len() != classes.len() {
        return Ok(());
    }
    let (top_idx, confidence, top2, topk) = topk_from_proba(&classes, &proba, 5);
    let mut top_class = classes.get(top_idx).cloned().unwrap_or_default();
    let mut is_unknown = confidence < unknown_confidence_threshold;
    match &cached.model {
        CachedModelKind::Mlp(model) => {
            if let Some(thresholds) = &model.class_thresholds {
                if let Some(threshold) = thresholds.get(top_idx) {
                    if confidence < *threshold {
                        is_unknown = true;
                    }
                }
            }
            if let Some(margin) = model.top2_margin {
                if (confidence - top2) < margin {
                    is_unknown = true;
                }
            }
        }
        CachedModelKind::LogReg(model) => {
            if let Some(thresholds) = &model.class_thresholds {
                if let Some(threshold) = thresholds.get(top_idx) {
                    if confidence < *threshold {
                        is_unknown = true;
                    }
                }
            }
            if let Some(margin) = model.top2_margin {
                if (confidence - top2) < margin {
                    is_unknown = true;
                }
            }
        }
        _ => {}
    }
    if is_unknown {
        top_class = "UNKNOWN".to_string();
    }
    let topk_json = serde_json::to_string(&topk).map_err(|err| err.to_string())?;

    conn.execute(
        "INSERT INTO predictions (sample_id, model_id, content_hash, top_class, confidence, topk_json, computed_at)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)
         ON CONFLICT(sample_id, model_id) DO UPDATE SET
            content_hash = excluded.content_hash,
            top_class = excluded.top_class,
            confidence = excluded.confidence,
            topk_json = excluded.topk_json,
            computed_at = excluded.computed_at",
        params![
            sample_id,
            cached.model_id,
            content_hash,
            top_class,
            confidence as f64,
            topk_json,
            computed_at
        ],
    )
    .map_err(|err| format!("Failed to upsert prediction: {err}"))?;

    Ok(())
}

pub(super) fn ensure_bundled_model(conn: &Connection) -> Result<(), String> {
    let bundled_id = crate::ml::logreg::DEFAULT_CLASSIFIER_MODEL_ID;
    let exists: Option<String> = conn
        .query_row(
            "SELECT model_id FROM models WHERE model_id = ?1",
            params![bundled_id],
            |row| row.get(0),
        )
        .optional()
        .map_err(|err| format!("Failed to query bundled model: {err}"))?;
    if exists.is_some() {
        return Ok(());
    }
    let model = crate::ml::logreg::LogRegModel::bundled();
    let model_json = serde_json::to_string(&model).map_err(|err| err.to_string())?;
    let classes_json = serde_json::to_string(&model.classes).map_err(|err| err.to_string())?;
    let created_at = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_else(|_| Duration::from_secs(0))
        .as_secs() as i64;
    conn.execute(
        "INSERT INTO models (model_id, kind, model_version, feat_version, feature_len_f32, classes_json, model_json, created_at)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
        params![
            bundled_id,
            "logreg_v1",
            model.model_version,
            0i64,
            model.embedding_dim as i64,
            classes_json,
            model_json,
            created_at
        ],
    )
    .map_err(|err| format!("Failed to insert bundled model: {err}"))?;
    Ok(())
}

fn topk_from_proba(
    classes: &[String],
    proba: &[f32],
    k: usize,
) -> (usize, f32, f32, Vec<TopKProbability>) {
    let mut indices: Vec<usize> = (0..proba.len()).collect();
    indices.sort_by(|&a, &b| {
        proba[b]
            .partial_cmp(&proba[a])
            .unwrap_or(std::cmp::Ordering::Equal)
    });
    let k = k.min(indices.len());
    let mut topk = Vec::with_capacity(k);
    for &idx in indices.iter().take(k) {
        topk.push(TopKProbability {
            class_id: classes.get(idx).cloned().unwrap_or_default(),
            probability: proba[idx],
        });
    }
    let top_idx = indices.first().copied().unwrap_or(0);
    let top_prob = proba.get(top_idx).copied().unwrap_or(0.0);
    let top2_prob = indices
        .get(1)
        .and_then(|idx| proba.get(*idx))
        .copied()
        .unwrap_or(0.0);
    (top_idx, top_prob, top2_prob, topk)
}

fn infer_and_upsert_head_prediction(
    conn: &Connection,
    cache: &mut Option<CachedHead>,
    sample_id: &str,
    embedding: Option<&[f32]>,
    unknown_margin_threshold: f32,
) -> Result<(), String> {
    let user_label: Option<String> = conn
        .query_row(
            "SELECT class_id FROM labels_user WHERE sample_id = ?1",
            params![sample_id],
            |row| row.get(0),
        )
        .optional()
        .map_err(|err| format!("Failed to query labels_user: {err}"))?;
    if let Some(class_id) = user_label {
        return upsert_head_prediction(conn, sample_id, cache, class_id, 1.0);
    }

    refresh_latest_head(conn, cache)?;
    let Some(head) = cache.as_ref() else {
        return Ok(());
    };
    let Some(embedding) = embedding else {
        return Ok(());
    };
    if embedding.len() != head.dim {
        return Ok(());
    }
    let (logits, proba) = head_logits_and_proba(head, embedding)?;
    if logits.is_empty() || proba.is_empty() {
        return Ok(());
    }
    let (top_idx, confidence, top2, _topk) =
        topk_from_proba(&head.classes, &proba, 5);
    let margin = confidence - top2;
    let class_id = if margin < unknown_margin_threshold {
        "UNKNOWN".to_string()
    } else {
        head.classes.get(top_idx).cloned().unwrap_or_default()
    };
    upsert_head_prediction(conn, sample_id, cache, class_id, confidence as f64)?;
    Ok(())
}

fn upsert_head_prediction(
    conn: &Connection,
    sample_id: &str,
    cache: &mut Option<CachedHead>,
    class_id: String,
    score: f64,
) -> Result<(), String> {
    refresh_latest_head(conn, cache)?;
    let Some(head) = cache.as_ref() else {
        return Ok(());
    };
    conn.execute(
        "INSERT INTO predictions_head (sample_id, head_id, class_id, score)
         VALUES (?1, ?2, ?3, ?4)
         ON CONFLICT(sample_id, head_id) DO UPDATE SET
            class_id = excluded.class_id,
            score = excluded.score",
        params![sample_id, head.head_id, class_id, score],
    )
    .map_err(|err| format!("Failed to upsert head prediction: {err}"))?;
    Ok(())
}

fn refresh_latest_head(conn: &Connection, cache: &mut Option<CachedHead>) -> Result<(), String> {
    let row: Option<(String, String, i64, i64, String, f32, Vec<u8>, Vec<u8>)> = conn
        .query_row(
            "SELECT head_id, model_id, dim, num_classes, norm, temperature, weights, bias
             FROM classifier_models
             WHERE model_id = ?1
             ORDER BY head_id DESC
             LIMIT 1",
            params![crate::analysis::embedding::EMBEDDING_MODEL_ID],
            |row| {
                Ok((
                    row.get(0)?,
                    row.get(1)?,
                    row.get(2)?,
                    row.get(3)?,
                    row.get(4)?,
                    row.get(5)?,
                    row.get(6)?,
                    row.get(7)?,
                ))
            },
        )
        .optional()
        .map_err(|err| format!("Failed to query classifier_models: {err}"))?;
    let Some((head_id, model_id, dim, num_classes, norm, temperature, weights_blob, bias_blob)) = row
    else {
        *cache = None;
        return Ok(());
    };
    if cache
        .as_ref()
        .is_some_and(|cached| cached.head_id == head_id)
    {
        return Ok(());
    }

    let classes = load_classes_v1()?;
    if classes.len() != num_classes as usize {
        return Err(format!(
            "Head class count mismatch: expected {}, got {}",
            classes.len(),
            num_classes
        ));
    }
    let dim = dim as usize;
    if dim != crate::analysis::embedding::EMBEDDING_DIM {
        return Err(format!(
            "Head dim mismatch: expected {}, got {}",
            crate::analysis::embedding::EMBEDDING_DIM,
            dim
        ));
    }
    if !temperature.is_finite() || temperature <= 0.0 {
        return Err("Head temperature must be > 0".to_string());
    }
    if norm != "l2" {
        return Err(format!("Unsupported head norm: {}", norm));
    }
    let weights = crate::analysis::decode_f32_le_blob(&weights_blob)?;
    let bias = crate::analysis::decode_f32_le_blob(&bias_blob)?;
    if weights.len() != classes.len() * dim {
        return Err("Head weights length mismatch".to_string());
    }
    if bias.len() != classes.len() {
        return Err("Head bias length mismatch".to_string());
    }
    *cache = Some(CachedHead {
        head_id,
        model_id,
        dim,
        num_classes: classes.len(),
        temperature,
        weights,
        bias,
        classes,
    });
    Ok(())
}

fn head_logits_and_proba(
    head: &CachedHead,
    embedding: &[f32],
) -> Result<(Vec<f32>, Vec<f32>), String> {
    let mut logits = vec![0.0_f32; head.num_classes];
    for class_idx in 0..head.num_classes {
        let base = class_idx * head.dim;
        let mut sum = head.bias.get(class_idx).copied().unwrap_or(0.0);
        let weights = &head.weights[base..base + head.dim];
        for (w, x) in weights.iter().zip(embedding.iter()) {
            sum += w * x;
        }
        logits[class_idx] = sum / head.temperature;
    }
    let proba = crate::ml::gbdt_stump::softmax(&logits);
    Ok((logits, proba))
}

fn load_classes_v1() -> Result<Vec<String>, String> {
    #[derive(serde::Deserialize)]
    struct ClassesManifest {
        classes: Vec<ClassDef>,
    }
    #[derive(serde::Deserialize)]
    struct ClassDef {
        id: String,
    }
    let manifest: ClassesManifest = serde_json::from_str(include_str!(
        "../../../../assets/ml/classes_v1.json"
    ))
    .map_err(|err| format!("Failed to parse classes_v1.json: {err}"))?;
    let classes: Vec<String> = manifest
        .classes
        .into_iter()
        .map(|c| c.id)
        .collect();
    if classes.is_empty() {
        return Err("classes_v1.json has no classes".to_string());
    }
    Ok(classes)
}

#[cfg(test)]
mod tests {
    use super::*;
    use rusqlite::Connection;
    use serde::Deserialize;

    fn conn_with_schema() -> Connection {
        let conn = Connection::open_in_memory().unwrap();
        conn.execute_batch(
            "CREATE TABLE models (
                model_id TEXT PRIMARY KEY,
                kind TEXT NOT NULL,
                model_version INTEGER NOT NULL,
                feat_version INTEGER NOT NULL,
                feature_len_f32 INTEGER NOT NULL,
                classes_json TEXT NOT NULL,
                model_json TEXT NOT NULL,
                created_at INTEGER NOT NULL
            ) WITHOUT ROWID;
            CREATE TABLE predictions (
                sample_id TEXT NOT NULL,
                model_id TEXT NOT NULL,
                content_hash TEXT NOT NULL,
                top_class TEXT NOT NULL,
                confidence REAL NOT NULL,
                topk_json TEXT NOT NULL,
                computed_at INTEGER NOT NULL,
                PRIMARY KEY (sample_id, model_id)
            ) WITHOUT ROWID;",
        )
        .unwrap();
        conn
    }

    #[test]
    fn refresh_loads_latest_model() {
        let conn = conn_with_schema();
        let model = crate::ml::gbdt_stump::GbdtStumpModel {
            model_version: 1,
            feat_version: crate::analysis::FEATURE_VERSION_V1,
            feature_len_f32: crate::analysis::FEATURE_VECTOR_LEN_V1,
            classes: vec!["kick".into(), "snare".into()],
            learning_rate: 1.0,
            init_raw: vec![0.0, 0.0],
            stumps: vec![],
        };
        let model_json = serde_json::to_string(&model).unwrap();
        conn.execute(
            "INSERT INTO models (model_id, kind, model_version, feat_version, feature_len_f32, classes_json, model_json, created_at)
             VALUES ('m1','gbdt_stump_v1',1,?1,?2,'[]',?3,10)",
            params![
                crate::analysis::FEATURE_VERSION_V1,
                crate::analysis::FEATURE_VECTOR_LEN_V1 as i64,
                model_json
            ],
        )
        .unwrap();

        let mut cache = None;
        refresh_latest_model(&conn, &mut cache, Some("m1")).unwrap();
        assert!(cache.is_some());
        assert_eq!(cache.unwrap().model_id, "m1");
    }

    #[derive(Deserialize)]
    struct GoldenInference {
        dim: usize,
        num_classes: usize,
        temperature: f32,
        embedding: Vec<f32>,
        weights: Vec<f32>,
        bias: Vec<f32>,
        probs: Vec<f32>,
    }

    #[test]
    fn golden_inference_matches_python() {
        let path = match std::env::var("SEMPAL_GOLDEN_INFER_PATH") {
            Ok(path) if !path.trim().is_empty() => path,
            _ => return,
        };
        let payload = std::fs::read_to_string(path).expect("read golden json");
        let golden: GoldenInference =
            serde_json::from_str(&payload).expect("parse golden json");
        assert_eq!(golden.embedding.len(), golden.dim);
        assert_eq!(golden.weights.len(), golden.dim * golden.num_classes);
        assert_eq!(golden.bias.len(), golden.num_classes);
        assert_eq!(golden.probs.len(), golden.num_classes);

        let mut logits = vec![0.0_f32; golden.num_classes];
        for class_idx in 0..golden.num_classes {
            let base = class_idx * golden.dim;
            let mut sum = golden.bias[class_idx];
            let weights = &golden.weights[base..base + golden.dim];
            for (w, x) in weights.iter().zip(golden.embedding.iter()) {
                sum += w * x;
            }
            logits[class_idx] = sum / golden.temperature;
        }
        let probs = crate::ml::gbdt_stump::softmax(&logits);
        let mut max_diff = 0.0_f32;
        for (a, b) in probs.iter().zip(golden.probs.iter()) {
            max_diff = max_diff.max((a - b).abs());
        }
        const MAX_DIFF: f32 = 1e-5;
        assert!(
            max_diff <= MAX_DIFF,
            "max diff {max_diff} exceeds {MAX_DIFF}"
        );
    }
}
