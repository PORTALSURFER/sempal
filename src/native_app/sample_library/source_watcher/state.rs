use notify::Event;
use std::{
    collections::{HashMap, HashSet},
    path::PathBuf,
    time::{Duration, Instant},
};
use wavecrate::sample_sources::SampleSource;
use wavecrate_library::sample_sources::reconciliation::SourceAuditRequest;

use super::classification::path_is_source_refresh_candidate;
use super::debounce::{GuiSourceWatchEvent, PendingGuiSourceWatch};
use super::path_mapping::{source_for_path, source_relative_path};
use super::roots::{
    RootWatchUpdate, WatchedRootIdentities, observed_watcher_path_state, root_identity_is_current,
    source_root_is_available,
};
use crate::native_app::sample_library::committed_file_mutations::{
    CommittedWatcherEcho, CommittedWatcherPathState, RevisionFirstCursor,
};
use crate::native_app::source_processing::SourceProcessingRegistration;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum AuditRequestQueueOutcome {
    Queued,
    Coalesced,
    QueuedAfterDroppingFallback,
    FallbackCouldNotFit,
    MarkerCouldNotFit,
}

struct PendingSourceAuditRequest {
    request: SourceAuditRequest,
    marker_backed: bool,
}

/// Watcher-owned FIFO transport for exact source-audit requests.
///
/// Requests with one source/root/generation identity are covered into their original queue entry.
/// The capacity is supplied by the admission owner and includes ordinary plus emergency retained
/// uncertainty capacity; it is intentionally independent from the raw capture-context limit.
pub(super) struct PendingSourceAuditRequests {
    entries: Vec<PendingSourceAuditRequest>,
    capacity: usize,
    conservative_recovery_latched: bool,
}

impl Default for PendingSourceAuditRequests {
    fn default() -> Self {
        Self {
            entries: Vec::new(),
            capacity: 0,
            conservative_recovery_latched: false,
        }
    }
}

impl PendingSourceAuditRequests {
    pub(super) fn set_capacity(&mut self, capacity: usize) {
        debug_assert!(
            capacity >= self.entries.len(),
            "audit request capacity cannot displace retained queue entries"
        );
        self.capacity = capacity;
    }

    pub(super) fn insert(
        &mut self,
        request: SourceAuditRequest,
        marker_backed: bool,
    ) -> AuditRequestQueueOutcome {
        if let Some(entry) = self
            .entries
            .iter_mut()
            .find(|entry| entry.request.identity() == request.identity())
        {
            entry.request = entry
                .request
                .covering(&request)
                .expect("queue identity was checked before covering requests");
            entry.marker_backed |= marker_backed;
            return AuditRequestQueueOutcome::Coalesced;
        }

        if self.entries.len() < self.capacity {
            self.entries.push(PendingSourceAuditRequest {
                request,
                marker_backed,
            });
            return AuditRequestQueueOutcome::Queued;
        }

        if marker_backed {
            if let Some(index) = self.entries.iter().position(|entry| !entry.marker_backed) {
                self.entries.remove(index);
                self.conservative_recovery_latched = true;
                self.entries.push(PendingSourceAuditRequest {
                    request,
                    marker_backed,
                });
                return AuditRequestQueueOutcome::QueuedAfterDroppingFallback;
            }
            self.conservative_recovery_latched = true;
            return AuditRequestQueueOutcome::MarkerCouldNotFit;
        }

        self.conservative_recovery_latched = true;
        AuditRequestQueueOutcome::FallbackCouldNotFit
    }

    pub(super) fn purge_non_matching<F>(&mut self, mut matches: F) -> Vec<SourceAuditRequest>
    where
        F: FnMut(&SourceAuditRequest) -> bool,
    {
        let mut displaced = Vec::new();
        self.entries.retain(|entry| {
            let keep = matches(&entry.request);
            if !keep {
                displaced.push(entry.request.clone());
            }
            keep
        });
        displaced
    }

    #[cfg(test)]
    pub(super) fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    #[cfg(test)]
    pub(super) fn clear(&mut self) {
        self.entries.clear();
        self.conservative_recovery_latched = false;
    }

    pub(super) fn conservative_recovery_latched(&self) -> bool {
        self.conservative_recovery_latched
    }

    pub(super) fn clear_conservative_recovery_latch(&mut self) {
        self.conservative_recovery_latched = false;
    }

