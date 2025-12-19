use std::path::{Path, PathBuf};

use rusqlite::{Connection, OptionalExtension, params, params_from_iter};
use rusqlite::types::Value;

use crate::analysis::FEATURE_VERSION_V1;
use super::{ExportDiagnosticsSample, ExportError};

#[derive(Debug, Clone)]
pub struct ExportRow {
    pub sample_id: String,
    pub class_id: String,
    pub confidence: f32,
    pub rule_id: String,
    pub ruleset_version: i64,
    pub vec_blob: Vec<u8>,
}

#[derive(Debug, Clone)]
pub struct ExportDiagnostics {
    pub db_path: PathBuf,
    pub tables: Vec<String>,
    pub sources: Option<i64>,
    pub samples: Option<i64>,
    pub features_total: Option<i64>,
    pub features_v1: Option<i64>,
    pub labels_user_total: Option<i64>,
    pub labels_weak_total: Option<i64>,
    pub labels_weak_ruleset_ge_conf: Option<i64>,
    pub join_rows_user: Option<i64>,
    pub join_rows: Option<i64>,
    pub sample_splits: Vec<ExportDiagnosticsSample>,
}

pub fn open_db(db_path: &Path) -> Result<Connection, ExportError> {
    let conn = Connection::open(db_path)?;
    conn.execute_batch(
        "PRAGMA journal_mode=WAL;
         PRAGMA synchronous = NORMAL;
         PRAGMA foreign_keys=ON;",
    )?;
    Ok(conn)
}

pub fn diagnose_export(
    options: &crate::dataset::export::ExportOptions,
) -> Result<ExportDiagnostics, ExportError> {
    let db_path = options.resolved_db_path()?;
    let conn = open_db(&db_path)?;

    let tables = list_tables(&conn)?;

    let has = |name: &str| tables.iter().any(|t| t == name);

    let sources = if has("sources") {
        count_scalar(&conn, "SELECT COUNT(*) FROM sources", params![])? 
    } else { None };
    let samples = if has("samples") {
        count_scalar(&conn, "SELECT COUNT(*) FROM samples", params![])? 
    } else { None };
    let features_total = if has("features") {
        count_scalar(&conn, "SELECT COUNT(*) FROM features", params![])? 
    } else { None };
    let features_v1 = if has("features") {
        count_scalar(
            &conn,
            "SELECT COUNT(*) FROM features WHERE feat_version = ?1",
            params![FEATURE_VERSION_V1],
        )?
    } else {
        None
    };
    let labels_weak_total = if has("labels_weak") {
        count_scalar(&conn, "SELECT COUNT(*) FROM labels_weak", params![])? 
    } else { None };
    let labels_user_total = if has("labels_user") {
        count_scalar(&conn, "SELECT COUNT(*) FROM labels_user", params![])? 
    } else { None };
    let labels_weak_ruleset_ge_conf = if has("labels_weak") {
        count_scalar(
            &conn,
            "SELECT COUNT(*) FROM labels_weak WHERE ruleset_version = ?1 AND confidence >= ?2",
            params![1_i64, options.min_confidence],
        )?
    } else {
        None
    };
    let join_rows_user = if has("features") && has("labels_user") {
        count_scalar(
            &conn,
            "SELECT COUNT(*)
             FROM features f
             JOIN labels_user l ON l.sample_id = f.sample_id
             WHERE f.feat_version = ?1",
            params![FEATURE_VERSION_V1],
        )?
    } else {
        None
    };
    let join_rows = if has("features") && has("labels_weak") {
        count_scalar(
            &conn,
            "SELECT COUNT(*)
             FROM features f
             JOIN labels_weak l ON l.sample_id = f.sample_id
             WHERE f.feat_version = ?1 AND l.ruleset_version = ?2 AND l.confidence >= ?3",
            params![FEATURE_VERSION_V1, 1_i64, options.min_confidence],
        )?
    } else {
        None
    };

    let mut sample_splits = Vec::new();
    if let Ok(rows) =
        load_export_rows(&conn, options.min_confidence, 1, options.use_user_labels)
    {
        for row in rows.into_iter().take(10) {
            if let Some(pack_id) = super::pack_id_for_sample_id(&row.sample_id, options.pack_depth)
            {
                if let Ok(split) = super::split_for_pack_id(
                    &pack_id,
                    &options.seed,
                    options.test_fraction,
                    options.val_fraction,
                ) {
                    sample_splits.push(ExportDiagnosticsSample {
                        sample_id: row.sample_id,
                        pack_id,
                        split,
                    });
                }
            }
        }
    }

    Ok(ExportDiagnostics {
        db_path,
        tables,
        sources,
        samples,
        features_total,
        features_v1,
        labels_user_total,
        labels_weak_total,
        labels_weak_ruleset_ge_conf,
        join_rows_user,
        join_rows,
        sample_splits,
    })
}

