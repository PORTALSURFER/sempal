//! Structured tracing helpers for source-database open and schema work.
//!
//! The source DB can be opened from many call sites, so these helpers keep the
//! emitted fields consistent enough to compare contention reports across the app.

use std::path::Path;
use std::time::Duration;

use crate::diagnostics::{DbDebugEvent, emit_db_debug_event};

use super::SourceDbError;

#[cfg(debug_assertions)]
#[derive(Default)]
struct OpenTotalTestState {
    counts: std::collections::HashMap<std::path::PathBuf, usize>,
    active_scopes: std::collections::HashSet<std::path::PathBuf>,
}

#[cfg(debug_assertions)]
type OpenTotalTestControls = (std::sync::Mutex<OpenTotalTestState>, std::sync::Condvar);

#[cfg(debug_assertions)]
static SOURCE_DB_OPEN_TOTAL_TEST_CONTROLS: std::sync::OnceLock<OpenTotalTestControls> =
    std::sync::OnceLock::new();

const SLOW_SOURCE_DB_OPEN_STEP: Duration = Duration::from_millis(15);
const SLOW_SOURCE_DB_OPEN_TOTAL: Duration = Duration::from_millis(40);
const SLOW_SOURCE_DB_OPEN_TOTAL_JOB_WORKER: Duration = Duration::from_millis(150);

fn slow_success_outcome(elapsed: Duration, threshold: Duration) -> Option<&'static str> {
    (elapsed >= threshold).then_some("slow")
}

fn open_phase_success_threshold(mode: &str, _phase: &str, read_only: bool) -> Option<Duration> {
    if !read_only && mode == "job_worker" {
        return None;
    }
    Some(SLOW_SOURCE_DB_OPEN_STEP)
}

fn open_total_success_threshold(mode: &str, read_only: bool) -> Duration {
    if !read_only && mode == "job_worker" {
        return SLOW_SOURCE_DB_OPEN_TOTAL_JOB_WORKER;
    }
    SLOW_SOURCE_DB_OPEN_TOTAL
}

/// Emit a structured event for one source-db open phase when it is slow or fails.
pub(super) fn record_open_phase(
    source_root: &Path,
    db_path: &Path,
    mode: &'static str,
    phase: &'static str,
    read_only: bool,
    elapsed: Duration,
    result: Result<(), &SourceDbError>,
) {
    let elapsed_ms = elapsed.as_millis() as u64;
    let source = source_root.display().to_string();
    let operation = format!("source_db.open.{phase}");
    let success_threshold = open_phase_success_threshold(mode, phase, read_only);
    match result {
        Ok(()) => {
            let Some(threshold) = success_threshold else {
                return;
            };
            let Some(outcome) = slow_success_outcome(elapsed, threshold) else {
                return;
            };
            emit_db_debug_event(DbDebugEvent {
                operation: &operation,
                source: Some(&source),
                outcome,
                elapsed,
                error: None,
            });
            tracing::info!(
                target: "perf::source_db",
                action = "open_phase",
                phase,
                mode,
                read_only,
                elapsed_ms,
                source_root = %source_root.display(),
                db_path = %db_path.display(),
                "Source DB open phase was slow"
            );
        }
        Err(err) => {
            let error = err.to_string();
            emit_db_debug_event(DbDebugEvent {
                operation: &operation,
                source: Some(&source),
                outcome: "error",
                elapsed,
                error: Some(&error),
            });
            tracing::warn!(
                target: "perf::source_db",
                action = "open_phase",
                phase,
                mode,
                read_only,
                elapsed_ms,
                busy = matches!(err, SourceDbError::Busy),
                error = %err,
                source_root = %source_root.display(),
                db_path = %db_path.display(),
                "Source DB open phase failed"
            );
        }
    }
}

/// Emit a structured event for the total source-db open path when useful.
pub(super) fn record_open_total(
    source_root: &Path,
    db_path: &Path,
    mode: &'static str,
    read_only: bool,
    elapsed: Duration,
    result: Result<(), &SourceDbError>,
) {
    #[cfg(debug_assertions)]
    record_test_open_total(source_root);

    let elapsed_ms = elapsed.as_millis() as u64;
    let source = source_root.display().to_string();
    let operation = "source_db.open_total";
    let success_threshold = open_total_success_threshold(mode, read_only);
    match result {
        Ok(()) => {
            let Some(outcome) = slow_success_outcome(elapsed, success_threshold) else {
                return;
            };
            emit_db_debug_event(DbDebugEvent {
                operation,
                source: Some(&source),
                outcome,
                elapsed,
                error: None,
            });
            tracing::info!(
                target: "perf::source_db",
                action = "open_total",
                mode,
                read_only,
                elapsed_ms,
                source_root = %source_root.display(),
                db_path = %db_path.display(),
                "Source DB open completed"
            );
        }
        Err(err) => {
            let error = err.to_string();
            emit_db_debug_event(DbDebugEvent {
                operation,
                source: Some(&source),
                outcome: "error",
                elapsed,
                error: Some(&error),
            });
            tracing::warn!(
                target: "perf::source_db",
                action = "open_total",
                mode,
                read_only,
                elapsed_ms,
                busy = matches!(err, SourceDbError::Busy),
                error = %err,
                source_root = %source_root.display(),
                db_path = %db_path.display(),
                "Source DB open failed"
            );
        }
    }
}