    #[cfg(test)]
    pub(super) fn len(&self) -> usize {
        self.entries.len()
    }

    pub(super) fn front(&self) -> Option<&SourceAuditRequest> {
        self.entries.first().map(|entry| &entry.request)
    }

    pub(super) fn pop_front(&mut self) -> Option<SourceAuditRequest> {
        (!self.entries.is_empty()).then(|| self.entries.remove(0).request)
    }
}

#[derive(Default)]
pub(super) struct GuiSourceWatchState {
    pub(super) watched_roots: WatchedRootIdentities,
    pub(super) registrations: Vec<SourceProcessingRegistration>,
    pub(super) sources: Vec<SampleSource>,
    pub(super) pending: HashMap<String, PendingGuiSourceWatch>,
    pub(super) acknowledged_paths: HashMap<(String, PathBuf), (CommittedWatcherPathState, Instant)>,
    pub(super) pending_audit_requests: PendingSourceAuditRequests,
}

impl GuiSourceWatchState {
    #[cfg(test)]
    pub(super) fn set_sources(&mut self, sources: Vec<SampleSource>) {
        self.registrations.clear();
        self.set_source_list(sources);
    }

    pub(super) fn set_registrations(&mut self, registrations: Vec<SourceProcessingRegistration>) {
        let sources = registrations
            .iter()
            .map(|registration| registration.source.clone())
            .collect();
        self.registrations = registrations;
        self.set_source_list(sources);
    }

    pub(super) fn registration_for_source(
        &self,
        source_id: &str,
    ) -> Option<&SourceProcessingRegistration> {
        self.registrations
            .iter()
            .find(|registration| registration.source.id.as_str() == source_id)
    }

    fn set_source_list(&mut self, sources: Vec<SampleSource>) {
        self.sources = sources;
        let allowed = self
            .sources
            .iter()
            .map(|source| source.id.as_str().to_string())
            .collect::<HashSet<_>>();
        self.pending
            .retain(|source_id, _| allowed.contains(source_id));
        self.acknowledged_paths
            .retain(|(source_id, _), _| allowed.contains(source_id));
    }

    pub(super) fn set_audit_request_capacity(&mut self, capacity: usize) {
        self.pending_audit_requests.set_capacity(capacity);
    }

    pub(super) fn enqueue_audit_request(
        &mut self,
        request: SourceAuditRequest,
        marker_backed: bool,
    ) -> AuditRequestQueueOutcome {
        self.pending_audit_requests.insert(request, marker_backed)
    }

    pub(super) fn purge_non_matching_audit_requests<F>(
        &mut self,
        matches: F,
    ) -> Vec<SourceAuditRequest>
    where
        F: FnMut(&SourceAuditRequest) -> bool,
    {
        self.pending_audit_requests.purge_non_matching(matches)
    }

    /// Replace displaced typed work with the existing bounded conservative source recovery.
    ///
    /// A request whose source is still configured widens only that source. If its source was
    /// removed, the old identity cannot be emitted or acknowledged, so the remaining configured
    /// sources receive the existing all-source conservative recovery instead.
    pub(super) fn recover_displaced_audit_requests(
        &mut self,
        requests: &[SourceAuditRequest],
        now: Instant,
    ) {
        let mut recover_all = false;
        for request in requests {
            if self
                .sources
                .iter()
                .any(|source| source.id == *request.source_id())
            {
                self.mark_source_overflowed(request.source_id().as_str(), now);
            } else {
                recover_all = true;
            }
        }
        if recover_all {
            self.mark_all_overflowed(now);
        }
    }

    pub(super) fn apply_root_watch_update(
        &mut self,
        update: RootWatchUpdate,
        now: Instant,
        reconcile_changed_roots: bool,
    ) -> (bool, bool) {
        if reconcile_changed_roots {
            for root in update.changed_roots {
                let affected = self
                    .sources
                    .iter()
                    .filter(|source| source.root == root)
                    .map(|source| source.id.as_str().to_string())
                    .collect::<Vec<_>>();
                for source_id in affected {
                    self.mark_source_overflowed(&source_id, now);
                }
            }
        }
        (update.has_unavailable_roots, update.watch_failed)
    }

    pub(super) fn reset_watches(&mut self, now: Instant) {
        self.watched_roots.clear();
        self.mark_all_overflowed(now);
    }

    pub(super) fn clear_watches(&mut self) {
        self.watched_roots.clear();
    }

