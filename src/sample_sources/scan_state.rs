use std::collections::HashSet;

use super::SourceId;

/// Tracks scan lifecycles to avoid overlapping or unnecessary runs.
#[derive(Default)]
pub struct ScanTracker {
    active: HashSet<SourceId>,
    completed: HashSet<SourceId>,
}

impl ScanTracker {
    /// Determine whether a scan should start. Returns false if one is already
    /// running or if a completed scan exists and `force` is false.
    pub fn can_start(&self, id: &SourceId, force: bool) -> bool {
        if self.active.contains(id) {
            return false;
        }
        force || !self.completed.contains(id)
    }

    /// Mark a scan as running for the given source.
    pub fn mark_started(&mut self, id: &SourceId) {
        self.active.insert(id.clone());
    }

    /// Mark a scan as successfully completed and no longer active.
    pub fn mark_completed(&mut self, id: &SourceId) {
        self.active.remove(id);
        self.completed.insert(id.clone());
    }

    /// Mark a scan as finished with an error; allows retrying.
    pub fn mark_failed(&mut self, id: &SourceId) {
        self.active.remove(id);
        self.completed.remove(id);
    }

    /// Clear all tracking state, e.g., when removing a source.
    pub fn forget(&mut self, id: &SourceId) {
        self.active.remove(id);
        self.completed.remove(id);
    }

    /// True if a scan is currently running.
    pub fn is_active(&self, id: &SourceId) -> bool {
        self.active.contains(id)
    }

    /// True if any scan is currently running.
    pub fn has_active(&self) -> bool {
        !self.active.is_empty()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn id() -> SourceId {
        SourceId::new()
    }

    #[test]
    fn prevents_overlapping_scans() {
        let mut tracker = ScanTracker::default();
        let source = id();
        assert!(tracker.can_start(&source, false));
        tracker.mark_started(&source);
        assert!(!tracker.can_start(&source, false));
        tracker.mark_completed(&source);
        assert!(!tracker.is_active(&source));
    }

    #[test]
    fn allows_retry_after_failure() {
        let mut tracker = ScanTracker::default();
        let source = id();
        tracker.mark_started(&source);
        tracker.mark_failed(&source);
        assert!(tracker.can_start(&source, false));
    }

    #[test]
    fn skips_completed_when_not_forced() {
        let mut tracker = ScanTracker::default();
        let source = id();
        tracker.mark_started(&source);
        tracker.mark_completed(&source);
        assert!(!tracker.can_start(&source, false));
        assert!(tracker.can_start(&source, true));
    }
}
