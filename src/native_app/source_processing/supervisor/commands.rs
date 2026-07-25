use super::{
    Arc, BTreeMap, CommittedSourceDelta, ControlState, MAX_VISIBLE_PRIORITY_PATHS, SampleSource,
    SourceProcessingBudgetHandle, SourceProcessingSupervisor, register_source_for_scan_locked,
};

/// Lifecycle hints that may require source-audit admission, but are not proof that a complete
/// traversal is required. The durable readiness and watcher coverage gates decide that per source.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(in crate::native_app) enum SourceAuditLifecycleCause {
    Startup,
    WatcherReady,
    FocusRegained,
}

impl SourceAuditLifecycleCause {
    pub(super) fn reason(self) -> &'static str {
        match self {
            Self::Startup => "startup",
            Self::WatcherReady => "source_watcher_ready",
            Self::FocusRegained => "application_focus_regained",
        }
    }
}

impl SourceProcessingSupervisor {
    /// Admit a newly configured source before its first external scan starts.
    ///
    /// This deliberately only grows the configured set. Full replacement also
    /// retires removed lifecycle epochs and is owned by the configuration path.
    pub(in crate::native_app) fn register_source_for_scan(
        &self,
        source: SampleSource,
    ) -> Result<u64, String> {
        let _replacement = match self.shared.source_replacement.try_lock() {
            Ok(replacement) => replacement,
            Err(std::sync::TryLockError::Poisoned(poison)) => poison.into_inner(),
            Err(std::sync::TryLockError::WouldBlock) => {
                return Err("Configured sources are currently being replaced".to_string());
            }
        };
        register_source_for_scan_locked(self.shared.as_ref(), source)
    }

    pub(in crate::native_app) fn budget_handle(&self) -> SourceProcessingBudgetHandle {
        SourceProcessingBudgetHandle {
            shared: Arc::clone(&self.shared),
        }
    }

    pub(in crate::native_app) fn lifecycle_generations(&self) -> BTreeMap<String, u64> {
        self.shared.control().source_lifecycle_generations.clone()
    }

    /// Admit a cheap lifecycle health probe for every active source.
    ///
    /// Lifecycle transitions are intentionally only hints. The discovery gate decides whether a
    /// source needs a bounded manifest audit from durable revision, audit deadline, root identity,
    /// or watcher-history evidence; unchanged startup, watcher-ready, and focus transitions stay
    /// cheap no-ops.
    pub(in crate::native_app) fn request_lifecycle_audit_probe(
        &self,
        cause: SourceAuditLifecycleCause,
        deferred_source_ids: &[String],
    ) {
        let mut control = self.shared.control();
        if cause == SourceAuditLifecycleCause::WatcherReady {
            // The source watcher emits this only after it has either replayed the durable journal
            // or synchronously captured a per-source fallback-audit barrier.
            control.lifecycle_audits_deferred_until_watcher_ready = false;
            control.deferred_lifecycle_audit_sources = deferred_source_ids
                .iter()
                .filter(|source_id| control.source_is_active(source_id))
                .cloned()
                .collect();
        }
        control.mark_all_sources_for_safety_probe();
        control.notify(cause.reason());
        drop(control);
        self.shared.wake.notify_one();
    }

    /// Force the bounded manifest-audit fallback for one source whose durable watcher coverage
    /// has a proven gap. This is intentionally source-scoped; lifecycle hints must never turn an
    /// unrelated healthy source into a full traversal.
    pub(in crate::native_app) fn request_source_manifest_audit(
        &self,
        source_id: &str,
        reason: &'static str,
    ) {
        let mut control = self.shared.control();
        if !control.source_is_active(source_id) {
            return;
        }
        control
            .force_manifest_audit_sources
            .insert(source_id.to_string());
        control.deferred_lifecycle_audit_sources.remove(source_id);
        control.mark_source_dirty(source_id, reason);
        drop(control);
        self.shared.wake.notify_one();
    }