    pub(super) fn mark_roots_overflowed(&mut self, roots: &[PathBuf], now: Instant) {
        let roots = roots.iter().collect::<HashSet<_>>();
        let source_ids = self
            .sources
            .iter()
            .filter(|source| roots.contains(&source.root))
            .map(|source| source.id.as_str().to_string())
            .collect::<Vec<_>>();
        for source_id in source_ids {
            self.mark_source_overflowed(&source_id, now);
        }
    }

    pub(super) fn mark_all_overflowed(&mut self, now: Instant) {
        let source_ids = self
            .sources
            .iter()
            .map(|source| source.id.as_str().to_string())
            .collect::<Vec<_>>();
        for source_id in source_ids {
            self.mark_source_overflowed(&source_id, now);
        }
    }

    pub(super) fn mark_source_overflowed(&mut self, source_id: &str, now: Instant) {
        self.pending
            .entry(source_id.to_string())
            .and_modify(|pending| {
                pending.last_event = now;
                pending.overflowed = true;
                pending.paths.clear();
            })
            .or_insert_with(|| PendingGuiSourceWatch::new(now, None));
    }

    pub(super) fn collect_event(&mut self, event: &Event, now: Instant) -> bool {
        let mut root_invalidated = false;
        self.acknowledged_paths
            .retain(|_, (_, deadline)| *deadline > now);
        for path in &event.paths {
            if !path_is_source_refresh_candidate(path, event.kind) {
                continue;
            }
            if let Some(source) = source_for_path(&self.sources, path) {
                let relative = source_relative_path(source, path);
                let source_id = source.id.as_str().to_string();
                let source_root = source.root.clone();
                let matching_acknowledgement = relative.as_ref().is_some_and(|relative| {
                    self.acknowledged_paths
                        .remove(&(source_id.clone(), relative.clone()))
                        .is_some_and(|(expected, _)| {
                            observed_watcher_path_state(path).as_ref() == Some(&expected)
                        })
                });
                if matching_acknowledgement {
                    tracing::debug!(
                        source_id,
                        path = %path.display(),
                        kind = ?event.kind,
                        "Suppressing watcher echo for committed Wavecrate mutation"
                    );
                    continue;
                }
                // FSEvents may coalesce writes to `.wavecrate.db`, its WAL, or related source
                // metadata into an event for the watched root itself. Re-scanning that live root
                // would write the database again and create a self-sustaining watcher loop.
                //
                // A known changed identity proves that this path now names a replacement object.
                // When identity is unreadable, destructive/name root events fail toward a full
                // reconciliation while metadata-like echoes remain suppressed; the bounded
                // identity-recovery audit covers replacements that only produce ambiguous events.
                if path == &source_root {
                    let available = source_root_is_available(source);
                    let requires_reconciliation = root_event_can_replace_identity(event.kind)
                        || match root_identity_is_current(&self.watched_roots, &source_root) {
                            Some(current) => !current,
                            None if !available => true,
                            None => false,
                        };
                    if requires_reconciliation {
                        tracing::warn!(
                            source_id,
                            kind = ?event.kind,
                            "Source root event invalidated the active watcher"
                        );
                        self.mark_source_overflowed(&source_id, now);
                        root_invalidated = true;
                        continue;
                    }
                    tracing::debug!(
                        source_id,
                        kind = ?event.kind,
                        "Ignoring coalesced live-root watcher event"
                    );
                    continue;
                }
                self.pending
                    .entry(source_id)
                    .and_modify(|pending| {
                        pending.last_event = now;
                        pending.add_path(relative.clone());
                    })
                    .or_insert_with(|| PendingGuiSourceWatch::new(now, relative));
            }
        }
        root_invalidated
    }

