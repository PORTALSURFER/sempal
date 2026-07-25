//! Scoped isolation for tests that mutate process-global runtime state.

use std::{
    ffi::{OsStr, OsString},
    marker::PhantomData,
    path::{Path, PathBuf},
    rc::Rc,
    sync::{Arc, Condvar, Mutex, MutexGuard, OnceLock},
    thread::{self, ThreadId},
};

static PROCESS_STATE_LOCK: OnceLock<Arc<ReentrantProcessLock>> = OnceLock::new();

#[derive(Default)]
struct LockState {
    owner: Option<ThreadId>,
    depth: usize,
    failure: Option<String>,
}

#[derive(Default)]
struct ReentrantProcessLock {
    state: Mutex<LockState>,
    available: Condvar,
}

impl ReentrantProcessLock {
    fn acquire(&self) {
        let current = thread::current().id();
        let mut state = lock_unpoisoned(&self.state);
        loop {
            if let Some(failure) = state.failure.clone() {
                drop(state);
                panic!("process test runtime is unavailable: {failure}");
            }

            match state.owner.as_ref() {
                None => {
                    state.owner = Some(current);
                    state.depth = 1;
                    return;
                }
                Some(owner) if *owner == current => {
                    state.depth += 1;
                    return;
                }
                Some(_) => {
                    state = self
                        .available
                        .wait(state)
                        .unwrap_or_else(|poisoned| poisoned.into_inner());
                }
            }
        }
    }

    fn release(&self) {
        let current = thread::current().id();
        let mut state = lock_unpoisoned(&self.state);
        if state.failure.is_some() {
            return;
        }
        assert_eq!(
            state.owner.as_ref(),
            Some(&current),
            "process test runtime guard dropped by a non-owner thread"
        );
        state.depth -= 1;
        if state.depth == 0 {
            state.owner = None;
            self.available.notify_one();
        }
    }

    fn complete_scope(&self, current_dir_restore_error: Option<(PathBuf, std::io::Error)>) {
        let Some((path, error)) = current_dir_restore_error else {
            self.release();
            return;
        };

        let failure = format!(
            "failed to restore process working directory to {}: {error}",
            path.display()
        );
        {
            let mut state = lock_unpoisoned(&self.state);
            if state.failure.is_none() {
                state.failure = Some(failure.clone());
            }
            state.owner = None;
            state.depth = 0;
            self.available.notify_all();
        }

        if thread::panicking() {
            eprintln!("{failure} while unwinding; future process test runtime access is disabled");
        } else {
            panic!("{failure}; future process test runtime access is disabled");
        }
    }
}

fn lock_unpoisoned<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
}

enum Restoration {
    Environment {
        key: OsString,
        previous: Option<OsString>,
    },
    CurrentDirectory(PathBuf),
}

/// Serializes and restores process-global environment and working-directory changes.
///
/// The lock is shared by every Wavecrate test owner in the current test binary,
/// is re-entrant on one thread, and recovers from mutex poisoning. Each guard
/// restores its own mutations in reverse order before allowing another thread
/// to enter the scope.
pub struct TestRuntimeGuard {
    lock: Arc<ReentrantProcessLock>,
    restorations: Vec<Restoration>,
    _not_send_or_sync: PhantomData<Rc<()>>,
}

impl TestRuntimeGuard {
    /// Acquire exclusive ownership of process-global test runtime state.
    pub fn acquire() -> Self {
        Self::acquire_with(Arc::clone(process_state_lock()))
    }

    fn acquire_with(lock: Arc<ReentrantProcessLock>) -> Self {
        lock.acquire();
        Self {
            lock,
            restorations: Vec::new(),
            _not_send_or_sync: PhantomData,
        }
    }

    /// Set an environment variable for this scope and remember its exact prior value.
    pub fn set_var(&mut self, key: impl AsRef<OsStr>, value: impl AsRef<OsStr>) {
        let key = key.as_ref().to_os_string();
        let previous = std::env::var_os(&key);
        // SAFETY: every Wavecrate test environment mutation is serialized by
        // the shared process-state lock held for this guard's lifetime.
        unsafe {
            std::env::set_var(&key, value);
        }
        self.restorations
            .push(Restoration::Environment { key, previous });
    }

    /// Remove an environment variable for this scope and remember its exact prior value.
    pub fn remove_var(&mut self, key: impl AsRef<OsStr>) {
        let key = key.as_ref().to_os_string();
        let previous = std::env::var_os(&key);
        // SAFETY: every Wavecrate test environment mutation is serialized by
        // the shared process-state lock held for this guard's lifetime.
        unsafe {
            std::env::remove_var(&key);
        }
        self.restorations
            .push(Restoration::Environment { key, previous });
    }

    /// Change the process working directory for this scope.
    ///
    /// The exact prior directory is restored when the guard is dropped.
    pub fn set_current_dir(&mut self, path: impl AsRef<Path>) -> std::io::Result<()> {
        let previous = std::env::current_dir()?;
        std::env::set_current_dir(path)?;
        self.restorations
            .push(Restoration::CurrentDirectory(previous));
        Ok(())
    }
}

