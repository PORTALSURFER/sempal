use crate::egui_app::controller::analysis_jobs::db;
use std::collections::HashSet;
use std::path::PathBuf;
use std::time::{Duration, Instant};

pub(in crate::egui_app::controller::analysis_jobs) struct SourceClaimDb {
    pub(super) source: crate::sample_sources::SampleSource,
    pub(super) conn: rusqlite::Connection,
}

pub(in crate::egui_app::controller::analysis_jobs) const SOURCE_REFRESH_INTERVAL: Duration =
    Duration::from_secs(5);

pub(in crate::egui_app::controller::analysis_jobs) fn refresh_sources(
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
                tracing::debug!(
                    "Source DB open failed for {}: {err}",
                    source.root.display()
                );
                continue;
            }
        };
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
pub(in crate::egui_app::controller::analysis_jobs) fn worker_count_with_override(
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
pub(in crate::egui_app::controller::analysis_jobs) fn decode_worker_count_with_override(
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

pub(super) fn claim_batch_size() -> usize {
    if let Ok(value) = std::env::var("SEMPAL_ANALYSIS_CLAIM_BATCH") {
        if let Ok(parsed) = value.trim().parse::<usize>() {
            if parsed >= 1 {
                return parsed;
            }
        }
    }
    64
}

pub(in crate::egui_app::controller::analysis_jobs) fn decode_queue_target(
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
}
