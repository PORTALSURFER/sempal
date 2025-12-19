use rusqlite::{Connection, OptionalExtension, params};

use super::types::TopKProbability;


#[derive(Debug, Clone)]
pub(super) struct CachedModel {
    pub(super) model_id: String,
    pub(super) model: crate::ml::gbdt_stump::GbdtStumpModel,
}

pub(super) fn refresh_latest_model(
    conn: &Connection,
    cache: &mut Option<CachedModel>,
) -> Result<(), String> {
    let row: Option<(String, String)> = conn
        .query_row(
            "SELECT model_id, model_json
             FROM models
             ORDER BY created_at DESC, model_id DESC
             LIMIT 1",
            [],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .optional()
        .map_err(|err| format!("Failed to query latest model: {err}"))?;

    let Some((model_id, model_json)) = row else {
        *cache = None;
        return Ok(());
    };
    if cache.as_ref().is_some_and(|cached| cached.model_id == model_id) {
        return Ok(());
    }

    let model: crate::ml::gbdt_stump::GbdtStumpModel = serde_json::from_str(&model_json)
        .map_err(|err| format!("Failed to parse model_json: {err}"))?;
    model.validate()?;
    *cache = Some(CachedModel { model_id, model });
    Ok(())
}

pub(super) fn infer_and_upsert_prediction(
    conn: &Connection,
    cache: &mut Option<CachedModel>,
    sample_id: &str,
    content_hash: &str,
    features: &[f32],
    computed_at: i64,
    _unknown_confidence_threshold: f32,
) -> Result<(), String> {
    refresh_latest_model(conn, cache)?;
    let Some(cached) = cache.as_ref() else {
        return Ok(());
    };
    if cached.model.feat_version != crate::analysis::FEATURE_VERSION_V1
        || cached.model.feature_len_f32 != crate::analysis::FEATURE_VECTOR_LEN_V1
    {
        return Ok(());
    }
    if features.len() != cached.model.feature_len_f32 {
        return Ok(());
    }

    let proba = cached.model.predict_proba(features);
    if proba.is_empty() || proba.len() != cached.model.classes.len() {
        return Ok(());
    }
    let (top_class, confidence, topk) = topk_from_proba(&cached.model.classes, &proba, 5);
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

fn topk_from_proba(
    classes: &[String],
    proba: &[f32],
    k: usize,
) -> (String, f32, Vec<TopKProbability>) {
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
    let top = topk
        .first()
        .cloned()
        .unwrap_or(TopKProbability {
            class_id: String::new(),
            probability: 0.0,
        });
    (top.class_id, top.probability, topk)
}

#[cfg(test)]
mod tests {
    use super::*;
    use rusqlite::Connection;

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
             VALUES ('m1','gbdt',1,?1,?2,'[]',?3,10)",
            params![
                crate::analysis::FEATURE_VERSION_V1,
                crate::analysis::FEATURE_VECTOR_LEN_V1 as i64,
                model_json
            ],
        )
        .unwrap();

        let mut cache = None;
        refresh_latest_model(&conn, &mut cache).unwrap();
        assert!(cache.is_some());
        assert_eq!(cache.unwrap().model_id, "m1");
    }
}