impl Drop for TestRuntimeGuard {
    fn drop(&mut self) {
        let mut current_dir_restore_error = None;
        for restoration in self.restorations.drain(..).rev() {
            match restoration {
                Restoration::Environment { key, previous } => {
                    // SAFETY: the shared process-state lock remains held until
                    // every scoped mutation has been restored.
                    unsafe {
                        match previous {
                            Some(value) => std::env::set_var(key, value),
                            None => std::env::remove_var(key),
                        }
                    }
                }
                Restoration::CurrentDirectory(previous) => {
                    if let Err(error) = std::env::set_current_dir(&previous) {
                        current_dir_restore_error = Some((previous, error));
                    }
                }
            }
        }
        self.lock.complete_scope(current_dir_restore_error);
    }
}

fn process_state_lock() -> &'static Arc<ReentrantProcessLock> {
    PROCESS_STATE_LOCK.get_or_init(|| Arc::new(ReentrantProcessLock::default()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{
        panic::{AssertUnwindSafe, catch_unwind},
        sync::{
            atomic::{AtomicBool, Ordering},
            mpsc,
        },
        time::Duration,
    };
    use tempfile::tempdir;

    const TEST_ENV: &str = "WAVECRATE_TEST_RUNTIME_GUARD_REGRESSION";

    #[test]
    fn nested_scopes_restore_unset_and_empty_values_exactly() {
        let mut outer = TestRuntimeGuard::acquire();
        outer.remove_var(TEST_ENV);
        assert_eq!(std::env::var_os(TEST_ENV), None);

        {
            let mut nested = TestRuntimeGuard::acquire();
            nested.set_var(TEST_ENV, "");
            assert_eq!(std::env::var_os(TEST_ENV), Some(OsString::new()));
        }

        assert_eq!(std::env::var_os(TEST_ENV), None);
    }

    #[test]
    fn panic_unwind_restores_environment_and_current_directory() {
        let original_dir = std::env::current_dir().expect("current directory");
        let temp = tempdir().expect("temporary working directory");
        let mut outer = TestRuntimeGuard::acquire();
        outer.set_var(TEST_ENV, "before-panic");

        let result = catch_unwind(AssertUnwindSafe(|| {
            let mut nested = TestRuntimeGuard::acquire();
            nested.set_var(TEST_ENV, "during-panic");
            nested
                .set_current_dir(temp.path())
                .expect("set temporary working directory");
            panic!("exercise scoped cleanup");
        }));

        assert!(result.is_err());
        assert_eq!(
            std::env::var_os(TEST_ENV),
            Some(OsString::from("before-panic"))
        );
        assert_eq!(
            std::env::current_dir().expect("restored current directory"),
            original_dir
        );
    }

    #[test]
    fn concurrent_contender_waits_for_owner_cleanup() {
        let mut owner = TestRuntimeGuard::acquire();
        owner.set_var(TEST_ENV, "owner");
        let attempting = Arc::new(AtomicBool::new(false));
        let contender_attempting = Arc::clone(&attempting);
        let (acquired_tx, acquired_rx) = mpsc::channel();

        let contender = thread::spawn(move || {
            contender_attempting.store(true, Ordering::Release);
            let _guard = TestRuntimeGuard::acquire();
            acquired_tx.send(()).expect("report guard acquisition");
        });

        while !attempting.load(Ordering::Acquire) {
            thread::yield_now();
        }
        assert!(
            acquired_rx.recv_timeout(Duration::from_millis(25)).is_err(),
            "contender acquired process state before owner cleanup"
        );

        drop(owner);
        acquired_rx
            .recv_timeout(Duration::from_secs(1))
            .expect("contender acquires after owner cleanup");
        contender.join().expect("join contender");
    }

    #[test]
    fn internal_lock_recovers_after_poisoning() {
        let lock = ReentrantProcessLock::default();
        let _ = catch_unwind(AssertUnwindSafe(|| {
            let _state = lock.state.lock().expect("lock state before poisoning");
            panic!("poison test lock");
        }));

        lock.acquire();
        lock.release();
    }

    #[test]
    fn failed_current_directory_restore_disables_future_access_after_caught_panic() {
        let original_dir = std::env::current_dir().expect("current directory");
        let saved_dir = tempdir().expect("saved temporary working directory");
        let mutated_dir = tempdir().expect("mutated temporary working directory");
        let mut outer = TestRuntimeGuard::acquire();
        outer
            .set_current_dir(saved_dir.path())
            .expect("enter saved working directory");

        let isolated_lock = Arc::new(ReentrantProcessLock::default());
        let result = catch_unwind(AssertUnwindSafe(|| {
            let mut nested = TestRuntimeGuard::acquire_with(Arc::clone(&isolated_lock));
            nested
                .set_current_dir(mutated_dir.path())
                .expect("enter mutated working directory");
            saved_dir
                .close()
                .expect("remove the saved working directory");
            panic!("exercise failed restoration while unwinding");
        }));

        assert!(result.is_err());
        let reacquire = catch_unwind(AssertUnwindSafe(|| {
            let _guard = TestRuntimeGuard::acquire_with(Arc::clone(&isolated_lock));
        }));
        assert!(
            reacquire.is_err(),
            "a failed CWD restoration must disable future guarded access"
        );

        drop(outer);
        assert_eq!(
            std::env::current_dir().expect("outer guard restores current directory"),
            original_dir
        );
    }
}
