use std::{
    ffi::OsString,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
        mpsc,
    },
    thread,
    time::Duration,
};

use tempfile::tempdir;
use wavecrate_library::test_runtime::TestRuntimeGuard;

const SENTINEL_ENV: &str = "WAVECRATE_PARALLEL_ISOLATION_SENTINEL";
const INJECT_PROCESS_LEAK_ENV: &str = "WAVECRATE_ISOLATION_INJECT_PROCESS_LEAK";

#[test]
fn parallel_isolation_sentinel_process_state_guard_under_contention() {
    if std::env::var_os(INJECT_PROCESS_LEAK_ENV).is_some() {
        inject_process_state_leak();
    }

    let original_dir = std::env::current_dir().expect("read original working directory");
    let owner_dir = tempdir().expect("create owner working directory");
    let contender_dir = tempdir().expect("create contender working directory");
    let mut owner = TestRuntimeGuard::acquire();
    owner.remove_var(SENTINEL_ENV);
    owner.set_var(SENTINEL_ENV, "owner");
    owner
        .set_current_dir(owner_dir.path())
        .expect("enter owner working directory");

    let attempting = Arc::new(AtomicBool::new(false));
    let contender_attempting = Arc::clone(&attempting);
    let contender_path = contender_dir.path().to_path_buf();
    let expected_dir = original_dir.clone();
    let (acquired_tx, acquired_rx) = mpsc::channel();
    let contender = thread::spawn(move || {
        contender_attempting.store(true, Ordering::Release);
        let mut scope = TestRuntimeGuard::acquire();
        assert_eq!(
            std::env::var_os(SENTINEL_ENV),
            None,
            "owner environment mutation must be restored before handoff"
        );
        assert_eq!(
            std::env::current_dir().expect("read restored working directory"),
            expected_dir,
            "owner working-directory mutation must be restored before handoff"
        );
        scope.set_var(SENTINEL_ENV, "contender");
        scope
            .set_current_dir(&contender_path)
            .expect("enter contender working directory");
        acquired_tx.send(()).expect("report contender acquisition");
    });

    while !attempting.load(Ordering::Acquire) {
        thread::yield_now();
    }
    assert!(
        acquired_rx.recv_timeout(Duration::from_millis(25)).is_err(),
        "contender acquired shared process state before owner cleanup"
    );
    drop(owner);
    acquired_rx
        .recv_timeout(Duration::from_secs(1))
        .expect("contender acquired shared process state after cleanup");
    contender.join().expect("join process-state contender");

    let _verification_scope = TestRuntimeGuard::acquire();
    assert_eq!(std::env::var_os(SENTINEL_ENV), None);
    assert_eq!(
        std::env::current_dir().expect("read final working directory"),
        original_dir
    );
}

fn inject_process_state_leak() -> ! {
    let leaked_dir = tempdir()
        .expect("create injected leaked working directory")
        .keep();
    let mut leaked = TestRuntimeGuard::acquire();
    leaked.set_var(SENTINEL_ENV, "leaked");
    leaked
        .set_current_dir(&leaked_dir)
        .expect("enter injected leaked working directory");
    std::mem::forget(leaked);

    let leaked_environment = std::env::var_os(SENTINEL_ENV) == Some(OsString::from("leaked"));
    let leaked_directory = std::env::current_dir()
        .expect("read injected working directory")
        .canonicalize()
        .expect("canonicalize injected working directory")
        == leaked_dir
            .canonicalize()
            .expect("canonicalize leaked working directory");
    panic!(
        "WAVECRATE_ISOLATION:process_state_contamination environment_leaked={leaked_environment} current_directory_leaked={leaked_directory}"
    );
}
