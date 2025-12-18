use super::db;
use rusqlite::{Connection, params};
use std::collections::HashMap;
use std::path::PathBuf;

pub(in crate::egui_app::controller) fn failed_samples_for_source(
    source_id: &crate::sample_sources::SourceId,
) -> Result<HashMap<PathBuf, String>, String> {
    let db_path = library_db_path()?;
    let conn = db::open_library_db(&db_path)?;
    failed_samples_for_source_conn(&conn, source_id)
}

fn failed_samples_for_source_conn(
    conn: &Connection,
    source_id: &crate::sample_sources::SourceId,
) -> Result<HashMap<PathBuf, String>, String> {
    let prefix = format!("{}::%", source_id.as_str());
    let mut stmt = conn
        .prepare(
            "SELECT sample_id, last_error
             FROM analysis_jobs
             WHERE status = 'failed' AND sample_id LIKE ?1
             ORDER BY sample_id ASC",
        )
        .map_err(|err| format!("Failed to query failed analysis jobs: {err}"))?;
    let mut out = HashMap::new();
    let rows = stmt
        .query_map(params![prefix], |row| {
            let sample_id: String = row.get(0)?;
            let last_error: Option<String> = row.get(1)?;
            Ok((sample_id, last_error))
        })
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

fn library_db_path() -> Result<PathBuf, String> {
    let dir = crate::app_dirs::app_root_dir().map_err(|err| err.to_string())?;
    Ok(dir.join(crate::sample_sources::library::LIBRARY_DB_FILE_NAME))
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
        let root = crate::app_dirs::app_root_dir().unwrap();
        let db_path = root.join(crate::sample_sources::library::LIBRARY_DB_FILE_NAME);

        let conn = Connection::open(&db_path).unwrap();
        conn.execute_batch(
            "CREATE TABLE analysis_jobs (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                sample_id TEXT NOT NULL,
                job_type TEXT NOT NULL,
                content_hash TEXT,
                status TEXT NOT NULL,
                attempts INTEGER NOT NULL DEFAULT 0,
                created_at INTEGER NOT NULL,
                last_error TEXT,
                UNIQUE(sample_id, job_type)
            );",
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

        let map = failed_samples_for_source_conn(
            &conn,
            &crate::sample_sources::SourceId::from_string("s1"),
        )
        .unwrap();
        assert_eq!(map.len(), 2);
        assert_eq!(map.get(&PathBuf::from("Pack/a.wav")).map(|s| s.as_str()), Some("boom"));
        assert_eq!(
            map.get(&PathBuf::from("Pack/b.wav")).map(|s| s.as_str()),
            Some("Analysis failed")
        );
    }
}
