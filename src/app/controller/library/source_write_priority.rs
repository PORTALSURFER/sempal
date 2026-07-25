//! Source-local write-priority windows for browser-owned file operations.

use crate::sample_sources::SourceId;
use std::collections::{HashMap, HashSet, VecDeque};
use std::path::{Path, PathBuf};
use std::sync::{Mutex, OnceLock};

static ACTIVE_FILE_OPS: OnceLock<Mutex<HashMap<SourceId, usize>>> = OnceLock::new();
static COMPLETED_RENAME_REMAPS: OnceLock<Mutex<CompletedRenameRemaps>> = OnceLock::new();
const MAX_COMPLETED_RENAME_REMAPS: usize = 512;

#[derive(Debug, Default)]
struct CompletedRenameRemaps {
    entries: HashMap<(SourceId, PathBuf), PathBuf>,
    order: VecDeque<(SourceId, PathBuf)>,
}

fn active_file_ops() -> &'static Mutex<HashMap<SourceId, usize>> {
    ACTIVE_FILE_OPS.get_or_init(|| Mutex::new(HashMap::new()))
}

fn completed_rename_remaps() -> &'static Mutex<CompletedRenameRemaps> {
    COMPLETED_RENAME_REMAPS.get_or_init(|| Mutex::new(CompletedRenameRemaps::default()))
}

/// Mark `source_id` as owning a short file-op write-priority window.
pub(crate) fn begin_file_op_write_priority(source_id: &SourceId) {
    let mut active = active_file_ops()
        .lock()
        .expect("source file-op write-priority mutex poisoned");
    *active.entry(source_id.clone()).or_insert(0) += 1;
}

/// Clear one active file-op write-priority window for `source_id`.
pub(crate) fn finish_file_op_write_priority(source_id: &SourceId) {
    let mut active = active_file_ops()
        .lock()
        .expect("source file-op write-priority mutex poisoned");
    let Some(count) = active.get_mut(source_id) else {
        return;
    };
    *count = count.saturating_sub(1);
    if *count == 0 {
        active.remove(source_id);
    }
}

/// Return true when same-source maintenance writes should defer.
pub(crate) fn file_op_write_priority_active(source_id: &SourceId) -> bool {
    active_file_ops()
        .lock()
        .expect("source file-op write-priority mutex poisoned")
        .contains_key(source_id)
}

/// Return the source ids currently owning file-op write priority.
pub(crate) fn active_file_op_write_priority_sources() -> HashSet<SourceId> {
    active_file_ops()
        .lock()
        .expect("source file-op write-priority mutex poisoned")
        .keys()
        .cloned()
        .collect()
}

/// Remember one successful browser rename so stale queued metadata can follow
/// the path-derived sample identity remap instead of reading the old path.
pub(crate) fn record_completed_browser_rename(
    source_id: &SourceId,
    old_relative: &Path,
    new_relative: &Path,
) {
    if old_relative == new_relative {
        return;
    }
    let key = (source_id.clone(), old_relative.to_path_buf());
    let mut remaps = completed_rename_remaps()
        .lock()
        .expect("completed browser rename remap mutex poisoned");
    if !remaps.entries.contains_key(&key) {
        remaps.order.push_back(key.clone());
    }
    remaps.entries.insert(key, new_relative.to_path_buf());
    while remaps.order.len() > MAX_COMPLETED_RENAME_REMAPS {
        if let Some(expired) = remaps.order.pop_front() {
            remaps.entries.remove(&expired);
        }
    }
}

/// Return the new path for a recently completed browser rename.
pub(crate) fn completed_browser_rename_target(
    source_id: &SourceId,
    old_relative: &Path,
) -> Option<PathBuf> {
    completed_rename_remaps()
        .lock()
        .expect("completed browser rename remap mutex poisoned")
        .entries
        .get(&(source_id.clone(), old_relative.to_path_buf()))
        .cloned()
}

#[cfg(test)]
pub(crate) struct FileOpWritePriorityGuard {
    source_id: SourceId,
    _state_scope: SourceWritePriorityTestScope,
}

