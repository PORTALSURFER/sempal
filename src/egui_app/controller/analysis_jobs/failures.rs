use super::db;
use rusqlite::{Connection, params};
use std::collections::HashMap;
use std::path::PathBuf;

pub(in crate::egui_app::controller) fn failed_samples_for_source(
    source: &crate::sample_sources::SampleSource,
) -> Result<HashMap<PathBuf, String>, String> {
    let conn = db::open_source_db(&source.root)?;
    failed_samples_for_source_conn(&conn, &source.id)
}

fn failed_samples_for_source_conn(
    conn: &Connection,
    source_id: &crate::sample_sources::SourceId,
) -> Result<HashMap<PathBuf, String>, String> {
    let prefix = format!("{}::%", source_id.as_str());
    let embedding_model = crate::analysis::embedding::EMBEDDING_MODEL_ID;
    let analysis_version = crate::analysis::version::analysis_version();
    let mut stmt = conn
        .prepare(
            "SELECT aj.sample_id, aj.last_error
             FROM analysis_jobs aj
             LEFT JOIN samples s ON s.sample_id = aj.sample_id
             LEFT JOIN features f
                ON f.sample_id = aj.sample_id AND f.feat_version = ?2
             LEFT JOIN embeddings e
                ON e.sample_id = aj.sample_id AND e.model_id = ?3
             WHERE aj.status = 'failed' AND aj.sample_id LIKE ?1
               AND (
                  f.sample_id IS NULL
                  OR s.analysis_version IS NULL
                  OR s.analysis_version != ?4
                  OR e.sample_id IS NULL
               )
             ORDER BY aj.sample_id ASC",
        )
        .map_err(|err| format!("Failed to query failed analysis jobs: {err}"))?;
    let mut out = HashMap::new();
    let rows = stmt
        .query_map(
            params![prefix, 1i64, embedding_model, analysis_version],
            |row| {
                let sample_id: String = row.get(0)?;
                let last_error: Option<String> = row.get(1)?;
                Ok((sample_id, last_error))
            },
        )
        .map_err(|err| format!("Failed to query failed analysis jobs: {err}"))?;
    for row in rows {
        let (sample_id, last_error) =
            row.map_err(|err| format!("Failed to decode failed analysis job row: {err}"))?;
        let (_source, relative_path) = db::parse_sample_id(&sample_id)?;
        out.insert(
            relative_path,
            last_error.unwrap_or_else(|| "Analysis failed".to_string()),
        );
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::app_dirs::ConfigBaseGuard;
    use tempfile::tempdir;

    #[test]
    fn loads_failed_jobs_for_source() {
        let config_dir = tempdir().unwrap();
        let _guard = ConfigBaseGuard::set(config_dir.path().to_path_buf());
        let source_root = tempdir().unwrap();
        let source = crate::sample_sources::SampleSource::new_with_id(
            crate::sample_sources::SourceId::from_string("s1"),
            source_root.path().to_path_buf(),
        );
        let conn = db::open_source_db(&source.root).unwrap();
        conn.execute_batch(
            "DELETE FROM analysis_jobs;
             DELETE FROM samples;
             DELETE FROM features;
             DELETE FROM embeddings;",
        )
        .unwrap();
        conn.execute(
            "INSERT INTO analysis_jobs (sample_id, job_type, status, attempts, created_at, last_error)
             VALUES ('s1::Pack/a.wav', 'x', 'failed', 1, 0, 'boom')",
            [],
        )
        .unwrap();
        conn.execute(
            "INSERT INTO analysis_jobs (sample_id, job_type, status, attempts, created_at)
             VALUES ('s1::Pack/b.wav', 'x', 'failed', 1, 0)",
            [],
        )
        .unwrap();
        conn.execute(
            "INSERT INTO analysis_jobs (sample_id, job_type, status, attempts, created_at, last_error)
             VALUES ('s2::Other/c.wav', 'x', 'failed', 1, 0, 'nope')",
            [],
        )
        .unwrap();
        conn.execute(
            "INSERT INTO samples (sample_id, analysis_version)
             VALUES ('s1::Pack/a.wav', ?1)",
            params![crate::analysis::version::analysis_version()],
        )
        .unwrap();
        conn.execute(
            "INSERT INTO features (sample_id, feat_version, vec_blob, computed_at)
             VALUES ('s1::Pack/a.wav', 1, X'00', 0)",
            [],
        )
        .unwrap();
        conn.execute(
            "INSERT INTO embeddings (sample_id, model_id, dim, dtype, l2_normed, vec, created_at)
             VALUES ('s1::Pack/a.wav', ?1, 1, 'f32', 1, X'00', 0)",
            params![crate::analysis::embedding::EMBEDDING_MODEL_ID],
        )
        .unwrap();

        let map = failed_samples_for_source_conn(&conn, &source.id).unwrap();
        assert_eq!(map.len(), 1);
        assert_eq!(
            map.get(&PathBuf::from("Pack/b.wav")).map(|s| s.as_str()),
            Some("Analysis failed")
        );
    }
}
