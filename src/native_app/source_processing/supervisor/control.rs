use super::{
    Arc, AtomicBool, BTreeMap, BTreeSet, Ordering, PendingReadinessDelta, PendingSourceRetirement,
    PriorityContext, SampleSource, source_storage_identity_matches,
};

pub(super) struct ControlState {
    pub(super) sources: BTreeMap<String, SampleSource>,
    pub(super) source_work_cancels: BTreeMap<String, Arc<AtomicBool>>,
    pub(super) source_lifecycle_generations: BTreeMap<String, u64>,
    pub(super) next_lifecycle_generation: u64,
    pub(super) dirty_sources: BTreeSet<String>,
    pub(super) safety_probe_sources: BTreeSet<String>,
    /// Initial lifecycle probes must wait until the source watcher has replayed its durable
    /// journal. A journal-gap request clears the safety-probe bit for its source and can still
    /// proceed because its watcher-side audit barrier was captured first.
    pub(super) lifecycle_audits_deferred_until_watcher_ready: bool,
    pub(super) deferred_lifecycle_audit_sources: BTreeSet<String>,
    pub(super) pending_readiness_deltas: BTreeMap<String, PendingReadinessDelta>,
    pub(super) awaiting_foreground_refresh_sources: BTreeSet<String>,
    pub(super) force_manifest_audit_sources: BTreeSet<String>,
    pub(super) force_reanalysis_sources: BTreeSet<String>,
    pub(super) quarantined_sources: BTreeSet<String>,
    pub(super) pending_retirements: BTreeMap<u64, PendingSourceRetirement>,
    pub(super) next_retirement_id: u64,
    pub(super) wake_generation: u64,
    pub(super) wake_reason: &'static str,
    pub(super) playback_active: bool,
    pub(super) foreground_active: bool,
    pub(super) shutdown: bool,
    pub(super) priority: PriorityContext,
    #[cfg(test)]
    pub(super) reject_next_delta_delivery: bool,
    #[cfg(test)]
    pub(super) reject_next_source_replacement: bool,
}

impl ControlState {
    pub(super) fn source_is_configured(&self, source_id: &str) -> bool {
        self.sources.contains_key(source_id) && !self.quarantined_sources.contains(source_id)
    }

    pub(super) fn source_is_active(&self, source_id: &str) -> bool {
        let Some(source) = self.sources.get(source_id) else {
            return false;
        };
        !self.quarantined_sources.contains(source_id)
            && !self
                .pending_retirements
                .values()
                .any(|retirement| source_storage_identity_matches(source, &retirement.source))
    }

    pub(super) fn notify(&mut self, reason: &'static str) {
        self.wake_generation = self.wake_generation.wrapping_add(1);
        self.wake_reason = reason;
    }

    pub(super) fn allocate_lifecycle_generation(&mut self) -> u64 {
        let generation = self.next_lifecycle_generation;
        self.next_lifecycle_generation = self.next_lifecycle_generation.wrapping_add(1).max(1);
        generation
    }

    pub(super) fn mark_source_dirty(&mut self, source_id: &str, reason: &'static str) {
        if self.source_is_active(source_id) {
            self.safety_probe_sources.remove(source_id);
            self.dirty_sources.insert(source_id.to_string());
            self.notify(reason);
        }
    }

    pub(super) fn mark_all_sources_dirty(&mut self, reason: &'static str) {
        self.safety_probe_sources.clear();
        self.dirty_sources.extend(
            self.sources
                .keys()
                .filter(|source_id| !self.quarantined_sources.contains(*source_id))
                .cloned(),
        );
        self.notify(reason);
    }

    pub(super) fn mark_all_sources_for_safety_probe(&mut self) {
        let source_ids = self
            .sources
            .keys()
            .filter(|source_id| !self.quarantined_sources.contains(*source_id))
            .cloned()
            .collect::<Vec<_>>();
        self.safety_probe_sources.extend(source_ids.iter().cloned());
        self.dirty_sources.extend(source_ids);
        self.notify("periodic_safety_sweep");
    }

    pub(super) fn cancel_source_work(&mut self, source_id: &str) {
        if let Some(cancel) = self.source_work_cancels.get_mut(source_id) {
            cancel.store(true, Ordering::Release);
            if !self.quarantined_sources.contains(source_id) {
                *cancel = Arc::new(AtomicBool::new(false));
            }
        }
    }

    pub(super) fn cancel_all_source_work(&mut self) {
        for cancel in self.source_work_cancels.values() {
            cancel.store(true, Ordering::Release);
        }
    }

    pub(super) fn reset_source_work_tokens(&mut self) {
        self.source_work_cancels = self
            .sources
            .keys()
            .map(|source_id| {
                let cancelled = self.quarantined_sources.contains(source_id);
                (source_id.clone(), Arc::new(AtomicBool::new(cancelled)))
            })
            .collect();
    }
}