#[cfg(test)]
impl FileOpWritePriorityGuard {
    pub(crate) fn new(source_id: &SourceId) -> Self {
        let state_scope = SourceWritePriorityTestScope::new(source_id);
        begin_file_op_write_priority(source_id);
        Self {
            source_id: source_id.clone(),
            _state_scope: state_scope,
        }
    }
}

#[cfg(test)]
impl Drop for FileOpWritePriorityGuard {
    fn drop(&mut self) {
        finish_file_op_write_priority(&self.source_id);
    }
}

#[cfg(test)]
#[derive(Default)]
struct SourceWritePriorityState {
    active_count: Option<usize>,
    remaps: Vec<(PathBuf, PathBuf)>,
}

#[cfg(test)]
struct SourceWritePriorityTestScope {
    source_id: SourceId,
    previous: SourceWritePriorityState,
    _owner: SourceWritePriorityTestOwner,
}

#[cfg(test)]
struct SourceWritePriorityTestOwner {
    source_id: SourceId,
}

#[cfg(test)]
type SourceWritePriorityTestOwners = (Mutex<HashSet<SourceId>>, std::sync::Condvar);

#[cfg(test)]
fn source_write_priority_test_owners() -> &'static SourceWritePriorityTestOwners {
    static OWNERS: OnceLock<SourceWritePriorityTestOwners> = OnceLock::new();
    OWNERS.get_or_init(|| (Mutex::new(HashSet::new()), std::sync::Condvar::new()))
}

#[cfg(test)]
impl SourceWritePriorityTestOwner {
    fn new(source_id: &SourceId) -> Self {
        let (owners, released) = source_write_priority_test_owners();
        let mut owners = owners
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        while owners.contains(source_id) {
            owners = released
                .wait(owners)
                .unwrap_or_else(std::sync::PoisonError::into_inner);
        }
        owners.insert(source_id.clone());
        Self {
            source_id: source_id.clone(),
        }
    }
}

#[cfg(test)]
impl Drop for SourceWritePriorityTestOwner {
    fn drop(&mut self) {
        let (owners, released) = source_write_priority_test_owners();
        let mut owners = owners
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let owned = owners.remove(&self.source_id);
        drop(owners);
        debug_assert!(owned, "source write-priority scope must own its source");
        released.notify_all();
    }
}

#[cfg(test)]
impl SourceWritePriorityTestScope {
    fn new(source_id: &SourceId) -> Self {
        let owner = SourceWritePriorityTestOwner::new(source_id);
        let previous = take_source_state(source_id);
        Self {
            source_id: source_id.clone(),
            previous,
            _owner: owner,
        }
    }
}

#[cfg(test)]
impl Drop for SourceWritePriorityTestScope {
    fn drop(&mut self) {
        let _discarded = take_source_state(&self.source_id);
        restore_source_state(&self.source_id, std::mem::take(&mut self.previous));
    }
}

#[cfg(test)]
fn take_source_state(source_id: &SourceId) -> SourceWritePriorityState {
    let active_count = active_file_ops()
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
        .remove(source_id);
    let mut remaps = completed_rename_remaps()
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    let keys = remaps
        .entries
        .keys()
        .filter(|(candidate, _)| candidate == source_id)
        .cloned()
        .collect::<Vec<_>>();
    let entries = keys
        .iter()
        .filter_map(|key| {
            remaps
                .entries
                .remove(key)
                .map(|target| (key.1.clone(), target))
        })
        .collect();
    remaps.order.retain(|(candidate, _)| candidate != source_id);
    SourceWritePriorityState {
        active_count,
        remaps: entries,
    }
}

#[cfg(test)]
fn restore_source_state(source_id: &SourceId, state: SourceWritePriorityState) {
    if let Some(active_count) = state.active_count {
        active_file_ops()
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .insert(source_id.clone(), active_count);
    }
    for (old_relative, new_relative) in state.remaps {
        record_completed_browser_rename(source_id, &old_relative, &new_relative);
    }
}

#[cfg(test)]
pub(crate) struct CompletedBrowserRenameTestGuard {
    _state_scope: SourceWritePriorityTestScope,
}

