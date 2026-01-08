//! Claim selection helpers for analysis jobs.

use super::claim::{SourceClaimDb, claim_batch_size, refresh_sources};
use crate::egui_app::controller::library::analysis_jobs::db;
use crate::sample_sources::SourceId;
use std::collections::{HashSet, VecDeque};
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::time::Instant;

/// A selection outcome from the claim pool.
pub(crate) enum ClaimSelection {
    /// A job is ready to process.
    Job(db::ClaimedJob),
    /// No sources are available to claim from.
    NoSources,
    /// Sources exist, but no work is ready yet.
    Idle,
}

/// Tracks claim sources and selects the next job to work on.
pub(crate) struct ClaimSelector {
    sources: Vec<SourceClaimDb>,
    last_refresh: Instant,
    next_source: usize,
    local_queue: VecDeque<db::ClaimedJob>,
    claim_batch: usize,
    reset_done: Arc<Mutex<HashSet<PathBuf>>>,
    last_source_count: usize,
}

impl ClaimSelector {
    /// Creates a new claim selector for decoding workers.
    pub(crate) fn new(reset_done: Arc<Mutex<HashSet<PathBuf>>>) -> Self {
        Self {
            sources: Vec::new(),
            last_refresh: Instant::now() - super::claim::SOURCE_REFRESH_INTERVAL,
            next_source: 0,
            local_queue: VecDeque::new(),
            claim_batch: claim_batch_size(),
            reset_done,
            last_source_count: 0,
        }
    }

    #[cfg(test)]
    pub(crate) fn with_sources_for_tests(
        sources: Vec<SourceClaimDb>,
        claim_batch: usize,
        reset_done: Arc<Mutex<HashSet<PathBuf>>>,
    ) -> Self {
        Self {
            last_source_count: sources.len(),
            sources,
            last_refresh: Instant::now(),
            next_source: 0,
            local_queue: VecDeque::new(),
            claim_batch: claim_batch.max(1),
            reset_done,
        }
    }

    /// Selects the next job if one is available.
    pub(crate) fn select_next(
        &mut self,
        allowed_source_ids: Option<&HashSet<SourceId>>,
    ) -> ClaimSelection {
        self.refresh_sources_if_needed(allowed_source_ids);
        if self.sources.is_empty() {
            return ClaimSelection::NoSources;
        }
        if self.local_queue.is_empty() && !self.fill_local_queue() {
            return ClaimSelection::Idle;
        }
        self.pop_local()
    }

    fn refresh_sources_if_needed(&mut self, allowed_source_ids: Option<&HashSet<SourceId>>) {
        refresh_sources(
            &mut self.sources,
            &mut self.last_refresh,
            &self.reset_done,
            allowed_source_ids,
        );
        if self.sources.len() != self.last_source_count {
            self.last_source_count = self.sources.len();
            tracing::info!(
                "Analysis claim sources refreshed: {} source(s) available",
                self.last_source_count
            );
        }
    }

    fn fill_local_queue(&mut self) -> bool {
        let source_count = self.sources.len();
        for _ in 0..source_count {
            let idx = self.next_source % source_count;
            self.next_source = self.next_source.wrapping_add(1);
            let source = &mut self.sources[idx];
            let jobs = match db::claim_next_jobs(
                &mut source.conn,
                &source.source.root,
                self.claim_batch,
            ) {
                Ok(jobs) => jobs,
                Err(_) => continue,
            };
            if !jobs.is_empty() {
                self.local_queue.extend(jobs);
                return true;
            }
        }
        false
    }

    fn pop_local(&mut self) -> ClaimSelection {
        match self.local_queue.pop_front() {
            Some(job) => ClaimSelection::Job(job),
            None => ClaimSelection::Idle,
        }
    }
}