    pub(in crate::native_app) fn wake_source(&self, source_id: &str, reason: &'static str) {
        let mut control = self.shared.control();
        if !control.source_is_active(source_id) {
            return;
        }
        let bounded_delta_pending = control.pending_readiness_deltas.contains_key(source_id);
        if !bounded_delta_pending {
            control.cancel_source_work(source_id);
        }
        control.mark_source_dirty(source_id, reason);
        drop(control);
        if !bounded_delta_pending {
            self.shared
                .cancel_external_scans(|registration| registration.source_id == source_id);
            self.shared.budget_wake.notify_all();
        }
        self.shared.wake.notify_one();
    }

    /// Force complete source reconciliation when a bounded delta cannot describe all changes.
    pub(in crate::native_app) fn wake_source_for_full_reconciliation(
        &self,
        source_id: &str,
        reason: &'static str,
    ) {
        self.shared
            .control()
            .pending_readiness_deltas
            .remove(source_id);
        self.wake_source(source_id, reason);
    }

    pub(in crate::native_app) fn cancel_foreground_source_scan(
        &self,
        source_id: &str,
        reason: &'static str,
    ) {
        let mut control = self.shared.control();
        if !control.source_is_active(source_id) {
            return;
        }
        control.cancel_source_work(source_id);
        control.notify(reason);
        drop(control);
        self.shared
            .cancel_external_scans(|registration| registration.source_id == source_id);
        self.shared.budget_wake.notify_all();
        self.shared.wake.notify_one();
    }

    /// Reconcile a source without invalidating work that already owns its
    /// current lifecycle generation.
    ///
    /// UI projection refreshes and completed foreground scans can arrive well
    /// after the source database commit that scheduled them. Treating those
    /// delayed notifications as a new mutation would kill a valid long-running
    /// finalizer and immediately start the same work again.
    pub(in crate::native_app) fn request_source_processing(
        &self,
        source_id: &str,
        reason: &'static str,
    ) {
        let mut control = self.shared.control();
        if !control.source_is_active(source_id) {
            return;
        }
        control.mark_source_dirty(source_id, reason);
        drop(control);
        self.shared.wake.notify_one();
    }

    /// Publish the affected identity set from one authoritative committed manifest delta.
    pub(in crate::native_app) fn request_source_delta(
        &self,
        source_id: &str,
        delta: &CommittedSourceDelta,
        reason: &'static str,
    ) {
        if delta.is_empty() {
            return;
        }
        let mut control = self.shared.control();
        if !queue_source_delta(&mut control, source_id, delta, reason) {
            return;
        }
        drop(control);
        self.shared.wake.notify_one();
    }

    #[cfg(test)]
    pub(in crate::native_app) fn pending_source_delta_contains_identity_for_tests(
        &self,
        source_id: &str,
        identity: &str,
    ) -> bool {
        self.shared
            .control()
            .pending_readiness_deltas
            .get(source_id)
            .is_some_and(|delta| delta.scope_ids.contains(identity))
    }

    /// Requeue exact current feature, embedding, and similarity targets after
    /// an explicit user request.
    pub(in crate::native_app) fn request_source_reanalysis(
        &self,
        source_id: &str,
        reason: &'static str,
    ) {
        let mut control = self.shared.control();
        if !control.source_is_active(source_id) {
            return;
        }
        control.cancel_source_work(source_id);
        control
            .force_reanalysis_sources
            .insert(source_id.to_string());
        control.mark_source_dirty(source_id, reason);
        drop(control);
        self.shared
            .cancel_external_scans(|registration| registration.source_id == source_id);
        self.shared.budget_wake.notify_all();
        self.shared.wake.notify_one();
    }

