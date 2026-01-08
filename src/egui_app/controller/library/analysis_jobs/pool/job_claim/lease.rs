//! Claim lease helpers for analysis jobs.

use crate::egui_app::controller::library::analysis_jobs::db;
use crate::sample_sources::SourceId;
use std::collections::HashSet;

/// Returns true when the job is allowed for the current source filter.
pub(crate) fn job_allowed(
    job: &db::ClaimedJob,
    allowed_source_ids: Option<&HashSet<SourceId>>,
) -> bool {
    let Some(allowed) = allowed_source_ids else {
        return true;
    };
    let Ok((source_id, _)) = db::parse_sample_id(&job.sample_id) else {
        return true;
    };
    let source_id = SourceId::from_string(source_id);
    allowed.contains(&source_id)
}

/// Releases a claim back to pending.
pub(crate) fn release_claim(conn: &rusqlite::Connection, job_id: i64) {
    let _ = db::mark_pending(conn, job_id);
}
