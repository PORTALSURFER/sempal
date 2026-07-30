use super::{
    AcceptedManifestRevision, Arc, AtomicBool, BTreeMap, BTreeSet, CommittedSourceDelta, Ordering,
    PendingProjectionFence, PendingReadinessDelta, PendingReadinessDeltaMerge,
    PendingSourceRetirement, PriorityContext, SampleSource, source_storage_identity_matches,
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
    pub(super) pending_projection_fences: BTreeMap<String, PendingProjectionFence>,
    pub(super) accepted_manifest_revisions: BTreeMap<String, AcceptedManifestRevision>,
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

    pub(super) fn queue_source_delta(
        &mut self,
        source_id: &str,
        lifecycle_generation: u64,
        delta: &CommittedSourceDelta,
        reason: &'static str,
    ) -> SourceDeltaQueueResult {
        #[cfg(test)]
        if std::mem::take(&mut self.reject_next_delta_delivery) {
            return SourceDeltaQueueResult::Rejected;
        }
        if !self.source_is_active(source_id)
            || self.source_lifecycle_generations.get(source_id) != Some(&lifecycle_generation)
        {
            return SourceDeltaQueueResult::Rejected;
        }
        if let Some(fence) = self.accepted_manifest_revisions.get(source_id)
            && fence.lifecycle_generation == lifecycle_generation
            && (delta.revision <= fence.revision
                || fence
                    .recovery_floor
                    .is_some_and(|floor| delta.revision <= floor))
        {
            return SourceDeltaQueueResult::Ignored;
        }
        if self
            .accepted_manifest_revisions
            .get(source_id)
            .is_some_and(|fence| {
                fence.lifecycle_generation == lifecycle_generation
                    && fence.recovery_floor.is_none()
                    && delta.revision > fence.revision.saturating_add(1)
            })
        {
            self.pending_readiness_deltas.remove(source_id);
            let fence = self
                .accepted_manifest_revisions
                .get_mut(source_id)
                .expect("accepted revision was present");
            fence.recovery_floor = Some(delta.revision);
            self.cancel_source_work(source_id);
            self.mark_source_dirty(source_id, "source_delta_revision_gap");
            return SourceDeltaQueueResult::Fallback;
        }
        if self
            .accepted_manifest_revisions
            .get(source_id)
            .is_some_and(|fence| {
                fence.lifecycle_generation == lifecycle_generation && fence.recovery_floor.is_some()
            })
        {
            self.pending_readiness_deltas.remove(source_id);
            let fence = self
                .accepted_manifest_revisions
                .get_mut(source_id)
                .expect("recovery floor was present");
            fence.recovery_floor =
                Some(fence.recovery_floor.unwrap_or_default().max(delta.revision));
            self.cancel_source_work(source_id);
            self.mark_source_dirty(source_id, "source_delta_recovery_pending");
            return SourceDeltaQueueResult::Fallback;
        }
        let merge = self
            .pending_readiness_deltas
            .entry(source_id.to_string())
            .or_default()
            .merge(delta, reason);
        if merge == PendingReadinessDeltaMerge::RevisionGap {
            self.pending_readiness_deltas.remove(source_id);
            self.cancel_source_work(source_id);
            let fence = self
                .accepted_manifest_revisions
                .entry(source_id.to_string())
                .or_insert(AcceptedManifestRevision {
                    lifecycle_generation,
                    ..AcceptedManifestRevision::default()
                });
            fence.lifecycle_generation = lifecycle_generation;
            fence.recovery_floor =
                Some(fence.recovery_floor.unwrap_or_default().max(delta.revision));
            self.mark_source_dirty(source_id, "source_delta_revision_gap");
            return SourceDeltaQueueResult::Fallback;
        }
        if merge == PendingReadinessDeltaMerge::Stale {
            return SourceDeltaQueueResult::Ignored;
        }
        self.mark_source_dirty(source_id, reason);
        SourceDeltaQueueResult::Queued
    }

    pub(super) fn accept_reconciled_manifest_revision(
        &mut self,
        source_id: &str,
        lifecycle_generation: u64,
        revision: u64,
    ) {
        if !self.source_is_active(source_id)
            || self.source_lifecycle_generations.get(source_id) != Some(&lifecycle_generation)
        {
            return;
        }
        let fence = self
            .accepted_manifest_revisions
            .entry(source_id.to_string())
            .or_insert(AcceptedManifestRevision {
                lifecycle_generation,
                ..AcceptedManifestRevision::default()
            });
        if fence.lifecycle_generation != lifecycle_generation {
            *fence = AcceptedManifestRevision {
                lifecycle_generation,
                revision,
                recovery_floor: None,
            };
            return;
        }
        fence.revision = fence.revision.max(revision);
        fence.recovery_floor = fence.recovery_floor.filter(|floor| *floor > fence.revision);
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

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum SourceDeltaQueueResult {
    Queued,
    Ignored,
    Fallback,
    Rejected,
}