#[cfg(test)]
impl CompletedBrowserRenameTestGuard {
    pub(crate) fn new(source_id: &SourceId, old_relative: &Path, new_relative: &Path) -> Self {
        let state_scope = SourceWritePriorityTestScope::new(source_id);
        record_completed_browser_rename(source_id, old_relative, new_relative);
        Self {
            _state_scope: state_scope,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::mpsc;
    use std::time::Duration;

    #[test]
    fn file_op_guard_is_visible_to_worker_clients_and_cleans_up_after_panic() {
        let source_id = SourceId::from_string("file-op-guard-panic");
        let worker_source_id = source_id.clone();
        let (active_tx, active_rx) = mpsc::channel();
        let worker = std::thread::spawn(move || {
            let _guard = FileOpWritePriorityGuard::new(&worker_source_id);
            active_tx.send(()).unwrap();
            std::thread::sleep(std::time::Duration::from_millis(25));
            panic!("exercise file-op guard cleanup");
        });

        active_rx.recv().unwrap();
        assert!(file_op_write_priority_active(&source_id));
        assert!(worker.join().is_err());
        assert!(!file_op_write_priority_active(&source_id));
    }

    #[test]
    fn completed_rename_scope_cleans_up_after_panic() {
        let source_id = SourceId::from_string("completed-rename-restore");
        let old = Path::new("old.wav");
        let unwind = std::panic::catch_unwind(|| {
            let _guard =
                CompletedBrowserRenameTestGuard::new(&source_id, old, Path::new("scoped.wav"));
            assert_eq!(
                completed_browser_rename_target(&source_id, old),
                Some(PathBuf::from("scoped.wav"))
            );
            panic!("exercise completed-rename scope cleanup");
        });

        assert!(unwind.is_err());
        assert_eq!(completed_browser_rename_target(&source_id, old), None);
    }

    #[test]
    fn parallel_isolation_sentinel_scoped_global_control_lifecycle_under_contention() {
        let source_id = SourceId::from_string("same-source-scope-ownership");
        if std::env::var_os("WAVECRATE_ISOLATION_INJECT_GLOBAL_HOOK_LEAK").is_some() {
            let leaked = FileOpWritePriorityGuard::new(&source_id);
            std::mem::forget(leaked);
            assert!(
                !file_op_write_priority_active(&source_id),
                "WAVECRATE_ISOLATION:mutable_global_control_leak file-op priority guard leaked"
            );
        }

        let first_scope = FileOpWritePriorityGuard::new(&source_id);
        let worker_source_id = source_id.clone();
        let (started_tx, started_rx) = mpsc::channel();
        let (acquired_tx, acquired_rx) = mpsc::channel();
        let worker = std::thread::spawn(move || {
            started_tx.send(()).unwrap();
            let _scope = FileOpWritePriorityGuard::new(&worker_source_id);
            acquired_tx.send(()).unwrap();
        });

        started_rx.recv().unwrap();
        assert!(file_op_write_priority_active(&source_id));
        assert!(matches!(
            acquired_rx.recv_timeout(Duration::from_millis(50)),
            Err(mpsc::RecvTimeoutError::Timeout)
        ));

        drop(first_scope);
        acquired_rx.recv_timeout(Duration::from_secs(1)).unwrap();
        worker.join().unwrap();
        assert!(!file_op_write_priority_active(&source_id));
    }

    #[test]
    fn distinct_source_test_scopes_acquire_concurrently() {
        let first_source_id = SourceId::from_string("distinct-source-scope-a");
        let second_source_id = SourceId::from_string("distinct-source-scope-b");
        let first_scope = FileOpWritePriorityGuard::new(&first_source_id);
        let worker_source_id = second_source_id.clone();
        let (acquired_tx, acquired_rx) = mpsc::channel();
        let worker = std::thread::spawn(move || {
            let _scope = FileOpWritePriorityGuard::new(&worker_source_id);
            acquired_tx.send(()).unwrap();
        });

        acquired_rx.recv_timeout(Duration::from_secs(1)).unwrap();
        assert!(file_op_write_priority_active(&first_source_id));
        worker.join().unwrap();
        drop(first_scope);
        assert!(!file_op_write_priority_active(&first_source_id));
        assert!(!file_op_write_priority_active(&second_source_id));
    }
}