    pub(super) fn acknowledge_committed_paths(
        &mut self,
        source_id: &str,
        echoes: &[CommittedWatcherEcho],
        cursor: RevisionFirstCursor,
        now: Instant,
    ) {
        let deadline = now + super::SOURCE_CHANGE_DEBOUNCE.saturating_mul(2);
        let mut paths_with_pending_events = HashSet::new();
        let mut source_overflowed = false;
        let clear_pending = if let Some(pending) = self.pending.get_mut(source_id)
            && !pending.overflowed
        {
            let source_root = self
                .sources
                .iter()
                .find(|source| source.id.as_str() == source_id)
                .map(|source| source.root.as_path());
            for echo in echoes {
                if pending.paths.contains(&echo.relative_path) {
                    paths_with_pending_events.insert(echo.relative_path.clone());
                    if source_root
                        .map(|root| root.join(&echo.relative_path))
                        .as_deref()
                        .and_then(observed_watcher_path_state)
                        .as_ref()
                        == Some(&echo.expected_state)
                    {
                        pending.paths.remove(&echo.relative_path);
                    }
                }
            }
            pending.paths.is_empty()
        } else {
            source_overflowed = self
                .pending
                .get(source_id)
                .is_some_and(|pending| pending.overflowed);
            false
        };
        if clear_pending {
            self.pending.remove(source_id);
        }
        for echo in echoes {
            if !source_overflowed && !paths_with_pending_events.contains(&echo.relative_path) {
                self.acknowledged_paths.insert(
                    (source_id.to_string(), echo.relative_path.clone()),
                    (echo.expected_state.clone(), deadline),
                );
            }
        }
        tracing::debug!(
            source_id,
            revision = cursor.revision.as_raw(),
            correlation_id = cursor.correlation.as_raw(),
            path_count = echoes.len(),
            "Acknowledged committed mutation paths in source watcher"
        );
    }

    pub(super) fn drain_ready_sources(
        &mut self,
        now: Instant,
        debounce: Duration,
    ) -> Vec<GuiSourceWatchEvent> {
        let ready = self
            .pending
            .iter()
            .filter(|&(_source_id, pending)| {
                now.saturating_duration_since(pending.last_event) >= debounce
            })
            .filter_map(|(source_id, pending)| {
                let source = self
                    .sources
                    .iter()
                    .find(|source| source.id.as_str() == source_id)?;
                Some(GuiSourceWatchEvent {
                    source_id: source_id.clone(),
                    paths: pending.paths.iter().cloned().collect(),
                    overflowed: pending.overflowed,
                    source_root_available: source_root_is_available(source),
                })
            })
            .collect::<Vec<_>>();
        for event in &ready {
            self.pending.remove(&event.source_id);
        }
        ready
    }
}

fn root_event_can_replace_identity(kind: notify::EventKind) -> bool {
    matches!(
        kind,
        notify::EventKind::Create(_)
            | notify::EventKind::Remove(_)
            | notify::EventKind::Modify(notify::event::ModifyKind::Name(_))
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::native_app::source_processing::SourceProcessingRegistration;
    use wavecrate::sample_sources::{SampleSource, SourceId};

    #[test]
    fn registration_transport_replaces_descriptor_and_generation_as_one_pair() {
        let first = SampleSource::new_with_id(
            SourceId::from_string("registration-state"),
            PathBuf::from("/first-root"),
        );
        let replacement =
            SampleSource::new_with_id(first.id.clone(), PathBuf::from("/replacement-root"));
        let mut state = GuiSourceWatchState::default();

        state.set_registrations(vec![SourceProcessingRegistration::new(first, 7)]);
        assert_eq!(
            state
                .registration_for_source("registration-state")
                .expect("initial registration")
                .lifecycle_generation,
            7
        );

        state.set_registrations(vec![SourceProcessingRegistration::new(replacement, 8)]);
        let registration = state
            .registration_for_source("registration-state")
            .expect("replacement registration");
        assert_eq!(registration.lifecycle_generation, 8);
        assert_eq!(registration.source.root, PathBuf::from("/replacement-root"));
        assert_eq!(state.sources.len(), 1);

        state.set_registrations(Vec::new());
        assert!(
            state
                .registration_for_source("registration-state")
                .is_none()
        );
        assert!(state.sources.is_empty());
    }

    #[test]
    fn source_id_overflow_does_not_widen_shared_root_source() {
        let shared_root = PathBuf::from("/shared-source-root");
        let first = SampleSource::new_with_id(
            SourceId::from_string("shared-root-first"),
            shared_root.clone(),
        );
        let second =
            SampleSource::new_with_id(SourceId::from_string("shared-root-second"), shared_root);
        let mut state = GuiSourceWatchState {
            sources: vec![first.clone(), second.clone()],
            ..Default::default()
        };

        state.mark_source_overflowed(first.id.as_str(), Instant::now());

        assert!(
            state
                .pending
                .get(first.id.as_str())
                .is_some_and(|pending| { pending.overflowed })
        );
        assert!(!state.pending.contains_key(second.id.as_str()));
    }
}