fn list_tables(conn: &Connection) -> Result<Vec<String>, ExportError> {
    let mut stmt = conn.prepare("SELECT name FROM sqlite_master WHERE type='table' ORDER BY name")?;
    let rows = stmt
        .query_map([], |row| row.get::<_, String>(0))?
        .collect::<Result<Vec<_>, _>>()?;
    Ok(rows)
}

fn count_scalar<P: rusqlite::Params>(
    conn: &Connection,
    sql: &str,
    params: P,
) -> Result<Option<i64>, ExportError> {
    conn.query_row(sql, params, |row| row.get::<_, i64>(0))
        .optional()
        .map_err(ExportError::from)
}

pub fn load_export_rows(
    conn: &Connection,
    min_confidence: f32,
    ruleset_version: i64,
    include_user_labels: bool,
) -> Result<Vec<ExportRow>, ExportError> {
    load_export_rows_filtered(
        conn,
        min_confidence,
        ruleset_version,
        None,
        include_user_labels,
    )
}

pub fn load_export_rows_filtered(
    conn: &Connection,
    min_confidence: f32,
    ruleset_version: i64,
    source_id_prefixes: Option<&[String]>,
    include_user_labels: bool,
) -> Result<Vec<ExportRow>, ExportError> {
    let mut sql = String::from(
        "WITH best_weak AS (
            SELECT l.sample_id, l.class_id, l.confidence, l.rule_id, l.ruleset_version
            FROM labels_weak l
            WHERE l.ruleset_version = ?2
              AND l.confidence >= ?3
              AND l.class_id = (
                SELECT l2.class_id
                FROM labels_weak l2
                WHERE l2.sample_id = l.sample_id
                  AND l2.ruleset_version = ?2
                  AND l2.confidence >= ?3
                ORDER BY l2.confidence DESC, l2.class_id ASC
                LIMIT 1
              )
        )
        SELECT f.sample_id,
               f.vec_blob,",
    );
    if include_user_labels {
        sql.push_str(
            " COALESCE(u.class_id, w.class_id) AS class_id,
              CASE WHEN u.class_id IS NOT NULL THEN 1.0 ELSE w.confidence END AS confidence,
              CASE WHEN u.class_id IS NOT NULL THEN 'user_override' ELSE w.rule_id END AS rule_id,
              CASE WHEN u.class_id IS NOT NULL THEN 0 ELSE w.ruleset_version END AS ruleset_version
             FROM features f
             LEFT JOIN labels_user u ON u.sample_id = f.sample_id
             LEFT JOIN best_weak w ON w.sample_id = f.sample_id
             WHERE f.feat_version = ?1
               AND (u.class_id IS NOT NULL OR w.class_id IS NOT NULL)",
        );
    } else {
        sql.push_str(
            " w.class_id AS class_id,
              w.confidence AS confidence,
              w.rule_id AS rule_id,
              w.ruleset_version AS ruleset_version
             FROM features f
             JOIN best_weak w ON w.sample_id = f.sample_id
             WHERE f.feat_version = ?1",
        );
    }

    let mut params_vec: Vec<Value> = vec![
        Value::Integer(FEATURE_VERSION_V1),
        Value::Integer(ruleset_version),
        Value::Real(min_confidence as f64),
    ];

    if let Some(prefixes) = source_id_prefixes
        && !prefixes.is_empty()
    {
        sql.push_str(" AND (");
        for (idx, source_id) in prefixes.iter().enumerate() {
            if idx > 0 {
                sql.push_str(" OR ");
            }
            sql.push_str("f.sample_id LIKE ?");
            sql.push_str(&(params_vec.len() + 1).to_string());
            params_vec.push(Value::Text(format!("{source_id}::%")));
        }
        sql.push(')');
    }
    sql.push_str(" ORDER BY f.sample_id ASC");

    let mut stmt = conn.prepare(&sql)?;
    let rows = stmt.query_map(params_from_iter(params_vec), |row| {
            Ok(ExportRow {
                sample_id: row.get(0)?,
                vec_blob: row.get(1)?,
                class_id: row.get(2)?,
                confidence: row.get::<_, f64>(3)? as f32,
                rule_id: row.get(4)?,
                ruleset_version: row.get(5)?,
            })
        })?
        .collect::<Result<Vec<_>, _>>()?;
    Ok(rows)
}

