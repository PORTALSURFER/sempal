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

#[derive(Default)]
pub(super) struct GuiSourceWatchState {
    pub(super) watched_roots: WatchedRootIdentities,
    pub(super) sources: Vec<SampleSource>,
    pub(super) pending: HashMap<String, PendingGuiSourceWatch>,
    pub(super) acknowledged_paths: HashMap<(String, PathBuf), (CommittedWatcherPathState, Instant)>,
    pub(super) pending_audit_requests: Vec<SourceAuditRequest>,
}

impl GuiSourceWatchState {
    pub(super) fn set_sources(&mut self, sources: Vec<SampleSource>) {
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
        self.pending_audit_requests
            .retain(|request| allowed.contains(request.source_id().as_str()));
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

    fn mark_source_overflowed(&mut self, source_id: &str, now: Instant) {
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