#[cfg(debug_assertions)]
fn test_open_total_controls() -> &'static OpenTotalTestControls {
    SOURCE_DB_OPEN_TOTAL_TEST_CONTROLS.get_or_init(|| {
        (
            std::sync::Mutex::new(OpenTotalTestState::default()),
            std::sync::Condvar::new(),
        )
    })
}

#[cfg(debug_assertions)]
fn record_test_open_total(source_root: &Path) {
    let mut state = test_open_total_controls()
        .0
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    *state.counts.entry(source_root.to_path_buf()).or_insert(0) += 1;
}

#[cfg(debug_assertions)]
pub(super) fn acquire_open_total_count_scope(source_root: &Path) {
    let (state, released) = test_open_total_controls();
    let mut state = state
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    while state.active_scopes.contains(source_root) {
        state = released
            .wait(state)
            .unwrap_or_else(std::sync::PoisonError::into_inner);
    }
    state.active_scopes.insert(source_root.to_path_buf());
    state.counts.remove(source_root);
}

#[cfg(debug_assertions)]
pub(super) fn release_open_total_count_scope(source_root: &Path) {
    let (state, released) = test_open_total_controls();
    let mut state = state
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    state.counts.remove(source_root);
    let owned = state.active_scopes.remove(source_root);
    drop(state);
    debug_assert!(owned, "source DB open-count scope must own its root");
    released.notify_all();
}

#[cfg(debug_assertions)]
pub(super) fn reset_open_total_count(source_root: &Path) {
    test_open_total_controls()
        .0
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .counts
        .remove(source_root);
}

#[cfg(debug_assertions)]
pub(super) fn open_total_count(source_root: &Path) -> usize {
    test_open_total_controls()
        .0
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .counts
        .get(source_root)
        .copied()
        .unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::{
        SLOW_SOURCE_DB_OPEN_STEP, SLOW_SOURCE_DB_OPEN_TOTAL, SLOW_SOURCE_DB_OPEN_TOTAL_JOB_WORKER,
        open_phase_success_threshold, open_total_count, open_total_success_threshold,
        record_test_open_total, slow_success_outcome,
    };
    use std::path::{Path, PathBuf};
    use std::time::Duration;

    #[test]
    fn fast_open_phase_success_is_suppressed() {
        assert_eq!(
            slow_success_outcome(
                SLOW_SOURCE_DB_OPEN_STEP.saturating_sub(Duration::from_millis(1)),
                SLOW_SOURCE_DB_OPEN_STEP,
            ),
            None
        );
    }

    #[test]
    fn slow_open_phase_success_is_kept() {
        assert_eq!(
            slow_success_outcome(SLOW_SOURCE_DB_OPEN_STEP, SLOW_SOURCE_DB_OPEN_STEP),
            Some("slow")
        );
    }

    #[test]
    fn fast_open_total_success_is_suppressed() {
        assert_eq!(
            slow_success_outcome(
                SLOW_SOURCE_DB_OPEN_TOTAL.saturating_sub(Duration::from_millis(1)),
                SLOW_SOURCE_DB_OPEN_TOTAL,
            ),
            None
        );
    }

    #[test]
    fn job_worker_open_phase_success_is_suppressed() {
        assert_eq!(
            open_phase_success_threshold("job_worker", "pragmas", false),
            None
        );
    }

    #[test]
    fn ui_read_open_phase_success_keeps_default_threshold() {
        assert_eq!(
            open_phase_success_threshold("ui_read", "pragmas", true),
            Some(SLOW_SOURCE_DB_OPEN_STEP)
        );
    }

    #[test]
    fn job_worker_open_total_success_uses_higher_threshold() {
        assert_eq!(
            open_total_success_threshold("job_worker", false),
            SLOW_SOURCE_DB_OPEN_TOTAL_JOB_WORKER
        );
    }

    #[test]
    fn open_total_count_scope_cleans_up_after_unwind() {
        let root = Path::new("source-db-open-count-scope");
        let unwind = std::panic::catch_unwind(|| {
            let _scope = super::super::open::test_scope_source_db_open_total_count(root);
            record_test_open_total(root);
            assert_eq!(open_total_count(root), 1);
            panic!("exercise source DB open-count cleanup");
        });

        assert!(unwind.is_err());
        assert_eq!(open_total_count(root), 0);
    }

    #[test]
    fn concurrent_same_root_open_total_count_scopes_are_isolated() {
        let root = PathBuf::from("source-db-open-count-concurrent-scope");
        let first_scope = super::super::open::test_scope_source_db_open_total_count(root.as_path());
        record_test_open_total(root.as_path());
        record_test_open_total(root.as_path());

        let (started_tx, started_rx) = std::sync::mpsc::channel();
        let (acquired_tx, acquired_rx) = std::sync::mpsc::channel();
        let worker_root = root.clone();
        let worker = std::thread::spawn(move || {
            started_tx.send(()).unwrap();
            let _scope = super::super::open::test_scope_source_db_open_total_count(&worker_root);
            assert_eq!(open_total_count(&worker_root), 0);
            record_test_open_total(&worker_root);
            assert_eq!(open_total_count(&worker_root), 1);
            acquired_tx.send(()).unwrap();
        });

        started_rx.recv().unwrap();
        assert_eq!(open_total_count(root.as_path()), 2);
        assert!(matches!(
            acquired_rx.recv_timeout(Duration::from_millis(50)),
            Err(std::sync::mpsc::RecvTimeoutError::Timeout)
        ));

        drop(first_scope);
        acquired_rx.recv_timeout(Duration::from_secs(1)).unwrap();
        worker.join().unwrap();
        assert_eq!(open_total_count(root.as_path()), 0);
    }
}