pub fn load_embedding_export_rows_filtered(
    conn: &Connection,
    min_confidence: f32,
    ruleset_version: i64,
    source_id_prefixes: Option<&[String]>,
    include_user_labels: bool,
    embedding_model_id: &str,
) -> Result<Vec<ExportRow>, ExportError> {
    let mut sql = String::from(
        "WITH best_weak AS (
            SELECT l.sample_id, l.class_id, l.confidence, l.rule_id, l.ruleset_version
            FROM labels_weak l
            WHERE l.ruleset_version = ?2
              AND l.confidence >= ?3
              AND l.class_id = (
                SELECT l2.class_id
                FROM labels_weak l2
                WHERE l2.sample_id = l.sample_id
                  AND l2.ruleset_version = ?2
                  AND l2.confidence >= ?3
                ORDER BY l2.confidence DESC, l2.class_id ASC
                LIMIT 1
              )
        )
        SELECT e.sample_id,
               e.vec_blob,",
    );
    if include_user_labels {
        sql.push_str(
            " COALESCE(u.class_id, w.class_id) AS class_id,
              CASE WHEN u.class_id IS NOT NULL THEN 1.0 ELSE w.confidence END AS confidence,
              CASE WHEN u.class_id IS NOT NULL THEN 'user_override' ELSE w.rule_id END AS rule_id,
              CASE WHEN u.class_id IS NOT NULL THEN 0 ELSE w.ruleset_version END AS ruleset_version
             FROM embeddings e
             LEFT JOIN labels_user u ON u.sample_id = e.sample_id
             LEFT JOIN best_weak w ON w.sample_id = e.sample_id
             WHERE e.model_id = ?1
               AND (u.class_id IS NOT NULL OR w.class_id IS NOT NULL)",
        );
    } else {
        sql.push_str(
            " w.class_id AS class_id,
              w.confidence AS confidence,
              w.rule_id AS rule_id,
              w.ruleset_version AS ruleset_version
             FROM embeddings e
             JOIN best_weak w ON w.sample_id = e.sample_id
             WHERE e.model_id = ?1",
        );
    }

    let mut params_vec: Vec<Value> = vec![
        Value::Text(embedding_model_id.to_string()),
        Value::Integer(ruleset_version),
        Value::Real(min_confidence as f64),
    ];

    if let Some(prefixes) = source_id_prefixes
        && !prefixes.is_empty()
    {
        sql.push_str(" AND (");
        for (idx, source_id) in prefixes.iter().enumerate() {
            if idx > 0 {
                sql.push_str(" OR ");
            }
            sql.push_str("e.sample_id LIKE ?");
            sql.push_str(&(params_vec.len() + 1).to_string());
            params_vec.push(Value::Text(format!("{source_id}::%")));
        }
        sql.push(')');
    }
    sql.push_str(" ORDER BY e.sample_id ASC");

    let mut stmt = conn.prepare(&sql)?;
    let rows = stmt
        .query_map(params_from_iter(params_vec), |row| {
            Ok(ExportRow {
                sample_id: row.get(0)?,
                vec_blob: row.get(1)?,
                class_id: row.get(2)?,
                confidence: row.get::<_, f64>(3)? as f32,
                rule_id: row.get(4)?,
                ruleset_version: row.get(5)?,
            })
        })?
        .collect::<Result<Vec<_>, _>>()?;
    Ok(rows)
}
