use crate::egui_app::controller::library::analysis_jobs::db;
use crate::egui_app::controller::library::analysis_jobs::stale_running_job_seconds;
use std::collections::HashSet;
use std::path::PathBuf;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

pub(crate) struct SourceClaimDb {
    pub(crate) source: crate::sample_sources::SampleSource,
    pub(crate) conn: rusqlite::Connection,
}

pub(crate) const SOURCE_REFRESH_INTERVAL: Duration =
    Duration::from_secs(5);

pub(crate) fn refresh_sources(
    sources: &mut Vec<SourceClaimDb>,
    last_refresh: &mut Instant,
    reset_done: &std::sync::Arc<std::sync::Mutex<HashSet<PathBuf>>>,
    allowed_source_ids: Option<&HashSet<crate::sample_sources::SourceId>>,
) {
    if last_refresh.elapsed() < SOURCE_REFRESH_INTERVAL {
        return;
    }
    *last_refresh = Instant::now();
    let Ok(state) = crate::sample_sources::library::load() else {
        return;
    };
    let mut next = Vec::new();
    for source in state.sources {
        if !source.root.is_dir() {
            continue;
        }
        if let Some(allowed) = allowed_source_ids {
            if !allowed.contains(&source.id) {
                continue;
            }
        }
        let conn = match db::open_source_db(&source.root) {
            Ok(conn) => conn,
            Err(err) => {
                tracing::debug!("Source DB open failed for {}: {err}", source.root.display());
                continue;
            }
        };
        let stale_before = now_epoch_seconds().saturating_sub(stale_running_job_seconds());
        let _ = db::fail_stale_running_jobs(&conn, stale_before);
        let should_reset = match reset_done.lock() {
            Ok(mut guard) => guard.insert(source.root.clone()),
            Err(mut guard) => guard.get_mut().insert(source.root.clone()),
        };
        if should_reset {
            let _ = db::prune_jobs_for_missing_sources(&conn);
            let _ = db::reset_running_to_pending(&conn);
        }
        next.push(SourceClaimDb { source, conn });
    }
    *sources = next;
}

#[cfg_attr(test, allow(dead_code))]
pub(crate) fn worker_count_with_override(
    override_count: u32,
) -> usize {
    if override_count >= 1 {
        return override_count as usize;
    }
    if let Ok(value) = std::env::var("SEMPAL_ANALYSIS_WORKERS") {
        if let Ok(parsed) = value.trim().parse::<usize>() {
            if parsed >= 1 {
                return parsed;
            }
        }
    }
    std::thread::available_parallelism()
        .map(|n| n.get())
        .unwrap_or(1)
        .saturating_sub(2)
        .max(1)
}

#[cfg_attr(test, allow(dead_code))]
pub(crate) fn decode_worker_count_with_override(
    worker_count: usize,
    override_count: u32,
) -> usize {
    if override_count >= 1 {
        return override_count as usize;
    }
    if let Ok(value) = std::env::var("SEMPAL_DECODE_WORKERS") {
        if let Ok(parsed) = value.trim().parse::<usize>() {
            if parsed >= 1 {
                return parsed;
            }
        }
    }
    let max_workers = std::thread::available_parallelism()
        .map(|n| n.get())
        .unwrap_or(worker_count.max(1));
    std::cmp::min(worker_count.saturating_mul(2).max(2), max_workers)
}

pub(crate) fn claim_batch_size() -> usize {
    if let Ok(value) = std::env::var("SEMPAL_ANALYSIS_CLAIM_BATCH") {
        if let Ok(parsed) = value.trim().parse::<usize>() {
            if parsed >= 1 {
                return parsed;
            }
        }
    }
    64
}

#[cfg_attr(test, allow(dead_code))]
pub(crate) fn decode_queue_target(
    embedding_batch_max: usize,
    worker_count: usize,
) -> usize {
    if let Ok(value) = std::env::var("SEMPAL_DECODE_QUEUE_TARGET") {
        if let Ok(parsed) = value.trim().parse::<usize>() {
            if parsed >= 1 {
                return parsed;
            }
        }
    }
    (embedding_batch_max.saturating_mul(worker_count)).max(4)
}

fn now_epoch_seconds() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_else(|_| Duration::from_secs(0))
        .as_secs() as i64
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn claim_batch_size_respects_env_override() {
        unsafe {
            std::env::set_var("SEMPAL_ANALYSIS_CLAIM_BATCH", "7");
        }
        let value = claim_batch_size();
        unsafe {
            std::env::remove_var("SEMPAL_ANALYSIS_CLAIM_BATCH");
        }
        assert_eq!(value, 7);
    }

    #[test]
    fn claim_batch_size_defaults_when_invalid() {
        unsafe {
            std::env::set_var("SEMPAL_ANALYSIS_CLAIM_BATCH", "0");
        }
        let value = claim_batch_size();
        unsafe {
            std::env::remove_var("SEMPAL_ANALYSIS_CLAIM_BATCH");
        }
        assert_eq!(value, 64);
    }
}