    pub(in crate::native_app) fn finish_foreground_source_refresh(
        &self,
        source_id: &str,
        reason: &'static str,
    ) {
        let mut control = self.shared.control();
        if !control.source_is_active(source_id) {
            return;
        }
        let was_awaiting = control
            .awaiting_foreground_refresh_sources
            .remove(source_id);
        let bounded_delta_pending = control.pending_readiness_deltas.contains_key(source_id);
        if was_awaiting || bounded_delta_pending {
            control.mark_source_dirty(source_id, reason);
        }
        drop(control);
        if was_awaiting || bounded_delta_pending {
            self.shared.wake.notify_one();
        }
    }

    pub(in crate::native_app) fn set_selected_source(&self, source_id: Option<&str>) {
        let mut control = self.shared.control();
        let selected = source_id.map(str::to_string);
        if control.priority.selected_source != selected {
            control.priority.selected_source = selected;
            control.notify("selected_source_changed");
            self.shared.wake.notify_one();
        }
    }

    #[cfg(test)]
    pub(in crate::native_app) fn selected_source_priority_for_tests(&self) -> Option<String> {
        self.shared.control().priority.selected_source.clone()
    }

    pub(in crate::native_app) fn prioritize_path(
        &self,
        source_id: &str,
        relative_path: &str,
        immediate: bool,
    ) {
        let mut control = self.shared.control();
        let key = (source_id.to_string(), relative_path.to_string());
        let priorities = if immediate {
            control.priority.immediate_paths.clear();
            &mut control.priority.immediate_paths
        } else {
            &mut control.priority.visible_paths
        };
        if priorities.insert(key) {
            control.notify("interactive_path_priority");
            self.shared.wake.notify_one();
        }
    }

    pub(in crate::native_app) fn set_visible_paths<I>(&self, paths: I)
    where
        I: IntoIterator<Item = (String, String)>,
    {
        let visible_paths = paths.into_iter().take(MAX_VISIBLE_PRIORITY_PATHS).collect();
        let mut control = self.shared.control();
        if control.priority.visible_paths != visible_paths {
            control.priority.visible_paths = visible_paths;
            control.notify("visible_paths_changed");
            self.shared.wake.notify_one();
        }
    }

    pub(in crate::native_app) fn set_current_folder(&self, source_id: &str, relative_path: &str) {
        let mut control = self.shared.control();
        let current = Some((source_id.to_string(), relative_path.to_string()));
        if control.priority.current_folder != current {
            control.priority.current_folder = current;
            control.notify("current_folder_changed");
            self.shared.wake.notify_one();
        }
    }

    pub(in crate::native_app) fn set_playback_active(&self, active: bool) {
        let mut control = self.shared.control();
        if control.playback_active != active {
            control.playback_active = active;
            control.notify("playback_activity_changed");
            tracing::debug!(
                target: "wavecrate::source_processing",
                event = "source_processing.playback_activity_changed",
                active,
                "Playback activity changed without pausing source processing"
            );
            drop(control);
            self.shared.wake.notify_all();
        }
    }

    pub(in crate::native_app) fn set_foreground_activity(&self, active: bool) {
        let mut control = self.shared.control();
        if control.foreground_active == active {
            return;
        }
        control.foreground_active = active;
        control.notify("foreground_activity_changed");
        tracing::debug!(
            target: "wavecrate::source_processing",
            event = "source_processing.foreground_activity_changed",
            active,
            "Foreground loading activity changed without pausing source processing"
        );
        drop(control);
        self.shared.wake.notify_all();
    }
}

pub(super) fn queue_source_delta(
    control: &mut ControlState,
    source_id: &str,
    delta: &CommittedSourceDelta,
    reason: &'static str,
) -> bool {
    #[cfg(test)]
    if std::mem::take(&mut control.reject_next_delta_delivery) {
        return false;
    }
    if !control.source_is_active(source_id) {
        return false;
    }
    control
        .pending_readiness_deltas
        .entry(source_id.to_string())
        .or_default()
        .merge(delta, reason);
    control.mark_source_dirty(source_id, reason);
    true
}
