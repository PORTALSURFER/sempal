use super::types::SampleMetadata;
use rusqlite::{Connection, TransactionBehavior, params};

pub(super) fn enqueue_jobs(
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
    let mut stmt = tx
        .prepare(
            "INSERT INTO analysis_jobs (sample_id, job_type, content_hash, status, attempts, created_at)
             VALUES (?1, ?2, ?3, 'pending', 0, ?4)
             ON CONFLICT(sample_id, job_type) DO UPDATE SET
                content_hash = excluded.content_hash,
                status = 'pending',
                attempts = 0,
                created_at = excluded.created_at,
                last_error = NULL",
        )
        .map_err(|err| format!("Failed to prepare analysis enqueue statement: {err}"))?;
    let mut inserted = 0usize;
    for (sample_id, content_hash) in jobs {
        let changed = stmt
            .execute(params![sample_id, job_type, content_hash, created_at])
            .map_err(|err| format!("Failed to enqueue analysis job: {err}"))?;
        inserted += changed;
    }
    Ok(inserted)
}

pub(super) fn upsert_samples(
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
    let mut stmt = tx
        .prepare(
            "INSERT INTO samples (sample_id, content_hash, size, mtime_ns, duration_seconds, sr_used, analysis_version)
             VALUES (?1, ?2, ?3, ?4, NULL, NULL, NULL)
             ON CONFLICT(sample_id) DO UPDATE SET
                content_hash = excluded.content_hash,
                size = excluded.size,
                mtime_ns = excluded.mtime_ns,
                duration_seconds = CASE
                    WHEN samples.content_hash != excluded.content_hash
                      OR samples.size != excluded.size
                      OR samples.mtime_ns != excluded.mtime_ns
                    THEN NULL
                    ELSE samples.duration_seconds
                END,
                sr_used = CASE
                    WHEN samples.content_hash != excluded.content_hash
                      OR samples.size != excluded.size
                      OR samples.mtime_ns != excluded.mtime_ns
                    THEN NULL
                    ELSE samples.sr_used
                END,
                analysis_version = CASE
                    WHEN samples.content_hash != excluded.content_hash
                      OR samples.size != excluded.size
                      OR samples.mtime_ns != excluded.mtime_ns
                    THEN NULL
                    ELSE samples.analysis_version
                END",
        )
        .map_err(|err| format!("Failed to prepare samples upsert statement: {err}"))?;
    let mut changed = 0usize;
    for sample in samples {
        changed += stmt
            .execute(params![
                &sample.sample_id,
                &sample.content_hash,
                sample.size as i64,
                sample.mtime_ns
            ])
            .map_err(|err| format!("Failed to upsert sample metadata: {err}"))?;
    }
    Ok(changed)
}
