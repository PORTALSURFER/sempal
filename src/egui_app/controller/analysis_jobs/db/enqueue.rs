use super::types::SampleMetadata;
use rusqlite::{Connection, TransactionBehavior};
use rusqlite::types::Value;
use rusqlite::params_from_iter;

pub(in crate::egui_app::controller::analysis_jobs) fn enqueue_jobs(
    conn: &mut Connection,
    jobs: &[(String, String)],
    job_type: &str,
    created_at: i64,
) -> Result<usize, String> {
    if jobs.is_empty() {
        return Ok(0);
    }
    let tx = conn
        .transaction_with_behavior(TransactionBehavior::Immediate)
        .map_err(|err| format!("Failed to start analysis enqueue transaction: {err}"))?;
    let inserted = enqueue_jobs_tx(&tx, jobs, job_type, created_at)?;
    tx.commit()
        .map_err(|err| format!("Failed to commit analysis enqueue transaction: {err}"))?;
    Ok(inserted)
}

fn enqueue_jobs_tx(
    tx: &rusqlite::Transaction<'_>,
    jobs: &[(String, String)],
    job_type: &str,
    created_at: i64,
) -> Result<usize, String> {
    let mut inserted = 0usize;
    const BATCH_SIZE: usize = 200;
    for chunk in jobs.chunks(BATCH_SIZE) {
        let mut sql = String::from(
            "INSERT INTO analysis_jobs (sample_id, job_type, content_hash, status, attempts, created_at) VALUES ",
        );
        let mut params: Vec<Value> = Vec::with_capacity(chunk.len() * 4);
        for (idx, (sample_id, content_hash)) in chunk.iter().enumerate() {
            if idx > 0 {
                sql.push_str(", ");
            }
            let base = idx * 4;
            sql.push_str(&format!(
                "(?{}, ?{}, ?{}, 'pending', 0, ?{})",
                base + 1,
                base + 2,
                base + 3,
                base + 4
            ));
            params.push(Value::from(sample_id.clone()));
            params.push(Value::from(job_type.to_string()));
            params.push(Value::from(content_hash.clone()));
            params.push(Value::from(created_at));
        }
        sql.push_str(
            " ON CONFLICT(sample_id, job_type) DO UPDATE SET
                content_hash = excluded.content_hash,
                status = 'pending',
                attempts = 0,
                created_at = excluded.created_at,
                last_error = NULL",
        );
        let changed = tx
            .execute(&sql, params_from_iter(params))
            .map_err(|err| format!("Failed to enqueue analysis jobs: {err}"))?;
        inserted += changed as usize;
    }
    Ok(inserted)
}

pub(in crate::egui_app::controller::analysis_jobs) fn upsert_samples(
    conn: &mut Connection,
    samples: &[SampleMetadata],
) -> Result<usize, String> {
    if samples.is_empty() {
        return Ok(0);
    }
    let tx = conn
        .transaction_with_behavior(TransactionBehavior::Immediate)
        .map_err(|err| format!("Failed to start samples upsert transaction: {err}"))?;
    let changed = upsert_samples_tx(&tx, samples)?;
    tx.commit()
        .map_err(|err| format!("Failed to commit samples upsert transaction: {err}"))?;
    Ok(changed)
}

fn upsert_samples_tx(
    tx: &rusqlite::Transaction<'_>,
    samples: &[SampleMetadata],
) -> Result<usize, String> {
    let mut changed = 0usize;
    const BATCH_SIZE: usize = 200;
    for chunk in samples.chunks(BATCH_SIZE) {
        let mut sql = String::from(
            "INSERT INTO samples (sample_id, content_hash, size, mtime_ns, duration_seconds, sr_used, analysis_version) VALUES ",
        );
        let mut params: Vec<Value> = Vec::with_capacity(chunk.len() * 4);
        for (idx, sample) in chunk.iter().enumerate() {
            if idx > 0 {
                sql.push_str(", ");
            }
            let base = idx * 4;
            sql.push_str(&format!(
                "(?{}, ?{}, ?{}, ?{}, NULL, NULL, NULL)",
                base + 1,
                base + 2,
                base + 3,
                base + 4
            ));
            params.push(Value::from(sample.sample_id.clone()));
            params.push(Value::from(sample.content_hash.clone()));
            params.push(Value::from(sample.size as i64));
            params.push(Value::from(sample.mtime_ns));
        }
        sql.push_str(
            " ON CONFLICT(sample_id) DO UPDATE SET
                content_hash = excluded.content_hash,
                size = excluded.size,
                mtime_ns = excluded.mtime_ns,
                duration_seconds = CASE
                    WHEN samples.content_hash != excluded.content_hash
                    THEN NULL
                    ELSE samples.duration_seconds
                END,
                sr_used = CASE
                    WHEN samples.content_hash != excluded.content_hash
                    THEN NULL
                    ELSE samples.sr_used
                END,
                analysis_version = CASE
                    WHEN samples.content_hash != excluded.content_hash
                    THEN NULL
                    ELSE samples.analysis_version
                END",
        );
        let batch_changed = tx
            .execute(&sql, params_from_iter(params))
            .map_err(|err| format!("Failed to upsert sample metadata: {err}"))?;
        changed += batch_changed as usize;
    }
    Ok(changed)
}
